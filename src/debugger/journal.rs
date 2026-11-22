//! 出来事の列（段階 11）。止めずに読んだもの（SQL、例外、イベントなど）を、起きた順に時刻つきで積む。上限を超えたら
//! 古いものから捨てる。読む側（DAP の `netdbg.journal`、MCP のツール）は、前に読んだ番号のあとだけを取れる。

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// 積んでおく数の上限。
pub const CAPACITY: usize = 10_000;

/// 1 つの出来事。
#[derive(Debug, Clone)]
pub struct Record {
    /// 1 から増える番号（捨てても振り直さない）。
    pub seq: u64,
    /// UNIX 時刻（ミリ秒）。
    pub time: u64,
    /// 種類（`sql` など）。
    pub kind: &'static str,
    /// 種類ごとの中身。
    pub body: Value,
}

impl Record {
    /// 外に渡す形: 中身に、番号、時刻、種類を足したもの。
    pub fn to_json(&self) -> Value {
        let mut out = json!({ "seq": self.seq, "time": self.time, "kind": self.kind });
        if let (Value::Object(target), Value::Object(fields)) = (&mut out, &self.body) {
            for (key, value) in fields {
                target.insert(key.clone(), value.clone());
            }
        }
        out
    }
}

#[derive(Debug)]
pub struct Journal {
    records: VecDeque<Record>,
    next_seq: u64,
    capacity: usize,
    /// 上限を超えて捨てた数（読む側に、抜けがあることを伝える）。
    dropped: u64,
}

impl Default for Journal {
    fn default() -> Journal {
        Journal::with_capacity(CAPACITY)
    }
}

impl Journal {
    pub fn with_capacity(capacity: usize) -> Journal {
        Journal { records: VecDeque::new(), next_seq: 1, capacity: capacity.max(1), dropped: 0 }
    }

    /// 出来事を積み、積んだものを返す。
    pub fn push(&mut self, kind: &'static str, body: Value) -> &Record {
        if self.records.len() == self.capacity {
            self.records.pop_front();
            self.dropped += 1;
        }
        self.records.push_back(Record { seq: self.next_seq, time: now_ms(), kind, body });
        self.next_seq += 1;
        self.records.back().expect("pushed just now")
    }

    /// 積んだ出来事の中身を、あとから足す（SQL の実行時間など）。もう捨てた番号なら何もしない。
    pub fn update(&mut self, seq: u64, change: impl FnOnce(&mut Value)) {
        // 足すのはたいてい新しい出来事なので、後ろから探す。
        if let Some(record) = self.records.iter_mut().rev().find(|r| r.seq == seq) {
            change(&mut record.body);
        }
    }

    /// 番号 `after` より後の出来事（`kinds` が空でなければ、その種類だけ）を、古い順に `limit` まで。
    pub fn read(&self, after: u64, kinds: &[String], limit: usize) -> Vec<&Record> {
        self.records.iter().filter(|r| r.seq > after && (kinds.is_empty() || kinds.iter().any(|k| k == r.kind))).take(limit).collect()
    }

    /// いちばん新しい出来事。
    pub fn newest(&self) -> Option<&Record> {
        self.records.back()
    }

    /// 新しい順の出来事。
    pub fn recent(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().rev()
    }

    /// いちばん新しい出来事の番号（まだ無ければ 0）。
    pub fn last_seq(&self) -> u64 {
        self.next_seq - 1
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 新しいセッション（対象を起動し直した）では、空にする。番号は続ける（前に読んだ番号が、新しい出来事を隠さない）。
    pub fn clear(&mut self) {
        self.records.clear();
        self.dropped = 0;
    }
}

/// いまの UNIX 時刻（ミリ秒）。出来事の `time` と同じ物差し。
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_numbered_and_read_after_a_number() {
        let mut journal = Journal::default();
        journal.push("sql", json!({ "commandText": "a" }));
        journal.push("exception", json!({}));
        journal.push("sql", json!({ "commandText": "b" }));
        let all: Vec<u64> = journal.read(0, &[], 100).iter().map(|r| r.seq).collect();
        assert_eq!(all, vec![1, 2, 3]);
        let sql: Vec<u64> = journal.read(1, &["sql".to_string()], 100).iter().map(|r| r.seq).collect();
        assert_eq!(sql, vec![3]);
        assert_eq!(journal.read(0, &[], 2).len(), 2);
        assert_eq!(journal.last_seq(), 3);
        let shown = journal.read(2, &[], 1)[0].to_json();
        assert_eq!(shown["kind"], "sql");
        assert_eq!(shown["commandText"], "b");
        assert_eq!(shown["seq"], 3);
    }

    #[test]
    fn the_oldest_records_are_dropped_past_the_capacity_and_numbers_go_on_after_clear() {
        let mut journal = Journal::with_capacity(2);
        for _ in 0..3 {
            journal.push("sql", json!({}));
        }
        assert_eq!(journal.read(0, &[], 10).iter().map(|r| r.seq).collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(journal.dropped(), 1);
        journal.clear();
        assert!(journal.read(0, &[], 10).is_empty());
        assert_eq!(journal.push("sql", json!({})).seq, 4);
    }
}
