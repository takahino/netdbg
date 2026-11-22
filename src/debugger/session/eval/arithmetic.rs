//! 演算子の評価: 単項・二項演算子、文字列の連結と補間、対象の値とエンジンの側の値（`Operand`）の行き来。
//!
//! 計算そのものは `operators.rs`（対象を動かさない）。ここは、式を評価して被演算子にし、C# の規則（定数の変換、null 許容、
//! 利用者が定義した演算子、文字列の連結）のどれを使うかを決め、結果を対象の中の値に戻す。

use super::*;

impl Session {
    /// 単項演算子。
    pub(super) fn eval_unary(&mut self, thread: u32, depth: usize, op: &operators::UnaryOp, operand: &Expr) -> Result<Current, String> {
        Ok({
            let (value, operand, nullable) = self.evaluate_valued(thread, depth, operand)?;
            // 利用者が定義した演算子（decimal も）。
            if self.user_type(&value).is_some() {
                if let Some(result) = self.user_operator(thread, userops::unary_operator_name(*op), std::slice::from_ref(&value))? {
                    return Ok(Current::Value(result));
                }
            }
            // 値の無い null 許容の値でも、演算子が中身の型に使えることは要る（`-perhapsNot` は、C# でもエラー）。
            if let Some(probe) = self.probe(&value, &operand, nullable) {
                operators::unary(*op, &probe)?;
            }
            let result = operators::unary_lifted(*op, &operand, nullable)?;
            self.lifted_value(thread, result)?
        })
    }

    /// `x == default`: 型を書かない default は、相手の型の既定値。
    pub(super) fn eval_binary_with_default(&mut self, thread: u32, depth: usize, op: &BinaryOp, left: &Expr, right: &Expr) -> Result<Current, String> {
        Ok({
            let default_on_left = matches!(*left, Expr::DefaultLiteral);
            let other = if default_on_left { right } else { left };
            let (other_value, other_operand, other_nullable) = self.evaluate_valued(thread, depth, other)?;
            let default = self.default_like(thread, &other_value, other_nullable)?;
            let default_operand = self.operand(&default);
            if self.user_type(&other_value).is_some() {
                if let Some(name) = userops::binary_operator_name(*op) {
                    let arguments = if default_on_left { vec![default, other_value] } else { vec![other_value, default] };
                    if let Some(result) = self.user_operator(thread, name, &arguments)? {
                        return Ok(Current::Value(result));
                    }
                }
            }
            let result = if default_on_left {
                operators::binary_lifted(*op, &default_operand, other_nullable, &other_operand, other_nullable)?
            } else {
                operators::binary_lifted(*op, &other_operand, other_nullable, &default_operand, other_nullable)?
            };
            self.lifted_value(thread, result)?
        })
    }

    /// 二項演算子。
    pub(super) fn eval_binary(&mut self, thread: u32, depth: usize, op: &BinaryOp, left: &Expr, right: &Expr) -> Result<Current, String> {
        Ok({
            let (left_value, left_operand, left_nullable) = self.evaluate_valued(thread, depth, left)?;
            // `&&` と `||` は、左だけで決まるなら右を評価しない（右に副作用のある呼び出しがあっても、実行しない）。
            let result = match (op, &left_operand) {
                (BinaryOp::And, Operand::Bool(false)) | (BinaryOp::Or, Operand::Bool(true)) if !left_nullable => {
                    // 右辺は実行しないが、bool でなければならない（`yes || i` は、C# でもエラー）。呼び出しを含まなければ、確かめる。
                    if repeatable(right) {
                        match self.evaluate_typed(thread, depth, right) {
                            Ok((right_operand, right_nullable)) => {
                                operators::binary_lifted(*op, &left_operand, false, &right_operand, right_nullable)?;
                            }
                            // 右辺の中の演算子が型に合わない（`yes || no + yes`）のは、C# でもエラー。値のせいの失敗
                            // （null の参照、0 での割り算）は、実行しない右辺のことなので問わない。
                            Err(error) if operators::is_type_error(&error) => return Err(error),
                            Err(_) => {}
                        }
                    }
                    operators::Lifted { value: left_operand.clone(), nullable: false }
                }
                // null のリテラルどうし。C# は int? の演算子を選ぶ（`null - null` は int? の null、`null < null` は false）。
                // + と & | ^ は、文字列や bool? の演算子ともあいまいで、C# でもエラー。
                _ if *left == Expr::Null && *right == Expr::Null && !matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) => match op {
                    BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder | BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::ShiftRightUnsigned => operators::Lifted { value: Operand::Null, nullable: true },
                    BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual => operators::Lifted { value: Operand::Bool(false), nullable: false },
                    _ => return Err(tr!("Using operator {operator} on null and null does not tell which type's operator it is.", operator = operators::symbol(*op))),
                },
                _ => {
                    let (right_value, right_operand, right_nullable) = self.evaluate_valued(thread, depth, right)?;
                    // ポインターの足し引きと比べ算。
                    if let Some(result) = self.pointer_binary(thread, *op, &left_value, *left == Expr::Null, &right_value, *right == Expr::Null)? {
                        return Ok(result);
                    }
                    let shift =matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::ShiftRightUnsigned);
                    // 片方が null のリテラルで、もう片方が bool か参照型なら、使える + は文字列の連結だけ
                    // （`yes + null` は "True"、`boxed + null` は "42"。null は string になる）。
                    let null_concat = *op == BinaryOp::Add
                        && ((*right == Expr::Null && self.concatenates_with_null(&left_value, left_nullable)) || (*left == Expr::Null && self.concatenates_with_null(&right_value, right_nullable)));
                    let concat = *op == BinaryOp::Add && (null_concat || self.string_typed(&left_value, &left_operand) || self.string_typed(&right_value, &right_operand));
                    // decimal と、null 許容の値・null・nint の組み合わせ。
                    if !concat {
                        if let Some(result) = self.struct_binary(thread, *op, &left_value, left_nullable, &right_value, right_nullable)? {
                            return Ok(result);
                        }
                    }
                    // 利用者が定義した演算子（decimal も）。文字列の連結は、相手が何であっても連結。
                    if !concat && (self.user_type(&left_value).is_some() || self.user_type(&right_value).is_some()) {
                        if let Some(name) = userops::binary_operator_name(*op) {
                            if let Some(result) = self.user_operator(thread, name, &[left_value.clone(), right_value.clone()])? {
                                return Ok(Current::Value(result));
                            }
                        }
                    }
                    let (left_operand, right_operand) = if shift {
                        // シフトの両辺は、互いに型を合わせない（ずらす数は、いつも int）。
                        (left_operand, right_operand)
                    } else if concat {
                        // 連結では、null は空の文字列。
                        let text = |operand: Operand| if operand == Operand::Null { Operand::Str(String::new()) } else { operand };
                        (text(self.concat_operand(thread, depth, &left_value, left_operand)?), text(self.concat_operand(thread, depth, &right_value, right_operand)?))
                    } else {
                        // 負でない整数の定数は、相手の整数の型に合わせる（C# の定数の暗黙の変換。`u + 1` は uint のまま）。
                        // 列挙型と定数の 0 の足し算は `列挙型 + 数`。引き算では、Roslyn は 0 を列挙型にして
                        // `列挙型 - 列挙型`（結果は数）を選ぶ。ただし、もとの型が int の列挙型から 0 を引くときだけは
                        // `列挙型 - int`（結果は列挙型）。どれも総当たり試験のコンパイラの結果から。
                        let zero_as_enum = match op {
                            BinaryOp::Add => false,
                            BinaryOp::Subtract => !(*right == Expr::Int(0) && matches!(left_operand, Operand::Enum { underlying: IntKind::I32, .. })),
                            _ => true,
                        };
                        // ここに来た文字列は、宣言の型が object のもの。+ には、object どうしの演算子は無い。
                        let not_text = |operand: Operand| if *op == BinaryOp::Add && matches!(operand, Operand::Str(_)) { Operand::Reference(0) } else { operand };
                        let (left_operand, right_operand) = (not_text(left_operand), not_text(right_operand));
                        let left_operand = constant_as(left, left_operand, &right_operand, zero_as_enum);
                        let right_operand = constant_as(right, right_operand, &left_operand, zero_as_enum);
                        (left_operand, right_operand)
                    };
                    if operators::overflows(*op, &left_operand, &right_operand) {
                        if self.checked == Some(true) {
                            return Err(tr!("A calculation inside checked went out of the type's range."));
                        }
                        // 定数どうしの計算があふれるのは、C# ではコンパイルのエラー（`0 - 1u` など）。
                        if self.checked.is_none() && is_constant(left) && is_constant(right) {
                            return Err(tr!("A calculation between constants goes out of the type's range (a compile error in C#; wrap it in unchecked(…) to wrap around)."));
                        }
                    }
                    // 値の無い null 許容の値でも、演算子が中身の型に使えることは要る（`perhapsNot - perhapsNot` は、
                    // C# でもエラー）。相手が null のリテラルなら、同じ型の値を相手にして確かめる。
                    let left_probe = self.probe(&left_value, &left_operand, left_nullable);
                    let right_probe = self.probe(&right_value, &right_operand, right_nullable);
                    if left_probe.is_some() || right_probe.is_some() {
                        let left_check = left_probe.clone().or_else(|| (left_operand == Operand::Null).then(|| right_probe.clone()).flatten()).unwrap_or_else(|| left_operand.clone());
                        let right_check = right_probe.or_else(|| (right_operand == Operand::Null).then_some(left_probe).flatten()).unwrap_or_else(|| right_operand.clone());
                        operators::binary(*op, &left_check, &right_check)?;
                    }
                    operators::binary_lifted(*op, &left_operand, left_nullable, &right_operand, right_nullable)?
                }
            };
            self.lifted_value(thread, result)?
        })
    }

    /// 補間文字列（`$"{a}-{b:F2}"`）。
    pub(super) fn eval_interpolated(&mut self, thread: u32, depth: usize, pieces: &[Piece]) -> Result<Current, String> {
        Ok({
            let mut text = String::new();
            for piece in pieces {
                match piece {
                    Piece::Text(part) => text.push_str(part),
                    Piece::Hole { expr, alignment, format } => {
                        let current = self.evaluate_node(thread, depth, expr)?;
                        let value = self.value_of(current)?;
                        text.push_str(&self.formatted(thread, depth, &value, alignment.as_deref(), format.as_deref())?);
                    }
                }
            }
            Current::Value(self.new_string(thread, &text).map_err(|failure| failure.describe(self))?)
        })
    }

    /// 式を評価して、演算子に使える形（エンジンの側の値）にする。
    pub(super) fn evaluate_operand(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<Operand, String> {
        let current = self.evaluate_node(thread, depth, expr)?;
        let value = self.value_of(current)?;
        Ok(self.operand(&value))
    }

    /// 式を評価して、演算子に使える形にする。2 番目は、式の型が null 許容の値型（`int?`）かどうか。
    pub(super) fn evaluate_typed(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<(Operand, bool), String> {
        let current = self.evaluate_node(thread, depth, expr)?;
        let lifted = matches!(current, Current::Nullable(_));
        let value = self.value_of(current)?;
        let nullable = lifted || unsafe { value::nullable(&value, &self.modules) }.is_some();
        Ok((self.operand(&value), nullable))
    }

    /// 式を評価して、値と、演算子に使える形と、式の型が null 許容の値型かどうかを返す。
    pub(super) fn evaluate_valued(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<(ComPtr<ICorDebugValue>, Operand, bool), String> {
        let current = self.evaluate_node(thread, depth, expr)?;
        let lifted = matches!(current, Current::Nullable(_));
        let value = self.value_of(current)?;
        let nullable = lifted || unsafe { value::nullable(&value, &self.modules) }.is_some();
        let operand = self.operand(&value);
        Ok((value, operand, nullable))
    }

    /// 計算の結果を、対象の中の値にする。null 許容の値型の結果は、その印を付ける。
    pub(super) fn lifted_value(&mut self, thread: u32, result: operators::Lifted) -> Result<Current, String> {
        let value = self.operand_value(thread, result.value)?;
        Ok(if result.nullable { Current::Nullable(value) } else { Current::Value(value) })
    }

    /// 値の無い null 許容の値（`perhapsNot`）の代わりに、演算子がその型に使えるかを確かめるための、中身の型の値。
    /// bool、整数、浮動小数点のときだけ（値は 1。0 での割り算にならないように）。
    pub(super) fn probe(&self, value: &ComPtr<ICorDebugValue>, operand: &Operand, nullable: bool) -> Option<Operand> {
        if !nullable || *operand != Operand::Null {
            return None;
        }
        match self.typed_of(None, value, true).kind {
            Kind::Bool => Some(Operand::Bool(true)),
            Kind::Int(kind) => Some(Operand::Int { value: 1, kind }),
            Kind::Float { single } => Some(Operand::Float { value: 1.0, single }),
            _ => None,
        }
    }

    /// 式の型が string か。null の値も、変数の型が string なら string（`none + none` は、空の文字列の連結）。
    /// 文字列の入った変数でも、宣言の型が `object` なら string ではない（`boxed + boxedText` は、C# でもエラー）。
    pub(super) fn string_typed(&self, value: &ComPtr<ICorDebugValue>, operand: &Operand) -> bool {
        if self.declared_object(value) {
            return false;
        }
        match operand {
            Operand::Str(_) => true,
            Operand::Null => unsafe { value::argument_kind(value) == ArgumentKind::Null && value::describe(value, &self.modules).type_name == "string" },
            _ => false,
        }
    }

    /// 宣言の型が `object` の変数・フィールドから読んだ値か。ICorDebug の参照の値は、中身の実体の型しか教えないので、
    /// 読んだ所でメタデータの宣言の型を見て覚えておく（式を一つ評価し終えたら忘れる）。
    pub(super) fn declared_object(&self, value: &ComPtr<ICorDebugValue>) -> bool {
        self.object_typed.iter().any(|known| known.as_raw() == value.as_raw())
    }

    pub(super) fn remember_object_typed(&mut self, value: &ComPtr<ICorDebugValue>, kind: Option<ParameterKind>) {
        if kind == Some(ParameterKind::Object) {
            self.object_typed.push(value.clone());
        }
    }

    /// `値 + null` が文字列の連結になる値か。bool（bool? も）と参照型には、null と組める + が string の演算子しか無い。
    /// 数、列挙型、構造体は、null 許容に持ち上げた演算子になる。
    pub(super) fn concatenates_with_null(&self, value: &ComPtr<ICorDebugValue>, nullable: bool) -> bool {
        matches!(self.typed_of(None, value, nullable).kind, Kind::Bool | Kind::Object | Kind::Class(_)) || self.declared_object(value)
    }

    /// 文字列の連結で、文字列でない側を、連結できる形にする。整数、bool、null は、エンジンの側で文字列にする。
    /// そのほか（浮動小数点、列挙型、decimal、オブジェクト）は、C# と同じく、対象の `ToString()` を実行する。
    pub(super) fn concat_operand(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, operand: Operand) -> Result<Operand, String> {
        // `object` に入った数は、中身の数として書く。
        let operand = if matches!(operand, Operand::Reference(_)) { self.operand_unboxed(value) } else { operand };
        match operand {
            Operand::Str(_) | Operand::Null | Operand::Bool(_) | Operand::Int { .. } => Ok(operand),
            other => self.call_to_string(thread, depth, value).map(Operand::Str).ok_or_else(|| tr!("A value of {type} cannot be made a string.", type = operators::kind_name(&other))),
        }
    }

    /// 補間文字列の `{値,幅:書式}` を文字列にする。幅も書式も無ければ、文字列の連結と同じ。あれば、対象の
    /// `string.Format` に任せる（書式の解釈は、型ごとに違う）。
    pub(super) fn formatted(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, alignment: Option<&str>, format: Option<&str>) -> Result<String, String> {
        if alignment.is_none() && format.is_none() {
            let operand = self.operand(value);
            return Ok(match self.concat_operand(thread, depth, value, operand)? {
                Operand::Str(text) => text,
                Operand::Null => String::new(),
                other => match operators::binary(BinaryOp::Add, &Operand::Str(String::new()), &other)? {
                    Operand::Str(text) => text,
                    _ => String::new(),
                },
            });
        }
        let mut pattern = String::from("{0");
        if let Some(alignment) = alignment {
            pattern.push_str(&format!(",{alignment}"));
        }
        if let Some(format) = format {
            pattern.push_str(&format!(":{format}"));
        }
        pattern.push('}');
        let (module, row) = self.find_type(thread, depth, "System.String").ok_or_else(|| tr!("System.String is not found in the target."))?;
        let pattern = self.new_string(thread, &pattern).map_err(|failure| failure.describe(self))?;
        let text = self.call_method(thread, depth, Some(Current::Type { module, row }), "Format", vec![pattern, value.clone()])?.ok_or_else(|| tr!("string.Format returned no value."))?;
        unsafe { value::as_string(&text) }.ok_or_else(|| tr!("Cannot apply the format of the interpolated string."))
    }

    pub(super) fn operand(&self, value: &ComPtr<ICorDebugValue>) -> Operand {
        unsafe {
            // Nullable<T> は、値が無ければ null、あれば中身として比べる・計算する。
            match value::nullable(value, &self.modules) {
                Some(None) => return Operand::Null,
                Some(Some(inner)) => return self.operand(&inner),
                None => {}
            }
            match value::argument_kind(value) {
                ArgumentKind::Null => Operand::Null,
                ArgumentKind::String => Operand::Str(value::as_string(value).unwrap_or_default()),
                ArgumentKind::Primitive(element) => primitive_operand(element, &value::primitive_bytes(value).map(|(_, bytes)| bytes).unwrap_or_default()),
                // `object` に入った数（ボックス）は、ただのオブジェクト。C# でも、キャストしないと計算できない。
                // ただし nint / nuint は、計算の結果をボックスとしてしか作れないので、ボックスでも数として扱う。
                ArgumentKind::Reference if value::unboxed_primitive(value, &self.modules).is_some() => match value::unboxed_primitive(value, &self.modules) {
                    Some((element @ (0x18 | 0x19), bytes)) => primitive_operand(element as u8, &bytes),
                    _ => Operand::Reference(value::reference_address(value).unwrap_or_default()),
                },
                kind => match (self.enum_operand(value), value::reference_address(value)) {
                    (Some(operand), _) => operand,
                    // 構造体（decimal、DateTime など）は、ボックスとして見えていても参照ではない。同じものかどうかでは比べない。
                    (None, Some(address)) if kind == ArgumentKind::Reference && !value::type_names(value, &self.modules).iter().any(|name| name == "System.ValueType") => Operand::Reference(address),
                    _ => Operand::Other(value::describe(value, &self.modules).type_name),
                },
            }
        }
    }

    /// ボックス化されたプリミティブ型の値を、中身の値に作り直す。そうでなければ、そのまま。
    pub(in crate::debugger::session) fn unboxed(&self, thread: u32, value: ComPtr<ICorDebugValue>) -> ComPtr<ICorDebugValue> {
        match unsafe { value::unboxed_primitive(&value, &self.modules) } {
            Some((element, bytes)) => self.new_primitive(thread, element, &bytes).unwrap_or(value),
            None => value,
        }
    }

    /// `object` に入った数は、中身の数として読む（キャストとパターンで使う。演算子では使わない）。
    pub(super) fn operand_unboxed(&self, value: &ComPtr<ICorDebugValue>) -> Operand {
        match unsafe { value::unboxed_primitive(value, &self.modules) } {
            Some((element, bytes)) => primitive_operand(element as u8, &bytes),
            None => self.operand(value),
        }
    }

    /// 列挙型の値を、型と中の数にする。
    pub(super) fn enum_operand(&self, value: &ComPtr<ICorDebugValue>) -> Option<Operand> {
        let (module, token, element, bytes) = unsafe { value::enum_parts(value, &self.modules) }?;
        let module = self.module_index(&module)?;
        let row = token & 0x00FF_FFFF;
        let Operand::Int { value, kind } = primitive_operand(element as u8, &bytes) else { return None };
        Some(Operand::Enum { value, underlying: kind, ty: EnumType { name: csharp_type_name(&self.plain_type_name(module, row)), module, row } })
    }

    /// 演算の結果を、対象の中の値にする。
    pub(super) fn operand_value(&mut self, thread: u32, operand: Operand) -> Result<ComPtr<ICorDebugValue>, String> {
        match operand {
            Operand::Bool(flag) => self.new_bool(thread, flag),
            Operand::Int { value, kind } => self.new_number(thread, kind.element(), &kind.bytes(value)),
            Operand::Enum { value, ty, .. } => {
                let boxed = self.zero_value(thread, ty.module, ty.row)?;
                self.set_enum_bits(&boxed, ty.module, ty.row, value as u64)?;
                Ok(boxed)
            }
            Operand::Float { value, single: true } => self.new_primitive(thread, ELEMENT_TYPE_R4 as u32, &(value as f32).to_le_bytes()),
            Operand::Float { value, single: false } => self.new_primitive(thread, ELEMENT_TYPE_R8 as u32, &value.to_le_bytes()),
            Operand::Str(text) => self.new_string(thread, &text).map_err(|failure| failure.describe(self)),
            Operand::Null => self.new_null(thread),
            // 演算の結果がオブジェクトになることは無い（利用者が定義した演算子は呼ばない）。
            Operand::Reference(_) | Operand::Other(_) => Err(tr!("The result of this operation cannot be made a value.")),
        }
    }

    /// プリミティブ型の値を作る。nint / nuint は、じかには作れない（`ICorDebugEval::CreateValue` が受け付けない）ので、
    /// 空のボックスを作って中身を書き込む。
    pub(super) fn new_number(&mut self, thread: u32, element: u8, bytes: &[u8]) -> Result<ComPtr<ICorDebugValue>, String> {
        if !matches!(element, 0x18 | 0x19) {
            return self.new_primitive(thread, element as u32, bytes);
        }
        let name = primitive_type_name(element).unwrap_or_default();
        let (module, row) = self.modules.iter().enumerate().find_map(|(index, m)| Some((index, m.metadata()?.find_type(name)?))).ok_or_else(|| tr!("{name} is not found in the target.", name = name))?;
        let field = self.modules[module].metadata().and_then(|m| m.fields(row).into_iter().find(|f| !f.is_static).map(|f| f.token)).ok_or_else(|| tr!("Cannot create the nint value."))?;
        let boxed = self.zero_value(thread, module, row)?;
        let class = unsafe { take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | row, out)) }?;
        unsafe { value::fill_box(&boxed, &class, field, bytes) }?;
        Ok(boxed)
    }
}

/// C# の定数式か（リテラルと、それに演算子と組み込みの型へのキャストを組み合わせたもの）。定数の名前（`const`、
/// 列挙型のメンバー）は見ない。
pub(super) fn is_constant(expr: &Expr) -> bool {
    match expr {
        Expr::Int(_) | Expr::Long(_) | Expr::UInt(_) | Expr::ULong(_) | Expr::Char(_) | Expr::Double(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Decimal(_) => true,
        Expr::Unary(_, inner) | Expr::Checked(_, inner) => is_constant(inner),
        Expr::Cast(ty, inner) => primitive_element(&ty.name).is_some() && !ty.nullable && !ty.array && is_constant(inner),
        Expr::Binary(_, left, right) => is_constant(left) && is_constant(right),
        _ => false,
    }
}

/// プリミティブ型の値のバイト列を、演算子に使える形にする。
pub(super) fn primitive_operand(element: u8, bytes: &[u8]) -> Operand {
    let mut wide = [0u8; 8];
    wide[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
    let raw = u64::from_le_bytes(wide);
    match element {
        0x02 => Operand::Bool(raw != 0),
        0x0C => Operand::Float { value: f32::from_bits(raw as u32) as f64, single: true },
        0x0D => Operand::Float { value: f64::from_bits(raw), single: false },
        // 整数は、もとの型を覚えておく（`byte + uint` は uint、`sbyte + uint` は long、のように結果の型が変わる）。
        _ => {
            let kind = IntKind::from_element(element).unwrap_or(IntKind::I64);
            Operand::Int { value: kind.wrap(raw as i128), kind }
        }
    }
}

/// 式が負でない `int` の定数で、相手が `uint` / `ulong` なら、その型の値として扱う。
/// C# では、定数は値が収まる限り相手の型に暗黙に変換される。変数の `int` は変換されない（`uint` と `int` は `long` になる）。
///
/// 列挙型が相手のとき: 定数の 0 は、その列挙型の値になる（`tone == 0`）。ほかの定数は、列挙型のもとになる整数の型に
/// 収まれば、その型の数になる（`flags + 1`。`Flags` が `byte` でも足せる）。
/// `zero_as_enum` が false なら、定数の 0 を列挙型にせず、もとになる整数の型にする（`tone + 0` は `列挙型 + 数`）。
pub(super) fn constant_as(expr: &Expr, operand: Operand, other: &Operand, zero_as_enum: bool) -> Operand {
    match (expr, &operand, other) {
        (Expr::Int(0), _, Operand::Enum { .. }) if !zero_as_enum => match other {
            Operand::Enum { underlying, .. } => Operand::Int { value: 0, kind: *underlying },
            _ => operand,
        },
        (Expr::Int(number), Operand::Int { value, .. }, Operand::Int { kind: kind @ (IntKind::U32 | IntKind::U64 | IntKind::UPtr), .. }) if *number >= 0 => Operand::Int { value: *value, kind: *kind },
        // long の定数も、負でなければ ulong になれる（`ul + 4294967296`、`1L + ul`）。
        (Expr::Long(number), Operand::Int { value, .. }, Operand::Int { kind: IntKind::U64, .. }) if *number >= 0 => Operand::Int { value: *value, kind: IntKind::U64 },
        (Expr::Int(0), _, Operand::Enum { underlying, ty, .. }) => Operand::Enum { value: 0, underlying: *underlying, ty: ty.clone() },
        (Expr::Int(_), Operand::Int { value, .. }, Operand::Enum { underlying, .. }) if underlying.wrap(*value) == *value => Operand::Int { value: *value, kind: *underlying },
        _ => operand,
    }
}
