//! 行のブレークポイント（ファイルごと）。DAP の `setBreakpoints` は、ファイルの全部を毎回送り直す。
//!
//! Windows のパスは大文字と小文字を区別しないので、小文字にしたものを鍵にする。

use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub line: u32,
    /// エンジンが置けたか（まだ答えが無ければ `None`）。
    pub verified: Option<bool>,
    /// 置けなかった理由など。
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakpoints {
    files: BTreeMap<String, (String, Vec<Line>)>,
}

fn key(path: &str) -> String {
    path.to_lowercase()
}

impl Breakpoints {
    /// 行のブレークポイントを置くか外す。そのファイルの、送り直す行の並びを返す。
    pub fn toggle(&mut self, path: &str, line: u32) -> Vec<u32> {
        let (_, lines) = self.files.entry(key(path)).or_insert_with(|| (path.to_string(), Vec::new()));
        match lines.iter().position(|l| l.line == line) {
            Some(at) => {
                lines.remove(at);
            }
            None => {
                lines.push(Line { line, verified: None, message: None });
                lines.sort_by_key(|l| l.line);
            }
        }
        lines.iter().map(|l| l.line).collect()
    }

    /// `setBreakpoints` の答え（送った順と同じ順）。エンジンが行を動かしたら（文の始まりへ）、その行にする。
    pub fn apply(&mut self, path: &str, body: &Value) {
        let Some((_, lines)) = self.files.get_mut(&key(path)) else { return };
        let answers = body["breakpoints"].as_array().cloned().unwrap_or_default();
        for (line, answer) in lines.iter_mut().zip(answers.iter()) {
            line.verified = answer["verified"].as_bool();
            line.message = answer["message"].as_str().map(String::from);
            if let Some(moved) = answer["line"].as_u64().and_then(|l| u32::try_from(l).ok()) {
                line.line = moved;
            }
        }
        lines.sort_by_key(|l| l.line);
        lines.dedup_by_key(|l| l.line);
    }

    pub fn lines(&self, path: &str) -> Vec<Line> {
        self.files.get(&key(path)).map(|(_, lines)| lines.clone()).unwrap_or_default()
    }

    /// すべてのファイル（新しいセッションの始めに、まとめて送る）。
    pub fn files(&self) -> Vec<(String, Vec<u32>)> {
        self.files.values().filter(|(_, lines)| !lines.is_empty()).map(|(path, lines)| (path.clone(), lines.iter().map(|l| l.line).collect())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn toggling_adds_and_removes_in_line_order() {
        let mut b = Breakpoints::default();
        assert_eq!(b.toggle("C:\\A.cs", 9), vec![9]);
        assert_eq!(b.toggle("c:\\a.cs", 3), vec![3, 9], "paths are not case sensitive");
        assert_eq!(b.toggle("C:\\A.cs", 9), vec![3]);
        assert_eq!(b.files(), vec![("C:\\A.cs".to_string(), vec![3])]);
    }

    #[test]
    fn answers_mark_and_move_lines() {
        let mut b = Breakpoints::default();
        b.toggle("C:\\A.cs", 4);
        b.toggle("C:\\A.cs", 10);
        b.apply("C:\\A.cs", &json!({ "breakpoints": [{ "verified": true, "line": 5 }, { "verified": false, "message": "no code" }] }));
        let lines = b.lines("C:\\A.cs");
        assert_eq!((lines[0].line, lines[0].verified), (5, Some(true)));
        assert_eq!((lines[1].line, lines[1].message.as_deref()), (10, Some("no code")));
    }
}
