//! VS Code（DAP のクライアント）との連携。要求を本体に渡し（`hub`）、答えとイベントを DAP で書く。

pub mod protocol;

use crate::debugger::engine::{Flow, Reply};
use crate::debugger::session::Event;
use protocol::{Request, Writer};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{self, BufReader, Write};

/// DAP の要求を読むスレッド（標準入力）。
pub fn read_requests(input: File) {
    let mut input = BufReader::new(input);
    loop {
        match protocol::read_message(&mut input) {
            Ok(Some(message)) => {
                crate::log!("-> {message}");
                match Request::from_message(&message) {
                    Some(request) => super::post(super::ClientInput::Dap(request)),
                    // エンジンから逆向きの要求は出していないので、応答が来ることは無い。
                    None => crate::log!("ignored: not a request"),
                }
            }
            Ok(None) => return super::post(super::ClientInput::Closed(None)),
            Err(e) => return super::post(super::ClientInput::Closed(Some(e.to_string()))),
        }
    }
}

/// DAP のクライアント（VS Code）。
pub struct DapClient<W: Write> {
    writer: Writer<W>,
    /// `disconnect` の答えを、対象の終了まで保留しているとき。
    pending_disconnect: Option<Request>,
    /// クライアントが `invalidated` イベントを受け取れるか（initialize の `supportsInvalidatedEvent`）。
    invalidated_event: bool,
}

impl<W: Write> DapClient<W> {
    pub fn new(out: W) -> Self {
        DapClient { writer: Writer::new(out), pending_disconnect: None, invalidated_event: false }
    }

    /// クライアントの要求への、本体の答えを書く。`disconnect` で対象の終わりを待つときは、答えを保留する。
    pub fn answer(&mut self, request: &Request, reply: &Reply) -> io::Result<()> {
        if request.command == "initialize" {
            self.invalidated_event = request.arguments.get("supportsInvalidatedEvent").and_then(Value::as_bool).unwrap_or(false);
        }
        if reply.flow == Flow::ExitAfterProcess {
            self.pending_disconnect = Some(request.clone());
        } else {
            let text = match &reply.body {
                Ok(body) => self.writer.respond(request, body.clone())?,
                Err(message) => self.writer.fail(request, message)?,
            };
            crate::log!("<- {text}");
        }
        if reply.changed_values {
            self.values_changed()?;
        }
        self.events(&reply.events)
    }

    /// 本体の出来事を、イベントとして書く（このクライアントの操作でも、ほかの IF の操作でも）。
    pub fn events(&mut self, events: &[Event]) -> io::Result<()> {
        for (name, body) in events {
            self.event(name, body.clone())?;
        }
        Ok(())
    }

    pub fn event(&mut self, name: &str, body: Value) -> io::Result<()> {
        // `request:<command>` は、エンジンからクライアントへの逆向きの要求（子プロセスの `startDebugging` など）。
        let text = match name.strip_prefix(crate::debugger::session::REVERSE_REQUEST) {
            Some(command) => self.writer.request(command, body)?,
            None => self.writer.event(name, body)?,
        };
        crate::log!("<- {text}");
        Ok(())
    }

    /// 式で値を書き換えた、またはメソッドを呼んだ。変数の欄とウォッチを取り直してもらう。
    pub fn values_changed(&mut self) -> io::Result<()> {
        if self.invalidated_event {
            self.event("invalidated", json!({ "areas": ["variables"] }))?;
        }
        Ok(())
    }

    /// `disconnect` の答えを、対象の終わりまで保留しているか。
    pub fn is_disconnecting(&self) -> bool {
        self.pending_disconnect.is_some()
    }

    /// 保留していた `disconnect` に答える。答えたら true（ループを終える）。
    pub fn finish_disconnect(&mut self) -> io::Result<bool> {
        let Some(request) = self.pending_disconnect.take() else { return Ok(false) };
        let text = self.writer.respond(&request, Value::Null)?;
        crate::log!("<- {text}");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::engine::Engine;
    use crate::debugger::session::Owner;
    use protocol::read_message;
    use std::io::Cursor;

    /// クライアントの要求を本体に渡し、書いた DAP のメッセージを読む。
    fn run(requests: Vec<(&str, Value)>) -> Vec<Value> {
        let mut buffer = Vec::new();
        {
            let (mut engine, mut client) = (Engine::new(), DapClient::new(&mut buffer));
            for (seq, (command, arguments)) in requests.into_iter().enumerate() {
                let request = Request { seq: seq as i64 + 1, command: command.into(), arguments };
                let reply = engine.request(request.seq, &request.command, &request.arguments, Owner::Client);
                client.answer(&request, &reply).unwrap();
            }
        }
        let mut input = Cursor::new(buffer);
        let mut messages = Vec::new();
        while let Some(m) = read_message(&mut input).unwrap() {
            messages.push(m);
        }
        messages
    }

    #[test]
    fn initialize_is_answered_before_the_initialized_event() {
        let messages = run(vec![("initialize", Value::Null)]);
        assert_eq!((messages[0]["type"].as_str(), messages[0]["success"].as_bool()), (Some("response"), Some(true)));
        assert_eq!(messages[1]["event"], "initialized");
    }

    #[test]
    fn a_failure_carries_the_message_for_the_user() {
        let messages = run(vec![("noSuchRequest", Value::Null)]);
        assert_eq!(messages[0]["success"], false);
        assert_eq!(messages[0]["request_seq"], 1);
        assert!(messages[0]["message"].as_str().unwrap().contains("noSuchRequest"));
    }

    #[test]
    fn disconnect_without_a_debuggee_is_answered_at_once() {
        let messages = run(vec![("disconnect", Value::Null)]);
        assert_eq!(messages[0]["success"], true);
    }
}
