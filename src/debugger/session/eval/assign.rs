//! 代入（`a = b`、`a += b`、`a ??= b`）と、`++` / `--`。対象の変数、フィールド、配列の要素を書き換える。
//! プロパティとインデクサーは、setter（`set_名前`、`set_Item`）を実行する。
//!
//! 値は、代入先の型に合わせてから書く。`=` は暗黙の変換ができるときだけ（`int` の変数に `long` は入れられない）。
//! 複合代入と `++` / `--` は、計算の結果を代入先の型に戻す（C# も、`b += 1` を `b = (byte)(b + 1)` として扱う）。

use super::*;

/// 代入先。
enum Place {
    /// 変数、フィールド、配列の要素。対象の中の場所そのもの。
    Storage(ComPtr<ICorDebugValue>),
    /// インスタンスのプロパティ。
    Property { target: ComPtr<ICorDebugValue>, name: String },
    /// 静的プロパティ。
    StaticProperty { module: usize, row: u32, name: String },
    /// インデクサー。
    Indexer { target: ComPtr<ICorDebugValue>, indices: Vec<ComPtr<ICorDebugValue>> },
    /// ポインターの指す先（`*p`、`p[i]`）。番地と、指す先の型。
    Memory { address: u64, pointee: ComPtr<ICorDebugType> },
}

impl Session {
    /// 代入を評価する。結果は、代入した値。
    pub(super) fn assign(&mut self, thread: u32, depth: usize, target: &Expr, kind: Assignment, value: &Expr) -> Result<Current, String> {
        // 観察専用のアタッチ（D-089）: 利用者の評価の中の代入は断る（関数の評価を通らずに、場所へ直接書くため）。
        self.refuse_write_while_observing()?;
        // 代入先の、いまの値（型を知るため。`??=` では、null かどうかも）。
        let current = self.evaluate_node(thread, depth, target)?;
        let lifted = matches!(current, Current::Nullable(_));
        let current = self.value_of(current)?;
        let target_type = self.typed_of(None, &current, lifted);
        // C# 14 の、利用者が定義した複合代入の演算子（`public void operator +=(int x)`）。クラスの値は、その場で書き換わり、
        // 式の値は代入先の値（同じオブジェクト）。
        if let Assignment::Compound(op) = kind {
            let method = userops::compound_assignment_name(op);
            let declared = method.is_some_and(|method| self.class_row(&current).is_some_and(|(module, row)| self.modules[module].metadata().is_some_and(|metadata| !metadata.methods_named(row, method).is_empty())));
            if let (Some(method), true) = (method, declared) {
                let operand = self.evaluate_node(thread, depth, value)?;
                let operand = self.value_of(operand)?;
                self.call_method(thread, depth, Some(Current::Value(current.clone())), method, vec![operand])?;
                return Ok(Current::Value(current));
            }
        }
        let new_value = match kind {
            // `x = default`: 代入先の型の既定値。
            Assignment::Plain if matches!(value, Expr::DefaultLiteral) => self.default_like(thread, &current, target_type.nullable)?,
            // `x = new()`、`x = [1, 2]`: 代入先の型で作る（null の変数も、宣言の型を持っている）。
            Assignment::Plain if matches!(value, Expr::TargetNew { .. } | Expr::Collection { .. }) => {
                let target = unsafe { value::exact_type_of(&current) }.and_then(|ty| self.type_syntax_of(&ty)).ok_or_else(|| tr!("The type of the assignment target is not known."))?;
                self.build_targeted(thread, depth, value, &target).ok_or_else(|| tr!("The type of the assignment target is not known."))??
            }
            // `handler = x => x + 1`: 代入先のデリゲートの型で作る。
            Assignment::Plain if matches!(value, Expr::Lambda(..)) => {
                // null のフィールドの値は、型を持たないことがある。そのときは、メンバーの宣言の型を、メタデータから読む。
                let from_value = unsafe { value::exact_type_of(&current) }.and_then(|ty| self.type_syntax_of(&ty)).filter(|ty| ty.name != "System.Object");
                let delegate = match from_value {
                    Some(ty) => ty,
                    None => self.declared_member_type(thread, depth, target).ok_or_else(|| tr!("The delegate type of the assignment target is not known."))?,
                };
                self.lambda_as(thread, depth, &delegate, value)?
            }
            Assignment::Plain => {
                let source = self.evaluate_node(thread, depth, value)?;
                let source_lifted = matches!(source, Current::Nullable(_));
                let source = self.value_of(source)?;
                let source_type = self.typed_of(Some(value), &source, source_lifted);
                // 代入先が値型なら、その型に暗黙に変換できる値だけを入れられる。参照型は、実行時の型しか分からないので、
                // 値型を入れようとしたときだけ断る（`object` の変数は、いまの値が null でなければ見分けられない）。
                if target_type.is_value_type() && !types::converts_with_constant(&source_type, &target_type) {
                    return Err(tr!("This value cannot be converted implicitly to the assignment target's type (cast it)."));
                }
                let converted = self.convert_to(thread, source, &source_type, &target_type)?;
                self.value_of(converted)?
            }
            Assignment::Compound(op) => {
                let computed = self.evaluate_node(thread, depth, &Expr::Binary(op, Box::new(target.clone()), Box::new(value.clone())))?;
                let computed = self.value_of(computed)?;
                self.back_to(thread, computed, &target_type)?
            }
            Assignment::Coalesce => {
                let is_null = unsafe { value::argument_kind(&current) == ArgumentKind::Null || matches!(value::nullable(&current, &self.modules), Some(None)) };
                if !is_null {
                    return Ok(Current::Value(self.unwrapped(current)));
                }
                let source = self.evaluate_node(thread, depth, value)?;
                self.value_of(source)?
            }
        };
        self.store(thread, depth, target, &new_value)?;
        // 代入の式の型は、代入先の型（`maybe = 5` は int?）。
        let is_null = unsafe { value::argument_kind(&new_value) } == ArgumentKind::Null;
        Ok(if target_type.nullable && target_type.is_value_type() && !is_null { Current::Nullable(self.unwrapped(new_value)) } else { Current::Value(new_value) })
    }

    /// `対象.名前` のメンバー（フィールドか、プロパティ）の、宣言の型。メタデータのシグネチャから読む。
    fn declared_member_type(&mut self, thread: u32, depth: usize, target: &Expr) -> Option<TypeSyntax> {
        let Expr::Member(owner, name) = target else { return None };
        let owner = self.evaluate_node(thread, depth, owner).ok()?;
        let owner = self.value_of(owner).ok()?;
        let (module, row) = self.class_row(&owner)?;
        let class_arguments = unsafe { value::exact_type_arguments(&owner, &self.modules) };
        let metadata = self.modules[module].metadata()?;
        let shape = match metadata.fields(row).into_iter().find(|field| !field.is_static && field.name == name) {
            Some(field) => metadata.field_type(field.token, &class_arguments)?,
            None => metadata.return_type(*metadata.find_methods_shaped(row, &format!("get_{name}"), false, 0).first()?, &class_arguments)?,
        };
        syntax::parse_type(&shape.display())
    }

    /// 分解の代入（`var (a, b) = 式`、`(x, _) = 式`）。結果は、C# と同じく、配った値のタプル。
    ///
    /// タプル（`ValueTuple`、`Tuple`）は `Item1`、`Item2`… を配る。そのほかの型は、C# と同じく `Deconstruct(out …)` を呼ぶ。
    /// 宣言した変数（`var a`）は、式の中だけの変数になる（`out var` と同じ）。
    pub(super) fn deconstruct(&mut self, thread: u32, depth: usize, targets: &[syntax::OutTarget], value: &Expr) -> Result<ComPtr<ICorDebugValue>, String> {
        let current = self.evaluate_node(thread, depth, value)?;
        let whole = self.value_of(current)?;
        self.deconstruct_into(thread, depth, targets, &whole)
    }

    /// 値を、分解の代入の左辺に配る。戻り値は、配った値のタプル（もとがタプルなら、そのもの）。
    fn deconstruct_into(&mut self, thread: u32, depth: usize, targets: &[syntax::OutTarget], whole: &ComPtr<ICorDebugValue>) -> Result<ComPtr<ICorDebugValue>, String> {
        use syntax::OutTarget;
        if !(2..=7).contains(&targets.len()) {
            return Err(tr!("Only 2 to 7 elements can be deconstructed."));
        }
        let source = self.unwrapped(whole.clone());
        // 型の名前は、型引数の数の印（`2）が付く形と付かない形の、どちらでも来る。
        let is_tuple = unsafe { value::type_names(&source, &self.modules) }.iter().any(|name| {
            let plain = name.split('`').next().unwrap_or(name);
            plain == "System.ValueTuple" || plain == "System.Tuple"
        });
        let mut parts = Vec::new();
        if is_tuple {
            for index in 1..=targets.len() {
                parts.push(self.member(thread, &source, &format!("Item{index}")).map_err(|_| tr!("The tuple's element count (fewer than {count}) does not match the deconstruction targets.", count = index))?);
            }
            if self.member(thread, &source, &format!("Item{}", targets.len() + 1)).is_ok() {
                return Err(tr!("The tuple has more elements than the deconstruction targets."));
            }
        } else {
            // `値.Deconstruct(out $d0, out $d1, …)`。値は、`?.` と同じ置き場を通して式に渡す。受け取った値は、
            // 式の中だけの変数として戻ってくるので、取り出して配る。
            let names: Vec<String> = (0..targets.len()).map(|index| format!("$d{index}")).collect();
            let outs = names.iter().map(|name| Expr::Out(OutTarget::Declare(None, name.clone()))).collect();
            let call = Expr::Call(Box::new(Expr::Member(Box::new(Expr::Receiver), "Deconstruct".into())), outs);
            self.receivers.push(source);
            let called = self.evaluate_node(thread, depth, &call);
            self.receivers.pop();
            called.map_err(|error| tr!("This value cannot be deconstructed (it is neither a tuple nor a type with Deconstruct): {error}", error = error))?;
            for name in &names {
                let position = self.pattern_locals.iter().rposition(|(existing, _)| existing == name).ok_or_else(|| tr!("Deconstruct returned no value."))?;
                parts.push(self.pattern_locals.remove(position).1);
            }
        }
        for (target, part) in targets.iter().zip(&parts) {
            match target {
                OutTarget::Discard => {}
                OutTarget::Tuple(inner) => {
                    self.deconstruct_into(thread, depth, inner, part)?;
                }
                OutTarget::Declare(_, name) => self.pattern_locals.push((name.clone(), part.clone())),
                OutTarget::Existing(place) => {
                    self.receivers.push(part.clone());
                    let assigned = self.assign(thread, depth, place, Assignment::Plain, &Expr::Receiver);
                    self.receivers.pop();
                    assigned?;
                }
            }
        }
        if is_tuple { Ok(whole.clone()) } else { self.tuple_of(thread, depth, parts) }
    }

    /// `++` / `--` を評価する。結果は、前置なら変えたあとの値、後置なら変える前の値。
    pub(super) fn increment(&mut self, thread: u32, depth: usize, target: &Expr, decrement: bool, prefix: bool) -> Result<Current, String> {
        let current = self.evaluate_node(thread, depth, target)?;
        let lifted = matches!(current, Current::Nullable(_));
        let current = self.value_of(current)?;
        let target_type = self.typed_of(None, &current, lifted);
        // 変数の値は、書き換えると変わってしまう。変える前の値を、写しとして取っておく。
        let before = self.operand_value(thread, self.operand(&current))?;
        let op = if decrement { BinaryOp::Subtract } else { BinaryOp::Add };
        let computed = self.evaluate_node(thread, depth, &Expr::Binary(op, Box::new(target.clone()), Box::new(Expr::Int(1))))?;
        let computed = self.value_of(computed)?;
        let after = self.back_to(thread, computed, &target_type)?;
        self.store(thread, depth, target, &after)?;
        Ok(Current::Value(if prefix { after } else { before }))
    }

    /// 計算の結果を、代入先の型に戻す（明示のキャストと同じ。`byte` の変数への `+=` は、結果の int を byte にする）。
    fn back_to(&mut self, thread: u32, value: ComPtr<ICorDebugValue>, target: &Typed) -> Result<ComPtr<ICorDebugValue>, String> {
        let element = match &target.kind {
            Kind::Int(kind) => kind.element(),
            Kind::Float { single: true } => ELEMENT_TYPE_R4,
            Kind::Float { single: false } => ELEMENT_TYPE_R8,
            _ => return Ok(value),
        };
        match operators::convert(&self.operand(&value), element) {
            Some(bytes) => self.new_number(thread, element, &bytes),
            None => Ok(value),
        }
    }

    /// 式が指している場所に、値を書く。
    fn store(&mut self, thread: u32, depth: usize, target: &Expr, value: &ComPtr<ICorDebugValue>) -> Result<(), String> {
        match self.place(thread, depth, target)? {
            Place::Storage(storage) => self.write(thread, &storage, value),
            Place::Property { target, name } => self.call_method(thread, depth, Some(Current::Value(target)), &format!("set_{name}"), vec![value.clone()]).map(|_| ()),
            Place::StaticProperty { module, row, name } => self.call_method(thread, depth, Some(Current::Type { module, row }), &format!("set_{name}"), vec![value.clone()]).map(|_| ()),
            Place::Indexer { target, mut indices } => {
                indices.push(value.clone());
                self.call_method(thread, depth, Some(Current::Value(target)), "set_Item", indices).map(|_| ())
            }
            Place::Memory { address, pointee } => self.write_through(address, &pointee, value),
        }
    }

    /// オブジェクトのメンバー（フィールドか、プロパティ）に値を入れる。初期化子（`new T { X = 1 }`）で使う。
    pub(super) fn store_member(&mut self, thread: u32, depth: usize, owner: &ComPtr<ICorDebugValue>, name: &str, value: &ComPtr<ICorDebugValue>) -> Result<(), String> {
        match unsafe { value::field(owner, &self.modules, name) } {
            Some(storage) => self.write(thread, &storage, value),
            None => self.call_method(thread, depth, Some(Current::Value(owner.clone())), &format!("set_{name}"), vec![value.clone()]).map(|_| ()),
        }
    }

    /// 式が、対象の中のどこを指しているか。
    fn place(&mut self, thread: u32, depth: usize, target: &Expr) -> Result<Place, String> {
        // ポインターの指す先（`*p = v`、`p[i] = v`）: 番地に直に書く。
        if let Some((address, pointee)) = self.pointer_place(thread, depth, target)? {
            return Ok(Place::Memory { address, pointee });
        }
        match target {
            Expr::Name(name) => {
                // 近い方から: ローカル変数と引数、this のフィールド、this のプロパティ、いまの型の静的フィールドとプロパティ。
                let variables = self.frame_variables(thread, depth)?;
                if let Some((_, storage)) = variables.iter().find(|(existing, _)| existing == name) {
                    return Ok(Place::Storage(storage.clone()));
                }
                if let Some((_, this)) = variables.into_iter().find(|(existing, _)| existing == "this") {
                    return Ok(match unsafe { value::field(&this, &self.modules, name) } {
                        Some(storage) => Place::Storage(storage),
                        None => Place::Property { target: this, name: name.clone() },
                    });
                }
                let (module, row) = self.frame_type(thread, depth).ok_or_else(|| tr!("No variable named '{name}' was found.", name = name))?;
                self.static_place(thread, depth, module, row, name)
            }
            Expr::Member(owner, name) => match self.evaluate_node(thread, depth, owner)? {
                // ポインターの指す先を読んで作った写しに書いても、対象には届かない。
                Current::Value(owner) if self.is_pointer_copy(&owner) => {
                    Err(tr!("A struct member at this position cannot be written (what a pointer variable itself points at can be written with p->X = 1)."))
                }
                Current::Value(owner) | Current::Nullable(owner) => Ok(match unsafe { value::field(&owner, &self.modules, name) } {
                    Some(storage) => Place::Storage(storage),
                    None => Place::Property { target: owner, name: name.clone() },
                }),
                Current::Type { module, row } => self.static_place(thread, depth, module, row, name),
                _ => Err(tr!("The left side of the assignment does not point at a variable or member.")),
            },
            Expr::Index(owner, indices) => {
                let owner = self.evaluate_node(thread, depth, owner)?;
                let owner = self.value_of(owner)?;
                let indices = self.evaluate_arguments(thread, depth, indices)?;
                let numbers: Vec<u32> = indices.iter().filter_map(|index| unsafe { value::describe(index, &self.modules) }.value.parse().ok()).collect();
                if numbers.len() == indices.len() {
                    if let Some(element) = unsafe { value::array_element(&owner, &numbers) }? {
                        return Ok(Place::Storage(element));
                    }
                }
                Ok(Place::Indexer { target: owner, indices })
            }
            _ => Err(tr!("Write a variable, field, property or indexer on the left side of the assignment.")),
        }
    }

    /// 型の静的フィールド（その場所）か、静的プロパティ。
    fn static_place(&mut self, thread: u32, depth: usize, module: usize, row: u32, name: &str) -> Result<Place, String> {
        let field = self.modules[module].metadata().and_then(|m| m.fields(row).into_iter().find(|f| f.is_static && f.name == name).map(|f| (f.token, f.is_literal)));
        match field {
            Some((_, true)) => Err(tr!("{name} is a constant, so it cannot be assigned to.", name = name)),
            Some((token, false)) => unsafe {
                let handle = self.modules[module].module.clone();
                let class = take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| handle.GetClassFromToken(TYPE_DEF | row, out))?;
                let frame = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().nth(depth));
                let mut raw = null_mut();
                let hr = class.GetStaticFieldValue(token, frame.as_ref().map_or(null_mut(), |f| f.as_raw()), &mut raw);
                let storage = ComPtr::<ICorDebugValue>::from_raw(raw).ok_or_else(|| tr!("Cannot write the static field {name} ({code}). The type may not be initialized yet.", name = name, code = hr))?;
                // 宣言の型がプリミティブ型なのにボックスとして見える場所は、ボックスの中に書く。
                let declared_primitive = matches!(self.modules[module].metadata().and_then(|m| m.field_kind(token)), Some(ParameterKind::Primitive(_)));
                Ok(Place::Storage(if declared_primitive { value::inside_box(&storage).unwrap_or(storage) } else { storage }))
            },
            None => Ok(Place::StaticProperty { module, row, name: name.to_string() }),
        }
    }

    /// 対象の中の場所に、値を書く。
    pub(super) fn write(&mut self, thread: u32, storage: &ComPtr<ICorDebugValue>, value: &ComPtr<ICorDebugValue>) -> Result<(), String> {
        // 入口（assign）を通らない書き込みができても、観察専用なら断る。
        self.refuse_write_while_observing()?;
        self.touched_target = true;
        let failed = |what: &str, hr: HRESULT| tr!("Cannot write the value ({what}: {code}).", what = what, code = hr);
        unsafe {
            // Nullable<T> の場所: 値の有無と、中身を別々に書く。
            if value::nullable(storage, &self.modules).is_some() {
                let (Some(has_value), Some(inner)) = (value::field(storage, &self.modules, "hasValue"), value::field(storage, &self.modules, "value")) else {
                    return Err(tr!("Cannot read the location of the nullable value."));
                };
                let is_null = value::argument_kind(value) == ArgumentKind::Null || matches!(value::nullable(value, &self.modules), Some(None));
                if !is_null {
                    let source = self.unwrapped(value.clone());
                    self.write(thread, &inner, &source)?;
                }
                let flag = self.new_bool(thread, !is_null)?;
                return self.write(thread, &has_value, &flag);
            }
            // 参照の場所（クラス、文字列、object、配列の変数）: 指す先を入れ替える。
            if let Ok(reference) = storage.cast::<ICorDebugReferenceValue>() {
                let source = match value::argument_kind(value) {
                    // 数を object の場所に入れるときは、ボックス化する。
                    ArgumentKind::Primitive(_) => {
                        let (module, row) = self.primitive_type(value).ok_or_else(|| tr!("This value cannot be boxed."))?;
                        self.box_primitive(thread, module, row, value)?
                    }
                    _ => value.clone(),
                };
                let source = source.cast::<ICorDebugReferenceValue>().map_err(|_| tr!("A value-type value cannot be put in a reference-type location."))?;
                let mut address = 0u64;
                source.GetValue(&mut address).ok().map_err(|hr| failed("ICorDebugReferenceValue::GetValue", hr))?;
                return reference.SetValue(address).ok().map_err(|hr| failed("ICorDebugReferenceValue::SetValue", hr));
            }
            let generic = storage.cast::<ICorDebugGenericValue>().map_err(|_| tr!("A value cannot be written to this location."))?;
            let (mut element, mut size) = (0, 0u32);
            storage.GetType(&mut element);
            storage.GetSize(&mut size);
            // プリミティブ型の場所: その型のバイト列にして書く。
            let mut bytes = match operators::convert(&self.operand(value), element as u8) {
                Some(bytes) => bytes,
                // bool は、数ではないので変換の表に無い。
                None if element == 0x02 => match self.operand(value) {
                    Operand::Bool(flag) => vec![flag as u8],
                    _ => return Err(tr!("Put a bool value in a bool location.")),
                },
                // 構造体と列挙型の場所: 同じ大きさの値の中身を、そのまま写す。
                None => {
                    let source = value::inside_box(value).unwrap_or_else(|| value.clone());
                    let source_generic = source.cast::<ICorDebugGenericValue>().map_err(|_| tr!("This value cannot be put in that location."))?;
                    let mut source_size = 0u32;
                    source.GetSize(&mut source_size);
                    if source_size != size {
                        return Err(tr!("The value's type differs from the assignment target's type."));
                    }
                    let mut buffer = vec![0u8; size as usize];
                    source_generic.GetValue(buffer.as_mut_ptr() as *mut c_void).ok().map_err(|hr| failed("ICorDebugGenericValue::GetValue", hr))?;
                    buffer
                }
            };
            if bytes.len() != size as usize {
                return Err(tr!("The value's size differs from the assignment target's."));
            }
            generic.SetValue(bytes.as_mut_ptr() as *mut c_void).ok().map_err(|hr| failed("ICorDebugGenericValue::SetValue", hr))
        }
    }
}
