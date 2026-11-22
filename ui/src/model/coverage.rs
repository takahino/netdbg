//! カバレッジ（B1、D-079）: 行ごとの印と、分かれ道の向きの文言。エンジンの `netdbg.coverage { file }` の答えから作る。
//!
//! 印は 3 つ: 通った、通っていない、分岐が一部（行は通ったが、数える分かれ道の向きのうち通っていないものがある）。

use serde_json::Value;
use std::collections::HashMap;

/// 合計（`netdbg.coverageChanged` / `netdbg.coverage` の本文）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    pub lines_covered: u64,
    pub lines_valid: u64,
    pub branches_covered: u64,
    pub branches_valid: u64,
    pub uncountable: u64,
    /// 書き出したファイル。
    pub path: Option<String>,
}

impl Totals {
    pub fn from_body(body: &Value) -> Totals {
        let number = |key: &str| body[key].as_u64().unwrap_or(0);
        Totals {
            lines_covered: number("linesCovered"),
            lines_valid: number("linesValid"),
            branches_covered: number("branchesCovered"),
            branches_valid: number("branchesValid"),
            uncountable: number("uncountableBranches"),
            path: body["path"].as_str().map(String::from),
        }
    }

    /// 上の帯の文言（`行 62% / 分岐 40%`）。
    pub fn summary(&self) -> String {
        crate::i18n::trf("Lines {lines} / branches {branches}", &[("lines", percent(self.lines_covered, self.lines_valid)), ("branches", percent(self.branches_covered, self.branches_valid))])
    }
}

/// 率（数えるものが無ければ `-`）。
pub fn percent(covered: u64, valid: u64) -> String {
    if valid == 0 { "-".into() } else { format!("{}%", covered * 100 / valid) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Hit,
    Missed,
    Partial,
}

impl Mark {
    /// 行の番号の横の印と、CSS の名前。
    pub fn symbol(self) -> &'static str {
        match self {
            Mark::Hit => "●",
            Mark::Missed => "○",
            Mark::Partial => "◐",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Mark::Hit => "cov-hit",
            Mark::Missed => "cov-miss",
            Mark::Partial => "cov-part",
        }
    }
}

/// 分かれ道の向き 1 つ。
#[derive(Clone, Debug, PartialEq)]
pub struct Way {
    /// 2 つの向きの分岐の、条件の値（真の側か）。`switch` では `None`。
    pub value: Option<bool>,
    /// `switch` の case の番号（空ならどれにも当たらなかった側）。
    pub cases: Vec<u64>,
    pub seen: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub switch: bool,
    pub countable: bool,
    pub ways: Vec<Way>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub hit: bool,
    pub decisions: Vec<Decision>,
}

impl Line {
    pub fn mark(&self) -> Mark {
        if !self.hit {
            return Mark::Missed;
        }
        let partial = self.decisions.iter().any(|d| d.countable && d.ways.iter().any(|w| !w.seen));
        if partial { Mark::Partial } else { Mark::Hit }
    }

    /// 印に載せたときの文言（1 行目が要約、そのあと分かれ道ごと）。
    pub fn describe(&self) -> Vec<String> {
        let mut out = vec![if self.hit { crate::i18n::tr("Run") } else { crate::i18n::tr("Not run") }.to_string()];
        let countable: Vec<&Decision> = self.decisions.iter().filter(|d| d.countable).collect();
        if !countable.is_empty() {
            let valid: usize = countable.iter().map(|d| d.ways.len()).sum();
            let covered: usize = countable.iter().map(|d| d.ways.iter().filter(|w| w.seen).count()).sum();
            out[0] = crate::i18n::trf("{state} · branches {covered}/{valid}", &[("state", out[0].clone()), ("covered", covered.to_string()), ("valid", valid.to_string())]);
        }
        for (index, decision) in self.decisions.iter().enumerate() {
            out.push(crate::i18n::trf("Branch {n}: {ways}", &[("n", (index + 1).to_string()), ("ways", decision_text(decision))]));
        }
        out
    }
}

fn decision_text(decision: &Decision) -> String {
    if !decision.countable {
        return crate::i18n::tr("Not countable (the ways cannot be told apart)").into();
    }
    if decision.switch {
        return decision
            .ways
            .iter()
            .map(|w| {
                let name = if w.cases.is_empty() { "default".to_string() } else { w.cases.iter().map(|c| format!("case {c}")).collect::<Vec<_>>().join(", ") };
                format!("{name} {}", if w.seen { "✓" } else { "✗" })
            })
            .collect::<Vec<_>>()
            .join(" / ");
    }
    let seen: Vec<&Way> = decision.ways.iter().filter(|w| w.seen).collect();
    match seen.as_slice() {
        [] => crate::i18n::tr("Neither way").into(),
        [one] if decision.ways.len() > 1 => match one.value {
            Some(true) => crate::i18n::tr("One way only (true only)").into(),
            Some(false) => crate::i18n::tr("One way only (false only)").into(),
            None => crate::i18n::tr("One way only").into(),
        },
        _ => crate::i18n::tr("Both ways ✓").into(),
    }
}

/// 開いているファイル 1 つの、行ごとの状態。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileCoverage {
    pub path: String,
    pub lines: HashMap<u32, Line>,
}

impl FileCoverage {
    /// `netdbg.coverage { file }` の答えから。
    pub fn from_body(path: &str, body: &Value) -> FileCoverage {
        let lines = body["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|line| {
                let number = u32::try_from(line["line"].as_u64()?).ok()?;
                let decisions = line["decisions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|d| Decision {
                        switch: d["kind"] == "switch",
                        countable: d["countable"].as_bool().unwrap_or(false),
                        ways: d["outcomes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|o| Way { value: o["value"].as_bool(), cases: o["cases"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect(), seen: o["seen"].as_bool().unwrap_or(false) })
                            .collect(),
                    })
                    .collect();
                Some((number, Line { hit: line["hit"].as_bool().unwrap_or(false), decisions }))
            })
            .collect();
        FileCoverage { path: path.to_string(), lines }
    }
}

/// 変わったファイルの一覧に、開いているファイルがあるか（読み直すか）。
pub fn changed(files: &[String], open: &str) -> bool {
    files.iter().any(|file| same_path(file, open))
}

fn same_path(a: &str, b: &str) -> bool {
    a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"))
}

/// ファイルごとの率（ソースのタブ。`netdbg.coverage { files: true }` の答え）。キーは比べる形のパス。
pub fn file_rates(body: &Value) -> HashMap<String, (u64, u64)> {
    body["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f["path"].as_str()?.replace('/', "\\").to_lowercase(), (f["linesCovered"].as_u64()?, f["linesValid"].as_u64()?))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file() -> FileCoverage {
        FileCoverage::from_body(
            "C:\\a\\P.cs",
            &json!({ "lines": [
                { "line": 10, "hit": true, "branchesCovered": 2, "branchesValid": 4, "decisions": [
                    { "kind": "jump", "countable": true, "outcomes": [{ "value": false, "seen": true }, { "value": true, "seen": false }] },
                    { "kind": "jump", "countable": true, "outcomes": [{ "value": true, "seen": true }, { "value": false, "seen": true }] },
                    { "kind": "jump", "countable": false, "outcomes": [{ "value": true, "seen": false }, { "value": false, "seen": false }] },
                ] },
                { "line": 11, "hit": false },
                { "line": 12, "hit": true },
                { "line": 13, "hit": true, "decisions": [{ "kind": "switch", "countable": true, "outcomes": [{ "cases": [0, 2], "seen": true }, { "cases": [], "seen": false }] }] },
            ] }),
        )
    }

    #[test]
    fn lines_are_marked_hit_missed_or_partial() {
        let f = file();
        assert_eq!(f.lines[&10].mark(), Mark::Partial);
        assert_eq!(f.lines[&11].mark(), Mark::Missed);
        assert_eq!(f.lines[&12].mark(), Mark::Hit);
        assert!(!f.lines.contains_key(&14), "コードの無い行には印を付けない");
    }

    #[test]
    fn the_hover_tells_each_branch() {
        let f = file();
        assert_eq!(f.lines[&10].describe(), ["通った ・ 分岐 3/4", "分かれ道 1: 片方だけ（偽の側だけ）", "分かれ道 2: 両方 ✓", "分かれ道 3: 数えられない（向きを見分けられない形）"]);
        assert_eq!(f.lines[&13].describe()[1], "分かれ道 1: case 0, case 2 ✓ / default ✗");
        assert_eq!(f.lines[&11].describe(), ["通っていない"]);
    }

    #[test]
    fn only_a_changed_open_file_is_read_again() {
        assert!(changed(&["c:/a/p.cs".into()], "C:\\a\\P.cs"));
        assert!(!changed(&["C:\\a\\Q.cs".into()], "C:\\a\\P.cs"));
    }

    #[test]
    fn totals_read_as_percentages() {
        let t = Totals::from_body(&json!({ "linesCovered": 62, "linesValid": 100, "branchesCovered": 0, "branchesValid": 0 }));
        assert_eq!(t.summary(), "行 62% / 分岐 -");
    }
}
