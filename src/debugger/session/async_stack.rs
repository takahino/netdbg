//! 非同期の論理コールスタック（段階 12）: `await` のあとで再開した async メソッドの本体（状態機械の MoveNext）は、
//! 物理的なスタックの下にスレッド プールなどの仕組みしかない。誰が `await` していたかを、状態機械からたどって戻す
//! （Visual Studio の「async のコール スタック」と同じもの）。
//!
//! runtime-async（状態機械を作らない async。D-106）は、状態機械のたどりを使わない。再開したフレームの直下の
//! ランタイムのフレームから続きの番地を取り、`ICorDebugProcess12::GetAsyncStack` で続きの鎖を足す。取れなければ足さない。
//!
//! たどり方: 状態機械の `<>t__builder` の Task（.NET Framework は `m_builder.m_task`）の `m_continuationObject` が、
//! `await` している側の続き。.NET では `AsyncStateMachineBox`（`StateMachine` に呼び出し元の状態機械）、.NET Framework
//! では `Action` の `_target` の `MoveNextRunner`（`m_stateMachine`）。間に、同期コンテキストの続き（`m_action`）、
//! 包み（`m_continuation`）、続きが複数のときの `List<object>` が入ることがある。呼び出し元の状態機械の `<>1__state`
//! が、どの `await` で待っているか（PDB の async の情報の何番目か）で、その行が分かる。

use super::inspect::object_address;
use super::*;
use crate::debugger::pdb::LATEST;

/// たどる深さ（呼び出し元の数）の上限。
const MAX_CALLERS: usize = 64;
/// 続きの包みをほどく深さの上限。
const MAX_UNWRAP: usize = 8;
/// 論理的なフレームの番号の頭（ふつうのフレームの番号と重ならない）。`scopes` は空で答える。
pub(super) const LOGICAL_FRAME_BASE: i64 = 1 << 40;

impl Session {
    /// 物理的なスタックに、論理的な呼び出し元を挟む。いちばん上の async の本体（続きの中のもの）の直後に、区切りと
    /// `await` している呼び出し元（新しい順）。挟むものが無ければ、そのまま。
    pub(super) fn with_async_callers(&self, thread: u32, frames: &[ComPtr<ICorDebugFrame>], described: Vec<Value>) -> Vec<Value> {
        let found = frames.iter().enumerate().take(described.len()).find_map(|(depth, frame)| {
            let this = unsafe { frame_this(frame) }?;
            // 型の名前だけを読む（describe は DebuggerDisplay などで対象のコードを動かすことがある。スタックのたびには重い）。
            let type_name = unsafe { value::runtime_type_name(&this, &self.modules) }?;
            is_machine(&type_name).then_some((depth, this))
        });
        let Some((depth, machine)) = found else { return self.with_runtime_async_callers(thread, frames, described) };
        let callers = self.awaiting_callers(&machine);
        if callers.is_empty() {
            return described;
        }
        let mut out = described;
        let mut logical = vec![json!({ "id": LOGICAL_FRAME_BASE, "name": "[Awaited by]", "line": 0, "column": 0, "presentationHint": "label" })];
        for (index, mut caller) in callers.into_iter().enumerate() {
            caller["id"] = json!(LOGICAL_FRAME_BASE + 1 + index as i64);
            logical.push(caller);
        }
        let at = (depth + 1).min(out.len());
        out.splice(at..at, logical);
        out
    }

    /// 状態機械から、`await` している呼び出し元を順に（近い方から）。
    fn awaiting_callers(&self, machine: &ComPtr<ICorDebugValue>) -> Vec<Value> {
        let mut callers = Vec::new();
        let mut current = machine.clone();
        while callers.len() < MAX_CALLERS {
            let Some(task) = self.task_of(&current) else { break };
            let Some(continuation) = (unsafe { value::field(&task, &self.modules, "m_continuationObject") }).filter(|v| unsafe { object_address(v) } != 0) else { break };
            let Some(next) = self.state_machine_of(&continuation, 0) else { break };
            callers.push(self.describe_state_machine(&next));
            current = next;
        }
        callers
    }

    /// 状態機械の Task（無ければ `None`。async void や、まだ一度も中断していないもの）。
    fn task_of(&self, machine: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
        let builder = unsafe { value::field(machine, &self.modules, "<>t__builder") }?;
        let task = unsafe { value::field(&builder, &self.modules, "m_task") }.or_else(|| {
            let inner = unsafe { value::field(&builder, &self.modules, "m_builder") }?;
            unsafe { value::field(&inner, &self.modules, "m_task") }
        })?;
        (unsafe { object_address(&task) } != 0).then_some(task)
    }

    /// 続きのオブジェクトから、`await` している側の状態機械。
    fn state_machine_of(&self, continuation: &ComPtr<ICorDebugValue>, depth: usize) -> Option<ComPtr<ICorDebugValue>> {
        if depth > MAX_UNWRAP || unsafe { object_address(continuation) } == 0 {
            return None;
        }
        let type_name = unsafe { value::runtime_type_name(continuation, &self.modules) }?;
        let field = |name: &str| unsafe { value::field(continuation, &self.modules, name) };
        // 箱を先に見る（`AsyncStateMachineBox<Program.<Outer>d__1>` は、型引数の中に状態機械の名前がある）。
        if type_name.contains("AsyncStateMachineBox") {
            return field("StateMachine");
        }
        if is_machine(&type_name) {
            return Some(continuation.clone());
        }
        if type_name.contains("MoveNextRunner") {
            return field("m_stateMachine");
        }
        // 包み: デリゲート（`_target`）、同期コンテキストなどの続き（`m_action`）、.NET Framework の包み（`m_continuation`）。
        for wrapper in ["_target", "m_action", "m_continuation"] {
            if let Some(inner) = field(wrapper).filter(|v| unsafe { object_address(v) } != 0) {
                if let Some(found) = self.state_machine_of(&inner, depth + 1) {
                    return Some(found);
                }
            }
        }
        // 続きが複数（`List<object>`）: 最初のもの。
        if type_name.starts_with("System.Collections.Generic.List<") {
            let items = field("_items")?;
            let first = unsafe { value::array_element(&items, &[0]) }.ok().flatten()?;
            return self.state_machine_of(&first, depth + 1);
        }
        None
    }

    /// 呼び出し元の状態機械の、メソッドの名前と、`await` している行。
    fn describe_state_machine(&self, machine: &ComPtr<ICorDebugValue>) -> Value {
        let type_name = unsafe { value::runtime_type_name(machine, &self.modules) }.unwrap_or_default();
        let mut described = json!({ "name": type_name, "line": 0, "column": 0 });
        let Some((module_index, type_row)) = self.class_row(machine) else { return described };
        let module = &self.modules[module_index];
        let Some(metadata) = module.metadata() else { return described };
        let Some(row) = metadata.method_rows_named(type_row, "MoveNext").into_iter().next() else { return described };
        let token = (crate::debugger::metadata::METHOD_DEF as u32) << 24 | row;
        if let Some(name) = metadata.method_name_with(token, &[]) {
            described["name"] = json!(format!("{name}()"));
        }
        // `<>1__state` が 0 以上なら、その番号の `await` で待っている。
        let state = unsafe { value::field(machine, &self.modules, "<>1__state") }.and_then(|v| unsafe { value::describe(&v, &self.modules) }.value.parse::<i64>().ok());
        let Some(pdb) = module.pdb.as_ref() else { return described };
        let awaits = pdb.awaits(token, LATEST);
        let Some(point) = state.and_then(|s| usize::try_from(s).ok()).and_then(|s| awaits.get(s)).and_then(|a| pdb.find_visible_point(token, a.yield_offset, LATEST)) else { return described };
        if let Some(document) = pdb.document(point.document) {
            let name = Path::new(document).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            described["source"] = json!({ "name": name, "path": document });
            described["line"] = json!(point.start_line);
            described["column"] = json!(point.start_column);
            described["endLine"] = json!(point.end_line);
            described["endColumn"] = json!(point.end_column);
        }
        described
    }

    /// runtime-async で再開したフレームの直後に、続きの鎖の呼び出し元を挟む。続きの番地が取れなければ、そのまま。
    fn with_runtime_async_callers(&self, thread: u32, frames: &[ComPtr<ICorDebugFrame>], described: Vec<Value>) -> Vec<Value> {
        // 状態機械でないメソッドでも、ふつうのメソッドに続きの鎖は無い。`AsyncHelpers.Await` を呼ぶものだけ見る。
        if !frames.first().is_some_and(|frame| self.calls_async_await(frame)) {
            return described;
        }
        let Some(address) = self.continuation_address(thread) else { return described };
        let Some(callers) = self.async_caller_frames(address) else { return described };
        let mut logical = Vec::new();
        let mut skipped_current = false;
        for frame in &callers {
            let mut token = 0u32;
            unsafe { frame.GetFunctionToken(&mut token) };
            let top = described.first().and_then(|frame| frame["name"].as_str()).unwrap_or("");
            let name = unsafe { self.frame_module(frame) }.and_then(|module| module.metadata()).and_then(|metadata| metadata.method_name(token)).unwrap_or_default();
            if name.contains("AsyncHelpers") || name.contains("RuntimeAsync") {
                continue;
            }
            // 続きの鎖の先頭は、いま止まっているフレーム自身であることがある。それは足さない。
            if !skipped_current && described.first().is_some_and(|shown| shown["name"].as_str().is_some_and(|shown| shown.starts_with(&name) && !name.is_empty())) && top.starts_with(&name) {
                skipped_current = true;
                continue;
            }
            let mut caller = unsafe { self.describe_frame(0, frame) };
            if caller["line"].as_u64().unwrap_or(0) == 0 && caller["source"].is_null() {
                continue;
            }
            caller["id"] = json!(LOGICAL_FRAME_BASE + 1 + logical.len() as i64);
            logical.push(caller);
        }
        if logical.is_empty() {
            return described;
        }
        let mut out = described;
        let label = json!({ "id": LOGICAL_FRAME_BASE, "name": "[Awaited by]", "line": 0, "column": 0, "presentationHint": "label" });
        out.splice(1.min(out.len())..1.min(out.len()), std::iter::once(label).chain(logical));
        out
    }

    /// runtime-async の stepOut。続きの鎖の、次の利用者のフレーム（呼び出し元の await）にブレークポイントを置く。
    pub(super) fn runtime_async_step_out_breakpoint(&self, thread: u32) -> Option<ComPtr<ICorDebugFunctionBreakpoint>> {
        let top = self.walk_frames(thread).ok()?.into_iter().next()?;
        if !self.calls_async_await(&top) {
            return None;
        }
        let address = self.continuation_address(thread)?;
        let callers = self.async_caller_frames(address)?;
        let current = self.walk_frames(thread).ok()?.into_iter().next().and_then(|frame| {
            let mut token = 0u32;
            unsafe { frame.GetFunctionToken(&mut token) };
            Some(token)
        });
        let mut skipped_current = false;
        for frame in &callers {
            let mut token = 0u32;
            unsafe { frame.GetFunctionToken(&mut token) };
            let name = unsafe { self.frame_module(frame) }.and_then(|module| module.metadata()).and_then(|metadata| metadata.method_name(token)).unwrap_or_default();
            if name.contains("AsyncHelpers") || name.contains("RuntimeAsync") {
                continue;
            }
            if !skipped_current && current == Some(token) {
                skipped_current = true;
                continue;
            }
            let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
            let (mut offset, mut mapping) = (0u32, 0);
            unsafe { il_frame.GetIP(&mut offset, &mut mapping) };
            if mapping & (MAPPING_EXACT | MAPPING_APPROXIMATE) == 0 {
                continue;
            }
            // 続きの鎖が見せる位置は、すでに実行した中断（`AsyncHelpers.Await` の呼び出し）か、その文の先頭。
            // 呼び出し元が次に実行するのは再開の命令なので、そこに置く。中断の位置に置くと、二度と通らずに最後まで走る。
            let resume = self.caller_resume_offset(frame, token, offset);
            let module = unsafe { self.frame_module(frame) }?.module.clone();
            return unsafe { break_at(&module, token, resume) }.map_err(|e| crate::log!("runtime-async step out: {e}")).ok();
        }
        None
    }

    /// 呼び出し元のフレームの IL を、続きが走り始める位置に直す。いまの文にある `await` の再開より前なら、その再開。
    fn caller_resume_offset(&self, frame: &ComPtr<ICorDebugFrame>, token: u32, offset: u32) -> u32 {
        let Some(il) = (unsafe { super::method_il(frame) }) else { return offset };
        let Some(module) = (unsafe { self.frame_module(frame) }) else { return offset };
        let Some(metadata) = module.metadata() else { return offset };
        let version = frame_version(frame);
        let recorded = module.pdb.as_ref().map(|pdb| pdb.awaits(token, version).to_vec()).unwrap_or_default();
        let awaits = if recorded.is_empty() { super::stepping::runtime_async_awaits(&il, metadata) } else { recorded };
        let span_end = module
            .pdb
            .as_ref()
            .and_then(|pdb| pdb.statement_span(token, version, offset))
            .and_then(|(_, end)| end)
            .unwrap_or(u32::MAX);
        awaits
            .into_iter()
            .filter(|await_| offset < await_.resume_offset && (await_.yield_offset < span_end || await_.yield_offset.saturating_sub(offset) <= 48))
            .min_by_key(|await_| await_.yield_offset)
            .map(|await_| await_.resume_offset)
            .unwrap_or(offset)
    }

    /// 再開したフレームの直下のランタイムのフレームから、続き（`Continuation`）の番地。
    fn continuation_address(&self, thread: u32) -> Option<u64> {
        let candidates = self.continuation_candidates(thread)?;
        for &address in &candidates {
            if self.async_caller_frames(address).is_some() {
                return Some(address);
            }
        }
        if !candidates.is_empty() {
            crate::log!("runtime-async: continuation address not accepted ({})", candidates.iter().map(|a| format!("{a:#x}")).collect::<Vec<_>>().join(", "));
        }
        None
    }

    /// 直下のランタイムのフレームが持つ、続きかもしれない番地。`GetAsyncStack` が受け付けるかは、呼んだ側で確かめる。
    fn continuation_candidates(&self, thread: u32) -> Option<Vec<u64>> {
        let process = self.process.clone()?;
        if process.cast::<ICorDebugProcess12>().is_err() {
            return None;
        }
        unsafe {
            let thread = take::<ICorDebugThread>("ICorDebugProcess::GetThread", |out| process.GetThread(thread, out)).ok()?;
            let thread3: ComPtr<ICorDebugThread3> = thread.cast().ok()?;
            let mut raw = null_mut();
            if !thread3.CreateStackWalk(&mut raw).is_ok() {
                return None;
            }
            let walk = ComPtr::<ICorDebugStackWalk>::from_raw(raw)?;
            let mut saw_user = false;
            for _ in 0..64 {
                let mut raw = null_mut();
                if !walk.GetFrame(&mut raw).is_ok() {
                    break;
                }
                let frame = ComPtr::<ICorDebugFrame>::from_raw(raw);
                if !saw_user {
                    if frame.as_ref().is_some_and(|frame| frame.cast::<ICorDebugILFrame>().is_ok() && !self.is_exception_machinery(frame)) {
                        saw_user = true;
                    }
                } else {
                    // 直下から、ランタイムの IL フレームが続くあいだ（間のネイティブのフレームは飛ばす）。
                    // 続きのオブジェクトは、すぐ下のメソッドの this とは限らず、その数段下の Continuation にある。
                    let mut candidates = Vec::new();
                    let mut under = frame;
                    let mut runtime_frames = 0;
                    for _ in 0..12 {
                        if let Some(frame) = under.as_ref() {
                            if frame.cast::<ICorDebugILFrame>().is_ok() {
                                if !self.frame_is_runtime_async(frame) {
                                    break;
                                }
                                runtime_frames += 1;
                                candidates.extend(self.continuation_from_frame(frame));
                                candidates.extend(register_and_stack_addresses(&process, &walk));
                                if runtime_frames >= 6 {
                                    break;
                                }
                            }
                        }
                        let mut raw = null_mut();
                        if walk.Next().0 != 0 || !walk.GetFrame(&mut raw).is_ok() {
                            break;
                        }
                        under = ComPtr::<ICorDebugFrame>::from_raw(raw);
                    }
                    if candidates.is_empty() && native_ip_is_runtime(self, &walk) {
                        candidates.extend(stack_slot_addresses(&process, &walk));
                    }
                    return Some(candidates);
                }
                if walk.Next().0 != 0 {
                    break;
                }
            }
            None
        }
    }

    /// `GetAsyncStack` が返す続きの鎖（IL のフレームだけ）。
    fn async_caller_frames(&self, address: u64) -> Option<Vec<ComPtr<ICorDebugFrame>>> {
        let process = self.process.as_ref()?;
        let process12: ComPtr<ICorDebugProcess12> = process.cast().ok()?;
        let mut raw = null_mut();
        let hr = unsafe { process12.GetAsyncStack(address, &mut raw) };
        if !hr.is_ok() {
            return None;
        }
        let walk = unsafe { ComPtr::<ICorDebugStackWalk>::from_raw(raw) }?;
        let mut frames = Vec::new();
        unsafe {
            for _ in 0..MAX_CALLERS {
                let mut raw = null_mut();
                if !walk.GetFrame(&mut raw).is_ok() {
                    break;
                }
                if let Some(frame) = ComPtr::<ICorDebugFrame>::from_raw(raw) {
                    if frame.cast::<ICorDebugILFrame>().is_ok() {
                        frames.push(frame);
                    }
                }
                if walk.Next().0 != 0 {
                    break;
                }
            }
        }
        (!frames.is_empty()).then_some(frames)
    }

    fn frame_is_runtime_async(&self, frame: &ComPtr<ICorDebugFrame>) -> bool {
        let mut token = 0u32;
        unsafe { frame.GetFunctionToken(&mut token) };
        unsafe { self.frame_module(frame) }
            .and_then(|module| module.metadata())
            .and_then(|metadata| metadata.method_name(token))
            .is_some_and(|name| name.contains("AsyncHelpers") || name.contains("RuntimeAsync") || name.contains(".Continuation"))
    }

    /// ランタイムのフレームが持つ続きの番地。`this` が `Continuation` ならその番地。
    /// `Task`（runtime-async の `RuntimeAsyncTask`）なら、`m_continuationObject`。
    fn continuation_from_frame(&self, frame: &ComPtr<ICorDebugFrame>) -> Vec<u64> {
        let mut found = Vec::new();
        let Ok(il_frame) = frame.cast::<ICorDebugILFrame>() else { return found };
        let mut raw = null_mut();
        if !unsafe { il_frame.GetArgument(0, &mut raw) }.is_ok() {
            return found;
        }
        let Some(this) = (unsafe { ComPtr::<ICorDebugValue>::from_raw(raw) }) else { return found };
        if let Some(continuation) = unsafe { value::field(&this, &self.modules, "m_continuationObject") } {
            let type_name = unsafe { value::runtime_type_name(&continuation, &self.modules) }.unwrap_or_default();
            let address = unsafe { object_address(&continuation) };
            // Task の m_continuationObject は、runtime-async では待ちの完了通知であって Continuation ではない。
            if address != 0 && type_name.contains("Continuation") {
                found.push(address);
            }
        }
        // `RuntimeAsyncTaskContinuation` のように、this 自身が続きのとき。
        let type_name = unsafe { value::runtime_type_name(&this, &self.modules) }.unwrap_or_default();
        if type_name.contains("Continuation") {
            let address = unsafe { object_address(&this) };
            if address != 0 {
                found.push(address);
            }
        }
        found
    }

    /// メソッドが `AsyncHelpers.Await` を呼ぶ（runtime-async の中断）。
    fn calls_async_await(&self, frame: &ComPtr<ICorDebugFrame>) -> bool {
        let Some(il) = (unsafe { method_il(frame) }) else { return false };
        let Some(metadata) = (unsafe { self.frame_module(frame) }).and_then(|module| module.metadata()) else { return false };
        crate::debugger::il::find_calls(&il, 0, il.len() as u32).iter().any(|(_, token)| metadata.call_name(*token) == "AsyncHelpers.Await")
    }
}

/// 直下のフレームの命令が、ランタイムのモジュール（coreclr.dll / clr.dll）の中か。
fn native_ip_is_runtime(session: &Session, walk: &ComPtr<ICorDebugStackWalk>) -> bool {
    let Some(process_id) = session.target_process_id() else { return false };
    let mut context = vec![0u8; crate::win32::CONTEXT_SIZE];
    let mut size = 0u32;
    if !unsafe { walk.GetContext(crate::win32::CONTEXT_CONTROL, context.len() as u32, &mut size, context.as_mut_ptr()) }.is_ok() {
        return false;
    }
    let Some(ip) = crate::win32::context_ip(&context).filter(|&ip| ip != 0) else { return false };
    crate::win32::process_module_ranges(process_id).iter().any(|module| {
        ip >= module.base && ip < module.base + module.size && matches!(module.name.to_ascii_lowercase().as_str(), "coreclr.dll" | "clr.dll")
    })
}

/// いまのフレームの this レジスターと、スタックの先頭の数語。続きの番地の候補にする。
fn register_and_stack_addresses(process: &ComPtr<ICorDebugProcess>, walk: &ComPtr<ICorDebugStackWalk>) -> Vec<u64> {
    let mut context = vec![0u8; crate::win32::CONTEXT_SIZE];
    let mut size = 0u32;
    let flags = crate::win32::CONTEXT_CONTROL | crate::win32::CONTEXT_INTEGER;
    if !unsafe { walk.GetContext(flags, context.len() as u32, &mut size, context.as_mut_ptr()) }.is_ok() {
        return stack_slot_addresses(process, walk);
    }
    let mut found = Vec::new();
    if let Some(this) = crate::win32::context_cx(&context).filter(|&this| this > 0x1_0000) {
        found.push(this as u64);
    }
    found.extend(stack_slot_addresses(process, walk));
    found
}

/// 直下のフレームのスタックから、ポインターの大きさの値を読む。
fn stack_slot_addresses(process: &ComPtr<ICorDebugProcess>, walk: &ComPtr<ICorDebugStackWalk>) -> Vec<u64> {
    let mut context = vec![0u8; crate::win32::CONTEXT_SIZE];
    let mut size = 0u32;
    if !unsafe { walk.GetContext(crate::win32::CONTEXT_CONTROL, context.len() as u32, &mut size, context.as_mut_ptr()) }.is_ok() {
        return Vec::new();
    }
    let Some(sp) = crate::win32::context_sp(&context).filter(|&sp| sp != 0) else { return Vec::new() };
    let width = std::mem::size_of::<usize>();
    let mut bytes = vec![0u8; width * 8];
    let mut read = 0usize;
    if !unsafe { process.ReadMemory(sp as u64, bytes.len() as u32, bytes.as_mut_ptr(), &mut read) }.is_ok() {
        return Vec::new();
    }
    bytes[..read]
        .chunks(width)
        .filter_map(|chunk| {
            if chunk.len() != width {
                return None;
            }
            let mut wide = [0u8; 8];
            wide[..width].copy_from_slice(chunk);
            let value = usize::from_le_bytes(wide[..width].try_into().ok()?);
            (value > 0x1_0000).then_some(value as u64)
        })
        .collect()
}

/// 型が、コンパイラの作った状態機械か（.NET の箱 `AsyncStateMachineBox<…>` は、型引数に状態機械の名前を持つが違う）。
fn is_machine(type_name: &str) -> bool {
    !type_name.contains("AsyncStateMachineBox") && value::is_state_machine(type_name)
}

/// フレームの `this`（インスタンスのメソッドの 0 番目の引数）。
unsafe fn frame_this(frame: &ComPtr<ICorDebugFrame>) -> Option<ComPtr<ICorDebugValue>> {
    let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
    take::<ICorDebugValue>("ICorDebugILFrame::GetArgument", |out| il_frame.GetArgument(0, out)).ok()
}
