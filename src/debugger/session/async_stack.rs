//! 非同期の論理コールスタック（段階 12）: `await` のあとで再開した async メソッドの本体（状態機械の MoveNext）は、
//! 物理的なスタックの下にスレッド プールなどの仕組みしかない。誰が `await` していたかを、状態機械からたどって戻す
//! （Visual Studio の「async のコール スタック」と同じもの）。
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
    pub(super) fn with_async_callers(&self, frames: &[ComPtr<ICorDebugFrame>], described: Vec<Value>) -> Vec<Value> {
        let found = frames.iter().enumerate().take(described.len()).find_map(|(depth, frame)| {
            let this = unsafe { frame_this(frame) }?;
            // 型の名前だけを読む（describe は DebuggerDisplay などで対象のコードを動かすことがある。スタックのたびには重い）。
            let type_name = unsafe { value::runtime_type_name(&this, &self.modules) }?;
            is_machine(&type_name).then_some((depth, this))
        });
        let Some((depth, machine)) = found else { return described };
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
