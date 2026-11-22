//! 止めずに読む記録（段階 11、12）の切り替えと止める条件、使用量、ヒープの統計、保持している経路。

use super::send;
use crate::api;
use crate::model::exceptions::ExceptionSettings;
use crate::state::Store;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};
/// SQL か例外の記録を切り替える（記録のタブ）。SQL は、読み込み済みのプロバイダーでは読めないことがある（D-063）。
pub fn set_record_log(store: Store, sql: bool, enabled: bool) {
    let (command, signal, name) = if sql { ("netdbg.setSqlLog", store.sql_log, "sqlLog") } else { ("netdbg.setExceptionLog", store.exception_log, "exceptionLog") };
    signal.set(enabled);
    api::remember(name, enabled);
    if store.session.with_untracked(|s| s.is_live()) {
        send(store, command, json!({ "enabled": enabled }));
    }
}

/// 「SQL で止める」「ダイアログで止める」の条件を足すか外す。対象があれば、すぐ送る。
pub fn set_record_conditions(store: Store, sql: bool, conditions: Vec<String>) {
    if sql {
        store.sql_breaks.set(conditions);
    } else {
        store.dialog_breaks.set(conditions);
    }
    if store.session.with_untracked(|s| s.is_live()) {
        spawn_local(async move { send_record_conditions(store).await });
    }
}

pub(super) async fn send_record_conditions(store: Store) {
    let sql: Vec<Value> = store.sql_breaks.get_untracked().into_iter().map(|text| json!({ "text": text })).collect();
    let dialog: Vec<Value> = store.dialog_breaks.get_untracked().into_iter().map(|text| json!({ "text": text })).collect();
    if !sql.is_empty() || store.sql_log.get_untracked() {
        if let Err(message) = api::dap("netdbg.setSqlBreakpoints", json!({ "breakpoints": sql })).await {
            store.fail(message);
        }
    }
    if let Err(message) = api::dap("netdbg.setDialogLog", json!({ "breakOn": dialog })).await {
        store.fail(message);
    }
}

/// 例外の設定を変える（例外の設定の窓）。ブラウザに覚え、対象があれば、すぐ送る（動いている間でも効く）。
pub fn set_exception_settings(store: Store, settings: ExceptionSettings) {
    crate::api::remember_text("exceptionSettings", &settings.to_json());
    store.exception_settings.set(settings);
    if store.session.with_untracked(|s| s.is_live() && !s.dump) {
        spawn_local(async move { send_exception_settings(store).await });
    }
}

pub(super) async fn send_exception_settings(store: Store) {
    let request = store.exception_settings.with_untracked(ExceptionSettings::to_request);
    if let Err(message) = api::dap("setExceptionBreakpoints", request).await {
        store.fail(message);
    }
}

/// 使用量の直近の値を残す数。
const USAGE_SAMPLES: usize = 60;

/// 使用量を 1 回読む（動いている間だけ）。
pub fn sample_usage(store: Store) {
    if !store.session.with_untracked(|s| s.is_running()) {
        return;
    }
    spawn_local(async move {
        if let Ok(body) = api::dap("netdbg.processUsage", json!({})).await {
            store.usage.update(|samples| {
                samples.push(body);
                if samples.len() > USAGE_SAMPLES {
                    samples.remove(0);
                }
            });
        }
    });
}

/// ヒープの統計を読む（止まっている間）。
pub fn load_heap(store: Store) {
    spawn_local(async move {
        match api::dap("netdbg.heapStats", json!({})).await {
            Ok(body) => store.heap.set(Some(body)),
            Err(message) => store.fail(message),
        }
    });
}

/// オブジェクトを保持している経路（GC のルートから）を読み、窓に出す。
pub fn retention_path(store: Store, reference: i64) {
    spawn_local(async move {
        match api::dap("netdbg.retentionPath", json!({ "variablesReference": reference })).await {
            Ok(body) => store.retention.set(Some(body)),
            Err(message) => store.fail(message),
        }
    });
}

/// オブジェクトのフィールドに書き込みの見張りを置く（D-093）。`parent` はフィールドを持つ値の番号。
pub fn watch_writes(store: Store, parent: i64, name: String) {
    spawn_local(async move {
        let info = match api::dap("dataBreakpointInfo", json!({ "variablesReference": parent, "name": name })).await {
            Ok(info) => info,
            Err(message) => return store.fail(message),
        };
        let description = info["description"].as_str().unwrap_or_default().to_string();
        let Some(id) = info["dataId"].as_str() else { return store.fail(description) };
        match api::dap("setDataBreakpoints", json!({ "breakpoints": [{ "dataId": id, "accessType": "write" }] })).await {
            Ok(set) if set["breakpoints"][0]["verified"] == true => store.data_watch.set(Some(description)),
            Ok(set) => store.fail(set["breakpoints"][0]["message"].as_str().unwrap_or_default().to_string()),
            Err(message) => store.fail(message),
        }
    });
}

/// 書き込みの見張りを外す。
pub fn clear_write_watch(store: Store) {
    spawn_local(async move {
        match api::dap("setDataBreakpoints", json!({ "breakpoints": [] })).await {
            Ok(_) => store.data_watch.set(None),
            Err(message) => store.fail(message),
        }
    });
}
