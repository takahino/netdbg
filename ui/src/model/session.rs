//! DAP の出来事から作る、セッションの状態（ブラウザ無しで動く。単体試験はここ）。
//!
//! `apply` は状態を変え、画面の側がそのあとにすること（止まったらスタックを読む、など）を `Follow` で返す。

use super::coverage::Totals;
use super::records::Records;
use super::report::Report;
use serde_json::Value;

/// 出力の行の上限（古いものから捨てる）。
pub const MAX_OUTPUT: usize = 5000;
/// AI の操作の履歴の上限。
pub const MAX_ACTIONS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    /// 何も開いていない。
    #[default]
    Idle,
    /// 起動やアタッチの途中。
    Starting,
    Running,
    Stopped {
        thread: i64,
        reason: String,
        /// 例外の文言など（無ければ空）。
        text: String,
    },
    /// 対象が終わった（終了コード）。
    Ended(Option<i64>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub path: String,
    pub symbols: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputLine {
    pub number: u64,
    pub category: String,
    pub text: String,
}

/// AI（MCP）の操作 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub id: u64,
    pub tool: String,
    pub arguments: Value,
    /// エンジンの時刻（ミリ秒）。
    pub time: f64,
    /// 終わっていれば、成否と時間と要約。
    pub finished: Option<(bool, u64, String)>,
}

impl Action {
    /// 操作がソースの場所を指していれば、そのファイルと行（`breakpoint_set` など）。
    pub fn location(&self) -> Option<(String, u32)> {
        let file = self.arguments["file"].as_str().or(self.arguments["path"].as_str())?;
        let line = self.arguments["line"].as_u64().and_then(|l| u32::try_from(l).ok()).unwrap_or(1);
        Some((file.to_string(), line))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub status: Status,
    /// 対象のプロセス（`process` の出来事）。
    pub process_id: Option<u32>,
    pub process_name: String,
    /// ダンプを開いた（読むだけ）。
    pub dump: bool,
    /// 観察専用でアタッチした（値の書き換えと関数の評価は断られる。D-089）。
    pub observe_only: bool,
    pub modules: Vec<Module>,
    pub output: Vec<OutputLine>,
    next_output: u64,
    pub actions: Vec<Action>,
    /// 止めずに読んだ記録（SQL、例外、ダイアログなど）。
    pub records: Records,
    /// 同じエンジンの MCP（AI がつなぐ先の URL）。
    pub mcp_url: Option<String>,
    /// エンジンの PID（`--mcp-connect=<PID>`）。
    pub engine_pid: Option<u64>,
    /// カバレッジの合計（数えているときだけ）。
    pub coverage: Option<Totals>,
    /// 成績書の項目（エンジンのハブが持つので、新しいセッションでも消さない）。
    pub report: Report,
    /// 前の停止から変わったコントロール（止まるたびに届く。D-087）。
    pub tree_changes: Option<TreeChanges>,
}

/// 前の停止から変わったコントロール（`netdbg.treeChanged`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeChanges {
    /// 何回目の停止か。
    pub stop: u64,
    /// 比べる元があったか（最初に読む前に止まると無い）。
    pub compared: bool,
    /// 変化の文（いまの言語）。
    pub changes: Vec<String>,
    /// 止まる直前に変わり、止まっている間は値を読めないコントロール。
    pub unread: Vec<String>,
}

impl TreeChanges {
    pub fn from_body(body: &Value) -> TreeChanges {
        let texts = |key: &str, field: Option<&str>| {
            body[key].as_array().into_iter().flatten().filter_map(|v| match field {
                Some(field) => v[field].as_str(),
                None => v.as_str(),
            }).map(String::from).collect()
        };
        TreeChanges { stop: body["stop"].as_u64().unwrap_or(0), compared: body["compared"] == true, changes: texts("changes", Some("text")), unread: texts("unread", None) }
    }
}

/// 状態を変えたあと、画面の側がすること。
#[derive(Clone, Debug, PartialEq)]
pub enum Follow {
    Nothing,
    /// 止まった: スレッド、スタック、変数を読み直す。
    Stopped(i64),
    /// 動き出した: スタックと変数を消す。
    Continued,
    /// 値が変わったかもしれない（`invalidated`）: 変数とウォッチを読み直す。
    Refresh,
    /// ブレークポイントの状態が変わった（`breakpoint` の出来事の本文）。
    Breakpoint(Value),
    /// カバレッジの印が変わったファイル（開いていれば読み直す）。
    Coverage(Vec<String>),
    Ended,
}

impl Session {
    /// 新しいセッションを始める（前の出力と履歴は残す）。
    pub fn begin(&mut self) {
        self.status = Status::Starting;
        self.process_id = None;
        self.process_name.clear();
        self.dump = false;
        self.observe_only = false;
        self.modules.clear();
        self.coverage = None;
        self.tree_changes = None;
        // エンジンも、新しい対象で出来事の列を空にする。
        self.records.clear();
    }

    /// DAP の出来事 1 つ（メッセージ全体）。
    pub fn apply(&mut self, message: &Value) -> Follow {
        if message["type"] != "event" {
            return Follow::Nothing;
        }
        let body = &message["body"];
        let name = message["event"].as_str().unwrap_or("");
        if self.records.apply(name, body) {
            return Follow::Nothing;
        }
        match name {
            "initialized" => {
                if self.status == Status::Idle || matches!(self.status, Status::Ended(_)) {
                    self.status = Status::Starting;
                }
                Follow::Nothing
            }
            "process" => {
                self.process_id = body["systemProcessId"].as_u64().and_then(|p| u32::try_from(p).ok());
                self.process_name = body["name"].as_str().unwrap_or("").to_string();
                self.dump = body.get("dumpProcessId").is_some();
                self.observe_only = body["netdbgObserveOnly"] == true;
                if self.status == Status::Starting || self.status == Status::Idle {
                    self.status = Status::Running;
                }
                Follow::Nothing
            }
            "stopped" => {
                let thread = body["threadId"].as_i64().unwrap_or(0);
                let text = body["text"].as_str().or(body["description"].as_str()).unwrap_or("").to_string();
                self.status = Status::Stopped { thread, reason: body["reason"].as_str().unwrap_or("").to_string(), text };
                Follow::Stopped(thread)
            }
            "continued" => {
                self.status = Status::Running;
                Follow::Continued
            }
            "exited" => {
                self.status = Status::Ended(body["exitCode"].as_i64());
                Follow::Ended
            }
            "terminated" => {
                if !matches!(self.status, Status::Ended(_)) {
                    self.status = Status::Ended(None);
                }
                Follow::Ended
            }
            "invalidated" => Follow::Refresh,
            "breakpoint" => Follow::Breakpoint(body.clone()),
            "module" => {
                self.module(body);
                Follow::Nothing
            }
            "output" => {
                let category = body["category"].as_str().unwrap_or("console");
                // 計測の記録（telemetry）は見せない。
                if category != "telemetry" {
                    self.push_output(category, body["output"].as_str().unwrap_or(""));
                }
                Follow::Nothing
            }
            "netdbg.agentAction" => {
                self.action(body);
                Follow::Nothing
            }
            "netdbg.coverageChanged" | "netdbg.coverage" => {
                let mut totals = Totals::from_body(body);
                // 書いた先は、書いたあとの知らせにだけ載る。
                if totals.path.is_none() {
                    totals.path = self.coverage.as_ref().and_then(|t| t.path.clone());
                }
                self.coverage = Some(totals);
                let files = body["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                Follow::Coverage(files)
            }
            "netdbg.treeChanged" => {
                self.tree_changes = Some(TreeChanges::from_body(body));
                Follow::Nothing
            }
            "netdbg.testItemChanged" => {
                self.report = Report::from_body(body);
                Follow::Nothing
            }
            "netdbg.mcpEndpoint" => {
                self.mcp_url = body["url"].as_str().map(String::from);
                self.engine_pid = body["enginePid"].as_u64();
                Follow::Nothing
            }
            _ => Follow::Nothing,
        }
    }

    /// 画面の側の出来事（要求の失敗、式の評価の結果）を出力に足す。
    pub fn push_output(&mut self, category: &str, text: &str) {
        self.next_output += 1;
        self.output.push(OutputLine { number: self.next_output, category: category.to_string(), text: text.to_string() });
        if self.output.len() > MAX_OUTPUT {
            let excess = self.output.len() - MAX_OUTPUT;
            self.output.drain(..excess);
        }
    }

    fn module(&mut self, body: &Value) {
        let module = &body["module"];
        let id = module["id"].as_str().map(String::from).or_else(|| module["id"].as_i64().map(|i| i.to_string())).unwrap_or_default();
        let position = self.modules.iter().position(|m| m.id == id);
        if body["reason"] == "removed" {
            if let Some(at) = position {
                self.modules.remove(at);
            }
            return;
        }
        let entry = Module {
            id,
            name: module["name"].as_str().unwrap_or("").to_string(),
            path: module["path"].as_str().unwrap_or("").to_string(),
            symbols: module["symbolStatus"].as_str().unwrap_or("").to_string(),
        };
        match position {
            Some(at) => self.modules[at] = entry,
            None => self.modules.push(entry),
        }
    }

    fn action(&mut self, body: &Value) {
        let id = body["id"].as_u64().unwrap_or(0);
        if body["state"] == "finished" {
            if let Some(action) = self.actions.iter_mut().rev().find(|a| a.id == id) {
                action.finished = Some((body["ok"].as_bool().unwrap_or(false), body["ms"].as_u64().unwrap_or(0), body["summary"].as_str().unwrap_or("").to_string()));
            }
            return;
        }
        self.actions.push(Action {
            id,
            tool: body["tool"].as_str().unwrap_or("").to_string(),
            arguments: body["arguments"].clone(),
            time: body["time"].as_f64().unwrap_or(0.0),
            finished: None,
        });
        if self.actions.len() > MAX_ACTIONS {
            self.actions.remove(0);
        }
    }

    pub fn is_stopped(&self) -> bool {
        matches!(self.status, Status::Stopped { .. })
    }

    pub fn is_running(&self) -> bool {
        self.status == Status::Running
    }

    /// 対象があるか（動いているか止まっている）。
    pub fn is_live(&self) -> bool {
        matches!(self.status, Status::Running | Status::Stopped { .. } | Status::Starting)
    }

    pub fn stopped_thread(&self) -> Option<i64> {
        match self.status {
            Status::Stopped { thread, .. } => Some(thread),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(name: &str, body: Value) -> Value {
        json!({ "type": "event", "event": name, "body": body })
    }

    #[test]
    fn a_session_runs_stops_continues_and_ends() {
        let mut s = Session::default();
        s.begin();
        assert_eq!(s.apply(&event("process", json!({ "name": "a.exe", "systemProcessId": 12 }))), Follow::Nothing);
        assert_eq!((s.process_id, s.is_running()), (Some(12), true));
        assert_eq!(s.apply(&event("stopped", json!({ "reason": "breakpoint", "threadId": 5 }))), Follow::Stopped(5));
        assert_eq!(s.stopped_thread(), Some(5));
        assert_eq!(s.apply(&event("continued", json!({ "threadId": 5 }))), Follow::Continued);
        assert_eq!(s.apply(&event("exited", json!({ "exitCode": 3 }))), Follow::Ended);
        s.apply(&event("terminated", json!({})));
        assert_eq!(s.status, Status::Ended(Some(3)));
        assert!(!s.is_live());
    }

    #[test]
    fn the_changes_since_the_previous_stop_are_kept_until_the_next_session() {
        let mut s = Session::default();
        s.apply(&event("netdbg.treeChanged", json!({ "stop": 2, "compared": true, "changes": [{ "control": "resultLabel", "text": "resultLabel の文字: 「」→「Hello」" }], "unread": ["nameBox"] })));
        let changes = s.tree_changes.clone().unwrap();
        assert_eq!((changes.stop, changes.compared, changes.changes.len(), changes.unread.as_slice()), (2, true, 1, ["nameBox".to_string()].as_slice()));
        s.begin();
        assert_eq!(s.tree_changes, None);
    }

    #[test]
    fn exceptions_keep_their_text_and_dumps_are_marked() {
        let mut s = Session::default();
        s.apply(&event("process", json!({ "name": "a.dmp", "dumpProcessId": 7 })));
        assert!(s.dump);
        assert_eq!(s.process_id, None);
        s.apply(&event("stopped", json!({ "reason": "exception", "threadId": 1, "text": "An unhandled exception" })));
        assert_eq!(s.status, Status::Stopped { thread: 1, reason: "exception".into(), text: "An unhandled exception".into() });
    }

    #[test]
    fn modules_are_added_changed_and_removed() {
        let mut s = Session::default();
        let module = |reason: &str, status: &str| event("module", json!({ "reason": reason, "module": { "id": "C:\\a.dll", "name": "a.dll", "path": "C:\\a.dll", "symbolStatus": status } }));
        s.apply(&module("new", "Symbols not found."));
        s.apply(&module("changed", "Symbols decompiled."));
        assert_eq!((s.modules.len(), s.modules[0].symbols.as_str()), (1, "Symbols decompiled."));
        s.apply(&module("removed", ""));
        assert!(s.modules.is_empty());
    }

    #[test]
    fn output_is_capped_and_telemetry_is_hidden() {
        let mut s = Session::default();
        for i in 0..MAX_OUTPUT + 3 {
            s.apply(&event("output", json!({ "category": "stdout", "output": i.to_string() })));
        }
        s.apply(&event("output", json!({ "category": "telemetry", "output": "x" })));
        assert_eq!(s.output.len(), MAX_OUTPUT);
        assert_eq!(s.output[0].text, "3");
        assert_eq!(s.output.last().unwrap().number, MAX_OUTPUT as u64 + 3);
    }

    #[test]
    fn agent_actions_start_and_finish() {
        let mut s = Session::default();
        s.apply(&event("netdbg.agentAction", json!({ "id": 1, "tool": "breakpoint_set", "state": "started", "arguments": { "file": "C:\\P.cs", "line": 9 }, "time": 5.0 })));
        s.apply(&event("netdbg.agentAction", json!({ "id": 1, "tool": "breakpoint_set", "state": "finished", "ok": true, "ms": 3, "summary": "{}" })));
        assert_eq!(s.actions[0].finished, Some((true, 3, "{}".into())));
        assert_eq!(s.actions[0].location(), Some(("C:\\P.cs".into(), 9)));
    }

    #[test]
    fn records_are_collected_and_cleared_for_a_new_session() {
        let mut s = Session::default();
        s.apply(&event("netdbg.bindingFailed", json!({ "seq": 1, "direction": "parse", "control": "amountBox", "property": "Text", "member": "Amount" })));
        assert_eq!(s.records.list[0].summary(), "parse amountBox.Text ↔ Amount");
        s.begin();
        assert!(s.records.list.is_empty());
    }

    #[test]
    fn responses_are_not_events() {
        let mut s = Session::default();
        assert_eq!(s.apply(&json!({ "type": "response", "command": "threads" })), Follow::Nothing);
        assert_eq!(s, Session::default());
    }
}
