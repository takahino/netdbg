//! 差分のタブ（B2、D-080）: 前の版の exe（フォルダー）を入れて、いまデバッグしているプログラムと比べる。変わった /
//! 足された / 消えたメソッドに、カバレッジの印（● 通った、○ 再試験の範囲）を付け、押すとそのソースを開く。

use crate::actions;
use crate::model::diff;
use crate::state::store;
use crate::text;
use leptos::prelude::*;

#[component]
pub fn DiffView() -> impl IntoView {
    let store = store();
    let input = RwSignal::new(store.diff_old.get_untracked());
    let run = move || {
        let old = input.get_untracked().trim().trim_matches('"').to_string();
        if !old.is_empty() {
            actions::run_diff(store, old);
        }
    };
    let rows = move || {
        let retest_only = store.diff_retest_only.get();
        store.diff.with(|body| body.as_ref().map(|b| diff::rows(b, retest_only)).unwrap_or_default()).into_iter().map(|row| {
            let (mark, title) = row.mark();
            let place = row.source.clone();
            let clickable = place.is_some();
            view! {
                <tr class:retest=row.retest class:clickable=clickable on:click=move |_| { if let Some((path, line)) = place.clone() { actions::open_source(store, path, line) } }>
                    <td><span class=format!("badge diff-{}", row.kind)>{row.kind_text()}</span></td>
                    <td class="diff-mark" title=title>{mark}</td>
                    <td class="method">{row.method.clone()}</td>
                    <td class="assembly">{row.assembly.clone()}</td>
                </tr>
            }
        }).collect::<Vec<_>>()
    };
    view! {
        <div class="diff">
            <div class="bar">
                <input class="diff-old grow" placeholder=text::diff_old_placeholder() prop:value=move || input.get()
                    on:input=move |e| input.set(event_target_value(&e))
                    on:keydown=move |e| { if e.key() == "Enter" { run() } } />
                <button class="primary" on:click=move |_| run() disabled=move || store.session.with(|s| s.status == crate::model::session::Status::Idle)>{text::diff_run()}</button>
                <label class="toggle"><input type="checkbox" prop:checked=move || store.diff_retest_only.get() on:change=move |e| store.diff_retest_only.set(event_target_checked(&e)) />{text::diff_retest_only()}</label>
            </div>
            <p class="hint">{move || store.diff.with(|b| b.as_ref().map(diff::summary)).unwrap_or_else(|| text::diff_hint().to_string())}</p>
            <table class="grid diff-list"><tbody>{rows}</tbody></table>
        </div>
    }
}
