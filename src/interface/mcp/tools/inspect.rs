//! 止まったプログラムを調べるツール: スレッド、スタック、変数、式、代入、例外の情報、停止の文脈、ソース、モジュール。

use super::{frame_schema, frame_summary, thread_schema, tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

/// `variables` で、1 つの値を開いたときに返すメンバーの数の上限。
const MEMBERS_PER_VALUE: usize = 100;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    let (thread, frame) = (thread_schema(), frame_schema());
    vec![
        tool("threads", "The managed threads of the stopped program.", json!({}), &[]),
        tool(
            "stack_trace",
            "The call stack of a thread of the stopped program (function, file, line per frame).",
            json!({ "threadId": thread, "levels": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Frames to return (default 20)." } }),
            &[],
        ),
        tool(
            "variables",
            "The local variables and arguments of a frame, with values and types. Objects can be expanded to a depth.",
            json!({
                "threadId": thread,
                "frameIndex": frame,
                "variablesReference": { "type": "integer", "description": "Instead of a frame: expand this value (from an earlier variables or evaluate result)." },
                "depth": { "type": "integer", "minimum": 0, "maximum": 3, "description": "How many levels of members to expand (default 0)." },
            }),
            &[],
        ),
        tool(
            "evaluate",
            "Evaluate a C# expression in a frame of the stopped program (values and types as the C# compiler gives them; lambdas, LINQ and method calls run in the program).",
            json!({ "expression": { "type": "string" }, "threadId": thread, "frameIndex": frame }),
            &["expression"],
        ),
        tool("exception_info", "The exception the program stopped on: type, message, stack trace, inner exception.", json!({ "threadId": thread }), &[]),
        tool(
            "debug_stop_context",
            "Everything about the current stop in one call: where (function, file, line), the source lines around it, the local variables, and the exception if it stopped on one. Without frameIndex it uses the first frame with source (your code), also when the program stopped inside the runtime.",
            json!({
                "threadId": thread,
                "frameIndex": frame,
                "contextLines": { "type": "integer", "minimum": 0, "maximum": 50, "description": "Source lines before and after (default 5)." },
            }),
            &[],
        ),
        tool(
            "source_context",
            "Lines of a source file around a line, numbered, with the line marked.",
            json!({
                "file": { "type": "string" },
                "line": { "type": "integer", "minimum": 1 },
                "contextLines": { "type": "integer", "minimum": 0, "maximum": 200, "description": "Lines before and after (default 5)." },
            }),
            &["file", "line"],
        ),
        tool(
            "set_variable",
            "Assign a value to a variable, field, property or element in a frame of the stopped program. The value is a C# expression.",
            json!({ "target": { "type": "string", "description": "What to assign to: a name or an expression such as order.Total or items[0]." }, "value": { "type": "string" }, "threadId": thread, "frameIndex": frame }),
            &["target", "value"],
        ),
        tool("modules", "The assemblies the program has loaded, with whether their symbols (PDB) were found.", json!({}), &[]),
    ]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    let outcome = match name {
        "threads" => ctx.request("threads", Value::Null)?,
        "stack_trace" => stack_trace(ctx, arguments)?,
        "variables" => variables(ctx, arguments)?,
        "evaluate" => evaluate(ctx, arguments)?,
        "exception_info" => match ctx.stopped_thread(arguments) {
            Ok(thread) => ctx.request("exceptionInfo", json!({ "threadId": thread }))?,
            Err(e) => Err(e),
        },
        "debug_stop_context" => stop_context(ctx, arguments)?,
        "source_context" => source_context(arguments),
        "set_variable" => set_variable(ctx, arguments)?,
        "modules" => Ok(json!({ "modules": ctx.state.modules })),
        _ => return Ok(None),
    };
    Ok(Some(Outcome::Done(outcome)))
}

fn stack_trace(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let thread = match ctx.stopped_thread(arguments) {
        Ok(thread) => thread,
        Err(e) => return Ok(Err(e)),
    };
    let levels = arguments.get("levels").and_then(Value::as_u64).unwrap_or(20).clamp(1, 200);
    Ok(ctx.request("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": levels }))?.map(|trace| {
        let frames: Vec<Value> = trace.get("stackFrames").and_then(Value::as_array).into_iter().flatten().enumerate().map(|(index, frame)| {
            let mut summary = frame_summary(frame);
            summary["index"] = json!(index);
            summary
        }).collect();
        json!({ "threadId": thread, "frames": frames, "totalFrames": trace.get("totalFrames") })
    }))
}

fn variables(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let depth = arguments.get("depth").and_then(Value::as_u64).unwrap_or(0).min(3) as u32;
    let reference = match arguments.get("variablesReference").and_then(Value::as_i64) {
        Some(reference) => reference,
        None => {
            let frame = match ctx.frame_id(arguments)? {
                Ok(frame) => frame,
                Err(e) => return Ok(Err(e)),
            };
            match ctx.request("scopes", json!({ "frameId": frame }))? {
                Ok(scopes) => scopes.pointer("/scopes/0/variablesReference").and_then(Value::as_i64).unwrap_or(0),
                Err(e) => return Ok(Err(e)),
            }
        }
    };
    Ok(expand(ctx, reference, depth)?.map(|variables| json!({ "variables": variables })))
}

/// 値の中身を、`depth` の深さまで開く。
fn expand(ctx: &mut Context, reference: i64, depth: u32) -> io::Result<Result<Vec<Value>, String>> {
    let listed = match ctx.request("variables", json!({ "variablesReference": reference }))? {
        Ok(listed) => listed,
        Err(e) => return Ok(Err(e)),
    };
    let mut variables = Vec::new();
    let all = listed.get("variables").and_then(Value::as_array).cloned().unwrap_or_default();
    for variable in all.iter().take(MEMBERS_PER_VALUE) {
        let mut summary = json!({ "name": variable.get("name"), "value": variable.get("value"), "type": variable.get("type") });
        let child = variable.get("variablesReference").and_then(Value::as_i64).unwrap_or(0);
        if let Some(path) = variable.get("evaluateName") {
            summary["evaluateName"] = path.clone();
        }
        if child > 0 {
            summary["variablesReference"] = json!(child);
            if depth > 0 {
                summary["members"] = match expand(ctx, child, depth - 1)? {
                    Ok(members) => json!(members),
                    Err(e) => json!(e),
                };
            }
        }
        variables.push(summary);
    }
    // 上限で切ったことを、切った数と一緒に知らせる（AI が「全部見た」と思わないように）。
    if all.len() > MEMBERS_PER_VALUE {
        variables.push(json!({ "truncated": all.len() - MEMBERS_PER_VALUE }));
    }
    Ok(Ok(variables))
}

fn evaluate(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let Some(expression) = arguments.get("expression").and_then(Value::as_str) else {
        return Ok(Err(tr!("There is no expression.")));
    };
    let frame = match ctx.frame_id(arguments)? {
        Ok(frame) => frame,
        Err(e) => return Ok(Err(e)),
    };
    Ok(ctx.request("evaluate", json!({ "expression": expression, "frameId": frame, "context": "repl" }))?.map(|body| {
        json!({ "result": body.get("result"), "type": body.get("type"), "variablesReference": body.get("variablesReference") })
    }))
}

fn set_variable(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let (Some(target), Some(value)) = (arguments.get("target").and_then(Value::as_str), arguments.get("value").and_then(Value::as_str)) else {
        return Ok(Err(tr!("Specify target and value.")));
    };
    let frame = match ctx.frame_id(arguments)? {
        Ok(frame) => frame,
        Err(e) => return Ok(Err(e)),
    };
    Ok(ctx.request("setExpression", json!({ "expression": target, "value": value, "frameId": frame }))?.map(|body| json!({ "target": target, "value": body.get("value"), "type": body.get("type") })))
}

/// `debug_stop_context`: 止まった場所、前後のソース、ローカル変数、例外。
fn stop_context(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let thread = match ctx.stopped_thread(arguments) {
        Ok(thread) => thread,
        Err(e) => return Ok(Err(e)),
    };
    let mut context = ctx.status()?;
    // 指定が無ければ、ソースのある最初のフレーム（自分のコード）の文脈。
    let index = arguments.get("frameIndex").and_then(Value::as_u64).or_else(|| context.pointer("/userFrame/index").and_then(Value::as_u64)).unwrap_or(0);
    let frame = match ctx.request("stackTrace", json!({ "threadId": thread, "startFrame": index, "levels": 1 }))? {
        Ok(trace) => trace.pointer("/stackFrames/0").cloned().unwrap_or(Value::Null),
        Err(e) => return Ok(Err(e)),
    };
    // 上のフレームを指定したときだけ、そのフレームを別に示す（0 なら `at` と同じ）。
    if index > 0 {
        context["frame"] = frame_summary(&frame);
    }
    let context_lines = arguments.get("contextLines").and_then(Value::as_u64).unwrap_or(5).min(50) as usize;
    if let (Some(path), Some(line)) = (frame.pointer("/source/path").and_then(Value::as_str), frame.get("line").and_then(Value::as_u64)) {
        context["source"] = match read_source(path) {
            Ok(text) => json!(source_excerpt(&text, line as usize, context_lines)),
            Err(e) => json!(e),
        };
    }
    context["locals"] = match variables(ctx, &json!({ "threadId": thread, "frameIndex": index }))? {
        Ok(locals) => locals.get("variables").cloned().unwrap_or(Value::Null),
        Err(e) => json!(e),
    };
    if context.get("reason").and_then(Value::as_str) == Some("exception") {
        context["exception"] = match ctx.request("exceptionInfo", json!({ "threadId": thread }))? {
            Ok(info) => json!({ "type": info.get("exceptionId"), "description": info.get("description"), "message": info.pointer("/details/message"), "stackTrace": info.pointer("/details/stackTrace"), "inner": info.pointer("/details/innerException/0/message") }),
            Err(e) => json!(e),
        };
    }
    Ok(Ok(context))
}

fn source_context(arguments: &Value) -> Result<Value, String> {
    let (Some(file), Some(line)) = (arguments.get("file").and_then(Value::as_str), arguments.get("line").and_then(Value::as_u64)) else {
        return Err(tr!("Specify file and line."));
    };
    let context = arguments.get("contextLines").and_then(Value::as_u64).unwrap_or(5).min(200) as usize;
    read_source(file).map(|text| json!({ "file": file, "lines": source_excerpt(&text, line as usize, context) }))
}

/// ソースのファイルを読む（UTF-8。先頭の BOM は外す）。
fn read_source(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| tr!("Cannot read {path}: {error}", path = path, error = e))?;
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

/// `line`（1 から）の前後 `context` 行。止まった行には `current: true`。
fn source_excerpt(text: &str, line: usize, context: usize) -> Vec<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let first = line.saturating_sub(context).max(1);
    let last = (line + context).min(lines.len());
    (first..=last)
        .filter_map(|number| {
            let text = lines.get(number - 1)?;
            let mut entry = json!({ "line": number, "text": text });
            if number == line {
                entry["current"] = json!(true);
            }
            Some(entry)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_excerpt_marks_the_line_and_stays_inside_the_file() {
        let excerpt = source_excerpt("a\nb\nc\nd\n", 2, 5);
        assert_eq!(excerpt.len(), 4);
        assert_eq!(excerpt[1], json!({ "line": 2, "text": "b", "current": true }));
        assert_eq!(source_excerpt("a\nb\nc", 3, 0), vec![json!({ "line": 3, "text": "c", "current": true })]);
    }
}
