//! 利用者が定義した演算子と変換。`decimal` も、言語に組み込みの型ではあるが、計算はここを通る
//! （`System.Decimal` の `op_Addition` などを、対象の中で実行する）。
//!
//! C# の規則のうち、扱うのは次の範囲:
//! - 二項・単項の演算子: 被演算子の型が宣言している `op_…` を、引数の型で選ぶ。
//! - 引数の型が合わないときは、演算子を宣言している型への暗黙の変換（`op_Implicit`）を 1 回だけ試す
//!   （`price + 1` は、1 を decimal にしてから足す）。
//! - キャスト: 変換元か変換先の型が宣言している `op_Implicit` / `op_Explicit` のうち、戻り値が変換先の型のもの。

use super::*;

/// C# 14 の、利用者が定義した複合代入の演算子（インスタンスの `operator +=`）のメソッドの名前。
pub(super) fn compound_assignment_name(op: BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Add => "op_AdditionAssignment",
        BinaryOp::Subtract => "op_SubtractionAssignment",
        BinaryOp::Multiply => "op_MultiplicationAssignment",
        BinaryOp::Divide => "op_DivisionAssignment",
        BinaryOp::Remainder => "op_ModulusAssignment",
        BinaryOp::BitAnd => "op_BitwiseAndAssignment",
        BinaryOp::BitOr => "op_BitwiseOrAssignment",
        BinaryOp::BitXor => "op_ExclusiveOrAssignment",
        BinaryOp::ShiftLeft => "op_LeftShiftAssignment",
        BinaryOp::ShiftRight => "op_RightShiftAssignment",
        BinaryOp::ShiftRightUnsigned => "op_UnsignedRightShiftAssignment",
        _ => return None,
    })
}

/// 二項演算子に当たるメソッドの名前。
pub(super) fn binary_operator_name(op: BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Add => "op_Addition",
        BinaryOp::Subtract => "op_Subtraction",
        BinaryOp::Multiply => "op_Multiply",
        BinaryOp::Divide => "op_Division",
        BinaryOp::Remainder => "op_Modulus",
        BinaryOp::BitAnd => "op_BitwiseAnd",
        BinaryOp::BitOr => "op_BitwiseOr",
        BinaryOp::BitXor => "op_ExclusiveOr",
        BinaryOp::ShiftLeft => "op_LeftShift",
        BinaryOp::ShiftRight => "op_RightShift",
        BinaryOp::ShiftRightUnsigned => "op_UnsignedRightShift",
        BinaryOp::Equal => "op_Equality",
        BinaryOp::NotEqual => "op_Inequality",
        BinaryOp::Less => "op_LessThan",
        BinaryOp::LessOrEqual => "op_LessThanOrEqual",
        BinaryOp::Greater => "op_GreaterThan",
        BinaryOp::GreaterOrEqual => "op_GreaterThanOrEqual",
        // && と || は、利用者が定義できない（true / false 演算子と & | の組み合わせになるが、扱わない）。
        BinaryOp::And | BinaryOp::Or => return None,
    })
}

/// 単項演算子に当たるメソッドの名前。
pub(super) fn unary_operator_name(op: operators::UnaryOp) -> &'static str {
    match op {
        operators::UnaryOp::Not => "op_LogicalNot",
        operators::UnaryOp::Plus => "op_UnaryPlus",
        operators::UnaryOp::Negate => "op_UnaryNegation",
        operators::UnaryOp::Complement => "op_OnesComplement",
    }
}

impl Session {
    /// 値の型が、演算子や変換を宣言できる型（クラスか構造体）なら、その型。プリミティブ型、文字列、列挙型、配列、
    /// `object` に入った数、null は `None`。
    pub(super) fn user_type(&self, value: &ComPtr<ICorDebugValue>) -> Option<(usize, u32)> {
        unsafe {
            if !matches!(value::argument_kind(value), ArgumentKind::ValueType | ArgumentKind::Reference) || value::unboxed_primitive(value, &self.modules).is_some() || self.enum_operand(value).is_some() {
                return None;
            }
            self.class_row(value)
        }
    }

    /// 引数の型が宣言している演算子を選んで、実行する。当てはまる演算子が無ければ `Ok(None)`。
    pub(super) fn user_operator(&mut self, thread: u32, name: &str, arguments: &[ComPtr<ICorDebugValue>]) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let mut owners: Vec<(usize, u32)> = Vec::new();
        for argument in arguments {
            if let Some(owner) = self.user_type(argument).filter(|owner| !owners.contains(owner)) {
                owners.push(owner);
            }
        }
        let count = arguments.len() as u32;
        for owner in owners {
            // 1 回目はそのまま、2 回目は、型の違う引数を演算子の持ち主の型に暗黙に変換してから。
            for convert in [false, true] {
                let mut attempt = arguments.to_vec();
                if convert {
                    for argument in attempt.iter_mut() {
                        if self.user_type(argument) != Some(owner) {
                            if let Some(converted) = self.implicitly_converted(thread, owner, argument) {
                                *argument = converted;
                            }
                        }
                    }
                }
                let candidates = self.type_method_candidates(owner.0, owner.1, name, true, count);
                if candidates.is_empty() {
                    break;
                }
                let Some(callable) = self.choose_overload(thread, candidates, &mut attempt) else { continue };
                return match self.call(thread, &callable, &attempt) {
                    Ok(Some(result)) => Ok(Some(result)),
                    Ok(None) => Err(tr!("{name} returned no value.", name = name)),
                    // 演算子の中の例外（decimal のあふれ、0 での割り算）は、計算の失敗として伝える。
                    Err(failure) => Err(tr!("Cannot calculate: {reason}", reason = failure.describe(self))),
                };
            }
        }
        Ok(None)
    }

    /// 構造体（decimal も）の値なら、演算子を探す型。null 許容の値の中身もここで見る。
    fn struct_owner(&self, value: &ComPtr<ICorDebugValue>) -> Option<(usize, u32)> {
        let inner = self.unwrapped(value.clone());
        matches!(self.operand(&inner), Operand::Other(_)).then(|| self.user_type(&inner)).flatten()
    }

    /// 構造体（decimal も）の二項演算のうち、相手が null 許容の値、null のリテラル、nint のもの。当てはまらなければ `Ok(None)`。
    ///
    /// C# は、構造体の演算子を「持ち上げて」使う（`m + maybe` は decimal?、`m + null` は decimal? の null、
    /// `m == null` は false、`m < null` は false）。nint / nuint から decimal への暗黙の変換は、decimal が宣言しておらず、
    /// コンパイラは long / ulong を経由させる。
    pub(super) fn struct_binary(&mut self, thread: u32, op: BinaryOp, left: &ComPtr<ICorDebugValue>, left_nullable: bool, right: &ComPtr<ICorDebugValue>, right_nullable: bool) -> Result<Option<Current>, String> {
        let Some(name) = binary_operator_name(op) else { return Ok(None) };
        let Some(owner) = self.struct_owner(left).or_else(|| self.struct_owner(right)) else { return Ok(None) };
        let (left, right) = (self.unwrapped(left.clone()), self.unwrapped(right.clone()));
        let (left_operand, right_operand) = (self.operand(&left), self.operand(&right));
        let native = |operand: &Operand| matches!(operand, Operand::Int { kind: IntKind::IPtr | IntKind::UPtr, .. });
        let null_side = left_operand == Operand::Null || right_operand == Operand::Null;
        let lifted = left_nullable || right_nullable || null_side;
        if !lifted && !native(&left_operand) && !native(&right_operand) {
            return Ok(None);
        }
        let comparison = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual);
        if null_side {
            // 結果は値によらないが、演算子そのものは要る（演算子の無い構造体と null は、C# でもエラー）。
            if self.type_method_candidates(owner.0, owner.1, name, true, 2).is_empty() {
                return Ok(None);
            }
            let both = left_operand == Operand::Null && right_operand == Operand::Null;
            let value = match op {
                BinaryOp::Equal => Operand::Bool(both),
                BinaryOp::NotEqual => Operand::Bool(!both),
                _ if comparison => Operand::Bool(false),
                _ => Operand::Null,
            };
            return self.lifted_value(thread, operators::Lifted { value, nullable: !comparison }).map(Some);
        }
        let left = self.without_native(thread, left, &left_operand)?;
        let right = self.without_native(thread, right, &right_operand)?;
        let Some(result) = self.user_operator(thread, name, &[left, right])? else { return Ok(None) };
        Ok(Some(if lifted && !comparison { Current::Nullable(result) } else { Current::Value(result) }))
    }

    /// nint / nuint の値を long / ulong にする（decimal への変換は、そこから）。ほかの値は、そのまま。
    pub(super) fn without_native(&mut self, thread: u32, value: ComPtr<ICorDebugValue>, operand: &Operand) -> Result<ComPtr<ICorDebugValue>, String> {
        match operand {
            Operand::Int { value: number, kind: IntKind::IPtr } => self.operand_value(thread, Operand::Int { value: *number, kind: IntKind::I64 }),
            Operand::Int { value: number, kind: IntKind::UPtr } => self.operand_value(thread, Operand::Int { value: *number, kind: IntKind::U64 }),
            _ => Ok(value),
        }
    }

    /// 値を、`owner` の型へ暗黙に変換する（`owner` が宣言している `op_Implicit`）。できなければ `None`。
    pub(super) fn implicitly_converted(&mut self, thread: u32, owner: (usize, u32), value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        let owner_name = self.plain_type_name(owner.0, owner.1);
        let candidates = self.conversion_candidates(owner, &["op_Implicit"], |metadata, token| metadata.return_type(token, &[]).is_some_and(|shape| shape.name == owner_name));
        let mut arguments = [value.clone()];
        let callable = self.choose_overload(thread, candidates, &mut arguments)?;
        self.call(thread, &callable, &arguments).ok().flatten()
    }

    /// `(型)値` を、利用者が定義した変換（`op_Implicit` / `op_Explicit`）で行う。当てはまる変換が無ければ `Ok(None)`。
    pub(super) fn user_conversion(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>, target: &ResolvedType) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        // 変換を宣言しているのは、変換元か変換先の型。
        let mut owners: Vec<(usize, u32)> = self.user_type(value).into_iter().collect();
        if let ResolvedType::Named { module, row, .. } = target {
            if !owners.contains(&(*module, *row)) {
                owners.push((*module, *row));
            }
        }
        for owner in owners {
            let candidates = match target {
                ResolvedType::Primitive(element) => {
                    let wanted = ParameterKind::Primitive(*element);
                    self.conversion_candidates(owner, &["op_Implicit", "op_Explicit"], |metadata, token| metadata.return_kind(token) == Some(wanted))
                }
                ResolvedType::Named { module, row, .. } => {
                    let wanted = self.plain_type_name(*module, *row);
                    self.conversion_candidates(owner, &["op_Implicit", "op_Explicit"], |metadata, token| metadata.return_type(token, &[]).is_some_and(|shape| shape.name == wanted))
                }
                ResolvedType::String | ResolvedType::Object => Vec::new(),
            };
            let mut arguments = [value.clone()];
            let Some(callable) = self.choose_overload(thread, candidates, &mut arguments) else { continue };
            return match self.call(thread, &callable, &arguments) {
                Ok(Some(result)) => Ok(Some(result)),
                Ok(None) => Err(tr!("The conversion returned no value.")),
                Err(failure) => Err(tr!("Cannot convert: {reason}", reason = failure.describe(self))),
            };
        }
        Ok(None)
    }

    /// 文字列を、`System.String` が宣言している変換（`op_Implicit` / `op_Explicit`。`ReadOnlySpan<char>` へ）で、型 `ty` に
    /// する。当てはまる変換が無ければ `Ok(None)`。
    pub(super) fn string_conversion(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, ty: &TypeSyntax) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let Some(owner) = self.find_type(thread, depth, "System.String") else { return Ok(None) };
        // 書かれた名前（`ReadOnlySpan`）を、完全な名前にする。
        let wanted = match self.resolve_type(thread, depth, &TypeSyntax { nullable: false, ..ty.clone() }) {
            Ok(ResolvedType::Named { module, row, .. }) => self.plain_type_name(module, row),
            _ => return Ok(None),
        };
        let written: Vec<String> = ty.arguments.iter().map(lambda::type_syntax_text).collect();
        let candidates = self.conversion_candidates(owner, &["op_Implicit", "op_Explicit"], |metadata, token| metadata.return_type(token, &[]).is_some_and(|shape| shape.name == wanted && calls::same_types(&shape.arguments, &written)));
        let mut arguments = [value.clone()];
        let Some(callable) = self.choose_overload(thread, candidates, &mut arguments) else { return Ok(None) };
        match self.call(thread, &callable, &arguments) {
            Ok(Some(result)) => Ok(Some(result)),
            Ok(None) => Err(tr!("The conversion returned no value.")),
            Err(failure) => Err(tr!("Cannot convert: {reason}", reason = failure.describe(self))),
        }
    }

    /// 型が宣言している、引数 1 個の静的メソッドのうち、名前が `names` のどれかで、`accept` が認めたもの。
    fn conversion_candidates(&self, owner: (usize, u32), names: &[&str], accept: impl Fn(&Metadata, u32) -> bool) -> Vec<(Callable, Vec<ParameterKind>)> {
        let Some(metadata) = self.modules[owner.0].metadata() else { return Vec::new() };
        let mut candidates = Vec::new();
        for name in names {
            for token in metadata.find_methods_shaped(owner.1, name, true, 1) {
                if !accept(metadata, token) {
                    continue;
                }
                let mut raw = null_mut();
                unsafe { self.modules[owner.0].module.GetFunctionFromToken(token, &mut raw) };
                if let (Some(kinds), Some(function)) = (metadata.parameter_kinds(token), unsafe { ComPtr::<ICorDebugFunction>::from_raw(raw) }) {
                    candidates.push((Callable { function, type_arguments: Vec::new() }, kinds));
                }
            }
        }
        candidates
    }

    /// `decimal` のリテラル（`1.5m`）の値を作る。対象の `decimal.Parse(文字列, CultureInfo.InvariantCulture)` を実行する
    /// （小数点の書き方が、対象の地域の設定に左右されないように）。末尾の 0 も保たれる（`1.50m` は 1.50）。
    pub(super) fn new_decimal(&mut self, thread: u32, depth: usize, digits: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let unavailable = |what: &str| tr!("Cannot create the decimal literal ({name} is not found in the target).", name = what);
        let (culture_module, culture_row) = self.find_type(thread, depth, "System.Globalization.CultureInfo").ok_or_else(|| unavailable("System.Globalization.CultureInfo"))?;
        let culture = self.static_member(thread, depth, culture_module, culture_row, "InvariantCulture")?.ok_or_else(|| unavailable("CultureInfo.InvariantCulture"))?;
        let (module, row) = self.find_type(thread, depth, "System.Decimal").ok_or_else(|| unavailable("System.Decimal"))?;
        let text = self.new_string(thread, digits).map_err(|failure| failure.describe(self))?;
        self.call_method(thread, depth, Some(Current::Type { module, row }), "Parse", vec![text, culture])?.ok_or_else(|| tr!("Cannot create the decimal literal."))
    }

    /// 値を、対象の `ToString()` で文字列にする（文字列の連結で、C# がすることと同じ）。
    pub(super) fn call_to_string(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>) -> Option<String> {
        let text = self.call_method(thread, depth, Some(Current::Value(value.clone())), "ToString", Vec::new()).ok()??;
        unsafe { value::as_string(&text) }
    }
}
