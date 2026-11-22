//! 画面の待ち受け: 127.0.0.1 の空いた番号で、画面のファイル、DAP（`POST /dap` と `GET /events`）、MCP（`/mcp`）を
//! 1 つの口で出す。
//!
//! 守り（D-062）:
//! - すべての道で、`Host` がこの待ち受けそのものであること（DNS の付け替えへの備え）。
//! - データを扱う道はトークンが要る。`fetch` は `Authorization: Bearer`、`EventSource` と最初のページは頭を付けられない
//!   ので `?token=`。画面のファイル（wasm、JS、CSS）はデータを持たないので、トークン無しで返す。
//! - ブラウザからの `POST` は、`Origin` が自分（`http://127.0.0.1:<port>`）と一致すること。CORS は許さない。

use super::sse::Broadcaster;
use super::{assets, files, processes, relaunch, windows};
use crate::interface::dap::protocol::Request;
use crate::interface::http::{self, Connection, HttpRequest};
use crate::interface::{mcp, post, ClientInput};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// `POST /dap` の答えを待つ上限（関数の評価やアタッチの長いものより長く）。
const REPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// `GET /events` で、何も無くても送る間隔（閉じた接続を見つけるため）。
const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// 画面が閉じたまま（対象も動いていない）この時間が過ぎたら、エンジンを終える。開き直し（再読み込み）の間は待つ。
const IDLE_EXIT: Duration = Duration::from_secs(30);

/// 開いた画面の口。
pub struct Started {
    /// 既定のブラウザで開く URL（トークンつき）。
    pub url: String,
    /// 同じ口の MCP（接続先のファイルを持つ。落とすと消える）。
    pub mcp: mcp::http::Endpoint,
}

struct Shared {
    token: String,
    port: u16,
    broadcaster: Arc<Broadcaster>,
    /// 起動の引数で渡された、画面が開いたら始めるもの（`GET /startup`。最初の 1 回だけ渡す。開き直したページが、
    /// もう一度起動しないように）。
    startup: std::sync::Mutex<Value>,
    /// 画面からの要求の `seq`（MCP の負の番号と重ならないように、1 から増やす）。
    next_seq: AtomicI64,
    /// いま開いている `GET /events` の数と、一度でも開いたか。
    pages: AtomicUsize,
    opened: AtomicBool,
}

/// 待ち受けを始める。画面へのメッセージは `broadcaster` から流す。`startup` は、画面が開いたら始めるもの。
pub fn start(broadcaster: Arc<Broadcaster>, startup: Value) -> io::Result<Started> {
    let (listener, port) = http::bind()?;
    let token = http::random_token()?;
    let shared = Arc::new(Shared { token: token.clone(), port, broadcaster, startup: std::sync::Mutex::new(startup), next_seq: AtomicI64::new(1), pages: AtomicUsize::new(0), opened: AtomicBool::new(false) });
    let watched = shared.clone();
    std::thread::spawn(move || exit_when_idle(&watched));
    http::serve(listener, "ui", Arc::new(move |request: &HttpRequest, stream: &mut TcpStream| route(&shared, request, stream)));
    let url = format!("http://127.0.0.1:{port}/?token={token}");
    let mcp = mcp::http::Endpoint::publish(port, token, json!({ "ui": url }));
    crate::log!("ui listening on http://127.0.0.1:{port}/");
    Ok(Started { url, mcp })
}

/// 道に要る守り。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Access {
    /// 画面のファイル（データを持たない）。
    Open,
    /// `?token=`（最初のページ、`EventSource`、`<img>`。頭を付けられないもの）。
    QueryToken,
    /// `Authorization: Bearer`（`fetch` で読むもの）。
    Bearer,
    /// `Authorization: Bearer` と、`Origin` が自分（状態を変えるもの）。
    BearerSameOrigin,
}

/// 道ごとの守り（守りの決まりは、この表だけで決める）。`None` は無い道。
fn access(method: &str, path: &str) -> Option<Access> {
    Some(match (method, path) {
        ("GET", "/" | "/events" | "/ui/capture") => Access::QueryToken,
        ("GET", "/fs/list" | "/fs/read" | "/sys/processes" | "/ui/tree" | "/startup") => Access::Bearer,
        ("POST", "/dap" | "/relaunch" | "/quit") => Access::BearerSameOrigin,
        ("GET", _) => Access::Open,
        _ => return None,
    })
}

fn route(shared: &Shared, request: &HttpRequest, stream: &mut TcpStream) -> io::Result<Connection> {
    if !http::local_host(request, shared.port) {
        return refuse(stream, "403 Forbidden");
    }
    if request.path == "/mcp" {
        return mcp::http::serve(request, stream, &shared.token, shared.port);
    }
    let Some(access) = access(&request.method, &request.path) else { return refuse(stream, "404 Not Found") };
    if !shared.allows(access, request) {
        return refuse(stream, "401 Unauthorized");
    }
    match request.path.as_str() {
        "/" => serve_asset(stream, "/index.html"),
        "/events" => events(shared, stream),
        "/dap" => dap(shared, request, stream),
        "/ui/capture" => {
            match request.param("window").and_then(|w| w.parse::<usize>().ok()).ok_or_else(|| tr!("There is no window.")).and_then(windows::capture) {
                Ok(png) => http::respond(stream, "200 OK", "image/png", &png)?,
                Err(message) => answer(stream, Err(message))?,
            }
            Ok(Connection::KeepAlive)
        }
        "/fs/list" | "/fs/read" | "/sys/processes" | "/ui/tree" | "/startup" => {
            answer(stream, read_only(shared, request))?;
            Ok(Connection::KeepAlive)
        }
        "/relaunch" => {
            let outcome = relaunch_other(&request.body);
            let relaunched = outcome.is_ok();
            answer(stream, outcome)?;
            if relaunched {
                post(ClientInput::Quit);
            }
            Ok(Connection::KeepAlive)
        }
        "/quit" => {
            post(ClientInput::Quit);
            http::respond_and_close(stream, "202 Accepted")?;
            Ok(Connection::Close)
        }
        path => serve_asset(stream, path),
    }
}

/// 読むだけの、画面だけの道。
fn read_only(shared: &Shared, request: &HttpRequest) -> Result<Value, String> {
    let path = request.param("path").unwrap_or_default();
    match request.path.as_str() {
        "/fs/list" => files::list(&path),
        "/fs/read" => files::read(&path),
        "/sys/processes" => Ok(processes::list()),
        "/ui/tree" => {
            let pid = request.param("pid").and_then(|p| p.parse::<u32>().ok()).ok_or_else(|| tr!("There is no pid."))?;
            windows::tree(pid, request.param("running").as_deref() != Some("false"))
        }
        _ => {
            let startup = std::mem::take(&mut *shared.startup.lock().unwrap_or_else(|e| e.into_inner()));
            Ok(json!({ "startup": startup, "live": shared.broadcaster.is_live() }))
        }
    }
}
impl Shared {
    fn allows(&self, access: Access, request: &HttpRequest) -> bool {
        match access {
            Access::Open => true,
            Access::QueryToken => self.query_token(request) && self.same_origin(request),
            Access::Bearer => request.has_bearer(&self.token),
            Access::BearerSameOrigin => request.has_bearer(&self.token) && self.same_origin(request),
        }
    }

    fn query_token(&self, request: &HttpRequest) -> bool {
        request.param("token").is_some_and(|given| http::constant_time_eq(&given, &self.token))
    }

    /// `Origin` が無い（ブラウザの外の道具）か、自分のページのもの。
    fn same_origin(&self, request: &HttpRequest) -> bool {
        request.header("origin").is_none_or(|origin| origin == format!("http://127.0.0.1:{}", self.port) || origin == format!("http://localhost:{}", self.port))
    }
}

/// 画面だけの口の答え: 成功なら本文、失敗なら 400 と `{ message }`。
fn answer(stream: &mut TcpStream, result: Result<Value, String>) -> io::Result<()> {
    match result {
        Ok(body) => http::respond_json(stream, "200 OK", Some(&body.to_string())),
        Err(message) => http::respond_json(stream, "400 Bad Request", Some(&json!({ "message": message }).to_string())),
    }
}

/// `POST /relaunch`: 本文は `{ program, args }` か `{ attach }`。対象のビット数のエンジンを起動する（このエンジンは
/// 答えたあとに終わる）。
fn relaunch_other(body: &[u8]) -> Result<Value, String> {
    let request: Value = serde_json::from_slice(body).map_err(|e| tr!("Cannot read it as JSON: {error}", error = e))?;
    let attach = request["attach"].as_u64().and_then(|p| u32::try_from(p).ok());
    let program = request["program"].as_str();
    let bits = relaunch::target_bits(program, attach).ok_or_else(|| tr!("Cannot tell the bitness of the target."))?;
    if bits == crate::debugger::target::Bits::of_this_process() {
        return Err(tr!("This engine ({bits}) can open it.", bits = bits.name()));
    }
    let arguments: Vec<String> = match (attach, program) {
        (Some(pid), _) => vec!["--attach".into(), pid.to_string()],
        (None, Some(program)) => std::iter::once(program.to_string()).chain(request["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from))).collect(),
        (None, None) => return Err(tr!("There is no program or attach.")),
    };
    relaunch::start(bits, &arguments)?;
    Ok(json!({ "engine": bits.name() }))
}

fn refuse(stream: &mut TcpStream, status: &str) -> io::Result<Connection> {
    http::respond_and_close(stream, status)?;
    Ok(Connection::Close)
}

fn serve_asset(stream: &mut TcpStream, path: &str) -> io::Result<Connection> {
    match assets::find(path) {
        // 頁の言語（<html lang>）は、エンジンが決めた表示の言語にする。画面はこれを見て文言を選ぶ（D-084）。
        Some((kind, bytes)) if path == "/index.html" => {
            let page = String::from_utf8_lossy(bytes).replacen("<html lang=\"ja\">", &format!("<html lang=\"{}\">", crate::i18n::tag()), 1);
            http::respond(stream, "200 OK", kind, page.as_bytes())?
        }
        Some((kind, bytes)) => http::respond(stream, "200 OK", kind, bytes)?,
        None => http::respond_json(stream, "404 Not Found", None)?,
    }
    Ok(Connection::KeepAlive)
}

/// `POST /dap`: 本文は `{ "command", "arguments" }`。入力の列に積み、DAP の応答（JSON）をそのまま返す。
fn dap(shared: &Shared, request: &HttpRequest, stream: &mut TcpStream) -> io::Result<Connection> {
    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(e) => return answer(stream, Err(tr!("Cannot read it as JSON: {error}", error = e))).map(|_| Connection::KeepAlive),
    };
    let Some(command) = message["command"].as_str() else {
        return answer(stream, Err(tr!("There is no command."))).map(|_| Connection::KeepAlive);
    };
    let seq = shared.next_seq.fetch_add(1, Ordering::Relaxed);
    let answer = shared.broadcaster.wait_for(seq);
    crate::log!("ui -> {command} #{seq}");
    post(ClientInput::Dap(Request { seq, command: command.to_string(), arguments: message.get("arguments").cloned().unwrap_or(Value::Null) }));
    match answer.recv_timeout(REPLY_TIMEOUT) {
        Ok(response) => http::respond_json(stream, "200 OK", Some(&response))?,
        Err(_) => {
            shared.broadcaster.forget(seq);
            http::respond_json(stream, "504 Gateway Timeout", Some(&json!({ "message": tr!("No answer to {command} came within {seconds} seconds.", command = command, seconds = REPLY_TIMEOUT.as_secs()) }).to_string()))?
        }
    }
    Ok(Connection::KeepAlive)
}

/// 画面が開いたあと、閉じたまま（対象も動いていない）`IDLE_EXIT` が過ぎたら、エンジンを終える（ブラウザのタブを
/// 閉じただけで、エンジンの窓が残り続けないように）。
fn exit_when_idle(shared: &Shared) {
    const TICK: Duration = Duration::from_secs(5);
    let mut idle = Duration::ZERO;
    loop {
        std::thread::sleep(TICK);
        let closed = shared.opened.load(Ordering::Relaxed) && shared.pages.load(Ordering::Relaxed) == 0 && !shared.broadcaster.is_live();
        idle = if closed { idle + TICK } else { Duration::ZERO };
        if idle >= IDLE_EXIT {
            crate::log!("ui: no page for {} s; exiting", idle.as_secs());
            return post(ClientInput::Quit);
        }
    }
}

/// 開いている `GET /events` を数える（落とすと減る）。
struct Page<'a>(&'a AtomicUsize);

impl Drop for Page<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// `GET /events`: Server-Sent Events で、これまでとこれからの出来事を流し続ける。
fn events(shared: &Shared, stream: &mut TcpStream) -> io::Result<Connection> {
    shared.pages.fetch_add(1, Ordering::Relaxed);
    shared.opened.store(true, Ordering::Relaxed);
    let _page = Page(&shared.pages);
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n")?;
    let (backlog, receiver) = shared.broadcaster.subscribe();
    for message in backlog {
        write!(stream, "data: {message}\n\n")?;
    }
    // これまでの分の終わり。画面は、ここまでを状態の作り直しにだけ使い、ここから先に応じて動く（止まったら
    // スタックを読む、など）。
    write!(stream, "data: {{\"type\":\"ui.replayed\"}}\n\n")?;
    stream.flush()?;
    loop {
        match receiver.recv_timeout(KEEP_ALIVE) {
            Ok(message) => write!(stream, "data: {message}\n\n")?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => write!(stream, ": keep-alive\n\n")?,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        stream.flush()?;
    }
    Ok(Connection::Close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_data_route_needs_the_token_and_changes_need_the_origin() {
        assert_eq!(access("GET", "/"), Some(Access::QueryToken));
        assert_eq!(access("GET", "/ui_bg.wasm"), Some(Access::Open));
        assert_eq!(access("GET", "/fs/read"), Some(Access::Bearer));
        assert_eq!(access("POST", "/dap"), Some(Access::BearerSameOrigin));
        assert_eq!(access("POST", "/quit"), Some(Access::BearerSameOrigin));
        assert_eq!(access("POST", "/fs/read"), None);
        assert_eq!(access("DELETE", "/"), None);
    }
}