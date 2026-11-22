//! 変数の表示（scopes / variables）と evaluate。プロパティ、静的メンバー、定数、DebuggerDisplay。

use super::*;

impl Session {
    /// フレームの `this`、引数、ローカル変数（この順）。最適化で消えた変数は出さない。
    pub(super) fn frame_variables(&self, thread: u32, depth: usize) -> Result<Vec<(String, ComPtr<ICorDebugValue>)>, String> {
        let frames = self.walk_frames(thread)?;
        let frame = frames.get(depth).ok_or_else(|| tr!("There is no frame."))?;
        let mut variables = Vec::new();
        unsafe {
            // マネージドのコードでないフレームには変数が無い。
            let Ok(il_frame) = frame.cast::<ICorDebugILFrame>() else { return Ok(variables) };
            let Some(module) = self.frame_module(frame) else { return Ok(variables) };
            let mut token = 0u32;
            frame.GetFunctionToken(&mut token);
            let mut hoisted = Vec::new();
            let mut argument = |index: u32, name: &str| {
                let mut raw = null_mut();
                il_frame.GetArgument(index, &mut raw);
                if let Some(value) = ComPtr::from_raw(raw) {
                    variables.push((name.to_string(), value));
                }
            };
            if let Some(metadata) = module.metadata() {
                let row = token & 0x00FF_FFFF;
                let has_this = metadata.method_has_this(row);
                // async メソッドやイテレーターの本体は、コンパイラが作った状態機械の MoveNext。そこでの `this` は状態機械で、
                // もとのメソッドの引数とローカル変数（await をまたぐもの）は、そのフィールドになっている。もとの名前で見せる。
                // ラムダの本体も同じで、`this` はクロージャ（取り込まれた変数の入れ物）。
                let generated = has_this && metadata.declaring_type(row).map(|ty| metadata.type_name(ty)).is_some_and(|name| value::is_state_machine(&name) || value::is_closure(&name));
                if generated {
                    let mut raw = null_mut();
                    il_frame.GetArgument(0, &mut raw);
                    if let Some(container) = ComPtr::<ICorDebugValue>::from_raw(raw) {
                        self.hoisted_variables(&container, 0, &mut hoisted);
                    }
                } else if has_this {
                    argument(0, "this");
                }
                for (index, name) in metadata.parameter_names(row).iter().enumerate() {
                    argument(index as u32 + has_this as u32, name);
                }
            }
            // `this` を先頭にする（ほかの順は、宣言の順のまま）。
            hoisted.sort_by_key(|(name, _)| name != "this");
            variables.extend(hoisted);
            if let Some(pdb) = module.pdb.as_ref() {
                let (mut offset, mut mapping) = (0u32, 0);
                il_frame.GetIP(&mut offset, &mut mapping);
                for local in pdb.locals(token, offset, frame_version(frame)) {
                    let mut raw = null_mut();
                    il_frame.GetLocalVariable(local.index, &mut raw);
                    let Some(value) = ComPtr::<ICorDebugValue>::from_raw(raw) else { continue };
                    if !value::holds_closure(&local.name) {
                        variables.push((local.name.clone(), value));
                        continue;
                    }
                    // ラムダに取り込まれた変数は、クロージャの中にある。もとの名前で見せる。引数は、取り込まれると写しが
                    // クロージャに入り、以後はそちらが使われるので、同じ名前の引数はクロージャの値で置き換える。
                    let mut captured = Vec::new();
                    self.hoisted_variables(&value, 0, &mut captured);
                    for (name, value) in captured {
                        match variables.iter_mut().find(|(existing, _)| *existing == name) {
                            Some(existing) => existing.1 = value,
                            None => variables.push((name, value)),
                        }
                    }
                }
            }
        }
        Ok(variables)
    }

    /// コンパイラが作った入れ物（状態機械、クロージャ）のフィールドを、もとの変数として並べる。
    /// 入れ物の中に別の入れ物（外側のクロージャ）があれば、その中もたどる。
    fn hoisted_variables(&self, container: &ComPtr<ICorDebugValue>, nesting: u32, variables: &mut Vec<(String, ComPtr<ICorDebugValue>)>) {
        const MAX_NESTING: u32 = 8;
        if nesting > MAX_NESTING {
            return;
        }
        for member in unsafe { value::children(container, &self.modules, 0, usize::MAX) } {
            let Member::Value { name, value } = member else { continue };
            if value::holds_closure(&name) {
                self.hoisted_variables(&value, nesting + 1, variables);
            } else if let Some(name) = value::state_machine_variable(&name) {
                if !variables.iter().any(|(existing, _)| existing == name) {
                    variables.push((name.to_string(), value));
                }
            }
        }
    }

    pub(super) fn new_handle(&mut self, handle: Handle) -> i64 {
        self.handles.push(handle);
        self.handles.len() as i64
    }

    /// `scopes`。netcoredbg と同じく、引数とローカル変数をまとめた `Locals` 一つだけを返す。
    pub fn scopes(&mut self, arguments: &Value) -> Result<Value, String> {
        // 非同期の論理的な呼び出し元（実際のフレームではない。async_stack.rs）には、変数は無い。
        if arguments.get("frameId").and_then(Value::as_i64).is_some_and(|id| id >= async_stack::LOGICAL_FRAME_BASE) {
            return Ok(json!({ "scopes": [] }));
        }
        let (thread, depth) = self.frame_of(arguments)?;
        let reference = self.new_handle(Handle::Locals { thread, depth });
        Ok(json!({ "scopes": [{ "name": "Locals", "variablesReference": reference, "expensive": false }] }))
    }

    /// `variables`。
    pub fn variables(&mut self, arguments: &Value) -> Result<Value, String> {
        let reference = arguments.get("variablesReference").and_then(Value::as_u64).ok_or_else(|| tr!("variables has no variablesReference."))? as usize;
        let start = arguments.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let count = arguments.get("count").and_then(Value::as_u64).filter(|&c| c > 0).map_or(usize::MAX, |c| c as usize);
        if self.state != State::Stopped {
            return Err(tr!("Variables can be read only while the target is stopped."));
        }
        let handle = reference.checked_sub(1).and_then(|index| self.handles.get(index)).cloned().ok_or_else(|| tr!("That variablesReference is not from the current stop."))?;
        let thread = handle.thread();
        // 中のメンバーの `evaluateName`（そのメンバーを指す式。netcoredbg と同じ形）の作り方。
        let paths = match &handle {
            Handle::Locals { .. } => ChildPaths::Local,
            Handle::Children { path: Some(parent), .. } | Handle::Raw { path: Some(parent), .. } => ChildPaths::Member(parent.clone()),
            Handle::Statics { ty, .. } => ChildPaths::Static(unsafe { value::type_name(ty, &self.modules) }),
            _ => ChildPaths::Unknown,
        };
        // いちばん上のフレームの変数の先頭に、直前のステップで呼ばれたメソッドの戻り値を見せる（Visual Studio と同じ）。
        let mut variables = match &handle {
            Handle::Locals { thread, depth: 0 } => self.return_value_variables(*thread),
            _ => Vec::new(),
        };
        let Listed { members, proxied, raw_view } = self.members_of(handle, start, count)?;
        for member in members {
            let variable = match member {
                Member::Value { name, value } => {
                    // 代わりの型のメンバーは、もとの値のメンバーではない。要素（`[0]`）だけは、もとの値の添字で指せる。
                    let path = if proxied && !name.starts_with('[') { None } else { paths.of(&name) };
                    self.describe_variable(&name, value, thread, path)
                }
                Member::Note { name, text } => json!({ "name": name, "value": text, "type": "", "variablesReference": 0 }),
                Member::Statics(ty) => json!({ "name": "Static members", "value": "", "type": "", "variablesReference": self.new_handle(Handle::Statics { ty, thread }), "presentationHint": { "kind": "class" } }),
                Member::Lazy { name, getter, this } => {
                    let reference = self.new_handle(Handle::Lazy { name: name.clone(), getter, this, thread });
                    json!({ "name": name, "value": tr_str!("<click to run the getter>"), "type": "", "variablesReference": reference, "presentationHint": { "kind": "property", "lazy": true } })
                }
            };
            variables.push(variable);
        }
        if let Some(reference) = raw_view {
            variables.push(json!({ "name": "Raw View", "value": "", "type": "", "variablesReference": reference, "presentationHint": { "kind": "virtual" } }));
        }
        Ok(json!({ "variables": variables }))
    }

    /// 節の中に並べるもの。
    fn members_of(&mut self, handle: Handle, start: usize, count: usize) -> Result<Listed, String> {
        let members = match handle {
            Handle::Locals { thread, depth } => self.frame_variables(thread, depth)?.into_iter().map(|(name, value)| Member::Value { name, value }).collect(),
            Handle::Children { value, thread, path } => match self.type_proxy_of(thread, &value) {
                // DebuggerTypeProxy: 代わりの型のメンバーを見せ、もとのメンバーは「Raw View」の中に入れる。
                Some(proxy) => {
                    let raw_view = self.new_handle(Handle::Raw { value, thread, path });
                    return Ok(Listed { members: self.proxy_members(&proxy, thread), proxied: true, raw_view: Some(raw_view) });
                }
                None => self.browsable_members(&value, thread, start, count),
            },
            Handle::Raw { value, thread, .. } => self.browsable_members(&value, thread, start, count),
            Handle::Statics { ty, thread } => self.static_members(&ty, thread),
            Handle::Pointee { value, .. } => vec![Member::Value { name: String::new(), value }],
            // クリックで評価するプロパティ: ここで初めて getter を実行し、値を 1 つ返す（DAP の lazy の約束）。
            Handle::Lazy { name, getter, this, thread } => {
                let mut members = Vec::new();
                self.run_property(&mut members, name, &getter, this.as_slice(), thread);
                members
            }
        };
        Ok(Listed { members, proxied: false, raw_view: None })
    }

    /// 代わりの型（DebuggerTypeProxy）のオブジェクトのメンバー。公開されていないフィールド（もとのオブジェクトを持つ
    /// フィールドなど）は見せない。
    fn proxy_members(&mut self, proxy: &ComPtr<ICorDebugValue>, thread: u32) -> Vec<Member> {
        let hidden = unsafe { value::class_of_value(proxy) }.and_then(|(module, token)| Some(self.modules.get(self.module_index(&module)?)?.metadata()?.non_public_fields(token & 0x00FF_FFFF)));
        let mut members = self.browsable_members(proxy, thread, 0, usize::MAX);
        members.retain(|member| !matches!(member, Member::Value { name, .. } if hidden.as_ref().is_some_and(|hidden| hidden.contains(name))));
        members
    }

    /// 直前のステップで呼ばれたメソッドの戻り値（``Type.Method returned``）の変数。
    fn return_value_variables(&mut self, thread: u32) -> Vec<Value> {
        let returned: Vec<(String, String, String, Option<ComPtr<ICorDebugValue>>)> =
            self.return_values.iter().filter(|v| v.thread == thread).map(|v| (format!("{} returned", v.name), v.value.clone(), v.type_name.clone(), v.handle.clone())).collect();
        returned
            .into_iter()
            .map(|(name, value, type_name, value_handle)| {
                let reference = value_handle.map_or(0, |h| self.new_handle(Handle::Children { value: h, thread, path: None }));
                json!({ "name": name, "value": value, "type": type_name, "variablesReference": reference })
            })
            .collect()
    }
    /// 値の中身（フィールド、プロパティ、配列の要素）。`[DebuggerBrowsable]` に従い、Never は出さず、RootHidden は中身を
    /// その場に並べる（C# と Visual Studio の意味。netcoredbg は、プロパティの Never だけを見ている）。
    fn browsable_members(&mut self, value: &ComPtr<ICorDebugValue>, thread: u32, start: usize, count: usize) -> Vec<Member> {
        let mut members = unsafe { value::children(value, &self.modules, start, count) };
        self.append_properties(&mut members, value, thread);
        let (never, root_hidden) = unsafe { value::browsable_rules(value, &self.modules) };
        if never.is_empty() && root_hidden.is_empty() {
            return members;
        }
        let mut shown = Vec::with_capacity(members.len());
        for member in members {
            match member {
                Member::Value { name, .. } if never.contains(&name) => {}
                Member::Value { name, value } if root_hidden.contains(&name) => {
                    // null は、並べる中身が無い。中身の中の RootHidden は、そのまま（入れ子は一段だけ開く）。
                    if unsafe { value::argument_kind(&value) } != value::ArgumentKind::Null {
                        let mut inner = unsafe { value::children(&value, &self.modules, 0, usize::MAX) };
                        self.append_properties(&mut inner, &value, thread);
                        shown.extend(inner.into_iter().filter(|member| !matches!(member, Member::Statics(_))));
                    }
                }
                other => shown.push(other),
            }
        }
        shown
    }

    /// 値の型に `[DebuggerTypeProxy]` が付いていれば、代わりの型のオブジェクトを作って返す。利用者の型も、BCL の型
    /// （`List<T>` の `ICollectionDebugView<T>`、`Dictionary<K, V>` の `IDictionaryDebugView<K, V>` など）も
    /// （Visual Studio と同じ。docs/DECISIONS.md D-031 の追記）。代わりの型がジェネリックなら、もとの型の型引数を付けて作る。
    fn type_proxy_of(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        if !self.implicit_evaluation() {
            return None;
        }
        let (module, token) = unsafe { value::class_of_value(value) }?;
        let index = self.module_index(&module)?;
        let proxy_name = self.modules[index].metadata()?.type_proxy(token & 0x00FF_FFFF)?;
        // `名前`1, アセンブリ` の形。型の名前と、型引数の数に分ける。
        let full = proxy_name.split(',').next()?.trim();
        let (name, arity) = match full.split_once('`') {
            Some((name, arity)) => (name, arity.parse::<usize>().ok()?),
            None => (full, 0),
        };
        let type_arguments = if arity == 0 { Vec::new() } else { unsafe { value::exact_type_of(value).map(|ty| value::type_arguments_of(&ty)) }.unwrap_or_default() };
        if type_arguments.len() != arity {
            return None;
        }
        // 代わりの型は、同じモジュールにあることが多い。無ければ、ほかのモジュールも探す。
        let (proxy_module, proxy_row) = std::iter::once(index).chain(0..self.modules.len()).find_map(|candidate| {
            let metadata = self.modules[candidate].metadata()?;
            let row = if arity == 0 { metadata.find_type(name) } else { metadata.find_generic_type(name, arity) };
            Some((candidate, row?))
        })?;
        let mut arguments = vec![value.clone()];
        let mut candidates = self.type_method_candidates(proxy_module, proxy_row, ".ctor", false, 1);
        for (callable, _) in candidates.iter_mut() {
            callable.type_arguments = type_arguments.clone();
        }
        let constructor = self.choose_overload(thread, candidates, &mut arguments)?;
        self.new_object(thread, &constructor, &arguments).ok()
    }

    /// 型と基底の型の静的メンバー（netcoredbg と同じ。拡張の「イベントで止める」は、基底の型の `Control.s_clickEvent` を
    /// ここから探す）。派生した型の分が先で、同じ名前は派生した型のものだけ。型ごとには、フィールドを宣言の順に、そのあと
    /// 静的プロパティ。定数も、同じ値を対象の中に作って見せる。まだ初期化されていない型の静的フィールドは読めないので、出さない。
    pub(super) fn static_members(&mut self, ty: &ComPtr<ICorDebugType>, thread: u32) -> Vec<Member> {
        let mut members = Vec::new();
        let mut current = Some(ty.clone());
        // 壊れた情報で輪になっても止まるよう、深さを限る。
        for depth in 0..64 {
            let Some(ty) = current else { break };
            // System.Object / ValueType / Enum の静的メンバーは出さない（列挙型の節は、その列挙値だけ。netcoredbg と同じ）。
            let name = unsafe { value::type_name(&ty, &self.modules) };
            if depth > 0 && matches!(name.as_str(), "object" | "System.ValueType" | "System.Enum") {
                break;
            }
            for member in self.declared_static_members(&ty, thread) {
                let duplicate = member.name().is_some_and(|name| members.iter().any(|existing: &Member| existing.name() == Some(name)));
                if !duplicate {
                    members.push(member);
                }
            }
            current = unsafe { take::<ICorDebugType>("ICorDebugType::GetBase", |out| ty.GetBase(out)) }.ok();
        }
        members
    }

    /// 型自身が宣言している静的メンバー。
    fn declared_static_members(&mut self, ty: &ComPtr<ICorDebugType>, thread: u32) -> Vec<Member> {
        let Some((_, module, type_row)) = (unsafe { value::class_of(ty) }) else { return Vec::new() };
        let Some(index) = self.module_index(&module) else { return Vec::new() };
        // 名前などを写してから読む（読むときに self を借りるので、メタデータを借りたままにできない）。
        let Some(metadata) = self.modules[index].metadata() else { return Vec::new() };
        let fields: Vec<(String, u32, bool)> = metadata.fields(type_row).into_iter().filter(|f| f.is_static).map(|f| (f.name.to_string(), f.token, f.is_literal)).collect();
        let properties: Vec<(String, u32)> = metadata.static_properties(type_row).into_iter().map(|(name, getter)| (name.to_string(), getter)).collect();
        let mut members = Vec::new();
        for (name, token, is_literal) in fields {
            let value = if is_literal {
                self.constant_value(thread, index, type_row, token).map_err(|e| crate::log!("constant {name}: {e}")).ok()
            } else {
                // スレッドごとの静的フィールドのために、開いているスレッドのいちばん上のフレームを渡す。
                let frame = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().next());
                unsafe { value::static_field(ty, token, frame.as_ref()) }
            };
            if let Some(value) = value {
                members.push(Member::Value { name: value::member_name(&name).to_string(), value });
            }
        }
        let type_arguments = unsafe { value::type_arguments_of(ty) };
        for (name, getter) in properties {
            // 自動実装の静的プロパティは、裏のフィールドがもうプロパティの名前で出ている。
            if members.iter().any(|m| matches!(m, Member::Value { name: existing, .. } if *existing == name)) {
                continue;
            }
            let Some(function) = (unsafe { take::<ICorDebugFunction>("ICorDebugModule::GetFunctionFromToken", |out| module.GetFunctionFromToken(getter, out)) }).ok() else { continue };
            let callable = value::Callable { function, type_arguments: type_arguments.clone() };
            if !self.push_property(&mut members, name, &callable, &[], thread) {
                break;
            }
        }
        members
    }

    /// 定数（`const`）のフィールドと同じ値を、対象の中に作る。定数は実体が無く、値はメタデータにある。
    /// 列挙型のメンバー（`Shade.Dark`）は、その列挙型の値にする（数ではなく、名前で表示される）。
    pub(super) fn constant_value(&mut self, thread: u32, module: usize, type_row: u32, field: u32) -> Result<ComPtr<ICorDebugValue>, String> {
        const ELEMENT_TYPE_STRING: u8 = 0x0E;
        const ELEMENT_TYPE_CLASS: u8 = 0x12;
        let metadata = self.modules[module].metadata().ok_or_else(|| tr!("Cannot read the metadata."))?;
        let (element, bytes) = metadata.constant(field).map(|(element, bytes)| (element, bytes.to_vec())).ok_or_else(|| tr!("The constant's value is not in the metadata."))?;
        // 列挙型は、中身の数を入れるインスタンスフィールド `value__` を一つだけ持つ。
        let enum_slot = metadata.field_is_value_type(field).then(|| metadata.fields(type_row).into_iter().find(|f| !f.is_static && f.name == "value__").map(|f| f.token)).flatten();
        match (element, enum_slot) {
            (ELEMENT_TYPE_STRING, _) => {
                let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                self.new_string(thread, &String::from_utf16_lossy(&units)).map_err(|failure| failure.describe(self))
            }
            (ELEMENT_TYPE_CLASS, _) => self.new_null(thread),
            // 列挙型の値は、空のボックスを作って中の数を書き込む（プリミティブ型の値を `this` にするときと同じ手）。
            // `ICorDebugEval2::CreateValueForType` で作った値は、フィールドとしても値としても書き込めなかった。
            (_, Some(slot)) => {
                let class = unsafe { take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| self.modules[module].module.GetClassFromToken(TYPE_DEF | type_row, out)) }?;
                let boxed = self.new_object_without_constructor(thread, &class).map_err(|failure| failure.describe(self))?;
                unsafe { value::fill_box(&boxed, &class, slot, &bytes) }?;
                Ok(boxed)
            }
            (element, None) => self.new_primitive(thread, element as u32, &bytes),
        }
    }

    /// オブジェクトのプロパティを、対象の getter を実行して読み、フィールドのあと（静的メンバーの節の前）に並べる。
    ///
    /// 自動実装プロパティは、裏のフィールドがもうプロパティの名前で出ているので、実行しない。getter が例外を投げたら、
    /// その例外を値として見せる（netcoredbg と同じ）。この停止では実行できないと分かったら、残りは読まない。
    pub(super) fn append_properties(&mut self, members: &mut Vec<Member>, target: &ComPtr<ICorDebugValue>, thread: u32) {
        let getters = unsafe { value::property_getters(target, &self.modules) };
        if getters.is_empty() {
            return;
        }
        let statics = match members.last() {
            Some(Member::Statics(_)) => members.pop(),
            _ => None,
        };
        for (name, getter) in getters {
            if members.iter().any(|m| matches!(m, Member::Value { name: existing, .. } if *existing == name)) {
                continue;
            }
            if !self.push_property(members, name, &getter, std::slice::from_ref(target), thread) {
                break;
            }
        }
        members.extend(statics);
    }

    /// プロパティをメンバーとして足す。ふつうは getter を実行して値（失敗なら理由）を、暗黙の評価をしない設定なら、
    /// クリックで評価する形（`Member::Lazy`）を足す。`this` は、静的プロパティなら空。残りのプロパティも読んでよければ true。
    fn push_property(&mut self, members: &mut Vec<Member>, name: String, getter: &value::Callable, this: &[ComPtr<ICorDebugValue>], thread: u32) -> bool {
        if !self.implicit_evaluation_allowed {
            members.push(Member::Lazy { name, getter: getter.clone(), this: this.first().cloned() });
            return true;
        }
        if self.implicit_evaluation_off {
            members.push(Member::Note { name, text: tr!("<properties are not read until continuing, because an earlier evaluation timed out>") });
            return true;
        }
        self.run_property(members, name, getter, this, thread)
    }

    /// getter を実行し、その値（失敗なら理由）をメンバーとして足す。残りのプロパティも読んでよければ true。
    fn run_property(&mut self, members: &mut Vec<Member>, name: String, getter: &value::Callable, this: &[ComPtr<ICorDebugValue>], thread: u32) -> bool {
        match self.call(thread, getter, this) {
            Ok(Some(value)) => members.push(Member::Value { name, value }),
            Ok(None) => {}
            Err(funceval::EvalFailure::Threw(exception)) => members.push(Member::Value { name, value: exception }),
            // 時間切れ: このプロパティには理由を書き、続行するまで暗黙の評価をやめる（残りは IMPLICIT_OFF になる）。
            Err(funceval::EvalFailure::TimedOut(message)) => {
                crate::log!("property {name}: {message}");
                self.implicit_evaluation_off = true;
                members.push(Member::Note { name, text: format!("<{message}>") });
            }
            // この停止では対象のコードを実行できない: 理由と直し方を見せ、残りは読まない（同じ理由で失敗する）。
            Err(funceval::EvalFailure::Unavailable(message)) => {
                members.push(Member::Note { name, text: format!("<{message}>") });
                return false;
            }
            Err(failure) => {
                let message = failure.describe(self);
                crate::log!("property {name}: {message}");
                // クリックで評価したときは、黙って消さずに理由を見せる（ほかに知らせる場所が無い）。
                if !self.implicit_evaluation_allowed {
                    members.push(Member::Note { name, text: format!("<{message}>") });
                }
                return false;
            }
        }
        true
    }

    /// 暗黙の評価（DebuggerDisplay、代わりの型）をしてよいか。設定で止めているときと、時間切れのあとは、しない。
    fn implicit_evaluation(&self) -> bool {
        self.implicit_evaluation_allowed && !self.implicit_evaluation_off
    }

    /// 型に `DebuggerDisplay` が付いていれば、その書式で作った文字列（`Count = 1`）。付いていない、または
    /// 作れない（扱えない式、メンバーが読めない）なら `None` で、呼び出し側はふつうの `{型名}` を見せる。
    pub(super) fn display_string(&mut self, target: &ComPtr<ICorDebugValue>, thread: u32) -> Option<String> {
        // 暗黙の評価をしない間は、DebuggerDisplay も使わない（ふつうの `{型名}` を見せる）。
        if !self.implicit_evaluation() {
            return None;
        }
        let format = unsafe { value::debugger_display(target, &self.modules) }?;
        self.formatted_display(target, thread, &format)
    }

    /// `DebuggerDisplay` の書式（`Count = {Count}`）を、オブジェクトを this として埋めた文字列。
    fn formatted_display(&mut self, target: &ComPtr<ICorDebugValue>, thread: u32, format: &str) -> Option<String> {
        let mut text = String::new();
        for part in value::parse_display_format(format)? {
            match part {
                value::DisplayPart::Text(literal) => text.push_str(&literal),
                value::DisplayPart::Member { name, no_quotes } => {
                    // `{…}` の中は、オブジェクトを this とした式（`{Count}`、`{Items.Count * 2}`、`{Describe()}`）。
                    // 評価できなければ、Visual Studio と同じく、その部分にエラーを書く。
                    let shown = match self.evaluate_on(thread, target, &name) {
                        Ok(eval::Evaluated::Value(member) | eval::Evaluated::Nullable(member)) => unsafe { value::describe(&member, &self.modules) }.value,
                        Ok(eval::Evaluated::Void) => String::new(),
                        Ok(eval::Evaluated::Pointer { target, address, .. }) => pointer_text(target.as_ref(), address, &self.modules),
                        Err(error) => format!("<{error}>"),
                    };
                    let unquoted = shown.strip_prefix('"').and_then(|s| s.strip_suffix('"')).filter(|_| no_quotes);
                    text.push_str(unquoted.unwrap_or(&shown));
                }
            }
        }
        Some(text)
    }

    /// `path` は、この値を指す式（`evaluateName`。中を開いたときのメンバーの式も、これから作る）。
    pub(super) fn describe_variable(&mut self, name: &str, value: ComPtr<ICorDebugValue>, thread: u32, path: Option<String>) -> Value {
        let mut path = path;
        let mut described = unsafe { value::describe(&value, &self.modules) };
        let mut name = name.to_string();
        if described.expandable {
            if let Some(text) = self.display_string(&value, thread) {
                described.value = text;
            } else if FORMATTED_TYPES.contains(&described.type_name.as_str()) && self.implicit_evaluation() {
                // 日時、時間、GUID は、中身（ticks など）より ToString() の方が読める（Visual Studio と同じく `{…}` で囲む）。
                if let Some(text) = self.string_property(thread, &value, "ToString") {
                    described.value = format!("{{{text}}}");
                }
            }
            // `DebuggerDisplay` の `Name`（`Dictionary<K, V>` の要素の `[{Key}]` など）は、変数の欄の名前にする。
            if !self.implicit_evaluation_off {
                if let Some(shown) = unsafe { value::debugger_display_name(&value, &self.modules) }.and_then(|format| self.formatted_display(&value, thread, &format)) {
                    name = shown;
                    // 見せる名前が変わったので、もとの位置（`[0]`）の式は、見た目と合わない（辞書の要素など）。付けない。
                    path = None;
                }
            }
        }
        // オブジェクト ID の付いたオブジェクトは、値のあとに ` {$1}`（Visual Studio と同じ）。
        described.value.push_str(&self.object_id_suffix(&value));
        let reference = if described.expandable { self.new_handle(Handle::Children { value, thread, path: path.clone() }) } else { 0 };
        let mut variable = json!({ "name": name, "value": described.value, "type": described.type_name, "variablesReference": reference });
        if let Some(path) = path {
            variable["evaluateName"] = json!(path);
        }
        variable
    }

    /// `evaluate`。式の書き方は `session/eval.rs`。
    /// `setVariable`: 変数の欄で、変数・フィールド・プロパティ・要素の値を書き換える。値は式（`42`、`sum + 1`、`"x"`）。
    /// 応答は、書き換えたあとの値（netcoredbg は value だけを返す。こちらは型と、開ける値なら参照も返す）。
    pub fn set_variable(&mut self, arguments: &Value) -> Result<Value, String> {
        let reference = arguments.get("variablesReference").and_then(Value::as_u64).ok_or_else(|| tr!("setVariable has no variablesReference."))? as usize;
        let name = arguments.get("name").and_then(Value::as_str).ok_or_else(|| tr!("setVariable has no name."))?;
        let text = arguments.get("value").and_then(Value::as_str).ok_or_else(|| tr!("setVariable has no value."))?;
        if self.state != State::Stopped {
            return Err(tr!("Variables can be changed only while the target is stopped."));
        }
        let handle = reference.checked_sub(1).and_then(|index| self.handles.get(index)).cloned().ok_or_else(|| tr!("That variablesReference is not from the current stop."))?;
        let (thread, written) = match handle {
            Handle::Locals { thread, depth } => (thread, self.set_local(thread, depth, name, text)?),
            Handle::Children { value, thread, .. } | Handle::Raw { value, thread, .. } => (thread, self.set_member(thread, value, name, text)?),
            Handle::Statics { ty, thread } => {
                // 静的メンバーは、型の完全な名前から指す。
                let type_name = unsafe { value::class_of(&ty) }.and_then(|(_, module, row)| {
                    Some(self.modules.get(self.module_index(&module)?)?.metadata()?.plain_name(row & 0x00FF_FFFF))
                });
                let type_name = type_name.ok_or_else(|| tr!("The type name is not known."))?;
                (thread, self.set_static(thread, &type_name, name, text)?)
            }
            // クリックで評価したプロパティの値を書き換える: 持ち主のオブジェクトのプロパティとして書く。
            Handle::Lazy { this: Some(value), thread, .. } => (thread, self.set_member(thread, value, name, text)?),
            Handle::Pointee { .. } => return Err(tr!("What a computed pointer points at cannot be changed here. Assign it in Watch or the Debug Console, like *p = 1.")),
            Handle::Lazy { this: None, .. } => return Err(tr!("This static property cannot be changed here. Assign it in Watch or the Debug Console.")),
        };
        let described = self.describe_variable(name, written, thread, None);
        Ok(json!({ "value": described["value"], "type": described["type"], "variablesReference": described["variablesReference"] }))
    }

    /// `setExpression`: フレームの文脈で、式が指す場所（変数、フィールド、プロパティ、添字）の値を書き換える。
    pub fn set_expression(&mut self, arguments: &Value) -> Result<Value, String> {
        let expression = arguments.get("expression").and_then(Value::as_str).map(str::trim).filter(|e| !e.is_empty()).ok_or_else(|| tr!("setExpression has no expression."))?;
        let text = arguments.get("value").and_then(Value::as_str).ok_or_else(|| tr!("setExpression has no value."))?;
        let (thread, depth) = self.frame_of(arguments)?;
        let written = self.set_place(thread, depth, expression, text)?;
        let described = self.describe_variable(expression, written, thread, Some(expression.to_string()));
        Ok(json!({ "value": described["value"], "type": described["type"], "variablesReference": described["variablesReference"] }))
    }

    pub fn evaluate(&mut self, arguments: &Value) -> Result<Value, String> {
        let expression = arguments.get("expression").and_then(Value::as_str).map(str::trim).filter(|e| !e.is_empty()).ok_or_else(|| tr!("evaluate has no expression."))?;
        let (thread, depth) = self.frame_of(arguments)?;
        let (value, nullable) = match self.evaluate_expression(thread, depth, expression)? {
            eval::Evaluated::Value(value) => (value, false),
            eval::Evaluated::Nullable(value) => (value, true),
            // 文言と型は netcoredbg と同じ（拡張は、成功の応答であることだけを見ている）。
            eval::Evaluated::Void => return Ok(json!({ "result": "Expression has been evaluated and has no value", "type": "void", "variablesReference": 0 })),
            // 計算したポインター: ポインターの変数と同じ見せ方（値の欄に指す先、開くと指す先が 1 つ）。
            eval::Evaluated::Pointer { type_name, target, address } => {
                let result = pointer_text(target.as_ref(), address, &self.modules);
                let reference = target.map_or(0, |value| self.new_handle(Handle::Pointee { value, thread }));
                return Ok(json!({ "result": result, "type": type_name, "variablesReference": reference }));
            }
        };
        let described = self.describe_variable(expression, value, thread, Some(expression.to_string()));
        // 計算の結果が null 許容の値型（`maybe + 1` は int?）のとき、値は中身の型で持っているので、型の名前に ? を足す。
        let type_name = match described["type"].as_str() {
            Some(name) if nullable && name != "object" && !name.ends_with('?') => json!(format!("{name}?")),
            _ => described["type"].clone(),
        };
        Ok(json!({ "result": described["value"], "type": type_name, "variablesReference": described["variablesReference"] }))
    }
}

/// `variables` の節に並べるものと、その出どころ。
struct Listed {
    members: Vec<Member>,
    /// 代わりの型（DebuggerTypeProxy）のメンバーか。
    proxied: bool,
    /// 代わりの型を見せたときの、もとのメンバーの節（「Raw View」）の variablesReference。
    raw_view: Option<i64>,
}

/// 開いた節の中のメンバーの `evaluateName`（そのメンバーを指す式。VS Code の「ウォッチに追加」などが使う）の作り方。
enum ChildPaths {
    /// ローカル変数: 名前そのもの。
    Local,
    /// 開いた値（式）のメンバー: `親.名前`、要素は `親[0]`。
    Member(String),
    /// 型の静的メンバー: `型.名前`。
    Static(String),
    /// 式で指せない節の中（ポインターの指す先など）。
    Unknown,
}

impl ChildPaths {
    fn of(&self, name: &str) -> Option<String> {
        match self {
            ChildPaths::Local => Some(name.to_string()),
            ChildPaths::Member(parent) => member_path(parent, name),
            ChildPaths::Static(type_name) => Some(format!("{type_name}.{name}")),
            ChildPaths::Unknown => None,
        }
    }
}

/// 開いた値の式 `parent` の、メンバー `name` を指す式。要素（`[0]`、`[1, 2]`）は添字、それ以外は `.名前`。
/// 名前の無いもの（ポインターの指す先）や、式にならない名前は `None`。
fn member_path(parent: &str, name: &str) -> Option<String> {
    if name.starts_with('[') {
        return Some(format!("{parent}{name}"));
    }
    let identifier = name.chars().next().is_some_and(|c| c == '_' || c.is_alphabetic()) && name.chars().all(|c| c == '_' || c.is_alphanumeric());
    identifier.then(|| format!("{parent}.{name}"))
}

/// 値の欄に、ToString() の結果を見せる型（DebuggerDisplay を持たない、BCL の日時・時間・GUID）。
const FORMATTED_TYPES: &[&str] = &["System.DateTime", "System.DateTimeOffset", "System.TimeSpan", "System.Guid", "System.DateOnly", "System.TimeOnly"];

/// ポインターの値の欄: 指す先の値。null なら `null`、指す先の無い void* は番地。
fn pointer_text(target: Option<&ComPtr<ICorDebugValue>>, address: u64, modules: &dyn value::Modules) -> String {
    match target {
        Some(target) => unsafe { value::describe(target, modules) }.value,
        None if address == 0 => "null".into(),
        None => format!("0x{address:0width$X}", width = std::mem::size_of::<usize>() * 2),
    }
}
