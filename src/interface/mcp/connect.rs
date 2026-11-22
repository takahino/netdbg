//! `--mcp-connect`: 外の AI のクライアント（Claude Code、GitHub Copilot CLI、Cursor など）から、VS Code（または Cursor）
//! の下で動いているエンジン（`--mcp-listen`）への橋渡し。標準入出力の MCP（1 行に 1 つ）を、エンジンの HTTP に渡す。
//!
//! AI のクライアントは、デバッグを始める前にこれを起動することが多い。そこで、つなぐ先はメッセージごとに探す
//! （接続先のファイルのうち、動いているエンジンの、いちばん新しいもの）。エンジンが無い間は、initialize と tools/list
//! には自分で答え（ツールの一覧は同じもの）、ツールの呼び出しには「デバッグ中のエンジンがありません」と答える。

use super::{error, http::endpoint_directory, result, tools, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

/// つなぐ先（`http.rs` が書いた接続先のファイルの中身）。
#[derive(Clone, Debug, PartialEq)]
struct Target {
    url: String,
    token: String,
    engine_pid: u32,
}

/// 橋渡しを動かす。`engine` は、つなぐエンジンの PID（`--mcp-connect=<PID>`）。指定が無ければ、いちばん新しいもの。
pub fn run(engine: Option<u32>) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(e) => {
                writeln!(stdout, "{}", error(&Value::Null, super::PARSE_ERROR, &tr!("Cannot read it as JSON: {error}", error = e)))?;
                stdout.flush()?;
                continue;
            }
        };
        if let Some(answer) = forward(&message, engine) {
            writeln!(stdout, "{answer}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// メッセージ 1 つを、つなぐ先のエンジンに渡す。エンジンが無ければ、ここで答える。通知なら `None`。
fn forward(message: &Value, engine: Option<u32>) -> Option<Value> {
    let id = message.get("id").cloned();
    if let Some(target) = find_target(engine) {
        match post(&target, message) {
            Ok(answer) => return answer,
            Err(e) => {
                // つなげなかった（エンジンがちょうど終わった、など）。下で、エンジンが無いときと同じに答える。
                eprintln!("netdbg --mcp-connect: {e}");
            }
        }
    }
    let id = id?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    Some(match method {
        "initialize" => result(
            &id,
            json!({
                "protocolVersion": message.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "netdbg", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Debugger for .NET programs running under VS Code or Cursor (netdbg with netdbg.mcp.enabled). Start debugging in the editor first; then the tools control that session.",
            }),
        ),
        "ping" => result(&id, json!({})),
        "tools/list" => result(&id, json!({ "tools": tools::list(false) })),
        "tools/call" => result(
            &id,
            json!({
                "content": [{ "type": "text", "text": tr!("No engine is debugging. Start debugging with netdbg in VS Code (or Cursor), with the setting netdbg.mcp.enabled on.") }],
                "isError": true,
            }),
        ),
        other => error(&id, super::METHOD_NOT_FOUND, &tr!("Unknown method: {method}", method = other)),
    })
}

/// 動いているエンジンの接続先。`engine` が指定されていれば、その PID のもの。
fn find_target(engine: Option<u32>) -> Option<Target> {
    let directory = endpoint_directory()?;
    let mut candidates: Vec<(std::time::SystemTime, Target)> = Vec::new();
    for entry in std::fs::read_dir(&directory).ok()?.flatten() {
        let path: PathBuf = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(target) = std::fs::read_to_string(&path).ok().and_then(|text| parse_target(&text)) else { continue };
        if engine.is_some_and(|pid| pid != target.engine_pid) {
            continue;
        }
        // 落ちたエンジンの残したファイルは、使わずに消す。
        if !process_alive(target.engine_pid) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        candidates.push((modified, target));
    }
    candidates.into_iter().max_by_key(|(modified, _)| *modified).map(|(_, target)| target)
}

fn parse_target(text: &str) -> Option<Target> {
    let value: Value = serde_json::from_str(text).ok()?;
    Some(Target {
        url: value.get("url")?.as_str()?.to_string(),
        token: value.get("token")?.as_str()?.to_string(),
        engine_pid: value.get("enginePid")?.as_u64()? as u32,
    })
}

fn process_alive(process_id: u32) -> bool {
    const STILL_ACTIVE: u32 = 259;
    let Some(process) = crate::win32::open_process_for_query(process_id) else { return false };
    let mut code = 0u32;
    unsafe { crate::win32::GetExitCodeProcess(process.raw(), &mut code) != 0 && code == STILL_ACTIVE }
}

/// エンジンに POST する。答えの本文（通知なら `None`）。
fn post(target: &Target, message: &Value) -> std::io::Result<Option<Value>> {
    let address = target.url.trim_start_matches("http://").split('/').next().unwrap_or("").to_string();
    let mut stream = TcpStream::connect(&address)?;
    stream.set_read_timeout(Some(Duration::from_millis(tools::MAX_WAIT_MS + 120_000)))?;
    let body = message.to_string();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        target.token,
        body.len()
    )?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    let code: u16 = status.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| std::io::Error::other(tr!("Not an HTTP response: {status}", status = format!("{status:?}"))))?;
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    match code {
        202 => Ok(None),
        200 => serde_json::from_slice(&body).map(Some).map_err(|e| std::io::Error::other(tr!("The response is not JSON: {error}", error = e))),
        _ => Err(std::io::Error::other(tr!("The engine answered {code} ({status})", code = code, status = status.trim()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_file_is_read() {
        let target = parse_target(r#"{"url":"http://127.0.0.1:5000/mcp","token":"abc","enginePid":42,"bits":32}"#).unwrap();
        assert_eq!(target, Target { url: "http://127.0.0.1:5000/mcp".into(), token: "abc".into(), engine_pid: 42 });
        assert!(parse_target("{}").is_none());
    }

    #[test]
    fn without_an_engine_the_bridge_answers_initialize_and_refuses_tool_calls() {
        // PID 0 のエンジンは無いので、つなぐ先は見つからない。
        let init = forward(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), Some(0)).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let list = forward(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), Some(0)).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "evaluate"));
        let call = forward(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "debug_status" } }), Some(0)).unwrap();
        assert_eq!(call["result"]["isError"], true);
        assert!(forward(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), Some(0)).is_none());
    }
}
