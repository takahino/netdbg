//! コール スタックとスレッド。

use crate::actions;
use crate::state::store;
use crate::text;
use leptos::prelude::*;

#[component]
pub fn StackView() -> impl IntoView {
    let store = store();
    let rows = move || {
        let selected = store.frame.get();
        store
            .frames
            .get()
            .into_iter()
            .map(|frame| {
                let place = match &frame.path {
                    Some(path) => format!("{}:{}", crate::model::paths::file_name(path), frame.line),
                    None => String::new(),
                };
                let origin = frame.origin.clone().unwrap_or_default();
                let subtle = frame.path.is_none() || frame.hint.as_deref() == Some("subtle");
                let label = frame.hint.as_deref() == Some("label");
                let current = selected == Some(frame.id);
                view! {
                    <li class:selected=current class:subtle=subtle class:label=label on:click=move |_| actions::select_frame(store, frame.clone())>
                        <span class="name">{frame.name.clone()}</span>
                        <span class="place">{place}</span>
                        <span class="origin">{origin}</span>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <Show when=move || store.frames.with(|f| !f.is_empty()) fallback=|| view! { <p class="empty">{text::not_stopped()}</p> }>
            <ul class="list stack">{rows}</ul>
        </Show>
    }
}

#[component]
pub fn ThreadsView() -> impl IntoView {
    let store = store();
    let rows = move || {
        let selected = store.thread.get();
        store
            .threads
            .get()
            .into_iter()
            .map(|thread| {
                let id = thread.id;
                view! {
                    <li class:selected=selected == Some(id) on:click=move |_| actions::select_thread(store, id)>
                        <span class="id">{id}</span>
                        <span class="name">{thread.name.clone()}</span>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <Show when=move || store.threads.with(|t| !t.is_empty()) fallback=|| view! { <p class="empty">{text::not_stopped()}</p> }>
            <ul class="list threads">{rows}</ul>
        </Show>
    }
}
