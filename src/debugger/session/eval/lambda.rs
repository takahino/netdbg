//! ラムダ式。対象の中に式木（`System.Linq.Expressions`）を組み立てて `Compile()` し、本物のデリゲートにする
//! （docs/DECISIONS.md D-029）。デリゲートを受け取るメソッド（LINQ、`List<T>.Find`、利用者のメソッド）にそのまま渡せる。
//!
//! 本体のうち、ラムダの引数を使わない部分（止まっている場所の変数、リテラル、静的メンバー）は、エンジンで評価した
//! 値を定数として式木に入れる。引数を使う部分だけを、`Expression.Add` などの呼び出しで組み立てる。
//! 演算子の両辺の型は C# の規則で揃える（`Expression` は、型の違う被演算子を受け付けない）。

use super::*;
use crate::debugger::metadata::TypeShape;
use crate::debugger::session::eval::syntax::{parse_reflection_type, parse_type, Assignment, Statement};

mod create;
mod expressions;
mod forms;
#[cfg(test)]
pub(in crate::debugger::session) mod gaps;
mod nested;
mod patterns;
mod statements;

use forms::construct_name;
use nested::CallTarget;

/// よく使うデリゲートの型から分かる、ラムダの引数の型と戻り値の型（表示の書き方）。戻り値の `None` は void。
/// ほかの型（利用者が定義したデリゲート）は `Session::delegate_signature_of` が、`Invoke` のシグネチャから読む。
fn delegate_signature(shape: &TypeShape) -> Option<(Vec<String>, Option<String>)> {
    let arguments = &shape.arguments;
    Some(match shape.name.as_str() {
        "System.Func" => {
            let (output, inputs) = arguments.split_last()?;
            (inputs.to_vec(), Some(output.clone()))
        }
        "System.Action" => (arguments.clone(), None),
        "System.Predicate" => (arguments.clone(), Some("bool".into())),
        "System.Converter" => (vec![arguments.first()?.clone()], Some(arguments.get(1)?.clone())),
        "System.Comparison" => (vec![arguments.first()?.clone(); 2], Some("int".into())),
        _ => return None,
    })
}

/// 型引数が名前の中に書かれた形（名前が `System.Func<int, int>` で、型引数の並びが空）を、名前と型引数に分ける。
/// はじめから分かれている形や、読めない名前は、そのまま。
pub(super) fn split_shape(shape: TypeShape) -> TypeShape {
    if !shape.arguments.is_empty() || !shape.name.contains('<') {
        return shape;
    }
    match parse_type(&shape.name) {
        Some(ty) if !ty.array && !ty.nullable => TypeShape { name: ty.name.clone(), arguments: ty.arguments.iter().map(type_syntax_text).collect() },
        _ => shape,
    }
}

/// 型の構文を、型の名前の文字列に戻す（`System.Collections.Generic.List<System.Int32>[]`）。
pub(super) fn type_syntax_text(ty: &TypeSyntax) -> String {
    let mut text = ty.name.clone();
    if !ty.arguments.is_empty() {
        text = format!("{text}<{}>", ty.arguments.iter().map(type_syntax_text).collect::<Vec<_>>().join(", "));
    }
    if ty.nullable {
        text.push('?');
    }
    if ty.array {
        text.push_str("[]");
    }
    text
}

/// 型引数がまだ決まっていないことを表す名前（`!!1` は、メソッドの 2 番目の型引数）。
pub(super) fn placeholder(name: &str) -> Option<usize> {
    name.strip_prefix("!!")?.parse().ok()
}

/// 型引数を入れ子に含む型の名前（`System.Collections.Generic.IEnumerable<!!1>`）の、ジェネリック型の名前、型引数の番号、
/// その位置、型引数の数。入れ子が 1 段で、型引数がそのまま並んでいる形だけ。
pub(super) fn nested_placeholder(text: &str) -> Option<(String, usize, usize, usize)> {
    let (generic, rest) = text.split_once('<')?;
    let arguments: Vec<&str> = rest.strip_suffix('>')?.split(',').map(str::trim).collect();
    let at = arguments.iter().position(|argument| placeholder(argument).is_some())?;
    Some((generic.trim().to_string(), placeholder(arguments[at])?, at, arguments.len()))
}

/// `async` ラムダが返すもの。本体は同期で実行し、結果を完了済みの Task に包む（docs/DECISIONS.md D-034）。
struct TaskShape {
    /// Task に包むか。値を返さないデリゲート（`Action`）に渡す async void のラムダは、包まない。
    wrap: bool,
    /// `ValueTask` / `ValueTask<T>` を返すデリゲートか。
    value_task: bool,
    /// `Task<T>` の T（表示の書き方）。値の無い `Task` と、デリゲートの型が決まっていないときは `None`。
    result: Option<String>,
    /// デリゲートの型が決まっていない（本体の型から `Task` か `Task<T>` を決める）。
    inferred: bool,
}

impl TaskShape {
    /// `output` はデリゲートの戻り値の型（表示の書き方）。
    fn of(output: Option<&str>, has_delegate: bool) -> Result<TaskShape, String> {
        let Some(output) = output else { return Ok(TaskShape { wrap: !has_delegate, value_task: false, result: None, inferred: !has_delegate }) };
        let ty = parse_type(output).ok_or_else(|| tr!("Cannot read the type name {name}.", name = output))?;
        let value_task = match ty.name.as_str() {
            "System.Threading.Tasks.Task" => false,
            "System.Threading.Tasks.ValueTask" => true,
            _ => return Err(tr!("An async lambda can be passed only to a delegate that returns Task or ValueTask (the target returns {type}).", type = output)),
        };
        Ok(TaskShape { wrap: true, value_task, result: ty.arguments.first().map(type_syntax_text), inferred: false })
    }
}

/// ラムダ式の return の飛び先（`LabelTarget`）と、戻す値の型（`System.Type` と完全な名前。値を返さなければ `None`）。
#[derive(Clone)]
pub(in crate::debugger::session) struct ReturnTarget {
    label: ComPtr<ICorDebugValue>,
    value_type: Option<(ComPtr<ICorDebugValue>, String)>,
}

/// 組み立てている途中のラムダ式の、return / break / continue の飛び先。
pub(in crate::debugger::session) struct LambdaFlow {
    /// return が戻す型（表示の書き方）。外側の `None` は「まだ分からない（最初の return の式の型にする）」、
    /// 内側の `None` は「値を返さない」。
    returns: Option<Option<String>>,
    /// return の飛び先。最初の return で作る。
    exit: Option<ReturnTarget>,
    /// いまいるループと switch の、break と continue の飛び先（内側が後ろ）。
    loops: Vec<(ComPtr<ICorDebugValue>, Option<ComPtr<ICorDebugValue>>)>,
    /// 式の途中で作った変数（パターンの変数 `x is int k` の `k` と、値を一度だけ実行するための置き場所）。式の途中では
    /// 宣言できないので、ラムダ式の本体の外側のブロックで、まとめて宣言する。
    locals: Vec<ComPtr<ICorDebugValue>>,
    /// そのうち、あとの式から名前で読めるもの（パターンの変数）。同じ名前は、あとから作ったものが見つかる。
    named: Vec<(String, ComPtr<ICorDebugValue>)>,
    /// 組み立てている `?.` の続きの、受け手の節（内側が後ろ）。
    receivers: Vec<ComPtr<ICorDebugValue>>,
    /// `checked(…)` の中か（整数の計算があふれたら、例外にする）。`unchecked(…)` の中では false に戻す。
    checked: bool,
}

impl LambdaFlow {
    fn new(returns: Option<Option<String>>) -> LambdaFlow {
        LambdaFlow { returns, exit: None, loops: Vec::new(), locals: Vec::new(), named: Vec::new(), receivers: Vec::new(), checked: false }
    }
}

/// 文の中（入れ子の文も）に return があるか。
fn returns(statement: &Statement) -> bool {
    let any = |statements: &[Statement]| statements.iter().any(returns);
    match statement {
        Statement::Return(_) => true,
        Statement::Block(inner) => any(inner),
        Statement::If(_, then, otherwise) => returns(then) || otherwise.as_deref().is_some_and(returns),
        Statement::While(_, body) | Statement::Do(body, _) | Statement::Lock(_, body) => returns(body),
        Statement::For { body, .. } | Statement::ForEach { body, .. } => returns(body),
        Statement::Switch(_, sections) => sections.iter().any(|section| any(&section.body)),
        Statement::Try { body, catches, finally } => any(body) || catches.iter().any(|handler| any(&handler.body)) || finally.as_deref().is_some_and(any),
        Statement::Using(_, body) => any(body),
        Statement::Local(..) | Statement::Expr(_) | Statement::Break | Statement::Continue | Statement::Rethrow => false,
    }
}

const EXPRESSION: &str = "System.Linq.Expressions.Expression";
const PARAMETER: &str = "System.Linq.Expressions.ParameterExpression";

/// 式がラムダの引数（やローカル変数）を使っているか。
fn mentions(expr: &Expr, names: &[String]) -> bool {
    expr.mentions(names)
}

/// リフレクションの型の名前（`Type.FullName`）が、配列の型か（`System.Int32[]`、`System.Int32[,]`）。型引数つきの型の
/// 名前（``List`1[[System.Int32, …]]``）も `]` で終わるので、中を見て区別する。
fn is_array_type(full: &str) -> bool {
    full.ends_with("[]") || full.ends_with(",]")
}

/// 型の名前（`System.Int32`）を、C# のキーワード（`int`）にする。キーワードの無い型は、そのまま。
fn keyword_of(full: &str) -> &str {
    crate::debugger::builtin::by_full_name(full).map_or(full, |ty| ty.keyword)
}

/// 呼び出しの相手が見つからない、の誤りか（ほかの手で呼べるか試してよい）。文はいまの言語で作るので、いまの言語の
/// 文の形で見分ける。
fn is_not_found(message: &str) -> bool {
    [
        tr_str!("Method '{name}' is not found."),
        tr_str!("No method '{name}' that takes these arguments was found."),
        tr_str!("'{name}' is not found."),
        tr_str!("No type named '{name}' was found."),
        tr_str!("No variable, member or type named '{name}' was found."),
        tr_str!("The constructor is not found."),
        tr_str!("There is no constructor that takes these arguments."),
    ]
    .iter()
    .any(|template| crate::i18n::matches(message, template))
}
impl Session {
    /// デリゲートの型の、引数の型と戻り値の型（表示の書き方）。戻り値の `None` は void。デリゲートの型でなければ `None`。
    /// 利用者が定義したデリゲート（`delegate int Transform(int value)`）は、`Invoke` のシグネチャから読む。
    pub(super) fn delegate_signature_of(&self, thread: u32, depth: usize, shape: &TypeShape) -> Option<(Vec<String>, Option<String>)> {
        if let Some(signature) = delegate_signature(shape) {
            return Some(signature);
        }
        let (module, row) = self.find_type_with_arity(thread, depth, &shape.name, shape.arguments.len())?;
        let metadata = self.modules[module].metadata()?;
        if metadata.base_type_name(row).as_deref() != Some("System.MulticastDelegate") {
            return None;
        }
        let invoke = metadata.methods_named(row, "Invoke").into_iter().next()?;
        let inputs = metadata.parameter_types(invoke.token, &shape.arguments, &[])?.into_iter().map(|parameter| parameter.map(|parameter| parameter.display())).collect::<Option<Vec<_>>>()?;
        let output = metadata.return_type(invoke.token, &shape.arguments).map(|output| output.display());
        Some((inputs, output))
    }

    /// `System.Linq.Expressions.Expression` の型。対象にまだ読み込まれていなければ、`Type.GetType` で読み込ませる。
    fn expression_class(&mut self, thread: u32, depth: usize) -> Result<(usize, u32), String> {
        self.system_core_type(thread, depth, "System.Linq.Expressions.Expression", "System.Linq.Expressions").ok_or_else(|| tr!("System.Linq.Expressions, needed for lambdas, cannot be loaded into the target."))
    }

    /// .NET では `assembly`、.NET Framework では System.Core にある型（`system_type`）。
    pub(super) fn system_core_type(&mut self, thread: u32, depth: usize, name: &str, assembly: &str) -> Option<(usize, u32)> {
        self.system_type(thread, depth, name, assembly, "System.Core, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089")
    }

    /// .NET では `core`、.NET Framework では `framework` のアセンブリにある型。対象にまだ読み込まれていなければ、
    /// `Type.GetType` で読み込ませる。読み込みは、評価の途中に知らされるモジュールの読み込みとして記録されるので、そのあとは
    /// 名前で探せる。
    pub(super) fn system_type(&mut self, thread: u32, depth: usize, name: &str, core: &str, framework: &str) -> Option<(usize, u32)> {
        if let Some(found) = self.find_type(thread, depth, name) {
            return Some(found);
        }
        let on_core = self.modules.iter().any(|module| module.path.ends_with("System.Private.CoreLib.dll"));
        let assembly = if on_core { core } else { framework };
        let (module, row) = self.find_type(thread, depth, "System.Type")?;
        let text = self.new_string(thread, &format!("{name}, {assembly}")).ok()?;
        self.call_method(thread, depth, Some(Current::Type { module, row }), "GetType", vec![text]).ok()?;
        // 読み込みで、名前から探した型の覚えは捨てられている（D-035）。
        self.find_type(thread, depth, name)
    }

    /// `Expression` の静的メソッドを呼ぶ。
    fn factory(&mut self, thread: u32, depth: usize, name: &str, arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        let (module, row) = self.expression_class(thread, depth)?;
        let result = self.call_method(thread, depth, Some(Current::Type { module, row }), name, arguments)?.ok_or_else(|| tr!("Expression.{name} returned no value.", name = name))?;
        // 組み立てに失敗すると、例外のオブジェクトが返ってくる（型が合わない、など）。
        if unsafe { value::type_names(&result, &self.modules) }.iter().any(|name| name == "System.Exception") {
            let message = self.member(thread, &result, "Message").ok().and_then(|message| unsafe { value::as_string(&message) }).unwrap_or_default();
            return Err(tr!("Cannot build the lambda (Expression.{name}): {message}", name = name, message = message));
        }
        Ok(result)
    }

    /// 型の名前（表示の書き方）から、対象の中の `System.Type` のオブジェクト。
    fn type_object(&mut self, thread: u32, depth: usize, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let key = (thread, depth, name.to_string());
        if let Some(known) = self.eval_cache.type_objects.get(&key) {
            return Ok(known.clone());
        }
        let ty = parse_type(name).ok_or_else(|| tr!("Cannot read the type name {name}.", name = name))?;
        let current = self.evaluate_node(thread, depth, &Expr::TypeOf(ty))?;
        let object = self.value_of(current)?;
        self.eval_cache.type_objects.insert(key, object.clone());
        Ok(object)
    }

    /// 式木の節の型の名前（`System.Int32` など）。
    fn node_type(&mut self, thread: u32, node: &ComPtr<ICorDebugValue>) -> Result<String, String> {
        // 同じ節の型を、演算子の両辺の型合わせなどで何度も聞く。聞くたびに関数評価が 2 回要るので、覚えておく。
        if let Some((_, known)) = self.eval_cache.node_types.iter().find(|(seen, _)| seen.as_raw() == node.as_raw()) {
            return Ok(known.clone());
        }
        let ty = self.member(thread, node, "Type")?;
        let name = self.member(thread, &ty, "FullName")?;
        let name = unsafe { value::as_string(&name) }.ok_or_else(|| tr!("Cannot read the expression tree's type name."))?;
        self.eval_cache.node_types.push((node.clone(), name.clone()));
        Ok(name)
    }

    /// ラムダ式をデリゲートにする。`inputs` は引数の型（表示の書き方）。`delegate` はデリゲートの型で、分かっていなければ
    /// `None`（そのときは、本体の型から `Func<…>` になる）。`exact` は `lambda_expression` と同じ。戻り値は、デリゲートと、
    /// 本体の型の名前（`System.Int32` など）。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compile_lambda(&mut self, thread: u32, depth: usize, parameters: &[(Option<TypeSyntax>, String)], body: &Expr, inputs: &[String], delegate: Option<&TypeShape>, exact: bool) -> Result<(ComPtr<ICorDebugValue>, String), String> {
        let (lambda, body_type) = self.lambda_expression(thread, depth, parameters, body, inputs, delegate, &[], exact)?;
        let compiled = self.call_method(thread, depth, Some(Current::Value(lambda)), "Compile", Vec::new())?.ok_or_else(|| tr!("Could not compile the lambda."))?;
        Ok((compiled, body_type))
    }

    /// ラムダ式の節（`Expression.Lambda`）を組み立てる。`outer` は、外側のラムダ式の引数と変数（内側のラムダ式から使える）。
    /// `exact` なら、本体の型がデリゲートの戻り値の型と違うときに、変換を挟まずに断る（オーバーロードを選ぶときに、
    /// 戻り値の型がそのまま合うものを先に選ぶため）。戻り値は、節と、本体の型の名前。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lambda_expression(&mut self, thread: u32, depth: usize, parameters: &[(Option<TypeSyntax>, String)], body: &Expr, inputs: &[String], delegate: Option<&TypeShape>, outer: &[(String, ComPtr<ICorDebugValue>)], exact: bool) -> Result<(ComPtr<ICorDebugValue>, String), String> {
        if parameters.len() != inputs.len() {
            return Err(tr!("The lambda's parameter count ({count}) differs from the target delegate's ({expected}).", count = parameters.len(), expected = inputs.len()));
        }
        let mut scope = outer.to_vec();
        let mut own = Vec::new();
        for ((written, name), input) in parameters.iter().zip(inputs) {
            let type_name = match written {
                Some(ty) => type_syntax_text(ty),
                None => input.clone(),
            };
            let type_object = self.type_object(thread, depth, &type_name)?;
            let text = self.text(thread, name)?;
            let parameter = self.factory(thread, depth, "Parameter", vec![type_object, text])?;
            scope.push((name.clone(), parameter.clone()));
            own.push(parameter);
        }
        let delegate_output = delegate.and_then(delegate_signature).and_then(|(_, output)| output);
        // `async` ラムダは、本体を同期で実行して、結果を完了済みの Task に包む（D-034）。本体の型は、Task<T> の T。
        let (body, task) = match body {
            Expr::Async(inner) => (&**inner, Some(TaskShape::of(delegate_output.as_deref(), delegate.is_some())?)),
            other => (other, None),
        };
        let output = match &task {
            Some(task) => task.result.clone(),
            None => delegate_output,
        };
        // 本体の中の return が戻す型。デリゲートの型から分かっていれば、それ（値を返さないデリゲートは `Some(None)`）。
        let returns = match (&task, delegate.is_some()) {
            (Some(task), _) if task.inferred => None,
            (Some(task), _) => Some(task.result.clone()),
            (None, true) => Some(output.clone()),
            (None, false) => None,
        };
        self.lambda_flows.push(LambdaFlow::new(returns));
        let built = match (body, &output) {
            // `x => throw e` は、デリゲートの戻り値の型の式（void ではない）。
            (Expr::Throw(operand), Some(output)) => self.type_object(thread, depth, output).and_then(|ty| self.throw_node(thread, depth, operand, &scope, Some(ty))),
            // `x => y => x + y`: 本体のラムダ式は、戻り値のデリゲートの型で作る。
            (Expr::Lambda(..), Some(output)) => self.lambda_for_type(thread, depth, output, body, &scope),
            // `(a, b) => new(a, b)`、`() => [1, 2]`、`() => default`: 戻り値の型で作る。
            (Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral, Some(output)) => self.targeted_node(thread, depth, body, output, &scope),
            _ => self.tree(thread, depth, body, &scope),
        };
        let locals = self.lambda_flows.pop().map(|flow| flow.locals).unwrap_or_default();
        let mut tree = built?;
        // 式の途中で作った変数を、本体の外側で宣言する。
        if !locals.is_empty() {
            tree = self.block_of(thread, depth, &locals, &[tree])?;
        }
        let mut body_type = self.node_type(thread, &tree)?;
        // 戻り値の型が決まっていて、本体の型と違えば、変換を挟む（`Func<int, long>` に `x => x` など）。
        if let Some(output) = output {
            let wanted = parse_type(&output).map(|ty| ty.name).unwrap_or(output.clone());
            if wanted != body_type && keyword_of(&body_type) != output {
                if exact && !reflection_name_is(&body_type, &output) {
                    return Err(tr!("The lambda body's type ({type}) differs from the return type ({expected}).", type = keyword_of(&body_type), expected = output));
                }
                let target = self.type_object(thread, depth, &output)?;
                tree = self.factory(thread, depth, "Convert", vec![tree, target])?;
                body_type = wanted;
            }
        }
        if let Some(task) = &task {
            tree = self.completed_task(thread, depth, tree, task)?;
            body_type = self.node_type(thread, &tree)?;
        }
        let array = self.node_array(thread, depth, "System.Linq.Expressions.ParameterExpression", &own)?;
        let lambda = match delegate {
            Some(shape) => {
                let delegate_type = self.type_object(thread, depth, &shape.display())?;
                self.factory(thread, depth, "Lambda", vec![delegate_type, tree, array])?
            }
            None => self.factory(thread, depth, "Lambda", vec![tree, array])?,
        };
        Ok((lambda, body_type))
    }

    /// `async` ラムダの本体の節を、完了済みの Task を返す節にする。値があれば `Task.FromResult(本体)`、無ければ
    /// 「本体を実行してから `Task.CompletedTask`」。`ValueTask` を返すデリゲートには、それをさらに ValueTask に包む。
    fn completed_task(&mut self, thread: u32, depth: usize, body: ComPtr<ICorDebugValue>, task: &TaskShape) -> Result<ComPtr<ICorDebugValue>, String> {
        const EXPRESSION: &str = "System.Linq.Expressions.Expression";
        if !task.wrap {
            return Ok(body);
        }
        let has_value = self.node_type(thread, &body)? != "System.Void" && (task.inferred || task.result.is_some());
        let task_type = self.type_object(thread, depth, "System.Threading.Tasks.Task")?;
        let text = |session: &mut Session, text: &str| session.text(thread, text);
        let element = self.member(thread, &body, "Type")?;
        let node = if has_value {
            let name = text(self, "FromResult")?;
            let types = self.node_array(thread, depth, "System.Type", std::slice::from_ref(&element))?;
            let arguments = self.node_array(thread, depth, EXPRESSION, &[body])?;
            self.factory(thread, depth, "Call", vec![task_type.clone(), name, types, arguments])?
        } else {
            let name = text(self, "CompletedTask")?;
            let no_instance = self.new_null(thread)?;
            let completed = self.factory(thread, depth, "Property", vec![no_instance, task_type.clone(), name])?;
            let variables = self.node_array(thread, depth, "System.Linq.Expressions.ParameterExpression", &[])?;
            let expressions = self.node_array(thread, depth, EXPRESSION, &[body, completed])?;
            self.factory(thread, depth, "Block", vec![variables, expressions])?
        };
        // 本体が投げた例外は、呼んだ場所には出さず、返す Task に入れる（C# の async メソッドと同じ。`IsFaulted` が true）。
        let exception_type = self.type_object(thread, depth, "System.Exception")?;
        let caught = self.variable(thread, depth, exception_type, "exception")?;
        let from_exception = text(self, "FromException")?;
        let type_arguments = if has_value { self.node_array(thread, depth, "System.Type", &[element])? } else { self.new_null(thread)? };
        let arguments = self.node_array(thread, depth, EXPRESSION, std::slice::from_ref(&caught))?;
        let faulted = self.factory(thread, depth, "Call", vec![task_type, from_exception, type_arguments, arguments])?;
        let handler = self.factory(thread, depth, "Catch", vec![caught, faulted])?;
        let handlers = self.node_array(thread, depth, "System.Linq.Expressions.CatchBlock", &[handler])?;
        let node = self.factory(thread, depth, "TryCatch", vec![node, handlers])?;
        if !task.value_task {
            return Ok(node);
        }
        // `new ValueTask(Task)` / `new ValueTask<T>(Task<T>)`。
        let wrapper = match (&task.result, has_value) {
            (Some(result), true) => format!("System.Threading.Tasks.ValueTask<{result}>"),
            _ => "System.Threading.Tasks.ValueTask".to_string(),
        };
        let wrapper = self.type_object(thread, depth, &wrapper)?;
        let inner_type = self.member(thread, &node, "Type")?;
        let parameter_types = self.node_array(thread, depth, "System.Type", &[inner_type])?;
        let constructor = self.call_method(thread, depth, Some(Current::Value(wrapper)), "GetConstructor", vec![parameter_types])?.ok_or_else(|| tr!("The ValueTask constructor is not found."))?;
        let arguments = self.node_array(thread, depth, EXPRESSION, &[node])?;
        self.factory(thread, depth, "New", vec![constructor, arguments])
    }

    /// 節の並びを、決まった型の配列（`ParameterExpression[]`、`Expression[]`）にする。
    fn node_array(&mut self, thread: u32, depth: usize, element: &str, nodes: &[ComPtr<ICorDebugValue>]) -> Result<ComPtr<ICorDebugValue>, String> {
        let element = parse_type(element).ok_or_else(|| tr!("Cannot read the type name."))?;
        let element = self.runtime_type(thread, depth, &element)?;
        let array = self.new_array(thread, &element, &[nodes.len() as u32]).map_err(|failure| failure.describe(self))?;
        for (index, node) in nodes.iter().enumerate() {
            let slot = unsafe { value::array_element(&array, &[index as u32]) }?.ok_or_else(|| tr!("Cannot create the list of expression tree nodes."))?;
            self.write(thread, &slot, node)?;
        }
        Ok(array)
    }

    /// いま評価して、定数の節にする（ラムダの引数を使わない部分）。
    fn constant_node(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<ComPtr<ICorDebugValue>, String> {
        let current = self.evaluate_node(thread, depth, expr)?;
        self.constant_of(thread, depth, current)
    }

    /// 評価した値を、定数の節にする。
    fn constant_of(&mut self, thread: u32, depth: usize, current: Current) -> Result<ComPtr<ICorDebugValue>, String> {
        let value = self.value_of(current)?;
        // 宣言の型が object の変数（`object boxedText = "t"`）は、中身の型ではなく object の定数にする（C# の型は object）。
        if self.declared_object(&value) {
            let object = self.type_object(thread, depth, "object")?;
            return self.factory(thread, depth, "Constant", vec![value, object]);
        }
        self.factory(thread, depth, "Constant", vec![value])
    }

    /// 節の型が `like` の型と違えば、`like` の型への変換を挟む（代入先の変数の型に合わせる）。
    fn converted_like(&mut self, thread: u32, depth: usize, node: ComPtr<ICorDebugValue>, like: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        if self.node_type(thread, &node)? == self.node_type(thread, like)? {
            return Ok(node);
        }
        let ty = self.member(thread, like, "Type")?;
        self.factory(thread, depth, "Convert", vec![node, ty])
    }

    /// `throw 式` の節。`ty` は、式としての型（条件演算子のもう一方の枝の型、ラムダの戻り値の型）。無ければ void。
    fn throw_node(&mut self, thread: u32, depth: usize, operand: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)], ty: Option<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        let exception = self.tree(thread, depth, operand, scope)?;
        match ty {
            Some(ty) => self.factory(thread, depth, "Throw", vec![exception, ty]),
            None => self.factory(thread, depth, "Throw", vec![exception]),
        }
    }

    /// 対象の中の文字列（`Expression` のメソッドに渡す名前）。
    fn text(&mut self, thread: u32, text: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        if let Some(known) = self.eval_cache.strings.get(text) {
            return Ok(known.clone());
        }
        let created = self.new_string(thread, text).map_err(|failure| failure.describe(self))?;
        self.eval_cache.strings.insert(text.to_string(), created.clone());
        Ok(created)
    }

    /// ラムダの中の代入（`y = 1`、`o.Count += x`、`a[i]++`）の左辺の節。ラムダの引数とローカル変数、メンバー、添字。
    ///
    /// 止まっている場所のローカル変数には代入できない（デリゲートが動くのは別の呼び出しの中で、止まっているフレームの
    /// 変数には届かない）。this のフィールドとプロパティ、静的メンバーには代入できる。
    fn assignable(&mut self, thread: u32, depth: usize, target: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let names = self.lambda_names(scope);
        match target {
            Expr::Name(name) => {
                if let Some(variable) = self.lambda_variable(scope, name) {
                    return Ok(variable);
                }
                let is_local = self.frame_variables(thread, depth)?.iter().any(|(existing, _)| existing == name);
                if is_local {
                    return Err(tr!("The variable '{name}' of the stopped location cannot be assigned from inside a lambda (the delegate runs in another call).", name = name));
                }
                let text = self.text(thread, name)?;
                // this のメンバー。無ければ、いまの型の静的メンバー。
                if let Ok(Some(this)) = self.resolve_name(thread, depth, "this") {
                    let instance = self.factory(thread, depth, "Constant", vec![this])?;
                    if let Ok(node) = self.factory(thread, depth, "PropertyOrField", vec![instance, text.clone()]) {
                        return Ok(node);
                    }
                }
                let (module, row) = self.frame_type(thread, depth).ok_or_else(|| tr!("'{name}' is not found.", name = name))?;
                let type_name = self.plain_type_name(module, row);
                self.static_member_node(thread, depth, &type_name, text).map_err(|_| tr!("The assignment target '{name}' is not found.", name = name))
            }
            // 型の静的メンバー（`Counter.Total = 1`）。
            Expr::Member(inner, name) if !mentions(inner, &names) && matches!(self.evaluate_node(thread, depth, inner), Ok(Current::Type { .. })) => {
                let Current::Type { module, row } = self.evaluate_node(thread, depth, inner)? else { unreachable!() };
                let type_name = self.plain_type_name(module, row);
                let text = self.text(thread, name)?;
                self.static_member_node(thread, depth, &type_name, text)
            }
            Expr::Member(inner, name) => {
                let inner = self.tree(thread, depth, inner, scope)?;
                let text = self.text(thread, name)?;
                self.factory(thread, depth, "PropertyOrField", vec![inner, text])
            }
            Expr::Index(inner, indices) => {
                let inner = self.tree(thread, depth, inner, scope)?;
                let mut index_nodes = Vec::new();
                for index in indices {
                    index_nodes.push(self.tree(thread, depth, index, scope)?);
                }
                let indices = self.node_array(thread, depth, EXPRESSION, &index_nodes)?;
                if is_array_type(&self.node_type(thread, &inner)?) {
                    return self.factory(thread, depth, "ArrayAccess", vec![inner, indices]);
                }
                let item = self.text(thread, "Item")?;
                self.factory(thread, depth, "Property", vec![inner, item, indices])
            }
            _ => Err(tr!("Write a variable, member or indexer on the left of an assignment inside a lambda.")),
        }
    }

    /// 型の静的なプロパティかフィールドの節。
    fn static_member_node(&mut self, thread: u32, depth: usize, type_name: &str, name: ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let ty = self.type_object(thread, depth, type_name)?;
        let no_instance = self.new_null(thread)?;
        match self.factory(thread, depth, "Property", vec![no_instance.clone(), ty.clone(), name.clone()]) {
            Ok(node) => Ok(node),
            Err(_) => self.factory(thread, depth, "Field", vec![no_instance, ty, name]),
        }
    }

    /// 代入（`=`、複合代入、`??=`）の節。値は代入先の型にそろえる。
    fn assign_node(&mut self, thread: u32, depth: usize, target: &Expr, kind: &Assignment, value: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let variable = self.assignable(thread, depth, target, scope)?;
        let value = match kind {
            // 代入先の型で作る値（`f = x => x + 1`、`p = new()`、`xs = [1, 2]`、`n = default`）。
            Assignment::Plain if matches!(value, Expr::Lambda(..) | Expr::Generic(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral) => {
                let full = self.node_type(thread, &variable)?;
                let shown = display_of_reflection(&full).ok_or_else(|| tr!("Cannot read the type name {name}.", name = full))?;
                match value {
                    // メソッドの集まり（`f = registry.Get<string>`）も、代入先のデリゲートの型で、ラムダ式に直して作る。
                    Expr::Lambda(..) | Expr::Generic(..) => self.lambda_for_type(thread, depth, &shown, value, scope)?,
                    other => self.targeted_node(thread, depth, other, &shown, scope)?,
                }
            }
            Assignment::Plain => match self.tree(thread, depth, value, scope) {
                Ok(node) => node,
                // 値にならない名前がメソッドを指していれば、代入先のデリゲートの型で作る（`h.Step = Doubled`）。
                Err(error) => {
                    if !self.is_method_group(thread, depth, value) {
                        return Err(error);
                    }
                    let full = self.node_type(thread, &variable)?;
                    let shown = display_of_reflection(&full).ok_or_else(|| tr!("Cannot read the type name {name}.", name = full))?;
                    self.lambda_for_type(thread, depth, &shown, value, scope)?
                }
            },
            // 複合代入は、演算の結果を変数の型に戻す（C# の `b += 1` は、byte の b に戻る）。左辺の値は、
            // 代入先の節から読む（デリゲートを呼んだときの値）。
            Assignment::Compound(op) => {
                let value = self.tree(thread, depth, value, scope)?;
                self.binary_of(thread, depth, *op, variable.clone(), value)?
            }
            Assignment::Coalesce => {
                let value = self.tree(thread, depth, value, scope)?;
                let value = self.converted_like(thread, depth, value, &variable)?;
                let coalesced = self.factory(thread, depth, "Coalesce", vec![variable.clone(), value])?;
                return self.factory(thread, depth, "Assign", vec![variable, coalesced]);
            }
        };
        let value = self.converted_like(thread, depth, value, &variable)?;
        self.factory(thread, depth, "Assign", vec![variable, value])
    }

    /// 単項演算子の節。int より小さい型は、int に広げてから（C# の単項演算子の規則）。
    fn unary_node(&mut self, thread: u32, depth: usize, op: UnaryOpKind, operand: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let node = self.tree(thread, depth, operand, scope)?;
        let operand_type = self.node_type(thread, &node)?;
        let checked = self.lambda_flows.last().is_some_and(|flow| flow.checked);
        let name = match op {
            UnaryOpKind::Not => "Not",
            UnaryOpKind::Plus => "UnaryPlus",
            UnaryOpKind::Negate if checked => "NegateChecked",
            UnaryOpKind::Negate => "Negate",
            UnaryOpKind::Complement => "OnesComplement",
        };
        // int より小さい型は、int に広げてから（C# の単項演算子の規則）。
        let node = match keyword_of(&operand_type) {
            "sbyte" | "byte" | "short" | "ushort" | "char" if op != UnaryOpKind::Not => self.convert_node(thread, depth, node, "int")?,
            _ => node,
        };
        self.factory(thread, depth, name, vec![node])
    }

    /// 本体の式を、式木の節にする。`scope` は、ラムダの引数とローカル変数の名前と、その ParameterExpression。
    fn tree(&mut self, thread: u32, depth: usize, expr: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let names = self.lambda_names(scope);
        // ラムダの引数を使わず、何も変えない部分は、いま評価して定数にする。呼び出しは、デリゲートを呼ぶたびに実行する
        // ように、呼び出しの節にする（`() => Run()` を作っただけで Run が動かないように）。
        // 変数を作るパターン（`boxed is int k && k > n`）は、定数にしない。合わなかったときに、変数を使う側（`k > n`）を
        // 組み立てられなくなる（実行時には `&&` の右へ進まないので、問題にならない）。
        // `?.` の続きの中の受け手（`w?.Length` の `w`）も、ラムダの中の値。
        let in_null_conditional = self.lambda_flows.last().is_some_and(|flow| !flow.receivers.is_empty());
        let mentioned = mentions(expr, &names) || (in_null_conditional && expr.any(&mut |inner| *inner == Expr::Receiver));
        if !mentioned && repeatable(expr) && !expr.declares() {
            return self.constant_node(thread, depth, expr);
        }
        match expr {
            Expr::Receiver if in_null_conditional => self.flow()?.receivers.last().cloned().ok_or_else(|| tr!("The position in the expression is not valid.")),
            // 値を作る書き方は、デリゲートを呼ぶたびに作る（`n => new StringBuilder("x").Append(n)` の StringBuilder は、
            // 呼ぶたびに新しい）。ラムダの引数を使わず、節に組み立てられない形は、前と同じく、いま評価した値の定数にする。
            Expr::As(..) | Expr::Coalesce(..) | Expr::NullConditional(..) | Expr::Interpolated(_) | Expr::New { .. } | Expr::Is(..) | Expr::Switch(..) | Expr::NewArray { .. } | Expr::NewMatrix { .. } | Expr::Anonymous(_) => {
                let built = self.value_node(thread, depth, expr, scope);
                match built {
                    Err(_) if !mentioned && !expr.declares() => self.constant_node(thread, depth, expr),
                    built => built,
                }
            }
            Expr::Block(statements) => self.block_node(thread, depth, statements, scope),
            Expr::Throw(operand) => self.throw_node(thread, depth, operand, scope, None),
            Expr::Tuple(items) if !items.is_empty() => {
                let mut nodes = Vec::new();
                for item in items {
                    nodes.push(self.tree(thread, depth, item, scope)?);
                }
                self.tuple_node(thread, depth, nodes)
            }
            // `checked(式)` / `unchecked(式)`: 中の整数の計算と変換を、あふれたら例外にする（しない）節にする。
            Expr::Checked(checked, inner) => {
                let outer = std::mem::replace(&mut self.flow()?.checked, *checked);
                let built = self.tree(thread, depth, inner, scope);
                if let Some(flow) = self.lambda_flows.last_mut() {
                    flow.checked = outer;
                }
                built
            }
            Expr::With(source, members) => self.with_node(thread, depth, source, members, scope),
            Expr::Deconstruct(targets, value) => self.deconstruct_node(thread, depth, targets, value, scope),
            // `await 式`: デリゲートを呼んだときに、GetAwaiter().GetResult() で終わるまで待つ。
            Expr::Await(operand) => {
                let awaited = self.tree(thread, depth, operand, scope)?;
                let awaiter = self.parameterless_call_node(thread, depth, awaited, "GetAwaiter")?;
                self.parameterless_call_node(thread, depth, awaiter, "GetResult")
            }
            Expr::Assign(target, kind, value) => self.assign_node(thread, depth, target, kind, value, scope),
            Expr::Increment { target, decrement, prefix } => {
                let variable = self.assignable(thread, depth, target, scope)?;
                let name = match (*prefix, *decrement) {
                    (true, false) => "PreIncrementAssign",
                    (true, true) => "PreDecrementAssign",
                    (false, false) => "PostIncrementAssign",
                    (false, true) => "PostDecrementAssign",
                };
                self.factory(thread, depth, name, vec![variable])
            }
            Expr::Name(name) => self.lambda_variable(scope, name).ok_or_else(|| tr!("'{name}' is not found.", name = name)),
            Expr::Member(target, name) => {
                let target = self.tree(thread, depth, target, scope)?;
                self.member_node(thread, depth, &target, name)
            }
            Expr::Index(target, indices) => {
                let target_node = self.tree(thread, depth, target, scope)?;
                // `^n`（終わりから数えた位置）と範囲（`a..b`）。
                if let [special @ (Expr::FromEnd(_) | Expr::Range(..))] = indices.as_slice() {
                    return self.special_index_node(thread, depth, target_node, special, scope);
                }
                let mut index_nodes = Vec::new();
                for index in indices {
                    index_nodes.push(self.tree(thread, depth, index, scope)?);
                }
                self.element_node(thread, depth, target_node, index_nodes)
            }
            Expr::Call(method, arguments) => {
                // 型引数を書いた呼び出し（`w.Cast<char>()`）。
                let (method, written) = match &**method {
                    Expr::Generic(inner, types) => (&**inner, types.as_slice()),
                    other => (other, &[][..]),
                };
                let receiver_used = mentions(method, &names) || (in_null_conditional && method.any(&mut |inner| *inner == Expr::Receiver));
                let Some(target) = self.call_target(thread, depth, method, receiver_used, arguments.len(), scope)? else {
                    // ほかの書き方は、引数を使わなければ、いま評価する。
                    return if mentioned { Err(tr!("Inside a lambda, only methods named directly can be called.")) } else { self.constant_node(thread, depth, expr) };
                };
                // 引数にラムダ式、名前つき引数、型を渡す先から決める式があれば、渡す先のメソッドを、シグネチャから選ぶ
                // （nested.rs）。
                if arguments.iter().any(|argument| matches!(argument, Expr::Lambda(..) | Expr::Named(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral | Expr::Out(_))) {
                    return self.call_by_signature_node(thread, depth, target, written, arguments, scope);
                }
                let mut type_arguments = Vec::new();
                for ty in written {
                    type_arguments.push(self.type_object(thread, depth, &type_syntax_text(ty))?);
                }
                let mut argument_nodes = Vec::new();
                for argument in arguments {
                    match self.tree(thread, depth, argument, scope) {
                        Ok(node) => argument_nodes.push(node),
                        // 値にならない引数（メソッドの集まりなど）があれば、シグネチャから選ぶ。
                        Err(error) => return self.call_by_signature_node(thread, depth, target, written, arguments, scope).map_err(|other| if is_not_found(&other) { error } else { other }),
                    }
                }
                let plain = match target.clone() {
                    CallTarget::Instance(node, name) => self.call_node_with(thread, depth, Some(node), None, &name, &type_arguments, argument_nodes),
                    CallTarget::Static { type_object, name, .. } => self.call_node_with(thread, depth, None, Some(type_object), &name, &type_arguments, argument_nodes),
                    CallTarget::Invoke(delegate) => {
                        let arguments = self.node_array(thread, depth, EXPRESSION, &argument_nodes)?;
                        return self.factory(thread, depth, "Invoke", vec![delegate, arguments]);
                    }
                    CallTarget::Constructor { .. } => return Err(tr!("The call target is a constructor.")),
                };
                // 名前での呼び出しで合うものが無ければ（`params` をばらで渡した、省略できる引数を省いた）、シグネチャから選ぶ。
                plain.or_else(|error| self.call_by_signature_node(thread, depth, target, written, arguments, scope).map_err(|_| error))
            }
            Expr::Unary(op, operand) => self.unary_node(thread, depth, *op, operand, scope),
            Expr::Binary(op, left, right) => self.binary_node(thread, depth, *op, left, right, scope),
            // 片方の枝が `throw` なら、その枝は、もう一方の枝の型の式。
            Expr::Conditional(condition, when_true, when_false) if matches!(**when_true, Expr::Throw(_)) || matches!(**when_false, Expr::Throw(_)) => {
                let condition = self.tree(thread, depth, condition, scope)?;
                let thrown_first = matches!(**when_true, Expr::Throw(_));
                let (thrown, other) = if thrown_first { (when_true, when_false) } else { (when_false, when_true) };
                let other = self.tree(thread, depth, other, scope)?;
                let ty = self.member(thread, &other, "Type")?;
                let Expr::Throw(operand) = &**thrown else { unreachable!() };
                let thrown = self.throw_node(thread, depth, operand, scope, Some(ty))?;
                let (when_true, when_false) = if thrown_first { (thrown, other) } else { (other, thrown) };
                self.factory(thread, depth, "Condition", vec![condition, when_true, when_false])
            }
            Expr::Conditional(condition, when_true, when_false) => {
                let condition = self.tree(thread, depth, condition, scope)?;
                let branches = vec![self.branch_node(thread, depth, when_true, scope)?, self.branch_node(thread, depth, when_false, scope)?];
                let mut branches = self.unify_branches(thread, depth, branches, &[&**when_true, &**when_false])?;
                branches.insert(0, condition);
                self.factory(thread, depth, "Condition", branches)
            }
            Expr::Cast(ty, operand) => {
                let node = self.tree(thread, depth, operand, scope)?;
                let target = self.type_object(thread, depth, &type_syntax_text(ty))?;
                let convert = if self.lambda_flows.last().is_some_and(|flow| flow.checked) { "ConvertChecked" } else { "Convert" };
                self.factory(thread, depth, convert, vec![node, target])
            }
            // 引数を使わない、そのほかの式（`new Foo()` など）は、前と同じく、いま評価した値を定数にする。
            _ if !mentioned => self.constant_node(thread, depth, expr),
            _ => Err(tr!("Inside a lambda,{construct} is not supported.", construct = construct_name(expr))),
        }
    }

    /// 値を作る書き方（`as`、`??`、`?.`、補間文字列、`new`、`is`、switch 式）の節。
    fn value_node(&mut self, thread: u32, depth: usize, expr: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        match expr {
            Expr::As(operand, ty) => {
                let node = self.tree(thread, depth, operand, scope)?;
                let target = self.type_object(thread, depth, &type_syntax_text(ty))?;
                self.factory(thread, depth, "TypeAs", vec![node, target])
            }
            Expr::Coalesce(left, right) => self.coalesce_node(thread, depth, left, right, scope),
            Expr::NullConditional(target, rest) => self.null_conditional_node(thread, depth, target, rest, scope),
            Expr::Interpolated(pieces) => self.interpolated_node(thread, depth, pieces, scope),
            Expr::New { ty, arguments, initializer } => self.new_object_node(thread, depth, ty, arguments, initializer, scope),
            Expr::Is(operand, pattern) => {
                let value = self.tree(thread, depth, operand, scope)?;
                self.pattern_on(thread, depth, value, pattern)
            }
            Expr::Switch(subject, arms) => self.switch_expression_node(thread, depth, subject, arms, scope),
            Expr::NewArray { element, sizes, items } => self.new_array_node(thread, depth, element.as_ref(), sizes, items.as_deref(), scope),
            Expr::NewMatrix { element, dimensions, items } => self.new_matrix_node(thread, depth, element.as_ref(), dimensions, items, scope),
            Expr::Anonymous(members) => self.anonymous_node(thread, depth, members, scope),
            _ => Err(tr!("It is not a construct that makes a value.")),
        }
    }

    /// ラムダ式の中で見える変数の名前（引数、ローカル変数、パターンの変数）。
    fn lambda_names(&self, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Vec<String> {
        let patterns = self.lambda_flows.last().map(|flow| flow.named.as_slice()).unwrap_or_default();
        scope.iter().chain(patterns).map(|(name, _)| name.clone()).collect()
    }

    /// 名前の指す変数の節（引数、ローカル変数、パターンの変数）。同じ名前があれば、あとから作ったもの。
    fn lambda_variable(&self, scope: &[(String, ComPtr<ICorDebugValue>)], name: &str) -> Option<ComPtr<ICorDebugValue>> {
        let patterns = self.lambda_flows.last().map(|flow| flow.named.as_slice()).unwrap_or_default();
        scope.iter().chain(patterns).rev().find(|(existing, _)| existing == name).map(|(_, variable)| variable.clone())
    }

    /// `対象.名前`（プロパティかフィールド）の節。
    fn member_node(&mut self, thread: u32, depth: usize, target: &ComPtr<ICorDebugValue>, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        // 要素が 8 個以上のタプルの `Item8` 以降は、入れ子のタプル（`Rest`）の中。
        if let Some(number) = name.strip_prefix("Item").and_then(|number| number.parse::<usize>().ok()).filter(|number| *number >= 8) {
            if self.node_type(thread, target)?.starts_with("System.ValueTuple`8") {
                return self.tuple_item_node(thread, depth, target, number);
            }
        }
        let text = self.text(thread, name)?;
        self.factory(thread, depth, "PropertyOrField", vec![target.clone(), text])
    }

    /// `対象[添字, …]` を読む節。配列は `ArrayIndex`、ほかはインデクサーの getter（文字列は `get_Chars`）。
    fn element_node(&mut self, thread: u32, depth: usize, target: ComPtr<ICorDebugValue>, mut indices: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        let target_type = self.node_type(thread, &target)?;
        if is_array_type(&target_type) {
            if indices.len() == 1 {
                let index = indices.remove(0);
                return self.factory(thread, depth, "ArrayIndex", vec![target, index]);
            }
            let indices = self.node_array(thread, depth, EXPRESSION, &indices)?;
            return self.factory(thread, depth, "ArrayIndex", vec![target, indices]);
        }
        let getter = if target_type == "System.String" { "get_Chars" } else { "get_Item" };
        self.call_node(thread, depth, Some(target), None, getter, indices)
    }

    /// 一つの型に揃えなければならない節の並び（条件演算子の枝、switch 式の腕、`new[] { … }` の要素）を、どの節も
    /// 受け取れる型へ変換する。`sources` は、それぞれの節のもとの式。`null` のリテラルは、ほかの節から決まった型の null。
    fn unified(&mut self, thread: u32, depth: usize, nodes: Vec<ComPtr<ICorDebugValue>>, sources: &[&Expr]) -> Result<Vec<ComPtr<ICorDebugValue>>, String> {
        let mut widest: Option<ComPtr<ICorDebugValue>> = None;
        for (node, source) in nodes.iter().zip(sources) {
            if **source == Expr::Null {
                continue;
            }
            widest = Some(match widest {
                Some(widest) => self.wider_of(thread, depth, widest, node.clone())?,
                None => node.clone(),
            });
        }
        let Some(widest) = widest else { return Ok(nodes) };
        let mut converted = Vec::new();
        for node in nodes {
            converted.push(self.converted_like(thread, depth, node, &widest)?);
        }
        Ok(converted)
    }

    /// 一つの型に揃える枝（条件演算子の枝、switch 式の腕）の節。型を書かない `default` は、ほかの枝の型が決まるまで
    /// 作れないので `None`（`unify_branches` が作る）。
    fn branch_node(&mut self, thread: u32, depth: usize, expr: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        if *expr == Expr::DefaultLiteral {
            return Ok(None);
        }
        self.tree(thread, depth, expr, scope).map(Some)
    }

    /// 枝の節を `unified` で揃え、型を書かない `default` の枝を、揃えた型の既定値にする。
    fn unify_branches(&mut self, thread: u32, depth: usize, branches: Vec<Option<ComPtr<ICorDebugValue>>>, sources: &[&Expr]) -> Result<Vec<ComPtr<ICorDebugValue>>, String> {
        let (built, built_sources): (Vec<_>, Vec<_>) = branches.iter().zip(sources).filter_map(|(node, source)| Some((node.clone()?, *source))).unzip();
        let unified = self.unified(thread, depth, built, &built_sources)?;
        if unified.len() == branches.len() {
            return Ok(unified);
        }
        let first = unified.first().ok_or_else(|| tr!("The type of default without a type cannot be determined from the other branches."))?;
        let ty = self.member(thread, first, "Type")?;
        let mut unified = unified.into_iter();
        let mut result = Vec::new();
        for branch in &branches {
            result.push(match branch {
                Some(_) => unified.next().ok_or_else(|| tr!("Cannot build the branches."))?,
                None => self.factory(thread, depth, "Default", vec![ty.clone()])?,
            });
        }
        Ok(result)
    }

    /// 二つの節のうち、もう一方も受け取れる型のほう（その型の節）。数どうしは、C# の数値の規則で決まる型へ変換した節。
    fn wider_of(&mut self, thread: u32, depth: usize, left: ComPtr<ICorDebugValue>, right: ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        if self.node_type(thread, &left)? == self.node_type(thread, &right)? {
            return Ok(left);
        }
        let (balanced_left, balanced_right) = self.balance(thread, depth, left.clone(), right.clone(), false)?;
        if self.node_type(thread, &balanced_left)? == self.node_type(thread, &balanced_right)? {
            return Ok(balanced_left);
        }
        let (left_type, right_type) = (self.member(thread, &left, "Type")?, self.member(thread, &right, "Type")?);
        if self.assignable_from(thread, depth, &left_type, &right_type)? {
            return Ok(left);
        }
        if self.assignable_from(thread, depth, &right_type, &left_type)? {
            return Ok(right);
        }
        Err(tr!("{left} and {right} cannot be made one type (cast to one of the two).", left = keyword_of(&self.node_type(thread, &left)?), right = keyword_of(&self.node_type(thread, &right)?)))
    }

    /// `to` の型の変数に、`from` の型の値をそのまま入れられるか（どちらも対象の中の `System.Type`）。
    fn assignable_from(&mut self, thread: u32, depth: usize, to: &ComPtr<ICorDebugValue>, from: &ComPtr<ICorDebugValue>) -> Result<bool, String> {
        let result = self.call_method(thread, depth, Some(Current::Value(to.clone())), "IsAssignableFrom", vec![from.clone()])?.ok_or_else(|| tr!("Cannot compare the types."))?;
        Ok(self.operand(&result) == Operand::Bool(true))
    }

    /// メソッドの呼び出しの節（`Expression.Call(対象, 名前, 型引数, 引数…)`）。対象が無ければ、型の静的メソッド。
    /// 値にメソッドが無ければ、`System.Linq.Enumerable` の拡張メソッド（`g.Count()`、`ws.Sum()`、`xs.First()`）として呼ぶ。
    fn call_node(&mut self, thread: u32, depth: usize, target: Option<ComPtr<ICorDebugValue>>, type_object: Option<ComPtr<ICorDebugValue>>, name: &str, arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        self.call_node_with(thread, depth, target, type_object, name, &[], arguments)
    }

    /// 型引数を書いた呼び出し（`w.Cast<char>()`）もできる `call_node`。`written` は、書かれた型引数（`System.Type`）。
    #[allow(clippy::too_many_arguments)]
    fn call_node_with(&mut self, thread: u32, depth: usize, target: Option<ComPtr<ICorDebugValue>>, type_object: Option<ComPtr<ICorDebugValue>>, name: &str, written: &[ComPtr<ICorDebugValue>], arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        let array = self.node_array(thread, depth, "System.Linq.Expressions.Expression", &arguments)?;
        let text = self.text(thread, name)?;
        let no_type_arguments = self.new_null(thread)?;
        let written = if written.is_empty() { None } else { Some(self.node_array(thread, depth, "System.Type", written)?) };
        let first = target.clone().or(type_object).ok_or_else(|| tr!("There is no call target."))?;
        let own_type_arguments = written.clone().unwrap_or_else(|| no_type_arguments.clone());
        let error = match self.factory(thread, depth, "Call", vec![first.clone(), text.clone(), own_type_arguments, array]) {
            Ok(node) => return Ok(node),
            Err(error) => error,
        };
        // 引数の型が、仮引数の型そのものではない（`stream.WriteByte(1)` の 1 は int で、仮引数は byte）。名前と引数の数が合う
        // メソッドがただ一つなら、引数をその仮引数の型へ変換して呼ぶ。
        if written.is_none() {
            if let Some(node) = self.converted_call_node(thread, depth, target.as_ref(), first.clone(), name, &arguments) {
                return Ok(node);
            }
        }
        let Some(receiver) = target else { return Err(error) };
        let element = self.sequence_element(thread, &receiver);
        if element.is_none() && written.is_none() {
            return Err(error);
        }
        let enumerable = self.type_object(thread, depth, "System.Linq.Enumerable")?;
        let mut with_receiver = vec![receiver];
        with_receiver.extend(arguments);
        let array = self.node_array(thread, depth, "System.Linq.Expressions.Expression", &with_receiver)?;
        // 書かれた型引数（`Cast<char>`）、要素の型を型引数にする形（Count<TSource> など）、型引数の無い形（Sum(IEnumerable<int>)
        // など）の順に試す。
        let type_arguments = match (written, element) {
            (Some(written), _) => written,
            (None, Some(element)) => self.node_array(thread, depth, "System.Type", &[element])?,
            (None, None) => return Err(error),
        };
        if let Ok(node) = self.factory(thread, depth, "Call", vec![enumerable.clone(), text.clone(), type_arguments, array.clone()]) {
            return Ok(node);
        }
        self.factory(thread, depth, "Call", vec![enumerable, text, no_type_arguments, array]).map_err(|_| error)
    }

    /// 名前と引数の数が合う公開メソッドがただ一つなら、引数をその仮引数の型へ変換して呼ぶ節。そうでなければ `None`。
    /// `first` は、インスタンスメソッドなら対象の節、静的メソッドなら型（`System.Type`）。
    fn converted_call_node(&mut self, thread: u32, depth: usize, target: Option<&ComPtr<ICorDebugValue>>, first: ComPtr<ICorDebugValue>, name: &str, arguments: &[ComPtr<ICorDebugValue>]) -> Option<ComPtr<ICorDebugValue>> {
        let ty = match target {
            Some(target) => self.member(thread, target, "Type").ok()?,
            None => first.clone(),
        };
        let text = self.text(thread, name).ok()?;
        let members = self.call_method(thread, depth, Some(Current::Value(ty)), "GetMember", vec![text]).ok()??;
        let count = unsafe { value::array_length(&members) }?;
        let mut found = None;
        for index in 0..count {
            let member = unsafe { value::array_element(&members, &[index]) }.ok()??;
            // メソッドでないメンバー（同じ名前のプロパティなど）には GetParameters が無い。
            let Ok(Some(parameters)) = self.call_method(thread, depth, Some(Current::Value(member.clone())), "GetParameters", Vec::new()) else { continue };
            if unsafe { value::array_length(&parameters) } == Some(arguments.len() as u32) {
                if found.is_some() {
                    return None;
                }
                found = Some((member, parameters));
            }
        }
        let (method, parameters) = found?;
        let mut converted = Vec::new();
        for (index, argument) in arguments.iter().enumerate() {
            let parameter = unsafe { value::array_element(&parameters, &[index as u32]) }.ok()??;
            let wanted = self.member(thread, &parameter, "ParameterType").ok()?;
            converted.push(self.factory(thread, depth, "Convert", vec![argument.clone(), wanted]).ok()?);
        }
        let array = self.node_array(thread, depth, EXPRESSION, &converted).ok()?;
        match target {
            Some(target) => self.factory(thread, depth, "Call", vec![target.clone(), method, array]).ok(),
            None => self.factory(thread, depth, "Call", vec![method, array]).ok(),
        }
    }

    /// 引数の無いインスタンスメソッドの呼び出しの節。`Expression.Call(対象, 名前, …)` は、基底の型の同じ名前のメソッドを
    /// 隠している型（`Task<T>.GetAwaiter` と `Task.GetAwaiter`）で「どちらか決まらない」と断るので、リフレクションで
    /// いちばん派生した側のメソッドを選んでから呼ぶ。見つからなければ（拡張メソッドなど）、名前での呼び出しに任せる。
    fn parameterless_call_node(&mut self, thread: u32, depth: usize, target: ComPtr<ICorDebugValue>, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let ty = self.member(thread, &target, "Type")?;
        let text = self.text(thread, name)?;
        let no_parameters = self.node_array(thread, depth, "System.Type", &[])?;
        match self.call_method(thread, depth, Some(Current::Value(ty)), "GetMethod", vec![text, no_parameters]) {
            Ok(Some(method)) if unsafe { value::argument_kind(&method) } != ArgumentKind::Null => self.factory(thread, depth, "Call", vec![target, method]),
            _ => self.call_node(thread, depth, Some(target), None, name, Vec::new()),
        }
    }

    /// 節の型が `IEnumerable<T>`（を実装した型）なら、その `T`（対象の中の `System.Type`）。
    fn sequence_element(&mut self, thread: u32, node: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        let ty = self.member(thread, node, "Type").ok()?;
        let full = self.member(thread, &ty, "FullName").ok().and_then(|name| unsafe { value::as_string(&name) })?;
        let sequence = if full.starts_with("System.Collections.Generic.IEnumerable`1[[") {
            ty
        } else {
            let name = self.text(thread, "IEnumerable`1").ok()?;
            let interface = self.call_method(thread, 0, Some(Current::Value(ty)), "GetInterface", vec![name]).ok()??;
            if unsafe { value::argument_kind(&interface) } == ArgumentKind::Null {
                return None;
            }
            interface
        };
        let arguments = self.call_method(thread, 0, Some(Current::Value(sequence)), "GetGenericArguments", Vec::new()).ok()??;
        let index = self.new_int(thread, 0).ok()?;
        self.index(thread, &arguments, &[index]).ok()
    }

    /// 二項演算子の節。両辺の型を C# の規則で揃えてから、`Expression.Add` などを呼ぶ。
    fn binary_node(&mut self, thread: u32, depth: usize, op: BinaryOp, left: &Expr, right: &Expr, scope: &[(String, ComPtr<ICorDebugValue>)]) -> Result<ComPtr<ICorDebugValue>, String> {
        // 型を書かない `default` は、もう一方の辺の型の既定値（`n == default`）。
        let (left, right) = match (left, right) {
            (Expr::DefaultLiteral, Expr::DefaultLiteral) => return Err(tr!("Both sides are default without a type, so the type cannot be determined.")),
            (Expr::DefaultLiteral, other) => {
                let other = self.tree(thread, depth, other, scope)?;
                (self.default_node_like(thread, depth, &other)?, other)
            }
            (other, Expr::DefaultLiteral) => {
                let other = self.tree(thread, depth, other, scope)?;
                let default = self.default_node_like(thread, depth, &other)?;
                (other, default)
            }
            (left, right) => (self.tree(thread, depth, left, scope)?, self.tree(thread, depth, right, scope)?),
        };
        self.binary_of(thread, depth, op, left, right)
    }

    /// 節と同じ型の既定値の節。
    fn default_node_like(&mut self, thread: u32, depth: usize, node: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let ty = self.member(thread, node, "Type")?;
        self.factory(thread, depth, "Default", vec![ty])
    }

    /// 二項演算子の節を、両辺の節から作る。
    fn binary_of(&mut self, thread: u32, depth: usize, op: BinaryOp, left: ComPtr<ICorDebugValue>, right: ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let (left_type, right_type) = (self.node_type(thread, &left)?, self.node_type(thread, &right)?);
        // 文字列の連結は、string.Concat(object, object) の呼び出し。
        if op == BinaryOp::Add && (left_type == "System.String" || right_type == "System.String") {
            let left = self.convert_node(thread, depth, left, "object")?;
            let right = self.convert_node(thread, depth, right, "object")?;
            let string_type = self.type_object(thread, depth, "string")?;
            return self.call_node(thread, depth, None, Some(string_type), "Concat", vec![left, right]);
        }
        if op == BinaryOp::ShiftRightUnsigned {
            return self.unsigned_shift_node(thread, depth, left, right);
        }
        // `checked(…)` の中の整数の計算は、あふれたら OverflowException（Expression の …Checked）。
        let checked = self.lambda_flows.last().is_some_and(|flow| flow.checked);
        let name = match op {
            BinaryOp::Add if checked => "AddChecked",
            BinaryOp::Subtract if checked => "SubtractChecked",
            BinaryOp::Multiply if checked => "MultiplyChecked",
            BinaryOp::Add => "Add",
            BinaryOp::Subtract => "Subtract",
            BinaryOp::Multiply => "Multiply",
            BinaryOp::Divide => "Divide",
            BinaryOp::Remainder => "Modulo",
            BinaryOp::BitAnd => "And",
            BinaryOp::BitOr => "Or",
            BinaryOp::BitXor => "ExclusiveOr",
            BinaryOp::ShiftLeft => "LeftShift",
            BinaryOp::ShiftRight => "RightShift",
            BinaryOp::Equal => "Equal",
            BinaryOp::NotEqual => "NotEqual",
            BinaryOp::Less => "LessThan",
            BinaryOp::LessOrEqual => "LessThanOrEqual",
            BinaryOp::Greater => "GreaterThan",
            BinaryOp::GreaterOrEqual => "GreaterThanOrEqual",
            BinaryOp::And => "AndAlso",
            BinaryOp::Or => "OrElse",
            BinaryOp::ShiftRightUnsigned => unreachable!(),
        };
        let shift = matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight);
        let (left, right) = if shift { (left, right) } else { self.balance(thread, depth, left, right, true)? };
        self.factory(thread, depth, name, vec![left, right])
    }

    /// 二つの節の型を揃える。数どうしなら C# の数値昇格の型へ、片方が null 許容なら両方を null 許容へ。
    /// `arithmetic` は、二項演算子の被演算子として揃える（int より小さい型は int に広げる）こと。
    fn balance(&mut self, thread: u32, depth: usize, left: ComPtr<ICorDebugValue>, right: ComPtr<ICorDebugValue>, arithmetic: bool) -> Result<(ComPtr<ICorDebugValue>, ComPtr<ICorDebugValue>), String> {
        let (left_type, right_type) = (self.node_type(thread, &left)?, self.node_type(thread, &right)?);
        // null 許容の値型（`int?`）が片方でもあれば、中身の型で揃えてから、両方を null 許容にする（C# の持ち上げた演算子）。
        let (left_inner, right_inner) = (nullable_inner(&left_type), nullable_inner(&right_type));
        if left_inner.is_some() || right_inner.is_some() {
            let (a, b) = (left_inner.as_deref().unwrap_or(&left_type), right_inner.as_deref().unwrap_or(&right_type));
            // 数どうしは数の規則で、そうでなければ（bool、列挙型など）null 許容の側の中身の型で。
            let target = numeric_target(a, b, arithmetic).map_or_else(|| keyword_of(left_inner.as_deref().or(right_inner.as_deref()).unwrap_or(a)).to_string(), str::to_string);
            let wanted = format!("{target}?");
            let left = if left_inner.as_deref().map(keyword_of) == Some(target.as_str()) { left } else { self.convert_node(thread, depth, left, &wanted)? };
            let right = if right_inner.as_deref().map(keyword_of) == Some(target.as_str()) { right } else { self.convert_node(thread, depth, right, &wanted)? };
            return Ok((left, right));
        }
        let Some(keyword) = numeric_target(&left_type, &right_type, arithmetic) else { return Ok((left, right)) };
        let left = if keyword_of(&left_type) != keyword { self.convert_node(thread, depth, left, keyword)? } else { left };
        let right = if keyword_of(&right_type) != keyword { self.convert_node(thread, depth, right, keyword)? } else { right };
        Ok((left, right))
    }

    fn convert_node(&mut self, thread: u32, depth: usize, node: ComPtr<ICorDebugValue>, type_name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let target = self.type_object(thread, depth, type_name)?;
        self.factory(thread, depth, "Convert", vec![node, target])
    }
}

use operators::UnaryOp as UnaryOpKind;

/// 数の型（完全な名前）どうしを揃える型のキーワード。`arithmetic` は、二項演算子の被演算子として揃える（int より小さい
/// 型は int に広げる）こと。数でない型があれば `None`。
fn numeric_target(left: &str, right: &str, arithmetic: bool) -> Option<&'static str> {
    let (a, b) = (Typed::from_keyword(keyword_of(left)), Typed::from_keyword(keyword_of(right)));
    let target = match (&a.kind, &b.kind) {
        (Kind::Int(x), Kind::Int(y)) if arithmetic => operators::promote_pair(*x, *y).map(Kind::Int),
        (Kind::Int(_) | Kind::Float { .. } | Kind::Decimal, Kind::Int(_) | Kind::Float { .. } | Kind::Decimal) => types::unify(&a, &b).map(|typed| typed.kind),
        _ => None,
    }?;
    Some(match target {
        Kind::Int(kind) => operators::int_keyword(kind),
        Kind::Float { single: true } => "float",
        Kind::Float { single: false } => "double",
        Kind::Decimal => "decimal",
        _ => return None,
    })
}

/// リフレクションの型の名前（`Type.FullName`）を、式に書く型の名前（`System.Collections.Generic.List<System.Int32>`）にする。
fn display_of_reflection(full: &str) -> Option<String> {
    parse_reflection_type(full).map(|ty| type_syntax_text(&ty))
}

/// リフレクションの型の名前（`Type.FullName`）が、表示の書き方の型の名前（`int?`、`System.Collections.Generic.List<int>`）と
/// 同じ型を指すか。
fn reflection_name_is(full: &str, display: &str) -> bool {
    display_of_reflection(full).is_some_and(|shown| calls::same_type(&shown, display))
}

/// null 許容の値型（`Type.FullName` が ``System.Nullable`1[[…]]``）の、中身の型の名前（`System.Int32`）。
fn nullable_inner(full: &str) -> Option<String> {
    if !full.starts_with("System.Nullable`1[") {
        return None;
    }
    let ty = parse_reflection_type(full)?;
    Some(type_syntax_text(&TypeSyntax { nullable: false, ..ty }))
}
