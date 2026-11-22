//! 人の手の操作を、成績書の言葉にする（「ボタン greetButton（Greet）を押した」「nameBox に「Taro」を入れた」）。
//!
//! 入力（クリックとキー）は、フック（`gui::recorder`）が受け、押した・打った先のコントロールを読んでから渡す。ここは
//! Win32 を知らない。文字の入力は 1 打ずつではなく、同じコントロールに続けて打った分をまとめ、フォーカスが移るか、
//! Enter / Tab か、項目の終わりで、そのときの文字を 1 つの操作にする。パスワードの欄は文字を残さない。

use super::Spot;
use std::collections::HashMap;

/// 入力を受けたコントロール（読んだときの状態）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Target {
    /// 窓のハンドル（同じコントロールかを見分ける）。
    pub id: usize,
    pub name: Option<String>,
    /// `Button`、`CheckBox`、`TextBox` など（`gui::kind_of`）。
    pub kind: String,
    pub text: Option<String>,
    pub checked: Option<bool>,
    /// パスワードの欄（`ES_PASSWORD`）。
    pub password: bool,
    /// 画面の上の範囲（成績書で、操作した箇所を塗る）。
    pub rect: Option<Spot>,
    /// トップレベルの窓（塗る画面に、その窓が写っているかを見る。0 は不明）。
    pub window: usize,
}

/// 操作 1 つ（言葉と、操作した箇所とその窓、流せる手順。D-086）。
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub text: String,
    pub spot: Option<Spot>,
    pub window: Option<usize>,
    /// もう一度流すときの手順（`report::steps`）。流せない操作（名前も文字も無い、座標だけ、パスワード）は `None`。
    pub step: Option<serde_json::Value>,
}

impl Recorded {
    fn on(target: &Target, text: String) -> Recorded {
        Recorded { text, spot: target.rect, window: (target.window != 0).then_some(target.window), step: None }
    }

    /// 手順を付ける（相手を名前か文字で指せるときだけ）。
    fn replayed_by(mut self, target: &Target, tool: &str, extra: serde_json::Value) -> Recorded {
        self.step = super::steps::target(target.name.as_deref(), target.text.as_deref()).map(|t| super::steps::step(tool, Some(t), extra));
        self
    }
}

impl Target {
    /// 操作の言葉に出す名前（名前が無ければ文字、それも無ければ種類）。
    fn label(&self) -> String {
        self.name.clone().filter(|n| !n.is_empty()).or_else(|| self.text.clone().filter(|t| !t.is_empty()).map(|t| tr!("\"{text}\"", text = t))).unwrap_or_else(|| self.kind.clone())
    }

    fn is_text_input(&self) -> bool {
        matches!(self.kind.as_str(), "TextBox" | "RichTextBox")
    }
}

/// キーの種類。
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    /// 文字を打つ・消す・動かすキー（入力のまとめに入る）。
    Edit,
    Enter,
    Tab,
    /// そのほかの名前のあるキー（`Esc`、`F5`、`Ctrl+S`）。
    Named(String),
}

/// 入力 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// マウスの左ボタンで押した（押したあとの状態を読んだもの）。
    Click(Target),
    /// キーを押した（フォーカスのあるコントロール）。
    Key { key: Key, target: Target },
}

/// 入力の並びを、操作の言葉にする。
#[derive(Default)]
pub struct Interpreter {
    /// 文字を打っている途中のコントロールと、打ち始める前の文字。
    editing: Option<(Target, Option<String>)>,
    /// 選んだ値を最後に見たときの文字（コンボボックスは、開くための押しでは変わらない）。
    seen: HashMap<usize, Option<String>>,
}

impl Interpreter {
    /// 入力 1 つ。`reread` は、コントロールのいまの状態を読み直す（打ち終えた文字を読む）。
    pub fn feed(&mut self, input: Input, reread: &mut dyn FnMut(usize) -> Option<Target>) -> Vec<Recorded> {
        let mut out = Vec::new();
        match input {
            Input::Click(target) => {
                if self.editing.as_ref().is_some_and(|(editing, _)| editing.id != target.id) {
                    out.extend(self.finish(reread));
                }
                out.extend(self.click(target));
            }
            Input::Key { key: Key::Edit, target } => {
                if self.editing.as_ref().is_some_and(|(editing, _)| editing.id != target.id) {
                    out.extend(self.finish(reread));
                }
                if self.editing.is_none() {
                    let before = self.seen.get(&target.id).cloned().unwrap_or_else(|| target.text.clone());
                    self.editing = Some((target, before));
                }
            }
            Input::Key { key: Key::Tab, .. } => out.extend(self.finish(reread)),
            Input::Key { key: Key::Enter, target } => {
                out.extend(self.finish(reread));
                out.push(Recorded::on(&target, tr!("Pressed Enter")).replayed_by(&target, "ui_keys", serde_json::json!({ "keys": "{Enter}" })));
            }
            Input::Key { key: Key::Named(name), target } => {
                out.extend(self.finish(reread));
                let recorded = Recorded::on(&target, tr!("Pressed {key}", key = name));
                // 名前の分からないキー（VKxx）は、流せない。
                let recorded = if name.starts_with("VK") || name.contains("+VK") { recorded } else { recorded.replayed_by(&target, "ui_keys", serde_json::json!({ "keys": format!("{{{name}}}") })) };
                out.push(recorded);
            }
        }
        out
    }

    /// 打っている途中の入力を、1 つの操作にする（フォーカスが移った、項目を終える）。
    pub fn finish(&mut self, reread: &mut dyn FnMut(usize) -> Option<Target>) -> Vec<Recorded> {
        let Some((target, before)) = self.editing.take() else { return Vec::new() };
        let now = reread(target.id).unwrap_or_else(|| target.clone());
        self.seen.insert(now.id, now.text.clone());
        if now.text == before {
            return Vec::new();
        }
        let label = target.label();
        let text = now.text.clone().unwrap_or_default();
        let words = joined(if target.password {
            tr!("Typed text into {control} (hidden)", control = label)
        } else if target.is_text_input() {
            tr!("Typed \"{text}\" into {control}", control = label, text = text)
        } else {
            tr!("Set {control} to \"{text}\"", control = label, text = text)
        });
        // 箇所は読み直した分（打っている間に窓が動いたなら、いまの場所）。パスワードは手順にも残さない。
        let recorded = Recorded { spot: now.rect.or(target.rect), ..Recorded::on(&target, words) };
        let recorded = if target.password { recorded } else { recorded.replayed_by(&target, "ui_set_text", serde_json::json!({ "value": text })) };
        vec![recorded]
    }

    fn click(&mut self, target: Target) -> Vec<Recorded> {
        let label = target.label();
        let text = target.text.clone().unwrap_or_default();
        let with_text = if !text.is_empty() && target.name.as_deref().is_some_and(|n| n != text) { tr!("{name} ({text})", name = label, text = text) } else { label.clone() };
        let (words, tool, extra) = match target.kind.as_str() {
            "Button" => (tr!("Pressed the button {control}", control = with_text), Some("ui_invoke"), serde_json::json!({})),
            "CheckBox" if target.checked == Some(true) => (tr!("Checked the check box {control}", control = with_text), Some("ui_toggle"), serde_json::json!({ "checked": true })),
            "CheckBox" => (tr!("Unchecked the check box {control}", control = with_text), Some("ui_toggle"), serde_json::json!({ "checked": false })),
            "RadioButton" => (tr!("Chose the radio button {control}", control = with_text), Some("ui_invoke"), serde_json::json!({})),
            // 選んだ値が変わったときだけ（開くための押しでは変わらない）。
            "ComboBox" | "ListBox" => {
                let previous = self.seen.insert(target.id, target.text.clone());
                if previous.is_none() || previous == Some(target.text.clone()) {
                    return Vec::new();
                }
                (tr!("Chose \"{text}\" in {control}", control = label, text = text), Some("ui_select"), serde_json::json!({ "item": text }))
            }
            // 押した位置で意味の変わるもの（一覧の行、タブ）は、名前だけでは流せない。
            "ListView" | "TreeView" | "TabControl" | "DateTimePicker" | "UpDown" | "TrackBar" | "Control" => (tr!("Clicked {control}", control = with_text), None, serde_json::json!({})),
            // 文字の欄、ラベル、窓そのものへの押しは、操作として残さない（入力はキーで残る）。
            _ => {
                self.seen.entry(target.id).or_insert(target.text);
                return Vec::new();
            }
        };
        let recorded = Recorded::on(&target, joined(words));
        // コンボボックスとリストは、選んだ項目の文字（`text`）ではなく名前で指す。
        let recorded = match tool {
            Some(tool) if matches!(target.kind.as_str(), "ComboBox" | "ListBox") => {
                let by_name = Target { text: None, ..target.clone() };
                recorded.replayed_by(&by_name, tool, extra)
            }
            Some(tool) => recorded.replayed_by(&target, tool, extra),
            None => recorded,
        };
        vec![recorded]
    }
}

/// 訳の文の、名前と続く言葉の間の空白を整える（日本語の訳は「{名前} を押した」と書き、名前が括弧やかぎ括弧で
/// 終われば空白を詰める。英語には全角の括弧が無いので変わらない）。
fn joined(text: String) -> String {
    text.replace("） ", "）").replace("」 ", "」")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: usize, name: &str, kind: &str, text: &str) -> Target {
        Target { id, name: Some(name.into()), kind: kind.into(), text: Some(text.into()), ..Target::default() }
    }

    fn no_reread(_: usize) -> Option<Target> {
        None
    }

    /// 言葉だけを比べる。
    impl PartialEq<&str> for Recorded {
        fn eq(&self, other: &&str) -> bool {
            self.text == *other
        }
    }

    #[test]
    fn the_operation_carries_the_spot_of_its_control() {
        let mut interpreter = Interpreter::default();
        let spot = Spot { left: 10, top: 20, right: 90, bottom: 44 };
        let button = Target { rect: Some(spot), ..target(1, "greetButton", "Button", "Greet") };
        assert_eq!(interpreter.feed(Input::Click(button), &mut no_reread)[0].spot, Some(spot));
        let moved = Spot { left: 50, ..spot };
        let mut reread = |id: usize| Some(Target { rect: Some(moved), ..target(id, "nameBox", "TextBox", "Taro") });
        interpreter.feed(Input::Key { key: Key::Edit, target: Target { rect: Some(spot), ..target(3, "nameBox", "TextBox", "") } }, &mut reread);
        assert_eq!(interpreter.finish(&mut reread)[0].spot, Some(moved), "打ち終えたときの場所");
    }

    #[test]
    fn buttons_and_check_boxes_are_named_with_their_state() {
        let mut interpreter = Interpreter::default();
        let mut reread = no_reread;
        assert_eq!(interpreter.feed(Input::Click(target(1, "greetButton", "Button", "Greet")), &mut reread), ["ボタン greetButton（Greet）を押した"]);
        let mut agree = target(2, "agreeBox", "CheckBox", "Agree");
        agree.checked = Some(true);
        assert_eq!(interpreter.feed(Input::Click(agree), &mut reread), ["チェックボックス agreeBox（Agree）を入にした"]);
        assert!(interpreter.feed(Input::Click(target(3, "nameBox", "TextBox", "")), &mut reread).is_empty(), "文字の欄への押しは残さない");
    }

    #[test]
    fn typing_is_one_operation_with_the_final_text() {
        let mut interpreter = Interpreter::default();
        let mut reread = |id: usize| Some(target(id, "nameBox", "TextBox", "Taro"));
        let name = target(3, "nameBox", "TextBox", "");
        for _ in 0..4 {
            assert!(interpreter.feed(Input::Key { key: Key::Edit, target: name.clone() }, &mut reread).is_empty());
        }
        // 別のコントロールを押すと、打っていた分が 1 つの操作になる。
        let out = interpreter.feed(Input::Click(target(1, "greetButton", "Button", "Greet")), &mut reread);
        assert_eq!(out, ["nameBox に「Taro」を入れた", "ボタン greetButton（Greet）を押した"]);
    }

    #[test]
    fn passwords_are_not_kept_and_named_keys_are_recorded() {
        let mut interpreter = Interpreter::default();
        let mut password = target(4, "passwordBox", "TextBox", "");
        password.password = true;
        let mut reread = |id: usize| Some(Target { password: true, ..target(id, "passwordBox", "TextBox", "secret") });
        interpreter.feed(Input::Key { key: Key::Edit, target: password.clone() }, &mut reread);
        let out = interpreter.feed(Input::Key { key: Key::Enter, target: password }, &mut reread);
        assert_eq!(out, ["passwordBox に文字を入れた（伏せ字）", "Enter を押した"]);
        assert!(!out.iter().any(|o| o.text.contains("secret")));
        assert_eq!(interpreter.feed(Input::Key { key: Key::Named("Ctrl+S".into()), target: target(5, "form", "Form", "") }, &mut reread), ["Ctrl+S を押した"]);
    }

    #[test]
    fn operations_read_in_english_in_an_english_environment() {
        crate::i18n::with_lang("en", || {
            let mut interpreter = Interpreter::default();
            assert_eq!(interpreter.feed(Input::Click(target(1, "greetButton", "Button", "Greet")), &mut no_reread), ["Pressed the button greetButton (Greet)"]);
            let mut reread = |id: usize| Some(target(id, "nameBox", "TextBox", "Taro"));
            interpreter.feed(Input::Key { key: Key::Edit, target: target(3, "nameBox", "TextBox", "") }, &mut reread);
            assert_eq!(interpreter.finish(&mut reread), ["Typed \"Taro\" into nameBox"]);
        });
    }

    #[test]
    fn a_combo_box_records_only_a_changed_selection() {
        let mut interpreter = Interpreter::default();
        let mut reread = no_reread;
        assert!(interpreter.feed(Input::Click(target(6, "colorBox", "ComboBox", "Red")), &mut reread).is_empty(), "開くための押し");
        assert_eq!(interpreter.feed(Input::Click(target(6, "colorBox", "ComboBox", "Green")), &mut reread), ["colorBox で「Green」を選んだ"]);
        assert!(interpreter.feed(Input::Click(target(6, "colorBox", "ComboBox", "Green")), &mut reread).is_empty());
    }

    #[test]
    fn hand_operations_carry_steps_that_replay_them_by_name() {
        use serde_json::json;
        let mut interpreter = Interpreter::default();
        let mut reread = |id: usize| Some(target(id, "nameBox", "TextBox", "Taro"));
        let steps = |out: Vec<Recorded>| out.into_iter().map(|r| r.step).collect::<Vec<_>>();
        interpreter.feed(Input::Key { key: Key::Edit, target: target(3, "nameBox", "TextBox", "") }, &mut reread);
        let mut agree = target(2, "agreeBox", "CheckBox", "Agree");
        agree.checked = Some(true);
        assert_eq!(steps(interpreter.feed(Input::Click(agree), &mut reread)), [Some(json!({ "tool": "ui_set_text", "name": "nameBox", "value": "Taro" })), Some(json!({ "tool": "ui_toggle", "name": "agreeBox", "checked": true }))]);
        interpreter.feed(Input::Click(target(6, "colorBox", "ComboBox", "Red")), &mut reread);
        assert_eq!(steps(interpreter.feed(Input::Click(target(6, "colorBox", "ComboBox", "Blue")), &mut reread)), [Some(json!({ "tool": "ui_select", "name": "colorBox", "item": "Blue" }))]);
        let ok = Target { name: None, ..target(7, "", "Button", "OK") };
        assert_eq!(steps(interpreter.feed(Input::Click(ok), &mut reread)), [Some(json!({ "tool": "ui_invoke", "text": "OK" }))], "名前の無いボタンは文字で");
        assert_eq!(steps(interpreter.feed(Input::Key { key: Key::Named("Ctrl+S".into()), target: target(5, "form", "Form", "") }, &mut reread)), [Some(json!({ "tool": "ui_keys", "name": "form", "keys": "{Ctrl+S}" }))]);
        assert_eq!(steps(interpreter.feed(Input::Click(target(8, "grid", "ListView", "")), &mut reread)), [None], "位置で意味の変わる押しは流せない");
    }

    #[test]
    fn a_password_is_not_kept_in_the_steps_either() {
        let mut interpreter = Interpreter::default();
        let mut password = target(4, "passwordBox", "TextBox", "");
        password.password = true;
        let mut reread = |id: usize| Some(Target { password: true, ..target(id, "passwordBox", "TextBox", "secret") });
        interpreter.feed(Input::Key { key: Key::Edit, target: password }, &mut reread);
        assert_eq!(interpreter.finish(&mut reread)[0].step, None);
    }

    #[test]
    fn unchanged_text_after_typing_is_not_an_operation() {
        let mut interpreter = Interpreter::default();
        let mut reread = |id: usize| Some(target(id, "nameBox", "TextBox", "same"));
        interpreter.feed(Input::Key { key: Key::Edit, target: target(3, "nameBox", "TextBox", "same") }, &mut reread);
        assert!(interpreter.finish(&mut reread).is_empty());
    }
}
