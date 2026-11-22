//! IF の層: デバッガーの本体（`crate::debugger`）を外から使う口。相手ごとに縦に分ける。
//!
//! - `dap`: VS Code（DAP のクライアント）との連携。
//! - `mcp`: AI のクライアントとの連携（MCP）。
//! - `http`: 127.0.0.1 で待ち受ける HTTP の最小限（MCP の `--mcp-listen` と、専用の画面の `--ui`）。
//! - `ui`: 専用の画面（ブラウザ。`--ui`）。hub から見ると、DAP のクライアントの 1 つ。
//! - `hub`: 入力のループの前段。入力を本体と各 IF に振り分け、本体の出来事を両方の IF に伝える。
//!
//! IF どうしは互いを知らない。MCP の操作で起きた出来事を VS Code にも伝える（VS Code の画面が AI の操作についてくる）
//! のは、`hub` の役目。

pub mod changes;
pub mod dap;
pub mod http;
pub mod hub;
pub mod mcp;
pub mod report;
pub mod ui;

use dap::protocol::Request;
use serde_json::Value;

/// IF の層から、入力のループに届くもの。
pub enum ClientInput {
    /// DAP の要求（VS Code から）。
    Dap(Request),
    /// 要求を送ってくる相手（DAP の標準入力、`--mcp` の標準入力）が閉じた、または読めなくなった。
    Closed(Option<String>),
    /// MCP のメッセージ（`mcp::stdio`、`mcp::http`）。
    Mcp(mcp::Call),
    /// MCP の GUI のツールの処理が、GUI のスレッドで終わった（番号や名前の写しを含んで大きいので、箱に入れて運ぶ）。
    Ui(Box<mcp::UiDone>),
    /// 専用の画面の「終了」（`POST /quit`）。対象を止めて、エンジンを終える。
    Quit,
}

impl ClientInput {
    /// 要求 `seq` を取り消す入力か（DAP の `cancel`）。本体が、関数評価の途中で見る。
    pub fn cancels(&self, seq: i64) -> bool {
        matches!(self, ClientInput::Dap(request) if request.command == "cancel" && request.arguments.get("requestId").and_then(Value::as_i64) == Some(seq))
    }
}

/// 入力の列に積む（どのスレッドからでも）。
pub fn post(input: ClientInput) {
    crate::events::post(crate::events::Input::Client(input));
}
