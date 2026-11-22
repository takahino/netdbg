//! 入力のループの前段。入力を本体（`Engine`）と各 IF（DAP、MCP）に振り分け、本体の出来事を両方の IF に伝える。
//!
//! - DAP のクライアント（VS Code）の要求: 本体に渡し、答えを DAP で書く。出来事は MCP の側も見る（VS Code の操作で
//!   止まったことを、AI の待ちにも伝える）。
//! - MCP の呼び出し: `mcp::Server` が処理する。本体は `Link`（`mcp::Backend`）越しに使い、そのとき起きた出来事は
//!   DAP のクライアントにも書く（VS Code の画面が AI の操作についてくる）。
//! - 対象の出来事（ICorDebug のコールバック、出力）: 本体に渡し、出来事を両方の IF に伝える。

use super::dap::DapClient;
use super::mcp::{self, Backend};
use super::changes::{self, TreeChanges};
use super::report::{self, TestRecorder};
use super::ClientInput;
use crate::debugger::engine::{Engine, Flow, Reply};
use crate::debugger::session::{Event, Owner};
use crate::events::Input;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::time::Instant;

pub struct Hub<W: Write> {
    engine: Engine,
    /// DAP のクライアント（VS Code）。`--mcp` では無い。
    dap: Option<DapClient<W>>,
    /// MCP のサーバー。`--mcp` と `--mcp-listen` のときだけ。
    mcp: Option<mcp::Server>,
    /// MCP からの要求の `seq`（-1 から減らしていく。DAP のクライアントの要求と重ならない。取り消しの照合に使う）。
    next_seq: i64,
    /// `--mcp-listen` の接続先（URL とトークン）。DAP のクライアントの initialize に答えたあと、`netdbg.mcpEndpoint`
    /// で知らせる。
    endpoint: Option<(String, String)>,
    /// セッションが終わっても、エンジンを終えずに次のセッションを待つ（専用の画面。終えるのは画面の「終了」）。
    keep_alive: bool,
    /// 成績書の記録（B3。エンジンを作り直しても残す）。
    tests: TestRecorder,
    /// 停止のあいだに変わったコントロール（D-087）。
    changes: TreeChanges,
}

/// MCP から本体を使う口。MCP の操作で起きた出来事は、DAP のクライアントにも書く。
struct Link<'a, W: Write> {
    engine: &'a mut Engine,
    dap: Option<&'a mut DapClient<W>>,
    next_seq: &'a mut i64,
    tests: &'a mut TestRecorder,
    changes: &'a TreeChanges,
}

impl<W: Write> Backend for Link<'_, W> {
    fn request(&mut self, command: &str, arguments: Value) -> io::Result<(Result<Value, String>, Vec<Event>)> {
        // 成績書の要求は、ハブの記録が受ける（エンジンには渡さない）。
        if report::handles(command) {
            let (body, events) = self.tests.request(self.engine, command, &arguments);
            if let Some(dap) = self.dap.as_deref_mut() {
                dap.events(&events)?;
            }
            return Ok((body, events));
        }
        if changes::handles(command) {
            return Ok((self.changes.request(), Vec::new()));
        }
        let seq = *self.next_seq;
        *self.next_seq -= 1;
        let reply = self.engine.request(seq, command, &arguments, Owner::Agent);
        if let Some(dap) = self.dap.as_deref_mut() {
            if reply.changed_values {
                dap.values_changed()?;
            }
            dap.events(&reply.events)?;
        }
        Ok((reply.body, reply.events))
    }

    fn restart(&mut self) {
        self.tests.before_engine_replaced(self.engine);
        *self.engine = Engine::new();
    }

    fn notify(&mut self, name: &str, body: Value) {
        // AI の操作は、開いている成績書の項目にも足す。
        let changed = if name == "netdbg.agentAction" { self.tests.agent_action(self.engine, &body) } else { None };
        if let Some(dap) = self.dap.as_deref_mut() {
            for (name, body) in std::iter::once((name, body)).chain(changed) {
                if let Err(e) = dap.event(name, body) {
                    crate::log!("cannot write {name}: {e}");
                }
            }
        }
    }
}

impl<W: Write> Hub<W> {
    pub fn new(dap: Option<DapClient<W>>, mcp: Option<mcp::Server>) -> Self {
        Hub { engine: Engine::new(), dap, mcp, next_seq: -1, endpoint: None, keep_alive: false, tests: TestRecorder::default(), changes: TreeChanges::default() }
    }

    /// `--mcp-listen` の接続先を覚えておき、DAP のクライアント（拡張）に `netdbg.mcpEndpoint` で知らせる。
    pub fn announce(&mut self, url: &str, token: &str) {
        self.endpoint = Some((url.to_string(), token.to_string()));
    }

    /// セッションが終わっても、エンジンを終えない（専用の画面）。
    pub fn keep_alive(&mut self) {
        self.keep_alive = true;
    }

    /// セッションが終わった（`disconnect`、対象の終わり）。エンジンを終えないなら、新しいセッションにして true。
    pub fn restart_if_kept(&mut self) -> bool {
        if self.keep_alive {
            crate::log!("session ended; waiting for the next one");
            self.replace_engine();
        }
        self.keep_alive
    }

    /// エンジンを作り直す（専用の画面の次のセッション）。成績書の開いている項目に、前のエンジンの記録を先に移す。
    fn replace_engine(&mut self) {
        self.tests.before_engine_replaced(&mut self.engine);
        self.engine = Engine::new();
    }

    /// 止めた対象の終わりを、まだ待つか（入力のループの期限を残すか）。エンジンを終えないときは、`disconnect` の答えを
    /// 保留している間だけ（終わって次のセッションになったら、前の期限で新しいセッションを捨てないように）。
    pub fn waits_for_exit(&self) -> bool {
        !self.keep_alive || self.dap.as_ref().is_some_and(DapClient::is_disconnecting)
    }

    /// 入力を 1 つ処理する。
    pub fn handle(&mut self, input: Input) -> io::Result<Flow> {
        let flow = self.dispatch(input)?;
        self.with_mcp(|mcp, link| mcp.settle(link))?;
        self.engine.settle();
        // 成績書の項目の間、いまの対象のプロセスの手の操作を記録する（起動し直したら付け替える）。
        self.tests.follow_target(&self.engine);
        // 画面の木を見続け、止まった瞬間に、前の停止からの変化を知らせる（D-087）。
        if let (Some(event), Some(dap)) = (self.changes.follow(&self.engine), &mut self.dap) {
            dap.events(&[event])?;
        }
        // 入力が続いて期限切れの処理（`expire`）が回らなくても、操作のあとの画面を写す。
        self.capture_due()?;
        if flow == Flow::Exit && self.restart_if_kept() {
            return Ok(Flow::Continue);
        }
        Ok(flow)
    }

    fn dispatch(&mut self, input: Input) -> io::Result<Flow> {
        let flow = match input {
            Input::Client(ClientInput::Dap(request)) => {
                // エンジンを終えない形（専用の画面）では、対象が自分で終わったあとも前のセッションが残っている。次の
                // セッションの始まり（initialize）で、新しいセッションにする（残ったままだと、起動しても何も始まらない）。
                if self.keep_alive && request.command == "initialize" && !self.engine.is_live() {
                    self.replace_engine();
                }
                // 成績書の要求は、ハブの記録が受ける（エンジンには渡さない）。
                let reply = if report::handles(&request.command) {
                    let (body, events) = self.tests.request(&mut self.engine, &request.command, &request.arguments);
                    Reply { events, ..Reply::new(body) }
                } else if changes::handles(&request.command) {
                    Reply::new(self.changes.request())
                } else {
                    self.engine.request(request.seq, &request.command, &request.arguments, Owner::Client)
                };
                if let Some(dap) = &mut self.dap {
                    dap.answer(&request, &reply)?;
                    if request.command == "initialize" && reply.body.is_ok() {
                        if let Some((url, token)) = &self.endpoint {
                            dap.event("netdbg.mcpEndpoint", json!({ "url": url, "token": token, "enginePid": std::process::id() }))?;
                        }
                    }
                }
                if let Some(mcp) = &mut self.mcp {
                    mcp.observe(Some((&request.command, reply.body.is_ok())), &reply.events);
                }
                reply.flow
            }
            Input::Client(ClientInput::Closed(error)) => {
                if let Some(error) = error {
                    crate::log!("input error: {error}");
                }
                self.engine.client_closed()
            }
            Input::Client(ClientInput::Mcp(call)) => {
                self.with_mcp(|mcp, link| mcp.call(call, link))?;
                Flow::Continue
            }
            Input::Client(ClientInput::Quit) => {
                self.keep_alive = false;
                self.engine.client_closed()
            }
            Input::Client(ClientInput::Ui(done)) => {
                self.with_mcp(|mcp, link| mcp.ui_finished(*done, link))?;
                Flow::Continue
            }
            Input::RuntimeStarted(result) => {
                let events = self.engine.runtime_started(result);
                self.publish(&events)?;
                Flow::Continue
            }
            Input::TargetExited { process_id, exit_code } => {
                let events = self.engine.target_exited(process_id, exit_code);
                self.publish(&events)?;
                self.after_exit()?
            }
            Input::Debug(event) => {
                let events = self.engine.debug_event(event);
                self.publish(&events)?;
                self.after_exit()?
            }
            Input::Output { category, text } => {
                let events = self.engine.output(category, text);
                self.publish(&events)?;
                Flow::Continue
            }
            Input::UiFrozen { process_id, thread, ms, generation } => {
                let events = self.engine.ui_frozen(process_id, thread, ms, generation);
                self.publish(&events)?;
                Flow::Continue
            }
            Input::CoverageTick => {
                let events = self.engine.coverage_tick();
                self.publish(&events)?;
                Flow::Continue
            }
            Input::HandOperation(recorded) => {
                if let (Some(event), Some(dap)) = (self.tests.hand_operation(&self.engine, recorded), &mut self.dap) {
                    dap.events(&[event])?;
                }
                Flow::Continue
            }
        };
        Ok(flow)
    }

    /// 待っている呼び出しのうち、いちばん早い期限。入力のループは、これまでに入力が無ければ `expire` を呼ぶ。
    pub fn deadline(&self) -> Option<Instant> {
        [self.mcp.as_ref().and_then(mcp::Server::deadline), self.tests.deadline()].into_iter().flatten().min()
    }

    pub fn expire(&mut self) -> io::Result<()> {
        self.with_mcp(|mcp, link| mcp.expire(link))?;
        self.capture_due()
    }

    /// 成績書の、時刻の来た「操作のあとの画面」を写し、画面の一覧を更新する。
    fn capture_due(&mut self) -> io::Result<()> {
        if let (Some(event), Some(dap)) = (self.tests.capture_due(&self.engine), &mut self.dap) {
            dap.events(&[event])?;
        }
        Ok(())
    }

    /// `disconnect` で待っていた対象の終了が、時間内に来なかったとき。
    pub fn finish_pending_disconnect(&mut self) -> io::Result<()> {
        if let Some(dap) = &mut self.dap {
            dap.finish_disconnect()?;
        }
        Ok(())
    }

    /// 対象の出来事を、両方の IF に伝える。
    fn publish(&mut self, events: &[Event]) -> io::Result<()> {
        if let Some(dap) = &mut self.dap {
            dap.events(events)?;
        }
        if let Some(mcp) = &mut self.mcp {
            mcp.observe(None, events);
        }
        Ok(())
    }

    /// 対象が終わったなら、`disconnect` で保留していた答えを返して終わる。
    fn after_exit(&mut self) -> io::Result<Flow> {
        if !self.engine.is_live() {
            if let Some(dap) = &mut self.dap {
                if dap.finish_disconnect()? {
                    return Ok(Flow::Exit);
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// MCP のサーバーに、本体を使う口を渡して処理させる。
    fn with_mcp(&mut self, work: impl FnOnce(&mut mcp::Server, &mut dyn Backend) -> io::Result<()>) -> io::Result<()> {
        let Hub { engine, dap, mcp, next_seq, tests, changes, .. } = self;
        let Some(mcp) = mcp else { return Ok(()) };
        let mut link = Link { engine, dap: dap.as_mut(), next_seq, tests, changes };
        work(mcp, &mut link)
    }
}
