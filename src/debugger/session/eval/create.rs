//! `new`: オブジェクトと配列の生成。型引数つきの型（`List<int>`）の解決もここ。
//!
//! オブジェクトは、対象の中でコンストラクターを実行して作る。初期化子（`{ X = 1 }`、`{ 1, 2 }`、`{ ["k"] = 1 }`）は、
//! 作ったあとに、メンバーへの代入、`Add` の呼び出し、インデクサーへの代入として順に実行する（C# のコンパイラがすることと同じ）。

use super::*;
use crate::debugger::session::eval::syntax::{parse_type, Initializer};

const ELEMENT_TYPE_CLASS: u32 = 0x12;
const ELEMENT_TYPE_VALUETYPE: u32 = 0x11;



impl Session {
    /// 式に書かれた型を、対象の中の型（`ICorDebugType`）にする。型引数も解決する。
    pub(super) fn runtime_type(&mut self, thread: u32, depth: usize, ty: &TypeSyntax) -> Result<ComPtr<ICorDebugType>, String> {
        const ELEMENT_TYPE_SZARRAY: u32 = 0x1D;
        const ELEMENT_TYPE_ARRAY: u32 = 0x14;
        const ELEMENT_TYPE_PTR: u32 = 0x0F;
        // ジェネリックのメソッドの中の型引数（`T`）は、その呼び出しでの実際の型。
        if let Some(actual) = self.type_parameter(thread, depth, ty) {
            return self.runtime_type(thread, depth, &actual);
        }
        // ポインター（`uint*`）は、指す先の型から作る。
        if ty.pointers > 0 {
            let pointee = self.runtime_type(thread, depth, &TypeSyntax { pointers: ty.pointers - 1, ..ty.clone() })?;
            let domain = self.app_domain(thread)?;
            return unsafe { take::<ICorDebugType>("ICorDebugAppDomain2::GetArrayOrPointerType", |out| domain.GetArrayOrPointerType(ELEMENT_TYPE_PTR, 0, pointee.as_raw(), out)) };
        }
        // 配列（`int[]`、`int[,]`、`int[][]`）は、要素の型から作る。
        if let Some(&rank) = ty.shape().first() {
            let element = self.runtime_type(thread, depth, &ty.element())?;
            let domain = self.app_domain(thread)?;
            let kind = if rank == 1 { ELEMENT_TYPE_SZARRAY } else { ELEMENT_TYPE_ARRAY };
            return unsafe { take::<ICorDebugType>("ICorDebugAppDomain2::GetArrayOrPointerType", |out| domain.GetArrayOrPointerType(kind, rank, element.as_raw(), out)) };
        }
        // null 許容の値型（`int?`）は、Nullable<T>。参照型の `?`（null 許容の注釈）は、型を変えない。
        if ty.nullable {
            let inner = TypeSyntax { nullable: false, ..ty.clone() };
            let is_value = primitive_element(&inner.name).is_some() || matches!(self.resolve_type(thread, depth, &inner), Ok(ResolvedType::Named { kind: TypeKind::Struct | TypeKind::Enum, .. }));
            if is_value {
                let nullable = TypeSyntax { arguments: vec![inner], ..TypeSyntax::named("System.Nullable") };
                return self.runtime_type(thread, depth, &nullable);
            }
            return self.runtime_type(thread, depth, &inner);
        }
        // プリミティブ型は、その型の値を一つ作って、型を聞く（型だけを直に得る方法が無い）。
        if let Some(element) = primitive_element(&ty.name).filter(|_| ty.arguments.is_empty()) {
            let sample = self.new_number(thread, element, &vec![0u8; primitive_size(element)])?;
            return unsafe { value::exact_type_of(&sample) }.ok_or_else(|| tr!("Cannot get the type {name}.", name = ty.name));
        }
        let (module, row) = self.find_type_with_arity(thread, depth, &ty.name, ty.arguments.len()).ok_or_else(|| tr!("No type named '{name}' was found.", name = ty.name))?;
        let arguments = ty.arguments.iter().map(|argument| self.runtime_type(thread, depth, argument)).collect::<Result<Vec<_>, _>>()?;
        self.parameterized(module, row, &arguments)
    }

    /// スレッドのアプリケーションドメイン（配列の型を作るのに使う）。
    pub(super) fn app_domain(&self, thread: u32) -> Result<ComPtr<ICorDebugAppDomain2>, String> {
        let process = self.process.clone().ok_or_else(|| tr!("There is no target process."))?;
        unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread, out))?;
            let domain = take::<ICorDebugAppDomain>("ICorDebugThread::GetAppDomain", |out| thread.GetAppDomain(out))?;
            domain.cast::<ICorDebugAppDomain2>().map_err(|hr| failed(tr_str!("Getting ICorDebugAppDomain2"), hr))
        }
    }

    /// 型と型引数から、対象の中の型を作る。
    pub(super) fn parameterized(&self, module: usize, row: u32, arguments: &[ComPtr<ICorDebugType>]) -> Result<ComPtr<ICorDebugType>, String> {
        let is_value = matches!(self.modules[module].metadata().and_then(|m| m.base_type_name(row)).as_deref(), Some("System.ValueType" | "System.Enum"));
        unsafe {
            let class = take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | row, out))?;
            let class2: ComPtr<ICorDebugClass2> = class.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugClass2"), hr))?;
            let mut raw: Vec<*mut c_void> = arguments.iter().map(|argument| argument.as_raw()).collect();
            take::<ICorDebugType>("ICorDebugClass2::GetParameterizedType", |out| class2.GetParameterizedType(if is_value { ELEMENT_TYPE_VALUETYPE } else { ELEMENT_TYPE_CLASS }, raw.len() as u32, raw.as_mut_ptr(), out))
        }
    }

    /// 名前と型引数の数に合う型。`find_type` と同じ順で探す。
    pub(super) fn find_type_with_arity(&self, thread: u32, depth: usize, name: &str, arity: usize) -> Option<(usize, u32)> {
        if arity == 0 {
            return self.find_type(thread, depth, name);
        }
        let candidates = self.type_name_candidates(thread, depth, name);
        let search = |with_symbols: bool| {
            self.modules.iter().enumerate().filter(|(_, m)| m.pdb.is_some() == with_symbols).find_map(|(index, module)| {
                let metadata = module.metadata()?;
                candidates.iter().find_map(|candidate| metadata.find_generic_type(candidate, arity)).map(|row| (index, row))
            })
        };
        search(true).or_else(|| search(false))
    }

    /// `new 型(引数) { 初期化子 }`
    pub(super) fn create(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, arguments: &[Expr], initializer: &Initializer) -> Result<ComPtr<ICorDebugValue>, String> {
        let type_name = ty.name.as_str();
        if primitive_element(type_name).is_some() || ty.nullable || ty.array {
            return Err(tr!("{type} cannot be created with new.", type = ty.name));
        }
        // `new Func<int, int>(x => x + 1)`: デリゲートの生成。
        if let [lambda @ Expr::Lambda(..)] = arguments {
            return self.lambda_as(thread, depth, ty, lambda);
        }
        // `new Func<int, int>(Doubled)`: メソッドの集まりから、デリゲートを作る。
        if let [method @ (Expr::Name(_) | Expr::Member(..) | Expr::Generic(..))] = arguments {
            if self.evaluate_node(thread, depth, method).and_then(|current| self.value_of(current)).is_err() && self.is_method_group(thread, depth, method) {
                if let Some(lambda) = self.group_lambda_for(thread, depth, ty, method) {
                    return self.lambda_as(thread, depth, ty, &lambda);
                }
            }
        }
        // 文字列のコンストラクターは、ランタイムの特別なもので、関数評価からは呼べない。同じ結果になる呼び出しに直す。
        if type_name == "System.String" {
            let values = self.evaluate_arguments(thread, depth, arguments)?;
            let kinds: Vec<ArgumentKind> = values.iter().map(|value| unsafe { value::argument_kind(value) }).collect();
            if kinds == [ArgumentKind::Primitive(0x03), ArgumentKind::Primitive(0x08)] {
                // new string(c, n) は、空の文字列を c で n 文字に埋めたもの。
                let empty = self.new_string(thread, "").map_err(|failure| failure.describe(self))?;
                return self.call_method(thread, depth, Some(Current::Value(empty)), "PadLeft", vec![values[1].clone(), values[0].clone()])?.ok_or_else(|| tr!("Cannot create the string."));
            }
            return Err(tr!("new string(…) can be written only as new string(char, count) for now."));
        }
        let (module, row) = self.find_type_with_arity(thread, depth, type_name, ty.arguments.len()).ok_or_else(|| tr!("No type named '{name}' was found.", name = ty.name))?;
        let type_arguments = ty.arguments.iter().map(|argument| self.runtime_type(thread, depth, argument)).collect::<Result<Vec<_>, _>>()?;
        // 位置の引数と、名前つき引数（`new List<int>(capacity: 8)`、コレクション式の `with(capacity: 8)`）。
        let mut extra = bind::CallExtra::default();
        let mut values = Vec::new();
        for argument in arguments {
            match argument {
                Expr::Named(name, value) => {
                    let value = self.evaluate_node(thread, depth, value)?;
                    extra.named.push((name.clone(), self.value_of(value)?));
                }
                other => {
                    let value = self.evaluate_node(thread, depth, other)?;
                    values.push(self.value_of(value)?);
                }
            }
        }
        let shown = self.type_name(module, row);

        let mut candidates = if extra.named.is_empty() { self.type_method_candidates(module, row, ".ctor", false, values.len() as u32) } else { Vec::new() };
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = type_arguments.clone();
        }
        let is_value = matches!(self.modules[module].metadata().and_then(|m| m.base_type_name(row)).as_deref(), Some("System.ValueType" | "System.Enum"));
        let created = if candidates.is_empty() && values.is_empty() && is_value {
            // 構造体の `new T()` は、コンストラクターが無ければ、中身がすべて 0 の値。
            let exact = self.parameterized(module, row, &type_arguments)?;
            self.new_object_of_type(thread, &exact).map_err(|failure| failure.describe(self))?
        } else {
            // 引数がそのまま合うものが無ければ、広げた形（名前つき引数、省略できる引数、params）で探す。
            let plain = if candidates.is_empty() { None } else { self.choose_overload(thread, candidates, &mut values) };
            let constructor = match plain {
                Some(constructor) => constructor,
                None => {
                    let (constructor, bound) = self.bind_expanded(thread, depth, (module, row), ".ctor", false, &type_arguments, &values, &extra).ok_or_else(|| tr!("{type} has no constructor that takes these arguments.", type = shown))?;
                    values = bound;
                    constructor
                }
            };
            match self.new_object(thread, &constructor, &values) {
                Ok(created) => created,
                Err(funceval::EvalFailure::Threw(exception)) => return Ok(exception),
                Err(failure) => return Err(tr!("Cannot create {type}: {reason}", type = shown, reason = failure.describe(self))),
            }
        };
        self.apply_initializer(thread, depth, &created, initializer)?;
        Ok(created)
    }

    /// 初期化子（`{ X = 1 }`、`{ 1, 2 }`、`{ ["k"] = 1 }`）を、作ったオブジェクトに順に実行する。
    /// `名前 = { … }` の形は、メンバーを作り直さずに、いまの値に、中の初期化子を実行する（C# と同じ）。
    fn apply_initializer(&mut self, thread: u32, depth: usize, created: &ComPtr<ICorDebugValue>, initializer: &Initializer) -> Result<(), String> {
        match initializer {
            Initializer::None => {}
            Initializer::Members(members) => {
                for (name, expr) in members {
                    if let Expr::Nested(inner) = expr {
                        let member = self.member(thread, created, name)?;
                        if unsafe { value::argument_kind(&member) } == ArgumentKind::Null {
                            return Err(tr!("The member {name} is null, so its contents cannot be initialized (it would throw NullReferenceException).", name = name));
                        }
                        self.apply_initializer(thread, depth, &member, inner)?;
                        continue;
                    }
                    // 型を代入先から決める値（ラムダ式、型を省いた new、コレクション式、default）は、代入として評価する。
                    if matches!(expr, Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral) {
                        self.receivers.push(created.clone());
                        let assigned = self.assign(thread, depth, &Expr::Member(Box::new(Expr::Receiver), name.clone()), Assignment::Plain, expr);
                        self.receivers.pop();
                        assigned?;
                        continue;
                    }
                    let value = self.evaluate_node(thread, depth, expr)?;
                    let value = self.value_of(value)?;
                    self.store_member(thread, depth, created, name, &value)?;
                }
            }
            Initializer::Items(items) => {
                // `Add(要素)` の呼び出しとして評価する（要素がラムダ式でも、引数の型からデリゲートになる）。
                for item in items {
                    let call = Expr::Call(Box::new(Expr::Member(Box::new(Expr::Receiver), "Add".into())), item.clone());
                    self.receivers.push(created.clone());
                    let added = self.evaluate_node(thread, depth, &call);
                    self.receivers.pop();
                    added?;
                }
            }
            Initializer::Indexed(entries) => {
                for (indices, expr) in entries {
                    let mut call = self.evaluate_arguments(thread, depth, indices)?;
                    let value = self.evaluate_node(thread, depth, expr)?;
                    call.push(self.value_of(value)?);
                    self.call_method(thread, depth, Some(Current::Value(created.clone())), "set_Item", call)?;
                }
            }
        }
        Ok(())
    }

    /// 多次元の配列の、中身を書いた生成（`new int[,] { { 1, 2 }, { 3, 4 } }`）。`items` は、行を優先した順の要素。
    pub(super) fn create_matrix(&mut self, thread: u32, depth: usize, element: Option<&TypeSyntax>, dimensions: &[usize], items: &[Expr]) -> Result<ComPtr<ICorDebugValue>, String> {
        let values = self.evaluate_arguments(thread, depth, items)?;
        let sizes: Vec<u32> = dimensions.iter().map(|&size| size as u32).collect();
        let type_object = match element {
            Some(element) => {
                let type_object = self.evaluate_node(thread, depth, &Expr::TypeOf(element.clone()))?;
                self.value_of(type_object)?
            }
            // `new[,] { … }`: 要素の型は、最初の要素の型。
            None => {
                let first = values.first().ok_or_else(|| tr!("new[,] { } is empty, so the element type cannot be determined."))?;
                self.call_method(thread, depth, Some(Current::Value(first.clone())), "GetType", Vec::new())?.ok_or_else(|| tr!("Cannot get the element type."))?
            }
        };
        let array = self.create_instance_of(thread, depth, type_object, &sizes)?;
        for (position, value) in values.iter().enumerate() {
            // 行を優先した順の位置を、次元ごとの添字に直す。
            let (mut rest, mut indices) = (position, vec![0u32; sizes.len()]);
            for (axis, size) in sizes.iter().enumerate().rev() {
                indices[axis] = (rest % (*size).max(1) as usize) as u32;
                rest /= (*size).max(1) as usize;
            }
            let slot = unsafe { value::array_element(&array, &indices) }?.ok_or_else(|| tr!("Cannot read an element of the created array."))?;
            self.write(thread, &slot, value)?;
        }
        Ok(array)
    }

    /// 配列の生成。
    pub(super) fn create_array(&mut self, thread: u32, depth: usize, element: Option<&TypeSyntax>, sizes: &[Expr], items: Option<&[Expr]>) -> Result<ComPtr<ICorDebugValue>, String> {
        let values = match items {
            Some(items) => self.evaluate_arguments(thread, depth, items)?,
            None => Vec::new(),
        };
        let mut dimensions = Vec::new();
        for size in sizes {
            match self.evaluate_operand(thread, depth, size)? {
                Operand::Int { value, .. } if (0..=i32::MAX as i128).contains(&value) => dimensions.push(value as u32),
                other => return Err(tr!("The array size must be an integer of 0 or more (it is {type}).", type = operators::kind_name(&other))),
            }
        }
        if dimensions.is_empty() {
            dimensions.push(values.len() as u32);
        }
        if items.is_some() && (dimensions.len() != 1 || dimensions[0] as usize != values.len()) {
            return Err(tr!("The array size and the number of elements do not match (elements can be written only for one-dimensional arrays)."));
        }
        // 多次元の配列は、関数評価では直に作れない（RankException になる）。System.Array.CreateInstance(型, 大きさ…) で作る。
        if dimensions.len() > 1 {
            let element = element.ok_or_else(|| tr!("Write the element type for a multidimensional array."))?;
            return self.create_instance(thread, depth, element, &dimensions);
        }
        let element_type = match element {
            Some(element) => self.runtime_type(thread, depth, element)?,
            // `new[] { … }`: 要素の型は、最初の要素の型（C# は、すべての要素に合う型を選ぶ。ここでは、同じ型の並びだけを扱う）。
            None => values.first().and_then(|first| unsafe { value::exact_type_of(first) }).ok_or_else(|| tr!("new[] { } is empty or null, so the element type cannot be determined."))?,
        };
        // 要素が null 許容の値型（`int?`）のときは、NewParameterizedArray は Nullable を外した `int[]` を作ってしまう。
        // ほかの型引数つきの値型では ArgumentException になる。どちらも CreateInstance で作る。
        let array = match element {
            Some(element) if element.nullable => self.create_instance(thread, depth, element, &dimensions)?,
            _ => match (self.new_array(thread, &element_type, &dimensions), element) {
                (Ok(array), _) => array,
                (Err(_), Some(element)) => self.create_instance(thread, depth, element, &dimensions)?,
                // 要素の型を書かない配列で、直に作れない要素の型（列挙型など）は、最初の要素の型で CreateInstance。
                (Err(failure), None) => {
                    let shown = unsafe { value::type_name(&element_type, &self.modules) };
                    let element = parse_type(&shown).ok_or_else(|| tr!("Cannot create the array: {reason}", reason = failure.describe(self)))?;
                    self.create_instance(thread, depth, &element, &dimensions)?
                }
            },
        };
        for (index, value) in values.iter().enumerate() {
            let slot = unsafe { value::array_element(&array, &[index as u32]) }?.ok_or_else(|| tr!("Cannot read an element of the created array."))?;
            self.write(thread, &slot, value)?;
        }
        Ok(array)
    }

    /// `System.Array.CreateInstance(要素の型, 大きさ…)` で配列を作る。
    fn create_instance(&mut self, thread: u32, depth: usize, element: &TypeSyntax, dimensions: &[u32]) -> Result<ComPtr<ICorDebugValue>, String> {
        let type_object = self.evaluate_node(thread, depth, &Expr::TypeOf(element.clone()))?;
        let type_object = self.value_of(type_object)?;
        self.create_instance_of(thread, depth, type_object, dimensions)
    }

    /// `System.Array.CreateInstance(要素の型, 大きさ…)`。`type_object` は、要素の型の `System.Type`。
    fn create_instance_of(&mut self, thread: u32, depth: usize, type_object: ComPtr<ICorDebugValue>, dimensions: &[u32]) -> Result<ComPtr<ICorDebugValue>, String> {
        let int_type = self.runtime_type(thread, depth, &TypeSyntax::named("System.Int32"))?;
        let lengths = self.new_array(thread, &int_type, &[dimensions.len() as u32]).map_err(|failure| failure.describe(self))?;
        for (index, size) in dimensions.iter().enumerate() {
            let slot = unsafe { value::array_element(&lengths, &[index as u32]) }?.ok_or_else(|| tr!("Cannot create the array."))?;
            let size = self.new_int(thread, *size as i32)?;
            self.write(thread, &slot, &size)?;
        }
        let (module, row) = self.find_type(thread, depth, "System.Array").ok_or_else(|| tr!("System.Array is not found in the target."))?;
        self.call_method(thread, depth, Some(Current::Type { module, row }), "CreateInstance", vec![type_object, lengths])?.ok_or_else(|| tr!("Cannot create the array."))
    }

    /// その名前の並びの匿名型。プログラムの中に（C# のコンパイラが作ったものか、前に読み込ませたものが）無ければ、同じ形の型を
    /// 入れたアセンブリを書き出して、対象に読み込ませる（`anonymous.rs`、docs/DECISIONS.md D-038）。
    pub(super) fn anonymous_shape(&mut self, thread: u32, depth: usize, names: &[&str]) -> Result<(usize, u32), String> {
        let find = |session: &Session| session.modules.iter().enumerate().find_map(|(index, module)| Some((index, module.metadata()?.anonymous_type(names)?)));
        if let Some(found) = find(self) {
            return Ok(found);
        }
        self.anonymous_assemblies += 1;
        let target = self.process.as_ref().map_or(0, |process| {
            let mut id = 0u32;
            unsafe { process.GetID(&mut id) };
            id
        });
        let name = format!("{}{target}_{}", crate::debugger::anonymous::file_prefix(), self.anonymous_assemblies);
        let directory = crate::debugger::anonymous::directory();
        std::fs::create_dir_all(&directory).map_err(|error| tr!("Cannot create the folder for the anonymous type assembly ({folder}): {error}", folder = directory.display(), error = error))?;
        let path = directory.join(format!("{name}.dll"));
        let stamp = (std::process::id() as u128) << 64 | (target as u128) << 32 | self.anonymous_assemblies as u128;
        std::fs::write(&path, crate::debugger::anonymous::assembly(&name, names, stamp)).map_err(|error| tr!("Cannot write the anonymous type assembly ({path}): {error}", path = path.display(), error = error))?;
        // 読み込みは、評価の途中に知らされるモジュールの読み込みとして記録されるので、そのあとは名前で探せる。
        let shown = path.display().to_string().replace('"', "\"\"");
        self.evaluate_text(thread, depth, &format!("System.Reflection.Assembly.LoadFrom(@\"{shown}\")"))?;
        find(self).ok_or_else(|| tr!("The anonymous type assembly ({path}) could not be loaded into the target.", path = path.display()))
    }

    /// 匿名型（`new { A = 1, B = "x" }`）。
    pub(super) fn create_anonymous(&mut self, thread: u32, depth: usize, members: &[(String, Expr)]) -> Result<ComPtr<ICorDebugValue>, String> {
        let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
        let (module, row) = self.anonymous_shape(thread, depth, &names)?;
        let exprs: Vec<Expr> = members.iter().map(|(_, expr)| expr.clone()).collect();
        let mut values = self.evaluate_arguments(thread, depth, &exprs)?;
        let mut types = Vec::new();
        for value in &values {
            types.push(unsafe { value::exact_type_of(value) }.ok_or_else(|| tr!("null cannot be written for an anonymous type member (its type cannot be determined)."))?);
        }
        let mut candidates = self.type_method_candidates(module, row, ".ctor", false, values.len() as u32);
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = types.clone();
        }
        let constructor = self.choose_overload(thread, candidates, &mut values).ok_or_else(|| tr!("Cannot create the anonymous type."))?;
        self.new_object(thread, &constructor, &values).map_err(|failure| tr!("Cannot create the anonymous type: {reason}", reason = failure.describe(self)))
    }
}
