//! 型の変換と検査: キャスト、`as`、`is`、`typeof`、条件演算子と `??` の型の統一、式の型（`Typed`）の割り出し。

use super::*;

impl Session {
    /// 条件演算子（`条件 ? 真のとき : 偽のとき`）。
    pub(super) fn eval_conditional(&mut self, thread: u32, depth: usize, condition: &Expr, when_true: &Expr, when_false: &Expr) -> Result<Current, String> {
        Ok({
            let flag = match self.evaluate_typed(thread, depth, condition)? {
                // bool? は、bool へ暗黙に変換できない（`perhaps ? 1 : 2` は、C# でもエラー）。
                (_, true) => return Err(tr!("The condition of the conditional operator must be bool (make a bool? into bool with == true, for example).")),
                (Operand::Bool(flag), false) => flag,
                (other, false) => return Err(tr!("The condition of the conditional operator must be bool (it is {type}).", type = operators::kind_name(&other))),
            };
            let (chosen, other) = if flag { (when_true, when_false) } else { (when_false, when_true) };
            let current = self.evaluate_node(thread, depth, chosen)?;
            // 投げた側が選ばれたら、型を揃えずに、その例外が結果。
            if matches!(*chosen, Expr::Throw(_)) {
                return Ok(current);
            }
            if !matches!(current, Current::Value(_) | Current::Nullable(_)) || !repeatable(other) {
                return Ok(current);
            }
            let lifted = matches!(current, Current::Nullable(_));
            let value = self.value_of(current)?;
            // 結果の型は、両側の型から決まる（`flag ? 1 : 2.5` は double）。選ばれなかった側は、型を知るためだけに
            // 評価する。呼び出しを含む式は、実行すると何かが変わるかもしれないので評価しない（型は、選ばれた側のまま）。
            let chosen_type = self.typed_of(Some(chosen), &value, lifted);
            match self.evaluate_node(thread, depth, other) {
                Ok(other_current @ (Current::Value(_) | Current::Nullable(_))) => {
                    let other_lifted = matches!(other_current, Current::Nullable(_));
                    let other_value = self.value_of(other_current)?;
                    let other_type = self.typed_of(Some(other), &other_value, other_lifted);
                    match types::unify(&chosen_type, &other_type) {
                        Some(result) => self.convert_to(thread, value, &chosen_type, &result)?,
                        // 参照型どうしは、実行時の型しか分からないので、通す（基底の型が同じかもしれない）。
                        None if !chosen_type.is_value_type() && !other_type.is_value_type() && (chosen_type.kind != Kind::Null || other_type.kind != Kind::Null) => Current::Value(value),
                        None => return Err(tr!("The two sides of the conditional operator cannot be made one type (cast one of them).")),
                    }
                }
                _ => if lifted { Current::Nullable(value) } else { Current::Value(value) },
            }
        })
    }

    /// `左 ?? 右`。
    pub(super) fn eval_coalesce(&mut self, thread: u32, depth: usize, left: &Expr, right: &Expr) -> Result<Current, String> {
        Ok({

            let current = self.evaluate_node(thread, depth, left)?;
            let lifted = matches!(current, Current::Nullable(_));
            let value = self.value_of(current)?;
            let left_type = self.typed_of(Some(left), &value, lifted);
            // 左辺は、null になれる型（参照型か、null 許容の値型）でなければならない（`i ?? j` は、C# でもエラー）。
            if left_type.never_null() {
                return Err(tr!("The left side of ?? is a non-nullable type ({type}).", type = unsafe { value::describe(&value, &self.modules) }.type_name));
            }
            let (is_null, inner) = match unsafe { (value::argument_kind(&value), value::nullable(&value, &self.modules)) } {
                (ArgumentKind::Null, _) | (_, Some(None)) => (true, value.clone()),
                // 値のある Nullable<T> は、中身の T になる。
                (_, Some(Some(inner))) => (false, inner),
                _ => (false, value.clone()),
            };
            // 右辺は、左辺が null のときだけ使う。そうでなくても、結果の型を知るために評価する（呼び出しを含まなければ）。
            // `a ?? throw 式`: a が null なら例外が結果。null でなければ a（値のある Nullable<T> は、中身の T）。
            if let Expr::Throw(_) = right {
                return Ok(if is_null { self.evaluate_node(thread, depth, right)? } else { self.convert_to(thread, inner, &left_type.underlying(), &left_type.underlying())? });
            }
            let right_current = if is_null {
                Some(self.evaluate_node(thread, depth, right)?)
            } else if repeatable(right) {
                self.evaluate_node(thread, depth, right).ok()
            } else {
                None
            };
            match right_current {
                Some(right_current @ (Current::Value(_) | Current::Nullable(_))) => {
                    let right_lifted = matches!(right_current, Current::Nullable(_));
                    let right_value = self.value_of(right_current)?;
                    let right_type = self.typed_of(Some(right), &right_value, right_lifted);
                    // 左辺が `null` のリテラルのとき、右辺は null になれる型でなければならない（`null ?? 1` は、C# でもエラー）。
                    if *left == Expr::Null && (right_type.never_null() || right_type.kind == Kind::Null) {
                        return Err(tr!("When the left side of ?? is the null literal, the right side must be a type that can be null."));
                    }
                    let result = match types::coalesce(&left_type, &right_type) {
                        Some(result) => result,
                        // 左辺が参照型の null のときは、左辺の型が分からない。右辺の型とする。
                        None if !left_type.is_value_type() && !right_type.is_value_type() => right_type.clone(),
                        None if left_type.kind == Kind::Null => right_type.clone(),
                        None => return Err(tr!("The two sides of ?? cannot be made one type.")),
                    };
                    if is_null { self.convert_to(thread, right_value, &right_type, &result)? } else { self.convert_to(thread, inner, &left_type.underlying(), &result)? }
                }
                Some(other) => other,
                None => Current::Value(inner),
            }
        })
    }

    /// `(型)式`。
    pub(super) fn eval_cast(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, operand: &Expr) -> Result<Current, String> {
        // `(dynamic)x`: 値はそのままで、メンバーと呼び出しを、バインダで実行時に解決する印を付ける。
        if ty.name == "dynamic" && ty.arguments.is_empty() && !ty.array {
            let current = self.evaluate_node(thread, depth, operand)?;
            let value = self.value_of(current)?;
            let value = self.unwrapped(value);
            self.mark_dynamic(&value);
            return Ok(Current::Value(value));
        }
        Ok({
            let current = match self.evaluate_node(thread, depth, operand).and_then(|current| match current {
                // 名前が値でも型でもない（メソッドの集まりかもしれない）。
                Current::Namespace(name) => Err(tr!("No variable, member or type named '{name}' was found.", name = name)),
                other => Ok(other),
            }) {
                Ok(current) => current,
                // メソッドの集まりをデリゲートの型にキャストする（`(Func<string, int>)int.Parse`）。
                Err(error) => {
                    let lambda = if self.is_method_group(thread, depth, operand) { self.group_lambda_for(thread, depth, ty, operand) } else { None };
                    let Some(lambda) = lambda else { return Err(error) };
                    return Ok(Current::Value(self.lambda_as(thread, depth, ty, &lambda)?));
                }
            };
            let value = self.value_of(current)?;
            // ポインターの型へのキャスト（`(byte*)p`、`(int*)0`）と、ポインターから整数へのキャスト（`(long)p`）。
            if let Some(converted) = self.pointer_cast(thread, depth, ty, &value, *operand == Expr::Null)? {
                return Ok(Current::Value(converted));
            }
            // 整数の定数を、収まらない整数の型（列挙型も）へキャストするのは、C# ではコンパイルのエラー（`(byte)(-1)`）。
            if self.checked != Some(false) && is_constant(operand) {
                if let Operand::Int { value: number, .. } = self.operand(&value) {
                    let element = match self.resolve_type(thread, depth, ty)? {
                        ResolvedType::Primitive(element) => Some(element),
                        ResolvedType::Named { module, row, kind: TypeKind::Enum } => self.enum_underlying(module, row),
                        _ => None,
                    };
                    // nint / nuint の幅は実行するまで決まらないので、コンパイラは確かめない。
                    if let Some(kind) = element.and_then(IntKind::from_element).filter(|kind| !matches!(kind, IntKind::IPtr | IntKind::UPtr)) {
                        if kind.wrap(number) != number {
                            return Err(tr!("The constant {number} does not fit in {type} (a compile error in C#; wrap it in unchecked(…) to wrap around).", number = number, type = ty.name));
                        }
                    }
                }
            }
            // string が宣言している変換（`(ReadOnlySpan<char>)"34"` は `String.op_Implicit`）。
            if self.string_typed(&value, &self.operand(&value)) && unsafe { value::argument_kind(&value) } == ArgumentKind::String {
                if let Some(converted) = self.string_conversion(thread, depth, &value, ty)? {
                    return Ok(Current::Value(converted));
                }
            }
            // string の変数の null は、値型へはキャストできない（`(int?)none` は、C# でもエラー）。
            if self.string_typed(&value, &self.operand(&value)) && !matches!(self.resolve_type(thread, depth, ty)?, ResolvedType::String | ResolvedType::Object | ResolvedType::Named { kind: TypeKind::Class, .. }) {
                return Err(tr!("A string value cannot be cast to {type}.", type = ty.name));
            }
            let converted = self.cast(thread, depth, ty, value)?;
            // `(string)boxedText` は、もう object ではない（同じ値がそのまま返ってくるので、印を外す）。
            if ty.name != "System.Object" {
                self.object_typed.retain(|known| known.as_raw() != converted.as_raw());
            }
            // `(int?)x` の型は int?。値は int のまま持つ。
            let to_nullable_value = ty.nullable && !ty.array && unsafe { value::argument_kind(&converted) } != ArgumentKind::Null;
            if to_nullable_value { Current::Nullable(converted) } else { Current::Value(converted) }
        })
    }

    /// `式 as 型`。
    pub(super) fn eval_as(&mut self, thread: u32, depth: usize, operand: &Expr, ty: &TypeSyntax) -> Result<Current, String> {
        Ok({
            let current = self.evaluate_node(thread, depth, operand)?;
            let lifted = matches!(current, Current::Nullable(_));
            let value = self.value_of(current)?;
            let resolved = self.resolve_type(thread, depth, ty)?;
            let fits = self.value_is(&value, &resolved) && self.type_arguments_match(thread, depth, &value, ty, &resolved);
            let target = Typed { nullable: ty.nullable, ..self.typed_of_resolved(&resolved) };
            if !ty.array {
                let source = self.typed_of(Some(operand), &value, lifted);
                types::as_is_allowed(&source, &target, target.is_value_type(), fits)?;
            }
            if !fits {
                Current::Value(self.new_null(thread)?)
            } else if ty.nullable && !ty.array && target.is_value_type() {
                Current::Nullable(self.unwrapped(value))
            } else {
                // `boxedText as string` は、もう object ではない。
                if ty.name != "System.Object" {
                    self.object_typed.retain(|known| known.as_raw() != value.as_raw());
                }
                // 値のある null 許容の値は、中身のボックスになる（`maybe as object` は int）。
                Current::Value(self.unwrapped(value))
            }
        })
    }

    /// `式 is パターン`。
    pub(super) fn eval_is(&mut self, thread: u32, depth: usize, operand: &Expr, pattern: &Pattern) -> Result<Current, String> {
        Ok({
            // null のリテラルに書けるのは `null is 型` だけ（型の検査。パターンは、C# でもエラー）。
            if *operand == Expr::Null {
                let is_type = match pattern {
                    Pattern::TypeOrConstant(Some(ty), _) => self.resolve_type(thread, depth, ty).is_ok(),
                    _ => false,
                };
                if !is_type {
                    return Err(tr!("A pattern cannot be written on the null literal (only null is type can)."));
                }
            }
            let current = self.evaluate_node(thread, depth, operand)?;
            let lifted = matches!(current, Current::Nullable(_));
            let value = self.value_of(current)?;
            let value = self.union_case(thread, &value).unwrap_or(value);
            let input = self.typed_of(Some(operand), &value, lifted);
            let matches = self.matches_pattern(thread, depth, &value, &input, pattern, false)?;
            Current::Value(self.new_bool(thread, matches)?)
        })
    }

    /// `typeof(型)`。
    pub(super) fn eval_typeof(&mut self, thread: u32, depth: usize, ty: &TypeSyntax) -> Result<Current, String> {
        Ok({
            // 型のオブジェクトは、対象の System.Type.GetType(名前) を呼んで得る。
            let (name, assembly) = self.reflection_name(thread, depth, ty)?;
            let name = match assembly {
                Some(assembly) => format!("{name}, {assembly}"),
                None => name,
            };
            Current::Value(self.type_named(thread, depth, &name)?)
        })
    }

    /// 対象の `System.Type.GetType(名前)` で、型のオブジェクトを得る。`name` は、リフレクションの書き方の名前。
    pub(super) fn type_named(&mut self, thread: u32, depth: usize, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let (module, row) = self.find_type(thread, depth, "System.Type").ok_or_else(|| tr!("System.Type is not found in the target."))?;
        let argument = self.new_string(thread, name).map_err(|failure| failure.describe(self))?;
        match self.call_method(thread, depth, Some(Current::Type { module, row }), "GetType", vec![argument])? {
            Some(found) if unsafe { value::argument_kind(&found) } != ArgumentKind::Null => Ok(found),
            _ => Err(tr!("Cannot get the object of the type {name}.", name = name)),
        }
    }

    /// `(型)値`。数どうしは変換し、列挙型へは数から作り、それ以外は「その型として扱えるか」を確かめる。
    pub(super) fn cast(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, value: ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        // 配列の型へ（`(byte[])parameter.Value`）: 実体の配列の型と同じなら、そのまま。null は null のまま。
        if ty.array && !ty.nullable && ty.arguments.is_empty() {
            return self.cast_to_array(ty, value);
        }
        let resolved = self.resolve_type(thread, depth, ty)?;
        let reference_target = matches!(resolved, ResolvedType::String | ResolvedType::Object | ResolvedType::Named { kind: TypeKind::Class, .. });
        // null 許容の値は、値があれば中身として変換する（`(object)maybe` は、int の値のボックス）。値が無ければ、null 許容の型と
        // 参照型へは null。null 許容でない値型へは、実行時に InvalidOperationException になる。
        let value = match unsafe { value::nullable(&value, &self.modules) } {
            Some(None) => {
                // 変換そのものが書けるかは、型で決まる（`(string)maybeNot` や `(int?)perhapsNot` は、C# でもエラー）。
                let source = self.typed_of(None, &value, true).underlying();
                let target = self.typed_of_resolved(&resolved);
                let allowed = match &target.kind {
                    Kind::Object | Kind::Class(_) => true,
                    _ if target.is_value_type() => types::explicitly_converts(&source, &target),
                    _ => false,
                };
                if !allowed {
                    return Err(tr!("A value of {from}? cannot be cast to {to}.", from = unsafe { value::exact_type_arguments(&value, &self.modules) }.into_iter().next().unwrap_or_default(), to = ty.name));
                }
                if ty.nullable || reference_target {
                    return self.new_null(thread);
                }
                return Err(tr!("The nullable value is null, so it cannot be converted to {type} (it would throw InvalidOperationException).", type = ty.name));
            }
            Some(Some(inner)) => inner,
            None => value,
        };
        let enum_underlying = match &resolved {
            ResolvedType::Named { module, row, kind: TypeKind::Enum } => Some(self.enum_underlying(*module, *row).ok_or_else(|| tr!("The underlying number type of the enum is not known."))?),
            _ => None,
        };
        // `object` に入った数は、同じ型へだけ取り出せる（`(long)boxedInt` は、実行時に InvalidCastException になる）。
        // 列挙型へは、もとになる整数の型が同じときだけ。
        if let Some((element, bytes)) = unsafe { value::unboxed_primitive(&value, &self.modules) } {
            let target = match &resolved {
                ResolvedType::Primitive(target) => Some(*target),
                _ => enum_underlying,
            };
            if let Some(target) = target {
                if element != target as u32 {
                    return Err(tr!("A boxed {from} value cannot be taken out directly as {to} (cast it to its own type first).", from = unsafe { value::describe(&value, &self.modules) }.type_name, to = ty.name));
                }
                if enum_underlying.is_none() {
                    return self.new_primitive(thread, element, &bytes);
                }
            }
        }
        let operand = if enum_underlying.is_some() { self.operand_unboxed(&value) } else { self.operand(&value) };
        // decimal との間の明示の変換。列挙型は、もとになる整数の型を経由し、nint / nuint は long / ulong を経由する
        // （decimal は、列挙型と nint への変換を宣言していない）。
        let decimal_source = operand == Operand::Other("decimal".into());
        let decimal_target = matches!(&resolved, ResolvedType::Named { module, row, .. } if self.plain_type_name(*module, *row) == "System.Decimal");
        let (value, operand) = if decimal_target && matches!(operand, Operand::Enum { .. } | Operand::Int { kind: IntKind::IPtr | IntKind::UPtr, .. }) {
            let plain = match operand {
                Operand::Enum { value: number, underlying, .. } => Operand::Int { value: number, kind: underlying },
                other => other,
            };
            let source = self.operand_value(thread, plain.clone())?;
            let source = self.without_native(thread, source, &plain)?;
            let operand = self.operand(&source);
            (source, operand)
        } else if decimal_source {
            let via = match (&resolved, enum_underlying) {
                (_, Some(element)) => Some(element),
                (ResolvedType::Primitive(0x18), _) => Some(ELEMENT_TYPE_I8),
                (ResolvedType::Primitive(0x19), _) => Some(0x0B),
                _ => None,
            };
            match via {
                Some(element) => match self.user_conversion(thread, &value, &ResolvedType::Primitive(element))? {
                    Some(converted) => {
                        let operand = self.operand(&converted);
                        (converted, operand)
                    }
                    None => (value, operand),
                },
                None => (value, operand),
            }
        } else {
            (value, operand)
        };
        match &resolved {
            ResolvedType::Primitive(element) => {
                if let Some(bytes) = operators::convert(&operand, *element) {
                    return self.new_number(thread, *element, &bytes);
                }
            }
            // 数（や、ほかの列挙型）から列挙型へ。数は、まず列挙型のもとになる整数の型に変換する（浮動小数点は、その型の端の値で止まる）。
            ResolvedType::Named { module, row, kind: TypeKind::Enum } => {
                let underlying = enum_underlying.unwrap_or(ELEMENT_TYPE_I8);
                if let (false, Some(bytes)) = (self.value_is(&value, &resolved), operators::convert(&operand, underlying)) {
                    let narrow = primitive_operand(underlying, &bytes);
                    let Operand::Int { value: number, .. } = narrow else { return Err(tr!("Cannot create the enum value.")) };
                    let boxed = self.zero_value(thread, *module, *row)?;
                    self.set_enum_bits(&boxed, *module, *row, number as i64 as u64)?;
                    return Ok(boxed);
                }
            }
            _ => {}
        }
        // 数を、インターフェイス（や object）へ。ボックス化して、その型として扱えるかを確かめる。
        if let (ResolvedType::Named { kind: TypeKind::Class, .. }, Some((module, row))) = (&resolved, self.primitive_type(&value)) {
            let boxed = self.box_primitive(thread, module, row, &value)?;
            if self.value_is(&boxed, &resolved) {
                return Ok(boxed);
            }
        }
        // 参照型への null のキャストは、null のまま。
        let is_null = unsafe { value::argument_kind(&value) } == ArgumentKind::Null;
        if self.value_is(&value, &resolved) || (is_null && (reference_target || ty.nullable)) {
            return Ok(value);
        }
        // 利用者が定義した変換（decimal と数の間の変換も）。
        if let Some(converted) = self.user_conversion(thread, &value, &resolved)? {
            return Ok(converted);
        }
        Err(tr!("A value of {from} cannot be cast to {to}.", from = unsafe { value::describe(&value, &self.modules) }.type_name, to = ty.name))
    }

    /// `(T[])値`。値の実体の型の名前（`byte[]`、`string[]`、`Ns.Order[]`）と比べる。配列の共変の変換（`(object[])strings`）は
    /// まだ扱わない。
    fn cast_to_array(&self, ty: &TypeSyntax, value: ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        if unsafe { value::argument_kind(&value) } == ArgumentKind::Null {
            return Ok(value);
        }
        // 値の型の名前は、組み込みの型を C# のキーワードで書く（`byte[]`）。
        let element = match ty.name.as_str() {
            "System.Boolean" => "bool",
            "System.Char" => "char",
            "System.SByte" => "sbyte",
            "System.Byte" => "byte",
            "System.Int16" => "short",
            "System.UInt16" => "ushort",
            "System.Int32" => "int",
            "System.UInt32" => "uint",
            "System.Int64" => "long",
            "System.UInt64" => "ulong",
            "System.Single" => "float",
            "System.Double" => "double",
            "System.Decimal" => "decimal",
            "System.String" => "string",
            "System.Object" => "object",
            other => other,
        };
        let actual = unsafe { value::describe(&value, &self.modules) }.type_name;
        let wanted = format!("{element}[]");
        // 名前空間を省いて書いた型（`(Order[])x`）も、実体の型の名前の末尾で合わせる。
        if actual == wanted || actual.ends_with(&format!(".{wanted}")) {
            return Ok(value);
        }
        Err(tr!("A value of {from} cannot be cast to {to}.", from = actual, to = wanted))
    }

    /// 値を、その型として扱えるか（`is`、`as`、キャストの判定）。実体の型、基底の型、インターフェイスを、名前で比べる。
    pub(super) fn value_is(&self, value: &ComPtr<ICorDebugValue>, ty: &ResolvedType) -> bool {
        unsafe {
            // Nullable<T> は、値があれば中身の T として判定する。無ければ、どの型でもない。
            match value::nullable(value, &self.modules) {
                Some(None) => return false,
                Some(Some(inner)) => return self.value_is(&inner, ty),
                None => {}
            }
            let kind = value::argument_kind(value);
            match ty {
                ResolvedType::Primitive(element) => kind == ArgumentKind::Primitive(*element) || value::unboxed_primitive(value, &self.modules).is_some_and(|(found, _)| found == *element as u32),
                ResolvedType::String => kind == ArgumentKind::String,
                ResolvedType::Object => kind != ArgumentKind::Null,
                ResolvedType::Named { module, row, .. } => kind != ArgumentKind::Null && value::type_names(value, &self.modules).contains(&self.plain_type_name(*module, *row)),
            }
        }
    }

    /// 式の中の型の名前を解決する。
    pub(super) fn resolve_type(&self, thread: u32, depth: usize, ty: &TypeSyntax) -> Result<ResolvedType, String> {
        // ジェネリックのメソッドの中の型引数（`T`）は、その呼び出しでの実際の型。
        if let Some(actual) = self.type_parameter(thread, depth, ty) {
            return self.resolve_type(thread, depth, &actual);
        }
        if let Some(element) = primitive_element(&ty.name) {
            return Ok(ResolvedType::Primitive(element));
        }
        match ty.name.as_str() {
            "System.String" => return Ok(ResolvedType::String),
            "System.Object" => return Ok(ResolvedType::Object),
            _ => {}
        }
        let (module, row) = self.find_type_with_arity(thread, depth, &ty.name, ty.arguments.len()).ok_or_else(|| tr!("No type named '{name}' was found.", name = ty.name))?;
        let kind = match self.modules[module].metadata().and_then(|m| m.base_type_name(row)).as_deref() {
            Some("System.Enum") => TypeKind::Enum,
            Some("System.ValueType") => TypeKind::Struct,
            _ => TypeKind::Class,
        };
        Ok(ResolvedType::Named { module, row, kind })
    }

    /// 値の型の型引数が、式に書かれた型引数と同じか（`words is List<string>`）。値の型そのものが、その型であるときだけ
    /// 比べる（基底の型やインターフェイスとして合っているときは、型引数までは確かめない）。
    pub(super) fn type_arguments_match(&mut self, thread: u32, depth: usize, value: &ComPtr<ICorDebugValue>, ty: &TypeSyntax, resolved: &ResolvedType) -> bool {
        let ResolvedType::Named { module, row, .. } = resolved else { return true };
        if ty.arguments.is_empty() || unsafe { value::type_names(value, &self.modules) }.first() != Some(&self.plain_type_name(*module, *row)) {
            return true;
        }
        let mut wanted = Vec::new();
        for argument in &ty.arguments {
            match self.runtime_type(thread, depth, argument) {
                Ok(argument) => wanted.push(unsafe { value::type_name(&argument, &self.modules) }),
                Err(_) => return false,
            }
        }
        unsafe { value::exact_type_arguments(value, &self.modules) == wanted }
    }

    /// 型の、リフレクションでの名前（`Type.GetType` に渡す形）と、アセンブリの名前。
    /// ランタイムの中心のライブラリ（最初に読み込まれるモジュール）の型は、アセンブリの名前を添えなくても見つかる。
    /// ジェネリック型は ``名前`数[[型引数],[型引数]]``、配列は末尾に `[]`。
    pub(super) fn reflection_name(&self, thread: u32, depth: usize, ty: &TypeSyntax) -> Result<(String, Option<String>), String> {
        // null 許容の値型（`int?`）は Nullable<T>。参照型の `?` は、型を変えない注釈。
        if ty.nullable {
            let inner = TypeSyntax { nullable: false, ..ty.clone() }.with_shape(&[]);
            let is_value = matches!(self.resolve_type(thread, depth, &inner)?, ResolvedType::Primitive(_) | ResolvedType::Named { kind: TypeKind::Struct | TypeKind::Enum, .. });
            if is_value {
                let (inner_name, inner_assembly) = self.reflection_name(thread, depth, &inner)?;
                let argument = match inner_assembly {
                    Some(assembly) => format!("[{inner_name}, {assembly}]"),
                    None => format!("[{inner_name}]"),
                };
                return Ok((format!("System.Nullable`1[{argument}]{}", array_suffix(ty)), None));
            }
        }
        let (mut name, assembly) = match self.resolve_type(thread, depth, ty)? {
            ResolvedType::Primitive(element) => (primitive_type_name(element).unwrap_or_default().to_string(), None),
            ResolvedType::String => ("System.String".to_string(), None),
            ResolvedType::Object => ("System.Object".to_string(), None),
            ResolvedType::Named { module, row, .. } => {
                let assembly = (module != 0).then(|| self.assembly_name_for_lookup(module));
                (self.plain_type_name(module, row), assembly)
            }
        };
        // 開いたジェネリック型（`Dictionary<,>`）は、型引数を書かない。
        if !ty.arguments.is_empty() && ty.arguments.iter().all(|argument| argument.name.is_empty()) {
            name = format!("{name}`{}", ty.arguments.len());
        } else if !ty.arguments.is_empty() {
            let mut arguments = Vec::new();
            for argument in &ty.arguments {
                arguments.push(match self.reflection_name(thread, depth, argument)? {
                    (name, Some(assembly)) => format!("[{name}, {assembly}]"),
                    (name, None) => format!("[{name}]"),
                });
            }
            name = format!("{name}`{}[{}]", ty.arguments.len(), arguments.join(","));
        }
        name.push_str(&"*".repeat(ty.pointers as usize));
        name.push_str(&array_suffix(ty));
        Ok((name, assembly))
    }

    /// `Type.GetType` に添えるアセンブリの名前。.NET は短い名前で見つかる。.NET Framework は、GAC のアセンブリ（System.Core など）を
    /// 短い名前では見つけないので、読み込まれているモジュールの参照から、完全な名前（版と公開鍵トークンつき）を探して使う。
    pub(super) fn assembly_name_for_lookup(&self, module: usize) -> String {
        let stem = Path::new(&self.modules[module].path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let core = self.modules.iter().any(|loaded| loaded.path.ends_with("System.Private.CoreLib.dll"));
        if core {
            return stem;
        }
        self.modules.iter().find_map(|loaded| loaded.metadata()?.assembly_reference(&stem)).unwrap_or(stem)
    }

    /// 値から分かる、式の型。`expr` が整数のリテラルなら、定数としての値も付ける。
    pub(super) fn typed_of(&self, expr: Option<&Expr>, value: &ComPtr<ICorDebugValue>, lifted: bool) -> Typed {
        let constant = match expr {
            Some(Expr::Int(number)) => Some(*number as i128),
            Some(Expr::Long(number)) => Some(*number as i128),
            Some(Expr::Unary(operators::UnaryOp::Negate, inner)) => match &**inner {
                Expr::Int(number) => Some(-(*number as i128)),
                Expr::Long(number) => Some(-(*number as i128)),
                _ => None,
            },
            _ => None,
        };
        unsafe {
            match value::nullable(value, &self.modules) {
                Some(Some(inner)) => return Typed { nullable: true, constant: None, ..self.typed_of(None, &inner, false) },
                // 値の無い Nullable<T> は、型引数から中身の型を知る。
                Some(None) => {
                    let inner = value::exact_type_arguments(value, &self.modules).into_iter().next().unwrap_or_default();
                    return Typed { nullable: true, ..Typed::from_keyword(&inner) };
                }
                None => {}
            }
            let kind = match self.operand(value) {
                Operand::Bool(_) => Kind::Bool,
                Operand::Int { kind, .. } => Kind::Int(kind),
                Operand::Float { single, .. } => Kind::Float { single },
                Operand::Enum { ty, .. } => Kind::Enum(ty),
                Operand::Str(_) => Kind::Str,
                Operand::Null => Kind::Null,
                Operand::Reference(_) if value::unboxed_primitive(value, &self.modules).is_some() => Kind::Object,
                Operand::Reference(_) => Kind::Class(value::describe(value, &self.modules).type_name),
                Operand::Other(name) if name == "decimal" => Kind::Decimal,
                Operand::Other(name) => Kind::Struct(name),
            };
            Typed { kind, nullable: lifted, constant }
        }
    }

    /// 式に書かれた型（null 許容の印は、呼び出し側で付ける）。
    pub(super) fn typed_of_resolved(&self, resolved: &ResolvedType) -> Typed {
        Typed::of(match resolved {
            ResolvedType::Primitive(0x02) => Kind::Bool,
            ResolvedType::Primitive(0x0C) => Kind::Float { single: true },
            ResolvedType::Primitive(0x0D) => Kind::Float { single: false },
            ResolvedType::Primitive(element) => IntKind::from_element(*element).map_or(Kind::Struct("?".into()), Kind::Int),
            ResolvedType::String => Kind::Str,
            ResolvedType::Object => Kind::Object,
            ResolvedType::Named { module, row, kind } => {
                let name = self.plain_type_name(*module, *row);
                match (kind, name.as_str()) {
                    (TypeKind::Enum, _) => Kind::Enum(EnumType { name: csharp_type_name(&name), module: *module, row: *row }),
                    (TypeKind::Struct, "System.Decimal") => Kind::Decimal,
                    (TypeKind::Struct, "System.IntPtr") => Kind::Int(IntKind::IPtr),
                    (TypeKind::Struct, "System.UIntPtr") => Kind::Int(IntKind::UPtr),
                    (TypeKind::Struct, _) => Kind::Struct(name),
                    (TypeKind::Class, _) => Kind::Class(name),
                }
            }
        })
    }

    /// 値を、決まった型に合わせる（`flag ? 1 : 2.5` の 1 を double にする、など）。null は、そのまま。
    pub(super) fn convert_to(&mut self, thread: u32, value: ComPtr<ICorDebugValue>, from: &Typed, to: &Typed) -> Result<Current, String> {
        let value = self.unwrapped(value);
        let operand = self.operand(&value);
        let converted = match &to.kind {
            _ if operand == Operand::Null || from.kind == to.kind => value,
            Kind::Int(kind) => match operators::convert(&operand, kind.element()) {
                Some(bytes) => self.new_number(thread, kind.element(), &bytes)?,
                None => value,
            },
            Kind::Float { single } => {
                let element = if *single { ELEMENT_TYPE_R4 } else { ELEMENT_TYPE_R8 };
                match operators::convert(&operand, element) {
                    Some(bytes) => self.new_primitive(thread, element as u32, &bytes)?,
                    None => value,
                }
            }
            // 整数から decimal へは、decimal が宣言している暗黙の変換で（nint / nuint は long / ulong を経由する）。
            Kind::Decimal => match self.modules.iter().enumerate().find_map(|(index, m)| Some((index, m.metadata()?.find_type("System.Decimal")?))) {
                Some(owner) => {
                    let source = self.without_native(thread, value.clone(), &operand)?;
                    self.implicitly_converted(thread, owner, &source).unwrap_or(value)
                }
                None => value,
            },
            // 定数の 0 から列挙型へ。
            Kind::Enum(ty) => match operand {
                Operand::Int { value: number, kind } => self.operand_value(thread, Operand::Enum { value: number, underlying: kind, ty: ty.clone() })?,
                _ => value,
            },
            _ => value,
        };
        let is_null = unsafe { value::argument_kind(&converted) } == ArgumentKind::Null;
        Ok(if to.nullable && to.is_value_type() && !is_null { Current::Nullable(converted) } else { Current::Value(converted) })
    }

    /// 値のある `Nullable<T>` は、中身の値にする。そうでなければ、そのまま。
    pub(super) fn unwrapped(&self, value: ComPtr<ICorDebugValue>) -> ComPtr<ICorDebugValue> {
        match unsafe { value::nullable(&value, &self.modules) } {
            Some(Some(inner)) => inner,
            _ => value,
        }
    }

    /// 値と同じ型の既定値（数は 0、bool は false、構造体は中身がすべて 0、参照型と null 許容の値型は null）。
    pub(super) fn default_like(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>, nullable: bool) -> Result<ComPtr<ICorDebugValue>, String> {
        if nullable {
            return self.new_null(thread);
        }
        match self.operand(value) {
            Operand::Int { kind, .. } => self.new_number(thread, kind.element(), &kind.bytes(0)),
            Operand::Float { single, .. } => self.operand_value(thread, Operand::Float { value: 0.0, single }),
            Operand::Bool(_) => self.new_bool(thread, false),
            Operand::Enum { underlying, ty, .. } => self.operand_value(thread, Operand::Enum { value: 0, underlying, ty }),
            Operand::Other(_) => {
                let ty = unsafe { value::exact_type_of(value) }.ok_or_else(|| tr!("The value's type is not known."))?;
                self.new_object_of_type(thread, &ty).map_err(|failure| failure.describe(self))
            }
            Operand::Str(_) | Operand::Reference(_) | Operand::Null => self.new_null(thread),
        }
    }

    /// 構造体や列挙型の、中身がすべて 0 の値（ボックスとして作る）。
    pub(super) fn zero_value(&mut self, thread: u32, module: usize, type_row: u32) -> Result<ComPtr<ICorDebugValue>, String> {
        let class = unsafe { take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | type_row, out)) }?;
        self.new_object_without_constructor(thread, &class).map_err(|failure| failure.describe(self))
    }

    /// 列挙型のもとになる整数の型（CorElementType）。
    pub(super) fn enum_underlying(&self, module: usize, type_row: u32) -> Option<u8> {
        let metadata = self.modules[module].metadata()?;
        let field = metadata.fields(type_row).into_iter().find(|f| !f.is_static && f.name == "value__")?;
        match metadata.field_kind(field.token)? {
            ParameterKind::Primitive(element) => Some(element),
            _ => None,
        }
    }

    /// ボックスとして作った列挙型の値に、中の数を書き込む。
    pub(super) fn set_enum_bits(&self, boxed: &ComPtr<ICorDebugValue>, module: usize, type_row: u32, bits: u64) -> Result<(), String> {
        let slot = self.modules[module].metadata().and_then(|m| m.fields(type_row).into_iter().find(|f| !f.is_static && f.name == "value__").map(|f| f.token)).ok_or_else(|| tr!("The location of the enum's number is not known."))?;
        unsafe {
            let class = take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | type_row, out))?;
            value::fill_box(boxed, &class, slot, &bits.to_le_bytes())
        }
    }
}

/// リフレクションの型の名前の、配列の部分（`[]`、`[,]`）。配列の配列は、C# の書き方と並びが逆になる
/// （C# の `int[][,]` は「2 次元の配列」の配列で、リフレクションでは `System.Int32[,][]`）。
fn array_suffix(ty: &TypeSyntax) -> String {
    ty.shape().iter().rev().map(|rank| format!("[{}]", ",".repeat(*rank as usize - 1))).collect()
}
