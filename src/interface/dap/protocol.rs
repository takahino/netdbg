//! Debug Adapter Protocol の読み書き。
//!
//! メッセージは `Content-Length: <バイト数>\r\n\r\n<JSON>` の形で流れる。ここでは枠の読み書きと、
//! 応答・イベントの組み立てだけを行い、要求の意味は扱わない。

use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};

/// 1 メッセージの上限。壊れたヘッダーで巨大な確保をしないための歯止め。
const MAX_CONTENT_LENGTH: usize = 64 * 1024 * 1024;

/// クライアントからの要求。
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub seq: i64,
    pub command: String,
    pub arguments: Value,
}

impl Request {
    /// JSON の 1 メッセージを要求として読む。要求でないもの（応答やイベント）は `None`。
    pub fn from_message(message: &Value) -> Option<Request> {
        if message.get("type")?.as_str()? != "request" {
            return None;
        }
        Some(Request {
            seq: message.get("seq")?.as_i64()?,
            command: message.get("command")?.as_str()?.to_string(),
            arguments: message.get("arguments").cloned().unwrap_or(Value::Null),
        })
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// 次のメッセージを読む。入力がメッセージの境界で終わっていれば `Ok(None)`。
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut first_line = true;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            if first_line {
                return Ok(None);
            }
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, tr!("The input ended in the middle of a DAP header")));
        }
        // .NET Framework の Process クラスは、コンソールが UTF-8 のとき子プロセスの標準入力の先頭に BOM を書く。
        // PowerShell から起動する試験クライアントがこれに当たるので、最初のヘッダーの前の BOM は読み飛ばす。
        let line = if first_line { line.trim_start_matches('\u{feff}') } else { line.as_str() };
        first_line = false;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| invalid(tr!("Not a DAP header: {line}", line = format!("{line:?}"))))?;
        if name.trim().eq_ignore_ascii_case("Content-Length") {
            let length: usize = value.trim().parse().map_err(|_| invalid(tr!("Content-Length is not a number: {value}", value = format!("{value:?}"))))?;
            if length > MAX_CONTENT_LENGTH {
                return Err(invalid(tr!("Content-Length is too large: {length}", length = length)));
            }
            content_length = Some(length);
        }
        // ほかのヘッダー（Content-Type など）は読み飛ばす。
    }
    let length = content_length.ok_or_else(|| invalid(tr!("There is no Content-Length header")))?;
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|e| invalid(tr!("The DAP body is not JSON: {error}", error = e)))
}

/// 応答とイベントを書き出す。`seq` はここで採番する。
pub struct Writer<W: Write> {
    out: W,
    next_seq: i64,
}

impl<W: Write> Writer<W> {
    pub fn new(out: W) -> Self {
        Writer { out, next_seq: 1 }
    }

    /// `seq` を付けて 1 メッセージを書き、フラッシュする。書いた JSON を返す（ログ用）。
    fn send(&mut self, mut message: Map<String, Value>) -> io::Result<String> {
        message.insert("seq".into(), json!(self.next_seq));
        self.next_seq += 1;
        let text = Value::Object(message).to_string();
        write!(self.out, "Content-Length: {}\r\n\r\n{}", text.len(), text)?;
        self.out.flush()?;
        Ok(text)
    }

    /// 成功の応答。`body` が null のときは `body` を付けない。
    pub fn respond(&mut self, request: &Request, body: Value) -> io::Result<String> {
        let mut message = response_base(request, true);
        if !body.is_null() {
            message.insert("body".into(), body);
        }
        self.send(message)
    }

    /// 失敗の応答。`message` は利用者に見える文言。
    pub fn fail(&mut self, request: &Request, message: &str) -> io::Result<String> {
        let mut response = response_base(request, false);
        response.insert("message".into(), json!(message));
        // VS Code はこの形の本文を優先して表示する。
        response.insert("body".into(), json!({ "error": { "id": 1, "format": message, "showUser": true } }));
        self.send(response)
    }

    /// アダプターからクライアントへの要求（逆向きの要求。`startDebugging` など）。答えは待たない。
    pub fn request(&mut self, command: &str, arguments: Value) -> io::Result<String> {
        let mut message = Map::new();
        message.insert("type".into(), json!("request"));
        message.insert("command".into(), json!(command));
        message.insert("arguments".into(), arguments);
        self.send(message)
    }

    pub fn event(&mut self, event: &str, body: Value) -> io::Result<String> {
        let mut message = Map::new();
        message.insert("type".into(), json!("event"));
        message.insert("event".into(), json!(event));
        if !body.is_null() {
            message.insert("body".into(), body);
        }
        self.send(message)
    }
}

fn response_base(request: &Request, success: bool) -> Map<String, Value> {
    let mut message = Map::new();
    message.insert("type".into(), json!("response"));
    message.insert("request_seq".into(), json!(request.seq));
    message.insert("command".into(), json!(request.command));
    message.insert("success".into(), json!(success));
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(json: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", json.len(), json).into_bytes()
    }

    #[test]
    fn reads_one_message() {
        let mut input = Cursor::new(frame(r#"{"seq":1,"type":"request","command":"initialize"}"#));
        let message = read_message(&mut input).unwrap().unwrap();
        assert_eq!(message["command"], "initialize");
    }

    #[test]
    fn reads_messages_back_to_back() {
        let mut bytes = frame(r#"{"seq":1}"#);
        bytes.extend(frame(r#"{"seq":2}"#));
        let mut input = Cursor::new(bytes);
        assert_eq!(read_message(&mut input).unwrap().unwrap()["seq"], 1);
        assert_eq!(read_message(&mut input).unwrap().unwrap()["seq"], 2);
        assert!(read_message(&mut input).unwrap().is_none());
    }

    #[test]
    fn counts_the_length_in_bytes_not_characters() {
        let mut input = Cursor::new(frame(r#"{"text":"日本語"}"#));
        assert_eq!(read_message(&mut input).unwrap().unwrap()["text"], "日本語");
    }

    #[test]
    fn ignores_other_headers_and_header_case() {
        let json = r#"{"seq":7}"#;
        let bytes = format!("Content-Type: application/json\r\ncontent-length: {}\r\n\r\n{}", json.len(), json);
        let mut input = Cursor::new(bytes.into_bytes());
        assert_eq!(read_message(&mut input).unwrap().unwrap()["seq"], 7);
    }

    #[test]
    fn skips_a_byte_order_mark_before_the_first_header() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(frame(r#"{"seq":9}"#));
        assert_eq!(read_message(&mut Cursor::new(bytes)).unwrap().unwrap()["seq"], 9);
    }

    #[test]
    fn end_of_input_at_a_boundary_is_not_an_error() {
        assert!(read_message(&mut Cursor::new(Vec::new())).unwrap().is_none());
    }

    #[test]
    fn rejects_a_missing_content_length() {
        let error = read_message(&mut Cursor::new(b"Content-Type: x\r\n\r\n{}".to_vec())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_a_truncated_body() {
        let error = read_message(&mut Cursor::new(b"Content-Length: 10\r\n\r\n{}".to_vec())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn rejects_a_body_that_is_not_json() {
        let error = read_message(&mut Cursor::new(frame("not json"))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn only_requests_become_requests() {
        let request = Request::from_message(&json!({"seq": 3, "type": "request", "command": "threads"})).unwrap();
        assert_eq!(request, Request { seq: 3, command: "threads".into(), arguments: Value::Null });
        assert!(Request::from_message(&json!({"seq": 3, "type": "response", "command": "threads"})).is_none());
    }

    fn written(buffer: &[u8]) -> Vec<Value> {
        let mut input = Cursor::new(buffer.to_vec());
        let mut messages = Vec::new();
        while let Some(message) = read_message(&mut input).unwrap() {
            messages.push(message);
        }
        messages
    }

    #[test]
    fn writes_responses_and_events_with_increasing_seq() {
        let request = Request { seq: 5, command: "initialize".into(), arguments: Value::Null };
        let mut buffer = Vec::new();
        {
            let mut writer = Writer::new(&mut buffer);
            writer.respond(&request, json!({"supportsX": true})).unwrap();
            writer.event("initialized", Value::Null).unwrap();
            writer.fail(&request, "だめでした").unwrap();
        }
        let messages = written(&buffer);
        assert_eq!(messages[0], json!({"seq": 1, "type": "response", "request_seq": 5, "command": "initialize", "success": true, "body": {"supportsX": true}}));
        assert_eq!(messages[1], json!({"seq": 2, "type": "event", "event": "initialized"}));
        assert_eq!(messages[2]["success"], false);
        assert_eq!(messages[2]["message"], "だめでした");
        assert_eq!(messages[2]["seq"], 3);
    }
}
