//! ラムダ式の `{ … }` の本体の文を、式木の節にする。
//!
//! - `if` は `IfThen` / `IfThenElse`、ループは `Loop` と、break / continue の飛び先（`LabelTarget`）。
//! - 途中の `return` は、本体の最後に置いた飛び先への `Return`。飛び先の型は、デリゲートの戻り値の型か、最初の return の式の型。
//! - `foreach` は `GetEnumerator` / `MoveNext` / `Current` と、終わりの `Dispose`。`using` は try-finally と `Dispose`、
//!   `lock` は `Monitor.Enter` / `Exit`。どれも、C# のコンパイラが作る形と同じ。
//! - `switch` 文は、`case` が定数だけなら `Switch`。パターンや `when` があれば、上から順に確かめる if の並び。

use super::*;
use crate::debugger::session::eval::syntax::{Catch, Pattern, SwitchSection};

type Node = ComPtr<ICorDebugValue>;
type Scope = Vec<(String, Node)>;

impl Session {
    pub(super) fn flow(&mut self) -> Result<&mut LambdaFlow, String> {
        self.lambda_flows.last_mut().ok_or_else(|| tr!("A block of statements { } can be written only in a lambda body."))
    }

    /// 節の並びを、変数つきのブロックにする。節が無ければ、何もしない節。
    pub(super) fn block_of(&mut self, thread: u32, depth: usize, variables: &[Node], nodes: &[Node]) -> Result<Node, String> {
        if nodes.is_empty() {
            return self.factory(thread, depth, "Empty", Vec::new());
        }
        let variables = self.node_array(thread, depth, PARAMETER, variables)?;
        let expressions = self.node_array(thread, depth, EXPRESSION, nodes)?;
        self.factory(thread, depth, "Block", vec![variables, expressions])
    }

    /// 節のあとに何もしない節を置いて、値の無い（void の）節にする。try と catch、switch の腕は、型を揃えなければならない。
    fn without_value(&mut self, thread: u32, depth: usize, node: Node) -> Result<Node, String> {
        let empty = self.factory(thread, depth, "Empty", Vec::new())?;
        self.block_of(thread, depth, &[], &[node, empty])
    }

    /// ラムダ式の本体（`{ var y = x * 2; return y + 1; }`）。
    pub(super) fn block_node(&mut self, thread: u32, depth: usize, statements: &[Statement], scope: &[(String, Node)]) -> Result<Node, String> {
        // return が最後の文だけなら、飛び先を作らずに、その式の値を本体の値にする（節が少なくて済む）。
        if let Some((Statement::Return(Some(value)), before)) = statements.split_last() {
            if !before.iter().any(returns) {
                let mut scope = scope.to_vec();
                let (mut variables, mut nodes) = (Vec::new(), Vec::new());
                self.statement_nodes(thread, depth, before, &mut scope, &mut variables, &mut nodes)?;
                nodes.push(self.tree(thread, depth, value, &scope)?);
                return self.block_of(thread, depth, &variables, &nodes);
            }
        }
        let body = self.statements_block(thread, depth, statements, scope)?;
        let Some(ReturnTarget { label, value_type: ty }) = self.flow()?.exit.take() else { return Ok(body) };
        // return の飛び先。return を通らずに最後まで来たときの値は、その型の既定値（値を返すラムダでは、C# がそういう道を許さない）。
        let end = match ty {
            Some((type_object, _)) => {
                let default = self.factory(thread, depth, "Default", vec![type_object])?;
                self.factory(thread, depth, "Label", vec![label, default])?
            }
            None => self.factory(thread, depth, "Label", vec![label])?,
        };
        self.block_of(thread, depth, &[], &[body, end])
    }

    /// 文の並びを、自分の変数を持つブロックにする。
    fn statements_block(&mut self, thread: u32, depth: usize, statements: &[Statement], scope: &[(String, Node)]) -> Result<Node, String> {
        let mut scope = scope.to_vec();
        let (mut variables, mut nodes) = (Vec::new(), Vec::new());
        self.statement_nodes(thread, depth, statements, &mut scope, &mut variables, &mut nodes)?;
        self.block_of(thread, depth, &variables, &nodes)
    }

    /// 文の並びを節にして `nodes` に足す。宣言された変数は `scope` と `variables` に足す（あとの文から見える）。
    fn statement_nodes(&mut self, thread: u32, depth: usize, statements: &[Statement], scope: &mut Scope, variables: &mut Vec<Node>, nodes: &mut Vec<Node>) -> Result<(), String> {
        for statement in statements {
            match statement {
                Statement::Local(written, declared) => self.declare(thread, depth, written.as_ref(), declared, scope, variables, nodes)?,
                other => nodes.push(self.statement_node(thread, depth, other, scope)?),
            }
        }
        Ok(())
    }

    /// ローカル変数の宣言。`var` は初期値の型。型を書いたら、初期値をその型へ変換する（`long y = x`）。
    #[allow(clippy::too_many_arguments)]
    fn declare(&mut self, thread: u32, depth: usize, written: Option<&TypeSyntax>, declared: &[(String, Option<Expr>)], scope: &mut Scope, variables: &mut Vec<Node>, nodes: &mut Vec<Node>) -> Result<(), String> {
        for (name, initial) in declared {
            if scope.iter().any(|(existing, _)| existing == name) {
                return Err(tr!("The name '{name}' is already used inside the lambda.", name = name));
            }
            let initial = match (initial, written) {
                // 型を書いた変数の初期値のラムダ式や `new()` / `[…]` / `default` は、その型で作る（`Func<int, int> f = x => x + n`）。
                (Some(initial @ (Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral)), Some(written)) => {
                    let shown = type_syntax_text(written);
                    Some(match initial {
                        Expr::Lambda(..) => self.lambda_for_type(thread, depth, &shown, initial, scope)?,
                        _ => self.targeted_node(thread, depth, initial, &shown, scope)?,
                    })
                }
                // `var f = (int x) => x + 1`: 型の決まったラムダ式は、Func / Action（C# 10 の、ラムダ式の自然な型）。
                (Some(Expr::Lambda(parameters, body)), None) if parameters.iter().all(|(ty, _)| ty.is_some()) => {
                    let inputs: Vec<String> = parameters.iter().map(|(ty, _)| ty.as_ref().map(type_syntax_text).unwrap_or_default()).collect();
                    let outer = self.outer_scope(scope);
                    Some(self.lambda_expression(thread, depth, parameters, body, &inputs, None, &outer, false)?.0)
                }
                (Some(initial), _) => Some(self.tree(thread, depth, initial, scope)?),
                (None, _) => None,
            };
            let ty = match (written, &initial) {
                (Some(written), _) => self.type_object(thread, depth, &type_syntax_text(written))?,
                (None, Some(initial)) => self.member(thread, initial, "Type")?,
                (None, None) => return Err(tr!("Give an initial value to a variable declared with var.")),
            };
            let variable = self.variable(thread, depth, ty, name)?;
            if let Some(initial) = initial {
                let initial = self.converted_like(thread, depth, initial, &variable)?;
                nodes.push(self.factory(thread, depth, "Assign", vec![variable.clone(), initial])?);
            }
            variables.push(variable.clone());
            scope.push((name.clone(), variable));
        }
        Ok(())
    }

    pub(super) fn variable(&mut self, thread: u32, depth: usize, ty: Node, name: &str) -> Result<Node, String> {
        let text = self.text(thread, name)?;
        self.factory(thread, depth, "Variable", vec![ty, text])
    }

    /// 変数を作り、値を代入する節を `nodes` に足す。
    fn assigned_variable(&mut self, thread: u32, depth: usize, name: &str, value: Node, variables: &mut Vec<Node>, nodes: &mut Vec<Node>) -> Result<Node, String> {
        let ty = self.member(thread, &value, "Type")?;
        let variable = self.variable(thread, depth, ty, name)?;
        nodes.push(self.factory(thread, depth, "Assign", vec![variable.clone(), value])?);
        variables.push(variable.clone());
        Ok(variable)
    }

    /// 文を一つ、節にする。
    fn statement_node(&mut self, thread: u32, depth: usize, statement: &Statement, scope: &[(String, Node)]) -> Result<Node, String> {
        match statement {
            // 文として 1 つだけ書かれた宣言（`if (x) int y = 1;` は C# では書けないが、ブロックと同じに扱う）。
            Statement::Local(..) => self.statements_block(thread, depth, std::slice::from_ref(statement), scope),
            Statement::Expr(expr) => self.tree(thread, depth, expr, scope),
            Statement::Block(inner) => self.statements_block(thread, depth, inner, scope),
            Statement::Return(value) => self.return_node(thread, depth, value.as_ref(), scope),
            Statement::If(condition, then, otherwise) => {
                let condition = self.tree(thread, depth, condition, scope)?;
                let then = self.statement_node(thread, depth, then, scope)?;
                match otherwise {
                    Some(otherwise) => {
                        let otherwise = self.statement_node(thread, depth, otherwise, scope)?;
                        self.factory(thread, depth, "IfThenElse", vec![condition, then, otherwise])
                    }
                    None => self.factory(thread, depth, "IfThen", vec![condition, then]),
                }
            }
            Statement::While(condition, body) => {
                let (exit, next) = self.loop_labels(thread, depth)?;
                let condition = self.tree(thread, depth, condition, scope)?;
                let body = self.in_loop(thread, depth, &exit, Some(&next), body, scope)?;
                let leave = self.factory(thread, depth, "Break", vec![exit.clone()])?;
                let round = self.factory(thread, depth, "IfThenElse", vec![condition, body, leave])?;
                self.factory(thread, depth, "Loop", vec![round, exit, next])
            }
            Statement::Do(body, condition) => {
                // continue は、条件の検査の前へ飛ぶ。
                let (exit, next) = self.loop_labels(thread, depth)?;
                let body = self.in_loop(thread, depth, &exit, Some(&next), body, scope)?;
                let condition = self.tree(thread, depth, condition, scope)?;
                let mark = self.factory(thread, depth, "Label", vec![next])?;
                let leave = self.factory(thread, depth, "Break", vec![exit.clone()])?;
                let stay = self.factory(thread, depth, "Empty", Vec::new())?;
                let check = self.factory(thread, depth, "IfThenElse", vec![condition, stay, leave])?;
                let round = self.block_of(thread, depth, &[], &[body, mark, check])?;
                self.factory(thread, depth, "Loop", vec![round, exit])
            }
            Statement::For { init, condition, step, body } => {
                let mut scope = scope.to_vec();
                let (mut variables, mut nodes) = (Vec::new(), Vec::new());
                self.statement_nodes(thread, depth, init, &mut scope, &mut variables, &mut nodes)?;
                let (exit, next) = self.loop_labels(thread, depth)?;
                let condition = match condition {
                    Some(condition) => self.tree(thread, depth, condition, &scope)?,
                    None => self.constant_node(thread, depth, &Expr::Bool(true))?,
                };
                // continue は、更新の式の前へ飛ぶ。
                let mut round = vec![self.in_loop(thread, depth, &exit, Some(&next), body, &scope)?, self.factory(thread, depth, "Label", vec![next])?];
                for step in step {
                    round.push(self.tree(thread, depth, step, &scope)?);
                }
                let round = self.block_of(thread, depth, &[], &round)?;
                let leave = self.factory(thread, depth, "Break", vec![exit.clone()])?;
                let round = self.factory(thread, depth, "IfThenElse", vec![condition, round, leave])?;
                nodes.push(self.factory(thread, depth, "Loop", vec![round, exit])?);
                self.block_of(thread, depth, &variables, &nodes)
            }
            Statement::ForEach { ty, names, source, body } => self.foreach_node(thread, depth, ty.as_ref(), names, source, body, scope),
            Statement::Switch(subject, sections) => self.switch_node(thread, depth, subject, sections, scope),
            Statement::Try { body, catches, finally } => self.try_node(thread, depth, body, catches, finally.as_deref(), scope),
            Statement::Using(resource, body) => {
                let mut scope = scope.to_vec();
                let (mut variables, mut nodes) = (Vec::new(), Vec::new());
                let before = variables.len();
                match &**resource {
                    Statement::Local(written, declared) => self.declare(thread, depth, written.as_ref(), declared, &mut scope, &mut variables, &mut nodes)?,
                    Statement::Expr(expr) => {
                        let value = self.tree(thread, depth, expr, &scope)?;
                        self.assigned_variable(thread, depth, "resource", value, &mut variables, &mut nodes)?;
                    }
                    _ => return Err(tr!("Write a variable declaration or an expression as the using resource.")),
                }
                let body = self.statements_block(thread, depth, body, &scope)?;
                // 宣言と逆の順に Dispose する。
                let mut disposals = Vec::new();
                for resource in variables[before..].iter().rev().cloned() {
                    disposals.push(self.dispose_node(thread, depth, resource)?);
                }
                let disposals = self.block_of(thread, depth, &[], &disposals)?;
                nodes.push(self.factory(thread, depth, "TryFinally", vec![body, disposals])?);
                self.block_of(thread, depth, &variables, &nodes)
            }
            Statement::Lock(gate, body) => {
                let (mut variables, mut nodes) = (Vec::new(), Vec::new());
                let gate = self.tree(thread, depth, gate, scope)?;
                let gate = self.convert_node(thread, depth, gate, "object")?;
                let gate = self.assigned_variable(thread, depth, "gate", gate, &mut variables, &mut nodes)?;
                let monitor = self.type_object(thread, depth, "System.Threading.Monitor")?;
                nodes.push(self.call_node(thread, depth, None, Some(monitor.clone()), "Enter", vec![gate.clone()])?);
                let body = self.statement_node(thread, depth, body, scope)?;
                let release = self.call_node(thread, depth, None, Some(monitor), "Exit", vec![gate])?;
                nodes.push(self.factory(thread, depth, "TryFinally", vec![body, release])?);
                self.block_of(thread, depth, &variables, &nodes)
            }
            Statement::Break => {
                let exit = self.flow()?.loops.last().map(|(exit, _)| exit.clone()).ok_or_else(|| tr!("break can be written only inside a loop or switch."))?;
                self.factory(thread, depth, "Break", vec![exit])
            }
            Statement::Continue => {
                let next = self.flow()?.loops.iter().rev().find_map(|(_, next)| next.clone()).ok_or_else(|| tr!("continue can be written only inside a loop."))?;
                self.factory(thread, depth, "Continue", vec![next])
            }
            Statement::Rethrow => self.factory(thread, depth, "Rethrow", Vec::new()),
        }
    }

    /// `return 式;` / `return;`。飛び先は、最初の return で作る。
    fn return_node(&mut self, thread: u32, depth: usize, value: Option<&Expr>, scope: &[(String, Node)]) -> Result<Node, String> {
        let returns = self.flow()?.returns.clone();
        let value = match (value, returns) {
            // `return x => x + 1;`: 戻り値のデリゲートの型で作る。
            (Some(lambda @ Expr::Lambda(..)), Some(Some(output))) => Some(self.lambda_for_type(thread, depth, &output, lambda, scope)?),
            (Some(value), _) => Some(self.tree(thread, depth, value, scope)?),
            (None, _) => None,
        };
        if self.flow()?.exit.is_none() {
            let ty = match (self.flow()?.returns.clone(), &value) {
                (Some(Some(output)), _) => Some(self.type_object(thread, depth, &output)?),
                (None, Some(value)) => Some(self.member(thread, value, "Type")?),
                (Some(None), _) | (None, None) => None,
            };
            let exit = match ty {
                Some(ty) => {
                    let name = self.member(thread, &ty, "FullName")?;
                    let name = unsafe { value::as_string(&name) }.ok_or_else(|| tr!("Cannot read the name of the return type."))?;
                    ReturnTarget { label: self.factory(thread, depth, "Label", vec![ty.clone()])?, value_type: Some((ty, name)) }
                }
                None => ReturnTarget { label: self.factory(thread, depth, "Label", Vec::new())?, value_type: None },
            };
            self.flow()?.exit = Some(exit);
        }
        let Some(ReturnTarget { label, value_type: ty }) = self.flow()?.exit.clone() else { return Err(tr!("Cannot create the target of return.")) };
        match (value, ty) {
            (Some(value), Some((type_object, name))) => {
                let value = if self.node_type(thread, &value)? == name { value } else { self.factory(thread, depth, "Convert", vec![value, type_object])? };
                self.factory(thread, depth, "Return", vec![label, value])
            }
            // 値を返さないラムダの `return 式;` は、C# では書けない。式は実行して、戻る。
            (Some(value), None) => {
                let leave = self.factory(thread, depth, "Return", vec![label])?;
                self.block_of(thread, depth, &[], &[value, leave])
            }
            (None, _) => self.factory(thread, depth, "Return", vec![label]),
        }
    }

    /// ループの break と continue の飛び先。
    fn loop_labels(&mut self, thread: u32, depth: usize) -> Result<(Node, Node), String> {
        Ok((self.factory(thread, depth, "Label", Vec::new())?, self.factory(thread, depth, "Label", Vec::new())?))
    }

    /// ループ（や switch）の本体を、break / continue の飛び先を積んだ状態で組み立てる。
    fn in_loop(&mut self, thread: u32, depth: usize, exit: &Node, next: Option<&Node>, body: &Statement, scope: &[(String, Node)]) -> Result<Node, String> {
        self.flow()?.loops.push((exit.clone(), next.cloned()));
        let built = self.statement_node(thread, depth, body, scope);
        if let Some(flow) = self.lambda_flows.last_mut() {
            flow.loops.pop();
        }
        built
    }

    /// `値 as IDisposable` が null でなければ `Dispose()` を呼ぶ節。
    fn dispose_node(&mut self, thread: u32, depth: usize, target: Node) -> Result<Node, String> {
        let disposable = self.type_object(thread, depth, "System.IDisposable")?;
        let cast = self.factory(thread, depth, "TypeAs", vec![target, disposable])?;
        let nothing = self.constant_node(thread, depth, &Expr::Null)?;
        let present = self.factory(thread, depth, "ReferenceNotEqual", vec![cast.clone(), nothing])?;
        let dispose = self.call_node(thread, depth, Some(cast), None, "Dispose", Vec::new())?;
        self.factory(thread, depth, "IfThen", vec![present, dispose])
    }

    /// `foreach (型 名前 in 式) 文`。
    #[allow(clippy::too_many_arguments)]
    fn foreach_node(&mut self, thread: u32, depth: usize, written: Option<&TypeSyntax>, names: &[String], source: &Expr, body: &Statement, scope: &[(String, Node)]) -> Result<Node, String> {
        let source = self.tree(thread, depth, source, scope)?;
        // 要素の型が分かれば IEnumerable<T> として、分からなければ IEnumerable（要素は object）として回す。
        let (sequence_type, enumerator_type) = match self.sequence_element(thread, &source) {
            Some(element) => {
                let arguments = self.node_array(thread, depth, "System.Type", &[element])?;
                let mut closed = Vec::new();
                for open in ["System.Collections.Generic.IEnumerable<>", "System.Collections.Generic.IEnumerator<>"] {
                    let open = self.type_object(thread, depth, open)?;
                    closed.push(self.call_method(thread, depth, Some(Current::Value(open)), "MakeGenericType", vec![arguments.clone()])?.ok_or_else(|| tr!("Cannot create the enumeration type."))?);
                }
                (closed.remove(0), closed.remove(0))
            }
            None => (self.type_object(thread, depth, "System.Collections.IEnumerable")?, self.type_object(thread, depth, "System.Collections.IEnumerator")?),
        };
        let (mut variables, mut nodes) = (Vec::new(), Vec::new());
        let sequence = self.factory(thread, depth, "Convert", vec![source, sequence_type])?;
        let enumerator = self.call_node(thread, depth, Some(sequence), None, "GetEnumerator", Vec::new())?;
        let enumerator = self.factory(thread, depth, "Convert", vec![enumerator, enumerator_type])?;
        let enumerator = self.assigned_variable(thread, depth, "enumerator", enumerator, &mut variables, &mut nodes)?;
        // MoveNext は、型引数の無い IEnumerator のメソッド。
        let plain = self.convert_node(thread, depth, enumerator.clone(), "System.Collections.IEnumerator")?;
        let advance = self.call_node(thread, depth, Some(plain), None, "MoveNext", Vec::new())?;
        let current_name = self.text(thread, "Current")?;
        let current = self.factory(thread, depth, "Property", vec![enumerator.clone(), current_name])?;

        // 要素の変数。分解（`var (k, v)`）は、タプルの ItemN か、KeyValuePair の Key / Value。
        let mut inner_scope = scope.to_vec();
        let (mut item_variables, mut item_nodes) = (Vec::new(), Vec::new());
        if let [name] = names {
            let current = match written {
                Some(written) => self.convert_node(thread, depth, current, &type_syntax_text(written))?,
                None => current,
            };
            let item = self.assigned_variable(thread, depth, name, current, &mut item_variables, &mut item_nodes)?;
            inner_scope.push((name.clone(), item));
        } else {
            let whole = self.assigned_variable(thread, depth, "item", current, &mut item_variables, &mut item_nodes)?;
            let pair = self.node_type(thread, &whole)?.starts_with("System.Collections.Generic.KeyValuePair`2");
            for (index, name) in names.iter().enumerate() {
                let member = match (pair, index) {
                    (true, 0) => "Key".to_string(),
                    (true, 1) => "Value".to_string(),
                    _ => format!("Item{}", index + 1),
                };
                let member = self.text(thread, &member)?;
                let part = self.factory(thread, depth, "PropertyOrField", vec![whole.clone(), member]).map_err(|_| tr!("foreach can deconstruct only tuple and KeyValuePair elements."))?;
                let part = self.assigned_variable(thread, depth, name, part, &mut item_variables, &mut item_nodes)?;
                inner_scope.push((name.clone(), part));
            }
        }
        let (exit, next) = self.loop_labels(thread, depth)?;
        item_nodes.push(self.in_loop(thread, depth, &exit, Some(&next), body, &inner_scope)?);
        let round = self.block_of(thread, depth, &item_variables, &item_nodes)?;
        let leave = self.factory(thread, depth, "Break", vec![exit.clone()])?;
        let round = self.factory(thread, depth, "IfThenElse", vec![advance, round, leave])?;
        let looped = self.factory(thread, depth, "Loop", vec![round, exit, next])?;
        let dispose = self.dispose_node(thread, depth, enumerator)?;
        nodes.push(self.factory(thread, depth, "TryFinally", vec![looped, dispose])?);
        self.block_of(thread, depth, &variables, &nodes)
    }

    /// switch 文の、ラベルの付いた文の並び。`break` は switch の終わり（`exit`）へ、`continue` は外のループへ。
    fn section_body(&mut self, thread: u32, depth: usize, section: &SwitchSection, exit: &Node, scope: &[(String, Node)]) -> Result<Node, String> {
        let outer_next = self.flow()?.loops.iter().rev().find_map(|(_, next)| next.clone());
        self.flow()?.loops.push((exit.clone(), outer_next));
        let body = self.statements_block(thread, depth, &section.body, scope);
        if let Some(flow) = self.lambda_flows.last_mut() {
            flow.loops.pop();
        }
        self.without_value(thread, depth, body?)
    }

    /// `switch (式) { case …: … default: … }`。
    fn switch_node(&mut self, thread: u32, depth: usize, subject: &Expr, sections: &[SwitchSection], scope: &[(String, Node)]) -> Result<Node, String> {
        let value = self.tree(thread, depth, subject, scope)?;
        let exit = self.factory(thread, depth, "Label", Vec::new())?;
        // ラベルが定数だけなら、式木の `Switch`。パターンや `when` があれば、上から順に確かめる if の並び。
        let plain = sections.iter().flat_map(|section| &section.labels).flatten().all(|(pattern, guard)| {
            guard.is_none()
                && match pattern {
                    Pattern::Null => true,
                    Pattern::TypeOrConstant(ty, _) => ty.as_ref().is_none_or(|ty| self.resolve_type(thread, depth, ty).is_err()),
                    _ => false,
                }
        });
        if !plain {
            return self.pattern_switch_node(thread, depth, value, sections, exit, scope);
        }
        let (mut cases, mut default) = (Vec::new(), None);
        for section in sections {
            let mut tests = Vec::new();
            for label in &section.labels {
                match label {
                    None => {}
                    Some((Pattern::TypeOrConstant(_, constant), _)) => {
                        let test = self.tree(thread, depth, constant, scope)?;
                        tests.push(self.converted_like(thread, depth, test, &value)?);
                    }
                    Some(_) => tests.push(self.constant_node(thread, depth, &Expr::Null)?),
                }
            }
            let body = self.section_body(thread, depth, section, &exit, scope)?;
            if section.labels.iter().any(Option::is_none) {
                default = Some(body.clone());
            }
            if !tests.is_empty() {
                let tests = self.node_array(thread, depth, EXPRESSION, &tests)?;
                cases.push(self.factory(thread, depth, "SwitchCase", vec![body, tests])?);
            }
        }
        let end = self.factory(thread, depth, "Label", vec![exit])?;
        // case が一つも無ければ、式を評価して default を実行するだけ。
        if cases.is_empty() {
            let mut nodes = vec![value];
            nodes.extend(default);
            nodes.push(end);
            return self.block_of(thread, depth, &[], &nodes);
        }
        let cases = self.node_array(thread, depth, "System.Linq.Expressions.SwitchCase", &cases)?;
        let switch = match default {
            Some(default) => self.factory(thread, depth, "Switch", vec![value, default, cases])?,
            None => self.factory(thread, depth, "Switch", vec![value, cases])?,
        };
        self.block_of(thread, depth, &[], &[switch, end])
    }

    /// パターンや `when` のある switch 文。式木の `Switch` は定数しか比べられないので、ラベルを上から順に確かめる if の
    /// 並びにする（`default` は、書かれた場所によらず、最後）。
    fn pattern_switch_node(&mut self, thread: u32, depth: usize, value: Node, sections: &[SwitchSection], exit: Node, scope: &[(String, Node)]) -> Result<Node, String> {
        let mut steps = Vec::new();
        let input = if self.rereadable(&value) {
            value
        } else {
            let (assign, variable) = self.stored(thread, depth, value, None)?;
            steps.push(assign);
            variable
        };
        let (mut branches, mut default) = (Vec::new(), None);
        for section in sections {
            // 一つの文の並びに付いたラベルは、どれかが合えばよい。
            let mut test: Option<Node> = None;
            for (pattern, guard) in section.labels.iter().flatten() {
                let mut one = self.pattern_node(thread, depth, &input, pattern)?;
                if let Some(guard) = guard {
                    let guard = self.tree(thread, depth, guard, scope)?;
                    one = self.factory(thread, depth, "AndAlso", vec![one, guard])?;
                }
                test = Some(match test {
                    Some(test) => self.factory(thread, depth, "OrElse", vec![test, one])?,
                    None => one,
                });
            }
            let body = self.section_body(thread, depth, section, &exit, scope)?;
            if section.labels.iter().any(Option::is_none) {
                default = Some(body.clone());
            }
            if let Some(test) = test {
                branches.push((test, body));
            }
        }
        let mut chain = match default {
            Some(default) => default,
            None => self.factory(thread, depth, "Empty", Vec::new())?,
        };
        for (test, body) in branches.into_iter().rev() {
            chain = self.factory(thread, depth, "IfThenElse", vec![test, body, chain])?;
        }
        steps.push(chain);
        steps.push(self.factory(thread, depth, "Label", vec![exit])?);
        self.block_of(thread, depth, &[], &steps)
    }

    /// `try { … } catch (型 名前) when (条件) { … } finally { … }`。
    fn try_node(&mut self, thread: u32, depth: usize, body: &[Statement], catches: &[Catch], finally: Option<&[Statement]>, scope: &[(String, Node)]) -> Result<Node, String> {
        let body = self.statements_block(thread, depth, body, scope)?;
        let body = self.without_value(thread, depth, body)?;
        let mut handlers = Vec::new();
        for handler in catches {
            let ty = match &handler.ty {
                Some(ty) => self.type_object(thread, depth, &type_syntax_text(ty))?,
                None => self.type_object(thread, depth, "System.Exception")?,
            };
            let mut scope = scope.to_vec();
            // 例外を受ける変数があれば、それを。無ければ、型だけを渡す。
            let caught = match &handler.name {
                Some(name) => {
                    let variable = self.variable(thread, depth, ty, name)?;
                    scope.push((name.clone(), variable.clone()));
                    variable
                }
                None => ty,
            };
            let handler_body = self.statements_block(thread, depth, &handler.body, &scope)?;
            let handler_body = self.without_value(thread, depth, handler_body)?;
            let mut arguments = vec![caught, handler_body];
            if let Some(filter) = &handler.filter {
                arguments.push(self.tree(thread, depth, filter, &scope)?);
            }
            handlers.push(self.factory(thread, depth, "Catch", arguments)?);
        }
        let finally = match finally {
            Some(finally) => Some(self.statements_block(thread, depth, finally, scope)?),
            None => None,
        };
        if handlers.is_empty() {
            let finally = finally.ok_or_else(|| tr!("There is neither catch nor finally after try."))?;
            return self.factory(thread, depth, "TryFinally", vec![body, finally]);
        }
        let handlers = self.node_array(thread, depth, "System.Linq.Expressions.CatchBlock", &handlers)?;
        match finally {
            Some(finally) => self.factory(thread, depth, "TryCatchFinally", vec![body, finally, handlers]),
            None => self.factory(thread, depth, "TryCatch", vec![body, handlers]),
        }
    }
}
