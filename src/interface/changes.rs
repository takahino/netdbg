//! 停止のあいだに変わったコントロール（D-087）。ハブが持つ（DAP のクライアントと MCP の両方に出す）。
//!
//! 対象が動いている間は `gui::watch` が画面の木を読み続け、止まるたびに、前の停止からの変化を `netdbg.treeChanged`
//! で知らせる。`netdbg.treeChanges`（MCP の `ui_changes`）は、最後の停止の分を返す。

use crate::debugger::engine::Engine;
use crate::debugger::session::Event;
use crate::gui::watch::{self, Watcher};
use serde_json::{json, Value};

/// 変化の要求か。
pub fn handles(command: &str) -> bool {
    command == "netdbg.treeChanges"
}

#[derive(Default)]
pub struct TreeChanges {
    /// 見ているプロセスと、その見張り。
    watching: Option<(u32, Watcher)>,
    /// 前に見たとき止まっていたか（止まった瞬間だけ比べる）。
    was_stopped: bool,
    /// いまのプロセスで止まった回数。
    stops: u64,
    /// 最後の停止の変化（`netdbg.treeChanged` の本文）。
    last: Option<Value>,
}

impl TreeChanges {
    /// 入力を 1 つ処理したあと: いまの対象のプロセスを見る（起動し直したら付け替える）。止まった瞬間なら、変化の知らせ。
    pub fn follow(&mut self, engine: &Engine) -> Option<Event> {
        let process_id = engine.target_process_id();
        if self.watching.as_ref().map(|(p, _)| *p) != process_id {
            *self = TreeChanges::default();
            if let Some(process_id) = process_id {
                match watch::start(process_id) {
                    Ok(watcher) => self.watching = Some((process_id, watcher)),
                    Err(e) => crate::log!("tree watch: {e}"),
                }
            }
        }
        let stopped = engine.is_stopped();
        let event = if stopped && !self.was_stopped { self.at_stop() } else { None };
        self.was_stopped = stopped;
        event
    }

    fn at_stop(&mut self) -> Option<Event> {
        let (_, watcher) = self.watching.as_ref()?;
        let found = watcher.at_stop();
        self.stops += 1;
        let changes: Vec<Value> = found.changes.iter().map(|c| json!({ "control": c.control, "what": c.what, "before": c.before, "after": c.after, "text": c.text() })).collect();
        let body = json!({ "stop": self.stops, "compared": found.compared, "changes": changes, "unread": found.unread });
        self.last = Some(body.clone());
        Some(("netdbg.treeChanged", body))
    }

    /// `netdbg.treeChanges`: 最後の停止の変化。
    pub fn request(&self) -> Result<Value, String> {
        if self.watching.is_none() {
            return Err(tr!("The program's windows are not being watched (start debugging a program with windows first)."));
        }
        let mut body = self.last.clone().ok_or_else(|| tr!("The program has not stopped yet. The changes are compared between two stops (a breakpoint, a pause or a step)."))?;
        if !self.was_stopped {
            body["note"] = json!(tr!("The program is running. These are the changes up to the last stop."));
        }
        Ok(body)
    }
}
