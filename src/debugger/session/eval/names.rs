//! 名前の解決: 変数、this のメンバー、静的メンバー、型と名前空間、添字。

use super::*;

/// C# の SDK の暗黙の using（`<ImplicitUsings>enable</ImplicitUsings>` のコンソールアプリの分）。
const IMPLICIT_USINGS: [&str; 7] = ["System", "System.Collections.Generic", "System.IO", "System.Linq", "System.Net.Http", "System.Threading", "System.Threading.Tasks"];

impl Session {
    /// 名前を、変数 → `this` のメンバー → いまの型の静的メンバー、の順に探す。どれでもなければ `None`。
    pub(super) fn resolve_name(&mut self, thread: u32, depth: usize, name: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        if name == "$exception" {
            return self.current_exception(thread).map(Some).ok_or_else(|| tr!("This thread is not handling an exception now."));
        }
        // オブジェクト ID（拡張が `$1` を書き換えて送ってくる名前）。
        if let Some(found) = self.object_id_value(name) {
            return found.map(Some);
        }
        // パターンが作った変数が、いちばん近い。
        if let Some((_, value)) = self.pattern_locals.iter().rev().find(|(existing, _)| existing == name) {
            return Ok(Some(value.clone()));
        }
        // DebuggerDisplay の式では、表示しているオブジェクトが this で、名前はそのメンバー。
        if let Some(this) = self.display_this.clone() {
            if name == "this" {
                return Ok(Some(this));
            }
            if let Ok(member) = self.member(thread, &this, name) {
                return Ok(Some(member));
            }
        }
        let mut variables = self.frame_variables(thread, depth)?;
        if let Some(index) = variables.iter().position(|(existing, _)| existing == name) {
            let value = variables.swap_remove(index).1;
            let kind = self.variable_kind(thread, depth, name);
            self.remember_object_typed(&value, kind);
            return Ok(Some(value));
        }
        if let Some((_, this)) = variables.into_iter().find(|(existing, _)| existing == "this") {
            if let Ok(member) = self.member(thread, &this, name) {
                return Ok(Some(member));
            }
        }
        for (module, row) in self.frame_types(thread, depth) {
            if let Some(found) = self.static_member(thread, depth, module, row, name)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// 引数かローカル変数の、宣言の型の大まかな種類（メソッドのシグネチャと、ローカル変数のシグネチャから）。
    /// クロージャや状態機械に移された変数は、移した先のフィールドの型から。
    pub(super) fn variable_kind(&self, thread: u32, depth: usize, name: &str) -> Option<ParameterKind> {
        self.declared_variable_kind(thread, depth, name).or_else(|| self.hoisted_variable_kind(thread, depth, name))
    }

    /// クロージャか状態機械に移された変数の、フィールドの宣言の型の大まかな種類。
    fn hoisted_variable_kind(&self, thread: u32, depth: usize, name: &str) -> Option<ParameterKind> {
        let frames = self.walk_frames(thread).ok()?;
        let frame = frames.get(depth)?;
        let mut containers = Vec::new();
        unsafe {
            let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
            let module = self.frame_module(frame)?;
            let mut token = 0u32;
            frame.GetFunctionToken(&mut token);
            // ラムダの本体や状態機械では、`this` が入れ物。
            let mut raw = null_mut();
            il_frame.GetArgument(0, &mut raw);
            containers.extend(ComPtr::<ICorDebugValue>::from_raw(raw));
            let (mut offset, mut mapping) = (0u32, 0);
            il_frame.GetIP(&mut offset, &mut mapping);
            for local in module.pdb.as_ref().map(|pdb| pdb.locals(token, offset, frame_version(frame))).unwrap_or_default() {
                if value::holds_closure(&local.name) {
                    let mut raw = null_mut();
                    il_frame.GetLocalVariable(local.index, &mut raw);
                    containers.extend(ComPtr::<ICorDebugValue>::from_raw(raw));
                }
            }
        }
        containers.iter().find_map(|container| self.field_kind_in(container, name, 0))
    }

    /// 入れ物（クロージャ、状態機械）の中の、もとの名前が `name` の変数のフィールドの型の種類。入れ子の入れ物もたどる。
    fn field_kind_in(&self, container: &ComPtr<ICorDebugValue>, name: &str, nesting: u32) -> Option<ParameterKind> {
        const MAX_NESTING: u32 = 8;
        if nesting > MAX_NESTING {
            return None;
        }
        let (module, row) = self.class_row(container)?;
        let metadata = self.modules[module].metadata()?;
        let type_name = metadata.type_name(row);
        if !value::is_closure(&type_name) && !value::is_state_machine(&type_name) {
            return None;
        }
        for field in metadata.fields(row).into_iter().filter(|field| !field.is_static) {
            if value::holds_closure(field.name) {
                let inner = unsafe { value::field(container, &self.modules, field.name) };
                if let Some(kind) = inner.and_then(|inner| self.field_kind_in(&inner, name, nesting + 1)) {
                    return Some(kind);
                }
            } else if value::state_machine_variable(field.name) == Some(name) {
                return metadata.field_kind(field.token);
            }
        }
        None
    }

    /// 引数とローカル変数（入れ物に移されていないもの）の、宣言の型の大まかな種類。
    fn declared_variable_kind(&self, thread: u32, depth: usize, name: &str) -> Option<ParameterKind> {
        let frames = self.walk_frames(thread).ok()?;
        let frame = frames.get(depth)?;
        unsafe {
            let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
            let module = self.frame_module(frame)?;
            let metadata = module.metadata()?;
            let mut token = 0u32;
            frame.GetFunctionToken(&mut token);
            if let Some(index) = metadata.parameter_names(token & 0x00FF_FFFF).iter().position(|parameter| *parameter == name) {
                return metadata.parameter_kinds(token)?.get(index).cloned();
            }
            let (mut offset, mut mapping) = (0u32, 0);
            il_frame.GetIP(&mut offset, &mut mapping);
            let local = module.pdb.as_ref()?.locals(token, offset, frame_version(frame)).into_iter().find(|local| local.name == name)?.index;
            let function = take::<ICorDebugFunction>("ICorDebugFrame::GetFunction", |out| frame.GetFunction(out)).ok()?;
            let mut signature = 0u32;
            function.GetLocalVarSigToken(&mut signature).ok().ok()?;
            metadata.local_kinds(signature)?.get(local as usize).cloned()
        }
    }

    /// 値のメンバー。フィールドが先で、無ければプロパティの getter を実行する。
    pub(super) fn member(&mut self, thread: u32, target: &ComPtr<ICorDebugValue>, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        // 配列の長さは、プロパティを呼ばずに、配列そのものから読む。
        if name == "Length" {
            if let Some(length) = unsafe { value::array_length(target) } {
                return self.new_int(thread, length as i32);
            }
        }
        if let Some(field) = unsafe { value::field(target, &self.modules, name) } {
            // フィールドの宣言の型（基底の型で宣言されたフィールドは見ない）。
            let owner = self.class_row(target);
            let kind = owner.and_then(|(module, row)| {
                let metadata = self.modules[module].metadata()?;
                let token = metadata.fields(row).into_iter().find(|f| !f.is_static && f.name == name)?.token;
                metadata.field_kind(token)
            });
            self.remember_object_typed(&field, kind);
            return Ok(field);
        }
        // 要素が 8 個以上のタプルの `Item8` 以降は、入れ子のタプル（`Rest`）の中にある（C# のコンパイラも同じに読む）。
        if let Some(number) = name.strip_prefix("Item").and_then(|number| number.parse::<usize>().ok()).filter(|number| *number >= 8) {
            if let Some(rest) = unsafe { value::field(target, &self.modules, "Rest") } {
                return self.member(thread, &rest, &format!("Item{}", number - 7));
            }
        }
        let type_name = unsafe { value::describe(target, &self.modules) }.type_name;
        match self.call_parameterless(thread, target, &format!("get_{name}")) {
            Some(Ok(Some(value))) => Ok(value),
            Some(Ok(None)) => Err(tr!("{type}.{name} returns no value.", type = type_name, name = name)),
            Some(Err(funceval::EvalFailure::Threw(exception))) => Ok(exception),
            Some(Err(failure)) => Err(tr!("Cannot read {type}.{name}: {reason}", type = type_name, name = name, reason = failure.describe(self))),
            None => Err(tr!("{type} has no member '{name}'.", type = type_name, name = name)),
        }
    }

    /// 型の静的メンバー（フィールド、またはプロパティ）。無ければ `Ok(None)`。
    pub(super) fn static_member(&mut self, thread: u32, depth: usize, module: usize, type_row: u32, name: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let Some(metadata) = self.modules[module].metadata() else { return Ok(None) };
        let field = metadata.fields(type_row).into_iter().find(|f| f.is_static && f.name == name).map(|f| (f.token, f.is_literal));
        let getter = metadata.static_property_getter(type_row, name);
        let handle = self.modules[module].module.clone();
        if let Some((token, is_literal)) = field {
            if is_literal {
                return self.constant_value(thread, module, type_row, token).map(Some).map_err(|e| tr!("Cannot read the constant {name}: {error}", name = name, error = e));
            }
            unsafe {
                let class = take::<ICorDebugClass>("ICorDebugModule::GetClassFromToken", |out| handle.GetClassFromToken(TYPE_DEF | type_row, out))?;
                // スレッドごとの静的フィールドのために、フレームを渡す。
                let frame = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().nth(depth));
                let mut raw = null_mut();
                let hr = class.GetStaticFieldValue(token, frame.as_ref().map_or(null_mut(), |f| f.as_raw()), &mut raw);
                let value = ComPtr::<ICorDebugValue>::from_raw(raw).ok_or_else(|| tr!("Cannot read the static field {name} ({code}). The type may not be initialized yet.", name = name, code = hr))?;
                // 宣言の型がプリミティブ型なのにボックスとして見える値は、中身の値にする（ボックスのままだと、object として扱ってしまう）。
                let declared = self.modules[module].metadata().and_then(|m| m.field_kind(token));
                let value = if matches!(declared, Some(ParameterKind::Primitive(_))) { self.unboxed(thread, value) } else { value };
                self.remember_object_typed(&value, declared);
                return Ok(Some(value));
            }
        }
        if let Some(getter) = getter {
            let function = unsafe { take::<ICorDebugFunction>("ICorDebugModule::GetFunctionFromToken", |out| handle.GetFunctionFromToken(getter, out)) }?;
            return match self.call(thread, &value::Callable { function, type_arguments: Vec::new() }, &[]) {
                Ok(value) => Ok(value),
                Err(funceval::EvalFailure::Threw(exception)) => Ok(Some(exception)),
                Err(failure) => Err(tr!("Cannot read {name}: {reason}", name = name, reason = failure.describe(self))),
            };
        }
        // 基底の型が宣言した静的メンバーも、派生した型の名前から読める（`Button.s_clickEvent` は `Control` のもの。
        // C# でも、見えるものは派生した型の名前で書ける。デバッガーなので、見えないものも読む。netcoredbg と同じ）。
        let base = self.modules[module].metadata().and_then(|m| m.base_type_name(type_row)).filter(|base| !matches!(base.as_str(), "System.Object" | "System.ValueType" | "System.Enum"));
        match base.and_then(|base| self.find_type_with_arity(thread, depth, &base, 0)) {
            Some((base_module, base_row)) if (base_module, base_row) != (module, type_row) => self.static_member(thread, depth, base_module, base_row, name),
            _ => Ok(None),
        }
    }

    /// 型引数つきの型の、表示の書き方（`System.Collections.Generic.IEnumerable<int>`）。
    pub(super) fn generic_display(&self, module: usize, row: u32, arguments: &[ComPtr<ICorDebugType>]) -> String {
        let arguments: Vec<String> = arguments.iter().map(|argument| unsafe { value::type_name(argument, &self.modules) }).collect();
        format!("{}<{}>", self.plain_type_name(module, row), arguments.join(", "))
    }

    /// 受け手の型が `receiver`（表示の書き方）の、C# 14 の拡張ブロックの静的メンバーの実装（名前が `name` の静的メソッド）を
    /// 持つ型。見える名前空間（`using`）のものだけ。
    pub(super) fn static_extension_owner(&self, thread: u32, depth: usize, receiver: &str, name: &str) -> Option<(usize, u32)> {
        let visible: Vec<String> = self.type_name_candidates(thread, depth, "?").into_iter().map(|candidate| candidate.trim_end_matches('?').trim_end_matches('.').to_string()).collect();
        self.modules.iter().enumerate().find_map(|(module, loaded)| {
            let metadata = loaded.metadata()?;
            metadata.static_extension_members(name).into_iter().find(|(owner, block)| calls::same_type(block, receiver) && visible.iter().any(|namespace| namespace == metadata.namespace_of(*owner))).map(|(owner, _)| (module, owner))
        })
    }

    /// C# 14 の拡張ブロックの静的プロパティ（`IEnumerable<int>.Answer`）の値。無ければ `Ok(None)`。
    pub(super) fn static_extension_property(&mut self, thread: u32, depth: usize, receiver: &str, name: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let getter = format!("get_{name}");
        let Some(owner) = self.static_extension_owner(thread, depth, receiver, &getter) else { return Ok(None) };
        self.call_method(thread, depth, Some(Current::Type { module: owner.0, row: owner.1 }), &getter, Vec::new())
    }

    /// ジェネリック型（型引数つき）の静的メンバー。フィールドは、型引数を付けた型から読む（型引数ごとに別の値）。
    /// 無ければ `Ok(None)`。
    pub(super) fn generic_static_member(&mut self, thread: u32, depth: usize, module: usize, row: u32, arguments: &[ComPtr<ICorDebugType>], name: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let Some(metadata) = self.modules[module].metadata() else { return Ok(None) };
        let field = metadata.fields(row).into_iter().find(|f| f.is_static && f.name == name).map(|f| (f.token, f.is_literal));
        let getter = metadata.static_property_getter(row, name);
        if let Some((token, is_literal)) = field {
            if is_literal {
                return self.constant_value(thread, module, row, token).map(Some);
            }
            let ty = self.parameterized(module, row, arguments)?;
            let frame = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().nth(depth));
            let mut raw = null_mut();
            let hr = unsafe { ty.GetStaticFieldValue(token, frame.as_ref().map_or(null_mut(), |f| f.as_raw()), &mut raw) };
            return unsafe { ComPtr::<ICorDebugValue>::from_raw(raw) }.map(Some).ok_or_else(|| tr!("Cannot read the static field {name} ({code}). The type may not be initialized yet.", name = name, code = hr));
        }
        if let Some(getter) = getter {
            let function = unsafe { take::<ICorDebugFunction>("ICorDebugModule::GetFunctionFromToken", |out| self.modules[module].module.GetFunctionFromToken(getter, out)) }?;
            return match self.call(thread, &value::Callable { function, type_arguments: arguments.to_vec() }, &[]) {
                Ok(value) => Ok(value),
                Err(funceval::EvalFailure::Threw(exception)) => Ok(Some(exception)),
                Err(failure) => Err(tr!("Cannot read {name}: {reason}", name = name, reason = failure.describe(self))),
            };
        }
        Ok(None)
    }

    /// C# 14 の拡張プロパティを読む。見つからなければ `None`。
    pub(super) fn extension_property(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, name: &str) -> Option<ComPtr<ICorDebugValue>> {
        let (callable, arguments) = self.bind_extension(thread, depth, receiver, &format!("get_{name}"), &[], &bind::CallExtra::default())?;
        match self.call(thread, &callable, &arguments) {
            Ok(value) => value,
            Err(funceval::EvalFailure::Threw(exception)) => Some(exception),
            Err(_) => None,
        }
    }

    /// C# 15 の共用体（`IUnion` を実装した値）なら、中の値（`Value`）。パターンは、共用体そのものではなく、中の値を見る。
    pub(super) fn union_case(&mut self, thread: u32, value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        let is_union = unsafe { value::type_names(value, &self.modules) }.iter().any(|name| name == "System.Runtime.CompilerServices.IUnion");
        if !is_union {
            return None;
        }
        self.member(thread, value, "Value").ok()
    }

    /// `式[添字, …]`。
    pub(super) fn eval_index(&mut self, thread: u32, depth: usize, target: &Expr, indices: &[Expr]) -> Result<Current, String> {
        Ok({
            let target = self.evaluate_node(thread, depth, target)?;
            let target = self.value_of(target)?;
            // ポインターの添字（`p[i]` は `*(p + i)`）。
            if let Some(pointer) = self.as_pointer(&target) {
                let [index] = indices else { return Err(tr!("A pointer takes only one index.")) };
                let index = self.evaluate_node(thread, depth, index)?;
                let index = self.value_of(index)?;
                return Ok(Current::Value(self.pointer_index(thread, &pointer, &index)?));
            }
            // `dynamic` の値の添字は、バインダで実行時に解決する。
            if self.is_dynamic(&target) {
                let indices = self.evaluate_arguments(thread, depth, indices)?;
                return Ok(Current::Value(self.dynamic_index(thread, depth, &target, indices)?));
            }
            if let Some(result) = self.index_special(thread, depth, &target, indices)? {
                return Ok(Current::Value(result));
            }
            let indices = self.evaluate_arguments(thread, depth, indices)?;
            Current::Value(self.index(thread, &target, &indices)?)
        })
    }

    /// 添字。配列はそのまま読む。それ以外（`List<T>`、`Dictionary<K, V>` など）は、インデクサー（`get_Item`）を実行する。
    pub(super) fn index(&mut self, thread: u32, target: &ComPtr<ICorDebugValue>, indices: &[ComPtr<ICorDebugValue>]) -> Result<ComPtr<ICorDebugValue>, String> {
        // 配列の添字は、対象の中の値ではなく数として要る。整数として読めた分だけを渡す。
        let numbers: Vec<u32> = indices.iter().filter_map(|index| unsafe { value::describe(index, &self.modules) }.value.parse().ok()).collect();
        if numbers.len() == indices.len() {
            if let Some(element) = unsafe { value::array_element(target, &numbers) }? {
                return Ok(element);
            }
        }
        let type_name = unsafe { value::describe(target, &self.modules) }.type_name;
        // インデクサーの getter は、ふつう get_Item。文字列は get_Chars。
        let mut candidates = unsafe { value::instance_method_candidates(target, &self.modules, "get_Item", indices.len() as u32) };
        if candidates.is_empty() {
            candidates = unsafe { value::instance_method_candidates(target, &self.modules, "get_Chars", indices.len() as u32) };
        }
        if candidates.is_empty() {
            return Err(tr!("{type} cannot be indexed.", type = type_name));
        }
        let mut indices = indices.to_vec();
        let indexer = self.choose_overload(thread, candidates, &mut indices).ok_or_else(|| tr!("A value of this type cannot be used as an index of {type}.", type = type_name))?;
        let mut arguments = vec![target.clone()];
        arguments.extend(indices);
        match self.call(thread, &indexer, &arguments) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(tr!("The indexer of {type} returns no value.", type = type_name)),
            Err(funceval::EvalFailure::Threw(exception)) => Ok(exception),
            Err(failure) => Err(tr!("Cannot read the indexer of {type}: {reason}", type = type_name, reason = failure.describe(self))),
        }
    }

    /// 変数のタプルの要素名（`pair.y`）を、`Item2` のような本当のフィールドの名前にする。
    pub(super) fn tuple_item_name(&self, thread: u32, depth: usize, variable: &str, element: &str) -> Option<String> {
        let frames = self.walk_frames(thread).ok()?;
        let frame = frames.get(depth)?;
        unsafe {
            let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
            let mut token = 0u32;
            frame.GetFunctionToken(&mut token);
            let (mut offset, mut mapping) = (0u32, 0);
            il_frame.GetIP(&mut offset, &mut mapping);
            let pdb = self.frame_module(frame)?.pdb.as_ref()?;
            let local = pdb.locals(token, offset, frame_version(frame)).into_iter().find(|local| local.name == variable)?;
            local.tuple_names.iter().position(|name| name == element).map(|index| format!("Item{}", index + 1))
        }
    }

    pub(super) fn type_or_namespace(&self, thread: u32, depth: usize, name: String) -> Current {
        match self.find_type(thread, depth, &name) {
            Some((module, row)) => Current::Type { module, row },
            None => Current::Namespace(name),
        }
    }

    /// 名前に合う型を、読み込まれているモジュールから探す。いまのメソッドの名前空間からの相対の名前も試す。
    /// シンボルのあるモジュール（利用者のコード）を先に見る。
    pub(super) fn find_type(&self, thread: u32, depth: usize, name: &str) -> Option<(usize, u32)> {
        // 同じ名前を、式 1 つの中で何度も探す（ラムダ式の組み立てでは、`Expression` の型を節ごとに探す）。
        let key = (thread, depth, name.to_string());
        if let Some(found) = self.eval_cache.types.borrow().get(&key) {
            return *found;
        }
        let candidates = self.type_name_candidates(thread, depth, name);
        let search = |with_symbols: bool| {
            self.modules.iter().enumerate().filter(|(_, m)| m.pdb.is_some() == with_symbols).find_map(|(index, module)| {
                let metadata = module.metadata()?;
                candidates.iter().find_map(|candidate| metadata.find_type(candidate)).map(|row| (index, row))
            })
        };
        let found = search(true).or_else(|| search(false));
        self.eval_cache.types.borrow_mut().insert(key, found);
        found
    }

    /// 型の名前として試す、完全な名前の候補。書かれたとおりの名前、いまのメソッドの名前空間からの相対の名前、
    /// そのメソッドのコードから見える `using` の名前空間（PDB に記録されている）を付けた名前の順。
    pub(super) fn type_name_candidates(&self, thread: u32, depth: usize, name: &str) -> Vec<String> {
        let mut candidates = vec![name.to_string()];
        if let Some((module, row)) = self.frame_type(thread, depth) {
            let namespace = self.modules[module].metadata().map_or("", |m| m.namespace_of(row));
            if !namespace.is_empty() {
                candidates.push(format!("{namespace}.{name}"));
            }
        }
        let usings = self.walk_frames(thread).ok().and_then(|frames| {
            let frame = frames.get(depth)?;
            let mut token = 0u32;
            unsafe { frame.GetFunctionToken(&mut token) };
            Some(unsafe { self.frame_module(frame) }?.pdb.as_ref()?.namespaces(token, frame_version(frame)).to_vec())
        });
        // C# 以外の言語（F#）のコードと、PDB に `using` の記録が無いメソッドでは、C# の暗黙の using（SDK の
        // ImplicitUsings）も見えるものとする。F# の `open` には System.Linq がまず無いが、C# の式で F# の値に
        // `xs.Sum()` などと書けるように（docs/DECISIONS.md D-037）。
        let foreign = self.frame_type(thread, depth).and_then(|(module, _)| self.modules[module].metadata()?.assembly_reference("FSharp.Core")).is_some();
        let mut usings = usings.unwrap_or_default();
        if foreign || usings.is_empty() {
            usings.extend(IMPLICIT_USINGS.iter().map(|namespace| namespace.to_string()));
        }
        for namespace in usings {
            let candidate = format!("{namespace}.{name}");
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        candidates
    }

    pub(super) fn type_name(&self, module: usize, row: u32) -> String {
        self.modules.get(module).and_then(|m| m.metadata()).map_or_else(|| "?".into(), |m| m.type_name(row))
    }

    pub(super) fn plain_type_name(&self, module: usize, row: u32) -> String {
        self.modules.get(module).and_then(|m| m.metadata()).map_or_else(|| "?".into(), |m| m.plain_name(row))
    }

    /// フレームのメソッドがあるモジュール（添字）と、そのメソッドが属する型。
    pub(super) fn frame_type(&self, thread: u32, depth: usize) -> Option<(usize, u32)> {
        let frames = self.walk_frames(thread).ok()?;
        let frame = frames.get(depth)?;
        let module = unsafe { self.frame_module(frame) }?;
        let index = self.modules.iter().position(|m| std::ptr::eq(m, module))?;
        let mut token = 0u32;
        unsafe { frame.GetFunctionToken(&mut token) };
        Some((index, module.metadata()?.declaring_type(token & 0x00FF_FFFF)?))
    }

    /// いまのメソッドの型と、それを囲む型（内側から順）。入れ子の型のコードからは、囲む型の静的メンバーを、型の名前を
    /// 付けずに使える（C# の入れ子のクラス、F# の入れ子のモジュール、ラムダのクロージャ）。
    pub(super) fn frame_types(&self, thread: u32, depth: usize) -> Vec<(usize, u32)> {
        let Some((module, mut row)) = self.frame_type(thread, depth) else { return Vec::new() };
        let mut types = vec![(module, row)];
        let Some(metadata) = self.modules[module].metadata() else { return types };
        for _ in 0..16 {
            match metadata.enclosing_type(row) {
                Some(outer) => {
                    row = outer;
                    types.push((module, row));
                }
                None => break,
            }
        }
        types
    }
}
