//! 止めずに読む記録のツール（段階 11、D-063）。`sql_log`: SQL ログを始め、プログラムが実行した SQL を読む。
//! `exception_log`: 投げられた例外を、止まらないものも含めて記録し、読む。`event_trace`: イベントのハンドラーが
//! 呼ばれた順と、どのコントロールから呼ばれたかを記録し、読む。
//! `sql_break`: 条件に合う SQL を、実行する直前で止める（段階 12）。`binding_log`: WinForms のデータバインディングが飲み込んだ
//! Format / Parse の失敗を読む（いつも記録している）。
//! 記録はエンジンの出来事の列にあり、対象を止めずに積まれる。読むのは、いつでもよい（走っていても、止まっていても）。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "sql_log",
        "The SQL statements the program ran through ADO.NET (Oracle ODP.NET, SqlClient, OleDb, Odbc), recorded at the provider \
         without stopping: the statement with its parameter values filled in (ready to paste into a SQL tool), the parameters, \
         the data source, the caller in the program, and how many times the same statement ran (repeated: 5 or more, a sign of \
         N+1 queries). Turn it on with enabled: true before the statements run (it stays on for the session, also across \
         restarts); methods replaces the provider methods hooked (Namespace.Type.Method). Read the statements after a number with \
         after (the last number is returned as last).",
        json!({
            "enabled": { "type": "boolean", "description": "Turn the SQL log on (true) or off (false). Omit to only read." },
            "methods": { "type": "array", "items": { "type": "string" }, "description": "With enabled: the provider methods to hook, replacing the defaults." },
            "transactionMethods": { "type": "array", "items": { "type": "string" }, "description": "With enabled: the BeginTransaction / Commit / Rollback methods to hook (see transactions), replacing the defaults." },
            "after": { "type": "integer", "minimum": 0, "description": "Only statements recorded after this number (default 0: all)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Most statements to return (default 50)." },
        }),
        &[],
    ), tool(
        "sql_break",
        "Stop the program right before it runs a SQL statement that matches, at the provider's entry (the statement has not run \
         yet; the stop's description names it). Each breakpoint gives text, parameter and/or value, and all given must match: \
         text is found in the statement or in the statement with its values filled in, ignoring case and whitespace; \
         parameter is a parameter name (with or without @ or :), value its value as sql_log shows it (strings without quotes, \
         null for DBNull). Replaces the earlier SQL breakpoints; an empty list clears them. Turns the SQL log on. Statements \
         that do not stop are recorded with their run time (durationMs in sql_log).",
        json!({
            "breakpoints": { "type": "array", "items": { "type": "object", "properties": {
                "text": { "type": "string", "description": "Part of the statement, e.g. \"delete from orders\"." },
                "parameter": { "type": "string", "description": "A parameter name, e.g. \"@id\"." },
                "value": { "type": "string", "description": "A parameter value, e.g. \"42\"." },
            } }, "description": "The SQL breakpoints (all of them, replacing the earlier ones)." },
        }),
        &["breakpoints"],
    ), tool(
        "binding_log",
        "The WinForms data binding failures the program swallowed while it ran: a value that could not be written to a control \
         (format: data source → control) or back to the data source (parse: control → data source, e.g. text that is not a \
         number; the field then keeps its old value and the user cannot leave it). For each: the control and its property, \
         the data source and member, the exception type and message, and the program's code that caused it (caller). Always \
         recorded. Read after a number with after (the last number is returned as last).",
        json!({
            "after": { "type": "integer", "minimum": 0, "description": "Only failures recorded after this number (default 0: all)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Most failures to return (default 50)." },
        }),
        &[],
    ), tool(
        "dialog_log",
        "The dialogs the program showed (MessageBox.Show, ShowDialog of WinForms and WPF), recorded at the call without stopping: \
         the text and caption, the form type, the program's code that showed it (caller) and the time. Always on (enabled: \
         false turns it off). breakOn [{text, caller}] stops the program before a dialog whose text, caption or form type \
         contains text and whose caller's name contains caller (replacing earlier ones; [] clears). Read after a number with \
         after (the last number is returned as last).",
        json!({
            "enabled": { "type": "boolean", "description": "Record dialogs (true, the default) or not (false). Omit to only read." },
            "breakOn": { "type": "array", "items": { "type": "object", "properties": {
                "text": { "type": "string", "description": "Part of the text, caption or form type." },
                "caller": { "type": "string", "description": "Part of the calling method's name." },
            } }, "description": "Stop before these dialogs (replacing the earlier conditions)." },
            "after": { "type": "integer", "minimum": 0, "description": "Only dialogs recorded after this number (default 0: all)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Most dialogs to return (default 50)." },
        }),
        &[],
    ), tool(
        "transactions",
        "Database transactions (needs the SQL log, sql_log): those still open, with how long they have been open, who began \
         them and the numbers of the SQL statements run in them (see sql_log), and the recent begin / commit / rollback \
         events with their duration and statements. A transaction left open (never committed) holds locks: look for one \
         open for long.",
        json!({
            "limit": { "type": "integer", "minimum": 1, "description": "Most recent events to return (default 50)." },
        }),
        &[],
    ), tool(
        "environment",
        "The environment of the program for a bug report, recorded when its runtime started: bitness, runtime version, Windows \
         user, machine, OS, culture and UI language, time zone, program, command line, working directory, and the connection \
         strings of its app.config / appsettings.json (passwords masked). Exceptions and dialogs in the logs refer to it by \
         environmentSeq; exceptions also carry context, the events of their thread since the last user action.",
        json!({}),
        &[],
    ), tool(
        "exception_log",
        "Every exception the program threw, also the ones that were caught and did not stop it: type, message, the place in \
         the program's code, whether it stopped, and a snapshot taken at the throw (the call stack and the locals of the first \
         frames of the program's code; the program is not stopped for it). Turn it on with enabled: true before reproducing; \
         snapshot: false records without the snapshot. Read after a number with after (the last number is returned as last).",
        json!({
            "enabled": { "type": "boolean", "description": "Turn the exception log on (true) or off (false). Omit to only read." },
            "snapshot": { "type": "boolean", "description": "With enabled: take the stack and locals at each throw (default true)." },
            "after": { "type": "integer", "minimum": 0, "description": "Only exceptions recorded after this number (default 0: all)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Most exceptions to return (default 50)." },
        }),
        &[],
    ), tool(
        "event_trace",
        "Which event handlers ran, in what order and from which control, without stopping: for each call the handler, the sender \
         (its Name, or its type) and the time. Give handlers (method names, Namespace.Type.Method, e.g. MainForm.saveButton_Click; \
         ui_tree and stack_trace show them) to start tracing them (added to those already traced; clear: true forgets them first). \
         Read after a number with after (the last number is returned as last).",
        json!({
            "handlers": { "type": "array", "items": { "type": "string" }, "description": "Handler methods to trace." },
            "clear": { "type": "boolean", "description": "Stop tracing every handler (before adding handlers)." },
            "after": { "type": "integer", "minimum": 0, "description": "Only calls recorded after this number (default 0: all)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Most calls to return (default 50)." },
        }),
        &[],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    if name == "environment" {
        let read = ctx.request("netdbg.journal", json!({ "kinds": ["environment"], "limit": 10_000 }))?;
        return Ok(Some(Outcome::Done(read.and_then(|body| body["records"].as_array().and_then(|r| r.last().cloned()).ok_or_else(|| "The environment is recorded when the program's runtime starts; start the program first.".to_string())))));
    }
    if name == "transactions" {
        let open = match ctx.request("netdbg.transactions", json!({}))? {
            Ok(body) => body["open"].clone(),
            Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
        };
        let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(50);
        let events = match ctx.request("netdbg.journal", json!({ "kinds": ["transaction"], "limit": 10_000 }))? {
            Ok(body) => {
                let all = body["records"].as_array().cloned().unwrap_or_default();
                let skip = all.len().saturating_sub(limit as usize);
                json!(all.into_iter().skip(skip).collect::<Vec<_>>())
            }
            Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
        };
        return Ok(Some(Outcome::Done(Ok(json!({ "open": open, "events": events })))));
    }
    if name == "sql_break" {
        let request = json!({ "breakpoints": arguments.get("breakpoints").cloned().unwrap_or_else(|| json!([])) });
        return Ok(Some(Outcome::Done(ctx.request("netdbg.setSqlBreakpoints", request)?)));
    }
    // ツールごとの、切り替えの要求と、答えの欄の名前、出来事の種類。
    let (setter, setting, items, kind, options): (&str, &str, &str, &str, &[&str]) = match name {
        "sql_log" => ("netdbg.setSqlLog", "sqlLog", "statements", "sql", &["methods", "transactionMethods"]),
        "exception_log" => ("netdbg.setExceptionLog", "exceptionLog", "exceptions", "exception", &["snapshot"]),
        "event_trace" => ("netdbg.setEventTrace", "eventTrace", "calls", "event", &[]),
        // 切り替えは無い（いつも記録している）。
        "binding_log" => ("", "", "failures", "binding", &[]),
        "dialog_log" => ("netdbg.setDialogLog", "dialogLog", "dialogs", "dialog", &["breakOn"]),
        _ => return Ok(None),
    };
    let mut answer = json!({});
    // イベント トレース: ハンドラーの名前を、拡張と同じ要求の形にして渡す。
    if name == "event_trace" && (arguments.get("handlers").is_some() || arguments.get("clear").is_some()) {
        let handlers: Vec<Value> = arguments.get("handlers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|h| json!({ "breakpointName": h })).collect();
        let request = json!({ "handlers": handlers, "clear": arguments.get("clear").and_then(Value::as_bool).unwrap_or(false) });
        match ctx.request(setter, request)? {
            Ok(body) => answer[setting] = body,
            Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
        }
    }
    // ダイアログの止める条件は、enabled なしでも置ける（記録は既定で入っている）。
    let enabled = arguments.get("enabled").and_then(Value::as_bool).or_else(|| (name == "dialog_log" && arguments.get("breakOn").is_some()).then_some(true));
    if let Some(enabled) = enabled.filter(|_| !setter.is_empty()) {
        let mut request = json!({ "enabled": enabled });
        for option in options {
            if let Some(value) = arguments.get(*option) {
                request[*option] = value.clone();
            }
        }
        match ctx.request(setter, request)? {
            Ok(body) => answer[setting] = body,
            Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
        }
    }
    let after = arguments.get("after").and_then(Value::as_u64).unwrap_or(0);
    let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(50);
    let read = ctx.request("netdbg.journal", json!({ "after": after, "kinds": [kind], "limit": limit }))?;
    let body = match read {
        Ok(body) => body,
        Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
    };
    // AI には、呼び出し元を 1 つだけ渡す（全部のフレームは長い）。
    let entries: Vec<Value> = body["records"]
        .as_array()
        .map(|records| {
            records
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    if let Value::Object(fields) = &mut r {
                        fields.remove("frames");
                        fields.remove("kind");
                    }
                    r
                })
                .collect()
        })
        .unwrap_or_default();
    answer[items] = json!(entries);
    answer["last"] = body["last"].clone();
    if body["dropped"].as_u64().unwrap_or(0) > 0 {
        answer["dropped"] = body["dropped"].clone();
    }
    Ok(Some(Outcome::Done(Ok(answer))))
}