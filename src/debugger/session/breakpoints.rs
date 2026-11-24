//! 行ブレークポイントと関数ブレークポイント。置く、外す、条件を確かめる。

use super::*;

impl Session {
    /// いま読み込まれたモジュールに入口のメソッドがあれば、その先頭にブレークポイントを置く（`stopAtEntry`）。
    pub(super) fn break_at_entry(&mut self) {
        let Some(module) = self.modules.last() else { return };
        // 入口を持つのはアプリ本体のアセンブリだけ。ほかのモジュール（ライブラリ）では何もしない。
        // ディスクに無いモジュール（単一ファイルのアプリ）は、対象のメモリから読む。
        let image = std::fs::read(&module.path).ok().or_else(|| unsafe { module_image(&module.module) });
        let Some(token) = image.and_then(|file| target::entry_point(&file)) else { return };
        // `async Main` の入口は、コンパイラが作った `<Main>`（中で `Main(args).GetAwaiter().GetResult()`）。利用者の
        // `Main` の中身は状態機械の MoveNext にあるので、その最初の行（`{`）で止める（Visual Studio と同じ）。
        let (token, offset) = Self::async_main_start(module, token).unwrap_or((token, 0));
        let handle = module.module.clone();
        self.seeking_entry = false;
        match unsafe { break_at(&handle, token, offset) } {
            Ok(created) => self.entry_breakpoint = Some(created),
            Err(e) => crate::log!("stopAtEntry: {e}"),
        }
    }

    /// 入口がコンパイラの作った `<Main>`（`async Main` のとき）なら、利用者の `Main` の本体（MoveNext）と、その最初の
    /// 見える行の IL オフセット。
    fn async_main_start(module: &Module, entry: u32) -> Option<(u32, u32)> {
        let metadata = module.metadata()?;
        let row = entry & 0x00FF_FFFF;
        if metadata.method_simple_name(row)? != "<Main>" {
            return None;
        }
        let ty = metadata.declaring_type(row)?;
        let kickoff = metadata.method_rows_named(ty, "Main").into_iter().find(|&candidate| candidate != row)?;
        module.pdb.as_ref()?.state_machine_start(kickoff)
    }

    /// `setBreakpoints`: そのファイルの、`owner` が置いたブレークポイントを、送られてきた一覧で置き換える。
    pub fn set_breakpoints(&mut self, owner: Owner, arguments: &Value) -> Result<Value, String> {
        let source = arguments.pointer("/source/path").and_then(Value::as_str).ok_or_else(|| tr!("setBreakpoints has no source.path."))?.to_string();
        let items: Vec<&Value> = match arguments.get("breakpoints") {
            Some(Value::Array(items)) => items.iter().filter(|b| b.get("line").and_then(Value::as_u64).is_some()).collect(),
            _ => Vec::new(),
        };
        let mut fresh = Vec::with_capacity(items.len());
        for item in items {
            let line = item.get("line").and_then(Value::as_u64).unwrap_or(0) as u32;
            // 列は 1 始まり。0 と欠ける指定は、行の最初の文（今までの結び方）。
            let column = item.get("column").and_then(Value::as_u64).filter(|&c| c > 0).map(|c| c as u32);
            // 条件の欄の印（`#once` など）は、C# の式としては評価しない（hits.rs）。
            let condition = condition_of(item);
            let marker = condition.as_deref().and_then(hits::Marker::parse);
            let hit_text = item.get("hitCondition").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
            let hit_condition = hit_text.and_then(hits::HitCondition::parse);
            if let (Some(text), None) = (hit_text, hit_condition) {
                self.deferred.push(("output", json!({ "category": "console", "output": format!("Ignored hit count condition \"{text}\": use N, ==N, >=N, >N or %N.\n") })));
            }
            let log_message = item.get("logMessage").and_then(Value::as_str).map(str::to_string);
            fresh.push(Breakpoint { id: self.next_breakpoint_id, line, column, condition: if marker.is_some() { None } else { condition }, hit_condition, marker, log_message, verified_line: None, verified_column: None, verified_end_column: None, bound: Vec::new() });
            self.next_breakpoint_id += 1;
        }

        // 逆コンパイルしたソースのファイルなら、そのモジュールを先に逆コンパイルする（前のセッションのファイルを開いて置いたとき。D-060）。
        self.decompile_for_source(&source);
        let key = (owner, source);
        let old = self.breakpoints.remove(&key).unwrap_or_default();
        let modules = &self.modules;
        self.while_synchronized(|| {
            for breakpoint in &old {
                for bound in &breakpoint.bound {
                    unsafe { bound.Activate(0) };
                }
            }
            for breakpoint in fresh.iter_mut() {
                for module in modules {
                    bind(module, &key.1, breakpoint);
                }
            }
        });
        let body = json!({ "breakpoints": fresh.iter().map(describe).collect::<Vec<_>>() });
        self.breakpoints.insert(key, fresh);
        Ok(body)
    }

    /// `setFunctionBreakpoints`: `owner` が置いた関数ブレークポイントを、送られてきた一覧で置き換える。
    pub fn set_function_breakpoints(&mut self, owner: Owner, arguments: &Value) -> Result<Value, String> {
        let mut fresh = Vec::new();
        if let Some(Value::Array(items)) = arguments.get("breakpoints") {
            for item in items {
                // 引数の並び（`Method(int)`）は見ない。オーバーロードはすべて対象にする。
                let name = item.get("name").and_then(Value::as_str).unwrap_or("").split('(').next().unwrap_or("").trim().to_string();
                let any_code = item.get("anyCode").and_then(Value::as_bool).unwrap_or(false);
                fresh.push(FunctionBreakpoint { id: self.next_breakpoint_id, owner, name, any_code, condition: condition_of(item), bound: Vec::new() });
                self.next_breakpoint_id += 1;
            }
        }
        let (old, kept): (Vec<FunctionBreakpoint>, Vec<FunctionBreakpoint>) = std::mem::take(&mut self.function_breakpoints).into_iter().partition(|b| b.owner == owner);
        self.function_breakpoints = kept;
        let modules = &self.modules;
        self.while_synchronized(|| {
            for bound in old.iter().flat_map(|b| &b.bound) {
                unsafe { bound.Activate(0) };
            }
            for breakpoint in fresh.iter_mut().filter(|b| !b.name.is_empty()) {
                for module in modules {
                    bind_function(module, breakpoint);
                }
            }
        });
        let body = json!({ "breakpoints": fresh.iter().map(describe_function).collect::<Vec<_>>() });
        self.function_breakpoints.extend(fresh);
        Ok(body)
    }

    /// ブレークポイントの条件が成り立つか。スレッドのいちばん上のフレームの文脈で評価する。
    pub(super) fn condition_holds(&mut self, thread: u32, condition: &str) -> Result<bool, String> {
        match self.evaluate_expression(thread, 0, condition)? {
            eval::Evaluated::Value(value) | eval::Evaluated::Nullable(value) => {
                let described = unsafe { value::describe(&value, &self.modules) };
                match (described.type_name.as_str(), described.value.as_str()) {
                    ("bool", "true") => Ok(true),
                    ("bool", "false") => Ok(false),
                    (type_name, shown) => Err(tr!("The condition must be a bool expression (the result is {value} of type {type}).", type = type_name, value = shown)),
                }
            }
            eval::Evaluated::Void => Err(tr!("The condition must be a bool expression (it is a call that returns no value).")),
            eval::Evaluated::Pointer { type_name, .. } => Err(tr!("The condition must be a bool expression (the result is a pointer of type {type}).", type = type_name)),
        }
    }
}

pub(super) fn describe(breakpoint: &Breakpoint) -> Value {
    let mut described = json!({
        "id": breakpoint.id,
        "verified": breakpoint.verified_line.is_some(),
        "line": breakpoint.verified_line.unwrap_or(breakpoint.line),
    });
    if let (Some(column), Some(end)) = (breakpoint.verified_column, breakpoint.verified_end_column) {
        described["column"] = json!(column);
        described["endColumn"] = json!(end);
    }
    described
}

/// ブレークポイントの指定に付いた条件。空なら、条件なし。
pub(super) fn condition_of(breakpoint: &Value) -> Option<String> {
    breakpoint.get("condition").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty()).map(str::to_string)
}

pub(super) fn describe_function(breakpoint: &FunctionBreakpoint) -> Value {
    // 行は無いが、netcoredbg と同じく 0 を入れておく。
    json!({ "id": breakpoint.id, "verified": !breakpoint.bound.is_empty(), "line": 0 })
}

/// メソッドの IL の `il_offset` にブレークポイントを置いて、有効にする。
pub(super) unsafe fn break_at(module: &ComPtr<ICorDebugModule>, method: u32, il_offset: u32) -> Result<ComPtr<ICorDebugFunctionBreakpoint>, String> {
    let function = take::<ICorDebugFunction>("GetFunctionFromToken", |out| module.GetFunctionFromToken(method, out))?;
    let code = take::<ICorDebugCode>("GetILCode", |out| function.GetILCode(out))?;
    let created = take::<ICorDebugFunctionBreakpoint>("CreateBreakpoint", |out| code.CreateBreakpoint(il_offset, out))?;
    created.Activate(1).ok().map_err(|hr| failed("Activate", hr))?;
    Ok(created)
}

/// 関数ブレークポイントを、モジュールの中の名前に合うメソッドすべてに置く。
pub(super) fn bind_function(module: &Module, breakpoint: &mut FunctionBreakpoint) {
    if !breakpoint.any_code && module.pdb.is_none() {
        return;
    }
    let Some(metadata) = module.metadata() else { return };
    for method in metadata.find_methods(&breakpoint.name) {
        match unsafe { break_at(&module.module, method, 0) } {
            Ok(created) => breakpoint.bound.push(created),
            // 本体の無いメソッド（abstract、extern）には置けない。
            Err(e) => crate::log!("function breakpoint {} at 0x{method:08X} in {}: {e}", breakpoint.name, module.path),
        }
    }
}

/// ブレークポイントを、モジュールの該当する場所に置く。そのモジュールにソースが無ければ何もしない。
pub(super) fn bind(module: &Module, source: &str, breakpoint: &mut Breakpoint) {
    let Some(pdb) = module.pdb.as_ref() else { return };
    let Some(document) = pdb.find_document(source) else { return };
    // 列があるときはその文。無ければ、その行の最初の文。
    let Some(location) = (match breakpoint.column {
        Some(column) => pdb.resolve_column(document, breakpoint.line, column),
        None => pdb.resolve_line(document, breakpoint.line),
    }) else {
        return;
    };
    match unsafe { break_at(&module.module, location.method, location.il_offset) } {
        Ok(created) => {
            breakpoint.bound.push(created);
            breakpoint.verified_line.get_or_insert(location.line);
            breakpoint.verified_column.get_or_insert(location.column);
            breakpoint.verified_end_column.get_or_insert(location.end_column);
        }
        Err(e) => crate::log!("breakpoint {} at {source}:{}: {e}", breakpoint.id, breakpoint.line),
    }
}
