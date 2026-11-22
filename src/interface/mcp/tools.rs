//! MCP のツール。機能ごとに縦に分け、それぞれのモジュールが定義（名前、説明、入力の JSON Schema。説明は AI の
//! クライアントが読む英語）と処理を持つ。ここは、その共通の部品と、一覧・呼び出しの振り分け。
//!
//! - `session`: 起動、アタッチ、状態、続行、一時停止、ステップ、止まるのを待つ、終える。
//! - `breakpoints`: ブレークポイント、トレースポイント、例外で止まる設定。
//! - `inspect`: スレッド、スタック、変数、式、代入、例外の情報、停止の文脈、ソース、モジュール。
//! - `output`: 対象の出力。
//! - `enc`: Edit and Continue。
//! - `decompile`: PDB の無いモジュールを逆コンパイルしたソース。
//! - `records`: 止めずに読む記録（SQL ログ、例外ログ、イベント トレース）。
//! - `monitor`: 対象の見張り（プロセスの使用量、UI のフリーズ）。
//! - `ui`: 対象のアプリの画面（WinForms、WPF）。
//! - `coverage`: 行と分岐のカバレッジ。
//! - `diff`: 差し替わった exe の差分と、再試験の範囲。
//! - `report`: 成績書（テスト項目の記録と、Excel / HTML）。

pub(super) mod breakpoints;
pub(super) mod coverage;
pub(super) mod diff;
pub(super) mod report;
pub(super) mod decompile;
pub(super) mod enc;
pub(super) mod inspect;
pub(super) mod monitor;
pub(super) mod output;
pub(super) mod records;
pub(super) mod session;
pub(super) mod ui;

use super::server::Mode;
use super::state::{State, Target};
use super::Backend;
use crate::debugger::session::Event;
use serde_json::{json, Value};
use std::io;
use std::time::Duration;

/// 待つツールの既定の待ち時間（ミリ秒）と上限。
pub const DEFAULT_WAIT_MS: u64 = 10_000;
pub const MAX_WAIT_MS: u64 = 300_000;

/// ツールの定義。
pub(super) fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// 入力の `waitMs` の形。
pub(super) fn wait_ms(what: &str) -> Value {
    json!({ "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS, "description": format!("Milliseconds to wait for {what}. 0 returns at once.") })
}

/// 入力の `threadId` と `frameIndex` の形。
pub(super) fn thread_schema() -> Value {
    json!({ "type": "integer", "description": "Thread id. Defaults to the thread that stopped." })
}

pub(super) fn frame_schema() -> Value {
    json!({ "type": "integer", "minimum": 0, "description": "Frame index in the call stack, 0 = top (default)." })
}

/// ツールの一覧。`standalone` は `--mcp`（エンジンを直接起動した）。起動とアタッチ、例外の設定は、そのときだけ出す
/// （VS Code の下では、VS Code が起動したセッションと、その設定を使う）。
pub fn list(standalone: bool) -> Vec<Value> {
    let mut tools = session::tools(standalone);
    tools.extend(breakpoints::tools(standalone));
    tools.extend(inspect::tools());
    tools.extend(output::tools());
    tools.extend(enc::tools());
    tools.extend(decompile::tools());
    tools.extend(records::tools());
    tools.extend(monitor::tools());
    tools.extend(ui::tools());
    tools.extend(coverage::tools());
    tools.extend(diff::tools());
    tools.extend(report::tools());
    tools
}

/// ツールの答え。
pub(super) enum Outcome {
    Done(Result<Value, String>),
    /// `tools/call` の答えの中身そのもの（画像）。
    Content(Value),
    /// 止まるか終わるまで待つ。
    WaitForStop(Duration),
    /// 画面を操作した。止まらないか少し待つ（止まらなければ、操作の結果を返す）。
    AfterAction(Duration, Value),
    /// 画面の条件を待つ（`ui_wait_for`）。
    WaitForUi(Duration, ui::UiQuery),
    /// GUI のスレッドで処理している（終わると `ClientInput::Ui` が届く）。待つ上限（無ければ既定の 15 秒）。
    Background(u64, Option<Duration>),
}

/// ツールを呼ぶ。知らない名前なら、そう答える。
pub(super) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Outcome> {
    if let Some(outcome) = session::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = breakpoints::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = inspect::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = enc::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = decompile::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = records::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = monitor::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = coverage::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = diff::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = report::call(ctx, name, arguments)? {
        return Ok(outcome);
    }
    if let Some(outcome) = output::call(ctx, name, arguments) {
        return Ok(outcome);
    }
    if let Some(outcome) = ui::call(ctx, name, arguments) {
        return Ok(outcome);
    }
    Ok(Outcome::Done(Err(tr!("Unknown tool: {name}", name = name))))
}

/// ツールの処理から見た、本体と MCP の側の状態。
pub(super) struct Context<'a> {
    pub backend: &'a mut dyn Backend,
    pub mode: Mode,
    pub state: &'a mut State,
    pub breakpoints: &'a mut breakpoints::Breakpoints,
    pub ui: &'a mut ui::UiMemory,
}

/// 本体の答えと出来事を、MCP の側の状態に映す（MCP の操作でも、DAP のクライアントの操作でも）。
pub(super) fn observe(state: &mut State, breakpoints: &mut breakpoints::Breakpoints, answered: Option<(&str, bool)>, events: &[Event]) {
    state.observe(answered, events);
    breakpoints.observe(events);
}

impl Context<'_> {
    /// 本体に要求を 1 つ渡し、答えの本文（失敗なら理由）を返す。起きた出来事で、状態を追う。
    pub fn request(&mut self, command: &str, arguments: Value) -> io::Result<Result<Value, String>> {
        let (body, events) = self.backend.request(command, arguments)?;
        observe(self.state, self.breakpoints, Some((command, body.is_ok())), &events);
        Ok(body)
    }

    /// 止まっているスレッド（指定が無ければ、止まったスレッド）。止まっていなければ理由。
    pub fn stopped_thread(&self, arguments: &Value) -> Result<i64, String> {
        let Target::Stopped(body) = &self.state.target else {
            return Err(tr!("{state}This works only while stopped (stop with debug_pause or a breakpoint first).", state = self.state.not_running()));
        };
        Ok(arguments.get("threadId").and_then(Value::as_i64).or_else(|| body.get("threadId").and_then(Value::as_i64)).unwrap_or(0))
    }

    /// フレームの番号（0 がいちばん上）から、本体のフレームの id。
    pub fn frame_id(&mut self, arguments: &Value) -> io::Result<Result<i64, String>> {
        let thread = match self.stopped_thread(arguments) {
            Ok(thread) => thread,
            Err(e) => return Ok(Err(e)),
        };
        let index = arguments.get("frameIndex").and_then(Value::as_u64).unwrap_or(0);
        Ok(self.request("stackTrace", json!({ "threadId": thread, "startFrame": index, "levels": 1 }))?.and_then(|trace| {
            trace.pointer("/stackFrames/0/id").and_then(Value::as_i64).ok_or_else(|| tr!("There is no frame {index}.", index = index))
        }))
    }

    /// いまの状態。止まっていれば、止まった場所（いちばん上のフレーム）も。
    pub fn status(&mut self) -> io::Result<Value> {
        let mut status = match &self.state.target {
            Target::NotStarted => json!({ "state": "notStarted" }),
            Target::Running => json!({ "state": "running" }),
            Target::Exited(code) => json!({ "state": "exited", "exitCode": code }),
            Target::Stopped(body) => {
                let body = body.clone();
                let thread = body.get("threadId").cloned().unwrap_or(Value::Null);
                // いちばん上のフレームにソースが無ければ（ランタイムの中で投げられた例外など）、ソースのある最初のフレーム
                // （自分のコード）も示す。Visual Studio が例外で止まったときに、自分のコードの行を示すのと同じ。
                let (top, user) = match self.request("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": 50 }))? {
                    Ok(trace) => {
                        let frames = trace.get("stackFrames").and_then(Value::as_array).cloned().unwrap_or_default();
                        let user = frames.iter().position(|f| f.pointer("/source/path").is_some()).filter(|&index| index > 0).map(|index| {
                            let mut summary = frame_summary(&frames[index]);
                            summary["index"] = json!(index);
                            summary
                        });
                        (frames.first().map(frame_summary).unwrap_or(Value::Null), user)
                    }
                    Err(e) => (json!(e), None),
                };
                let mut stopped = json!({ "state": "stopped", "reason": body.get("reason"), "threadId": thread, "at": top });
                if let Some(user) = user {
                    stopped["userFrame"] = user;
                }
                for key in ["description", "text"] {
                    if let Some(value) = body.get(key) {
                        stopped[key] = value.clone();
                    }
                }
                stopped
            }
        };
        if let Some(pid) = self.state.process_id {
            status["processId"] = json!(pid);
        }
        Ok(status)
    }

    /// 進めたあと: `waitMs` が 0 でなければ止まるまで待ち、0 ならいまの状態を返す。
    pub fn wait_or_status(&mut self, arguments: &Value, default_wait: u64) -> io::Result<Outcome> {
        let wait = arguments.get("waitMs").and_then(Value::as_u64).unwrap_or(default_wait).min(MAX_WAIT_MS);
        if wait == 0 {
            return Ok(Outcome::Done(Ok(self.status()?)));
        }
        Ok(Outcome::WaitForStop(Duration::from_millis(wait)))
    }
}

/// フレームの要約（関数、ファイル、行）。
pub(super) fn frame_summary(frame: &Value) -> Value {
    json!({ "function": frame.get("name"), "file": frame.pointer("/source/path"), "line": frame.get("line") })
}
