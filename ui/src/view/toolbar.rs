//! ツールバー（開く、続行、ステップ、止める、終了）と、状態の表示。F5 / F10 / F11 も、ここで受ける。

use crate::actions;
use crate::model::session::Status;
use crate::state::store;
use crate::text;
use leptos::ev;
use leptos::prelude::*;

#[component]
pub fn Toolbar() -> impl IntoView {
    let store = store();
    let stopped = move || store.session.with(|s| s.is_stopped());
    let running = move || store.session.with(|s| s.is_running());
    let live = move || store.session.with(|s| s.is_live());
    let dump = move || store.session.with(|s| s.dump);
    // ブラウザの F5（再読み込み）などを、デバッガーの操作にする。
    let keys = window_event_listener(ev::keydown, move |e| {
        let command = match (e.key().as_str(), e.shift_key()) {
            ("F5", false) if stopped() && !dump() => Some("continue"),
            ("F5", true) if live() => {
                e.prevent_default();
                actions::stop(store);
                None
            }
            ("F10", _) if stopped() && !dump() => Some("next"),
            ("F11", false) if stopped() && !dump() => Some("stepIn"),
            ("F11", true) if stopped() && !dump() => Some("stepOut"),
            ("F5" | "F10" | "F11", _) => {
                e.prevent_default();
                None
            }
            _ => None,
        };
        if let Some(command) = command {
            e.prevent_default();
            actions::run(store, command);
        }
    });
    on_cleanup(move || keys.remove());
    let status = move || {
        store.session.with(|s| {
            let state = match &s.status {
                Status::Idle => text::idle().to_string(),
                Status::Starting => text::starting().to_string(),
                Status::Running => text::running().to_string(),
                Status::Stopped { reason, text, .. } if !text.is_empty() => format!("{} ({reason}): {text}", text::stopped()),
                Status::Stopped { reason, .. } => format!("{} ({reason})", text::stopped()),
                Status::Ended(Some(code)) => crate::i18n::trf("{state} (exit code {code})", &[("state", text::ended().to_string()), ("code", code.to_string())]),
                Status::Ended(None) if store.last_start.with_untracked(|w| w.as_ref().is_some_and(|w| w.get("attach").is_some())) => text::detached().to_string(),
                Status::Ended(None) => text::ended().to_string(),
            };
            let dump = if s.dump { format!(" · {}", text::dump()) } else { String::new() };
            let dump = if s.observe_only { format!("{dump} · {}", text::observe_only_badge()) } else { dump };
            if s.process_name.is_empty() {
                state
            } else {
                format!("{state} · {}{dump}", crate::model::paths::file_name(&s.process_name))
            }
        })
    };
    let can_step = move || stopped() && !dump();
    let attached = move || store.last_start.with(|w| w.as_ref().is_some_and(|w| w.get("attach").is_some()));
    // 止め方は、始め方で変わる（Visual Studio の「デバッグの停止」と同じ。アタッチしたものは終わらせない）。
    let stop_label = move || if dump() { text::close_dump() } else if attached() { text::detach() } else { text::terminate() };
    let stop_title = move || if attached() { text::detach_title() } else { text::terminate_title() };
    view! {
        <header class="toolbar">
            <button on:click=move |_| store.dialog.set(true)>{text::open()}</button>
            <button
                on:click=move |_| { if let Some(what) = store.last_start.get_untracked() { actions::start(store, what) } }
                disabled=move || live() || store.last_start.with(Option::is_none)
            >
                {text::restart()}
            </button>
            <span class="sep"></span>
            <button on:click=move |_| actions::run(store, "continue") disabled=move || !can_step()>{text::resume()}</button>
            <button on:click=move |_| actions::run(store, "pause") disabled=move || !running()>{text::pause()}</button>
            <button on:click=move |_| actions::run(store, "next") disabled=move || !can_step()>{text::step_over()}</button>
            <button on:click=move |_| actions::run(store, "stepIn") disabled=move || !can_step()>{text::step_in()}</button>
            <button on:click=move |_| actions::run(store, "stepOut") disabled=move || !can_step()>{text::step_out()}</button>
            <button title=stop_title on:click=move |_| actions::stop(store) disabled=move || !live()>{stop_label}</button>
            <span class="sep"></span>
            // 何かで止まる設定なら、印を付ける（捕まえた例外で止まるのは、ここで選んだときだけ）。
            <button class="exceptions-button" class:active=move || store.exception_settings.with(|s| s.is_set()) title=text::exceptions_button_title() on:click=move |_| store.exception_dialog.set(true)>{text::exceptions_button()}</button>
            <span class="sep"></span>
            <span class="status" class:stopped=stopped>{status}</span>
            <Show when=move || store.session.with(|s| s.coverage.is_some())>
                <span class="coverage-summary" title=move || store.session.with(|s| s.coverage.as_ref().map(|t| format!("{}{}", text::coverage_hint(), t.path.as_deref().map(|p| format!("\n{p}")).unwrap_or_default())).unwrap_or_default())>
                    {move || store.session.with(|s| s.coverage.as_ref().map(|t| t.summary()).unwrap_or_default())}
                </span>
                <button on:click=move |_| actions::write_coverage(store)>{text::coverage_write()}</button>
            </Show>
            <span class="grow"></span>
            <button title=text::quit_title() on:click=move |_| actions::quit(store)>{text::quit()}</button>
        </header>
        <Show when=move || store.error.with(Option::is_some)>
            <div class="error">
                <span>{move || store.error.get().unwrap_or_default()}</span>
                <Show when=move || store.relaunch.with(Option::is_some)>
                    <button on:click=move |_| actions::relaunch(store)>{text::relaunch()}</button>
                </Show>
                <button class="close" on:click=move |_| store.error.set(None)>{text::close()}</button>
            </div>
        </Show>
    }
}
