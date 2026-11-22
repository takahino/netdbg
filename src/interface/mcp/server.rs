//! MCP のサーバー: JSON-RPC のメソッド（initialize、tools/list、tools/call）に答え、ツールの答えを待つ。
//!
//! 止まるまで待つツールは、ループを止めずに待ちを登録し、本体の出来事（止まった、終わった）か期限で答える。GUI の
//! ツールは GUI のスレッドで処理し、終わり（`UiDone`）が入力の列で届いたら答える。

use super::state::{State, Target};
use super::tools::breakpoints::{self, Breakpoints};
use super::tools::ui::{self, UiAnswer, UiMemory, UiQuery, UiWork};
use super::tools::{self, Context, Outcome};
use super::{error, result, Backend, Call, UiDone, INVALID_PARAMS, METHOD_NOT_FOUND, PROTOCOL_VERSION};
use crate::debugger::session::Event;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// MCP をどう使っているか。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// MCP を使わない（DAP だけ）。
    Off,
    /// `--mcp`: MCP だけ。起動とアタッチも MCP のツールで行う。
    Standalone,
    /// `--mcp-listen`: DAP のクライアント（VS Code）が起動したセッションを、MCP からも操作する。
    Listening,
}

/// GUI のツールの答えを待つ時間の上限。対象の UI のスレッドが止まったまま（ブレークポイントなど）だと、メッセージや
/// UI Automation の呼び出しが返らないことがある。
const UI_JOB_TIMEOUT: Duration = Duration::from_secs(15);

/// 答えを待っている呼び出し（答えの送り先と、AI の操作の履歴に載せる分）。
struct Pending {
    id: Value,
    reply: Sender<Option<Value>>,
    action: Option<Action>,
}

/// AI の操作（ツールの呼び出し）1 つ。始まりと終わりを `netdbg.agentAction` で知らせる（専用の画面の履歴）。
struct Action {
    number: u64,
    tool: String,
    /// 引数（縮めていないもの。画面の操作を、流せる手順にする。D-086）。
    arguments: Value,
    started: Instant,
}

/// 履歴に載せる値の長さの上限（引数の文字列、答えの要約）。
const ACTION_TEXT: usize = 200;

impl Pending {
    /// 答えを送る。ツールの呼び出しなら、終わったことを知らせる。
    fn answer(self, answer: Value, backend: &mut dyn Backend) {
        if let Some(action) = &self.action {
            let failed = answer["result"]["isError"].as_bool().unwrap_or(false) || answer.get("error").is_some();
            let text = answer["result"]["content"][0]["text"].as_str().or(answer["error"]["message"].as_str()).unwrap_or("");
            let mut finished = json!({ "id": action.number, "tool": action.tool, "state": "finished", "ok": !failed, "ms": action.started.elapsed().as_millis() as u64, "summary": shorten(text) });
            // 画面の操作なら、操作した箇所（画面の座標。成績書で塗る）: コントロールの範囲か、マウスの点。もう一度流す
            // ときの手順も付ける（相手は、答えにある名前で指す）。
            if let Some(done) = (!failed).then(|| serde_json::from_str::<Value>(text).ok()).flatten() {
                if let Some(spot) = done["target"].get("screen").or(done.get("screen")) {
                    finished["spot"] = spot.clone();
                }
                if let Some(step) = crate::report::steps::from_ai(&action.tool, &action.arguments, done.get("target")) {
                    finished["step"] = step;
                }
            }
            backend.notify("netdbg.agentAction", finished);
        }
        let _ = self.reply.send(Some(answer));
    }
}

/// 長い文字列を、先頭の `ACTION_TEXT` 文字に縮める。
fn shorten(text: &str) -> String {
    if text.chars().count() <= ACTION_TEXT {
        return text.to_string();
    }
    text.chars().take(ACTION_TEXT).collect::<String>() + "…"
}

/// 引数の中の長い文字列を縮める（履歴に載せる形）。
fn shorten_values(value: &Value) -> Value {
    match value {
        Value::String(s) => json!(shorten(s)),
        Value::Array(items) => Value::Array(items.iter().take(20).map(shorten_values).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), shorten_values(v))).collect()),
        other => other.clone(),
    }
}

/// 止まるのを待っている呼び出し。
struct Waiter {
    pending: Pending,
    deadline: Instant,
    kind: WaitKind,
    /// `WaitKind::Ui` で、次に画面を見直す時刻。
    poll_at: Instant,
    /// `WaitKind::Ui` で、見直しの最中（GUI のスレッドの処理の番号）。
    poll_job: Option<u64>,
}

/// 何を待っているか。
enum WaitKind {
    /// 止まるか終わるまで。
    Stop,
    /// 画面の操作のあと。止まらなければ、操作の結果（中身）と状態を返す。
    Action(Value),
    /// 画面の条件（`ui_wait_for`）。止まれば、そこで答える。
    Ui(UiQuery),
}

/// GUI のスレッドで処理している、GUI のツールの呼び出し。
struct UiJob {
    pending: Pending,
    deadline: Instant,
}

pub struct Server {
    mode: Mode,
    state: State,
    breakpoints: Breakpoints,
    ui: UiMemory,
    waiters: Vec<Waiter>,
    ui_jobs: HashMap<u64, UiJob>,
    /// AI の操作の通し番号。
    actions: u64,
}

/// ツールの処理の文脈（本体と、MCP の側の状態）を作る。待ちと GUI の処理の一覧は別に借りられるよう、マクロで欄ごとに借りる。
macro_rules! context {
    ($server:expr, $backend:expr) => {
        Context { backend: $backend, mode: $server.mode, state: &mut $server.state, breakpoints: &mut $server.breakpoints, ui: &mut $server.ui }
    };
}

impl Server {
    pub fn new(mode: Mode) -> Server {
        Server { mode, state: State::new(), breakpoints: Breakpoints::new(), ui: UiMemory::default(), waiters: Vec::new(), ui_jobs: HashMap::new(), actions: 0 }
    }

    /// 本体の答えと出来事を見る（DAP のクライアントの操作や、対象の出来事。MCP の操作の分は、ツールの処理の中で見る）。
    pub fn observe(&mut self, answered: Option<(&str, bool)>, events: &[Event]) {
        tools::observe(&mut self.state, &mut self.breakpoints, answered, events);
    }

    /// 入力を 1 つ処理したあと: トレースポイントで止まったものを記録して続行し、止まったか終わったなら、待っている
    /// 呼び出しに答える。
    pub fn settle(&mut self, backend: &mut dyn Backend) -> io::Result<()> {
        breakpoints::run_tracepoints(&mut context!(self, backend))?;
        self.answer_waiters(backend)
    }

    /// 待っている呼び出しのうち、いちばん早い期限。入力のループは、これまでに入力が無ければ `expire` を呼ぶ。
    pub fn deadline(&self) -> Option<Instant> {
        let waiters = self.waiters.iter().map(|w| if matches!(w.kind, WaitKind::Ui(_)) && w.poll_job.is_none() { w.deadline.min(w.poll_at) } else { w.deadline });
        waiters.chain(self.ui_jobs.values().map(|j| j.deadline)).min()
    }

    /// 期限の過ぎた待ちに、「まだ止まっていない」と答える。画面の条件を待っているものは、画面を見直す。
    pub fn expire(&mut self, backend: &mut dyn Backend) -> io::Result<()> {
        let now = Instant::now();
        let late: Vec<u64> = self.ui_jobs.iter().filter(|(_, j)| j.deadline <= now).map(|(&k, _)| k).collect();
        for job in late.into_iter().filter_map(|k| self.ui_jobs.remove(&k)) {
            let answer = result(&job.pending.id, text_result(Err(tr!("The target's windows do not answer (its UI thread may be stopped). Check with debug_status."))));
            job.pending.answer(answer, backend);
        }
        for mut waiter in std::mem::take(&mut self.waiters) {
            let mut ctx = context!(self, backend);
            let answer = match &waiter.kind {
                WaitKind::Ui(_) if waiter.deadline <= now => Some(Ok(json!({ "found": false, "waited": "timeout", "status": ctx.status()? }))),
                WaitKind::Ui(query) => {
                    // 見直しは GUI のスレッドで（終わると `ui_finished` に届く）。
                    if waiter.poll_job.is_none() && waiter.poll_at <= now {
                        waiter.poll_job = ui::spawn(&mut ctx, UiWork::Poll(query.clone())).ok();
                    }
                    None
                }
                _ if waiter.deadline > now => None,
                WaitKind::Action(done) => {
                    let mut done = done.clone();
                    done["status"] = ctx.status()?;
                    Some(Ok(done))
                }
                _ => Some(Ok(json!({ "waited": "timeout", "status": ctx.status()? }))),
            };
            match answer {
                Some(outcome) => {
                    let answer = result(&waiter.pending.id, text_result(outcome));
                    waiter.pending.answer(answer, backend);
                }
                None => self.waiters.push(waiter),
            }
        }
        Ok(())
    }

    /// MCP のメッセージ 1 つ。
    pub fn call(&mut self, call: Call, backend: &mut dyn Backend) -> io::Result<()> {
        let Call { message, reply } = call;
        let Some(id) = message.get("id").cloned() else {
            // 通知（notifications/initialized など）には答えない。
            let _ = reply.send(None);
            return Ok(());
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let answer = match message.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" => result(
                &id,
                json!({
                    "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION),
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "netdbg", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            ),
            "ping" => result(&id, json!({})),
            "tools/list" => result(&id, json!({ "tools": tools::list(self.mode == Mode::Standalone) })),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    let _ = reply.send(Some(error(&id, INVALID_PARAMS, &tr!("tools/call has no name."))));
                    return Ok(());
                };
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                self.actions += 1;
                let action = Action { number: self.actions, tool: name.to_string(), arguments: arguments.clone(), started: Instant::now() };
                backend.notify("netdbg.agentAction", json!({ "id": action.number, "tool": name, "state": "started", "arguments": shorten_values(&arguments), "time": crate::debugger::journal::now_ms() }));
                let outcome = tools::call(&mut context!(self, backend), name, &arguments)?;
                return self.finish(Pending { id, reply, action: Some(action) }, outcome, backend);
            }
            other => error(&id, METHOD_NOT_FOUND, &tr!("Unknown method: {method}", method = other)),
        };
        let _ = reply.send(Some(answer));
        Ok(())
    }

    /// GUI のスレッドの処理が終わった。待っている呼び出しに答える。
    pub fn ui_finished(&mut self, done: UiDone, backend: &mut dyn Backend) -> io::Result<()> {
        let (job, answer) = ui::finished(&mut self.ui, done);
        match answer {
            UiAnswer::Tool(outcome) => {
                // 時間切れで答えたあとに届いたものは捨てる。
                let Some(UiJob { pending, .. }) = self.ui_jobs.remove(&job) else { return Ok(()) };
                self.finish(pending, outcome, backend)
            }
            UiAnswer::Poll(outcome) => {
                let Some(index) = self.waiters.iter().position(|w| w.poll_job == Some(job)) else { return Ok(()) };
                match outcome {
                    Ok(None) => {
                        let waiter = &mut self.waiters[index];
                        waiter.poll_job = None;
                        waiter.poll_at = Instant::now() + ui::POLL_INTERVAL;
                    }
                    answer => {
                        let waiter = self.waiters.remove(index);
                        let answer = result(&waiter.pending.id, text_result(answer.map(Option::unwrap_or_default)));
                        waiter.pending.answer(answer, backend);
                    }
                }
                Ok(())
            }
        }
    }

    /// ツールの答え: すぐ答えるか、待ちを登録するか、GUI のスレッドの終わりを待つか。
    fn finish(&mut self, pending: Pending, outcome: Outcome, backend: &mut dyn Backend) -> io::Result<()> {
        let kind = match outcome {
            Outcome::Done(outcome) => {
                let answer = result(&pending.id, text_result(outcome));
                pending.answer(answer, backend);
                return Ok(());
            }
            Outcome::Content(content) => {
                let answer = result(&pending.id, content);
                pending.answer(answer, backend);
                return Ok(());
            }
            Outcome::Background(job, timeout) => {
                self.ui_jobs.insert(job, UiJob { pending, deadline: Instant::now() + timeout.unwrap_or(UI_JOB_TIMEOUT) });
                return Ok(());
            }
            Outcome::WaitForStop(wait) => (wait, WaitKind::Stop),
            Outcome::AfterAction(wait, action) => (wait, WaitKind::Action(action)),
            Outcome::WaitForUi(wait, query) => (wait, WaitKind::Ui(query)),
        };
        let now = Instant::now();
        self.waiters.push(Waiter { pending, deadline: now + kind.0, kind: kind.1, poll_at: now + ui::POLL_INTERVAL, poll_job: None });
        // すでに止まっていれば、すぐ答える。
        self.answer_waiters(backend)
    }

    /// 止まったか終わったなら、待っている呼び出しに答える。
    fn answer_waiters(&mut self, backend: &mut dyn Backend) -> io::Result<()> {
        if self.waiters.is_empty() || matches!(self.state.target, Target::Running | Target::NotStarted) {
            return Ok(());
        }
        let status = context!(self, backend).status()?;
        for waiter in std::mem::take(&mut self.waiters) {
            let answer = match waiter.kind {
                WaitKind::Stop => status.clone(),
                // 操作の途中でブレークポイントに当たった（AI に、止まった場所を知らせる）。
                WaitKind::Action(mut done) => {
                    done["stopped"] = json!(true);
                    done["status"] = status.clone();
                    done
                }
                WaitKind::Ui(_) => json!({ "found": false, "stopped": true, "status": status }),
            };
            let answer = result(&waiter.pending.id, text_result(Ok(answer)));
            waiter.pending.answer(answer, backend);
        }
        Ok(())
    }
}

/// ツールの結果を、MCP の tools/call の答えの形にする。
fn text_result(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(value) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }] }),
        Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
    }
}

/// initialize の答えに付ける、AI 向けの使い方。
const INSTRUCTIONS: &str = "Debugger for .NET and .NET Framework programs (netdbg). Typical flow: breakpoint_set, \
debug_launch (or use the session the IDE started), debug_wait_for_stop, then stack_trace / variables / evaluate, then \
debug_step or debug_continue. Tools that need a stopped program say so when it runs. Expressions are C#. \
After editing and saving the program's C# source, call debug_apply_changes to apply the edit without restarting (Edit and Continue). \
Assemblies without a PDB are decompiled to C# files by the debugger: their frames point at those files, and decompile lists or \
reads them (module, type, method) so you can set breakpoints there. sql_log records the SQL the program runs (with \
parameter values filled in) and exception_log every exception thrown, also caught ones (with the stack and locals at the \
throw), and event_trace which event handlers ran and from which control, all without stopping the program: turn them on \
before reproducing. \
debug_open_dump opens a crash dump (.dmp) as a stopped program to read its stacks, variables, exception and heap. \
A UI thread that stops answering for 2 s pauses the program with the reason \"UI not responding\" (ui_freeze changes that); \
process_usage shows memory, GDI / USER objects, handles and the 32-bit address space. \
For a WinForms or WPF program: ui_tree (or ui_screenshot with annotate) shows the controls with mark numbers; ui_invoke, ui_set_text, \
ui_select, ui_toggle, ui_keys and ui_mouse act on them and report where the program stopped if an event handler hits a breakpoint. \
ui_screenshot also works while stopped. Coordinates are screen pixels; ui_mouse relativeTo window uses the pixels of a window screenshot.";

#[cfg(test)]
mod tests {
    use super::*;

    /// 本体の代わり: 要求には失敗で答え、知らせを覚える。
    #[derive(Default)]
    struct Fake {
        notified: Vec<(String, Value)>,
    }

    impl Backend for Fake {
        fn request(&mut self, command: &str, _arguments: Value) -> io::Result<(Result<Value, String>, Vec<Event>)> {
            Ok((Err(format!("{command}: no target")), Vec::new()))
        }
        fn restart(&mut self) {}
        fn notify(&mut self, name: &str, body: Value) {
            self.notified.push((name.to_string(), body));
        }
    }

    #[test]
    fn a_tool_call_is_announced_when_it_starts_and_ends() {
        let (mut server, mut backend) = (Server::new(Mode::Listening), Fake::default());
        let (reply, replies) = std::sync::mpsc::channel();
        let long = "x".repeat(ACTION_TEXT + 50);
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "evaluate", "arguments": { "expression": long } } });
        server.call(Call { message, reply }, &mut backend).unwrap();
        assert!(replies.try_recv().unwrap().is_some());
        let [(started_name, started), (_, finished)] = &backend.notified[..] else { panic!("{:?}", backend.notified) };
        assert_eq!(started_name, "netdbg.agentAction");
        assert_eq!((started["state"].as_str(), started["tool"].as_str(), started["id"].as_u64()), (Some("started"), Some("evaluate"), Some(1)));
        assert_eq!(started["arguments"]["expression"].as_str().map(|s| s.chars().count()), Some(ACTION_TEXT + 1), "long values are shortened");
        assert_eq!((finished["state"].as_str(), finished["id"].as_u64()), (Some("finished"), Some(1)));
        assert!(finished["ms"].is_u64() && finished["ok"].is_boolean());
    }

    #[test]
    fn other_methods_are_not_actions() {
        let (mut server, mut backend) = (Server::new(Mode::Listening), Fake::default());
        let (reply, _replies) = std::sync::mpsc::channel();
        server.call(Call { message: json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }), reply }, &mut backend).unwrap();
        assert!(backend.notified.is_empty());
    }
}