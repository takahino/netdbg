//! MCP（Model Context Protocol）の口。AI のクライアントが、ツールでデバッグを操作する（D-053）。
//!
//! 使い方は 3 つ（main.rs の引数）:
//! - `--mcp`: 標準入出力で MCP を話す（`stdio`）。VS Code 無しで、AI のクライアントがエンジンを直接起動する。
//! - `--mcp-listen`: DAP のまま（VS Code の下）、加えて 127.0.0.1 で MCP を開く（`http`）。
//! - `--mcp-connect`: `--mcp-listen` のエンジンへの橋渡し（`connect`）。標準入出力の MCP を HTTP に渡す。
//!
//! どの口から来た呼び出しも、入力の列（`ClientInput::Mcp`）に積み、入力のループのスレッドで `Server` が処理する
//! （本体に触るのはループのスレッドだけ、の約束）。ツールは機能ごとに `tools` の下に縦に分け、それぞれのモジュールが
//! 定義（名前、説明、入力の形）と処理を持つ。本体は `Backend` 越しに使い、DAP（VS Code）のことは知らない。

pub mod connect;
pub mod http;
mod server;
mod state;
pub mod stdio;
mod tools;

pub use server::{Mode, Server};
pub use tools::ui::UiDone;

use crate::debugger::session::Event;
use serde_json::{json, Value};
use std::io;
use std::sync::mpsc::Sender;

/// MCP の版。クライアントが送ってきた版が分からなければ、これで答える。
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// 届いた MCP のメッセージ 1 つと、答えの送り先（通知には答えないので `None` を送る）。
pub struct Call {
    pub message: Value,
    pub reply: Sender<Option<Value>>,
}

/// MCP から見た本体。`interface::hub` が実装し、MCP の操作で起きた出来事を VS Code にも伝える。
pub trait Backend {
    /// 要求（コマンドと引数。語彙は DAP のもの）を本体に渡し、答えの本文（失敗なら理由）と、そのとき起きた出来事を返す。
    fn request(&mut self, command: &str, arguments: Value) -> io::Result<(Result<Value, String>, Vec<Event>)>;
    /// 終わったセッションを捨て、新しいセッションにする（`--mcp` で、終わったあとにまた起動するとき）。
    fn restart(&mut self);
    /// MCP の側の出来事を、ほかの IF に知らせる（AI の操作の履歴 `netdbg.agentAction`。専用の画面が表示する）。
    fn notify(&mut self, _name: &str, _body: Value) {}
}

/// JSON-RPC の成功の答え。
pub fn result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// JSON-RPC の失敗の答え。
pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const PARSE_ERROR: i64 = -32700;
