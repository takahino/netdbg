//! AI（MCP）の操作の履歴。同じエンジンに AI がつないで行った操作（ブレークポイント、続行、式、画面の操作）を、
//! 新しいものを下に並べる。場所を指す操作は、押すとそのソースを開く。

use crate::actions;
use crate::state::store;
use crate::text;
use super::format::clock;
use leptos::prelude::*;


#[component]
pub fn AgentView() -> impl IntoView {
    let store = store();
    let connect = move || {
        store.session.with(|s| match (&s.mcp_url, s.engine_pid) {
            (Some(url), Some(pid)) => crate::i18n::trf("AI clients can connect to this session with MCP at {url} (or netdbg --mcp-connect={pid}).", &[("url", url.to_string()), ("pid", pid.to_string())]),
            _ => String::new(),
        })
    };
    // セッションのほかの変化（出力の行など）では描き直さない。
    let actions = Memo::new(move |_| store.session.with(|s| s.actions.clone()));
    let rows = move || {
        actions.with(|list| {
            list.iter()
                .map(|action| {
                    let arguments = if action.arguments.as_object().is_some_and(|o| !o.is_empty()) { action.arguments.to_string() } else { String::new() };
                    let (state, class, summary) = match &action.finished {
                        None => ("…".to_string(), "running", String::new()),
                        Some((true, ms, summary)) => (format!("{ms} ms"), "ok", summary.clone()),
                        Some((false, ms, summary)) => (format!("{ms} ms"), "failed", summary.clone()),
                    };
                    let location = action.location();
                    let link = location.is_some();
                    view! {
                        <li class=class class:link=link title=summary on:click=move |_| {
                            if let Some((file, line)) = location.clone() {
                                actions::reveal_action(store, file, line);
                            }
                        }>
                            <span class="time">{clock(action.time, false)}</span>
                            <span class="tool">{action.tool.clone()}</span>
                            <span class="arguments">{arguments}</span>
                            <span class="state">{state}</span>
                        </li>
                    }
                })
                .collect::<Vec<_>>()
        })
    };
    view! {
        <p class="hint">{connect}</p>
        <Show when=move || actions.with(|a| !a.is_empty()) fallback=|| view! { <p class="empty">{text::no_actions()}</p> }>
            <ul class="list agent">{rows}</ul>
        </Show>
    }
}
