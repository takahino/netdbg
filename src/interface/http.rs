//! 127.0.0.1 で待ち受ける HTTP の最小限（MCP の `--mcp-listen` と、専用の画面の `--ui` が使う）。
//!
//! 依存を増やさないために手で書く（D-011）。接続ごとにスレッドを 1 つ起こし、HTTP/1.1 の持続的な接続で要求に順に
//! 答える。答え方（道ごとの処理）は、使う側が `Handler` で決める。守りの判定（`Host`、`Origin`、トークン）も、ここに
//! 置く。

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

/// 1 つの要求の本文の上限。
const MAX_BODY: usize = 16 * 1024 * 1024;

/// HTTP の要求 1 つ（読んだ分だけ）。
pub struct HttpRequest {
    pub method: String,
    /// `?` の前の部分。
    pub path: String,
    /// `?` のあとの部分（無ければ空）。
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// 問い合わせの文字列の値（`%xx` と `+` を戻したもの）。
    pub fn param(&self, name: &str) -> Option<String> {
        self.query.split('&').filter_map(|pair| pair.split_once('=').or(Some((pair, "")))).find(|(n, _)| *n == name).map(|(_, v)| percent_decode(v))
    }

    /// `Authorization: Bearer <token>` が合っているか。
    pub fn has_bearer(&self, token: &str) -> bool {
        self.header("authorization").is_some_and(|value| value.strip_prefix("Bearer ").is_some_and(|given| constant_time_eq(given, token)))
    }

    fn wants_close(&self) -> bool {
        self.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"))
    }
}

/// 答えたあと、接続を続けるか。
#[derive(Debug, PartialEq)]
pub enum Connection {
    KeepAlive,
    Close,
}

/// 要求 1 つに答える処理。答えは自分で書く（SSE のように、流し続けてもよい）。
pub type Handler = dyn Fn(&HttpRequest, &mut TcpStream) -> io::Result<Connection> + Send + Sync;

/// 127.0.0.1 の空いた番号で待ち受ける。
pub fn bind() -> io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// 受けた接続を、接続ごとのスレッドで `handler` に渡し続けるスレッドを起こす。
pub fn serve(listener: TcpListener, name: &'static str, handler: Arc<Handler>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let handler = handler.clone();
            std::thread::spawn(move || {
                if let Err(e) = serve_connection(stream, &*handler) {
                    crate::log!("{name} http: {e}");
                }
            });
        }
    });
}

/// 1 つの接続の要求に、順に答える（HTTP/1.1 の持続的な接続）。
fn serve_connection(stream: TcpStream, handler: &Handler) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    while let Some(request) = read_request(&mut reader)? {
        if handler(&request, &mut writer)? == Connection::Close || request.wants_close() {
            break;
        }
    }
    Ok(())
}

/// 要求を 1 つ読む。接続が閉じていれば `Ok(None)`。
pub fn read_request(reader: &mut impl BufRead) -> io::Result<Option<HttpRequest>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err(io::Error::other(tr!("Not an HTTP request line: {line}", line = format!("{line:?}"))));
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (method, path, query) = (method.to_string(), path.to_string(), query.to_string());
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::other(tr!("The connection closed in the middle of the headers")));
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let length: usize = headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if length > MAX_BODY {
        return Err(io::Error::other(tr!("The body is too large: {length}", length = length)));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(HttpRequest { method, path, query, headers, body }))
}

/// 答えを 1 つ書く。`content_type` が空なら、その頭を付けない。
pub fn respond(stream: &mut impl Write, status: &str, content_type: &str, body: &[u8]) -> io::Result<()> {
    write_response(stream, status, content_type, body, false)
}

/// 答えを書き、接続を閉じると知らせる（断ったときなど。クライアントが、閉じた接続を使い回さないように）。
pub fn respond_and_close(stream: &mut impl Write, status: &str) -> io::Result<()> {
    write_response(stream, status, "", b"", true)
}

fn write_response(stream: &mut impl Write, status: &str, content_type: &str, body: &[u8], close: bool) -> io::Result<()> {
    let kind = if content_type.is_empty() { String::new() } else { format!("Content-Type: {content_type}\r\n") };
    let connection = if close { "Connection: close\r\n" } else { "" };
    // 画面のページも答えも、ほかのサイトの枠に入れさせず、型を推測させない。
    write!(stream, "HTTP/1.1 {status}\r\n{kind}{connection}Content-Length: {}\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nCache-Control: no-store\r\n\r\n", body.len())?;
    stream.write_all(body)?;
    stream.flush()
}

/// JSON の答え（本文が無ければ頭だけ）。
pub fn respond_json(stream: &mut impl Write, status: &str, body: Option<&str>) -> io::Result<()> {
    match body {
        Some(body) if !body.is_empty() => respond(stream, status, "application/json", body.as_bytes()),
        _ => respond(stream, status, "", b""),
    }
}

/// `Host` が、この待ち受けそのもの（`127.0.0.1:<port>` か `localhost:<port>`）か。DNS の付け替え（DNS rebinding）で、
/// ほかの名前のページから届いた要求を断るため。`Host` の無い要求（HTTP/1.0 の古い道具）も断る。
pub fn local_host(request: &HttpRequest, port: u16) -> bool {
    let Some(host) = request.header("host") else { return false };
    let Some((name, given)) = host.rsplit_once(':') else { return false };
    given.parse::<u16>().ok() == Some(port) && matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

/// localhost からの Origin か（`http://localhost:…`、`http://127.0.0.1:…`、`vscode-file://…` など、ブラウザーの外のもの）。
pub fn allowed_origin(origin: &str) -> bool {
    let host = origin.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("");
    matches!(host, "localhost" | "127.0.0.1" | "[::1]") || origin.starts_with("vscode-file://") || origin.starts_with("vscode-webview://")
}

/// 推測できないトークン（128 ビットの乱数を 16 進で）。
pub fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG: アルゴリズムのハンドル無しで、OS の乱数を使う。
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x2;
    let status = unsafe { crate::win32::BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status != 0 {
        return Err(io::Error::other(tr!("BCryptGenRandom failed ({status})", status = format!("0x{status:08X}"))));
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 比べる時間が、どこで違うかに依らない比較（トークンを 1 文字ずつ当てられないように）。
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `%xx` と `+` を戻す（UTF-8 として。壊れた並びは置き換え文字にする）。
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match bytes.get(i + 1..i + 3).filter(|hex| hex.iter().all(u8::is_ascii_hexdigit)).and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request(headers: Vec<(&str, &str)>) -> HttpRequest {
        HttpRequest { method: "GET".into(), path: "/".into(), query: String::new(), headers: headers.into_iter().map(|(n, v)| (n.into(), v.into())).collect(), body: Vec::new() }
    }

    #[test]
    fn a_request_is_read_with_its_headers_query_and_body() {
        let raw = "POST /mcp?a=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer abc\r\nContent-Length: 2\r\n\r\n{}";
        let request = read_request(&mut Cursor::new(raw.as_bytes())).unwrap().unwrap();
        assert_eq!((request.method.as_str(), request.path.as_str(), request.query.as_str(), request.body.as_slice()), ("POST", "/mcp", "a=1", b"{}".as_slice()));
        assert_eq!(request.header("authorization"), Some("Bearer abc"));
        assert!(request.has_bearer("abc"));
        assert!(!request.has_bearer("abd"));
        assert!(read_request(&mut Cursor::new(b"".as_slice())).unwrap().is_none());
    }

    #[test]
    fn query_values_are_decoded() {
        let mut r = request(vec![]);
        r.query = "path=C%3A%5C%E6%97%A5%E6%9C%AC+x&flag&bad=%zz".into();
        assert_eq!(r.param("path").as_deref(), Some("C:\\日本 x"));
        assert_eq!(r.param("flag").as_deref(), Some(""));
        assert_eq!(r.param("bad").as_deref(), Some("%zz"));
        assert_eq!(r.param("none"), None);
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("a%41"), "aA");
    }

    #[test]
    fn only_this_listener_is_a_local_host() {
        assert!(local_host(&request(vec![("Host", "127.0.0.1:5000")]), 5000));
        assert!(local_host(&request(vec![("Host", "localhost:5000")]), 5000));
        assert!(!local_host(&request(vec![("Host", "127.0.0.1:5001")]), 5000));
        assert!(!local_host(&request(vec![("Host", "evil.example:5000")]), 5000));
        assert!(!local_host(&request(vec![("Host", "127.0.0.1")]), 5000));
        assert!(!local_host(&request(vec![]), 5000));
    }

    #[test]
    fn local_origins_are_allowed() {
        assert!(allowed_origin("http://localhost:3000"));
        assert!(allowed_origin("http://127.0.0.1:5000"));
        assert!(allowed_origin("vscode-file://vscode-app"));
        assert!(!allowed_origin("https://example.com"));
        assert!(!allowed_origin("http://localhost.example.com"));
    }

    #[test]
    fn tokens_are_random_and_long() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(constant_time_eq(&a, &a.clone()));
        assert!(!constant_time_eq(&a, &b));
    }

    #[test]
    fn a_response_has_its_length_and_type() {
        let mut out = Vec::new();
        respond(&mut out, "200 OK", "text/plain", "日本".as_bytes()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 6\r\n"));
        assert!(text.ends_with("\r\n\r\n日本"));
    }
}
