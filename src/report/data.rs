//! テンプレートに渡す値（D-083、D-084）。成績書の中身を、名前の付いた値にする。
//!
//! 名前は英語（`id`、`items`、`operations` …）。日本語の名前（`番号`、`項目`、`操作` …）も別名として使える（`ALIASES`。
//! 日本語の既定のテンプレートと、日本語で書いた現場のテンプレートのため）。名前の一覧と説明は `NAMES`（書き出す説明の
//! 文書にも使い、説明はいまの言語で出す）。

use super::template::{Scope, Target, Value};
use super::{Actor, Item, Report};

/// 日本語の名前 → 英語の名前（別名）。
pub const ALIASES: &[(&str, &str)] = &[
    ("プログラム", "program"),
    ("プログラム名", "programName"),
    ("版", "version"),
    ("更新日時", "modified"),
    ("開始", "started"),
    ("終了", "ended"),
    ("作成", "generated"),
    ("OK数", "okCount"),
    ("NG数", "ngCount"),
    ("項目数", "itemCount"),
    ("項目", "items"),
    ("連番", "index"),
    ("番号", "id"),
    ("項目名", "title"),
    ("結果", "result"),
    ("結果クラス", "resultClass"),
    ("状態クラス", "stateClass"),
    ("備考", "note"),
    ("終了時刻", "endedTime"),
    ("所要時間", "duration"),
    ("所要秒", "seconds"),
    ("exe名", "exeName"),
    ("exe版", "exeVersion"),
    ("exe更新日時", "exeModified"),
    ("操作数", "operationCount"),
    ("画面数", "pictureCount"),
    ("変化数", "changeCount"),
    ("SQL数", "sqlCount"),
    ("メッセージ数", "messageCount"),
    ("例外数", "exceptionCount"),
    ("操作あり", "hasOperations"),
    ("操作の画面あり", "hasOperationPictures"),
    ("操作", "operations"),
    ("変化", "changes"),
    ("変化の理由", "changeReason"),
    ("変化あり", "hasChanges"),
    ("変化一覧", "changeText"),
    ("SQL", "sql"),
    ("SQL全文", "sqlText"),
    ("メッセージ", "messages"),
    ("メッセージ一覧", "messageText"),
    ("例外", "exceptions"),
    ("例外一覧", "exceptionText"),
    ("写せなかった画面", "missedPictures"),
    ("画面", "pictures"),
    ("操作一覧", "operationText"),
    ("詳細あり", "hasDetails"),
    ("アンカー", "anchor"),
    ("前", "previous"),
    ("次", "next"),
    ("項目へのリンク", "itemLink"),
    ("一覧へのリンク", "listLink"),
    ("時刻", "time"),
    ("誰", "actor"),
    ("内容", "text"),
    ("誰クラス", "actorClass"),
    ("行クラス", "rowClass"),
    ("箇所画像", "spotPicture"),
    ("あと画像", "afterPicture"),
    ("文", "statement"),
    ("説明", "caption"),
    ("画像", "picture"),
];

/// 別名の、英語の名前（別名でなければそのまま）。
pub fn canonical(name: &str) -> &str {
    ALIASES.iter().find(|(alias, _)| *alias == name).map_or(name, |(_, english)| english)
}

/// 英語の名前の、日本語の別名（説明の文書に並べる）。
fn alias_of(english: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(_, e)| *e == english).map(|(alias, _)| *alias)
}

/// テンプレートで使える名前（`/` で並べた英語の名前）と、その説明（いまの言語）。見出しごと。
pub fn names() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        (
            tr_str!("Whole report"),
            vec![
                ("program", tr_str!("The path of the tested exe (of the last item)")),
                ("programName", tr_str!("The exe's file name")),
                ("version", tr_str!("The exe's file version")),
                ("modified", tr_str!("The exe's modified time")),
                ("started", tr_str!("When the first item started")),
                ("ended", tr_str!("When the last item ended")),
                ("generated", tr_str!("When the report was written")),
                ("okCount / ngCount / itemCount", tr_str!("The result counts")),
                ("items", tr_str!("List: the test items (the item names below can be used)")),
            ],
        ),
        (
            tr_str!("Item"),
            vec![
                ("index", tr_str!("From 1")),
                ("id / title / result / note", tr_str!("The number, what it checks, OK / NG / In progress, the note")),
                ("resultClass / stateClass", tr_str!("For HTML classes (ok / ng / open, passed / failed)")),
                ("started / ended / endedTime", tr_str!("Date and time (endedTime is the time only)")),
                ("duration / seconds", tr_str!("\"3.9 s\" and the number of seconds (a number cell in Excel)")),
                ("exe / exeName / exeVersion / exeModified", tr_str!("The exe tested in this item")),
                ("operationCount / pictureCount / changeCount / sqlCount / messageCount / exceptionCount", tr_str!("Counts")),
                ("operations", tr_str!("List: the actions (the action names below can be used)")),
                ("hasOperations / hasOperationPictures", tr_str!("Whether there are actions, and whether the actions have pictures")),
                ("changes / changeReason", tr_str!("List: the changed controls ({{text}}), and the reason when they could not be read")),
                ("hasChanges / changeText", tr_str!("Whether to show the changed controls, and them joined by line breaks")),
                ("sql / sqlText", tr_str!("List ({{statement}}) and the statements joined by blank lines")),
                ("messages / messageText", tr_str!("List ({{text}}) and them joined by line breaks")),
                ("exceptions / exceptionText", tr_str!("List ({{text}}) and them joined by line breaks")),
                ("missedPictures", tr_str!("List ({{text}}) of the pictures that could not be taken")),
                ("pictures", tr_str!("List: the start, capture and end pictures ({{caption}} and {{picture}})")),
                ("operationText", tr_str!("\"time [who] action\" joined by line breaks")),
                ("hasDetails", tr_str!("Whether there are changes, SQL, messages, exceptions or missed pictures")),
                ("anchor / previous / next", tr_str!("The HTML page mark (item-0), and the marks of the previous and next items (empty if none)")),
                ("itemLink", tr_str!("Shows the number and jumps to the item's page or sheet")),
                ("listLink", tr_str!("Shows \"← Back to the report\" and jumps to this item in the list")),
            ],
        ),
        (
            tr_str!("Action"),
            vec![
                ("index / time / actor / text", tr_str!("actor is AI / Hand / Step")),
                ("actorClass / rowClass", tr_str!("For HTML classes (who-ai / who-hand, note for step rows)")),
                ("spotPicture", tr_str!("The screen before the action, with the operated spot painted")),
                ("afterPicture", tr_str!("The screen after the action")),
            ],
        ),
        (tr_str!("Picture"), vec![("caption / picture", tr_str!("The caption and image of a start, capture or end picture"))]),
    ]
}

fn yes(value: bool) -> Value<'static> {
    Value::Bool(value)
}

fn texts<'a>(entries: impl IntoIterator<Item = String>, name: &'static str) -> Value<'a> {
    Value::List(
        entries
            .into_iter()
            .map(|entry| {
                let mut scope = Scope::default();
                scope.text(name, entry);
                scope
            })
            .collect(),
    )
}

/// 画像（無ければ空の文字。Excel では空のセル）。
fn image(picture: &Option<super::Picture>) -> Value<'_> {
    picture.as_ref().map_or(Value::Text(String::new()), Value::Image)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// 成績書の全体の値。`generated` は書いた時刻。
pub fn build<'a>(report: &'a Report, generated: &str) -> Scope<'a> {
    let program = report.items.iter().rev().find_map(|i| i.exe.as_ref());
    let (ok, ng) = report.counts();
    let mut root = Scope::default();
    root.text("program", program.map(|p| p.path.clone()).unwrap_or_default())
        .text("programName", program.map(|p| file_name(&p.path).to_string()).unwrap_or_default())
        .text("version", program.and_then(|p| p.version.clone()).unwrap_or_default())
        .text("modified", program.and_then(|p| p.modified.clone()).unwrap_or_default())
        .text("started", report.items.first().map(|i| i.started.clone()).unwrap_or_default())
        .text("ended", report.items.last().and_then(|i| i.ended.clone()).unwrap_or_default())
        .text("generated", generated)
        .set("okCount", Value::Number(ok as f64))
        .set("ngCount", Value::Number(ng as f64))
        .set("itemCount", Value::Number(report.items.len() as f64))
        .set("items", Value::List(report.items.iter().enumerate().map(|(index, item)| item_scope(index, report.items.len(), item)).collect()));
    root
}

fn item_scope(index: usize, count: usize, item: &Item) -> Scope<'_> {
    let exe = item.exe.as_ref();
    let changes: Vec<String> = item.changes.iter().map(|c| c.text()).collect();
    let change_text = if changes.is_empty() { item.change_problem.clone().unwrap_or_default() } else { changes.join("\n") };
    let operations: Vec<Scope> = item
        .operations
        .iter()
        .enumerate()
        .map(|(n, operation)| {
            let mut scope = Scope::default();
            scope
                .set("index", Value::Number((n + 1) as f64))
                .text("time", operation.time.clone())
                .text("actor", operation.actor.label())
                .text("text", operation.text.clone())
                .text("actorClass", match operation.actor {
                    Actor::Ai => "who-ai",
                    Actor::Hand => "who-hand",
                    Actor::Note => "",
                })
                .text("rowClass", if operation.actor == Actor::Note { "note" } else { "" })
                .set("spotPicture", image(&operation.marked))
                .set("afterPicture", image(&operation.picture));
            scope
        })
        .collect();
    let pictures: Vec<Scope> = item
        .pictures
        .iter()
        .map(|picture| {
            let mut scope = Scope::default();
            scope.text("caption", picture.caption.clone()).set("picture", Value::Image(picture));
            scope
        })
        .collect();
    let anchor = |i: usize| format!("item-{i}");
    let mut scope = Scope::default();
    scope
        .set("index", Value::Number((index + 1) as f64))
        .text("id", item.id.clone())
        .text("title", item.title.clone())
        .text("result", item.result.clone().unwrap_or_else(|| tr!("In progress")))
        .text("resultClass", match item.result.as_deref() {
            Some("OK") => "ok",
            Some("NG") => "ng",
            _ => "open",
        })
        .text("stateClass", match item.result.as_deref() {
            Some("OK") => "passed",
            Some("NG") => "failed",
            _ => "",
        })
        .text("note", item.note.clone())
        .text("started", item.started.clone())
        .text("ended", item.ended.clone().unwrap_or_default())
        .text("endedTime", item.ended.as_deref().map(|e| e.get(11..).unwrap_or(e).to_string()).unwrap_or_default())
        .text("duration", item.duration_text())
        .set("seconds", item.duration_ms.map_or(Value::Text(String::new()), |ms| Value::Number(ms as f64 / 1000.0)))
        .text("exe", exe.map(|e| e.path.clone()).unwrap_or_default())
        .text("exeName", exe.map(|e| file_name(&e.path).to_string()).unwrap_or_default())
        .text("exeVersion", exe.and_then(|e| e.version.clone()).unwrap_or_default())
        .text("exeModified", exe.and_then(|e| e.modified.clone()).unwrap_or_default())
        .set("operationCount", Value::Number(item.operations.len() as f64))
        .set("pictureCount", Value::Number(item.picture_count() as f64))
        .set("changeCount", Value::Number(item.changes.len() as f64))
        .set("sqlCount", Value::Number(item.sql.len() as f64))
        .set("messageCount", Value::Number(item.messages.len() as f64))
        .set("exceptionCount", Value::Number(item.exceptions.len() as f64))
        .set("hasOperations", yes(!item.operations.is_empty()))
        .set("hasOperationPictures", yes(item.operations.iter().any(|o| o.marked.is_some() || o.picture.is_some())))
        .set("operations", Value::List(operations))
        .set("changes", texts(changes, "text"))
        .text("changeReason", item.change_problem.clone().unwrap_or_default())
        .set("hasChanges", yes(!change_text.is_empty()))
        .text("changeText", change_text)
        .set("sql", texts(item.sql.clone(), "statement"))
        .text("sqlText", item.sql.join("\n\n"))
        .set("messages", texts(item.messages.clone(), "text"))
        .text("messageText", item.messages.join("\n"))
        .set("exceptions", texts(item.exceptions.clone(), "text"))
        .text("exceptionText", item.exceptions.join("\n"))
        .set("missedPictures", texts(item.picture_problems.clone(), "text"))
        .set("pictures", Value::List(pictures))
        .text("operationText", item.operations.iter().map(|o| format!("{} [{}] {}", o.time, o.actor.label(), o.text)).collect::<Vec<_>>().join("\n"))
        .set("hasDetails", yes(item.change_problem.is_some() || !item.changes.is_empty() || !item.sql.is_empty() || !item.messages.is_empty() || !item.exceptions.is_empty() || !item.picture_problems.is_empty()))
        .text("anchor", anchor(index))
        .text("previous", if index > 0 { anchor(index - 1) } else { String::new() })
        .text("next", if index + 1 < count { anchor(index + 1) } else { String::new() })
        .set("itemLink", Value::Link { text: item.id.clone(), target: Target::Item(index) })
        .set("listLink", Value::Link { text: tr!("← Back to the report"), target: Target::List(index) });
    scope
}

/// 使える名前の説明（テンプレートを書き出すときに、横に置く文書。いまの言語）。
pub fn names_text() -> String {
    let mut text = tr!("Names usable in report templates (netdbg; D-083)\n\nHow to write:\n  {{{{name}}}}            write a value (a picture is a thumbnail in HTML, and pasted into the cell in Excel)\n  {{{{name:uri}}}}        a picture's data URI (for the src of an HTML img)\n  {{{{#list}}}}…{{{{/list}}}}  repeat for each element of a list; if it is not a list, once when it has a value\n  {{{{^name}}}}…{{{{/name}}}}  only when there is no value\n\nExcel templates:\n  - A row with {{{{#list}}}} in one cell and {{{{/list}}}} in another cell of the same row is repeated per element (removed when there are none).\n  - A sheet whose name contains {{{{…}}}} (for example {{{{id}}}}) is copied per item, and the item names can be used in it.\n  - A cell that is exactly {{{{a picture name}}}} gets the picture pasted. A cell that is only a number such as {{{{seconds}}}} becomes a number cell.\n  - A cell that is only {{{{itemLink}}}} / {{{{listLink}}}} becomes a hyperlink between the sheets.\n  - Text too long for one cell continues on rows added just below.\n  - When rows are added, the rows below, merged cells, hyperlinks and drawings move down. References inside formulas do not move.\n\nHTML templates:\n  - Values are escaped. Scripts, outside CSS, pictures and URLs, and on… attributes cannot be written in a template (so that the report is one self-contained file).\n\nJapanese names (in parentheses) work as aliases.\n");
    for (scope, entries) in names() {
        text.push_str(&format!("\n[{scope}]\n"));
        for (names, description) in entries {
            let shown: Vec<String> = names.split(" / ").map(|n| alias_of(n).map_or_else(|| n.to_string(), |a| format!("{n} ({a})"))).collect();
            text.push_str(&format!("  {}: {description}\n", shown.join(" / ")));
        }
    }
    text
}
