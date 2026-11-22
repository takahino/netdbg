//! 成績書のタブ（B3、D-081）: 番号と項目名を入れて始め、写し、OK / NG で終える。項目の一覧と、記録中の項目の
//! 操作（AI、手、手順）がその場で増える様子を出す。

use crate::actions;
use crate::state::store;
use crate::text;
use leptos::prelude::*;

#[component]
pub fn ReportView() -> impl IntoView {
    let store = store();
    let recording = move || store.session.with(|s| s.report.open.is_some());
    let input = |signal: RwSignal<String>, class: &'static str, placeholder: &'static str| {
        view! { <input class=class placeholder=placeholder prop:value=move || signal.get() on:input=move |e| signal.set(event_target_value(&e)) /> }
    };
    let items = move || {
        store.session.with(|s| s.report.items.clone()).into_iter().map(|item| {
            let result = item.result.clone().unwrap_or_else(|| text::report_recording().trim_end_matches(": ").to_string());
            let class = format!("badge report-{}", item.result.as_deref().unwrap_or("open").to_ascii_lowercase());
            view! {
                <tr>
                    <td class="report-id">{item.id.clone()}</td>
                    <td class="grow">{item.title.clone()}</td>
                    <td><span class=class>{result}</span></td>
                    <td class="num">{item.duration_text()}</td>
                    <td class="num" title=crate::i18n::tr("Actions")>{crate::i18n::trf("Actions {n}", &[("n", item.operations.to_string())])}</td>
                    <td class="num" title=crate::i18n::tr("Pictures")>{crate::i18n::trf("Pictures {n}", &[("n", item.pictures.to_string())])}</td>
                    <td class="num" title=crate::i18n::tr("Changed controls")>{crate::i18n::trf("Changes {n}", &[("n", item.changes.to_string())])}</td>
                </tr>
            }
        }).collect::<Vec<_>>()
    };
    let operations = move || {
        store.session.with(|s| s.report.open.as_ref().map(|(_, _, ops)| ops.clone()).unwrap_or_default()).into_iter().map(|o| {
            let class = match o.kind.as_str() {
                "ai" => "actor actor-ai",
                "hand" => "actor actor-hand",
                _ => "actor actor-note",
            };
            let shots = (o.pictures > 0).then(|| view! { <span class="report-shots" title=crate::i18n::tr("Pictures taken (the operated spot, and after the action)")>{crate::i18n::trf("Pictures {n}", &[("n", o.pictures.to_string())])}</span> });
            // もう一度流せる操作には印（D-086）。
            let replayable = o.replayable.then(|| view! { <span class="report-replayable" title=text::replayable_title()>"↻"</span> });
            view! { <li><span class="time">{o.time}</span><span class=class>{o.actor}</span>{o.text}{replayable}{shots}</li> }
        }).collect::<Vec<_>>()
    };
    let heading = move || store.session.with(|s| s.report.open.as_ref().map(|(id, title, _)| format!("{}{id} {title}", text::report_recording())));
    view! {
        <div class="report">
            <div class="bar">
                {input(store.report_id, "report-id-input", text::report_id_placeholder())}
                {input(store.report_title, "grow", text::report_title_placeholder())}
                <button class="primary report-start" on:click=move |_| actions::start_test_item(store) disabled=recording>{text::report_start()}</button>
                <button class="report-capture" on:click=move |_| actions::capture_test_item(store) disabled=move || !recording()>{text::report_capture()}</button>
            </div>
            <div class="bar">
                {input(store.report_note, "grow", text::report_note_placeholder())}
                <button class="report-ok" on:click=move |_| actions::end_test_item(store, "OK") disabled=move || !recording()>{text::report_ok()}</button>
                <button class="report-ng" on:click=move |_| actions::end_test_item(store, "NG") disabled=move || !recording()>{text::report_ng()}</button>
                <button class="report-write" on:click=move |_| actions::write_test_report(store) disabled=move || store.session.with(|s| s.report.items.is_empty())>{text::report_write()}</button>
            </div>
            <div class="bar">
                <crate::view::open::OutputFolder />
            </div>
            <div class="bar">
                <input class="grow report-template" title=text::report_template_hint() placeholder=text::report_template_placeholder()
                    prop:value=move || store.report_template.get() on:change=move |e| actions::set_report_template(store, event_target_value(&e)) />
                <button class="report-template-export" title=text::report_template_hint() on:click=move |_| actions::export_report_template(store)>{text::report_template_export()}</button>
            </div>
            // 手順を流す（D-086）: 書いた成績書の手順（AI と手の操作）を、動いている対象にもう一度流す。
            <div class="bar">
                <input class="grow replay-file" title=text::replay_hint() placeholder=text::replay_file_placeholder()
                    prop:value=move || store.replay_file.get() on:change=move |e| actions::set_replay_file(store, event_target_value(&e)) />
                {input(store.replay_item, "report-id-input replay-item", text::replay_item_placeholder())}
                <button class="replay" title=text::replay_hint() on:click=move |_| actions::replay_steps(store) disabled=move || !store.session.with(|s| s.is_running())>{text::replay()}</button>
            </div>
            <p class="hint report-status">{move || heading().or_else(|| store.report_written.get()).unwrap_or_else(|| {
                let summary = store.session.with(|s| (!s.report.items.is_empty()).then(|| s.report.summary()));
                summary.unwrap_or_else(|| text::report_hint().to_string())
            })}</p>
            <div class="report-body">
                <table class="grid report-list"><tbody>{items}</tbody></table>
                <ol class="report-operations" reversed>{operations}</ol>
            </div>
        </div>
    }
}
