//! プログラムを開く画面: フォルダーをたどって exe / dll / ダンプを選んで起動するか、動いているプロセスにアタッチする。
//! ブラウザは手元のパスを渡せないので、フォルダーの中身はエンジンが返す（`/fs/list`）。

use crate::actions;
use crate::api;
use crate::model::paths;
use crate::state::store;
use crate::text;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Program,
    Attach,
}

#[component]
pub fn OpenDialog() -> impl IntoView {
    let store = store();
    let mode = RwSignal::new(Mode::Program);
    // 観察専用でアタッチするか（D-089。開くたびに外す: うっかり観察専用のまま値を書き換えられない、を避ける）。
    let observe = RwSignal::new(false);
    let entries = RwSignal::new(Vec::<Value>::new());
    let parent = RwSignal::new(None::<String>);
    let program = RwSignal::new(String::new());
    let arguments = RwSignal::new(String::new());
    let processes = RwSignal::new(Vec::<Value>::new());
    let live = move || store.session.with(|s| s.is_live());
    let browse = move |folder: String| {
        spawn_local(async move {
            match api::get(&format!("/fs/list?path={}", api::encode(&folder))).await {
                Ok(body) => {
                    store.folder.set(body["path"].as_str().unwrap_or("").to_string());
                    parent.set(body["parent"].as_str().map(String::from));
                    entries.set(body["entries"].as_array().cloned().unwrap_or_default());
                }
                Err(message) => store.fail(message),
            }
        })
    };
    let load_processes = move || {
        spawn_local(async move {
            match api::get("/sys/processes").await {
                Ok(body) => processes.set(body["processes"].as_array().cloned().unwrap_or_default()),
                Err(message) => store.fail(message),
            }
        })
    };
    // 開いたときに、最後のフォルダー（無ければドライブ）を読む。
    Effect::new(move |_| {
        if store.dialog.get() {
            browse(store.folder.get_untracked());
        }
    });
    let launch = move || {
        let path = program.get_untracked();
        if path.is_empty() {
            return;
        }
        let what = if path.to_lowercase().ends_with(".dmp") { json!({ "dump": path }) } else { json!({ "program": path, "args": paths::split_arguments(&arguments.get_untracked()) }) };
        actions::start(store, what);
    };
    let entry_rows = move || {
        entries
            .get()
            .into_iter()
            .map(|entry| {
                let name = entry["name"].as_str().unwrap_or("").to_string();
                let kind = entry["kind"].as_str().unwrap_or("").to_string();
                let full = paths::join(&store.folder.get_untracked(), &name);
                let is_dir = kind == "dir";
                let is_source = matches!(kind.as_str(), "cs" | "vb" | "fs");
                let selected = !is_dir && program.get().eq_ignore_ascii_case(&full);
                let (click_path, double_path) = (full.clone(), full);
                view! {
                    <li class=format!("entry {kind}") class:selected=selected
                        on:click=move |_| {
                            if is_dir {
                                browse(click_path.clone())
                            } else if is_source {
                                // ソースは、開いてブレークポイントを置くため（起動の前でも）。
                                store.dialog.set(false);
                                actions::open_source(store, click_path.clone(), 1);
                            } else {
                                program.set(click_path.clone())
                            }
                        }
                        on:dblclick=move |_| if !is_dir && !is_source && !live() { program.set(double_path.clone()); launch() }>
                        <span class="icon">{if is_dir { "📁" } else { "📄" }}</span>
                        {name}
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    let selected_pid = move || program.get().parse().ok().map(|pid: u64| pid);
    let process_rows = move || {
        processes
            .get()
            .into_iter()
            .map(|p| {
                let pid = p["pid"].as_u64().unwrap_or(0);
                let managed = p["managed"].as_bool();
                let note = match managed {
                    Some(true) => ".NET",
                    Some(false) => "",
                    None => "?",
                };
                view! {
                    <li class="entry" class:managed=managed == Some(true) on:dblclick=move |_| actions::start(store, json!({ "attach": pid, "observeOnly": observe.get_untracked() }))
                        on:click=move |_| program.set(pid.to_string())
                        class:selected=move || program.get() == pid.to_string()>
                        <span class="pid">{pid}</span>
                        <span class="name">{p["name"].as_str().unwrap_or("").to_string()}</span>
                        <span class="bits">{p["bits"].as_str().unwrap_or("").to_string()}</span>
                        <span class="note">{note}</span>
                        <span class="path">{p["path"].as_str().unwrap_or("").to_string()}</span>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <Show when=move || store.dialog.get()>
            <div class="backdrop">
                <div class="dialog">
                    <h2>{text::open_title()}</h2>
                    <div class="tabs">
                        <button class:active=move || mode.get() == Mode::Program on:click=move |_| mode.set(Mode::Program)>{text::program_tab()}</button>
                        <button class:active=move || mode.get() == Mode::Attach on:click=move |_| { mode.set(Mode::Attach); program.set(String::new()); load_processes() }>{text::attach_tab()}</button>
                        <span class="grow"></span>
                        <button on:click=move |_| store.dialog.set(false)>{text::cancel()}</button>
                    </div>
                    <Show when=move || mode.get() == Mode::Program fallback=move || view! {
                        <div class="bar"><button on:click=move |_| load_processes()>{text::reload()}</button></div>
                        <ul class="files processes">{process_rows}</ul>
                        <div class="bar">
                            <LogOptions/>
                            // 観察専用（D-089）: 共有の試験環境で、デバッガーがデータを変えないアタッチ。
                            <label class="toggle" title=text::observe_only_hint()>
                                <input type="checkbox" class="observe-only" prop:checked=move || observe.get() on:change=move |e| observe.set(event_target_checked(&e)) />
                                {text::observe_only()}
                            </label>
                            <span class="grow"></span>
                            <button class="primary" disabled={move || selected_pid().is_none() || live()}
                                on:click=move |_| { if let Some(pid) = selected_pid() { actions::start(store, json!({ "attach": pid, "observeOnly": observe.get_untracked() })) } }>{text::attach()}</button>
                        </div>
                    }>
                        <div class="bar">
                            <button on:click=move |_| browse(String::new())>{text::drives()}</button>
                            <button disabled=move || parent.with(Option::is_none) on:click=move |_| browse(parent.get_untracked().unwrap_or_default())>{text::up()}</button>
                            <span class="folder">{move || store.folder.get()}</span>
                        </div>
                        <ul class="files">{entry_rows}</ul>
                        <label>{text::program()}<input prop:value=move || program.get() on:input=move |e| program.set(event_target_value(&e)) /></label>
                        <label>{text::arguments()}<input prop:value=move || arguments.get() on:input=move |e| arguments.set(event_target_value(&e)) /></label>
                        <div class="bar">
                            <LogOptions/>
                            <span class="grow"></span>
                            <button class="primary" disabled=move || program.with(String::is_empty) || live() on:click=move |_| launch()>{text::launch()}</button>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

/// 起動とアタッチの前に選ぶ、SQL と例外の記録と、カバレッジ（ブラウザに覚えておく）。
#[component]
fn LogOptions() -> impl IntoView {
    let store = store();
    view! {
        <label class="toggle" title=text::log_hint()>
            <input type="checkbox" prop:checked=move || store.sql_log.get() on:change=move |e| actions::set_record_log(store, true, event_target_checked(&e)) />
            {text::log_sql()}
        </label>
        <label class="toggle">
            <input type="checkbox" prop:checked=move || store.exception_log.get() on:change=move |e| actions::set_record_log(store, false, event_target_checked(&e)) />
            {text::log_exceptions()}
        </label>
        <label class="toggle" title=text::coverage_option_hint()>
            <input type="checkbox" class="coverage-option" prop:checked=move || store.coverage_on.get() on:change=move |e| actions::set_coverage_option(store, event_target_checked(&e)) />
            {text::coverage_option()}
        </label>
        <OutputFolder />
    }
}

/// 成績書とカバレッジの XML の保存先（起動の画面と、成績書のタブの両方に出す。同じ値）。
#[component]
pub fn OutputFolder() -> impl IntoView {
    let store = store();
    view! {
        <input class="grow output-folder" title=text::output_folder_hint() placeholder=text::output_folder_placeholder()
            prop:value=move || store.output_folder.get() on:change=move |e| actions::set_output_folder(store, event_target_value(&e)) />
    }
}