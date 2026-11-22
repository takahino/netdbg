//! 例外の設定（どの例外で止まるか。D-085）。Visual Studio の「例外設定」と同じく、名前空間ごとの型の一覧から、
//! 投げられたらすぐ止まる型を選ぶ。誰も捕まえない例外では、設定に関係なく止まる（エンジンの決まり）。

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// よく使う例外の型（名前空間ごと）。拡張の `exceptionSettings.ts` の一覧と同じ。
pub const CATALOG: &[(&str, &[&str])] = &[
    (
        "System",
        &[
            "ArgumentException",
            "ArgumentNullException",
            "ArgumentOutOfRangeException",
            "InvalidOperationException",
            "NullReferenceException",
            "IndexOutOfRangeException",
            "InvalidCastException",
            "FormatException",
            "NotImplementedException",
            "NotSupportedException",
            "ObjectDisposedException",
            "OverflowException",
            "DivideByZeroException",
            "TimeoutException",
            "UnauthorizedAccessException",
            "OperationCanceledException",
            "AggregateException",
            "OutOfMemoryException",
        ],
    ),
    ("System.Collections.Generic", &["KeyNotFoundException"]),
    ("System.Threading.Tasks", &["TaskCanceledException"]),
    ("System.IO", &["IOException", "FileNotFoundException", "DirectoryNotFoundException", "PathTooLongException", "EndOfStreamException"]),
    ("System.Net", &["WebException"]),
    ("System.Net.Http", &["HttpRequestException"]),
    ("System.Net.Sockets", &["SocketException"]),
    ("System.Data", &["DataException", "DBConcurrencyException"]),
    ("System.Runtime.InteropServices", &["COMException", "SEHException", "ExternalException"]),
    ("System.ComponentModel", &["Win32Exception"]),
    ("System.Xml", &["XmlException"]),
    ("System.Text.Json", &["JsonException"]),
    ("System.Security", &["SecurityException"]),
];

/// 名前空間の型の、名前空間つきの名前。
pub fn types_of(namespace: &str) -> Vec<String> {
    CATALOG.iter().filter(|(n, _)| *n == namespace).flat_map(|(n, types)| types.iter().map(move |t| format!("{n}.{t}"))).collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExceptionSettings {
    /// 利用者のコードで投げられ、利用者のコードではない所で捕まえられる例外で止まる（フィルター `user-unhandled`）。
    pub user_unhandled: bool,
    /// 投げられたら、どの型でもすぐ止まる（フィルター `all`）。
    pub all: bool,
    /// 投げられたらすぐ止まる型（名前空間つき）。
    pub checked: BTreeSet<String>,
    /// 利用者が足した型（一覧に無いもの）。外しても一覧には残す。
    pub custom: Vec<String>,
}

impl ExceptionSettings {
    /// `setExceptionBreakpoints` の引数。型の条件は、エンジンの書き方（`A, B` はその型で止まる）。
    pub fn to_request(&self) -> Value {
        let mut filters = Vec::new();
        if self.user_unhandled {
            filters.push("user-unhandled");
        }
        if self.all {
            filters.push("all");
        }
        // 「すべて」のときは、型の条件は要らない（すべてで止まる）。
        let options = if self.all || self.checked.is_empty() {
            json!([])
        } else {
            json!([{ "filterId": "all", "condition": self.checked.iter().cloned().collect::<Vec<_>>().join(", ") }])
        };
        json!({ "filters": filters, "filterOptions": options })
    }

    /// 何かで止まる設定か（ツールバーの印）。
    pub fn is_set(&self) -> bool {
        self.user_unhandled || self.all || !self.checked.is_empty()
    }

    /// 型を選ぶか外す。
    pub fn set_checked(&mut self, types: &[String], on: bool) {
        for t in types {
            if on {
                self.checked.insert(t.clone());
            } else {
                self.checked.remove(t);
            }
        }
    }

    /// 利用者が型を足す（足した型は選んだ状態にする）。空や、名前に使えない文字を含むものは足さない。
    pub fn add_custom(&mut self, name: &str) -> bool {
        let name = name.trim();
        let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '+' | '`'));
        if !valid {
            return false;
        }
        let in_catalog = CATALOG.iter().any(|(n, types)| types.iter().any(|t| format!("{n}.{t}") == name));
        if !in_catalog && !self.custom.iter().any(|c| c == name) {
            self.custom.push(name.to_string());
        }
        self.checked.insert(name.to_string());
        true
    }

    /// 足した型を一覧から消す。
    pub fn remove_custom(&mut self, name: &str) {
        self.custom.retain(|c| c != name);
        self.checked.remove(name);
    }

    /// ブラウザに覚える形。
    pub fn to_json(&self) -> String {
        json!({ "userUnhandled": self.user_unhandled, "all": self.all, "checked": self.checked, "custom": self.custom }).to_string()
    }

    /// 覚えておいた形から（読めなければ既定）。
    pub fn from_json(text: &str) -> ExceptionSettings {
        let Ok(value) = serde_json::from_str::<Value>(text) else { return ExceptionSettings::default() };
        let strings = |key: &str| value[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string);
        ExceptionSettings { user_unhandled: value["userUnhandled"] == true, all: value["all"] == true, checked: strings("checked").collect(), custom: strings("custom").collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_set_sends_no_filters() {
        assert_eq!(ExceptionSettings::default().to_request(), json!({ "filters": [], "filterOptions": [] }));
    }

    #[test]
    fn checked_types_become_the_condition_of_the_all_filter() {
        let mut settings = ExceptionSettings { user_unhandled: true, ..Default::default() };
        settings.set_checked(&types_of("System.IO"), true);
        settings.set_checked(&["System.IO.IOException".to_string()], false);
        let request = settings.to_request();
        assert_eq!(request["filters"], json!(["user-unhandled"]));
        assert_eq!(request["filterOptions"], json!([{ "filterId": "all", "condition": "System.IO.DirectoryNotFoundException, System.IO.EndOfStreamException, System.IO.FileNotFoundException, System.IO.PathTooLongException" }]));
    }

    #[test]
    fn all_needs_no_condition() {
        let mut settings = ExceptionSettings { all: true, ..Default::default() };
        settings.set_checked(&["System.FormatException".to_string()], true);
        assert_eq!(settings.to_request(), json!({ "filters": ["all"], "filterOptions": [] }));
    }

    #[test]
    fn custom_types_are_added_checked_and_removed() {
        let mut settings = ExceptionSettings::default();
        assert!(settings.add_custom(" MyApp.OrderException "));
        assert!(settings.add_custom("System.FormatException"), "一覧にある型は選ぶだけ");
        assert!(!settings.add_custom("a b") && !settings.add_custom(""));
        assert_eq!(settings.custom, vec!["MyApp.OrderException"]);
        assert!(settings.checked.contains("MyApp.OrderException") && settings.checked.contains("System.FormatException"));
        settings.remove_custom("MyApp.OrderException");
        assert!(settings.custom.is_empty() && !settings.checked.contains("MyApp.OrderException"));
    }

    #[test]
    fn settings_survive_the_browser_storage() {
        let mut settings = ExceptionSettings { user_unhandled: true, ..Default::default() };
        settings.add_custom("MyApp.OrderException");
        assert_eq!(ExceptionSettings::from_json(&settings.to_json()), settings);
        assert_eq!(ExceptionSettings::from_json("not json"), ExceptionSettings::default());
    }
}
