//! 書き込みの見張り（D-093）: 選んだオブジェクトのフィールドが書き換えられたら止まる。書いたのがどこ（`ref` や `out`、
//! 別のオブジェクト経由、ソースの無い DLL）でも止まる。ランタイムのデータ ブレークポイントを使う。
//!
//! - フィールドの番地を、ハードウェアのデバッグ レジスター（Dr0 と Dr7）に、すべてのスレッドで置く
//!   （`ICorDebugProcess::SetThreadContext`。あとからできたスレッドにも置く）。書き込むと、ランタイムが
//!   `ICorDebugManagedCallback4::DataBreakpoint` で知らせる。
//! - GC はオブジェクトを動かすので、見張る間はオブジェクトを固定する（`HANDLE_PINNED`）。固定できないときだけ、GC の前に
//!   外し、GC のあとに新しい番地から置き直す（`ICorDebugProcess10::EnableGCNotificationEvents`。.NET 10 は答えない）。
//! - .NET（Core 3.0 以降）だけ。.NET Framework のランタイムはデータ ブレークポイントを知らせないので、理由を付けて断る。
//! - 見張れるのは 1 つ（Dr0）。
//! - x86 のランタイムは、ネイティブのコード（リフレクションの最初の `SetValue`、P/Invoke の書き戻し）の中で当たった見張りを
//!   処理できず、対象が 0xC0000409 で落ちる。x64 は呼んだマネージドのフレームで止まる。選ぶときの説明で知らせる。
//!
//! 口は DAP の標準の `dataBreakpointInfo`（`variablesReference` と `name`、または `expression` と `frameId`）と
//! `setDataBreakpoints`。

use super::*;

/// 見張っているフィールド。
pub(super) struct DataWatch {
    /// `dataBreakpointInfo` で渡した `dataId`（VS Code は、ほかを足すたびに全部を送り直す）。
    id: String,
    /// 見せる名前（`order.Status`）。
    label: String,
    /// 持ち主のオブジェクト（固定のハンドル。固定できなければ強いハンドルで、GC で動いたあとの番地を読む）。
    owner: ComPtr<ICorDebugHandleValue>,
    /// オブジェクトの先頭からのフィールドの位置と、大きさ。
    offset: u64,
    size: usize,
}

impl Session {
    /// `dataBreakpointInfo`: 見張れるフィールドなら、その `dataId`。
    pub fn data_breakpoint_info(&mut self, arguments: &Value) -> Result<Value, String> {
        match self.resolve_field(arguments) {
            Ok((id, label, ..)) => {
                let mut description = tr!("When {field} is written", field = label);
                if crate::win32::NATIVE_WRITE_ENDS_TARGET {
                    // x86 のランタイムの制限（D-093）。黙って落ちないよう、選ぶときに知らせる。
                    description = format!("{description} {}", tr!("(32-bit: if native code writes it, such as the first reflection SetValue or P/Invoke writing back, the program ends.)"));
                }
                Ok(json!({ "dataId": id, "description": description, "accessTypes": ["write"], "canPersist": false }))
            }
            Err(reason) => Ok(json!({ "dataId": Value::Null, "description": reason })),
        }
    }

    /// `setDataBreakpoints`: 見張りを置き直す（空なら外す）。同じ `dataId` は置いたままにする。
    pub fn set_data_breakpoints(&mut self, arguments: &Value) -> Result<Value, String> {
        let wanted: Vec<String> = arguments.get("breakpoints").and_then(Value::as_array).into_iter().flatten().filter_map(|b| b.get("dataId").and_then(Value::as_str)).map(str::to_string).collect();
        let mut answers = Vec::new();
        let keep = self.data_watch.as_ref().is_some_and(|w| wanted.first() == Some(&w.id));
        if !keep {
            self.clear_data_watch();
        }
        for (index, id) in wanted.iter().enumerate() {
            if index > 0 {
                answers.push(json!({ "verified": false, "message": tr!("Only one field can be watched for writes at a time.") }));
                continue;
            }
            if keep {
                answers.push(json!({ "verified": true }));
                continue;
            }
            match self.watch_field(id) {
                Ok(()) => answers.push(json!({ "verified": true })),
                Err(e) => answers.push(json!({ "verified": false, "message": e })),
            }
        }
        Ok(json!({ "breakpoints": answers }))
    }

    /// `dataId` の形: `<variablesReference>:<name>` か `expr:<frameId>:<expression>`。フィールドを引き、`(dataId, 見せる名前,
    /// 持ち主のオブジェクト, フィールドの値)`。
    fn resolve_field(&mut self, arguments: &Value) -> Result<(String, String, ComPtr<ICorDebugValue>, ComPtr<ICorDebugValue>), String> {
        if !self.supports_data_breakpoints() {
            return Err(tr!("Breaking on writes needs .NET (Core 3.0 or later); the .NET Framework runtime does not report data breakpoints."));
        }
        if self.state != State::Stopped {
            return Err(tr!("A field can be chosen only while the target is stopped."));
        }
        let (id, label, owner, field) = if let Some(expression) = arguments.get("expression").and_then(Value::as_str).map(str::trim).filter(|e| !e.is_empty()) {
            let frame = arguments.get("frameId").and_then(Value::as_i64).unwrap_or(0);
            let (owner_text, field) = expression.rsplit_once('.').ok_or_else(|| tr!("Write the field as object.Field (for example order.Status)."))?;
            let (thread, depth) = self.frame_of(&json!({ "frameId": frame }))?;
            let owner = match self.evaluate_expression(thread, depth, owner_text)? {
                eval::Evaluated::Value(value) | eval::Evaluated::Nullable(value) => value,
                _ => return Err(tr!("{owner} is not an object.", owner = owner_text)),
            };
            (format!("expr:{frame}:{expression}"), expression.to_string(), owner, field.to_string())
        } else {
            let reference = arguments.get("variablesReference").and_then(Value::as_u64).ok_or_else(|| tr!("Give variablesReference and name, or expression."))? as usize;
            let name = arguments.get("name").and_then(Value::as_str).ok_or_else(|| tr!("Give variablesReference and name, or expression."))?;
            let handle = reference.checked_sub(1).and_then(|index| self.handles.get(index)).cloned().ok_or_else(|| tr!("That variablesReference is not from the current stop."))?;
            let (Handle::Children { value, .. } | Handle::Raw { value, .. }) = handle else { return Err(tr!("Only a field of an object can be watched for writes.")) };
            (format!("{reference}:{name}"), name.to_string(), value, name.to_string())
        };
        let value = unsafe { value::field(&owner, &self.modules, &field) }.ok_or_else(|| tr!("{field} is not a field of the object (properties cannot be watched; watch their backing field).", field = field))?;
        Ok((id, label, owner, value))
    }

    /// `dataId` のフィールドを見張り始める（いまの停止で `dataBreakpointInfo` を引いたもの）。
    fn watch_field(&mut self, id: &str) -> Result<(), String> {
        let arguments = match id.strip_prefix("expr:").and_then(|rest| rest.split_once(':')) {
            Some((frame, expression)) => json!({ "frameId": frame.parse::<i64>().unwrap_or(0), "expression": expression }),
            None => {
                let (reference, name) = id.split_once(':').ok_or_else(|| tr!("This data breakpoint is from an earlier stop. Choose the field again."))?;
                json!({ "variablesReference": reference.parse::<u64>().unwrap_or(0), "name": name })
            }
        };
        let (id, label, owner, field) = self.resolve_field(&arguments)?;
        unsafe {
            let object = match owner.cast::<ICorDebugReferenceValue>() {
                Ok(reference) => take::<ICorDebugValue>("ICorDebugReferenceValue::Dereference", |out| reference.Dereference(out))?,
                Err(_) => owner.clone(),
            };
            let (object_address, mut field_address, mut size) = (inspect::object_address(&object), 0u64, 0u32);
            field.GetAddress(&mut field_address);
            field.GetSize(&mut size);
            if object_address == 0 || field_address < object_address {
                return Err(tr!("Only a field of an object on the heap can be watched (not a local variable or a struct value)."));
            }
            let heap: ComPtr<ICorDebugHeapValue2> = object.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugHeapValue2"), hr))?;
            // オブジェクトを固定する（GC で動かなければ、番地を置き直さなくてよい）。.NET 10 のランタイムは
            // ICorDebugProcess10（GC の通知）を答えないので、固定できないときだけ、強いハンドルと GC の通知に頼る。
            let handle = match take::<ICorDebugHandleValue>("ICorDebugHeapValue2::CreateHandle(PINNED)", |out| heap.CreateHandle(HANDLE_PINNED, out)) {
                Ok(handle) => handle,
                Err(_) => {
                    let process10 = self.process.as_ref().and_then(|p| p.cast::<ICorDebugProcess10>().ok()).ok_or_else(|| tr!("The object cannot be pinned, and the runtime does not report garbage collections, so its field cannot be followed."))?;
                    process10.EnableGCNotificationEvents(1);
                    take::<ICorDebugHandleValue>("ICorDebugHeapValue2::CreateHandle", |out| heap.CreateHandle(HANDLE_STRONG, out))?
                }
            };
            let size = (size as usize).min(std::mem::size_of::<usize>());
            let size = [8, 4, 2, 1].into_iter().find(|&s| s <= size).unwrap_or(1);
            self.data_watch = Some(DataWatch { id, label, owner: handle, offset: field_address - object_address, size });
        }

        self.apply_data_watch();
        Ok(())
    }

    /// ランタイムがデータ ブレークポイントを知らせるか（.NET。.NET Framework は知らせない）。
    fn supports_data_breakpoints(&self) -> bool {
        // dbgshim を読み込むのは .NET の対象だけ（launch.rs）。
        self.dbgshim.is_some() && self.process.is_some()
    }

    /// 見張りを外し、オブジェクトの固定も解く。切り離す前にも呼ぶ（デバッグ レジスターが残ると、デバッガーの無い対象が
    /// 次の書き込みで落ちる）。
    pub(super) fn clear_data_watch(&mut self) {
        if let Some(watch) = self.data_watch.take() {
            self.set_watch_on_threads(None, None);
            unsafe { watch.owner.Dispose() };
        }
    }

    /// 見張る番地と大きさ（オブジェクトのいまの番地から求める）。
    fn data_watch_place(&self) -> Option<(usize, usize)> {
        let w = self.data_watch.as_ref()?;
        let reference: ComPtr<ICorDebugReferenceValue> = w.owner.cast().ok()?;
        let object = unsafe { take::<ICorDebugValue>("ICorDebugReferenceValue::Dereference", |out| reference.Dereference(out)).ok()? };
        let address = unsafe { inspect::object_address(&object) };
        (address != 0).then_some(((address + w.offset) as usize, w.size))
    }

    /// 見張りを、すべてのスレッドに置く。
    fn apply_data_watch(&mut self) {
        let place = self.data_watch_place();
        self.set_watch_on_threads(place, None);
    }

    /// スレッド（`only` が無ければ、すべて）のデバッグ レジスターに、見張りを置くか外す。
    fn set_watch_on_threads(&self, watch: Option<(usize, usize)>, only: Option<u32>) {
        let Some(process) = self.process.as_ref() else { return };
        let threads: Vec<u32> = only.map_or_else(|| self.threads.clone(), |t| vec![t]);
        for thread in threads {
            let mut context = crate::win32::debug_context();
            unsafe {
                if !process.GetThreadContext(thread, context.len() as u32, context.as_mut_ptr()).is_ok() {
                    continue;
                }
                crate::win32::set_write_watch(&mut context, watch);
                let set = process.SetThreadContext(thread, context.len() as u32, context.as_mut_ptr());
                if !set.is_ok() {
                    crate::log!("data breakpoint: SetThreadContext({thread}) failed: {:#x}", set.0);
                }
            }
        }
    }

    /// あとからできたスレッドにも置く。
    pub(super) fn data_watch_new_thread(&mut self, thread: u32) {
        if let Some(place) = self.data_watch_place() {
            self.set_watch_on_threads(Some(place), Some(thread));
        }
    }
    /// GC の前: 外す（GC がオブジェクトを動かす書き込みで止まらないように）。
    pub(super) fn data_watch_before_gc(&mut self) {
        if self.data_watch.is_some() {
            self.set_watch_on_threads(None, None);
        }
    }

    /// GC のあと: 動いたオブジェクトの新しい番地に置き直す。
    pub(super) fn data_watch_after_gc(&mut self) {
        if self.data_watch.is_some() {
            self.apply_data_watch();
        }
    }

    /// 見張りに当たった: 止まる。
    pub(super) fn data_watch_hit(&mut self, thread: u32) -> Event {
        let label = self.data_watch.as_ref().map(|w| w.label.clone()).unwrap_or_default();
        let text = tr!("{field} was written", field = label);
        ("stopped", json!({ "reason": "data breakpoint", "threadId": thread, "description": text, "text": text, "allThreadsStopped": true }))
    }
}
