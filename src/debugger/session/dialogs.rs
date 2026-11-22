//! ダイアログを出した箇所（段階 12。障害の一連の流れの一つ）: `MessageBox.Show` と `ShowDialog` の入口で、表示する
//! 文字、題、フォームの型、利用者のコードの呼び出し元、時刻を、出来事の列に積む。文字や呼び出し元が条件に合えば、
//! ダイアログを出す前に止まる。
//!
//! プローブ（probes.rs）の上に載る。読むのは引数と `this` の型だけで、関数の評価はしない（だからプローブを置いた
//! WinForms / WPF のモジュールの最適化を切らない）。既定で入れる（ダイアログは人の操作の速さでしか出ない）。

use super::*;

/// 既定で見るメソッド（多重定義はすべて）。
const DEFAULT_METHODS: [&str; 5] = [
    "System.Windows.Forms.MessageBox.Show",
    "System.Windows.Forms.Form.ShowDialog",
    "System.Windows.Forms.CommonDialog.ShowDialog",
    "System.Windows.MessageBox.Show",
    "System.Windows.Window.ShowDialog",
];
/// 呼び出し元として探すフレームの数。
const CALLER_SEARCH: usize = 40;
/// 同じダイアログの記録が続いたとみなす間（ミリ秒）。`ShowDialog()` の中の `ShowDialog(owner)` を 1 つにする。
const SAME_DIALOG_MS: u64 = 200;

/// ダイアログで止める条件。書いた欄のすべてに合えば止まる（大文字と小文字は見ない）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct DialogCondition {
    /// 文字、題、フォームの型のどれかに含まれる文字。
    text: Option<String>,
    /// 呼び出し元のメソッドの名前に含まれる文字。
    caller: Option<String>,
}

impl DialogCondition {
    fn from_json(value: &Value) -> Option<DialogCondition> {
        let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase);
        let condition = DialogCondition { text: field("text"), caller: field("caller") };
        (condition != DialogCondition::default()).then_some(condition)
    }

    fn to_json(&self) -> Value {
        json!({ "text": self.text, "caller": self.caller })
    }

    fn matches(&self, shown: &[&str], caller: &str) -> bool {
        self.text.as_ref().is_none_or(|text| shown.iter().any(|s| s.to_lowercase().contains(text.as_str()))) && self.caller.as_ref().is_none_or(|c| caller.to_lowercase().contains(c.as_str()))
    }
}

impl Session {
    /// ダイアログの記録を入れるか切る（プローブを置き直す）。
    pub(super) fn configure_dialog_log(&mut self, enabled: bool) {
        let names = if enabled { DEFAULT_METHODS.iter().map(|m| m.to_string()).collect() } else { Vec::new() };
        self.replace_probes(probes::Feature::Dialog, names);
    }

    /// `netdbg.setDialogLog { enabled?, breakOn?: [{ text?, caller? }] }`: 記録を入れる / 切る、止める条件を置き換える。
    pub fn set_dialog_log(&mut self, arguments: &Value) -> Result<Value, String> {
        if let Some(enabled) = arguments.get("enabled").and_then(Value::as_bool) {
            self.configure_dialog_log(enabled);
        }
        if let Some(items) = arguments.get("breakOn").and_then(Value::as_array) {
            self.dialog_breaks.clear();
            for item in items {
                let condition = DialogCondition::from_json(item).ok_or_else(|| tr!("Write text or caller in a stop condition (an empty condition stops on every dialog)."))?;
                self.dialog_breaks.push((self.next_breakpoint_id, condition));
                self.next_breakpoint_id += 1;
            }
            if !self.dialog_breaks.is_empty() && !self.probe_feature_on(probes::Feature::Dialog) {
                self.configure_dialog_log(true);
            }
        }
        let breaks: Vec<Value> = self.dialog_breaks.iter().map(|(id, c)| json!({ "id": id, "condition": c.to_json() })).collect();
        Ok(json!({ "enabled": self.probe_feature_on(probes::Feature::Dialog), "breakOn": breaks }))
    }


    /// ダイアログを出すメソッドの入口（止めた形にしてある）: 引数を読み、出来事の列に積む。条件に合えば止まる通知を用意する。
    pub(super) fn record_dialog(&mut self, thread: u32, method: &str) -> Vec<Event> {
        let described = self.describe_top_frames(thread, CALLER_SEARCH);
        // `ShowDialog()` は `ShowDialog(owner)` を呼ぶ。内側の呼び出しは記録しない。
        let inner = described.get(1).and_then(|f| f["name"].as_str()).is_some_and(|caller| DEFAULT_METHODS.iter().any(|m| probes::is_method(caller, m)));
        if inner {
            return Vec::new();
        }
        let caller = stack::user_caller(&described, 1);
        // 引数（名前はメタデータから。名前が引けなければ、文字列の引数を順に、文字、題とする）。
        let mut text = None;
        let mut caption = None;
        let mut form = None;
        let mut strings = Vec::new();
        for (name, value) in self.frame_variables(thread, 0).unwrap_or_default() {
            let shown = unsafe { value::describe(&value, &self.modules) };
            match name.as_str() {
                "this" => form = Some(shown.type_name),
                "text" | "messageBoxText" => text = Some(crate::debugger::sqllog::unquote(&shown.value)),
                "caption" => caption = Some(crate::debugger::sqllog::unquote(&shown.value)),
                _ if shown.type_name == "string" && shown.value != "null" => strings.push(crate::debugger::sqllog::unquote(&shown.value)),
                _ => {}
            }
        }
        let mut strings = strings.into_iter();
        let is_message_box = method.ends_with("MessageBox.Show");
        if is_message_box {
            text = text.or_else(|| strings.next());
            caption = caption.or_else(|| strings.next());
        }
        let mut body = json!({ "kind": if is_message_box { "messageBox" } else { "dialog" }, "method": method, "threadId": thread });
        for (key, value) in [("text", &text), ("caption", &caption), ("form", &form)] {
            if let Some(value) = value {
                body[key] = json!(value);
            }
        }
        if let Some(caller) = &caller {
            body["caller"] = caller.clone();
        }
        // `ShowDialog()` は `ShowDialog(owner)` を呼ぶ。x64 の JIT は末尾呼び出しにして外側のフレームを消すので、上の
        // 呼び出し元の見分けでは足りない。同じスレッド、同じメソッドとフォーム、同じ呼び出し元の記録がすぐ前にあれば、
        // 同じダイアログとして記録しない。
        if self.repeats_newest_record("dialog", SAME_DIALOG_MS, |last| ["threadId", "method", "form", "text", "caller"].iter().all(|key| last[*key] == body[*key])) {
            return Vec::new();
        }
        // 止める条件。
        let caller_name = caller.as_ref().and_then(|c| c["name"].as_str()).unwrap_or("").to_string();
        let shown: Vec<&str> = [&text, &caption, &form].into_iter().flatten().map(String::as_str).collect();
        let hit: Vec<i64> = self.dialog_breaks.iter().filter(|(_, c)| c.matches(&shown, &caller_name)).map(|(id, _)| *id).collect();
        let headline = match (&caption, &text, &form) {
            (Some(caption), Some(text), _) => format!("{caption}: {text}"),
            (None, Some(text), _) => text.clone(),
            (_, None, Some(form)) => form.clone(),
            _ => method.to_string(),
        };
        if !hit.is_empty() {
            body["stopped"] = json!(true);
            let description = format!("Dialog: {headline}");
            self.probes.pending_stop = Some(("stopped", json!({ "reason": "breakpoint", "threadId": thread, "allThreadsStopped": true, "hitBreakpointIds": hit, "description": description, "text": description })));
        }
        self.mark_environment(&mut body);
        let record = self.probes.journal.push("dialog", body).to_json();
        vec![("netdbg.dialogShown", record), ("output", json!({ "category": "console", "output": format!("Dialog: {headline}\n") }))]
    }
}
