//! 成績書（B3、D-081）: `netdbg.testItemChanged` の本文を、項目の一覧と、記録中の項目の操作にする。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    /// `OK`、`NG`（記録中なら `None`）。
    pub result: Option<String>,
    pub duration_ms: Option<u64>,
    pub operations: u64,
    pub pictures: u64,
    pub changes: u64,
}

impl Item {
    /// 所要時間（`4.4 秒`。記録中なら空）。
    pub fn duration_text(&self) -> String {
        self.duration_ms.map(|ms| crate::i18n::trf("{seconds} s", &[("seconds", format!("{:.1}", ms as f64 / 1000.0))])).unwrap_or_default()
    }
}

/// 記録中の操作 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub time: String,
    /// `AI`、`手`、`手順`。
    pub actor: String,
    /// 言語によらない種類（`ai`、`hand`、`note`。見た目の色分けに使う）。
    pub kind: String,
    pub text: String,
    /// 写した画面の数（操作した箇所を塗ったものと、操作のあと）。
    pub pictures: u64,
    /// もう一度流せる操作か（手順のファイルに入る。D-086）。
    pub replayable: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub items: Vec<Item>,
    /// 記録中の項目（番号と項目名）と、その操作（新しいものから）。
    pub open: Option<(String, String, Vec<Operation>)>,
    /// 書いた先のフォルダー（まだ書いていなければ `None`）。
    pub folder: Option<String>,
}

impl Report {
    pub fn from_body(body: &Value) -> Report {
        let text = |v: &Value| v.as_str().unwrap_or("").to_string();
        let items = body["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| Item {
                id: text(&i["id"]),
                title: text(&i["title"]),
                result: i["result"].as_str().map(String::from),
                duration_ms: i["durationMs"].as_u64(),
                operations: i["operations"].as_u64().unwrap_or(0),
                pictures: i["pictures"].as_u64().unwrap_or(0),
                changes: i["changes"].as_u64().unwrap_or(0),
            })
            .collect();
        let open = body["open"].is_object().then(|| {
            let operations = body["open"]["operations"].as_array().into_iter().flatten().map(|o| Operation { time: text(&o["time"]), actor: text(&o["actor"]), kind: text(&o["kind"]), text: text(&o["text"]), pictures: o["pictures"].as_u64().unwrap_or(0), replayable: o["replayable"] == true }).collect();
            (text(&body["open"]["id"]), text(&body["open"]["title"]), operations)
        });
        Report { items, open, folder: body["folder"].as_str().map(String::from) }
    }

    /// 要約（`OK 1 / NG 0 / 全 2`）。
    pub fn summary(&self) -> String {
        let count = |r: &str| self.items.iter().filter(|i| i.result.as_deref() == Some(r)).count();
        crate::i18n::trf("OK {ok} / NG {ng} / all {all}", &[("ok", count("OK").to_string()), ("ng", count("NG").to_string()), ("all", self.items.len().to_string())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_changed_event_becomes_items_and_the_open_operations() {
        let report = Report::from_body(&json!({
            "items": [
                { "id": "1-1", "title": "挨拶", "result": "OK", "durationMs": 4400, "operations": 3, "pictures": 2, "changes": 2 },
                { "id": "1-2", "title": "保存", "result": null, "operations": 1, "pictures": 0, "changes": 0 },
            ],
            "open": { "id": "1-2", "title": "保存", "operations": [{ "time": "09:41:44", "actor": "手", "kind": "hand", "text": "ボタン saveButton（保存）を押した", "pictures": 2, "replayable": true }] },
            "folder": null,
        }));
        assert_eq!(report.summary(), "OK 1 / NG 0 / 全 2");
        assert_eq!(report.items[0].duration_text(), "4.4 秒");
        assert_eq!(report.items[1].duration_text(), "");
        let (id, _, operations) = report.open.unwrap();
        assert_eq!((id.as_str(), operations[0].actor.as_str(), operations[0].kind.as_str(), operations[0].pictures, operations[0].replayable), ("1-2", "手", "hand", 2, true));
        assert_eq!(report.folder, None);
    }
}
