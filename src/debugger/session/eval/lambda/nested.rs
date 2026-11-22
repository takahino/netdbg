//! ラムダ式の中の呼び出し: 呼ぶ相手の決め方と、引数にラムダ式を渡す呼び出し（`xs.Select(x => ys.Count(y => y > x))`）。
//!
//! 内側のラムダ式は、外側の引数を使えるように、同じ式木の中の `Expression.Lambda` の節にする。渡す先のメソッドと、
//! 内側のラムダ式の引数の型は、メソッドのシグネチャ（メタデータ）と、ほかの引数の節の型から決める。式を直に評価する
//! ときの結び付け（bind.rs）と同じ考え方を、値ではなく節の型で行う。
//! - 候補は、対象の型（と基底の型）のインスタンスメソッドが先で、無ければ、見える名前空間の拡張メソッド。
//! - メソッドの型引数は、ほかの引数の型から（`IEnumerable<T>` に `List<int>` なら int）、それで決まらないもの（Select の
//!   TResult）は、ラムダ式の本体の型から決める。
//! - 戻り値の型が決まったデリゲート（`Sum(Func<T, int>)` と `Sum(Func<T, long>)` など）は、本体の型がそのまま合うものを
//!   先に選ぶ。合うものが無ければ、変換を挟んで渡せるもの。
//! - 名前つき引数は仮引数の名前で並べ、渡さなかった省略できる引数は既定値、`params` は空の配列にする。型を渡す先から
//!   決める式（`new()`、`[…]`、`default`）は、仮引数の型で作る。

use super::*;
use crate::debugger::metadata::MethodShape;
use crate::debugger::session::eval::syntax::{Initializer, OutTarget};

type Node = ComPtr<ICorDebugValue>;
type Scope = [(String, Node)];

/// ラムダ式の中の呼び出しの相手。
#[derive(Clone)]
pub(in crate::debugger::session) enum CallTarget {
    /// インスタンスメソッド（対象の節と、メソッドの名前）。
    Instance(Node, String),
    /// 型の静的メソッド。`owner` は、メタデータの中の型。
    Static { type_object: Node, owner: (usize, u32), name: String },
    /// デリゲートの値の呼び出し（`Invoke`）。
    Invoke(Node),
    /// コンストラクター（`new 型(…)`）。`class_names` は、型の型引数。
    Constructor { type_object: Node, owner: (usize, u32), class_names: Vec<String> },
}

/// 呼び出しの引数の一つ。
struct Argument<'a> {
    /// 名前つき引数の名前。
    label: Option<&'a str>,
    /// 書かれた式（名前を外したもの）。
    expr: &'a Expr,
    /// 渡す先の型によらずに作れる節。ラムダ式と、型を渡す先から決める式は `None`。
    node: Option<Node>,
    /// メソッドの集まり（`char.ToUpper`）か。
    group: bool,
}

/// 呼ぶ候補のメソッド。
struct Candidate {
    module: usize,
    shape: MethodShape,
    /// メソッドを持つ型の型引数（`List<int>` の int）。
    class_names: Vec<String>,
    /// 拡張メソッドとして呼ぶ（対象を最初の引数にする）か。
    extension: bool,
}

impl Session {
    /// 呼び出しの `メソッド(…)` の部分から、呼ぶ相手を決める。`used` は、その部分がラムダの引数を使っているか。
    /// 名前で指したメソッドでも、デリゲートでもなければ `None`。
    pub(super) fn call_target(&mut self, thread: u32, depth: usize, method: &Expr, used: bool, argument_count: usize, scope: &Scope) -> Result<Option<CallTarget>, String> {
        let target = self.method_target(thread, depth, method, used, argument_count, scope)?;
        // フィールドやプロパティに入ったデリゲートの呼び出し（`holder.Step(1)`）は、その値の Invoke。
        if let Some(CallTarget::Instance(node, name)) = &target {
            if let Some(delegate) = self.delegate_member_node(thread, depth, node, name)? {
                return Ok(Some(CallTarget::Invoke(delegate)));
            }
        }
        Ok(target)
    }

    /// 対象の型にその名前のメソッドが無く、その名前のフィールドかプロパティがデリゲートなら、その節。
    fn delegate_member_node(&mut self, thread: u32, depth: usize, node: &Node, name: &str) -> Result<Option<Node>, String> {
        let Some((module, row, _)) = self.node_class(thread, depth, node) else { return Ok(None) };
        let declares = self.modules[module].metadata().is_some_and(|metadata| {
            let mut row = row;
            for _ in 0..16 {
                if !metadata.methods_named(row, name).is_empty() {
                    return true;
                }
                match metadata.base_type(row) {
                    Some(base) => row = base,
                    None => return false,
                }
            }
            false
        });
        if declares {
            return Ok(None);
        }
        let Ok(member) = self.member_node(thread, depth, node, name) else { return Ok(None) };
        let ty = self.member(thread, &member, "Type")?;
        let delegate = self.type_object(thread, depth, "System.Delegate")?;
        Ok(self.assignable_from(thread, depth, &delegate, &ty)?.then_some(member))
    }

    fn method_target(&mut self, thread: u32, depth: usize, method: &Expr, used: bool, argument_count: usize, scope: &Scope) -> Result<Option<CallTarget>, String> {
        Ok(Some(match method {
            // 型のメソッド（`Math.Abs(n)`）か、値のメソッド（`w.StartsWith("a")`）。対象は、何度評価しても同じもの（変数、
            // 型の名前）だけを、いま評価する。呼び出しや new は、デリゲートを呼ぶたびに実行する節にする（下の腕）。
            Expr::Member(target, name) if !used && (repeatable(target) || matches!(**target, Expr::Generic(..))) => match self.evaluate_node(thread, depth, target)? {
                Current::Type { module, row } => {
                    let type_name = self.plain_type_name(module, row);
                    CallTarget::Static { type_object: self.type_object(thread, depth, &type_name)?, owner: (module, row), name: name.clone() }
                }
                other => CallTarget::Instance(self.constant_of(thread, depth, other)?, name.clone()),
            },
            Expr::Member(target, name) => CallTarget::Instance(self.tree(thread, depth, target, scope)?, name.clone()),
            // ラムダの引数や変数に入ったデリゲート（`f(x)`）。
            Expr::Name(name) if self.lambda_variable(scope, name).is_some() => CallTarget::Invoke(self.lambda_variable(scope, name).ok_or_else(|| tr!("The variable is not found."))?),
            Expr::Name(name) => {
                // 止まっている場所の変数に入ったデリゲート。
                if let Some((_, delegate)) = self.frame_variables(thread, depth)?.into_iter().find(|(existing, _)| existing == name) {
                    return Ok(Some(CallTarget::Invoke(self.constant_of(thread, depth, Current::Value(delegate))?)));
                }
                // 型の名前を省いた呼び出し（`Run(x)`）: いまのメソッドの型と、それを囲む型の静的メソッドか、this のメソッド。
                let owners = self.frame_types(thread, depth);
                if owners.is_empty() {
                    return Err(tr!("The type of the current method is not known."));
                }
                for (module, row) in owners {
                    let has_static = !self.type_method_candidates(module, row, name, true, argument_count as u32).is_empty() || self.modules[module].metadata().is_some_and(|metadata| metadata.methods_named(row, name).iter().any(|shape| shape.is_static));
                    if has_static {
                        let type_name = self.plain_type_name(module, row);
                        return Ok(Some(CallTarget::Static { type_object: self.type_object(thread, depth, &type_name)?, owner: (module, row), name: name.clone() }));
                    }
                }
                let this = self.resolve_name(thread, depth, "this")?.ok_or_else(|| tr!("Method '{name}' is not found.", name = name))?;
                CallTarget::Instance(self.factory(thread, depth, "Constant", vec![this])?, name.clone())
            }
            // デリゲートの値の呼び出し（`((Func<int, int>)p)(x)`）。
            other if used => CallTarget::Invoke(self.tree(thread, depth, other, scope)?),
            _ => return Ok(None),
        }))
    }

    /// 引数にラムダ式、名前つき引数、型を渡す先から決める式（`new()`、`[…]`、`default`）がある呼び出しの節。
    /// 渡す先のメソッドを、シグネチャから選ぶ。
    pub(super) fn call_by_signature_node(&mut self, thread: u32, depth: usize, target: CallTarget, written: &[TypeSyntax], arguments: &[Expr], scope: &Scope) -> Result<Node, String> {
        let (receiver, name) = match &target {
            CallTarget::Instance(node, name) => (Some(node.clone()), name.clone()),
            CallTarget::Static { name, .. } => (None, name.clone()),
            CallTarget::Constructor { .. } => (None, ".ctor".to_string()),
            CallTarget::Invoke(_) => return Err(tr!("Passing lambdas or named arguments to a call of a delegate value inside a lambda is not supported yet.")),
        };
        // 引数の節。ラムダ式と、型を渡す先から決める式は、渡す先が決まってから作る。
        let mut written_arguments = Vec::new();
        for argument in arguments {
            let (label, expr) = match argument {
                Expr::Named(label, value) => (Some(label.as_str()), &**value),
                other => (None, other),
            };
            let mut group = false;
            let node = match expr {
                Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral | Expr::Out(_) => None,
                other => match self.tree(thread, depth, other, scope) {
                    Ok(node) => Some(node),
                    // 値にならない名前がメソッドを指していれば、メソッドの集まり（渡す先のデリゲートの型で、ラムダ式に直す）。
                    Err(error) => {
                        if !self.is_method_group(thread, depth, other) {
                            return Err(error);
                        }
                        group = true;
                        None
                    }
                },
            };
            written_arguments.push(Argument { label, expr, node, group });
        }
        let candidates = match &target {
            CallTarget::Instance(node, _) => {
                let mut found = self.instance_candidates(thread, depth, node, &name);
                found.extend(self.extension_candidates(thread, depth, &name));
                found
            }
            CallTarget::Static { owner, .. } => self.static_candidates(*owner, &name),
            CallTarget::Constructor { owner: (module, row), class_names, .. } => {
                let shapes = self.modules[*module].metadata().map(|metadata| metadata.methods_named(*row, ".ctor")).unwrap_or_default();
                shapes.into_iter().filter(|shape| !shape.is_static).map(|shape| Candidate { module: *module, shape, class_names: class_names.clone(), extension: false }).collect()
            }
            CallTarget::Invoke(_) => Vec::new(),
        };
        // 本体の型がそのまま合うものを先に、無ければ、変換を挟んで渡せるもの。
        let mut failure = None;
        for exact in [true, false] {
            for candidate in &candidates {
                match self.try_candidate(thread, depth, candidate, receiver.as_ref(), &target, &name, written, &written_arguments, scope, exact) {
                    Ok(Some(node)) => return Ok(node),
                    Ok(None) => {}
                    Err(reason) => {
                        failure.get_or_insert(reason);
                    }
                }
            }
        }
        Err(failure.unwrap_or_else(|| tr!("No method '{name}' that takes these arguments was found.", name = name)))
    }
    /// 候補のメソッドへ、引数を渡す呼び出しの節を作る。渡せなければ `Ok(None)`、ラムダ式を組み立てられなければ、その理由。
    #[allow(clippy::too_many_arguments)]
    fn try_candidate(&mut self, thread: u32, depth: usize, candidate: &Candidate, receiver: Option<&Node>, target: &CallTarget, name: &str, written: &[TypeSyntax], arguments: &[Argument], scope: &Scope, exact: bool) -> Result<Option<Node>, String> {
        let shape = &candidate.shape;
        let parameters = &shape.parameters;
        // 引数を、仮引数の場所に並べる: 拡張メソッドの対象、位置の引数、名前つき引数の順。
        let offset = candidate.extension as usize;
        let mut placed: Vec<Option<usize>> = vec![None; parameters.len()];
        // `params` の場所に、ばらで渡した引数（`Total(1, 2)`）。配列を 1 つだけ渡したなら、ふつうの引数として渡す。
        let params_slot = parameters.len().checked_sub(1).filter(|&last| parameters[last].is_params && last >= offset);
        let mut spread = Vec::new();
        let mut position = offset;
        for (index, argument) in arguments.iter().enumerate() {
            let slot = match argument.label {
                None => {
                    position += 1;
                    position - 1
                }
                Some(label) => match parameters.iter().position(|parameter| parameter.name == label) {
                    Some(slot) if slot >= offset => slot,
                    _ => return Ok(None),
                },
            };
            if argument.label.is_none() && params_slot.is_some_and(|last| slot >= last) {
                spread.push(index);
                continue;
            }
            match placed.get_mut(slot) {
                Some(entry @ None) => *entry = Some(index),
                _ => return Ok(None),
            }
        }
        if let ([only], Some(last)) = (spread.as_slice(), params_slot) {
            let passes_array = match &arguments[*only].node {
                Some(node) => is_array_type(&self.node_type(thread, node)?),
                None => matches!(arguments[*only].expr, Expr::Collection { .. } | Expr::DefaultLiteral),
            };
            if passes_array {
                placed[last] = Some(*only);
                spread.clear();
            }
        }
        // 渡さなかった仮引数は、省略できるか、`params` でなければならない。
        if (offset..parameters.len()).any(|slot| placed[slot].is_none() && !parameters[slot].optional && !parameters[slot].is_params) {
            return Ok(None);
        }
        let count = shape.type_parameters as usize;
        let mut inferred: Vec<Option<String>> = vec![None; count];
        if !written.is_empty() {
            if written.len() != count {
                return Ok(None);
            }
            for (slot, ty) in inferred.iter_mut().zip(written) {
                *slot = Some(type_syntax_text(ty));
            }
        }
        let mut all: Vec<Option<Node>> = placed.iter().map(|index| index.and_then(|index| arguments[index].node.clone())).collect();
        if candidate.extension {
            all[0] = receiver.cloned();
        }
        let placeholders: Vec<String> = (0..count).map(|number| format!("!!{number}")).collect();
        let Some(template) = self.modules[candidate.module].metadata().and_then(|metadata| metadata.parameter_types(shape.token, &candidate.class_names, &placeholders)) else { return Ok(None) };
        // ほかの引数の型から、メソッドの型引数を決める。
        for (index, node) in all.iter().enumerate() {
            let (Some(node), Some(Some(parameter))) = (node, template.get(index)) else { continue };
            self.infer_from_node(thread, parameter, node, &mut inferred);
        }
        // ばらで渡した `params` の引数は、配列の要素の型から（`params T[]` の T）。
        if let (Some(last), Some(&first)) = (params_slot, spread.first()) {
            let element = template.get(last).cloned().flatten().and_then(|shape| shape.display().strip_suffix("[]").map(str::to_string));
            if let (Some(element), Some(node)) = (element, arguments[first].node.clone()) {
                self.infer_from_node(thread, &crate::debugger::metadata::TypeShape { name: element, arguments: Vec::new() }, &node, &mut inferred);
            }
        }
        // ラムダ式を、渡す先のデリゲートの型で組み立てる。
        let outer = self.outer_scope(scope);
        for (slot, index) in placed.iter().enumerate() {
            let Some(argument) = index.map(|index| &arguments[index]) else { continue };
            if !argument.group && !matches!(argument.expr, Expr::Lambda(..)) {
                continue;
            }
            let Some(delegate) = self.parameter_type(candidate, &inferred, slot) else { return Ok(None) };
            let delegate = split_shape(delegate);
            let Some((inputs, output)) = self.delegate_signature_of(thread, depth, &delegate) else { return Ok(None) };
            // メソッドの集まりは、デリゲートの引数の数だけ引数を取るラムダ式に直す。
            let (lambda_parameters, body) = match argument.expr {
                Expr::Lambda(parameters, body) if !argument.group => (parameters.clone(), (**body).clone()),
                method => method_group::as_lambda(method, inputs.len()),
            };
            let (lambda_parameters, body) = (&lambda_parameters, &body);
            if inputs.len() != lambda_parameters.len() || inputs.iter().any(|input| input.contains("!!")) {
                return Ok(None);
            }
            let open = output.as_deref().and_then(placeholder);
            let nested = output.as_deref().filter(|output| open.is_none() && output.contains("!!")).map(str::to_string);
            let typed = if open.is_some() || nested.is_some() { None } else { Some(&delegate) };
            let (node, body_type) = self.lambda_expression(thread, depth, lambda_parameters, body, &inputs, typed, &outer, exact)?;
            if let Some(number) = open {
                inferred[number] = Some(display_of_reflection(&body_type).ok_or_else(|| tr!("Cannot read the type name {name}.", name = body_type))?);
            }
            // 戻り値の型の中に型引数があるもの（SelectMany の `IEnumerable<TResult>`）: 本体の型を、その型として見たときの型引数。
            if let Some(output) = nested {
                let Some((generic, number, at, arity)) = nested_placeholder(&output) else { return Ok(None) };
                let returned = self.member(thread, &node, "ReturnType")?;
                let Some(found) = self.generic_argument_of_type(thread, &returned, &generic, arity, at) else { return Ok(None) };
                inferred[number] = Some(unsafe { value::type_name(&found, &self.modules) });
            }
            // 型を決めずに組み立てたラムダ式は `Func<…>` になる。渡す先のデリゲートの型（`Converter<T, TOutput>` など）で
            // 包み直す（本体と引数はそのまま）。
            let node = if typed.is_none() {
                let Some(delegate) = self.parameter_type(candidate, &inferred, slot) else { return Ok(None) };
                let delegate = self.type_object(thread, depth, &split_shape(delegate).display())?;
                let (body, parameters) = (self.member(thread, &node, "Body")?, self.member(thread, &node, "Parameters")?);
                self.factory(thread, depth, "Lambda", vec![delegate, body, parameters])?
            } else {
                node
            };
            all[slot] = Some(node);
        }
        if inferred.iter().any(Option::is_none) {
            return Ok(None);
        }
        // 型を渡す先から決める式と、渡さなかった仮引数（既定値、空の `params`）。
        for slot in offset..parameters.len() {
            if all[slot].is_some() {
                continue;
            }
            let Some(ty) = self.parameter_type(candidate, &inferred, slot) else { return Ok(None) };
            let ty = ty.display();
            if Some(slot) == params_slot && !spread.is_empty() {
                all[slot] = Some(self.params_node(thread, depth, &ty, &spread, arguments, scope)?);
                continue;
            }
            all[slot] = Some(match placed[slot].map(|index| arguments[index].expr) {
                Some(expr) => self.targeted_node(thread, depth, expr, &ty, scope)?,
                None => self.omitted_node(thread, depth, &parameters[slot], &ty)?,
            });
        }
        let Some(mut all) = all.into_iter().collect::<Option<Vec<Node>>>() else { return Ok(None) };
        let inferred: Vec<String> = inferred.into_iter().flatten().collect();
        // 暗黙に変換できる引数（`object` の仮引数に int、`long` の仮引数に int）は、仮引数の型へ変換する
        // （`Expression.Call` は、型がそのまま合う引数しか受け付けない）。
        let names: Vec<Option<String>> = inferred.iter().cloned().map(Some).collect();
        for (slot, node) in all.iter_mut().enumerate() {
            // out / ref の引数は、仮引数と同じ型の変数を渡している。
            if placed[slot].is_some_and(|index| matches!(arguments[index].expr, Expr::Out(_))) {
                continue;
            }
            let Some(ty) = self.parameter_type(candidate, &names, slot) else { continue };
            let ty = ty.display();
            if !self.implicitly_converts(thread, depth, node, &ty)? {
                continue;
            }
            let target = self.type_object(thread, depth, &ty)?;
            *node = self.factory(thread, depth, "Convert", vec![node.clone(), target])?;
        }
        let mut type_objects = Vec::new();
        for ty in &inferred {
            type_objects.push(self.type_object(thread, depth, ty)?);
        }
        // コンストラクターは、引数の節の型（仮引数の型に揃えたもの）で探して `New`。
        if let CallTarget::Constructor { type_object, .. } = target {
            let mut types = Vec::new();
            for node in &all {
                types.push(self.member(thread, node, "Type")?);
            }
            let types = self.node_array(thread, depth, "System.Type", &types)?;
            let Some(constructor) = self.call_method(thread, depth, Some(Current::Value(type_object.clone())), "GetConstructor", vec![types])?.filter(|found| unsafe { value::argument_kind(found) } != ArgumentKind::Null) else { return Ok(None) };
            let arguments = self.node_array(thread, depth, EXPRESSION, &all)?;
            return Ok(self.factory(thread, depth, "New", vec![constructor, arguments]).ok());
        }
        let type_arguments = if type_objects.is_empty() { self.new_null(thread)? } else { self.node_array(thread, depth, "System.Type", &type_objects)? };
        let text = self.text(thread, name)?;
        let first = match (target, candidate.extension) {
            (CallTarget::Instance(node, _), false) => node.clone(),
            // 静的メソッドと拡張メソッドは、宣言している型の静的メソッドとして呼ぶ。
            _ => {
                let Some(owner) = self.modules[candidate.module].metadata().and_then(|metadata| metadata.declaring_type(shape.token & 0x00FF_FFFF)) else { return Ok(None) };
                let owner = self.plain_type_name(candidate.module, owner);
                self.type_object(thread, depth, &owner)?
            }
        };
        let arguments = self.node_array(thread, depth, EXPRESSION, &all)?;
        Ok(self.factory(thread, depth, "Call", vec![first, text, type_arguments, arguments]).ok())
    }

    /// 候補のメソッドの、`slot` 番目の仮引数の型（分かっている型引数を入れたもの）。
    fn parameter_type(&self, candidate: &Candidate, inferred: &[Option<String>], slot: usize) -> Option<crate::debugger::metadata::TypeShape> {
        let names: Vec<String> = inferred.iter().enumerate().map(|(number, ty)| ty.clone().unwrap_or_else(|| format!("!!{number}"))).collect();
        self.modules[candidate.module].metadata()?.parameter_types(candidate.shape.token, &candidate.class_names, &names)?.get(slot).cloned().flatten()
    }

    /// 節の値を、型 `ty`（表示の書き方）の仮引数へ渡すのに、変換を挟む必要があり、それが暗黙の変換（ボックス化、基底の型や
    /// インターフェイスへの変換、数の拡大）か。型がそのまま同じなら false。
    fn implicitly_converts(&mut self, thread: u32, depth: usize, node: &Node, ty: &str) -> Result<bool, String> {
        let full = self.node_type(thread, node)?;
        if reflection_name_is(&full, ty) {
            return Ok(false);
        }
        let target_keyword = parse_type(ty).map(|parsed| keyword_of(&parsed.name).to_string()).unwrap_or_default();
        if numeric_target(&full, &parse_type(ty).map(|parsed| parsed.name).unwrap_or_default(), false) == Some(target_keyword.as_str()) {
            return Ok(true);
        }
        let target = self.type_object(thread, depth, ty)?;
        let source = self.member(thread, node, "Type")?;
        self.assignable_from(thread, depth, &target, &source)
    }

    /// ばらで渡した `params` の引数（`spread` の位置の引数）を、配列（`ty` の型）にまとめる節。
    fn params_node(&mut self, thread: u32, depth: usize, ty: &str, spread: &[usize], arguments: &[Argument], scope: &Scope) -> Result<Node, String> {
        let element = ty.strip_suffix("[]").ok_or_else(|| tr!("The params type is not an array."))?;
        let element_type = self.type_object(thread, depth, element)?;
        let element_name = self.member(thread, &element_type, "FullName")?;
        let element_name = unsafe { value::as_string(&element_name) }.unwrap_or_default();
        let mut items = Vec::new();
        for &index in spread {
            let node = match &arguments[index].node {
                Some(node) => node.clone(),
                None => self.targeted_node(thread, depth, arguments[index].expr, element, scope)?,
            };
            items.push(self.converted_to(thread, depth, node, &element_type, &element_name)?);
        }
        let items = self.node_array(thread, depth, EXPRESSION, &items)?;
        self.factory(thread, depth, "NewArrayInit", vec![element_type, items])
    }

    /// 渡さなかった仮引数の節: 既定値か、空の `params` の配列。`ty` は仮引数の型。
    fn omitted_node(&mut self, thread: u32, depth: usize, parameter: &crate::debugger::metadata::Parameter, ty: &str) -> Result<Node, String> {
        if parameter.is_params {
            let element = ty.strip_suffix("[]").ok_or_else(|| tr!("The params type is not an array."))?;
            let element = self.type_object(thread, depth, element)?;
            let nothing = self.node_array(thread, depth, EXPRESSION, &[])?;
            return self.factory(thread, depth, "NewArrayInit", vec![element, nothing]);
        }
        let ty_object = self.type_object(thread, depth, ty)?;
        let Some((element, bytes)) = &parameter.default else { return self.factory(thread, depth, "Default", vec![ty_object]) };
        let value = self.default_value(thread, *element, bytes).ok_or_else(|| tr!("Cannot create the default value of the omitted argument."))?;
        let constant = self.factory(thread, depth, "Constant", vec![value])?;
        let name = self.member(thread, &ty_object, "FullName")?;
        let name = unsafe { value::as_string(&name) }.unwrap_or_default();
        self.converted_to(thread, depth, constant, &ty_object, &name)
    }

    /// 型を渡す先から決める式（`new(…)`、`[…]`、`default`）を、型 `ty` の値として作る節。そのほかの式は、ふつうに作る。
    pub(super) fn targeted_node(&mut self, thread: u32, depth: usize, expr: &Expr, ty: &str, scope: &Scope) -> Result<Node, String> {
        let target = parse_type(ty).ok_or_else(|| tr!("Cannot read the type name {name}.", name = ty))?;
        match expr {
            // `out var x`、`out _`、`out x`、`ref x`: 仮引数に、書き戻せる節（変数、フィールド、配列の要素）を渡す。
            // 新しく宣言する変数は、ラムダ式の本体の外側のブロックで宣言する（あとの式から名前で読める）。
            Expr::Out(place) => match place {
                OutTarget::Declare(written, name) => {
                    let ty = match written {
                        Some(written) => type_syntax_text(written),
                        None => ty.to_string(),
                    };
                    let ty = self.type_object(thread, depth, &ty)?;
                    self.lambda_local(thread, depth, ty, Some(name))
                }
                OutTarget::Discard => {
                    let ty = self.type_object(thread, depth, ty)?;
                    self.lambda_local(thread, depth, ty, None)
                }
                OutTarget::Existing(place) => self.assignable(thread, depth, place, scope),
                OutTarget::Tuple(_) => Err(tr!("A deconstruction cannot be written as an out argument.")),
            },
            Expr::DefaultLiteral => {
                let ty = self.type_object(thread, depth, ty)?;
                self.factory(thread, depth, "Default", vec![ty])
            }
            Expr::TargetNew { arguments, initializer } => self.new_object_node(thread, depth, &target, arguments, initializer, scope),
            Expr::Collection { arguments, items } if !arguments.is_empty() || items.iter().any(|(spread, _)| *spread) => self.built_collection_node(thread, depth, &target, arguments, items, scope),
            Expr::Collection { items, .. } => {
                let items: Vec<Expr> = items.iter().map(|(_, item)| item.clone()).collect();
                // 配列と、読むだけのインターフェイスは配列で、ほかは、その型（インターフェイスなら List<T>）に Add で足す。
                let read_only = ["System.Collections.Generic.IEnumerable", "System.Collections.Generic.IReadOnlyCollection", "System.Collections.Generic.IReadOnlyList"];
                if target.array || (read_only.contains(&target.name.as_str()) && target.arguments.len() == 1) {
                    let element = if target.array { target.element() } else { target.arguments[0].clone() };
                    return self.new_array_node(thread, depth, Some(&element), &[], Some(&items), scope);
                }
                let concrete = match target.name.as_str() {
                    "System.Collections.Generic.ICollection" | "System.Collections.Generic.IList" => TypeSyntax { name: "System.Collections.Generic.List".into(), ..target },
                    _ => target,
                };
                let initializer = Initializer::Items(items.into_iter().map(|item| vec![item]).collect());
                self.new_object_node(thread, depth, &concrete, &[], &initializer, scope)
            }
            other => self.tree(thread, depth, other, scope),
        }
    }

    /// 節の型の、メタデータの中の型と、型引数の名前（`List<int>` なら List と `[System.Int32]`）。配列などは `None`。
    fn node_class(&mut self, thread: u32, depth: usize, node: &Node) -> Option<(usize, u32, Vec<String>)> {
        let full = self.node_type(thread, node).ok()?;
        if is_array_type(&full) {
            return None;
        }
        let ty = parse_reflection_type(&full)?;
        let (module, row) = self.find_type_with_arity(thread, depth, &ty.name, ty.arguments.len())?;
        Some((module, row, ty.arguments.iter().map(type_syntax_text).collect()))
    }

    /// 対象の型（と、同じモジュールの基底の型）のインスタンスメソッド。
    fn instance_candidates(&mut self, thread: u32, depth: usize, node: &Node, name: &str) -> Vec<Candidate> {
        let Some((module, mut row, mut class_names)) = self.node_class(thread, depth, node) else { return Vec::new() };
        let mut found = Vec::new();
        for _ in 0..16 {
            let Some(metadata) = self.modules[module].metadata() else { break };
            for shape in metadata.methods_named(row, name).into_iter().filter(|shape| !shape.is_static) {
                found.push(Candidate { module, shape, class_names: class_names.clone(), extension: false });
            }
            // 基底の型の型引数は、派生した型の型引数から作り直せないので、型引数の無い基底の型までにする。
            match metadata.base_type(row) {
                Some(base) if metadata.type_parameter_count(base) == 0 => {
                    row = base;
                    class_names = Vec::new();
                }
                _ => break,
            }
        }
        found
    }

    /// 型の静的メソッド。
    fn static_candidates(&self, (module, row): (usize, u32), name: &str) -> Vec<Candidate> {
        let Some(metadata) = self.modules[module].metadata() else { return Vec::new() };
        metadata.methods_named(row, name).into_iter().filter(|shape| shape.is_static).map(|shape| Candidate { module, shape, class_names: Vec::new(), extension: false }).collect()
    }

    /// いまのメソッドのコードから見える名前空間（`using` と、自分の名前空間）の、その名前の拡張メソッド。
    fn extension_candidates(&mut self, thread: u32, depth: usize, name: &str) -> Vec<Candidate> {
        let visible: Vec<String> = self.type_name_candidates(thread, depth, "?").into_iter().map(|candidate| candidate.trim_end_matches('?').trim_end_matches('.').to_string()).collect();
        if visible.iter().any(|namespace| namespace == "System.Linq") {
            self.system_core_type(thread, depth, "System.Linq.Enumerable", "System.Linq");
        }
        let mut found = Vec::new();
        for module in 0..self.modules.len() {
            let Some(metadata) = self.modules[module].metadata() else { continue };
            for (shape, namespace) in metadata.extension_methods(name) {
                if visible.iter().any(|v| v == namespace) && shape.is_extension {
                    found.push(Candidate { module, shape, class_names: Vec::new(), extension: true });
                }
            }
        }
        found
    }

    /// `..` で広げる要素や `with(…)` のあるコレクション式を、型 `target` の値として作る節。`List<T>` を作って `Add` と
    /// `AddRange` で足し、配列と読むだけのインターフェイスには `ToArray()` で渡す。ほかの型は、その型を作って足す。
    fn built_collection_node(&mut self, thread: u32, depth: usize, target: &TypeSyntax, arguments: &[Expr], items: &[(bool, Expr)], scope: &Scope) -> Result<Node, String> {
        let read_only = ["System.Collections.Generic.IEnumerable", "System.Collections.Generic.IReadOnlyCollection", "System.Collections.Generic.IReadOnlyList"];
        let as_array = target.array || (read_only.contains(&target.name.as_str()) && target.arguments.len() == 1);
        if as_array && !arguments.is_empty() {
            return Err(tr!("A collection expression's with(…) cannot be written for an array."));
        }
        let builder = if as_array {
            let element = if target.array { target.element() } else { target.arguments[0].clone() };
            TypeSyntax { name: "System.Collections.Generic.List".into(), arguments: vec![element], ..TypeSyntax::default() }
        } else {
            match target.name.as_str() {
                "System.Collections.Generic.ICollection" | "System.Collections.Generic.IList" => TypeSyntax { name: "System.Collections.Generic.List".into(), ..target.clone() },
                _ => target.clone(),
            }
        };
        let created = self.new_object_node(thread, depth, &builder, arguments, &Initializer::None, scope)?;
        let (assign, collection) = self.stored(thread, depth, created, None)?;
        let mut steps = vec![assign];
        for (spread, item) in items {
            let node = self.tree(thread, depth, item, scope)?;
            steps.push(self.call_node(thread, depth, Some(collection.clone()), None, if *spread { "AddRange" } else { "Add" }, vec![node])?);
        }
        steps.push(if as_array { self.call_node(thread, depth, Some(collection), None, "ToArray", Vec::new())? } else { collection });
        self.block_of(thread, depth, &[], &steps)
    }

    /// ラムダ式（かメソッドの集まり）を、型 `ty`（表示の書き方）のデリゲートの節にする。`Expression<TDelegate>` なら、
    /// 式木のまま（`Quote`）渡す。外側のラムダ式の引数を使える。
    pub(super) fn lambda_for_type(&mut self, thread: u32, depth: usize, ty: &str, lambda: &Expr, scope: &Scope) -> Result<Node, String> {
        let parsed = parse_type(ty).ok_or_else(|| tr!("Cannot read the type name {name}.", name = ty))?;
        if parsed.name == "System.Linq.Expressions.Expression" && parsed.arguments.len() == 1 {
            let inner = self.lambda_for_type(thread, depth, &type_syntax_text(&parsed.arguments[0]), lambda, scope)?;
            return self.factory(thread, depth, "Quote", vec![inner]);
        }
        let runtime = self.runtime_type(thread, depth, &parsed)?;
        let display = unsafe { value::type_name(&runtime, &self.modules) };
        let parsed = parse_type(&display).ok_or_else(|| tr!("Cannot read the type name {name}.", name = display))?;
        let shape = crate::debugger::metadata::TypeShape { name: parsed.name.clone(), arguments: parsed.arguments.iter().map(type_syntax_text).collect() };
        let (inputs, _) = self.delegate_signature_of(thread, depth, &shape).ok_or_else(|| tr!("{type} is not a delegate type a lambda can be converted to.", type = display))?;
        let (parameters, body) = match lambda {
            Expr::Lambda(parameters, body) => (parameters.clone(), (**body).clone()),
            method => method_group::as_lambda(method, inputs.len()),
        };
        let outer = self.outer_scope(scope);
        Ok(self.lambda_expression(thread, depth, &parameters, &body, &inputs, Some(&shape), &outer, false)?.0)
    }

    /// 内側のラムダ式から見える、外側の変数（引数、ローカル変数、パターンの変数）。
    pub(super) fn outer_scope(&self, scope: &Scope) -> Vec<(String, Node)> {
        let mut outer = scope.to_vec();
        if let Some(flow) = self.lambda_flows.last() {
            outer.extend(flow.named.iter().cloned());
        }
        outer
    }

    /// 引数の型の形（`!!0`、`IEnumerable<!!0>`、`!!0[]`）と、渡す節の型から、メソッドの型引数を決める。
    fn infer_from_node(&mut self, thread: u32, parameter: &crate::debugger::metadata::TypeShape, node: &Node, inferred: &mut [Option<String>]) {
        let display = parameter.display();
        let Ok(full) = self.node_type(thread, node) else { return };
        let Some(shown) = display_of_reflection(&full) else { return };
        if let Some(number) = placeholder(&display) {
            if let Some(slot) = inferred.get_mut(number).filter(|slot| slot.is_none()) {
                *slot = Some(shown);
            }
            return;
        }
        // 配列（`!!0[]`）は、要素の型。
        if let Some(number) = display.strip_suffix("[]").and_then(placeholder) {
            if let (Some(slot), Some(element)) = (inferred.get_mut(number), shown.strip_suffix("[]")) {
                slot.get_or_insert_with(|| element.to_string());
            }
            return;
        }
        // 型引数の中の型引数（`IEnumerable<!!0>`、`Dictionary<!!0, !!1>`）は、節の型をその型として見たときの型引数。
        let arity = parameter.arguments.len();
        if arity == 0 {
            return;
        }
        let Ok(ty) = self.member(thread, node, "Type") else { return };
        for (at, argument) in parameter.arguments.iter().enumerate() {
            let Some(number) = placeholder(argument) else { continue };
            if inferred.get(number).is_none_or(Option::is_some) {
                continue;
            }
            if let Some(found) = self.generic_argument_of_type(thread, &ty, &parameter.name, arity, at) {
                inferred[number] = Some(unsafe { value::type_name(&found, &self.modules) });
            }
        }
    }
}
