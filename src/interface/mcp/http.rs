//! `--mcp-listen`: 127.0.0.1 の空いたポートで、MCP の Streamable HTTP の最小限を開く。
//!
//! - `POST /mcp` に JSON-RPC のメッセージを 1 つ。要求には JSON で答え（SSE は使わない）、通知には 202 で答える。
//! - `Authorization: Bearer <トークン>` が要る。トークンは起動ごとに作り、接続先のファイル（`endpoint_file`）にだけ書く。
//! - `Host` は、この待ち受けそのもの（127.0.0.1 か localhost と、この番号）だけを受ける。`Origin` が付いていれば、
//!   localhost のものだけを受ける（ブラウザーからの DNS rebinding を防ぐ。MCP の仕様の勧め）。
//!
//! 呼び出しは入力の列（`ClientInput::Mcp`）に積み、答えを待って返す。待つツールは最長 `tools::MAX_WAIT_MS` 待つ。
//! HTTP の読み書きは `interface::http`。専用の画面（`--ui`）は、同じ待ち受けの `/mcp` に、ここの `answer` をつなぐ。

use super::Call;
use crate::interface::http::{self, Connection, HttpRequest};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// 答えを待つ時間の上限（待つツールの上限より長く）。
const REPLY_TIMEOUT: Duration = Duration::from_millis(super::tools::MAX_WAIT_MS + 60_000);

/// 開いた口。`url` と `token` を、拡張と `--mcp-connect` に知らせる。
pub struct Endpoint {
    pub url: String,
    pub token: String,
    /// 接続先のファイル。終わるときに消す。
    pub file: Option<PathBuf>,
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            let _ = std::fs::remove_file(file);
        }
    }
}

impl Endpoint {
    /// 待ち受けの番号 `port` の MCP の口として、接続先のファイルを書く。`extra` は、ファイルに足す項目（画面の URL など）。
    pub fn publish(port: u16, token: String, extra: Value) -> Endpoint {
        let url = format!("http://127.0.0.1:{port}/mcp");
        let file = write_endpoint_file(&url, &token, extra).map_err(|e| crate::log!("mcp: cannot write the endpoint file: {e}")).ok();
        crate::log!("mcp listening on {url}");
        Endpoint { url, token, file }
    }
}

/// 待ち受けを始め、受けたものを入力の列に積むスレッドを起こす。
pub fn listen() -> std::io::Result<Endpoint> {
    let (listener, port) = http::bind()?;
    let token = http::random_token()?;
    let accepted = token.clone();
    http::serve(listener, "mcp", Arc::new(move |request: &HttpRequest, stream: &mut TcpStream| serve(request, stream, &accepted, port)));
    Ok(Endpoint::publish(port, token, Value::Null))
}

/// 要求 1 つに答える（`/mcp` の道）。
pub fn serve(request: &HttpRequest, stream: &mut TcpStream, token: &str, port: u16) -> std::io::Result<Connection> {
    let (status, body) = answer(request, token, port);
    http::respond_json(stream, status, body.as_deref())?;
    Ok(Connection::KeepAlive)
}

/// 接続先のファイルを置くフォルダー（`%LOCALAPPDATA%\netdbg\mcp`）。
pub fn endpoint_directory() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("netdbg").join("mcp"))
}

fn write_endpoint_file(url: &str, token: &str, extra: Value) -> std::io::Result<PathBuf> {
    let directory = endpoint_directory().ok_or_else(|| std::io::Error::other(tr!("LOCALAPPDATA is not set")))?;
    std::fs::create_dir_all(&directory)?;
    let file = directory.join(format!("{}.json", std::process::id()));
    let mut content = json!({ "url": url, "token": token, "enginePid": std::process::id(), "bits": usize::BITS });
    if let Value::Object(extra) = extra {
        content.as_object_mut().expect("object").extend(extra);
    }
    std::fs::write(&file, content.to_string())?;
    Ok(file)
}

/// 要求 1 つへの答え（状態と本文）。
fn answer(request: &HttpRequest, token: &str, port: u16) -> (&'static str, Option<String>) {
    if request.path != "/mcp" {
        return ("404 Not Found", None);
    }
    if !http::local_host(request, port) {
        return ("403 Forbidden", None);
    }
    if let Some(origin) = request.header("origin") {
        if !http::allowed_origin(origin) {
            return ("403 Forbidden", None);
        }
    }
    if !request.has_bearer(token) {
        return ("401 Unauthorized", None);
    }
    if request.method == "DELETE" {
        // セッションの終わり。状態は持たないので、何もしない。
        return ("200 OK", None);
    }
    if request.method != "POST" {
        // サーバーから送り出す流れ（GET の SSE）は持たない。
        return ("405 Method Not Allowed", None);
    }
    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(e) => return ("400 Bad Request", Some(super::error(&Value::Null, super::PARSE_ERROR, &tr!("Cannot read it as JSON: {error}", error = e)).to_string())),
    };
    crate::log!("mcp http -> {message}");
    let (reply, replies) = std::sync::mpsc::channel();
    crate::interface::post(crate::interface::ClientInput::Mcp(Call { message, reply }));
    match replies.recv_timeout(REPLY_TIMEOUT) {
        Ok(Some(answer)) => ("200 OK", Some(answer.to_string())),
        // 通知: 受け取ったとだけ答える。
        Ok(None) => ("202 Accepted", None),
        Err(_) => ("504 Gateway Timeout", None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_without_the_token_or_from_a_web_page_are_refused() {
        let request = |headers: Vec<(&str, &str)>| {
            let mut all = vec![("Host".to_string(), "127.0.0.1:7".to_string())];
            all.extend(headers.into_iter().map(|(n, v)| (n.to_string(), v.to_string())));
            HttpRequest { method: "POST".into(), path: "/mcp".into(), query: String::new(), headers: all, body: b"{}".to_vec() }
        };
        assert_eq!(answer(&request(vec![]), "t", 7).0, "401 Unauthorized");
        assert_eq!(answer(&request(vec![("Authorization", "Bearer wrong")]), "t", 7).0, "401 Unauthorized");
        assert_eq!(answer(&request(vec![("Authorization", "Bearer t"), ("Origin", "https://evil.example")]), "t", 7).0, "403 Forbidden");
        let mut other = request(vec![("Authorization", "Bearer t")]);
        other.headers[0].1 = "evil.example:7".into();
        assert_eq!(answer(&other, "t", 7).0, "403 Forbidden");
        other.headers[0].1 = "127.0.0.1:7".into();
        other.path = "/other".into();
        assert_eq!(answer(&other, "t", 7).0, "404 Not Found");
        other.path = "/mcp".into();
        other.method = "GET".into();
        assert_eq!(answer(&other, "t", 7).0, "405 Method Not Allowed");
    }
}
