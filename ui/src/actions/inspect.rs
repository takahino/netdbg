//! 止まったときに読むもの: スレッドとスタック、変数の木、ウォッチと式、ソースの名前の値の窓、画面の木。

use super::source::{open_source, open_source_now};
use crate::api;
use crate::model::{stack, variables, windows};
use crate::state::{Store, Tab};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::json;
use std::time::Duration;
/// スタックを読む深さ。
const STACK_LEVELS: u64 = 100;

/// 止まった: スレッドとスタックを読み、ソースのある一番上のフレームを開く。
pub fn refresh_stop(store: Store, thread: i64) {
    spawn_local(async move {
        if let Ok(body) = api::dap("threads", json!({})).await {
            store.threads.set(stack::threads(&body));
        }
        select_thread_now(store, thread).await;
    });
}

pub fn select_thread(store: Store, thread: i64) {
    spawn_local(async move { select_thread_now(store, thread).await });
}

async fn select_thread_now(store: Store, thread: i64) {
    store.thread.set(Some(thread));
    match api::dap("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": STACK_LEVELS })).await {
        Ok(body) => {
            let frames = stack::frames(&body);
            let first = stack::first_with_source(&frames).cloned();
            store.frames.set(frames);
            if let Some(frame) = first {
                select_frame_now(store, frame).await;
            }
        }
        Err(message) => store.fail(message),
    }
}

pub fn select_frame(store: Store, frame: stack::Frame) {
    spawn_local(async move { select_frame_now(store, frame).await });
}

async fn select_frame_now(store: Store, frame: stack::Frame) {
    store.frame.set(Some(frame.id));
    match &frame.path {
        Some(path) if frame.line > 0 => {
            store.current.set(Some((path.clone(), frame.line)));
            open_source_now(store, path, frame.line).await;
        }
        _ => store.current.set(None),
    }
    load_variables(store, frame.id).await;
    evaluate_watches(store, frame.id).await;
}

/// 変数の欄を読み直す（スコープと、開いておく枝）。
pub(super) async fn load_variables(store: Store, frame: i64) {
    let Ok(body) = api::dap("scopes", json!({ "frameId": frame })).await else {
        store.variables.set(Vec::new());
        return;
    };
    store.variables.set(variables::from_scopes(&body));
    load_pending(store.variables).await;
}

/// 開いているのに子をまだ読んでいない枝の子を読む（変数の欄と、値の窓の木の両方）。
async fn load_pending(tree: RwSignal<Vec<variables::Node>>) {
    for (path, reference) in tree.with_untracked(|v| variables::pending(v)) {
        let children = match api::dap("variables", json!({ "variablesReference": reference })).await {
            Ok(body) => variables::from_variables(&body),
            Err(message) => vec![variables::Node { name: "!".into(), value: message, type_name: String::new(), reference: 0, children: None, expanded: false }],
        };
        tree.update(|v| variables::set_children(v, &path, children));
    }
}

/// 木の枝を開くか閉じる。
pub fn toggle_node(tree: RwSignal<Vec<variables::Node>>, path: Vec<usize>) {
    let reference = tree.try_update(|v| variables::toggle(v, &path)).flatten();
    if reference.is_some() {
        spawn_local(async move { load_pending(tree).await });
    }
}

/// マウスを載せてから値の窓を出すまでと、外してから消すまでの間。
const HOVER_DELAY: Duration = Duration::from_millis(350);

const HOVER_HIDE_DELAY: Duration = Duration::from_millis(300);

/// ソースの名前にマウスを載せた: 止まっていれば、少し待ってから、その式（`a.b.c`）をいまのフレームで評価して、値の窓を
/// 出す。評価できないもの（メソッドの名前、型の名前など）は出さない。
pub fn hover_enter(store: Store, expression: String, x: i32, y: i32) {
    let Some(frame) = store.frame.get_untracked() else { return };
    if !store.session.with_untracked(|s| s.is_stopped()) {
        return;
    }
    let generation = store.hover_generation.get_untracked() + 1;
    store.hover_generation.set(generation);
    set_timeout(
        move || {
            if store.hover_generation.get_untracked() != generation {
                return;
            }
            spawn_local(async move {
                let Ok(body) = api::dap("evaluate", json!({ "expression": expression, "frameId": frame, "context": "hover" })).await else { return };
                if store.hover_generation.get_untracked() != generation {
                    return;
                }
                let reference = body["variablesReference"].as_i64().unwrap_or(0);
                let root = variables::Node {
                    name: expression,
                    value: body["result"].as_str().unwrap_or("").to_string(),
                    type_name: body["type"].as_str().unwrap_or("").to_string(),
                    reference,
                    children: None,
                    expanded: false,
                };
                store.hover_tree.set(vec![root]);
                store.hover.set(Some(crate::state::Hover { x, y }));
                // 中身のあるもの（オブジェクト、配列）は、1 段開いて見せる。
                if reference != 0 {
                    toggle_node(store.hover_tree, vec![0]);
                }
            });
        },
        HOVER_DELAY,
    );
}

/// 名前か値の窓からマウスが出た: 少し待って、窓の中に戻っていなければ消す。
pub fn hover_leave(store: Store) {
    let generation = store.hover_generation.get_untracked() + 1;
    store.hover_generation.set(generation);
    set_timeout(
        move || {
            if store.hover_generation.get_untracked() == generation && !store.hover_inside.get_untracked() {
                store.hover.set(None);
            }
        },
        HOVER_HIDE_DELAY,
    );
}

/// ウォッチを足す。
pub fn add_watch(store: Store, expression: String) {
    if expression.trim().is_empty() {
        return;
    }
    store.watches.update(|w| w.push(crate::state::Watch { expression, result: None }));
    if let Some(frame) = store.frame.get_untracked() {
        spawn_local(async move { evaluate_watches(store, frame).await });
    }
}

pub fn remove_watch(store: Store, index: usize) {
    store.watches.update(|w| {
        if index < w.len() {
            w.remove(index);
        }
    });
}

/// 変数の欄の値を、式で書き換える（`setVariable`）。空は送らない。失敗は理由を出し、表示は元のまま。
pub fn assign_variable(store: Store, parent: i64, name: String, value: String) {
    if value.trim().is_empty() {
        return;
    }
    spawn_local(async move {
        if let Err(message) = api::dap("setVariable", json!({ "variablesReference": parent, "name": name, "value": value })).await {
            store.fail(message);
        }
    });
}

/// ウォッチの式が指す場所を、式で書き換える（`setExpression`）。空は送らない。
pub fn assign_expression(store: Store, expression: String, value: String) {
    if value.trim().is_empty() {
        return;
    }
    let frame = store.frame.get_untracked().unwrap_or(0);
    spawn_local(async move {
        if let Err(message) = api::dap("setExpression", json!({ "expression": expression, "value": value, "frameId": frame })).await {
            store.fail(message);
        }
    });
}

/// 値が変わったかもしれない（`invalidated`）: 変数とウォッチを読み直す。
pub(super) async fn refresh_shown(store: Store, frame: i64) {
    load_variables(store, frame).await;
    evaluate_watches(store, frame).await;
}

async fn evaluate_watches(store: Store, frame: i64) {
    let expressions: Vec<String> = store.watches.with_untracked(|w| w.iter().map(|w| w.expression.clone()).collect());
    for (index, expression) in expressions.into_iter().enumerate() {
        let result = api::dap("evaluate", json!({ "expression": expression, "frameId": frame, "context": "watch" })).await.map(|body| body["result"].as_str().unwrap_or("").to_string());
        store.watches.update(|w| {
            if let Some(watch) = w.get_mut(index) {
                watch.result = Some(result);
            }
        });
    }
}

/// 式を評価して、出力に出す（出力の欄の入力）。
pub fn evaluate(store: Store, expression: String) {
    if expression.trim().is_empty() {
        return;
    }
    let frame = store.frame.get_untracked();
    store.session.update(|s| s.push_output("input", &format!("> {expression}\n")));
    spawn_local(async move {
        let mut arguments = json!({ "expression": expression, "context": "repl" });
        if let Some(frame) = frame {
            arguments["frameId"] = json!(frame);
        }
        let text = match api::dap("evaluate", arguments).await {
            Ok(body) => format!("{}\n", body["result"].as_str().unwrap_or("")),
            Err(message) => format!("{message}\n"),
        };
        store.session.update(|s| s.push_output("result", &text));
    });
}

/// 対象の画面の木を読む。
pub fn load_tree(store: Store) {
    let Some(pid) = store.session.with_untracked(|s| s.process_id) else {
        store.fail(crate::i18n::tr("There is no target process."));
        return;
    };
    let running = store.session.with_untracked(|s| s.is_running());
    spawn_local(async move {
        match api::get(&format!("/ui/tree?pid={pid}&running={running}")).await {
            Ok(body) => {
                store.tree.set(Some(windows::parse(&body)));
                store.shot.update(|n| *n += 1);
            }
            Err(message) => store.fail(message),
        }
    });
}

/// AI の操作の場所を開く。
pub fn reveal_action(store: Store, file: String, line: u32) {
    store.side.set(Tab::Variables);
    open_source(store, file, line);
}
