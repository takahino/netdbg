//! 差し替わった exe の差分（B2、D-080）: 前の版と、いまデバッグしているプログラムを比べる。カバレッジが変わったら、
//! 再試験の範囲を出し直す。

use crate::api;
use crate::state::{Store, Tab};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::json;
use std::cell::Cell;

thread_local! {
    /// 比べている途中か、その間にまた頼まれたか（カバレッジの知らせが続けて届く間、比べ直しを重ねない）。
    static RUNNING: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// 前の版（exe、dll、フォルダー）を覚えて、比べる。
pub fn run_diff(store: Store, old: String) {
    api::remember_text("diffOld", &old);
    store.diff_old.set(old);
    compare(store);
}

/// カバレッジが変わった: 差分を出していて、そのタブを見ていれば、比べ直す（通ったかが変わる）。
pub fn refresh_diff(store: Store) {
    if store.diff.with_untracked(Option::is_some) && store.bottom.get_untracked() == Tab::Diff {
        compare(store);
    }
}

fn compare(store: Store) {
    let (running, _) = RUNNING.with(Cell::get);
    if running {
        RUNNING.with(|r| r.set((true, true)));
        return;
    }
    RUNNING.with(|r| r.set((true, false)));
    spawn_local(async move {
        loop {
            let old = store.diff_old.get_untracked();
            match api::dap("netdbg.diffAssemblies", json!({ "old": old })).await {
                Ok(body) => store.diff.set(Some(body)),
                Err(message) => {
                    store.diff.set(None);
                    store.fail(message);
                }
            }
            let (_, again) = RUNNING.with(Cell::get);
            if !again {
                break;
            }
            RUNNING.with(|r| r.set((true, false)));
        }
        RUNNING.with(|r| r.set((false, false)));
    });
}
