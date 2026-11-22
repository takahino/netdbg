//! 変わったコントロール: 項目の始めと終わりに読んだ画面の木を比べ、文字、チェック、有効、表示の変化を出す。
//! 名前の無いコントロールは、窓と種類と並びの番号で見分ける。

use std::collections::HashMap;

/// コントロール 1 つの、比べる値。
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// トップレベルの窓の見出し。
    pub window: String,
    pub name: Option<String>,
    pub kind: String,
    pub text: Option<String>,
    pub checked: Option<bool>,
    pub enabled: bool,
    pub visible: bool,
}

/// 変化 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// `nameBox`（名前が無ければ `TextBox #2`）。
    pub control: String,
    /// 言語によらない種類: `text`、`checked`、`enabled`、`visible`、`appeared`、`disappeared`、`opened`、`closed`
    /// （見せる文は `text()` が、いまの言語で作る）。
    pub what: String,
    pub before: String,
    pub after: String,
}

impl Change {
    pub fn text(&self) -> String {
        let control = &self.control;
        match self.what.as_str() {
            "appeared" => tr!("{control}: appeared", control = control),
            "disappeared" => tr!("{control}: disappeared", control = control),
            "opened" => tr!("{control} opened", control = control),
            "closed" => tr!("{control} closed", control = control),
            what => {
                let what = match what {
                    "text" => tr_str!("text"),
                    "checked" => tr_str!("checked"),
                    "enabled" => tr_str!("enabled"),
                    "visible" => tr_str!("visible"),
                    other => other,
                };
                tr!("{control} {what}: \"{before}\" → \"{after}\"", control = control, what = what, before = self.before, after = self.after)
            }
        }
    }
}

/// 見分ける鍵と、見せる名前。
fn keyed(controls: &[Control]) -> Vec<(String, String, &Control)> {
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    controls
        .iter()
        .map(|control| match &control.name {
            Some(name) => (format!("{}|{name}", control.window), name.clone(), control),
            None => {
                let count = counts.entry((control.window.clone(), control.kind.clone())).or_insert(0);
                *count += 1;
                (format!("{}|{}#{count}", control.window, control.kind), format!("{} #{count}", control.kind), control)
            }
        })
        .collect()
}

/// 始めと終わりの木を比べる。並びは終わりの木の順（消えたものは最後）。窓ごと開いた・閉じたものは、中のコントロールを
/// 並べず、窓 1 つの変化にする（ダイアログを開いた、など）。
pub fn diff(before: &[Control], after: &[Control]) -> Vec<Change> {
    let windows = |controls: &[Control]| controls.iter().map(|c| c.window.clone()).collect::<std::collections::BTreeSet<String>>();
    let (old_windows, new_windows) = (windows(before), windows(after));
    let window_change = |title: &String, what: &str| Change { control: tr!("the window \"{title}\"", title = title), what: what.into(), before: String::new(), after: String::new() };
    let before_keyed = keyed(before);
    let old: HashMap<&str, &Control> = before_keyed.iter().map(|(key, _, c)| (key.as_str(), *c)).collect();
    let after_keyed = keyed(after);
    let mut out: Vec<Change> = new_windows.difference(&old_windows).map(|title| window_change(title, "opened")).collect();
    let flag = |value: bool| if value { tr_str!("yes") } else { tr_str!("no") }.to_string();
    let check = |value: Option<bool>| value.map_or(String::new(), |v| if v { tr_str!("on") } else { tr_str!("off") }.to_string());
    for (key, label, now) in &after_keyed {
        if !old_windows.contains(&now.window) {
            continue;
        }
        let Some(was) = old.get(key.as_str()) else {
            out.push(Change { control: label.clone(), what: "appeared".into(), before: String::new(), after: String::new() });
            continue;
        };
        let mut push = |what: &str, before: String, after: String| {
            if before != after {
                out.push(Change { control: label.clone(), what: what.into(), before, after });
            }
        };
        // 文字は、読めたときだけ比べる（読めない＝止まっていた、を変化にしない）。
        if let (Some(before), Some(after)) = (&was.text, &now.text) {
            push("text", before.clone(), after.clone());
        }
        push("checked", check(was.checked), check(now.checked));
        push("enabled", flag(was.enabled), flag(now.enabled));
        push("visible", flag(was.visible), flag(now.visible));
    }
    let new_keys: std::collections::HashSet<&str> = after_keyed.iter().map(|(key, ..)| key.as_str()).collect();
    for (key, label, was) in &before_keyed {
        if new_windows.contains(&was.window) && !new_keys.contains(key.as_str()) {
            out.push(Change { control: label.clone(), what: "disappeared".into(), before: String::new(), after: String::new() });
        }
    }
    out.extend(old_windows.difference(&new_windows).map(|title| window_change(title, "closed")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(name: Option<&str>, kind: &str, text: Option<&str>) -> Control {
        Control { window: "Smoke GUI".into(), name: name.map(String::from), kind: kind.into(), text: text.map(String::from), checked: None, enabled: true, visible: true }
    }

    #[test]
    fn changed_text_and_state_are_found_by_name() {
        let before = vec![control(Some("nameBox"), "TextBox", Some("")), control(Some("resultLabel"), "Label", Some("")), control(Some("agreeBox"), "CheckBox", Some("Agree"))];
        let mut after = before.clone();
        after[0].text = Some("Taro".into());
        after[1].text = Some("Hello, Taro".into());
        after[2].checked = Some(true);
        after[2].enabled = false;
        let changes: Vec<String> = diff(&before, &after).iter().map(Change::text).collect();
        assert_eq!(changes, ["nameBox の文字: 「」→「Taro」", "resultLabel の文字: 「」→「Hello, Taro」", "agreeBox のチェック: 「」→「入」", "agreeBox の有効: 「はい」→「いいえ」"]);
    }

    #[test]
    fn unreadable_text_is_not_a_change_and_whole_windows_are_one_change() {
        let before = vec![control(Some("nameBox"), "TextBox", Some("a")), control(Some("hint"), "Label", Some("x"))];
        let mut after = vec![control(Some("nameBox"), "TextBox", None), control(Some("newBox"), "TextBox", Some(""))];
        after.push(Control { window: "About".into(), ..control(None, "Button", Some("Close")) });
        after.push(Control { window: "About".into(), ..control(Some("aboutLabel"), "Label", Some("1.0")) });
        let changes: Vec<String> = diff(&before, &after).iter().map(Change::text).collect();
        assert_eq!(changes, ["窓「About」が開いた", "newBox: 現れた", "hint: 消えた"]);
        assert_eq!(diff(&before, &[]).iter().map(Change::text).collect::<Vec<_>>(), ["窓「Smoke GUI」が閉じた"]);
    }

    #[test]
    fn changes_read_in_english_in_an_english_environment() {
        let before = vec![control(Some("resultLabel"), "Label", Some(""))];
        let mut after = before.clone();
        after[0].text = Some("Hello".into());
        after.push(Control { window: "About".into(), ..control(None, "Button", Some("Close")) });
        let changes: Vec<String> = crate::i18n::with_lang("en", || diff(&before, &after).iter().map(Change::text).collect());
        assert_eq!(changes, ["the window \"About\" opened", "resultLabel text: \"\" → \"Hello\""]);
    }
}
