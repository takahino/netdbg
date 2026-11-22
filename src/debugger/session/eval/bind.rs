//! メソッドの呼び出しの「広げた形」での結び付け: 省略できる引数、名前つき引数、`params`、型引数つきのメソッド
//! （型引数を書いた呼び出しと、引数からの推論）、拡張メソッド。
//!
//! 引数の数と型がそのまま合うオーバーロードは、先に `call_method` が選ぶ。ここは、それで見つからなかったときに、
//! メソッドの「呼び出しの形」（引数の名前、既定値、`params`、型引数）を見て、渡す引数の並びを作り直す。

use super::*;
use crate::debugger::metadata::{GenericUse, MethodShape};
use crate::debugger::session::eval::syntax::{keyword_type, parse_reflection_type, parse_type, LambdaParameters, OutTarget};

/// 呼び出しに付いている、位置の引数以外のもの。
#[derive(Default, Clone)]
pub(super) struct CallExtra {
    /// 名前つき引数（`times: 2`）。
    pub named: Vec<(String, ComPtr<ICorDebugValue>)>,
    /// 書かれた型引数（`Echo<int>(a)` の `int`）。
    pub type_arguments: Vec<TypeSyntax>,
    /// `out` / `ref` の引数（位置の引数の中での位置と、受け先）。
    pub outs: Vec<(usize, OutTarget)>,
    /// ラムダ式の引数（位置の引数の中での位置と、引数の並び、本体）。デリゲートの型は、渡す先の引数の型で決まる。
    pub lambdas: Vec<(usize, LambdaParameters, Expr)>,
    /// 型を引数の型から決める式（`new()`、コレクション式、型を書かない `default`）。位置と式。
    pub targeted: Vec<(usize, Expr)>,
    /// メソッドの集まり（`Doubled`、`string.IsNullOrEmpty`）の引数。位置と、メソッドを指す式。渡す先のデリゲートの型で
    /// ラムダ式に直してから、ラムダ式と同じく作る（`method_group.rs`）。
    pub groups: Vec<(usize, Expr)>,
}

impl CallExtra {
    /// 位置の引数だけの、ふつうの呼び出しか。
    pub fn is_plain(&self) -> bool {
        self.named.is_empty() && self.type_arguments.is_empty() && !self.has_lambdas() && self.targeted.is_empty()
    }

    /// デリゲートを渡す引数（ラムダ式、メソッドの集まり）があるか。
    pub fn has_lambdas(&self) -> bool {
        !self.lambdas.is_empty() || !self.groups.is_empty()
    }
}

impl Session {
    /// 型（と、同じモジュールの中の基底の型）が宣言している、その名前のメソッドを、広げた形で結び付ける。
    /// 戻り値は、呼ぶメソッドと、渡す引数の並び（`this` は含まない）。当てはまるものが無ければ `None`。
    #[allow(clippy::too_many_arguments, reason = "式の評価の関数は (thread, depth) を先頭に取る約束。束ねると評価の全部のシグネチャが変わる (2026-11-18 の整理で見送った)")]
    pub(super) fn bind_expanded(
        &mut self,
        thread: u32,
        depth: usize,
        owner: (usize, u32),
        name: &str,
        is_static: bool,
        class_arguments: &[ComPtr<ICorDebugType>],
        positional: &[ComPtr<ICorDebugValue>],
        extra: &CallExtra,
    ) -> Option<(Callable, Vec<ComPtr<ICorDebugValue>>)> {
        let (module, mut row) = owner;
        for _ in 0..16 {
            let shapes: Vec<MethodShape> = self.modules[module].metadata()?.methods_named(row, name).into_iter().filter(|shape| shape.is_static == is_static).collect();
            if let Some(bound) = self.bind_shapes(thread, depth, module, shapes, class_arguments, positional, extra) {
                return Some(bound);
            }
            row = self.modules[module].metadata()?.base_type(row)?;
        }
        None
    }

    /// 拡張メソッドを探して結び付ける。見るのは、いまのメソッドのコードから見える名前空間（`using` と、自分の名前空間）
    /// にある型の拡張メソッド。戻り値の引数の並びは、受け手（`this` の引数）を先頭に含む。
    pub(super) fn bind_extension(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, name: &str, positional: &[ComPtr<ICorDebugValue>], extra: &CallExtra) -> Option<(Callable, Vec<ComPtr<ICorDebugValue>>)> {
        // 名前空間は、型の名前の候補の作り方を借りて得る（「名前空間.?」の形で返ってくる）。
        let visible: Vec<String> = self.type_name_candidates(thread, depth, "?").into_iter().map(|candidate| candidate.trim_end_matches('?').trim_end_matches('.').to_string()).collect();
        // LINQ を使わないプログラム（F# に多い）では、System.Linq がまだ読み込まれていない。
        if visible.iter().any(|namespace| namespace == "System.Linq") {
            self.system_core_type(thread, depth, "System.Linq.Enumerable", "System.Linq");
        }
        let mut arguments = vec![receiver.clone()];
        arguments.extend(positional.iter().cloned());
        // 受け手が先頭に入るので、ラムダの位置を一つずらす。
        let mut extra = extra.clone();
        for lambda in extra.lambdas.iter_mut() {
            lambda.0 += 1;
        }
        for targeted in extra.targeted.iter_mut() {
            targeted.0 += 1;
        }
        for group in extra.groups.iter_mut() {
            group.0 += 1;
        }
        let extra = &extra;
        for module in 0..self.modules.len() {
            let shapes: Vec<MethodShape> = match self.modules[module].metadata() {
                Some(metadata) => metadata.extension_methods(name).into_iter().filter(|(_, namespace)| visible.iter().any(|v| v == namespace)).map(|(shape, _)| shape).collect(),
                None => continue,
            };
            if shapes.is_empty() {
                continue;
            }
            if let Some(bound) = self.bind_shapes(thread, depth, module, shapes, &[], &arguments, extra) {
                return Some(bound);
            }
        }
        None
    }

    /// 候補のメソッドを順に試して、最初に結び付けられたものを返す。ラムダ式を渡す呼び出しは、本体の型がデリゲートの
    /// 戻り値の型とそのまま合うものを先に選ぶ（`Sum(Func<T, int>)` より `Sum(Func<T, decimal>)`）。無ければ、変換を
    /// 挟んで渡せるもの。
    #[allow(clippy::too_many_arguments, reason = "式の評価の関数は (thread, depth) を先頭に取る約束。束ねると評価の全部のシグネチャが変わる (2026-11-18 の整理で見送った)")]
    fn bind_shapes(
        &mut self,
        thread: u32,
        depth: usize,
        module: usize,
        shapes: Vec<MethodShape>,
        class_arguments: &[ComPtr<ICorDebugType>],
        positional: &[ComPtr<ICorDebugValue>],
        extra: &CallExtra,
    ) -> Option<(Callable, Vec<ComPtr<ICorDebugValue>>)> {
        let passes: &[bool] = if extra.has_lambdas() { &[true, false] } else { &[false] };
        for &exact in passes {
            for shape in &shapes {
                if let Some(bound) = self.bind_shape(thread, depth, module, shape, class_arguments, positional, extra, exact) {
                    return Some(bound);
                }
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments, reason = "式の評価の関数は (thread, depth) を先頭に取る約束。束ねると評価の全部のシグネチャが変わる (2026-11-18 の整理で見送った)")]
    fn bind_shape(
        &mut self,
        thread: u32,
        depth: usize,
        module: usize,
        shape: &MethodShape,
        class_arguments: &[ComPtr<ICorDebugType>],
        positional: &[ComPtr<ICorDebugValue>],
        extra: &CallExtra,
        exact: bool,
    ) -> Option<(Callable, Vec<ComPtr<ICorDebugValue>>)> {
        let count = shape.parameters.len();
        let has_params = shape.parameters.last().is_some_and(|parameter| parameter.is_params);
        let fixed = if has_params { count - 1 } else { count };
        if positional.len() > fixed && !has_params {
            return None;
        }
        // 位置の引数、名前つき引数の順に、引数の場所を埋める。
        let mut slots: Vec<Option<ComPtr<ICorDebugValue>>> = vec![None; count];
        let direct = positional.len().min(fixed);
        for (slot, value) in slots.iter_mut().zip(&positional[..direct]) {
            *slot = Some(value.clone());
        }
        let mut extras: Vec<ComPtr<ICorDebugValue>> = positional[direct..].to_vec();
        // `params` の場所に配列を 1 つだけ渡しているなら、そのまま渡す（ふつうの形）。
        if has_params && extras.len() == 1 && unsafe { value::array_length(&extras[0]) }.is_some() {
            slots[count - 1] = extras.pop();
        }
        for (name, value) in &extra.named {
            let index = shape.parameters.iter().position(|parameter| parameter.name == *name)?;
            if slots[index].is_some() {
                return None;
            }
            slots[index] = Some(value.clone());
        }

        // メソッドの型引数: 書かれていればそれ、無ければ引数の型から決める。ラムダ式の戻り値で決まるもの（Select の
        // TResult）は、ラムダ式をコンパイルしてから決める。
        let class_names: Vec<String> = class_arguments.iter().map(|ty| unsafe { value::type_name(ty, &self.modules) }).collect();
        let mut inferred: Vec<Option<ComPtr<ICorDebugType>>> = Vec::new();
        if !extra.type_arguments.is_empty() {
            if extra.type_arguments.len() != shape.type_parameters as usize {
                return None;
            }
            for ty in &extra.type_arguments {
                inferred.push(Some(self.runtime_type(thread, depth, ty).ok()?));
            }
        } else {
            let placeholders: Vec<String> = (0..shape.type_parameters).map(|number| format!("!!{number}")).collect();
            let template = self.modules[module].metadata()?.parameter_types(shape.token, &class_names, &placeholders)?;
            for number in 0..shape.type_parameters {
                let found = self.infer(thread, shape, &template, number, &slots, &extras);
                inferred.push(found);
            }
        }
        // ラムダ式とメソッドの集まりの引数: 渡す先のデリゲートの型を決めて、コンパイルする。メソッドの集まりは、デリゲートの
        // 引数の数が分かってから、ラムダ式に直す。
        let mut pending: Vec<(usize, Option<LambdaParameters>, Expr)> = extra.lambdas.iter().map(|(position, parameters, body)| (*position, Some(parameters.clone()), body.clone())).collect();
        pending.extend(extra.groups.iter().map(|(position, method)| (*position, None, method.clone())));
        for (position, parameters, body) in &pending {
            let delegate_of = |session: &mut Session, inferred: &[Option<ComPtr<ICorDebugType>>]| -> Option<crate::debugger::metadata::TypeShape> {
                let names: Vec<String> = inferred.iter().enumerate().map(|(number, ty)| ty.as_ref().map_or(format!("!!{number}"), |ty| unsafe { value::type_name(ty, &session.modules) })).collect();
                let shapes = session.modules[module].metadata()?.parameter_types(shape.token, &class_names, &names)?;
                // 引数の型が、型の型引数そのもの（`List<Func<int, int>>.Add(T)` の T）のときは、名前の中に型引数が
                // 入った形で返ってくる。名前と型引数に分け直す。
                shapes.get(*position)?.clone().map(lambda::split_shape)
            };
            let delegate = delegate_of(self, &inferred)?;
            let (inputs, output) = self.delegate_signature_of(thread, depth, &delegate)?;
            let (parameters, body) = match parameters {
                Some(parameters) => (parameters.clone(), body.clone()),
                None => method_group::as_lambda(body, inputs.len()),
            };
            let (parameters, body) = (&parameters, &body);
            if inputs.len() != parameters.len() || inputs.iter().any(|input| input.contains("!!")) {
                return None;
            }
            let open_output = output.as_deref().and_then(lambda::placeholder);
            let nested_output = output.as_deref().filter(|output| open_output.is_none() && output.contains("!!"));
            let typed = if open_output.is_some() || nested_output.is_some() { None } else { Some(&delegate) };
            let (compiled, body_type) = match self.compile_lambda(thread, depth, parameters, body, &inputs, typed, exact) {
                Ok(compiled) => compiled,
                Err(reason) => {
                    // このオーバーロードには渡せない。どれにも渡せなかったときのために、最初の理由を覚えておく
                    // (型がそのまま合うものを探す周の理由は、変換を挟む周で渡せることがあるので覚えない)。
                    if !exact {
                        self.lambda_failure.get_or_insert(reason);
                    }
                    return None;
                }
            };
            if let Some(number) = open_output {
                let ty = parse_reflection_type(&body_type).or_else(|| parse_type(&body_type))?;
                *inferred.get_mut(number)? = Some(self.runtime_type(thread, depth, &ty).ok()?);
            }
            // 型を決めずにコンパイルしたデリゲートは `Func<…>` になる。渡す先が別のデリゲートの型（`Converter<T, TOutput>`、
            // 利用者が定義したもの）なら、決まった型でコンパイルし直す。
            let compiled = if typed.is_none() && nested_output.is_none() && !matches!(delegate.name.as_str(), "System.Func" | "System.Action") {
                let delegate = delegate_of(self, &inferred)?;
                self.compile_lambda(thread, depth, parameters, body, &inputs, Some(&delegate), false).ok()?.0
            } else {
                compiled
            };
            // 戻り値の型の中に型引数があるもの（SelectMany の `IEnumerable<TCollection>`）: 本体の型を、その型として見たときの
            // 型引数から決めて、決まったデリゲートの型でコンパイルし直す。
            let compiled = match nested_output {
                Some(output) => {
                    let (generic, number, at, arity) = lambda::nested_placeholder(output)?;
                    let body_type = self.lambda_return_type(thread, &compiled)?;
                    let found = self.generic_argument_of_type(thread, &body_type, &generic, arity, at)?;
                    *inferred.get_mut(number)? = Some(found);
                    let delegate = delegate_of(self, &inferred)?;
                    let (inputs, _) = self.delegate_signature_of(thread, depth, &delegate)?;
                    self.compile_lambda(thread, depth, parameters, body, &inputs, Some(&delegate), false).ok()?.0
                }
                None => compiled,
            };
            *slots.get_mut(*position)? = Some(compiled);
        }
        let method_arguments: Vec<ComPtr<ICorDebugType>> = inferred.into_iter().collect::<Option<_>>()?;
        // 渡されなかった引数: `params` なら残りを配列にまとめ、省略できる引数なら既定値。
        let shapes = {
            let metadata = self.modules[module].metadata()?;
            let class_names: Vec<String> = class_arguments.iter().map(|ty| unsafe { value::type_name(ty, &self.modules) }).collect();
            let method_names: Vec<String> = method_arguments.iter().map(|ty| unsafe { value::type_name(ty, &self.modules) }).collect();
            metadata.parameter_types(shape.token, &class_names, &method_names)?
        };
        // 型を引数の型から決める式（`new()`、`[1, 2]`、`default`）を、その型で作る。
        for (position, expr) in &extra.targeted {
            let target = shapes.get(*position)?.as_ref().and_then(|shape| parse_type(&shape.display()))?;
            *slots.get_mut(*position)? = Some(self.build_targeted(thread, depth, expr, &target)?.ok()?);
        }
        for (index, slot) in slots.iter_mut().enumerate().take(count) {
            if slot.is_some() {
                continue;
            }
            let parameter = &shape.parameters[index];
            *slot = Some(if parameter.is_params && index == count - 1 {
                let element = match &parameter.generic {
                    GenericUse::ArrayOf(number) => method_arguments.get(*number as usize)?.clone(),
                    _ => {
                        let name = shapes.get(index)?.as_ref()?.name.strip_suffix("[]")?.to_string();
                        let ty = TypeSyntax::named(keyword_type(&name).unwrap_or(&name));
                        self.runtime_type(thread, depth, &ty).ok()?
                    }
                };
                let array = self.new_array(thread, &element, &[extras.len() as u32]).ok()?;
                for (position, value) in extras.iter().enumerate() {
                    let slot = unsafe { value::array_element(&array, &[position as u32]) }.ok()??;
                    self.write(thread, &slot, value).ok()?;
                }
                extras.clear();
                array
            } else {
                let (element, bytes) = parameter.default.as_ref().filter(|_| parameter.optional)?;
                let value = self.default_value(thread, *element, bytes)?;
                // 列挙型の仮引数の既定値は、メタデータには、もとになる整数の型で入っている。列挙型の値にして渡す
                // （`int.Parse(span, NumberStyles style = Integer, …)`）。
                let declared = shapes.get(index).cloned().flatten().and_then(|shape| parse_type(&shape.display()));
                match declared {
                    Some(ty) if matches!(self.resolve_type(thread, depth, &ty), Ok(ResolvedType::Named { kind: TypeKind::Enum, .. })) => self.cast(thread, depth, &ty, value).ok()?,
                    _ => value,
                }
            });
        }
        if !extras.is_empty() {
            return None;
        }
        let mut arguments: Vec<ComPtr<ICorDebugValue>> = slots.into_iter().collect::<Option<_>>()?;
        // Span への暗黙の変換（C# 14）: `ReadOnlySpan<T>` / `Span<T>` の仮引数に渡す文字列と配列は、その Span にする。
        for (index, argument) in arguments.iter_mut().enumerate() {
            let Some(Some(target)) = shapes.get(index) else { continue };
            if let Some(span) = self.span_of(thread, depth, argument, target) {
                *argument = span;
            }
        }

        let (function, kinds) = unsafe {
            let mut raw = null_mut();
            self.modules[module].module.GetFunctionFromToken(shape.token, &mut raw);
            (ComPtr::<ICorDebugFunction>::from_raw(raw)?, self.modules[module].metadata()?.parameter_kinds(shape.token)?)
        };
        let mut type_arguments = class_arguments.to_vec();
        type_arguments.extend(method_arguments);
        let callable = self.choose_overload(thread, vec![(Callable { function, type_arguments }, kinds)], &mut arguments)?;
        Some((callable, arguments))
    }

    /// `ReadOnlySpan<T>` / `Span<T>`（`target`）の仮引数に渡す文字列か配列を、その Span の値にする。ほかは `None`。
    /// 文字列は `ReadOnlySpan<char>` にだけ、配列は、要素の型が同じ Span にだけなる。
    fn span_of(&mut self, thread: u32, depth: usize, argument: &ComPtr<ICorDebugValue>, target: &crate::debugger::metadata::TypeShape) -> Option<ComPtr<ICorDebugValue>> {
        let read_only = match target.name.as_str() {
            "System.ReadOnlySpan" => true,
            "System.Span" => false,
            _ => return None,
        };
        let element = target.arguments.first()?;
        let kind = unsafe { value::argument_kind(argument) };
        if kind == ArgumentKind::String {
            if !read_only || !calls::same_type(element, "char") {
                return None;
            }
            let (module, row) = self.find_type(thread, depth, "System.MemoryExtensions")?;
            return self.call_method(thread, depth, Some(Current::Type { module, row }), "AsSpan", vec![argument.clone()]).ok()?;
        }
        unsafe { value::array_length(argument) }?;
        let array_element = unsafe { value::element_or_argument_types(argument) }.into_iter().next()?;
        if !calls::same_type(&unsafe { value::type_name(&array_element, &self.modules) }, element) {
            return None;
        }
        // `new ReadOnlySpan<T>(T[])` / `new Span<T>(T[])`。
        let span = syntax::parse_type(&target.display())?;
        let (module, row) = self.find_type_with_arity(thread, depth, &span.name, 1)?;
        let mut candidates = self.type_method_candidates(module, row, ".ctor", false, 1);
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = vec![array_element.clone()];
        }
        let mut values = vec![argument.clone()];
        let constructor = self.choose_overload(thread, candidates, &mut values)?;
        self.new_object(thread, &constructor, &values).ok()
    }

    /// メソッドの型引数を、渡された引数の型から決める。決められなければ `None`。`template` は、メソッドの型引数を
    /// `!!番号` にした、引数の型。
    fn infer(&mut self, thread: u32, shape: &MethodShape, template: &[Option<crate::debugger::metadata::TypeShape>], number: u32, slots: &[Option<ComPtr<ICorDebugValue>>], extras: &[ComPtr<ICorDebugValue>]) -> Option<ComPtr<ICorDebugType>> {
        for (index, (parameter, slot)) in shape.parameters.iter().zip(slots).enumerate() {
            let found = match (&parameter.generic, slot) {
                (GenericUse::Direct(n), Some(value)) if *n == number => unsafe { value::exact_type_of(value) },
                // `params T[]`: 配列を渡していれば要素の型、ばらで渡していれば最初の値の型。
                (GenericUse::ArrayOf(n), Some(value)) if *n == number => unsafe { value::element_or_argument_types(value) }.into_iter().next(),
                (GenericUse::ArrayOf(n), None) if *n == number && parameter.is_params => extras.first().and_then(|value| unsafe { value::exact_type_of(value) }),
                (GenericUse::Inside(positions), Some(value)) => match positions.iter().position(|p| *p == Some(number)) {
                    // Span への暗黙の変換（C# 14）: `ReadOnlySpan<T>` に文字列なら T は char、`Span<T>` / `ReadOnlySpan<T>` に
                    // 配列なら、T は要素の型。
                    Some(0) if template.get(index).and_then(Option::as_ref).is_some_and(|shape| matches!(shape.name.as_str(), "System.ReadOnlySpan" | "System.Span")) && self.span_element(thread, value).is_some() => self.span_element(thread, value),
                    Some(at) => self.generic_argument_of(thread, value, template.get(index).and_then(Option::as_ref), positions.len(), at),
                    None => None,
                },
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// 文字列か配列を Span として見たときの要素の型（文字列は char）。ほかは `None`。
    fn span_element(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugType>> {
        if unsafe { value::argument_kind(value) } == ArgumentKind::String {
            let sample = self.new_primitive(thread, 0x03, &[0, 0]).ok()?;
            return unsafe { value::exact_type_of(&sample) };
        }
        unsafe { value::array_length(value) }?;
        unsafe { value::element_or_argument_types(value) }.into_iter().next()
    }

    /// 値を、ジェネリック型 `generic`（`IEnumerable<T>` など）として見たときの、`at` 番目の型引数。
    /// 値の型そのものがその型（`List<int>` を `List<T>` に）か配列なら、値の型から読む。そうでなければ、値の型が実装している
    /// インターフェイスを、対象の中のリフレクションで調べる（`Where` の結果の型を `IEnumerable<T>` に、など）。
    pub(super) fn generic_argument_of(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>, generic: Option<&crate::debugger::metadata::TypeShape>, arity: usize, at: usize) -> Option<ComPtr<ICorDebugType>> {
        let own = unsafe { value::type_names(value, &self.modules) }.into_iter().next();
        let is_array = unsafe { value::array_length(value) }.is_some();
        let types = unsafe { value::element_or_argument_types(value) };
        if (is_array || own.as_deref() == generic.map(|shape| shape.name.as_str())) && types.len() == arity {
            return types.into_iter().nth(at);
        }
        let generic = generic?;
        let simple = format!("{}`{arity}", generic.name.rsplit('.').next()?);
        let ty = self.call_method(thread, 0, Some(Current::Value(value.clone())), "GetType", Vec::new()).ok()??;
        let name = self.new_string(thread, &simple).ok()?;
        let interface = self.call_method(thread, 0, Some(Current::Value(ty)), "GetInterface", vec![name]).ok()??;
        if unsafe { value::argument_kind(&interface) } == ArgumentKind::Null {
            return None;
        }
        let arguments = self.call_method(thread, 0, Some(Current::Value(interface)), "GetGenericArguments", Vec::new()).ok()??;
        let index = self.new_int(thread, at as i32).ok()?;
        let argument = self.index(thread, &arguments, &[index]).ok()?;
        let full = self.member(thread, &argument, "FullName").ok()?;
        let full = unsafe { value::as_string(&full) }?;
        self.runtime_type(thread, 0, &parse_reflection_type(&full).or_else(|| parse_type(&full))?).ok()
    }
    /// コンパイルしたデリゲートの戻り値の型（対象の中の `System.Type`）。
    fn lambda_return_type(&mut self, thread: u32, compiled: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        let method = self.member(thread, compiled, "Method").ok()?;
        self.member(thread, &method, "ReturnType").ok()
    }

    /// 型（対象の中の `System.Type`）を、ジェネリック型 `generic`（`System.Collections.Generic.IEnumerable` など）として
    /// 見たときの、`at` 番目の型引数。型そのものがその型ならその型引数、そうでなければ実装しているインターフェイスから。
    pub(super) fn generic_argument_of_type(&mut self, thread: u32, ty: &ComPtr<ICorDebugValue>, generic: &str, arity: usize, at: usize) -> Option<ComPtr<ICorDebugType>> {
        let definition = format!("{generic}`{arity}");
        // 組み立てたジェネリック型の FullName は ``名前`数[[型引数, …]]``。
        let own = self.member(thread, ty, "FullName").ok().and_then(|name| unsafe { value::as_string(&name) }).is_some_and(|name| name.starts_with(&format!("{definition}[[")));
        let matched = if own {
            ty.clone()
        } else {
            let simple = definition.rsplit('.').next()?.to_string();
            let name = self.new_string(thread, &simple).ok()?;
            let interface = self.call_method(thread, 0, Some(Current::Value(ty.clone())), "GetInterface", vec![name]).ok()??;
            if unsafe { value::argument_kind(&interface) } == ArgumentKind::Null {
                return None;
            }
            interface
        };
        let arguments = self.call_method(thread, 0, Some(Current::Value(matched)), "GetGenericArguments", Vec::new()).ok()??;
        let index = self.new_int(thread, at as i32).ok()?;
        let argument = self.index(thread, &arguments, &[index]).ok()?;
        let full = self.member(thread, &argument, "FullName").ok()?;
        let full = unsafe { value::as_string(&full) }?;
        self.runtime_type(thread, 0, &parse_reflection_type(&full).or_else(|| parse_type(&full))?).ok()
    }

    /// 省略された引数の既定値を、対象の中の値にする。
    pub(super) fn default_value(&mut self, thread: u32, element: u8, bytes: &[u8]) -> Option<ComPtr<ICorDebugValue>> {
        match element {
            0x0E => {
                let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                self.new_string(thread, &String::from_utf16_lossy(&units)).ok()
            }
            // 参照型の既定値は null。
            0x12 => self.new_null(thread).ok(),
            element => self.new_number(thread, element, bytes).ok(),
        }
    }
}
