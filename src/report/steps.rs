//! 流せる手順（D-086）。成績書の操作（AI と人の手）を、別の exe にもう一度流せる形で持つ。
//!
//! 手順 1 つは MCP の画面のツールの呼び出しそのもの（`{ "tool": "ui_invoke", "name": "greetButton" }`）。流すのは
//! `ui_replay` で、同じツールの処理を 1 手ずつ呼ぶ。コントロールは名前（無ければ表示の文字）で指す（窓のハンドルや
//! 番号は、起動し直すと変わるので残さない）。座標でしか指せない操作（`ui_mouse`）は残さない。

use serde_json::{json, Map, Value};

/// 手順を書いたファイルの形の版。
const VERSION: u64 = 1;

/// 手順に残せるツール。
const REPLAYABLE: &[&str] = &["ui_invoke", "ui_set_text", "ui_select", "ui_toggle", "ui_keys"];

/// コントロールの指し方（名前、無ければ表示の文字）。どちらも無ければ指せない。
pub fn target(name: Option<&str>, text: Option<&str>) -> Option<Map<String, Value>> {
    let mut target = Map::new();
    match (name.filter(|n| !n.is_empty()), text.filter(|t| !t.is_empty())) {
        (Some(name), _) => target.insert("name".into(), json!(name)),
        (None, Some(text)) => target.insert("text".into(), json!(text)),
        (None, None) => return None,
    };
    Some(target)
}

/// 手順 1 つ。`target` が無い手順（`ui_keys` を前の窓に送る）は `None` を渡す。
pub fn step(tool: &str, target: Option<Map<String, Value>>, extra: Value) -> Value {
    let mut step = Map::new();
    step.insert("tool".into(), json!(tool));
    step.extend(target.unwrap_or_default());
    if let Value::Object(extra) = extra {
        step.extend(extra);
    }
    Value::Object(step)
}

/// AI の画面の操作を手順にする。`done_target` は、操作の答えにある相手（`ui_tree` の形。名前と文字を読む）。番号や
/// ハンドルで指した操作も、名前で指す手順になる。流せない操作は `None`。
pub fn from_ai(tool: &str, arguments: &Value, done_target: Option<&Value>) -> Option<Value> {
    if !REPLAYABLE.contains(&tool) {
        return None;
    }
    let read = |value: Option<&Value>, key: &str| value.and_then(|v| v.get(key)).and_then(Value::as_str).map(str::to_string);
    let (name, text) = (read(done_target, "name").or_else(|| read(Some(arguments), "name")), read(done_target, "text").or_else(|| read(Some(arguments), "text")));
    let target = target(name.as_deref(), text.as_deref());
    // ui_keys だけは、相手が無くても流せる（前の窓に打つ）。
    if target.is_none() && tool != "ui_keys" {
        return None;
    }
    let keep = |keys: &[&str]| -> Value { Value::Object(keys.iter().filter_map(|k| arguments.get(*k).map(|v| (k.to_string(), v.clone()))).collect()) };
    let extra = match tool {
        "ui_set_text" => keep(&["value"]),
        "ui_select" => keep(&["item", "index"]),
        "ui_toggle" => keep(&["checked"]),
        "ui_keys" => keep(&["keys"]),
        _ => json!({}),
    };
    Some(step(tool, target, extra))
}

/// 手順のファイルの中身（項目ごと）。
pub fn file(items: &[(String, String, Vec<Value>)]) -> Value {
    json!({
        "version": VERSION,
        "items": items.iter().map(|(id, title, steps)| json!({ "id": id, "title": title, "steps": steps })).collect::<Vec<_>>(),
    })
}

/// 手順のファイルから、流す手順を取り出す。`item` があればその項目の、無ければすべての項目の手順を順に。
pub fn pick(file: &Value, item: Option<&str>) -> Result<Vec<Value>, String> {
    let items = file["items"].as_array().ok_or_else(|| tr!("This is not a steps file (it has no items)."))?;
    let chosen: Vec<&Value> = match item {
        Some(id) => {
            let found: Vec<&Value> = items.iter().filter(|i| i["id"] == id).collect();
            if found.is_empty() {
                let ids: Vec<&str> = items.iter().filter_map(|i| i["id"].as_str()).collect();
                return Err(tr!("The steps file has no item {id}. Items: {ids}", id = id, ids = ids.join(", ")));
            }
            found
        }
        None => items.iter().collect(),
    };
    Ok(chosen.into_iter().flat_map(|i| i["steps"].as_array().cloned().unwrap_or_default()).collect())
}

/// 手順を、ツールの名前と引数に分ける（流す前に確かめる）。
pub fn split(step: &Value) -> Result<(String, Value), String> {
    let tool = step["tool"].as_str().filter(|t| REPLAYABLE.contains(t)).ok_or_else(|| tr!("A step must have tool: one of {tools} ({step}).", tools = REPLAYABLE.join(", "), step = step))?;
    let mut arguments = step.as_object().cloned().unwrap_or_default();
    arguments.remove("tool");
    Ok((tool.to_string(), Value::Object(arguments)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_actions_become_steps_by_name() {
        // 番号で押しても、答えの相手の名前で残す。
        let done = json!({ "name": "greetButton", "text": "Greet", "mark": 3 });
        assert_eq!(from_ai("ui_invoke", &json!({ "mark": 3, "waitMs": 100 }), Some(&done)), Some(json!({ "tool": "ui_invoke", "name": "greetButton" })));
        assert_eq!(from_ai("ui_set_text", &json!({ "name": "nameBox", "value": "Taro", "window": 1234 }), None), Some(json!({ "tool": "ui_set_text", "name": "nameBox", "value": "Taro" })));
        assert_eq!(from_ai("ui_toggle", &json!({ "id": 5, "checked": true }), Some(&json!({ "name": "agreeBox" }))), Some(json!({ "tool": "ui_toggle", "name": "agreeBox", "checked": true })));
        assert_eq!(from_ai("ui_invoke", &json!({ "id": 5 }), Some(&json!({ "text": "OK" }))), Some(json!({ "tool": "ui_invoke", "text": "OK" })), "名前の無いものは文字で");
    }

    #[test]
    fn actions_that_cannot_be_replayed_are_left_out() {
        assert_eq!(from_ai("ui_mouse", &json!({ "x": 10, "y": 20 }), None), None, "座標でしか指せない");
        assert_eq!(from_ai("ui_invoke", &json!({ "id": 5 }), Some(&json!({ "kind": "Button" }))), None, "名前も文字も無い");
        assert_eq!(from_ai("ui_keys", &json!({ "keys": "{Enter}" }), None), Some(json!({ "tool": "ui_keys", "keys": "{Enter}" })), "キーは前の窓に打てる");
    }

    #[test]
    fn a_file_gives_the_steps_of_one_item_or_all() {
        let file = file(&[("1-1".into(), "a".into(), vec![json!({ "tool": "ui_invoke", "name": "x" })]), ("1-2".into(), "b".into(), vec![json!({ "tool": "ui_invoke", "name": "y" })])]);
        assert_eq!(pick(&file, Some("1-2")).unwrap(), vec![json!({ "tool": "ui_invoke", "name": "y" })]);
        assert_eq!(pick(&file, None).unwrap().len(), 2);
        assert!(pick(&file, Some("9-9")).unwrap_err().contains("1-1, 1-2"));
        assert!(pick(&json!({}), None).is_err());
    }

    #[test]
    fn a_step_is_split_into_the_tool_and_its_arguments() {
        assert_eq!(split(&json!({ "tool": "ui_set_text", "name": "nameBox", "value": "Taro" })).unwrap(), ("ui_set_text".to_string(), json!({ "name": "nameBox", "value": "Taro" })));
        assert!(split(&json!({ "tool": "ui_mouse", "x": 1 })).is_err());
        assert!(split(&json!({ "name": "x" })).is_err());
    }
}
