//! カバレッジ（B1、D-079）: 開いているファイルの行ごとの状態とファイルごとの率を読み、XML を書く。

use crate::api;
use crate::model::coverage::{self, FileCoverage};
use crate::state::Store;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::json;
use std::cell::Cell;

thread_local! {
    /// 読んでいる途中か、その間にまた頼まれたか（知らせが続けて届く間、読み直しを重ねない）。
    static LOADING: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// 起動の前に選ぶ「カバレッジを取る」（ブラウザに覚える）。
pub fn set_coverage_option(store: Store, on: bool) {
    store.coverage_on.set(on);
    api::remember("coverage", on);
}

/// 成績書とカバレッジの XML の保存先（ブラウザに覚える）。
pub fn set_output_folder(store: Store, folder: String) {
    api::remember_text("outputFolder", folder.trim());
    store.output_folder.set(folder);
}

/// 印の変わったファイルの知らせ（`files` が空なら、合計だけが変わった）。開いているファイルが含まれていれば読み直す。
pub fn coverage_changed(store: Store, files: &[String]) {
    let open = store.source.with_untracked(|s| s.as_ref().map(|s| s.path.clone()));
    if files.is_empty() || open.as_deref().is_some_and(|path| coverage::changed(files, path)) || store.coverage_file.with_untracked(Option::is_none) {
        load_coverage(store);
    }
}

/// 開いているファイルの行ごとの状態と、ファイルごとの率を読む（数えていなければ何もしない）。
pub fn load_coverage(store: Store) {
    if store.session.with_untracked(|s| s.coverage.is_none()) {
        return;
    }
    let (loading, _) = LOADING.with(Cell::get);
    if loading {
        LOADING.with(|l| l.set((true, true)));
        return;
    }
    LOADING.with(|l| l.set((true, false)));
    spawn_local(async move {
        loop {
            let open = store.source.with_untracked(|s| s.as_ref().map(|s| s.path.clone()));
            let mut request = json!({ "files": true });
            if let Some(path) = &open {
                request["file"] = json!(path);
            }
            if let Ok(body) = api::dap("netdbg.coverage", request).await {
                store.coverage_file.set(open.map(|path| FileCoverage::from_body(&path, &body)));
                store.coverage_rates.set(coverage::file_rates(&body));
            }
            let (_, again) = LOADING.with(Cell::get);
            if !again {
                break;
            }
            LOADING.with(|l| l.set((true, false)));
        }
        LOADING.with(|l| l.set((false, false)));
    });
}

/// いまの分を Cobertura XML に書く。書いた先は出力に出す。
pub fn write_coverage(store: Store) {
    spawn_local(async move {
        match api::dap("netdbg.coverage", json!({ "write": true, "folder": store.output_folder() })).await {
            Ok(body) => {
                let path = body["path"].as_str().unwrap_or("").to_string();
                store.session.update(|s| {
                    s.push_output("important", &format!("{}{path}\n", crate::text::coverage_written()));
                    if let Some(totals) = s.coverage.as_mut() {
                        totals.path = Some(path);
                    }
                });
            }
            Err(message) => store.fail(message),
        }
    });
}
