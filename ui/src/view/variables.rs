//! 変数の木と、ウォッチ。

use crate::actions;
use crate::model::variables::{self, Path};
use crate::state::store;
use crate::text;
use super::format::text_of;
use leptos::html::Input;
use leptos::prelude::*;

/// 止まっていて、値を書いてよいとき（観察専用とダンプは欄を出さない。エンジンも断る）。
fn can_edit() -> bool {
    let store = store();
    store.session.with(|s| s.is_stopped() && !s.dump && !s.observe_only)
}

/// 値の文字を、その場の式の入力にする。空の確定と Escape は送らず、元の表示に戻す。
fn value_editor(draft: RwSignal<String>, on_commit: impl Fn(String) + 'static, on_cancel: impl Fn() + 'static) -> impl IntoView {
    let field = NodeRef::<Input>::new();
    let suppress_blur = RwSignal::new(false);
    Effect::new(move |_| {
        if let Some(input) = field.get() {
            let _ = input.focus();
            let _ = input.select();
        }
    });
    let commit = on_commit;
    let cancel = std::rc::Rc::new(on_cancel);
    let cancel_key = cancel.clone();
    view! {
        <input
            class="value-edit"
            node_ref=field
            title=text::edit_value()
            prop:value=move || draft.get()
            on:input=move |e| draft.set(event_target_value(&e))
            on:click=|e| e.stop_propagation()
            on:keydown=move |e| {
                e.stop_propagation();
                if e.key() == "Escape" {
                    suppress_blur.set(true);
                    cancel_key();
                } else if e.key() == "Enter" {
                    suppress_blur.set(true);
                    commit(draft.get_untracked());
                }
            }
            on:blur=move |_| {
                if suppress_blur.get_untracked() {
                    suppress_blur.set(false);
                } else {
                    cancel();
                }
            }
        />
    }
}

/// 値の木（変数の欄と、ソースの名前にマウスを載せたときの値の窓）。根のうち値の無いもの（スコープ）は見出しにする。
/// `editable` の木だけ、値を押すとその場で式を書ける（値の窓は読むだけ）。
#[component]
pub fn VariableTree(tree: RwSignal<Vec<variables::Node>>, #[prop(default = false)] editable: bool) -> impl IntoView {
    let store = store();
    let editing = RwSignal::new(None::<Path>);
    let draft = RwSignal::new(String::new());
    let rows = move || {
        let current = editing.get();
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
                let open = editable && !heading && parent != 0 && can_edit();
                let editing_this = current.as_ref() == Some(&path);
                let shown = row.value.clone();
                let type_name = row.type_name.clone();
                let path_click = path.clone();
                let editor = editing_this.then(|| {
                    let name = row.name.clone();
                    value_editor(
                        draft,
                        move |text| {
                            editing.set(None);
                            actions::assign_variable(store, parent, name.clone(), text);
                        },
                        move || editing.set(None),
                    )
                });
                view! {
                    <li class:scope=heading style:padding-left=format!("{}px", 6 + row.depth * 14) on:click=move |_| actions::toggle_node(tree, path_click.clone())>
                        <span class="arrow">{arrow}</span>
                        <span class="name">{row.name.clone()}</span>
                        {(!heading).then(|| view! { <span class="eq">" = "</span> })}
                        {if let Some(editor) = editor {
                            editor.into_any()
                        } else {
                            let path_edit = path.clone();
                            let value_edit = shown.clone();
                            view! {
                                <span
                                    class="value"
                                    class:editable=open
                                    title=move || if open { text::edit_value().to_string() } else { type_name.clone() }
                                    on:click=move |e| {
                                        if open {
                                            e.stop_propagation();
                                            draft.set(value_edit.clone());
                                            editing.set(Some(path_edit.clone()));
                                        }
                                    }
                                >{shown.clone()}</span>
                            }.into_any()
                        }}
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
            <VariableTree tree=store.variables editable=true/>
        </Show>
    }
}

#[component]
pub fn WatchView() -> impl IntoView {
    let store = store();
    let input = RwSignal::new(String::new());
    let editing = RwSignal::new(None::<usize>);
    let draft = RwSignal::new(String::new());
    let rows = move || {
        let current = editing.get();
        store.watches.get().into_iter().enumerate().map(|(index, watch)| {
            let (value, failed) = match &watch.result {
                Some(Ok(value)) => (value.clone(), false),
                Some(Err(message)) => (message.clone(), true),
                None => (String::new(), false),
            };
            let open = can_edit();
            let expression = watch.expression.clone();
            let shown = value.clone();
            let body = if current == Some(index) {
                let expression = expression.clone();
                value_editor(
                    draft,
                    move |text| {
                        editing.set(None);
                        actions::assign_expression(store, expression.clone(), text);
                    },
                    move || editing.set(None),
                )
                .into_any()
            } else {
                let prefill = shown.clone();
                view! {
                    <span
                        class="value"
                        class:failed=failed
                        class:editable=open
                        title=move || if open { text::edit_value().to_string() } else { String::new() }
                        on:click=move |_| {
                            if open {
                                draft.set(prefill.clone());
                                editing.set(Some(index));
                            }
                        }
                    >{shown.clone()}</span>
                }
                .into_any()
            };
            view! {
                <li>
                    <span class="name">{expression}</span>
                    <span class="eq">" = "</span>
                    {body}
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