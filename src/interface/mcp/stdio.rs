//! `--mcp`: 標準入出力で MCP を話す（1 行に 1 つの JSON-RPC のメッセージ）。

use super::{error, Call, PARSE_ERROR};
use crate::interface::{post, ClientInput};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, Sender};

/// メッセージを 1 行ずつ読み、入力の列に積む。入力が閉じたら、そう知らせる。
pub fn read_lines(input: impl BufRead, reply: Sender<Option<Value>>) {
    for line in input.lines() {
        let Ok(line) = line else { break };
        // .NET Framework の Process クラスが標準入力の先頭に書く BOM（dap::protocol と同じ）。
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        crate::log!("mcp -> {line}");
        match serde_json::from_str::<Value>(line) {
            Ok(message) => post(ClientInput::Mcp(Call { message, reply: reply.clone() })),
            Err(e) => {
                let _ = reply.send(Some(error(&Value::Null, PARSE_ERROR, &tr!("Cannot read it as JSON: {error}", error = e))));
            }
        }
    }
    post(ClientInput::Closed(None));
}

/// 答えを 1 行ずつ書く。
pub fn write_lines(mut output: impl Write, replies: Receiver<Option<Value>>) {
    for reply in replies {
        let Some(reply) = reply else { continue };
        let text = reply.to_string();
        crate::log!("mcp <- {}", if text.len() > 2000 { &text[..text.floor_char_boundary(2000)] } else { &text });
        if writeln!(output, "{text}").and_then(|()| output.flush()).is_err() {
            return;
        }
    }
}
