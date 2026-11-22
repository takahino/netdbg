//! 例外の設定の窓（どの例外で止まるか。D-085）。選ぶたびに `actions::set_exception_settings` で覚えて送る。

use crate::actions;
use crate::model::exceptions::{types_of, ExceptionSettings, CATALOG};
use crate::state::{store, Store};
use crate::text;
use leptos::prelude::*;

/// 設定を 1 か所変えて、覚えて送る。
fn change(store: Store, edit: impl FnOnce(&mut ExceptionSettings)) {
    let mut settings = store.exception_settings.get_untracked();
    edit(&mut settings);
    actions::set_exception_settings(store, settings);
}

/// 型 1 つのチェック（`full` は名前空間つきの名前、`label` は見せる名前）。「すべて」のときは選べない。
fn type_checkbox(store: Store, full: String, label: String) -> impl IntoView {
    let checked = {
        let full = full.clone();
        move || store.exception_settings.with(|s| s.checked.contains(&full))
    };
    view! {
        <label class="exception-type">
            <input type="checkbox" prop:checked=checked disabled=move || store.exception_settings.with(|s| s.all) on:change=move |e| { let full = full.clone(); change(store, |s| s.set_checked(&[full], event_target_checked(&e))) } />
            {label}
        </label>
    }
}

/// 名前空間 1 つ: 見出しのチェック（すべて選ぶ / 外す）と、型ごとのチェック。
fn namespace_group(store: Store, namespace: &'static str, types: &'static [&'static str]) -> impl IntoView {
    let all = types_of(namespace);
    let group_checked = {
        let all = all.clone();
        move || store.exception_settings.with(|s| all.iter().all(|t| s.checked.contains(t)))
    };
    let disabled = move || store.exception_settings.with(|s| s.all);
    let rows = types.iter().map(|name| type_checkbox(store, format!("{namespace}.{name}"), name.to_string())).collect::<Vec<_>>();
    view! {
        <details class="exception-group">
            <summary>
                <input type="checkbox" prop:checked=group_checked disabled=disabled on:click=|e| e.stop_propagation() on:change=move |e| { let all = all.clone(); change(store, |s| s.set_checked(&all, event_target_checked(&e))) } />
                {namespace}
            </summary>
            {rows}
        </details>
    }
}

#[component]
pub fn ExceptionsDialog() -> impl IntoView {
    let store = store();
    let added = RwSignal::new(String::new());
    let invalid = RwSignal::new(false);
    let add = move || {
        let name = added.get_untracked();
        let mut ok = false;
        change(store, |s| ok = s.add_custom(&name));
        invalid.set(!ok);
        if ok {
            added.set(String::new());
        }
    };
    let custom = move || {
        store.exception_settings.with(|s| s.custom.clone()).into_iter().map(|name| {
            let remove = name.clone();
            view! {
                <div class="exception-custom">
                    {type_checkbox(store, name.clone(), name)}
                    <button class="link" on:click=move |_| { let r = remove.clone(); change(store, |s| s.remove_custom(&r)) }>{text::exceptions_remove()}</button>
                </div>
            }
        }).collect::<Vec<_>>()
    };
    view! {
        <Show when=move || store.exception_dialog.get()>
            <div class="backdrop" on:click=move |_| store.exception_dialog.set(false)>
                <div class="dialog small exceptions" on:click=|e| e.stop_propagation()>
                    <div class="tabs">
                        <h2>{text::exceptions_title()}</h2>
                        <span class="grow"></span>
                        <button on:click=move |_| store.exception_dialog.set(false)>{text::close()}</button>
                    </div>
                    <div class="pane">
                        <p class="hint">{text::exceptions_hint()}</p>
                        <label>
                            <input type="checkbox" class="exceptions-user-unhandled" prop:checked=move || store.exception_settings.with(|s| s.user_unhandled) on:change=move |e| change(store, |s| s.user_unhandled = event_target_checked(&e)) />
                            {text::exceptions_user_unhandled()}
                        </label>
                        <label>
                            <input type="checkbox" class="exceptions-all" prop:checked=move || store.exception_settings.with(|s| s.all) on:change=move |e| change(store, |s| s.all = event_target_checked(&e)) />
                            {text::exceptions_all()}
                        </label>
                        <h3>{text::exceptions_types()}</h3>
                        {CATALOG.iter().map(|(namespace, types)| namespace_group(store, namespace, types)).collect::<Vec<_>>()}
                        <details class="exception-group" open=true>
                            <summary>{text::exceptions_added()}</summary>
                            {custom}
                            <div class="exception-add">
                                <input type="text" class="exceptions-add" placeholder=text::exceptions_add_placeholder() prop:value=move || added.get() on:input=move |e| added.set(event_target_value(&e)) on:keydown=move |e| { if e.key() == "Enter" { add() } } />
                                <button on:click=move |_| add()>{text::exceptions_add()}</button>
                            </div>
                            <Show when=move || invalid.get()>
                                <p class="hint error-text">{text::exceptions_invalid()}</p>
                            </Show>
                        </details>
                    </div>
                </div>
            </div>
        </Show>
    }
}
