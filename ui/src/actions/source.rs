//! ソース: 開く、読み込んだソースの一覧、逆コンパイルして開く、行のブレークポイント。

use crate::api;
use crate::model::{csharp, paths};
use crate::state::{Source, Store};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};
use std::sync::Arc;
/// ソースを開き、その行を見せる。同じファイルなら、読み直さない。
pub fn open_source(store: Store, path: String, line: u32) {
    spawn_local(async move { open_source_now(store, &path, line).await });
}

pub(super) async fn open_source_now(store: Store, path: &str, line: u32) {
    let loaded = store.source.with_untracked(|s| s.as_ref().is_some_and(|s| s.path.eq_ignore_ascii_case(path)));
    if !loaded {
        match api::get(&format!("/fs/read?path={}", api::encode(path))).await {
            Ok(body) => {
                let lines = csharp::highlight(body["text"].as_str().unwrap_or(""));
                store.source.set(Some(Source { path: path.to_string(), lines: Arc::new(lines) }));
                // 別のファイルになった: カバレッジの印を読み直す。
                store.coverage_file.set(None);
                super::load_coverage(store);
            }
            Err(message) => {
                store.fail(message);
                return;
            }
        }
    }
    store.reveal.set(Some(line));
}

/// 読み込んだソースの一覧を読む。読んでいる途中にまた頼まれたら、終わってからもう一度読む。
pub fn load_sources(store: Store) {
    let (loading, _) = store.sources_loading.get_untracked();
    if loading {
        store.sources_loading.set((true, true));
        return;
    }
    store.sources_loading.set((true, false));
    spawn_local(async move {
        loop {
            match api::dap("loadedSources", json!({})).await {
                Ok(body) => store.sources.set(crate::model::sources::parse(&body)),
                Err(message) => store.fail(message),
            }
            let (_, again) = store.sources_loading.get_untracked();
            if !again {
                break;
            }
            store.sources_loading.set((true, false));
        }
        store.sources_loading.set((false, false));
    });
}

/// PDB の無いアプリのモジュールを逆コンパイルし、一覧を読み直して、最初のソースを開く。
pub fn decompile_module(store: Store, module: String) {
    spawn_local(async move {
        match api::dap("netdbg.decompile", json!({ "module": module })).await {
            Ok(body) => {
                if let Some(first) = body["files"].as_array().and_then(|f| f.iter().filter_map(Value::as_str).find(|f| f.to_lowercase().ends_with("program.cs")).or(f.first().and_then(Value::as_str))) {
                    open_source_now(store, first, 1).await;
                }
                load_sources(store);
            }
            Err(message) => store.fail(message),
        }
    });
}

/// 行のブレークポイントを置くか外す。対象があれば、すぐ送る。
pub fn toggle_breakpoint(store: Store, path: String, line: u32) {
    let lines = store.breakpoints.try_update(|b| b.toggle(&path, line)).unwrap_or_default();
    if store.session.with_untracked(|s| s.is_live()) {
        spawn_local(async move { send_breakpoints(store, &path, &lines).await });
    }
}

pub(super) async fn send_breakpoints(store: Store, path: &str, lines: &[u32]) {
    let breakpoints: Vec<Value> = lines.iter().map(|line| json!({ "line": line })).collect();
    match api::dap("setBreakpoints", json!({ "source": { "path": path, "name": paths::file_name(path) }, "breakpoints": breakpoints, "lines": lines })).await {
        Ok(body) => store.breakpoints.update(|b| b.apply(path, &body)),
        Err(message) => store.fail(message),
    }
}
