//! 変数の木と、ウォッチ。

use crate::actions;
use crate::model::variables;
use crate::state::store;
use crate::text;
use super::format::text_of;
use leptos::prelude::*;

/// 値の木（変数の欄と、ソースの名前にマウスを載せたときの値の窓）。根のうち値の無いもの（スコープ）は見出しにする。
#[component]
pub fn VariableTree(tree: RwSignal<Vec<variables::Node>>) -> impl IntoView {
    let store = store();
    let rows = move || {
        tree.with(|roots| variables::rows(roots))
            .into_iter()
            .map(|row| {
                let path = row.path.clone();
                let arrow = match (row.expandable, row.expanded) {
                    (false, _) => " ",
                    (true, false) => "▶",
                    (true, true) => "▼",
                };
                let heading = row.depth == 0 && row.value.is_empty();
                // 中身のある値（オブジェクト、配列）は、保持している経路を読める。
                let reference = row.reference;
                let object = row.expandable && !heading;
                // オブジェクトの中のフィールド（ローカル変数ではない）は、書き込みを見張れる（D-093）。
                let field = !heading && !row.parent_is_root && row.parent != 0;
                let (parent, field_name) = (row.parent, row.name.clone());
                view! {
                    <li class:scope=heading style:padding-left=format!("{}px", 6 + row.depth * 14) on:click=move |_| actions::toggle_node(tree, path.clone())>
                        <span class="arrow">{arrow}</span>
                        <span class="name">{row.name.clone()}</span>
                        {(!heading).then(|| view! { <span class="eq">" = "</span> })}
                        <span class="value" title=row.type_name.clone()>{row.value.clone()}</span>
                        {field.then(|| view! {
                            <button class="mini watch-writes" title=text::watch_writes_title() on:click=move |e| { e.stop_propagation(); actions::watch_writes(store, parent, field_name.clone()) }>{text::watch_writes()}</button>
                        })}
                        {object.then(|| view! {
                            <button class="mini" title=text::retention_title() on:click=move |e| { e.stop_propagation(); actions::retention_path(store, reference) }>{text::retention()}</button>
                        })}
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! { <ul class="list tree">{rows}</ul> }
}

#[component]
pub fn VariablesView() -> impl IntoView {
    let store = store();
    view! {
        <Show when=move || store.data_watch.with(Option::is_some)>
            <p class="hint data-watch">
                <span>{move || store.data_watch.get().unwrap_or_default()}</span>
                <button class="close" on:click=move |_| actions::clear_write_watch(store)>{text::close()}</button>
            </p>
        </Show>
        <Show when=move || store.variables.with(|v| !v.is_empty()) fallback=|| view! { <p class="empty">{text::not_stopped()}</p> }>
            <VariableTree tree=store.variables/>
        </Show>
    }
}

#[component]
pub fn WatchView() -> impl IntoView {
    let store = store();
    let input = RwSignal::new(String::new());
    let rows = move || {
        store.watches.get().into_iter().enumerate().map(|(index, watch)| {
            let (value, failed) = match &watch.result {
                Some(Ok(value)) => (value.clone(), false),
                Some(Err(message)) => (message.clone(), true),
                None => (String::new(), false),
            };
            view! {
                <li>
                    <span class="name">{watch.expression.clone()}</span>
                    <span class="eq">" = "</span>
                    <span class="value" class:failed=failed>{value}</span>
                    <button class="close" on:click=move |_| actions::remove_watch(store, index)>{text::close()}</button>
                </li>
            }
        }).collect::<Vec<_>>()
    };
    view! {
        <ul class="list watch">{rows}</ul>
        <input
            class="expression"
            placeholder=text::add_watch()
            prop:value=move || input.get()
            on:input=move |e| input.set(event_target_value(&e))
            on:keydown=move |e| {
                if e.key() == "Enter" {
                    actions::add_watch(store, input.get_untracked());
                    input.set(String::new());
                }
            }
        />
    }
}

/// 保持している経路の窓: GC のルート（ローカル変数、静的フィールド、ハンドルなど）から、選んだオブジェクトまでの並び。
#[component]
pub fn RetentionPanel() -> impl IntoView {
    let store = store();
    let content = move || {
        let answer = store.retention.get()?;

        let target = format!("{} ({})", text_of(&answer["target"]["type"]), text_of(&answer["target"]["address"]));
        let body = if answer["found"] == true {
            let root = &answer["root"];
            let local = &root["local"];
            let root_text = if local.is_null() { text_of(&root["kind"]) } else { format!("{}: {} ({})", text_of(&root["kind"]), text_of(&local["name"]), text_of(&local["function"])) };
            let steps: Vec<AnyView> = answer["path"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|step| view! { <li><span class="value">{text_of(&step["type"])}</span> " " <span class="subtle">{text_of(&step["next"])}</span></li> }.into_any())
                .collect();
            view! {
                <p>{root_text}</p>
                <ol class="list retention">{steps}</ol>
                <p class="hint">{crate::i18n::trf("{length} steps from a root ({visited} objects examined)", &[("length", text_of(&answer["length"])), ("visited", text_of(&answer["visited"]))])}</p>
            }
            .into_any()
        } else {
            view! { <p>{format!("{}: {}", text::retention_not_found(), text_of(&answer["reason"]))}</p> }.into_any()
        };
        Some(view! {
            <div class="backdrop" on:click=move |_| store.retention.set(None)>
                <div class="dialog small" on:click=|e| e.stop_propagation()>
                    <div class="tabs">
                        <h2>{text::retention_title()}</h2>
                        <span class="grow"></span>
                        <button on:click=move |_| store.retention.set(None)>{text::cancel()}</button>
                    </div>
                    <div class="pane">
                        <p class="value">{target}</p>
                        {body}
                    </div>
                </div>
            </div>
        })
    };
    view! { {content} }
}