//! コール スタックとスレッド（DAP の `stackTrace` と `threads` の答え）。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    /// ソースのファイル（無ければ `None`。ソースの無いフレーム）。
    pub path: Option<String>,
    pub line: u32,
    /// 逆コンパイルしたソースなど、ソースの出どころ。
    pub origin: Option<String>,
    /// `subtle`（ほかのコード）や `label`（区切り）。
    pub hint: Option<String>,
    /// メソッドの最初と最後の行（エンジンの `netdbgMethodLines`。通った行に変数の値を出す範囲）。
    pub method_lines: Option<(u32, u32)>,
}

pub fn frames(body: &Value) -> Vec<Frame> {
    body["stackFrames"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|f| Frame {
                    id: f["id"].as_i64().unwrap_or(0),
                    name: f["name"].as_str().unwrap_or("").to_string(),
                    path: f["source"]["path"].as_str().filter(|p| !p.is_empty()).map(String::from),
                    line: f["line"].as_u64().and_then(|l| u32::try_from(l).ok()).unwrap_or(0),
                    origin: f["source"]["origin"].as_str().map(String::from),
                    hint: f["presentationHint"].as_str().map(String::from),
                    method_lines: f["netdbgMethodLines"]["first"].as_u64().zip(f["netdbgMethodLines"]["last"].as_u64()).map(|(a, b)| (a as u32, b as u32)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 最初に開くフレーム: ソースのある、いちばん上のもの（無ければ一番上）。
pub fn first_with_source(frames: &[Frame]) -> Option<&Frame> {
    frames.iter().find(|f| f.path.is_some() && f.line > 0).or(frames.first())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

pub fn threads(body: &Value) -> Vec<Thread> {
    body["threads"]
        .as_array()
        .map(|list| list.iter().map(|t| Thread { id: t["id"].as_i64().unwrap_or(0), name: t["name"].as_str().unwrap_or("").to_string() }).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_without_source_are_skipped_for_the_first_view() {
        let body = json!({ "stackFrames": [
            { "id": 1, "name": "System.Threading.Monitor.Wait", "line": 0 },
            { "id": 2, "name": "Program.Main", "line": 12, "source": { "path": "C:\\a\\Program.cs", "origin": "decompiled" }, "netdbgMethodLines": { "first": 9, "last": 20 } }
        ] });
        let list = frames(&body);
        assert_eq!(list[0].path, None);
        assert_eq!(first_with_source(&list).map(|f| f.id), Some(2));
        assert_eq!(list[1].origin.as_deref(), Some("decompiled"));
        assert_eq!((list[0].method_lines, list[1].method_lines), (None, Some((9, 20))));
        assert_eq!(threads(&json!({ "threads": [{ "id": 3, "name": "Main Thread" }] }))[0].name, "Main Thread");
    }
}
