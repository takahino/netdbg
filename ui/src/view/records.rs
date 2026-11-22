//! 止めずに読んだ記録（段階 11、12）: SQL、例外、イベント、ダイアログ、トランザクション、データバインディング、
//! 子プロセス、UI のフリーズ、環境を、起きた順の 1 本の一覧にする。種類と文字で絞り、選んだ 1 件の中身を右に出す。
//! SQL と例外の記録の切り替えと、「SQL で止める」「ダイアログで止める」の条件も、ここで置く。

use crate::actions;
use crate::model::records::{Kind, Record};
use crate::state::{store, Store};
use crate::text;
use super::format::{clock, text_of};
use leptos::prelude::*;
use serde_json::Value;

/// 一覧に描く記録の数（新しいものから。それより古いものは、絞り込むと出る）。
const SHOWN: usize = 500;
/// これより長くかかった SQL は、色を変える。
const SLOW_SQL_MS: f64 = 1000.0;

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Sql => text::record_sql(),
        Kind::Exception => text::record_exception(),
        Kind::Event => text::record_event(),
        Kind::Dialog => text::record_dialog(),
        Kind::Transaction => text::record_transaction(),
        Kind::Binding => text::record_binding(),
        Kind::Process => text::record_process(),
        Kind::UiFreeze => text::record_freeze(),
        Kind::Environment => text::record_environment(),
    }
}


#[component]
pub fn RecordsView() -> impl IntoView {
    let store = store();
    // 記録が変わったときだけ作り直す（出力の行など、セッションのほかの変化では作り直さない。記録は写さない）。
    let revision = Memo::new(move |_| store.session.with(|s| s.records.revision));
    let shown = Memo::new(move |_| {
        revision.track();
        let kinds = store.record_kinds.get();
        let filter = store.record_filter.get();
        let matching = store.session.with_untracked(|s| s.records.matching(&kinds, &filter));
        let skip = matching.len().saturating_sub(SHOWN);
        matching[skip..].to_vec()
    });
    let empty = move || {
        revision.track();
        store.session.with_untracked(|s| s.records.list.is_empty())
    };
    let chips = move || {
        revision.track();
        Kind::ALL
            .into_iter()
            .map(|kind| {
                let count = store.session.with_untracked(|s| s.records.count(kind));
                let on = move || store.record_kinds.with(|k| k.contains(&kind));
                view! {
                    <button class=format!("chip {}", kind.class()) class:off=move || !on() on:click=move |_| store.record_kinds.update(|k| if !k.remove(&kind) { k.insert(kind); })>
                        {label(kind)} " " <span class="count">{count}</span>
                    </button>
                }
            })
            .collect::<Vec<_>>()
    };
    let rows = move || {
        let selected = store.record.get();
        shown
            .get()
            .into_iter()
            .rev()
            .map(|record| {
                let key = record.key;
                let slow = record.duration_ms().is_some_and(|ms| ms >= SLOW_SQL_MS) || record.body["failed"] == true;
                let caller = record.caller();
                let place = caller.as_ref().and_then(|(_, place)| place.clone());
                let caller_name = caller.map(|(name, _)| name).unwrap_or_default();
                view! {
                    <tr class:selected=selected == Some(key) class:slow=slow class:stopped=record.body["stopped"] == true on:click=move |_| store.record.set(Some(key))>
                        <td class="time">{clock(record.time, true)}</td>
                        <td><span class=format!("badge {}", record.kind.class())>{label(record.kind)}</span></td>
                        <td class="summary">{record.summary()}</td>
                        <td class="thread">{record.thread().map(|t| t.to_string()).unwrap_or_default()}</td>
                        <td class="caller">{place_link(store, caller_name, place)}</td>
                    </tr>
                }
            })
            .collect::<Vec<_>>()
    };
    let detail = move || {
        revision.track();
        let key = store.record.get()?;
        let record = store.session.with_untracked(|s| s.records.by_key(key).cloned())?;
        Some(view! { <RecordDetail record=record/> })
    };
    view! {
        <div class="records">
            <div class="bar wrap">
                {chips}
                <input class="filter" placeholder=text::filter() prop:value=move || store.record_filter.get() on:input=move |e| store.record_filter.set(event_target_value(&e)) />
                <button on:click=move |_| { store.session.update(|s| s.records.clear()); store.record.set(None); }>{text::clear()}</button>
            </div>
            <RecordSettings/>
            <div class="split">
                <div class="list-side">
                    <Show when=move || !empty() fallback=|| view! { <p class="empty">{text::no_records()}</p> }>
                        <table class="record-list"><tbody>{rows}</tbody></table>
                    </Show>
                </div>
                <div class="detail-side">{detail}</div>
            </div>
        </div>
    }
}

/// SQL と例外の記録の切り替えと、止める条件。
#[component]
fn RecordSettings() -> impl IntoView {
    let store = store();
    let toggle = move |sql: bool| {
        let signal = if sql { store.sql_log } else { store.exception_log };
        view! {
            <label class="toggle">
                <input type="checkbox" prop:checked=move || signal.get() on:change=move |e| actions::set_record_log(store, sql, event_target_checked(&e)) />
                {if sql { text::log_sql() } else { text::log_exceptions() }}
            </label>
        }
    };
    view! {
        <div class="bar wrap settings">
            {toggle(true)}
            {toggle(false)}
            <Conditions sql=true/>
            <Conditions sql=false/>
        </div>
    }
}

/// 「SQL で止める」か「ダイアログで止める」の条件（含む文字）の並びと、足す口。
#[component]
fn Conditions(sql: bool) -> impl IntoView {
    let store = store();
    let input = RwSignal::new(String::new());
    let list = move || if sql { store.sql_breaks.get() } else { store.dialog_breaks.get() };
    let add = move || {
        let value = input.get_untracked().trim().to_string();
        if value.is_empty() {
            return;
        }
        let mut conditions = list();
        conditions.push(value);
        actions::set_record_conditions(store, sql, conditions);
        input.set(String::new());
    };
    view! {
        <span class="conditions">
            <input
                class="condition"
                placeholder=if sql { text::break_on_sql() } else { text::break_on_dialog() }
                prop:value=move || input.get()
                on:input=move |e| input.set(event_target_value(&e))
                on:keydown=move |e| if e.key() == "Enter" { add() }
            />
            {move || list().into_iter().enumerate().map(|(index, condition)| view! {
                <span class="tag">
                    {condition}
                    <button class="close" on:click=move |_| { let mut c = list(); c.remove(index); actions::set_record_conditions(store, sql, c); }>{text::close()}</button>
                </span>
            }).collect::<Vec<_>>()}
        </span>
    }
}

/// 詳細の中身: 名前と値の表と、その下の段（文、表、並び）。
#[derive(Default)]
struct Detail {
    fields: Vec<AnyView>,
    sections: Vec<AnyView>,
}

impl Detail {
    /// 名前と値の表の 1 行（値が空なら出さない）。
    fn field(&mut self, name: &'static str, value: String) {
        if !value.is_empty() && value != "null" && value != "false" {
            self.fields.push(view! { <tr><th>{name}</th><td>{value}</td></tr> }.into_any());
        }
    }

    /// 本文の項目を、名前と値の表にまとめて足す（`(見出し, 項目の名前)`）。
    fn fields(&mut self, body: &Value, pairs: &[(&'static str, &str)]) {
        for (name, key) in pairs {
            self.field(name, text_of(&body[*key]));
        }
    }

    /// 見出しつきの段（中身が無ければ出さない）。
    fn section(&mut self, title: &'static str, content: AnyView, empty: bool) {
        if !empty {
            self.sections.push(view! { <h4>{title}</h4>{content} }.into_any());
        }
    }
}

/// 名前（ソースの場所があれば、押すとそのソースを開く）。
fn place_link(store: Store, name: String, place: Option<(String, u32)>) -> AnyView {
    match place {
        Some((file, line)) => view! { <a on:click=move |e| { e.stop_propagation(); actions::open_source(store, file.clone(), line) }>{name}</a> }.into_any(),
        None => view! { <span class="subtle">{name}</span> }.into_any(),
    }
}

/// フレームの JSON（`name`、`source`、`line`）のソースの場所。
fn frame_place(frame: &Value) -> Option<(String, u32)> {
    frame["source"].as_str().map(|source| (source.to_string(), frame["line"].as_u64().unwrap_or(1) as u32))
}

/// 選んだ記録の中身。
#[component]
fn RecordDetail(record: Record) -> impl IntoView {
    let store = store();
    let body = &record.body;
    let mut detail = Detail::default();
    detail.field(text::field_time(), clock(record.time, true));
    detail.field(text::field_thread(), record.thread().map(|t| t.to_string()).unwrap_or_default());
    match record.kind {
        Kind::Sql => sql_detail(store, &record, &mut detail),
        Kind::Exception => exception_detail(store, body, &mut detail),
        Kind::Transaction => transaction_detail(store, body, &mut detail),
        Kind::Environment => environment_detail(body, &mut detail),
        Kind::Dialog => detail.fields(body, &[(text::field_caption(), "caption"), (text::field_text(), "text"), (text::field_form(), "form"), (text::field_method(), "method"), (text::field_stopped(), "stopped")]),
        Kind::Binding => {
            detail.fields(body, &[(text::field_direction(), "direction")]);
            detail.field(text::field_control(), format!("{} ({})", text_of(&body["control"]), text_of(&body["controlType"])));
            detail.fields(body, &[(text::field_property(), "property"), (text::field_member(), "member"), (text::field_data_source(), "dataSource"), (text::field_type(), "exceptionType"), (text::field_message(), "message")]);
        }
        Kind::Event => detail.fields(body, &[(text::field_handler(), "handler"), (text::field_sender(), "sender")]),
        Kind::Process => detail.fields(
            body,
            &[
                (text::field_file(), "fileName"),
                (text::field_arguments(), "arguments"),
                (text::field_kind(), "kind"),
                (text::field_bitness(), "bitness"),
                (text::field_process_id(), "processId"),
                (text::field_followed(), "followed"),
                (text::field_note(), "notFollowed"),
            ],
        ),
        Kind::UiFreeze => detail.field(text::field_duration(), format!("{} ms", text_of(&body["ms"]))),
    }
    if let Some((name, place)) = record.caller() {
        detail.fields.push(view! { <tr><th>{text::field_caller()}</th><td>{place_link(store, name, place)}</td></tr> }.into_any());
    }
    let Detail { fields, sections } = detail;
    view! {
        <div class="record-detail">
            <h3><span class=format!("badge {}", record.kind.class())>{label(record.kind)}</span> " " {record.summary()}</h3>
            <table class="fields"><tbody>{fields}</tbody></table>
            {sections}
        </div>
    }
}

/// SQL: 値を埋めた文、時間、パラメーター、呼び出しの並び。
fn sql_detail(store: Store, record: &Record, detail: &mut Detail) {
    let body = &record.body;
    detail.sections.push(view! { <pre class="sql">{text_of(&body["sql"])}</pre> }.into_any());
    detail.field(text::field_duration(), record.duration_ms().map(|ms| format!("{ms} ms")).unwrap_or_default());
    detail.fields(body, &[(text::field_failed(), "failed"), (text::field_data_source(), "dataSource"), (text::field_command_type(), "commandType"), (text::field_transaction(), "transactionId")]);
    if body["repeated"] == true {
        detail.fields(body, &[(text::field_repeat(), "repeat")]);
    }
    detail.fields(body, &[(text::field_method(), "method")]);
    let parameters: Vec<AnyView> = body["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| view! { <tr><td>{text_of(&p["name"])}</td><td>{text_of(&p["value"])}</td><td>{text_of(&p["kind"])}</td><td>{text_of(&p["direction"])}</td></tr> }.into_any())
        .collect();
    let empty = parameters.is_empty();
    detail.section(text::field_parameters(), view! { <table class="grid"><tbody>{parameters}</tbody></table> }.into_any(), empty);
    let frames: Vec<AnyView> = body["frames"].as_array().into_iter().flatten().map(|f| view! { <li>{place_link(store, text_of(&f["name"]), frame_place(f))}</li> }.into_any()).collect();
    let empty = frames.is_empty();
    detail.section(text::field_frames(), view! { <ul class="list">{frames}</ul> }.into_any(), empty);
}

/// 例外: 型と文言、例外に至る流れ（押すとその記録へ）、投げたときのスタックとローカル変数。
fn exception_detail(store: Store, body: &Value, detail: &mut Detail) {
    detail.fields(body, &[(text::field_type(), "typeName"), (text::field_message(), "message"), (text::field_stopped(), "stopped")]);
    let context: Vec<AnyView> = body["context"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let target = store.session.with_untracked(|s| s.records.by_seq(c["seq"].as_u64().unwrap_or(0)).map(|r| r.key));
            view! {
                <li class:link=target.is_some() on:click=move |_| if let Some(key) = target { store.record.set(Some(key)) }>
                    <span class="time">{clock(c["time"].as_f64().unwrap_or(0.0), true)}</span> " " <span class="kind">{text_of(&c["kind"])}</span> " " {text_of(&c["summary"])}
                </li>
            }
            .into_any()
        })
        .collect();
    let empty = context.is_empty();
    detail.section(text::field_context(), view! { <ul class="list">{context}</ul> }.into_any(), empty);
    let snapshot: Vec<AnyView> = body["snapshot"]["frames"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|frame| {
            let locals: Vec<AnyView> =
                frame["locals"].as_array().into_iter().flatten().map(|l| view! { <div class="local">{format!("{} = {}", text_of(&l["name"]), text_of(&l["value"]))}</div> }.into_any()).collect();
            view! { <li>{place_link(store, text_of(&frame["name"]), frame_place(frame))}{locals}</li> }.into_any()
        })
        .collect();
    let empty = snapshot.is_empty();
    detail.section(text::field_snapshot(), view! { <ul class="list snapshot">{snapshot}</ul> }.into_any(), empty);
}

/// トランザクション: 出来事と時間、中で走った SQL（押すとその記録へ）。
fn transaction_detail(store: Store, body: &Value, detail: &mut Detail) {
    detail.fields(body, &[(text::field_event(), "event"), (text::field_transaction(), "transactionId")]);
    detail.field(text::field_duration(), body["durationMs"].as_f64().map(|ms| format!("{ms} ms")).unwrap_or_default());
    detail.fields(body, &[(text::field_method(), "method"), (text::field_note(), "note")]);
    let statements: Vec<AnyView> = body["statements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|seq| match store.session.with_untracked(|s| s.records.by_seq(seq).map(|r| (r.key, r.summary()))) {
            Some((key, summary)) => view! { <li class="link" on:click=move |_| store.record.set(Some(key))>{summary}</li> }.into_any(),
            None => view! { <li class="subtle">{format!("#{seq}")}</li> }.into_any(),
        })
        .collect();
    let empty = statements.is_empty();
    detail.section(text::field_statements(), view! { <ul class="list">{statements}</ul> }.into_any(), empty);
}

/// 環境: 障害票に書く項目と、接続文字列（パスワードはエンジンが隠したもの）。
fn environment_detail(body: &Value, detail: &mut Detail) {
    detail.fields(
        body,
        &[
            (text::env_user(), "user"),
            (text::env_machine(), "machine"),
            (text::env_os(), "os"),
            (text::env_culture(), "culture"),
            (text::env_time_zone(), "timeZone"),
            (text::field_bitness(), "bitness"),
            (text::env_runtime(), "runtime"),
            (text::env_program(), "program"),
            (text::env_command_line(), "commandLine"),
            (text::env_directory(), "workingDirectory"),
        ],
    );
    let strings: Vec<AnyView> = body["connectionStrings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| view! { <tr><td>{text_of(&c["name"])}</td><td>{text_of(&c["connectionString"])}</td><td>{text_of(&c["providerName"])}</td></tr> }.into_any())
        .collect();
    let empty = strings.is_empty();
    detail.section(text::env_connection_strings(), view! { <table class="grid"><tbody>{strings}</tbody></table> }.into_any(), empty);
}