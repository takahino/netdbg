//! メソッドの呼び出し: 対象の決定、引数の評価、オーバーロードの選択、引数の受け渡し（ボックス化、out / ref）。

use super::*;

/// 静的メソッドを選べなかった理由（エラー文を変えるのに使う）。
enum StaticMiss {
    /// その名前と引数の数のメソッドが、一つも無い。
    NoSuchMethod,
    /// メソッドはあるが、この型の引数を受け取るものが無い。
    NoneFits,
}

impl Session {
    /// `式(引数, …)`。
    pub(super) fn eval_call(&mut self, thread: u32, depth: usize, method: &Expr, arguments: &[Expr]) -> Result<Current, String> {
        Ok({
            // `名前<型>(…)`: 書かれた型引数。
            let (method, type_arguments) = match method {
                Expr::Generic(inner, types) => (&**inner, types.clone()),
                other => (other, Vec::new()),
            };
            let invoke = String::from("Invoke");
            let (target, name) = match method {
                Expr::Member(target, name) => {
                    let owner = self.evaluate_node(thread, depth, target)?;
                    // `dynamic` の値のメソッドは、バインダで実行時に選ぶ。
                    if let Current::Value(receiver) = &owner {
                        if self.is_dynamic(receiver) {
                            let arguments = self.evaluate_arguments(thread, depth, arguments)?;
                            let receiver = receiver.clone();
                            return Ok(match self.dynamic_invoke(thread, depth, &receiver, name, arguments)? {
                                Some(value) => Current::Value(value),
                                None => Current::Void,
                            });
                        }
                    }
                    // フィールドに入ったデリゲートの呼び出し（`holder.Step(4)`）は、その値の Invoke。
                    let delegate = match &owner {
                        Current::Value(owner) => unsafe { value::field(owner, &self.modules, name) }.filter(|field| unsafe { value::type_names(field, &self.modules) }.iter().any(|ty| ty == "System.Delegate")),
                        _ => None,
                    };
                    match delegate {
                        Some(delegate) => (Some(Current::Value(delegate)), &invoke),
                        None => (Some(owner), name),
                    }
                }
                Expr::Name(name) => (None, name),
                // デリゲートの値の呼び出し（`((Func<int, int>)(x => x))(1)`）は、その Invoke。
                other => (Some(self.evaluate_node(thread, depth, other)?), &invoke),
            };
            // 位置の引数と、名前つき引数。書かれた順に評価する。
            let mut extra = bind::CallExtra { type_arguments, ..bind::CallExtra::default() };
            let mut positional = Vec::new();
            for argument in arguments {
                match argument {
                    // 型を引数の型から決める式も、型が決まってから作る。
                    Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral => {
                        extra.targeted.push((positional.len(), argument.clone()));
                        positional.push(self.new_null(thread)?);
                    }
                    // ラムダ式は、渡す先の引数の型が決まってから作る。それまでは null を置いておく。
                    Expr::Lambda(parameters, body) => {
                        extra.lambdas.push((positional.len(), parameters.clone(), (**body).clone()));
                        positional.push(self.new_null(thread)?);
                    }
                    Expr::Out(target) => {
                        // 渡すのは、いまある変数ならその場所、新しい変数なら置き場所（呼び出しの直前に作る）。
                        let value = match target {
                            syntax::OutTarget::Existing(expr) => {
                                let current = self.evaluate_node(thread, depth, expr)?;
                                self.value_of(current)?
                            }
                            _ => self.new_null(thread)?,
                        };
                        extra.outs.push((positional.len(), target.clone()));
                        positional.push(value);
                    }
                    Expr::Named(name, value) => {
                        let value = self.evaluate_node(thread, depth, value)?;
                        extra.named.push((name.clone(), self.value_of(value)?));
                    }
                    _ if !extra.named.is_empty() => return Err(tr!("Positional arguments cannot come after named arguments.")),
                    other => match self.evaluate_node(thread, depth, other).and_then(|value| self.value_of(value)) {
                        Ok(value) => positional.push(value),
                        // 値にならない名前がメソッドを指していれば、メソッドの集まり（デリゲートを受け取る引数に渡す）。
                        Err(error) => {
                            if !self.is_method_group(thread, depth, other) {
                                return Err(error);
                            }
                            extra.groups.push((positional.len(), other.clone()));
                            positional.push(self.new_null(thread)?);
                        }
                    },
                }
            }
            match self.call_method_with(thread, depth, target, name, positional, &extra)? {
                Some(value) => Current::Value(value),
                None => Current::Void,
            }
        })
    }

    pub(super) fn evaluate_arguments(&mut self, thread: u32, depth: usize, arguments: &[Expr]) -> Result<Vec<ComPtr<ICorDebugValue>>, String> {
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let current = self.evaluate_node(thread, depth, argument)?;
            values.push(self.value_of(current)?);
        }
        Ok(values)
    }

    /// メソッドを呼ぶ（位置の引数だけ）。
    pub(super) fn call_method(&mut self, thread: u32, depth: usize, target: Option<Current>, name: &str, arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        self.call_method_with(thread, depth, target, name, arguments, &bind::CallExtra::default())
    }

    /// メソッドを呼ぶ。`target` が値ならインスタンスメソッド、型なら静的メソッド。無ければ（名前だけの呼び出し）、
    /// 変数に入ったデリゲート、`this` のメソッド、いまの型の静的メソッドの順に探す。
    ///
    /// 先に、引数の数と型がそのまま合うオーバーロードを選ぶ。無ければ、広げた形（省略できる引数、名前つき引数、
    /// `params`、型引数つきのメソッド、拡張メソッド。`bind.rs`）で探す。戻り値は、値を返さないメソッドなら `None`。
    pub(super) fn call_method_with(&mut self, thread: u32, depth: usize, target: Option<Current>, name: &str, arguments: Vec<ComPtr<ICorDebugValue>>, extra: &bind::CallExtra) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        if !extra.has_lambdas() {
            return self.bind_and_call(thread, depth, target, name, arguments, extra);
        }
        // ラムダ式を組み立てられなかったせいで、どのオーバーロードにも渡せなかったときは、「メソッドがありません」ではなく、
        // 組み立てられなかった理由を伝える。呼ぶメソッドが決まれば、理由は捨てる（`bind_and_call` の中）。
        let outer = self.lambda_failure.take();
        let result = self.bind_and_call(thread, depth, target, name, arguments, extra);
        match (result, std::mem::replace(&mut self.lambda_failure, outer)) {
            (Err(_), Some(reason)) => Err(reason),
            (result, _) => result,
        }
    }

    fn bind_and_call(&mut self, thread: u32, depth: usize, target: Option<Current>, name: &str, mut arguments: Vec<ComPtr<ICorDebugValue>>, extra: &bind::CallExtra) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let count = arguments.len() as u32;
        let marks: Vec<usize> = extra.outs.iter().map(|(position, _)| *position).collect();
        // out / ref を使う呼び出しは、引数がそのまま合うものだけ（広げた形の結び付けでは扱わない）。
        let expandable = marks.is_empty();
        let (callable, shown) = match target {
            Some(Current::Value(value) | Current::Nullable(value)) => {
                let type_name = unsafe { value::describe(&value, &self.modules) }.type_name;
                let shown = format!("{type_name}.{name}");
                let receiver = value.clone();
                let mut candidates = unsafe { value::instance_method_candidates(&value, &self.modules, name, count) };
                let mut value = value;
                if candidates.is_empty() {
                    if let Some((module, row)) = self.primitive_type(&value) {
                        candidates = self.primitive_method_candidates(module, row, name, count);
                        if !candidates.is_empty() {
                            // プリミティブ型の値は、ボックス化してから `this` として渡す。値をそのまま渡しても、写しを作って
                            // 渡しても、どのメソッドも ArgumentOutOfRangeException になった。ボックスなら正しく動く。
                            value = self.box_primitive(thread, module, row, &value)?;
                        }
                    }
                }
                // 配列のメソッド（GetLength など）は、System.Array のもの。
                if candidates.is_empty() && unsafe { value::array_length(&value) }.is_some() {
                    if let Some((module, row)) = self.modules.iter().enumerate().find_map(|(index, m)| Some((index, m.metadata()?.find_type("System.Array")?))) {
                        candidates = self.primitive_method_candidates(module, row, name, count);
                    }
                }
                let nothing_found = candidates.is_empty();
                let chosen = if extra.is_plain() && !nothing_found { self.choose_overload_marked(thread, candidates, &mut arguments, &marks) } else { None };
                match chosen {
                    Some(callable) => {
                        arguments.insert(0, value);
                        (callable, shown)
                    }
                    None => match self.bind_instance(thread, depth, &receiver, name, &arguments, extra).filter(|_| expandable) {
                        Some((callable, bound)) => {
                            arguments = bound;
                            (callable, shown)
                        }
                        None if nothing_found => return Err(tr!("{type} has no method '{name}' with {count} arguments.", type = type_name, count = count, name = name)),
                        None => return Err(tr!("No {method} takes arguments of these types.", method = shown)),
                    },
                }
            }
            Some(Current::Type { module, row }) => {
                let shown = format!("{}.{name}", self.type_name(module, row));
                match self.bind_static(thread, depth, (module, row), &[], name, &mut arguments, extra) {
                    Ok(callable) => (callable, shown),
                    // C# 14 の拡張ブロックの静的メソッド（`string.Shout("a")`）。
                    Err(miss) => match self.static_extension_owner(thread, depth, &self.plain_type_name(module, row), name).map(|owner| self.bind_static(thread, depth, owner, &[], name, &mut arguments, extra)) {
                        Some(Ok(callable)) => (callable, shown),
                        _ => match miss {
                            StaticMiss::NoSuchMethod => return Err(tr!("The type {type} has no static method '{name}' that takes these arguments.", type = self.type_name(module, row), name = name)),
                            StaticMiss::NoneFits => return Err(tr!("No {method} takes arguments of these types.", method = shown)),
                        },
                    },
                }
            }
            Some(Current::Generic { module, row, arguments: class_arguments }) => {
                let shown = format!("{}.{name}", self.type_name(module, row));
                match self.bind_static(thread, depth, (module, row), &class_arguments, name, &mut arguments, extra) {
                    Ok(callable) => (callable, shown),
                    // C# 14 の拡張ブロックの静的メソッド（`IEnumerable<int>.Triple(4)`）。
                    Err(_) => {
                        let receiver = self.generic_display(module, row, &class_arguments);
                        match self.static_extension_owner(thread, depth, &receiver, name).map(|owner| self.bind_static(thread, depth, owner, &[], name, &mut arguments, extra)) {
                            Some(Ok(callable)) => (callable, shown),
                            _ => return Err(tr!("{name} has no static method that takes these arguments.", name = shown)),
                        }
                    }
                }
            }
            Some(Current::Namespace(namespace)) => return Err(tr!("No variable, member or type named '{name}' was found.", name = namespace)),
            Some(Current::Void) => return Err(tr!("The result of a method that returns no value cannot be used in an expression.")),
            None => {
                // DebuggerDisplay の式では、名前だけの呼び出しは、表示しているオブジェクトのメソッド。
                if let Some(this) = self.display_this.clone() {
                    if let Ok(result) = self.call_method_with(thread, depth, Some(Current::Value(this)), name, arguments.clone(), extra) {
                        return Ok(result);
                    }
                }
                // 名前が変数（デリゲート）なら、その Invoke を呼ぶ。C# でも、変数の名前はメソッドの名前より先に見つかる。
                let variables = self.frame_variables(thread, depth)?;
                if let Some((_, delegate)) = variables.iter().find(|(existing, _)| existing == name) {
                    return self.call_method_with(thread, depth, Some(Current::Value(delegate.clone())), "Invoke", arguments, extra);
                }
                // `this` のメソッド。
                if let Some((_, this)) = variables.into_iter().find(|(existing, _)| existing == "this") {
                    let candidates = unsafe { value::instance_method_candidates(&this, &self.modules, name, count) };
                    let mut attempt = arguments.clone();
                    let chosen = if extra.is_plain() && !candidates.is_empty() { self.choose_overload_marked(thread, candidates, &mut attempt, &marks) } else { None };
                    let bound = match chosen {
                        Some(callable) => {
                            attempt.insert(0, this);
                            Some((callable, attempt))
                        }
                        None => self.bind_instance(thread, depth, &this, name, &arguments, extra).filter(|_| expandable),
                    };
                    if let Some((callable, bound)) = bound {
                        self.forget_lambda_failure(extra);
                        return self.run_call_with_outs(thread, depth, &callable, bound, count as usize, name, &extra.outs);
                    }
                }
                // いまの型と、それを囲む型の静的メソッド。
                let owners = self.frame_types(thread, depth);
                if owners.is_empty() {
                    return Err(tr!("Method '{name}' is not found.", name = name));
                }
                let mut bound = None;
                for owner in owners {
                    let mut attempt = arguments.clone();
                    if let Ok(callable) = self.bind_static(thread, depth, owner, &[], name, &mut attempt, extra) {
                        bound = Some((callable, attempt));
                        break;
                    }
                }
                let Some((callable, bound)) = bound else { return Err(tr!("No method '{name}' that takes these arguments was found.", name = name)) };
                arguments = bound;
                (callable, name.to_string())
            }
        };
        self.forget_lambda_failure(extra);
        self.run_call_with_outs(thread, depth, &callable, arguments, count as usize, &shown, &extra.outs)
    }

    /// 呼ぶメソッドが決まった。ほかのオーバーロードにラムダ式を渡せなかった理由は、もう要らない（呼んだメソッドの失敗を、
    /// その理由で置き換えないように）。ラムダ式を渡す呼び出しだけが消す。ラムダ式の組み立ての途中の呼び出し（`Compile`
    /// など）で消すと、組み立てられなかった理由が失われる。
    fn forget_lambda_failure(&mut self, extra: &bind::CallExtra) {
        if extra.has_lambdas() {
            self.lambda_failure = None;
        }
    }

    /// 型の静的メソッドを選ぶ。先に、引数の数と型がそのまま合うもの。無ければ、広げた形（`bind.rs`）で探し、見つかれば
    /// `arguments` を、その形に並べ直した引数に置き換える。`class_arguments` は、型引数つきの型（`List<int>`）の型引数。
    #[allow(clippy::too_many_arguments, reason = "式の評価の関数は (thread, depth) を先頭に取る約束。束ねると評価の全部のシグネチャが変わる (2026-11-18 の整理で見送った)")]
    fn bind_static(&mut self, thread: u32, depth: usize, owner: (usize, u32), class_arguments: &[ComPtr<ICorDebugType>], name: &str, arguments: &mut Vec<ComPtr<ICorDebugValue>>, extra: &bind::CallExtra) -> Result<Callable, StaticMiss> {
        let marks: Vec<usize> = extra.outs.iter().map(|(position, _)| *position).collect();
        let mut candidates = self.type_method_candidates(owner.0, owner.1, name, true, arguments.len() as u32);
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = class_arguments.to_vec();
        }
        let miss = if candidates.is_empty() { StaticMiss::NoSuchMethod } else { StaticMiss::NoneFits };
        if extra.is_plain() && !candidates.is_empty() {
            if let Some(callable) = self.choose_overload_marked(thread, candidates, arguments, &marks) {
                return Ok(callable);
            }
        }
        // out / ref を使う呼び出しは、引数がそのまま合うものだけ（広げた形の結び付けでは扱わない）。
        let (callable, bound) = self.bind_expanded(thread, depth, owner, name, true, class_arguments, arguments, extra).filter(|_| marks.is_empty()).ok_or(miss)?;
        *arguments = bound;
        Ok(callable)
    }

    /// `out` の引数の置き場所を作ってから呼び、呼んだあとで、置き場所の値を新しい変数に入れる。
    /// 置き場所は、要素 1 個の配列の要素（ランタイムは、場所を持つ値を参照で渡す。対象の中で作っただけの値は、
    /// 場所を持たない）。`explicit` は、`this` を除いた引数の数。
    #[allow(clippy::too_many_arguments, reason = "式の評価の関数は (thread, depth) を先頭に取る約束。束ねると評価の全部のシグネチャが変わる (2026-11-18 の整理で見送った)")]
    pub(super) fn run_call_with_outs(&mut self, thread: u32, depth: usize, callable: &Callable, mut arguments: Vec<ComPtr<ICorDebugValue>>, explicit: usize, shown: &str, outs: &[(usize, syntax::OutTarget)]) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        if outs.is_empty() {
            return self.run_call(thread, callable, &arguments, shown);
        }
        let offset = arguments.len() - explicit;
        let shapes = self.callable_parameter_types(callable).ok_or_else(|| tr!("Cannot read the type of the out argument."))?;
        let mut holders = Vec::new();
        for (position, target) in outs {
            if let syntax::OutTarget::Existing(_) = target {
                continue;
            }
            let shape = shapes.get(*position).and_then(|shape| shape.as_ref()).ok_or_else(|| tr!("The type of the out argument is not known."))?;
            let (name, array) = match shape.name.strip_suffix("[]") {
                Some(element) => (element.to_string(), true),
                None => (shape.name.clone(), false),
            };
            let arguments_of = shape.arguments.iter().map(|argument| TypeSyntax::named(syntax::keyword_type(argument).unwrap_or(argument))).collect();
            let ty = TypeSyntax { array, arguments: arguments_of, ..TypeSyntax::named(syntax::keyword_type(&name).unwrap_or(&name)) };
            let element = self.runtime_type(thread, depth, &ty)?;
            let holder = self.new_array(thread, &element, &[1]).map_err(|failure| failure.describe(self))?;
            let slot = unsafe { value::array_element(&holder, &[0]) }?.ok_or_else(|| tr!("Cannot create the place for out."))?;
            arguments[offset + position] = slot;
            holders.push((holder, target.clone()));
        }
        let result = self.run_call(thread, callable, &arguments, shown)?;
        for (holder, target) in holders {
            if let syntax::OutTarget::Declare(_, name) = target {
                let value = unsafe { value::array_element(&holder, &[0]) }?.ok_or_else(|| tr!("Cannot read the out value."))?;
                self.pattern_locals.push((name, value));
            }
        }
        Ok(result)
    }

    /// 選んだメソッドを実行する。中で例外が出たら、その例外のオブジェクトが結果になる。
    pub(super) fn run_call(&mut self, thread: u32, callable: &Callable, arguments: &[ComPtr<ICorDebugValue>], shown: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        match self.call(thread, callable, arguments) {
            Ok(result) => Ok(result),
            Err(funceval::EvalFailure::Threw(exception)) => Ok(Some(exception)),
            Err(failure) => Err(tr!("Cannot run {method}: {reason}", method = shown, reason = failure.describe(self))),
        }
    }

    /// 呼ぶメソッドの引数の型（型引数は、実際の型に置き換えたもの）。
    pub(super) fn callable_parameter_types(&self, callable: &Callable) -> Option<Vec<Option<crate::debugger::metadata::TypeShape>>> {
        unsafe {
            let mut token = 0;
            callable.function.GetToken(&mut token);
            let mut module = null_mut();
            callable.function.GetModule(&mut module);
            let type_arguments: Vec<String> = callable.type_arguments.iter().map(|ty| value::type_name(ty, &self.modules)).collect();
            let module = ComPtr::<ICorDebugModule>::from_raw(module)?;
            let metadata = value::Modules::metadata(&self.modules, &module)?;
            let of_class = metadata.declaring_type(token & 0x00FF_FFFF).map_or(0, |ty| metadata.type_parameter_count(ty)).min(type_arguments.len());
            metadata.parameter_types(token, &type_arguments[..of_class], &type_arguments[of_class..])
        }
    }

    /// 値のメソッドを、広げた形で結び付ける。インスタンスメソッドが先で、無ければ拡張メソッド。
    /// 戻り値の引数の並びは、受け手を先頭に含む。
    pub(super) fn bind_instance(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, name: &str, arguments: &[ComPtr<ICorDebugValue>], extra: &bind::CallExtra) -> Option<(Callable, Vec<ComPtr<ICorDebugValue>>)> {
        let primitive = self.primitive_type(receiver);
        let owner = self.user_type(receiver).or(primitive).or_else(|| {
            let is_string = unsafe { value::argument_kind(receiver) } == ArgumentKind::String;
            is_string.then(|| self.modules.iter().enumerate().find_map(|(index, m)| Some((index, m.metadata()?.find_type("System.String")?)))).flatten()
        });
        if let Some(owner) = owner {
            let class_arguments = unsafe { value::exact_type_of(receiver).map(|ty| value::type_arguments_of(&ty)) }.unwrap_or_default();
            if let Some((callable, mut bound)) = self.bind_expanded(thread, depth, owner, name, false, &class_arguments, arguments, extra) {
                // プリミティブ型の値は、ボックス化してから `this` として渡す。
                let this = match primitive {
                    Some((module, row)) => self.box_primitive(thread, module, row, receiver).ok()?,
                    None => receiver.clone(),
                };
                bound.insert(0, this);
                return Some((callable, bound));
            }
        }
        self.bind_extension(thread, depth, receiver, name, arguments, extra)
    }

    /// 型が宣言している、名前・静的かどうか・引数の数が合うメソッドすべてと、それぞれの引数の型の種類。
    pub(in crate::debugger::session) fn type_method_candidates(&self, module: usize, type_row: u32, name: &str, is_static: bool, parameters: u32) -> Vec<(Callable, Vec<ParameterKind>)> {
        let Some(metadata) = self.modules[module].metadata() else { return Vec::new() };
        let mut candidates = Vec::new();
        for token in metadata.find_methods_shaped(type_row, name, is_static, parameters) {
            let mut raw = null_mut();
            unsafe { self.modules[module].module.GetFunctionFromToken(token, &mut raw) };
            if let (Some(kinds), Some(function)) = (metadata.parameter_kinds(token), unsafe { ComPtr::<ICorDebugFunction>::from_raw(raw) }) {
                candidates.push((Callable { function, type_arguments: Vec::new() }, kinds));
            }
        }
        candidates
    }

    /// プリミティブ型の値（`int`、`bool` など）の、ランタイムでの型（`System.Int32` など）。モジュールの添字と型の行番号。
    /// 値からは型のクラスをたどれないので、名前で探す。
    pub(super) fn primitive_type(&self, target: &ComPtr<ICorDebugValue>) -> Option<(usize, u32)> {
        let ArgumentKind::Primitive(element) = (unsafe { value::argument_kind(target) }) else { return None };
        let type_name = primitive_type_name(element)?;
        self.modules.iter().enumerate().find_map(|(index, m)| Some((index, m.metadata()?.find_type(type_name)?)))
    }

    /// プリミティブ型のメソッド。基底の型（`ValueType`、`Object`）まで見る。
    pub(super) fn primitive_method_candidates(&self, module: usize, type_row: u32, name: &str, parameters: u32) -> Vec<(Callable, Vec<ParameterKind>)> {
        let mut candidates = Vec::new();
        let mut row = type_row;
        for _ in 0..8 {
            candidates.extend(self.type_method_candidates(module, row, name, false, parameters));
            match self.modules[module].metadata().and_then(|m| m.base_type(row)) {
                Some(base) => row = base,
                None => break,
            }
        }
        candidates
    }

    /// プリミティブ型の値を、対象の中でボックス化する（空のボックスを作って、値を書き込む）。
    pub(super) fn box_primitive(&mut self, thread: u32, module: usize, type_row: u32, target: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let (_, bytes) = unsafe { value::primitive_bytes(target) }.ok_or_else(|| tr!("Cannot read the value."))?;
        // 中身は、ただ一つのインスタンスフィールド（`m_value`）に入っている。
        let field = self.modules[module].metadata().and_then(|m| m.fields(type_row).into_iter().find(|f| !f.is_static).map(|f| f.token)).ok_or_else(|| tr!("A value of this type cannot be boxed."))?;
        let class = unsafe { take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | type_row, out)) }?;
        let boxed = self.new_object_without_constructor(thread, &class).map_err(|failure| failure.describe(self))?;
        unsafe { value::fill_box(&boxed, &class, field, &bytes) }?;
        Ok(boxed)
    }

    /// 構造体や列挙型の値を、同じ型のボックスに写す。
    pub(super) fn box_value(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        let ty = unsafe { value::exact_type_of(value) }.ok_or_else(|| tr!("The value's type is not known."))?;
        let boxed = self.new_object_of_type(thread, &ty).map_err(|failure| failure.describe(self))?;
        let inside = unsafe { value::inside_box(&boxed) }.ok_or_else(|| tr!("Cannot read the contents of the box."))?;
        self.write(thread, &inside, value)?;
        Ok(boxed)
    }

    /// 参照型（クラス、インターフェイス、配列）の引数に、渡そうとしている値の型が合っているか。
    ///
    /// 型の合わないオブジェクトを渡すと、対象のプロセスが落ちる（`string.Join(",", numbers)` で、`string[]` の引数に
    /// `int[]` を渡して起きた）。確かめられない型の引数には渡さない。
    pub(super) fn references_fit(&mut self, thread: u32, callable: &Callable, parameters: &[ParameterKind], arguments: &[ComPtr<ICorDebugValue>], kinds: &[ArgumentKind]) -> bool {
        let by_reference = |index: usize| parameters.get(index) == Some(&ParameterKind::Reference) && matches!(kinds.get(index), Some(ArgumentKind::Reference | ArgumentKind::String));
        let by_value = |index: usize| parameters.get(index) == Some(&ParameterKind::ValueType) && kinds.get(index) == Some(&ArgumentKind::ValueType);
        let checked = |index: usize| by_reference(index) || by_value(index);
        if !(0..arguments.len()).any(checked) {
            return true;
        }
        let shapes = unsafe {
            let mut token = 0;
            callable.function.GetToken(&mut token);
            let mut module = null_mut();
            callable.function.GetModule(&mut module);
            // 型引数は、メソッドを持つ型の分が先で、メソッドの分があと。
            let type_arguments: Vec<String> = callable.type_arguments.iter().map(|ty| value::type_name(ty, &self.modules)).collect();
            ComPtr::<ICorDebugModule>::from_raw(module).and_then(|module| {
                let metadata = value::Modules::metadata(&self.modules, &module)?;
                let of_class = metadata.declaring_type(token & 0x00FF_FFFF).map_or(0, |ty| metadata.type_parameter_count(ty)).min(type_arguments.len());
                metadata.parameter_types(token, &type_arguments[..of_class], &type_arguments[of_class..])
            })
        };
        let Some(shapes) = shapes else { return false };
        (0..arguments.len()).filter(|&index| checked(index)).all(|index| {
            let Some(Some(shape)) = shapes.get(index) else { return false };
            let argument = &arguments[index];
            unsafe {
                // 値型は、型そのものが同じでなければならない（別の構造体を渡すと、中身が別の型として読まれる）。
                if by_value(index) {
                    let exact = value::type_names(argument, &self.modules).into_iter().next().map(|name| crate::debugger::metadata::without_arity(&name));
                    return exact.as_deref() == Some(shape.name.as_str()) && same_types(&value::exact_type_arguments(argument, &self.modules), &shape.arguments);
                }
                if same_type(&value::describe(argument, &self.modules).type_name, &shape.display()) {
                    return true;
                }
                // 基底の型か、実装しているインターフェイス。ジェネリックなら、型引数も同じであること
                // （`List<string>` は `IEnumerable<string>` に渡せる。`IEnumerable<int>` には渡せない）。
                let known = value::type_names(argument, &self.modules).iter().any(|name| crate::debugger::metadata::without_arity(name) == shape.name);
                if !known {
                    return false;
                }
                if shape.arguments.is_empty() || same_types(&value::exact_type_arguments(argument, &self.modules), &shape.arguments) {
                    return true;
                }
            }
            // 値の型の型引数と、インターフェイスの型引数が違う形（`Select` の結果の `ArraySelectIterator<int, int>` を
            // `IEnumerable<int>` として、など）。対象のリフレクションで、そのインターフェイスとしての型引数を調べる。
            (0..shape.arguments.len()).all(|position| {
                self.generic_argument_of(thread, argument, Some(shape), shape.arguments.len(), position).is_some_and(|found| same_type(&unsafe { value::type_name(&found, &self.modules) }, &shape.arguments[position]))
            })
        })
    }

    /// 引数にいちばん合うオーバーロードを選ぶ。暗黙の変換が要る引数（`int` を `long` の引数に渡す、など）は、
    /// その型の値を作り直して `arguments` を書き換える。どれにも渡せなければ `None`。
    pub(in crate::debugger::session) fn choose_overload(&mut self, thread: u32, candidates: Vec<(Callable, Vec<ParameterKind>)>, arguments: &mut [ComPtr<ICorDebugValue>]) -> Option<Callable> {
        self.choose_overload_marked(thread, candidates, arguments, &[])
    }

    /// `choose_overload` と同じ。`marks` の位置の引数は、`out` / `ref` を付けて渡すもの。
    pub(super) fn choose_overload_marked(&mut self, thread: u32, candidates: Vec<(Callable, Vec<ParameterKind>)>, arguments: &mut [ComPtr<ICorDebugValue>], marks: &[usize]) -> Option<Callable> {
        // 構造体は、静的フィールドや呼び出しの結果ではボックスとして見える。引数としては、値型として扱う。
        let kinds: Vec<ArgumentKind> = arguments
            .iter()
            .map(|a| unsafe {
                match value::argument_kind(a) {
                    ArgumentKind::Reference if value::unboxed_primitive(a, &self.modules).is_none() && value::type_names(a, &self.modules).iter().any(|name| name == "System.ValueType") => ArgumentKind::ValueType,
                    kind => kind,
                }
            })
            .enumerate()
            .map(|(index, kind)| if marks.contains(&index) { ArgumentKind::ByRef } else { kind })
            .collect();
        // 参照型と値型の引数は、渡す値の型が合うものだけを残す。
        let mut fitting = Vec::new();
        for (callable, parameters) in candidates {
            if self.references_fit(thread, &callable, &parameters, arguments, &kinds) {
                fitting.push((callable, parameters));
            }
        }
        let mut candidates = fitting;
        let overloads: Vec<Vec<ParameterKind>> = candidates.iter().map(|(_, parameters)| parameters.clone()).collect();
        let (callable, parameters) = candidates.swap_remove(best_overload(&overloads, &kinds)?);
        // 型引数の仮引数（`List<long?>.Add(T)` の T）は、実際の型の値を渡す。int のまま渡すと、対象のランタイムが
        // 値を別の型として読んで落ちる（`new List<long?> { 1 }` で Internal CLR error になった）。
        if parameters.contains(&ParameterKind::TypeParameter) {
            let shapes = self.callable_parameter_types(&callable)?;
            for (index, parameter) in parameters.iter().enumerate() {
                if *parameter != ParameterKind::TypeParameter {
                    continue;
                }
                let Some(Some(shape)) = shapes.get(index) else { continue };
                if let Some(fitted) = self.fitted_to_type(thread, &arguments[index], &shape.display()).ok()? {
                    arguments[index] = fitted;
                }
            }
        }
        for ((argument, &parameter), &kind) in arguments.iter_mut().zip(&parameters).zip(&kinds) {
            // `object` の引数に数を渡すときは、ボックス化する。そのまま渡すと、数がオブジェクトの場所として読まれて、
            // 対象のプロセスが落ちる（.NET 11 で `string.Format("{0}", 1)` を評価して起きた）。
            // 値型の引数には、ボックスの中の値を渡す。
            if let (ParameterKind::ValueType, ArgumentKind::ValueType) = (parameter, kind) {
                if let Some(inner) = unsafe { value::inside_box(argument) } {
                    *argument = inner;
                }
                continue;
            }
            // 構造体と列挙型の値（ボックスでないもの）も、object の引数にはボックス化して渡す。
            if let (ParameterKind::Object | ParameterKind::Reference, ArgumentKind::ValueType) = (parameter, kind) {
                if argument.cast::<ICorDebugReferenceValue>().is_err() {
                    *argument = self.box_value(thread, argument).ok()?;
                }
                continue;
            }
            if let (ParameterKind::Object, ArgumentKind::Primitive(_)) = (parameter, kind) {
                let (module, row) = self.primitive_type(argument)?;
                *argument = self.box_primitive(thread, module, row, argument).ok()?;
                continue;
            }
            let (ParameterKind::Primitive(wanted), ArgumentKind::Primitive(given)) = (parameter, kind) else { continue };
            if wanted == given {
                continue;
            }
            *argument = self.widened(thread, argument, wanted)?;
        }
        Some(callable)
    }

    /// int の値を、long / float / double の値に作り直す（`fit` が受け付ける暗黙の変換と同じ範囲）。
    fn widened(&mut self, thread: u32, argument: &ComPtr<ICorDebugValue>, wanted: u8) -> Option<ComPtr<ICorDebugValue>> {
        let number: i32 = unsafe { value::describe(argument, &self.modules) }.value.parse().ok()?;
        let bytes: Vec<u8> = match wanted {
            ELEMENT_TYPE_I8 => (number as i64).to_le_bytes().to_vec(),
            ELEMENT_TYPE_R4 => (number as f32).to_le_bytes().to_vec(),
            ELEMENT_TYPE_R8 => (number as f64).to_le_bytes().to_vec(),
            _ => return None,
        };
        self.new_primitive(thread, wanted as u32, &bytes).ok()
    }

    /// 型引数の仮引数に渡す値を、実際の型（`ty`、表示の書き方）の値にする。作り直したら `Ok(Some)`、そのままでよければ
    /// `Ok(None)`、渡せなければ `Err`。いま見るのは、プリミティブ型と、その null 許容の型だけ。
    fn fitted_to_type(&mut self, thread: u32, argument: &ComPtr<ICorDebugValue>, ty: &str) -> Result<Option<ComPtr<ICorDebugValue>>, ()> {
        let Some(target) = syntax::parse_type(ty).filter(|target| !target.array && target.pointers == 0) else { return Ok(None) };
        let kind = unsafe { value::argument_kind(argument) };
        // `object` の型引数（`Func<object, object>.Invoke(T)`）に数を渡すときは、ボックス化する。
        if target.name == "System.Object" && !target.nullable {
            if let ArgumentKind::Primitive(_) = kind {
                let (module, row) = self.primitive_type(argument).ok_or(())?;
                return self.box_primitive(thread, module, row, argument).map(Some).map_err(|_| ());
            }
            return Ok(None);
        }
        let Some(element) = primitive_element(&target.name) else { return Ok(None) };
        if !target.nullable {
            return match kind {
                ArgumentKind::Primitive(given) if given == element => Ok(None),
                ArgumentKind::Primitive(ELEMENT_TYPE_I4) => self.widened(thread, argument, element).map(Some).ok_or(()),
                _ => Err(()),
            };
        }
        // 渡す値が、すでに同じ null 許容の型なら、そのまま。
        if unsafe { value::nullable(argument, &self.modules) }.is_some() {
            return Ok(None);
        }
        // `Nullable<T>` の値を作る: 中身が 0 の値（null）を作り、値があれば hasValue と value を書く。
        let runtime = self.runtime_type(thread, 0, &target).map_err(|_| ())?;
        let created = self.new_object_of_type(thread, &runtime).map_err(|_| ())?;
        let inner = unsafe { value::inside_box(&created) }.unwrap_or(created);
        match kind {
            ArgumentKind::Null => {}
            ArgumentKind::Primitive(given) => {
                let number = if given == element { argument.clone() } else if given == ELEMENT_TYPE_I4 { self.widened(thread, argument, element).ok_or(())? } else { return Err(()) };
                let slot = unsafe { value::field(&inner, &self.modules, "value") }.ok_or(())?;
                self.write(thread, &slot, &number).map_err(|_| ())?;
                let flag = unsafe { value::field(&inner, &self.modules, "hasValue") }.ok_or(())?;
                let yes = self.new_bool(thread, true).map_err(|_| ())?;
                self.write(thread, &flag, &yes).map_err(|_| ())?;
            }
            _ => return Err(()),
        }
        Ok(Some(inner))
    }
}

/// 二つの型の名前が、同じ型を指すか。書き方の違い（`int?` と `System.Nullable<System.Int32>`、`int` と `System.Int32`）は
/// 同じとみなす。
pub(super) fn same_type(left: &str, right: &str) -> bool {
    fn canonical(ty: crate::debugger::session::eval::syntax::TypeSyntax) -> crate::debugger::session::eval::syntax::TypeSyntax {
        let mut ty = ty;
        if ty.name == "System.Nullable" && ty.arguments.len() == 1 && !ty.nullable {
            let shape = ty.shape();
            let inner = ty.arguments.remove(0);
            ty = crate::debugger::session::eval::syntax::TypeSyntax { nullable: true, ..inner }.with_shape(&shape);
        }
        ty.arguments = ty.arguments.into_iter().map(canonical).collect();
        ty
    }
    if left == right {
        return true;
    }
    match (syntax::parse_type(left), syntax::parse_type(right)) {
        (Some(left), Some(right)) => canonical(left) == canonical(right),
        _ => false,
    }
}

/// 型の名前の並びが、同じ型の並びか（`same_type`）。
pub(super) fn same_types(left: &[String], right: &[String]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(left, right)| same_type(left, right))
}

/// 引数を、その型の引数に渡せるか。渡せるなら、合い方の良さ（大きいほど良い）。
fn fit(parameter: ParameterKind, argument: ArgumentKind) -> Option<u32> {
    match (parameter, argument) {
        // `out` / `ref` を付けた引数は、ref / out の引数にだけ渡せる（逆も同じ）。
        (ParameterKind::ByRef, ArgumentKind::ByRef) => Some(3),
        (ParameterKind::ByRef, _) | (_, ArgumentKind::ByRef) => None,
        (ParameterKind::Primitive(wanted), ArgumentKind::Primitive(given)) if wanted == given => Some(3),
        // int から long / float / double へは、C# と同じく暗黙に変換する（値は作り直して渡す）。
        (ParameterKind::Primitive(ELEMENT_TYPE_I8 | ELEMENT_TYPE_R4 | ELEMENT_TYPE_R8), ArgumentKind::Primitive(ELEMENT_TYPE_I4)) => Some(1),
        (ParameterKind::String, ArgumentKind::String | ArgumentKind::Null) => Some(3),
        // 参照型どうしは、型が合うかまでは見ていない（合わなければ、呼び出しが失敗する）。
        (ParameterKind::Reference, ArgumentKind::Reference | ArgumentKind::String | ArgumentKind::Null) => Some(2),
        (ParameterKind::ValueType, ArgumentKind::ValueType) => Some(2),
        (ParameterKind::Object | ParameterKind::TypeParameter, _) => Some(1),
        _ => None,
    }
}

/// 引数にいちばん合うオーバーロードの番号。どれにも渡せなければ `None`。同点なら先のもの（派生した型の方）。
fn best_overload(overloads: &[Vec<ParameterKind>], arguments: &[ArgumentKind]) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (index, parameters) in overloads.iter().enumerate() {
        if parameters.len() != arguments.len() {
            continue;
        }
        let Some(total) = parameters.iter().zip(arguments).map(|(&p, &a)| fit(p, a)).sum::<Option<u32>>() else { continue };
        if best.is_none_or(|(_, score)| total > score) {
            best = Some((index, total));
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overload_whose_parameter_types_match_exactly_wins() {
        use ParameterKind::*;
        // Math.Max(sbyte, sbyte)、(byte, byte)、(int, int)、(long, long)、(double, double) に int を 2 つ渡す。
        let overloads = vec![vec![Primitive(0x04); 2], vec![Primitive(0x05); 2], vec![Primitive(0x08); 2], vec![Primitive(0x0A); 2], vec![Primitive(0x0D); 2]];
        assert_eq!(best_overload(&overloads, &[ArgumentKind::Primitive(0x08); 2]), Some(2));
    }

    #[test]
    fn an_int_is_widened_when_no_overload_takes_int() {
        use ParameterKind::*;
        let overloads = vec![vec![Primitive(0x05)], vec![Primitive(0x0D)], vec![Primitive(0x0A)]];
        assert_eq!(best_overload(&overloads, &[ArgumentKind::Primitive(0x08)]), Some(1), "byte には渡せない。double と long は同点で、先のもの");
        assert_eq!(best_overload(&overloads[..1], &[ArgumentKind::Primitive(0x08)]), None);
    }

    #[test]
    fn a_string_parameter_is_preferred_over_object_for_a_string() {
        use ParameterKind::*;
        let overloads = vec![vec![Object], vec![String], vec![Primitive(0x08)]];
        assert_eq!(best_overload(&overloads, &[ArgumentKind::String]), Some(1));
        assert_eq!(best_overload(&overloads, &[ArgumentKind::Reference]), Some(0));
        assert_eq!(best_overload(&overloads, &[ArgumentKind::Primitive(0x08)]), Some(2));
        assert_eq!(best_overload(&overloads, &[ArgumentKind::Null]), Some(1));
    }

    #[test]
    fn ref_parameters_and_wrong_counts_are_never_chosen() {
        use ParameterKind::*;
        assert_eq!(best_overload(&[vec![Unsupported]], &[ArgumentKind::Primitive(0x08)]), None);
        assert_eq!(best_overload(&[vec![Object, Object]], &[ArgumentKind::Null]), None);
        assert_eq!(best_overload(&[vec![]], &[]), Some(0));
    }
}
