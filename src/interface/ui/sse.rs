//! 画面への DAP のメッセージの振り分け。
//!
//! 画面は、VS Code と同じ DAP のクライアントとして hub につなぐ（`DapClient<SseSink>`）。`DapClient` が書いた
//! メッセージ（`protocol::Writer` は 1 つ書くたびに `flush` する）を、`flush` の時点で 1 つずつ取り出し、
//! - 応答は、その `request_seq` を待っている `POST /dap` に返す。
//! - それ以外（イベント、逆向きの要求）は、すべての `GET /events`（Server-Sent Events）に流す。
//!
//! 開き直したページが今の状態を作り直せるように、流したものを `BACKLOG` 件まで持っておき、つないだときに先に流す。

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// 持っておく出来事の数と、そのうちの `output` の数。出力の多い対象でも、止まった、などの出来事が押し出されないように。
const BACKLOG: usize = 5000;
const BACKLOG_OUTPUT: usize = 2000;

/// 画面に送るものの振り分け先。hub の書き手（入力のループのスレッド）と、HTTP の各スレッドが共有する。
#[derive(Default)]
pub struct Broadcaster {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    subscribers: Vec<Sender<Arc<str>>>,
    /// 流したもの（`output` か、本文）。
    backlog: VecDeque<(bool, Arc<str>)>,
    outputs: usize,
    /// 答えを待っている `POST /dap`（`request_seq` ごと）。
    waiting: HashMap<i64, Sender<String>>,
    /// 対象が動いているか（`process` から `terminated` / `exited` まで）。
    live: bool,
}

impl Broadcaster {
    pub fn new() -> Arc<Broadcaster> {
        Arc::new(Broadcaster::default())
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // 持っているスレッドが落ちても、中身は壊れていない（足し引きだけ）ので使い続ける。
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 要求 `seq` の答えを待つ口。要求を入力の列に積む前に作る（答えが先に来ても取りこぼさない）。
    pub fn wait_for(&self, seq: i64) -> Receiver<String> {
        let (sender, receiver) = channel();
        self.state().waiting.insert(seq, sender);
        receiver
    }

    /// 待つのをやめた（時間切れ）。
    pub fn forget(&self, seq: i64) {
        self.state().waiting.remove(&seq);
    }

    /// 出来事を受け取り始める。これまでの出来事と、これからの出来事の口。
    pub fn subscribe(&self) -> (Vec<Arc<str>>, Receiver<Arc<str>>) {
        let (sender, receiver) = channel();
        let mut state = self.state();
        state.subscribers.push(sender);
        (state.backlog.iter().map(|(_, m)| m.clone()).collect(), receiver)
    }

    /// 対象が動いているか（画面が閉じたまま、エンジンを終えてよいかの判断に使う）。
    pub fn is_live(&self) -> bool {
        self.state().live
    }

    #[cfg(test)]
    fn subscribers(&self) -> usize {
        self.state().subscribers.len()
    }

    /// DAP のメッセージ 1 つ（JSON の本文）を振り分ける。
    fn deliver(&self, text: &str) {
        let message: Value = match serde_json::from_str(text) {
            Ok(message) => message,
            Err(e) => return crate::log!("ui: not a DAP message: {e}"),
        };
        let mut state = self.state();
        if message["type"] == "response" {
            // 新しいセッションの始まり。前のセッションの出来事は、開き直したページに要らない。
            if message["command"] == "initialize" {
                state.backlog.clear();
                state.outputs = 0;
            }
            if let Some(waiter) = message["request_seq"].as_i64().and_then(|seq| state.waiting.remove(&seq)) {
                let _ = waiter.send(text.to_string());
            }
            return;
        }
        let shared: Arc<str> = Arc::from(text);
        match message["event"].as_str() {
            Some("process") => state.live = true,
            Some("terminated" | "exited") => state.live = false,
            _ => {}
        }
        let output = message["event"] == "output";
        state.backlog.push_back((output, shared.clone()));
        state.outputs += usize::from(output);
        if state.outputs > BACKLOG_OUTPUT {
            if let Some(at) = state.backlog.iter().position(|(o, _)| *o) {
                state.backlog.remove(at);
                state.outputs -= 1;
            }
        }
        if state.backlog.len() > BACKLOG {
            if let Some((o, _)) = state.backlog.pop_front() {
                state.outputs -= usize::from(o);
            }
        }
        // 閉じた画面（受け手が落ちた口）は、ここで外す。
        state.subscribers.retain(|s| s.send(shared.clone()).is_ok());
    }
}

/// `DapClient` の書き先。`Content-Length` の頭を外して、`Broadcaster` に渡す。
pub struct SseSink {
    buffer: Vec<u8>,
    broadcaster: Arc<Broadcaster>,
}

impl SseSink {
    pub fn new(broadcaster: Arc<Broadcaster>) -> SseSink {
        SseSink { buffer: Vec::new(), broadcaster }
    }
}

impl Write for SseSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let framed = std::mem::take(&mut self.buffer);
        let body = match framed.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(at) => &framed[at + 4..],
            None => &framed[..],
        };
        self.broadcaster.deliver(&String::from_utf8_lossy(body));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::dap::protocol::{Request, Writer};
    use serde_json::json;

    #[test]
    fn responses_go_to_the_waiting_request_and_events_to_every_page() {
        let broadcaster = Broadcaster::new();
        let (_, first) = broadcaster.subscribe();
        let answer = broadcaster.wait_for(7);
        let mut writer = Writer::new(SseSink::new(broadcaster.clone()));
        writer.respond(&Request { seq: 7, command: "threads".into(), arguments: Value::Null }, json!({ "threads": [] })).unwrap();
        writer.event("stopped", json!({ "reason": "breakpoint" })).unwrap();
        let response: Value = serde_json::from_str(&answer.try_recv().unwrap()).unwrap();
        assert_eq!((response["request_seq"].as_i64(), response["body"]["threads"].as_array().map(Vec::len)), (Some(7), Some(0)));
        let event: Value = serde_json::from_str(&first.try_recv().unwrap()).unwrap();
        assert_eq!(event["event"], "stopped");
        assert!(first.try_recv().is_err(), "the response is not broadcast");
    }

    #[test]
    fn a_page_that_opens_later_gets_the_events_so_far() {
        let broadcaster = Broadcaster::new();
        let mut writer = Writer::new(SseSink::new(broadcaster.clone()));
        writer.event("process", json!({ "name": "a.exe" })).unwrap();
        let (backlog, _) = broadcaster.subscribe();
        assert_eq!(backlog.len(), 1);
        // initialize の答えで、前のセッションの出来事を捨てる。
        writer.respond(&Request { seq: 1, command: "initialize".into(), arguments: Value::Null }, json!({})).unwrap();
        assert!(broadcaster.subscribe().0.is_empty());
    }

    #[test]
    fn output_does_not_push_out_the_other_events() {
        let broadcaster = Broadcaster::new();
        let mut writer = Writer::new(SseSink::new(broadcaster.clone()));
        writer.event("stopped", json!({})).unwrap();
        for i in 0..BACKLOG_OUTPUT + 10 {
            writer.event("output", json!({ "output": i.to_string() })).unwrap();
        }
        let (backlog, _) = broadcaster.subscribe();
        assert_eq!(backlog.len(), BACKLOG_OUTPUT + 1);
        assert!(backlog[0].contains("stopped"));
        assert!(backlog[1].contains("\"10\""), "the oldest output lines are dropped first");
    }

    #[test]
    fn closed_pages_are_dropped() {
        let broadcaster = Broadcaster::new();
        drop(broadcaster.subscribe());
        let (_, open) = broadcaster.subscribe();
        Writer::new(SseSink::new(broadcaster.clone())).event("output", json!({})).unwrap();
        assert_eq!(broadcaster.subscribers(), 1);
        assert!(open.try_recv().is_ok());
    }
}
