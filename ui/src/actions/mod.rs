//! 画面の操作（DAP の要求の並び）。どれも `spawn_local` で動かし、結果を `Store` に入れる。
//!
//! - ここ: セッションの始め方と止め方、届いた出来事の当て方、実行の操作。
//! - `inspect`: 止まったときに読むもの（スタック、変数、ウォッチ、式、値の窓、画面の木）。
//! - `source`: ソースを開く、読み込んだソースの一覧、逆コンパイル、行のブレークポイント。
//! - `records`: 止めずに読む記録の切り替えと止める条件、使用量、ヒープ、保持している経路。

mod coverage;
mod diff;
mod inspect;
mod records;
mod report;
mod source;

pub use coverage::*;
pub use diff::*;
pub use inspect::*;
pub use records::*;
pub use report::*;
pub use source::*;

use inspect::load_variables;
use records::{send_exception_settings, send_record_conditions};
use source::send_breakpoints;

use crate::api;
use crate::model::paths;
use crate::model::session::{Follow, Status};
use crate::state::Store;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::Duration;
/// 始めるもの: `{ program, args }`、`{ attach }`、`{ dump }`。
pub fn start(store: Store, what: Value) {
    store.dialog.set(false);
    store.error.set(None);
    store.relaunch.set(None);
    store.last_start.set(Some(what.clone()));
    store.clear_stop();
    store.threads.set(Vec::new());
    store.tree.set(None);
    store.data_watch.set(None);
    store.session.update(|s| s.begin());
    spawn_local(async move {
        if let Err(message) = start_session(store, &what).await {
            store.session.update(|s| s.status = Status::Ended(None));
            // ビット数が合わない（target.rs の文言）なら、隣のエンジンで開き直せる。文言は言語で変わるので、どの言語でも
            // 同じエンジンのファイル名で見分ける（起動とアタッチの誤りで、これを挙げるのはビット数の食い違いだけ）。
            if message.contains("netdbg.exe") {
                store.relaunch.set(Some(what.clone()));
            }
            store.fail(message);
        }
    });
}

async fn start_session(store: Store, what: &Value) -> Result<(), String> {
    api::dap(
        "initialize",
        json!({ "clientID": "netdbg-ui", "clientName": "netdbg", "adapterID": "netdbg", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsInvalidatedEvent": true, "locale": crate::i18n::tag() }),
    )
    .await?;
    // 記録は、対象のアセンブリが読み込まれる前に始める（SQL のプロバイダーは、読み込みのときにしか読める形にできない）。
    let (sql_log, exception_log) = (store.sql_log.get_untracked(), store.exception_log.get_untracked());
    // カバレッジも、モジュールが読み込まれる前に始める（入口にブレークポイントを置くのは読み込みのとき）。
    let coverage = match (store.coverage_on.get_untracked(), store.output_folder()) {
        (true, Some(folder)) => json!({ "enabled": true, "folder": folder }),
        (on, _) => json!(on),
    };
    store.coverage_file.set(None);
    store.coverage_rates.set(Default::default());
    if let Some(pid) = what["attach"].as_u64() {
        api::dap("attach", json!({ "processId": pid, "justMyCode": true, "sqlLog": sql_log, "exceptionLog": exception_log, "coverage": coverage, "observeOnly": what["observeOnly"] == true })).await?;
    } else if let Some(dump) = what["dump"].as_str() {
        api::dap("launch", json!({ "dumpPath": dump })).await?;
    } else {
        let program = what["program"].as_str().ok_or(crate::i18n::tr("There is no program."))?;
        let cwd = paths::parent(program).to_string();
        api::dap("launch", json!({ "program": program, "args": what["args"], "cwd": cwd, "justMyCode": true, "decompile": "app", "console": "internalConsole", "sqlLog": sql_log, "exceptionLog": exception_log, "coverage": coverage })).await?;
    }
    // ダンプは読むだけなので、止める条件は送らない。
    if what.get("dump").is_none() {
        send_record_conditions(store).await;
        send_exception_settings(store).await;
    }
    for (path, lines) in store.breakpoints.get_untracked().files() {
        send_breakpoints(store, &path, &lines).await;
    }
    api::dap("configurationDone", json!({})).await?;
    Ok(())
}

/// 届いた出来事を、まとめて当てる間隔。出力の多い対象では 1 秒に数千の出来事が届くので、1 つずつ当てて描き直すと
/// 画面が追いつかない（3,000 行で約 8 秒かかった）。
const BATCH: Duration = Duration::from_millis(16);

thread_local! {
    /// まだ当てていない出来事。
    static PENDING: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

/// 出来事 1 つ（SSE）。ためておき、`BATCH` ごとにまとめて当てる。
pub fn on_message(store: Store, message: Value) {
    let first = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        pending.push(message);
        pending.len() == 1
    });
    if first {
        set_timeout(move || apply_pending(store), BATCH);
    }
}

/// ためた出来事を、セッションに 1 回で当て（描き直しも 1 回）、そのあとですること（止まったらスタックを読む、など）を
/// 行う。続けて届いた止まった / 動き出したは、最後のものだけでよい。
fn apply_pending(store: Store) {
    let batch = PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    let mut follows = Vec::new();
    let mut replayed = false;
    store.session.update(|session| {
        for message in &batch {
            if message["type"] == "ui.replayed" {
                replayed = true;
                follows.clear();
                continue;
            }
            let follow = session.apply(message);
            if follow != Follow::Nothing {
                follows.push(follow);
            }
        }
    });
    if replayed {
        store.replayed.set(true);
        // 開き直したページ: 止まっていれば、その場所を読む。
        if let Some(thread) = store.session.with_untracked(|s| s.stopped_thread()) {
            refresh_stop(store, thread);
        }
        load_coverage(store);
    }
    if !store.replayed.get_untracked() {
        return;
    }
    let last_run_state = follows.iter().rposition(|f| matches!(f, Follow::Stopped(_) | Follow::Continued | Follow::Ended));
    for (index, follow) in follows.into_iter().enumerate() {
        if matches!(follow, Follow::Stopped(_) | Follow::Continued | Follow::Ended) && Some(index) != last_run_state {
            continue;
        }
        follow_up(store, follow);
    }
}

fn follow_up(store: Store, follow: Follow) {
    match follow {
        Follow::Stopped(thread) => refresh_stop(store, thread),
        Follow::Continued | Follow::Ended => store.clear_stop(),
        Follow::Refresh => {
            if let Some(frame) = store.frame.get_untracked() {
                spawn_local(async move { load_variables(store, frame).await });
            }
        }
        Follow::Coverage(files) => {
            coverage_changed(store, &files);
            refresh_diff(store);
        }
        Follow::Breakpoint(_) | Follow::Nothing => {}
    }
}

/// 答えの本文を使わない要求を送る（失敗だけ見せる）。
fn send(store: Store, command: &'static str, arguments: Value) {
    spawn_local(async move {
        if let Err(message) = api::dap(command, arguments).await {
            store.fail(message);
        }
    });
}

/// 実行の操作（続行、一時停止、ステップ）。止まっているスレッドに対して。
pub fn run(store: Store, command: &'static str) {
    let thread = store.thread.get_untracked().or_else(|| store.session.with_untracked(|s| s.stopped_thread())).unwrap_or(0);
    send(store, command, json!({ "threadId": thread }));
}

/// 対象を止める（起動したものは終わらせ、アタッチしたものは切り離す）。
pub fn stop(store: Store) {
    let attached = store.last_start.with_untracked(|w| w.as_ref().is_some_and(|w| w.get("attach").is_some()));
    send(store, "disconnect", json!({ "terminateDebuggee": !attached }));
}

/// エンジンを終える。
pub fn quit(store: Store) {
    spawn_local(async move {
        let _ = api::post("/quit", json!({})).await;
        store.fail(crate::i18n::tr("The engine has quit. You can close this tab."));
    });
}

/// 隣のビット数のエンジンで開き直す。
pub fn relaunch(store: Store) {
    let Some(what) = store.relaunch.get_untracked() else { return };
    let body = match what.get("dump") {
        Some(dump) => json!({ "program": dump }),
        None => what,
    };
    spawn_local(async move {
        match api::post("/relaunch", body).await {
            Ok(answer) => store.fail(crate::i18n::trf("Opened in a new tab with the {engine} engine. You can close this tab.", &[("engine", answer["engine"].as_str().unwrap_or("").to_string())])),
            Err(message) => store.fail(message),
        }
    });
}
