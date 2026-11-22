//! 読み込んだソースの一覧（DAP の `loadedSources` の答え）。止まる前にソースを開いて、ブレークポイントを置くため。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// 逆コンパイルした C# か。
    pub decompiled: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    pub entries: Vec<Entry>,
    /// 逆コンパイルがまだのアプリのモジュール（頼めばソースになる）。
    pub pending: Vec<String>,
}

pub fn parse(body: &Value) -> Sources {
    let entries = body["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let path = s["path"].as_str()?.to_string();
            let name = s["name"].as_str().map(String::from).unwrap_or_else(|| crate::model::paths::file_name(&path).to_string());
            Some(Entry { name, path, decompiled: s["origin"] == "decompiled" })
        })
        .collect();
    let pending = body["netdbgPendingDecompile"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    Sources { entries, pending }
}

impl Sources {
    /// 名前かパスに `filter` を含むもの（大文字と小文字は区別しない）。
    pub fn matching(&self, filter: &str) -> Vec<Entry> {
        let filter = filter.trim().to_lowercase();
        self.entries.iter().filter(|e| filter.is_empty() || e.path.to_lowercase().contains(&filter)).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sources_are_filtered_by_name_or_path() {
        let sources = parse(&json!({
            "sources": [
                { "name": "Form1.cs", "path": "C:\\app\\Form1.cs" },
                { "name": "Program.cs", "path": "C:\\cache\\x\\Program.cs", "origin": "decompiled" }
            ],
            "netdbgPendingDecompile": ["C:\\app\\Lib.dll"]
        }));
        assert_eq!(sources.pending, vec!["C:\\app\\Lib.dll".to_string()]);
        assert!(sources.entries[1].decompiled);
        assert_eq!(sources.matching("form").len(), 1);
        assert_eq!(sources.matching("CACHE")[0].name, "Program.cs");
        assert_eq!(sources.matching(" ").len(), 2);
    }
}
