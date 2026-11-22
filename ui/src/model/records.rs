//! 止めずに読んだ記録（段階 11、12。エンジンの出来事の列）。SQL、例外、イベント、ダイアログ、トランザクション、
//! データバインディング、子プロセス、UI のフリーズ、環境を、別々の一覧にせず、起きた順の 1 本の列にする。
//!
//! 記録は、エンジンが種類ごとの出来事（`netdbg.sqlExecuted` など）で送る。中身は出来事の列の記録と同じ平らな JSON
//! （`seq`、`time`、`kind` と本文）。種類は出来事の名前で決める（ダイアログと子プロセスは、本文の `kind` を別の意味に
//! 使っている）。SQL の実行時間は、あとから `netdbg.sqlCompleted` で届く。

use serde_json::Value;
use std::collections::BTreeSet;

/// 持っておく記録の数（エンジンの出来事の列と同じ）。
pub const CAPACITY: usize = 10_000;
/// 1 行の要約の長さの上限（エンジンの `fault.rs` と同じ）。
const SUMMARY_LENGTH: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Sql,
    Exception,
    Event,
    Dialog,
    Transaction,
    Binding,
    Process,
    UiFreeze,
    Environment,
}

impl Kind {
    pub const ALL: [Kind; 9] = [Kind::Sql, Kind::Exception, Kind::Event, Kind::Dialog, Kind::Transaction, Kind::Binding, Kind::Process, Kind::UiFreeze, Kind::Environment];

    /// 出来事の名前から。記録の出来事でなければ `None`。
    pub fn of_event(name: &str) -> Option<Kind> {
        Some(match name {
            "netdbg.sqlExecuted" => Kind::Sql,
            "netdbg.exceptionThrown" => Kind::Exception,
            "netdbg.controlEvent" => Kind::Event,
            "netdbg.dialogShown" => Kind::Dialog,
            "netdbg.transaction" => Kind::Transaction,
            "netdbg.bindingFailed" => Kind::Binding,
            "netdbg.childProcess" => Kind::Process,
            "netdbg.uiFrozen" => Kind::UiFreeze,
            "netdbg.environment" => Kind::Environment,
            _ => return None,
        })
    }

    /// CSS の class に使う名前。
    pub fn class(self) -> &'static str {
        match self {
            Kind::Sql => "sql",
            Kind::Exception => "exception",
            Kind::Event => "event",
            Kind::Dialog => "dialog",
            Kind::Transaction => "transaction",
            Kind::Binding => "binding",
            Kind::Process => "process",
            Kind::UiFreeze => "freeze",
            Kind::Environment => "environment",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// エンジンの出来事の列の番号（UI のフリーズは番号を持たないので 0）。
    pub seq: u64,
    /// UNIX 時刻（ミリ秒）。
    pub time: f64,
    pub kind: Kind,
    pub body: Value,
    /// この画面の中での通し番号（一覧の鍵。`seq` の無いものもあるので別に持つ）。
    pub key: u64,
}

impl Record {
    pub fn text(&self, field: &str) -> &str {
        self.body[field].as_str().unwrap_or("")
    }

    /// 1 行の要約（エンジンの `fault.rs` の `summary` と同じ規則）。
    pub fn summary(&self) -> String {
        let text = |field: &str| self.text(field).to_string();
        let line = match self.kind {
            Kind::Sql => {
                let sql = self.body["sql"].as_str().or(self.body["commandText"].as_str()).unwrap_or("");
                let mut line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                if let Some(ms) = self.duration_ms() {
                    line.push_str(&format!(" ({ms} ms)"));
                }
                line
            }
            Kind::Dialog => [text("caption"), text("text"), text("form")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": "),
            Kind::Event => format!("{} (sender {})", text("handler"), text("sender")),
            Kind::Transaction => format!("{} #{}", text("event"), self.body["transactionId"]),
            Kind::Binding => format!("{} {}.{} ↔ {}", text("direction"), text("control"), text("property"), text("member")),
            Kind::Exception => format!("{}: {}", text("typeName"), text("message")),
            Kind::Process => format!("{} {}", text("fileName"), text("arguments")).trim().to_string(),
            Kind::UiFreeze => format!("UI thread {} did not respond for {} ms", self.body["threadId"], self.body["ms"]),
            Kind::Environment => format!("{} / {} / {}", text("program"), text("runtime"), text("bitness")),
        };
        if line.chars().count() > SUMMARY_LENGTH {
            line.chars().take(SUMMARY_LENGTH).collect::<String>() + "…"
        } else {
            line
        }
    }

    /// SQL の実行時間（届いていれば）。
    pub fn duration_ms(&self) -> Option<f64> {
        self.body["durationMs"].as_f64()
    }

    /// 呼び出し元（ソースの場所があれば、そのファイルと行）。
    pub fn caller(&self) -> Option<(String, Option<(String, u32)>)> {
        let caller = &self.body["caller"];
        let name = caller["name"].as_str().or(self.body["functionName"].as_str()).or(self.body["handler"].as_str())?;
        let place = caller["source"].as_str().or(self.body["source"].as_str()).map(|source| {
            let line = caller["line"].as_u64().or(self.body["line"].as_u64()).unwrap_or(1) as u32;
            (source.to_string(), line)
        });
        Some((name.to_string(), place))
    }

    pub fn thread(&self) -> Option<i64> {
        self.body["threadId"].as_i64()
    }
}

/// 記録の列。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Records {
    pub list: Vec<Record>,
    next_key: u64,
    /// 変わるたびに増える（画面は、これが変わったときだけ一覧を作り直す。記録は多いので、写して比べない）。
    pub revision: u64,
}

impl Records {
    /// 出来事 1 つ。記録の出来事なら足して true。SQL の実行時間の知らせなら、その記録に足す。
    pub fn apply(&mut self, name: &str, body: &Value) -> bool {
        if name == "netdbg.sqlCompleted" {
            let Some(seq) = body["seq"].as_u64() else { return false };
            if let Some(record) = self.list.iter_mut().rev().find(|r| r.kind == Kind::Sql && r.seq == seq) {
                record.body["durationMs"] = body["durationMs"].clone();
                if body["failed"].as_bool() == Some(true) {
                    record.body["failed"] = Value::Bool(true);
                }
                self.revision += 1;
                return true;
            }
            return false;
        }
        let Some(kind) = Kind::of_event(name) else { return false };
        self.next_key += 1;
        self.list.push(Record { seq: body["seq"].as_u64().unwrap_or(0), time: body["time"].as_f64().unwrap_or(0.0), kind, body: body.clone(), key: self.next_key });
        if self.list.len() > CAPACITY {
            let excess = self.list.len() - CAPACITY;
            self.list.drain(..excess);
        }
        self.revision += 1;
        true
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.revision += 1;
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.list.iter().filter(|r| r.kind == kind).count()
    }

    pub fn by_key(&self, key: u64) -> Option<&Record> {
        self.list.iter().rev().find(|r| r.key == key)
    }

    /// 選んだ種類で、要約か呼び出し元に `filter` を含むもの（大文字と小文字は区別しない）。
    pub fn matching(&self, kinds: &BTreeSet<Kind>, filter: &str) -> Vec<Record> {
        let filter = filter.trim().to_lowercase();
        self.list
            .iter()
            .filter(|r| kinds.contains(&r.kind))
            .filter(|r| filter.is_empty() || r.summary().to_lowercase().contains(&filter) || r.caller().is_some_and(|(name, _)| name.to_lowercase().contains(&filter)))
            .cloned()
            .collect()
    }

    pub fn by_seq(&self, seq: u64) -> Option<&Record> {
        self.list.iter().rev().find(|r| r.seq == seq && seq != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_come_from_the_event_name_not_the_body() {
        let mut records = Records::default();
        assert!(records.apply("netdbg.dialogShown", &json!({ "seq": 3, "time": 10.0, "kind": "messageBox", "caption": "確認", "text": "保存しますか" })));
        assert!(!records.apply("output", &json!({})));
        assert_eq!((records.list[0].kind, records.list[0].seq), (Kind::Dialog, 3));
        assert_eq!(records.list[0].summary(), "確認: 保存しますか");
    }

    #[test]
    fn sql_gets_its_duration_later() {
        let mut records = Records::default();
        records.apply("netdbg.sqlExecuted", &json!({ "seq": 7, "sql": "SELECT *\n  FROM  T WHERE id = 1", "caller": { "name": "Form1.Load", "source": "C:\\a\\Form1.cs", "line": 20 } }));
        assert_eq!(records.list[0].summary(), "SELECT * FROM T WHERE id = 1");
        assert!(records.apply("netdbg.sqlCompleted", &json!({ "seq": 7, "durationMs": 12.5, "failed": true })));
        assert_eq!(records.list[0].summary(), "SELECT * FROM T WHERE id = 1 (12.5 ms)");
        assert_eq!(records.list[0].body["failed"], true);
        assert_eq!(records.list[0].caller(), Some(("Form1.Load".into(), Some(("C:\\a\\Form1.cs".into(), 20)))));
        assert!(!records.apply("netdbg.sqlCompleted", &json!({ "seq": 99, "durationMs": 1 })));
    }

    #[test]
    fn records_are_filtered_by_kind_and_text_and_capped() {
        let mut records = Records::default();
        for i in 0..CAPACITY + 5 {
            records.apply("netdbg.exceptionThrown", &json!({ "seq": i + 1, "typeName": "System.InvalidOperationException", "message": format!("m{i}") }));
        }
        records.apply("netdbg.uiFrozen", &json!({ "threadId": 4, "ms": 2300 }));
        assert_eq!(records.list.len(), CAPACITY);
        let exceptions: BTreeSet<Kind> = [Kind::Exception].into_iter().collect();
        assert_eq!(records.matching(&exceptions, "m10004").len(), 1);
        assert_eq!(records.matching(&exceptions, "M1000").len(), 6, "filter is not case sensitive");
        let all: BTreeSet<Kind> = Kind::ALL.into_iter().collect();
        assert_eq!(records.matching(&all, "did not respond").len(), 1);
        assert_eq!(records.by_seq(10005).map(|r| r.text("message")), Some("m10004"));
    }

    #[test]
    fn long_summaries_are_shortened() {
        let mut records = Records::default();
        records.apply("netdbg.exceptionThrown", &json!({ "typeName": "E", "message": "x".repeat(300) }));
        assert_eq!(records.list[0].summary().chars().count(), SUMMARY_LENGTH + 1);
    }
}
