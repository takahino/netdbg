//! 差し替わった exe の差分（B2、D-080）: `netdbg.diffAssemblies` の答えを、一覧の行にする。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// `changed`、`added`、`removed`。
    pub kind: String,
    /// `Class.Method(引数)`。
    pub method: String,
    pub assembly: String,
    /// 試験で通ったか（カバレッジと重ねたときだけ）。
    pub executed: Option<bool>,
    pub retest: bool,
    /// 押したときに開くソースと行。
    pub source: Option<(String, u32)>,
}

impl Row {
    /// 種類の文言。
    pub fn kind_text(&self) -> &'static str {
        match self.kind.as_str() {
            "changed" => crate::i18n::tr("Changed"),
            "added" => crate::i18n::tr("Added"),
            _ => crate::i18n::tr("Removed"),
        }
    }

    /// 試験の印（● 通った、○ 通っていない＝再試験）と、その説明。
    pub fn mark(&self) -> (&'static str, &'static str) {
        match self.executed {
            Some(true) => ("●", crate::i18n::tr("Run in the tests")),
            Some(false) => ("○", crate::i18n::tr("Retest scope (changed but not run in the tests)")),
            None => ("", ""),
        }
    }
}

/// 答えから行を作る。`retest_only` なら、再試験の範囲だけ。
pub fn rows(body: &Value, retest_only: bool) -> Vec<Row> {
    body["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Row {
            kind: c["kind"].as_str().unwrap_or("").to_string(),
            method: format!("{}.{}{}", c["class"].as_str().unwrap_or(""), c["name"].as_str().unwrap_or(""), c["signature"].as_str().unwrap_or("")),
            assembly: c["assembly"].as_str().unwrap_or("").to_string(),
            executed: c["executed"].as_bool(),
            retest: c["retest"].as_bool().unwrap_or(false),
            source: c["source"]["path"].as_str().map(|p| (p.to_string(), c["line"].as_u64().and_then(|l| u32::try_from(l).ok()).unwrap_or(1))),
        })
        .filter(|row| !retest_only || row.retest)
        .collect()
}

/// 要約の文言（`変わった 1、足された 1、消えた 0、再試験の範囲 2`）。
pub fn summary(body: &Value) -> String {
    let s = &body["summary"];
    let count = |key: &str| s[key].as_u64().unwrap_or(0);
    let mut text = crate::i18n::trf("Changed {changed}, added {added}, removed {removed}", &[("changed", count("changed").to_string()), ("added", count("added").to_string()), ("removed", count("removed").to_string())]);
    if body["coverage"] == true {
        text.push_str(&crate::i18n::trf(", retest scope {n}", &[("n", count("retest").to_string())]));
    } else {
        text.push_str(crate::i18n::tr(" (coverage was not collected, so there is no retest scope)"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body() -> Value {
        json!({
            "summary": { "changed": 1, "added": 1, "removed": 1, "retest": 1 },
            "coverage": true,
            "changes": [
                { "kind": "changed", "assembly": "App", "class": "P", "name": "Adjust", "signature": "(int)", "executed": false, "retest": true, "source": { "path": "C:\\d\\P.cs" }, "line": 12 },
                { "kind": "added", "assembly": "App", "class": "P", "name": "Label", "signature": "()", "executed": true, "retest": false },
                { "kind": "removed", "assembly": "App", "class": "P", "name": "Old", "signature": "()" },
            ]
        })
    }

    #[test]
    fn rows_carry_the_mark_and_the_place() {
        let rows = rows(&body(), false);
        assert_eq!(rows[0].method, "P.Adjust(int)");
        assert_eq!(rows[0].mark().0, "○");
        assert_eq!(rows[0].source, Some(("C:\\d\\P.cs".into(), 12)));
        assert_eq!(rows[1].mark().0, "●");
        assert_eq!((rows[2].kind_text(), rows[2].mark().0), ("消えた", ""));
    }

    #[test]
    fn only_the_retest_range_can_be_shown() {
        assert_eq!(rows(&body(), true).iter().map(|r| r.method.as_str()).collect::<Vec<_>>(), ["P.Adjust(int)"]);
    }

    #[test]
    fn the_summary_says_when_there_is_no_coverage() {
        assert_eq!(summary(&body()), "変わった 1、足された 1、消えた 1、再試験の範囲 1");
        let mut without = body();
        without["coverage"] = json!(false);
        assert!(summary(&without).contains("カバレッジを取っていない"));
    }
}
