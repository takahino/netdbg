//! 対象の画面（WinForms / WPF）の木と写し。木でコントロールを選ぶと、写しの上にその枠を描く。

use crate::actions;
use crate::api;
use crate::state::store;
use crate::text;
use leptos::prelude::*;

#[component]
pub fn WindowsView() -> impl IntoView {
    let store = store();
    let rows = move || {
        let Some(tree) = store.tree.get() else { return Vec::new() };
        let selected = store.control.get();
        tree.elements
            .iter()
            .map(|element| {
                let mark = element.mark;
                view! {
                    <li class:selected=selected == Some(mark) class:disabled=!element.enabled style:padding-left=format!("{}px", 6 + element.depth * 14) on:click=move |_| store.control.set(Some(mark))>
                        <span class="kind">{element.label()}</span>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    // 選んだコントロールの窓（無ければ最初の窓）の写しと、枠。
    let shot = move || {
        let tree = store.tree.get()?;
        let element = store.control.get().and_then(|mark| tree.elements.iter().find(|e| e.mark == mark).cloned());
        let window = element.as_ref().map(|e| e.window).or(tree.windows.first().map(|w| w.id))?;
        let frame = element.as_ref().and_then(|e| tree.overlay(e));
        let url = api::capture_url(window, store.shot.get());
        Some(view! {
            <div class="shot">
                <img src=url alt="" />
                {frame.map(|r| view! { <div class="frame" style:left=format!("{}%", r.x) style:top=format!("{}%", r.y) style:width=format!("{}%", r.width) style:height=format!("{}%", r.height)></div> })}
            </div>
        })
    };
    view! {
        <div class="windows">
            <div class="tree-side">
                <div class="bar">
                    <button on:click=move |_| actions::load_tree(store) disabled=move || store.session.with(|s| s.process_id.is_none())>{text::load_tree()}</button>
                </div>
                // 前の停止から変わったもの（D-087。止まるたびにエンジンが知らせる）。
                {move || store.session.with(|s| s.tree_changes.clone()).map(|c| {
                    let heading = if c.compared { crate::i18n::trf("Changed since the previous stop (stop {n})", &[("n", c.stop.to_string())]) } else { text::tree_changes_first().to_string() };
                    let empty = c.compared && c.changes.is_empty() && c.unread.is_empty();
                    view! {
                        <div class="tree-changes">
                            <p class="hint">{heading}</p>
                            {empty.then(|| view! { <p class="hint">{text::tree_changes_none()}</p> })}
                            <ul class="list">{c.changes.into_iter().map(|t| view! { <li>{t}</li> }).collect::<Vec<_>>()}</ul>
                            {(!c.unread.is_empty()).then(|| view! { <p class="hint">{crate::i18n::trf("Changed just before the stop (the value can be read after continuing): {controls}", &[("controls", c.unread.join(", "))])}</p> })}
                        </div>
                    }
                })}                <Show when=move || store.tree.with(|t| t.as_ref().is_some_and(|t| !t.running))>
                    <p class="hint">{text::tree_stopped()}</p>
                </Show>
                <ul class="list controls">{rows}</ul>
            </div>
            <div class="shot-side">{shot}</div>
        </div>
    }
}
