//! パターンの組み合わせ（`and` / `or`）、変数を作るパターン（`int k`、`var v`）、プロパティのパターン、位置のパターン、
//! リストのパターンと、`switch` 式。
//!
//! パターンが作った変数（`k`）は、同じ式の中であとから使える（`x is int k && k > 40`）。式を評価している間だけの変数と
//! して、セッションに持っておく（`Session::pattern_locals`）。

use super::*;
use crate::debugger::session::eval::syntax::Arm;

impl Session {
    /// 組み合わせと、入れ子のパターン。単純なもの（型、定数、比較、null、not）は `matches_pattern` が見る。
    pub(super) fn matches_compound(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, input: &Typed, pattern: &Pattern, negated: bool) -> Result<bool, String> {
        Ok(match pattern {
            Pattern::And(left, right) => self.matches_pattern(thread, depth, value, input, left, negated)? && self.matches_pattern(thread, depth, value, input, right, negated)?,
            Pattern::Or(left, right) => self.matches_pattern(thread, depth, value, input, left, negated)? || self.matches_pattern(thread, depth, value, input, right, negated)?,
            Pattern::Discard => true,
            Pattern::Var(name) => {
                self.bind(name, value.clone());
                true
            }
            Pattern::Declaration(ty, name) => {
                let resolved = self.resolve_type(thread, depth, ty)?;
                let fits = self.value_is(value, &resolved) && self.type_arguments_match(thread, depth, value, ty, &resolved);
                if fits {
                    // `object` に入った数は、中身の数として変数に入れる（`k > 40` が計算できるように）。
                    let bound = if matches!(resolved, ResolvedType::Primitive(_)) { self.unboxed(thread, self.unwrapped(value.clone())) } else { self.unwrapped(value.clone()) };
                    self.bind(name, bound);
                }
                fits
            }
            Pattern::Recursive { ty, positional, members, designation } => {
                if self.operand(value) == Operand::Null {
                    return Ok(false);
                }
                if let Some(ty) = ty {
                    let resolved = self.resolve_type(thread, depth, ty)?;
                    if !self.value_is(value, &resolved) || !self.type_arguments_match(thread, depth, value, ty, &resolved) {
                        return Ok(false);
                    }
                }
                let value = self.unwrapped(value.clone());
                if let Some(items) = positional {
                    // タプル（ValueTuple の Item1、Item2、…）は要素を、そのほかの型は Deconstruct(out …) の結果を比べる。
                    let is_tuple = unsafe { value::type_names(&value, &self.modules) }.first().is_some_and(|name| name.starts_with("System.ValueTuple"));
                    let elements = if is_tuple {
                        if unsafe { value::field(&value, &self.modules, &format!("Item{}", items.len() + 1)) }.is_some() {
                            return Ok(false);
                        }
                        let mut elements = Vec::new();
                        for index in 0..items.len() {
                            elements.push(unsafe { value::field(&value, &self.modules, &format!("Item{}", index + 1)) }.ok_or_else(|| tr!("The number of tuple elements differs from the pattern."))?);
                        }
                        elements
                    } else {
                        self.deconstructed(thread, depth, &value, items.len())?
                    };
                    for (element, item) in elements.iter().zip(items) {
                        let element_type = self.typed_of(None, element, false);
                        if !self.matches_pattern(thread, depth, element, &element_type, item, false)? {
                            return Ok(false);
                        }
                    }
                }
                for (path, sub) in members {
                    let mut current = value.clone();
                    for (step, name) in path.iter().enumerate() {
                        // 入れ子のメンバー（`A.B`）は、途中が null なら合わない。
                        if step > 0 && self.operand(&current) == Operand::Null {
                            return Ok(false);
                        }
                        current = self.member(thread, &current, name)?;
                    }
                    let member_type = self.typed_of(None, &current, false);
                    if !self.matches_pattern(thread, depth, &current, &member_type, sub, false)? {
                        return Ok(false);
                    }
                }
                if let Some(name) = designation {
                    self.bind(name, value);
                }
                true
            }
            Pattern::List { items, slice, designation } => {
                if self.operand(value) == Operand::Null {
                    return Ok(false);
                }
                let length = self.count_of(thread, value)?;
                let fixed = items.len();
                let fits = match slice {
                    None => length == fixed,
                    Some(_) => length >= fixed,
                };
                if !fits {
                    return Ok(false);
                }
                for (index, item) in items.iter().enumerate() {
                    // `..` よりあとの要素は、終わりから数える。
                    let position = match slice {
                        Some(at) if index >= *at => length - (fixed - index),
                        _ => index,
                    };
                    let element = self.element_at(thread, value, position)?;
                    let element_type = self.typed_of(None, &element, false);
                    if !self.matches_pattern(thread, depth, &element, &element_type, item, false)? {
                        return Ok(false);
                    }
                }
                if let Some(name) = designation {
                    self.bind(name, value.clone());
                }
                true
            }
            // 単純なパターンは、ここには来ない。
            _ => self.matches_pattern(thread, depth, value, input, pattern, negated)?,
        })
    }

    /// 値の `Deconstruct(out …)` を呼んで、`count` 個に分けた値。
    pub(super) fn deconstructed(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, count: usize) -> Result<Vec<ComPtr<ICorDebugValue>>, String> {
        let names: Vec<String> = (0..count).map(|index| format!("<deconstruct>{index}")).collect();
        let mut extra = bind::CallExtra::default();
        let mut arguments = Vec::new();
        for (position, name) in names.iter().enumerate() {
            extra.outs.push((position, syntax::OutTarget::Declare(None, name.clone())));
            arguments.push(self.new_null(thread)?);
        }
        let mark = self.pattern_locals.len();
        let called = self.call_method_with(thread, depth, Some(Current::Value(value.clone())), "Deconstruct", arguments, &extra);
        let parts: Option<Vec<_>> = names.iter().map(|name| self.pattern_locals.iter().rev().find(|(existing, _)| existing == name).map(|(_, part)| part.clone())).collect();
        self.pattern_locals.truncate(mark);
        called.map_err(|error| tr!("Cannot call the Deconstruct with {count} parts that the positional pattern needs: {error}", count = count, error = error))?;
        parts.ok_or_else(|| tr!("Cannot read the result of Deconstruct."))
    }

    /// パターンが作った変数を、式の残りから見えるようにする。
    fn bind(&mut self, name: &str, value: ComPtr<ICorDebugValue>) {
        self.pattern_locals.push((name.to_string(), value));
    }

    /// 配列やコレクションの要素の数（配列は Length、ほかは Length か Count のプロパティ）。
    pub(super) fn count_of(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Result<usize, String> {
        if let Some(length) = unsafe { value::array_length(value) } {
            return Ok(length as usize);
        }
        for name in ["Length", "Count"] {
            if let Ok(count) = self.member(thread, value, name) {
                if let Operand::Int { value, .. } = self.operand(&count) {
                    return usize::try_from(value).map_err(|_| tr!("The number of elements is negative."));
                }
            }
        }
        Err(tr!("This value has no element count (Length or Count)."))
    }

    /// 配列やコレクションの、位置の要素。
    pub(super) fn element_at(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>, position: usize) -> Result<ComPtr<ICorDebugValue>, String> {
        let index = self.new_int(thread, i32::try_from(position).map_err(|_| tr!("The position is too large."))?)?;
        self.index(thread, value, &[index])
    }

    /// `値 switch { 腕, … }`。上の腕から順に、パターンと `when` の条件を確かめ、最初に合った腕の結果を返す。
    pub(super) fn evaluate_switch(&mut self, thread: u32, depth: usize, subject: &Expr, arms: &[Arm]) -> Result<Current, String> {
        let current = self.evaluate_node(thread, depth, subject)?;
        let lifted = matches!(current, Current::Nullable(_));
        let value = self.value_of(current)?;
        // 共用体は、中の値で照合する。
        let value = self.union_case(thread, &value).unwrap_or(value);
        let input = self.typed_of(Some(subject), &value, lifted);
        for (index, arm) in arms.iter().enumerate() {
            let scope = self.pattern_locals.len();
            let mut taken = self.matches_pattern(thread, depth, &value, &input, &arm.pattern, false)?;
            if taken {
                if let Some(guard) = &arm.guard {
                    taken = match self.evaluate_operand(thread, depth, guard)? {
                        Operand::Bool(flag) => flag,
                        other => return Err(tr!("The when condition must be bool (it is {type}).", type = operators::kind_name(&other))),
                    };
                }
            }
            if !taken {
                self.pattern_locals.truncate(scope);
                continue;
            }
            let result = self.evaluate_node(thread, depth, &arm.result)?;
            // 結果の型は、すべての腕の結果の型から決まる（`_ => 2.5` があれば、ほかの腕の 1 も double）。
            // ほかの腕の結果は、呼び出しを含まず、パターンの変数を使わないものだけを、型を知るために評価する。
            let result_lifted = matches!(result, Current::Nullable(_));
            let result_value = match result {
                Current::Value(value) | Current::Nullable(value) => value,
                other => return Ok(other),
            };
            let mut result_type = self.typed_of(Some(&arm.result), &result_value, result_lifted);
            let chosen_type = result_type.clone();
            for (other_index, other) in arms.iter().enumerate() {
                if other_index == index || !repeatable(&other.result) {
                    continue;
                }
                let Ok(other_current) = self.evaluate_node(thread, depth, &other.result) else { continue };
                let other_lifted = matches!(other_current, Current::Nullable(_));
                let Ok(other_value) = self.value_of(other_current) else { continue };
                let other_type = self.typed_of(Some(&other.result), &other_value, other_lifted);
                if let Some(unified) = types::unify(&result_type, &other_type) {
                    result_type = unified;
                }
            }
            return self.convert_to(thread, result_value, &chosen_type, &result_type);
        }
        Err(tr!("No arm of the switch expression matched (it would be a SwitchExpressionException at run time)."))
    }

    /// タプル（`(1, "a")`）。要素の型の ValueTuple を作る。
    pub(super) fn create_tuple(&mut self, thread: u32, depth: usize, items: &[Expr]) -> Result<ComPtr<ICorDebugValue>, String> {
        if !(2..=7).contains(&items.len()) {
            return Err(tr!("Only tuples of 2 to 7 elements can be created."));
        }
        let values = self.evaluate_arguments(thread, depth, items)?;
        self.tuple_of(thread, depth, values)
    }

    /// 値の並びから、タプル（`ValueTuple<…>`）を作る。
    pub(super) fn tuple_of(&mut self, thread: u32, depth: usize, mut values: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        let items = &values.clone();
        let mut types = Vec::new();
        for value in &values {
            types.push(unsafe { value::exact_type_of(value) }.ok_or_else(|| tr!("null cannot be written as a tuple element (its type cannot be determined). Cast it."))?);
        }
        let (module, row) = self.find_type_with_arity(thread, depth, "System.ValueTuple", items.len()).ok_or_else(|| tr!("System.ValueTuple is not found in the target."))?;
        let mut candidates = self.type_method_candidates(module, row, ".ctor", false, items.len() as u32);
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = types.clone();
        }
        let constructor = self.choose_overload(thread, candidates, &mut values).ok_or_else(|| tr!("Cannot create the tuple."))?;
        self.new_object(thread, &constructor, &values).map_err(|failure| tr!("Cannot create the tuple: {reason}", reason = failure.describe(self)))
    }

    /// `値 with { 名前 = 値, … }`。レコード（クラス）は `<Clone>$` で写しを作り、構造体は値の写しを作ってから、
    /// メンバーに値を入れる。
    pub(super) fn evaluate_with(&mut self, thread: u32, depth: usize, source: &Expr, members: &[(String, Expr)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let current = self.evaluate_node(thread, depth, source)?;
        let original = self.value_of(current)?;
        let copy = match self.call_method(thread, depth, Some(Current::Value(original.clone())), "<Clone>$", Vec::new()) {
            Ok(Some(copy)) => copy,
            _ => {
                let is_struct = unsafe { value::type_names(&original, &self.modules) }.iter().any(|name| name == "System.ValueType");
                if !is_struct {
                    return Err(tr!("with can be used only on records and structs."));
                }
                // 構造体の写し: 同じ型の空の値を作り、中身を写す。
                let ty = unsafe { value::exact_type_of(&original) }.ok_or_else(|| tr!("The value's type is not known."))?;
                let copy = self.new_object_of_type(thread, &ty).map_err(|failure| failure.describe(self))?;
                let target = unsafe { value::inside_box(&copy) }.unwrap_or_else(|| copy.clone());
                self.write(thread, &target, &original)?;
                copy
            }
        };
        for (name, expr) in members {
            let value = self.evaluate_node(thread, depth, expr)?;
            let value = self.value_of(value)?;
            self.store_member(thread, depth, &copy, name, &value)?;
        }
        Ok(copy)
    }

    /// 添字の `^n`（終わりから）と範囲（`a..b`）。数の添字に直すか、範囲なら部分を作る。添字が範囲でも `^` でも
    /// なければ `None`。
    pub(super) fn index_special(&mut self, thread: u32, depth: usize, target: &ComPtr<ICorDebugValue>, indices: &[Expr]) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let [index] = indices else { return Ok(None) };
        match index {
            Expr::FromEnd(inner) => {
                let length = self.count_of(thread, target)?;
                let from_end = self.position_of(thread, depth, inner)?;
                let position = length.checked_sub(from_end).filter(|_| from_end > 0).ok_or_else(|| tr!("The position of ^ is out of range."))?;
                Ok(Some(self.element_at(thread, target, position)?))
            }
            Expr::Range(start, end) => {
                let length = self.count_of(thread, target)?;
                let start = match start {
                    Some(start) => self.bound_of(thread, depth, start, length)?,
                    None => 0,
                };
                let end = match end {
                    Some(end) => self.bound_of(thread, depth, end, length)?,
                    None => length,
                };
                if start > end || end > length {
                    return Err(tr!("The range points outside the elements."));
                }
                // 文字列は Substring、配列は新しい配列に写す。
                if unsafe { value::argument_kind(target) } == ArgumentKind::String {
                    let (from, count) = (self.new_int(thread, start as i32)?, self.new_int(thread, (end - start) as i32)?);
                    return self.call_method(thread, depth, Some(Current::Value(target.clone())), "Substring", vec![from, count]);
                }
                if unsafe { value::array_length(target) }.is_none() {
                    return Err(tr!("Range indexes work only on strings and arrays for now."));
                }
                let element = unsafe { value::element_or_argument_types(target) }.into_iter().next().ok_or_else(|| tr!("The array's element type is not known."))?;
                let part = self.new_array(thread, &element, &[(end - start) as u32]).map_err(|failure| failure.describe(self))?;
                for offset in 0..end - start {
                    let source = self.element_at(thread, target, start + offset)?;
                    let slot = unsafe { value::array_element(&part, &[offset as u32]) }?.ok_or_else(|| tr!("Cannot read an element of the created array."))?;
                    self.write(thread, &slot, &source)?;
                }
                Ok(Some(part))
            }
            _ => Ok(None),
        }
    }

    /// 範囲の端（`^n` なら、終わりから数えた位置）。
    fn bound_of(&mut self, thread: u32, depth: usize, bound: &Expr, length: usize) -> Result<usize, String> {
        match bound {
            Expr::FromEnd(inner) => length.checked_sub(self.position_of(thread, depth, inner)?).ok_or_else(|| tr!("The position of ^ is out of range.")),
            other => self.position_of(thread, depth, other),
        }
    }

    /// 0 以上の整数の位置。
    fn position_of(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<usize, String> {
        match self.evaluate_operand(thread, depth, expr)? {
            Operand::Int { value, .. } => usize::try_from(value).map_err(|_| tr!("The position is negative.")),
            other => Err(tr!("The position must be an integer (it is {type}).", type = operators::kind_name(&other))),
        }
    }
}

impl Session {
    /// `値 is パターン`。`input` は値の（コンパイル時の）型で、パターンが書ける組み合わせかを確かめるのに使う。
    /// `negated` は、`not` の中にいること。
    pub(super) fn matches_pattern(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, input: &Typed, pattern: &Pattern, negated: bool) -> Result<bool, String> {
        Ok(match pattern {
            Pattern::Not(inner) => !self.matches_pattern(thread, depth, value, input, inner, true)?,
            Pattern::Null => {
                if input.never_null() {
                    return Err(tr!("A pattern comparing a value of a non-nullable type with null cannot be written."));
                }
                self.operand(value) == Operand::Null
            }
            // 以下、`object` に入った数は中身で比べる（パターンは、実行時の型を見る）。
            // decimal の値は、decimal の比較の演算子で（定数は decimal に変換される。`m is > 0`）。
            Pattern::Relational(op, bound) if input.kind == Kind::Decimal && matches!(self.operand(value), Operand::Other(_)) => {
                let current = self.evaluate_node(thread, depth, bound)?;
                let bound_value = self.value_of(current)?;
                let name = userops::binary_operator_name(*op).ok_or_else(|| tr!("This comparison operator cannot be written in a pattern."))?;
                match self.user_operator(thread, name, &[self.unwrapped(value.clone()), bound_value])? {
                    Some(result) => self.operand(&result) == Operand::Bool(true),
                    None => return Err(tr!("The pattern's constant cannot be compared with decimal.")),
                }
            }
            Pattern::Relational(op, bound) => {
                let bound_operand = self.pattern_constant(thread, depth, input, bound)?;
                let bound_operand = constant_as(bound, bound_operand, &self.operand_unboxed(value), true);
                // 比べられない値（数でないもの）は、パターンに合わない。
                operators::binary(*op, &self.operand_unboxed(value), &bound_operand) == Ok(Operand::Bool(true))
            }
            Pattern::TypeOrConstant(ty, constant_expr) => match ty.as_ref().map(|ty| (ty, self.resolve_type(thread, depth, ty))) {
                Some((ty, Ok(resolved))) => {
                    // `not 型` は、値型の値には、いつも合うか決して合わないかのどちらかで、C# はエラーにする。
                    // null 許容の値型（int?）には、中身の型についてだけ書ける。
                    // string と値型の間も、決して合わない（`s is not int`、`maybe is not string`）。
                    let target = self.typed_of_resolved(&resolved);
                    if negated && input.is_value_type() && (!input.nullable || ty.nullable || (target.is_value_type() && target.kind != input.kind) || target.kind == Kind::Str) {
                        return Err(tr!("A not pattern of that type cannot be written for a value of this type (the result would always be the same)."));
                    }
                    if negated && input.kind == Kind::Str && target.is_value_type() {
                        return Err(tr!("A not pattern of a value type cannot be written for a string value (it never matches)."));
                    }
                    self.value_is(value, &resolved) && self.type_arguments_match(thread, depth, value, ty, &resolved)
                }
                // 型の名前でなければ、定数として値を比べる。
                _ => {
                    let constant = self.pattern_constant(thread, depth, input, constant_expr)?;
                    let constant = constant_as(constant_expr, constant, &self.operand_unboxed(value), true);
                    operators::binary(BinaryOp::Equal, &self.operand_unboxed(value), &constant) == Ok(Operand::Bool(true))
                }
            },
            // 組み合わせと、入れ子のパターン（patterns.rs）。
            _ => self.matches_compound(thread, depth, value, input, pattern, negated)?,
        })
    }

    /// パターンの中の定数を評価する。値の型が値型なら、定数がその型に暗黙に変換できることを確かめる
    /// （`i is 1.5`、`sb is true` は、C# でもエラー）。
    pub(super) fn pattern_constant(&mut self, thread: u32, depth: usize, input: &Typed, expr: &Expr) -> Result<Operand, String> {
        let current = self.evaluate_node(thread, depth, expr)?;
        let lifted = matches!(current, Current::Nullable(_));
        let constant = self.value_of(current)?;
        if input.is_value_type() {
            let constant_type = self.typed_of(Some(expr), &constant, lifted);
            if !types::converts_with_constant(&constant_type, &input.underlying()) {
                return Err(tr!("The pattern's constant cannot be converted to the type of the compared value."));
            }
        }
        // string の値と比べられる定数は、文字列と null だけ（`s is 1` は、C# でもエラー）。
        if input.kind == Kind::Str && !matches!(self.typed_of(Some(expr), &constant, lifted).kind, Kind::Str | Kind::Null) {
            return Err(tr!("A pattern constant compared with a string value must be a string or null."));
        }
        Ok(self.operand(&constant))
    }
}
