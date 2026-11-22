//! 成績書（B3、D-081）: テスト項目を始める・写す・終える、成績書を書く。項目の一覧は、エンジンの知らせ
//! （`netdbg.testItemChanged`）で更新される。

use crate::api;
use crate::state::Store;
use crate::text;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};

/// 項目を始める（番号と項目名は入れたもの）。
pub fn start_test_item(store: Store) {
    let id = store.report_id.get_untracked().trim().to_string();
    let title = store.report_title.get_untracked().trim().to_string();
    item(store, json!({ "action": "start", "id": id, "title": title, "folder": store.output_folder(), "template": store.report_template() }), false);
}

/// 開いている項目に、いまの画面の写しを足す。
pub fn capture_test_item(store: Store) {
    item(store, json!({ "action": "capture" }), false);
}

/// 開いている項目を終える（`OK` / `NG`）。終えると成績書が書き直される。
pub fn end_test_item(store: Store, result: &'static str) {
    let note = store.report_note.get_untracked().trim().to_string();
    item(store, json!({ "action": "end", "result": result, "note": note, "folder": store.output_folder(), "template": store.report_template() }), true);
}

/// 成績書を書き、書いた先を見せる。
pub fn write_test_report(store: Store) {
    spawn_local(async move {
        match api::dap("netdbg.testReport", json!({ "folder": store.output_folder(), "template": store.report_template() })).await {
            Ok(body) => written(store, &body),
            Err(message) => store.fail(message),
        }
    });
}

/// 成績書の現場のテンプレートのフォルダー（ブラウザに覚える）。
pub fn set_report_template(store: Store, folder: String) {
    api::remember_text("reportTemplate", folder.trim());
    store.report_template.set(folder);
}

/// 既定のテンプレートを、テンプレートの欄のフォルダーに書き出す（現場はそれを直して使う）。
pub fn export_report_template(store: Store) {
    let Some(folder) = store.report_template() else {
        store.fail(text::report_template_needed().to_string());
        return;
    };
    spawn_local(async move {
        match api::dap("netdbg.testTemplate", json!({ "output": folder })).await {
            Ok(body) => store.report_written.set(Some(format!("{}{}", text::report_template_exported(), body["folder"].as_str().unwrap_or("")))),
            Err(message) => store.fail(message),
        }
    });
}

fn item(store: Store, arguments: Value, ending: bool) {
    spawn_local(async move {
        match api::dap("netdbg.testItem", arguments).await {
            Ok(body) if ending => {
                // 次の項目のために、備考を空にする（番号と項目名は、続けて似た番号を入れやすいよう残す）。
                store.report_note.set(String::new());
                written(store, &body);
            }
            Ok(_) => {}
            Err(message) => store.fail(message),
        }
    });
}

fn written(store: Store, body: &Value) {
    let files = [&body["html"], &body["xlsx"]].iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("、");
    store.report_written.set((!files.is_empty()).then(|| format!("{}{files}", text::report_written())));
    // いま書いた手順を、流す手順の欄に入れる（D-086）。
    if let Some(steps) = body["steps"].as_str() {
        set_replay_file(store, steps.to_string());
    }
}

/// 流す手順のファイル（ブラウザに覚える）。
pub fn set_replay_file(store: Store, file: String) {
    api::remember_text("replayFile", file.trim());
    store.replay_file.set(file);
}

/// 手順のファイルを、動いている対象に流す（MCP の `ui_replay`。D-086）。結果は成績書のタブの知らせの行に出す。
pub fn replay_steps(store: Store) {
    let file = store.replay_file.get_untracked().trim().to_string();
    if file.is_empty() {
        store.fail(text::replay_file_needed().to_string());
        return;
    }
    let item = store.replay_item.get_untracked().trim().to_string();
    let mut arguments = json!({ "file": file });
    if !item.is_empty() {
        arguments["item"] = json!(item);
    }
    store.report_written.set(Some(text::replaying().to_string()));
    spawn_local(async move {
        match api::mcp("ui_replay", arguments).await {
            Ok(done) => store.report_written.set(Some(crate::i18n::trf("Replayed {completed} of {total} steps.", &[("completed", done["completed"].to_string()), ("total", done["total"].to_string())]))),
            Err(message) => {
                // 失敗の文の 1 行目だけを見せる（2 行目からは、手ごとの結果の JSON）。
                let first = message.lines().next().unwrap_or("").to_string();
                store.report_written.set(None);
                store.fail(first);
            }
        }
    });
}
