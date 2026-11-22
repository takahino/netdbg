//! ブレークポイントのツール: 行と関数のブレークポイント、トレースポイント（止めずに記録する）、例外で止まる設定、
//! フィールドへの書き込みで止める（D-093）。
//!
//! AI が置いたものは AI の分（`Owner::Agent`）として本体に渡すので、IDE のブレークポイントを置き換えない。本体の要求は
//! ファイルごと・関数全体で置き換える形なので、一覧をここで持ち、変わるたびに送り直す。

use super::{frame_schema, frame_summary, thread_schema, tool, Context, Outcome};
use crate::debugger::session::Event;
use crate::interface::mcp::server::Mode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io;
use std::time::Instant;

/// 覚えておくトレースポイントの記録の数。
const TRACE_ENTRIES: usize = 2000;

pub(in crate::interface::mcp) fn tools(standalone: bool) -> Vec<Value> {
    let mut tools = vec![
        tool(
            "breakpoint_set",
            "Set a breakpoint on a source line, or on a method by name. Breakpoints set here are the agent's own: they do not replace the ones of the IDE.",
            json!({
                "file": { "type": "string", "description": "Full path of the C# / VB / F# source file." },
                "line": { "type": "integer", "minimum": 1 },
                "function": { "type": "string", "description": "Instead of file and line: Namespace.Type.Method, Type.Method or Method (all overloads)." },
                "condition": { "type": "string", "description": "C# expression; the program stops only when it is true." },
                "logMessage": { "type": "string", "description": "Make it a tracepoint: do not stop, write this message to trace_log instead. {expression} parts are evaluated as C#, e.g. \"total={total}, count={items.Count}\"." },
            }),
            &[],
        ),
        tool(
            "breakpoint_remove",
            "Remove a breakpoint set with breakpoint_set (by file and line, or by function).",
            json!({ "file": { "type": "string" }, "line": { "type": "integer" }, "function": { "type": "string" } }),
            &[],
        ),
        tool("breakpoint_list", "The breakpoints set with breakpoint_set, with whether each is bound to code yet.", json!({}), &[]),
        tool("breakpoint_clear", "Remove every breakpoint and tracepoint set with breakpoint_set.", json!({}), &[]),
        tool(
            "trace_log",
            "Messages written by tracepoints (breakpoint_set with logMessage), oldest first. Pass the cursor of the previous call to get only the new ones.",
            json!({ "sinceCursor": { "type": "integer", "minimum": 0 }, "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Entries to return (default 100)." } }),
            &[],
        ),
    ];
    if standalone {
        tools.push(tool(
            "exception_breakpoints",
            "Which exceptions stop the program: none (only unhandled ones, always), userUnhandled (also those your code does not handle), all (every throw). \
             With all, types limits it to these types (\"System.InvalidOperationException\", or \"!\" first to exclude).",
            json!({
                "mode": { "type": "string", "enum": ["none", "userUnhandled", "all"] },
                "types": { "type": "array", "items": { "type": "string" } },
            }),
            &["mode"],
        ));
        tools.push(tool(
            "break_on_write",
            "Stop when a field of an object is written, by any code (another method through ref or out, code without source, after garbage collections). \
             .NET only, one field at a time. Give the field as object.Field in a frame of the stopped program, or clear to stop watching. \
             On 32-bit (x86), a write from native code (reflection's first SetValue, P/Invoke writing back) ends the program.",
            json!({
                "expression": { "type": "string", "description": "The field, e.g. order.Status." },
                "clear": { "type": "boolean", "description": "Stop watching." },
                "threadId": thread_schema(),
                "frameIndex": frame_schema(),
            }),
            &[],
        ));
    }
    tools
}

/// AI が置いた行のブレークポイント。
#[derive(Clone)]
struct LineBreakpoint {
    line: u32,
    condition: Option<String>,
    /// トレースポイントの文言（止まらずに `trace_log` に書く）。`{式}` は C# として評価する。
    log: Option<String>,
    id: Option<i64>,
}

/// AI が置いた関数のブレークポイント。
struct FunctionBreakpoint {
    name: String,
    condition: Option<String>,
    id: Option<i64>,
}

/// AI のブレークポイントとトレースポイント、例外の設定。
pub(in crate::interface::mcp) struct Breakpoints {
    lines: BTreeMap<String, Vec<LineBreakpoint>>,
    functions: Vec<FunctionBreakpoint>,
    /// `exception_breakpoints` の設定（本体の setExceptionBreakpoints の引数）。起動のたびに送り直す。
    exceptions: Option<Value>,
    /// ブレークポイントの id ごとの、コードに結び付いたか。
    verified: HashMap<i64, bool>,
    /// トレースポイントの記録と、これまでの数（`trace_log` のカーソル）。
    trace: VecDeque<Value>,
    trace_total: u64,
    /// トレースポイントで止まった `stopped` の本文。記録して続行する（`run_tracepoints`）。
    pending_traces: Vec<Value>,
    started: Instant,
}

impl Breakpoints {
    pub fn new() -> Breakpoints {
        Breakpoints { lines: BTreeMap::new(), functions: Vec::new(), exceptions: None, verified: HashMap::new(), trace: VecDeque::new(), trace_total: 0, pending_traces: Vec::new(), started: Instant::now() }
    }

    /// 新しいセッション。置いたものは残し（起動のときに置き直す）、結び付きだけ忘れる。
    pub fn restart(&mut self) {
        self.verified.clear();
    }

    /// 本体の出来事を見る: 結び付いたか、トレースポイントだけに当たって止まったか。
    pub fn observe(&mut self, events: &[Event]) {
        for (name, body) in events {
            match *name {
                "stopped" => {
                    // AI のトレースポイントだけに当たったなら、記録して続行する（待っている呼び出しには答えない）。
                    let hits: Vec<i64> = body.get("hitBreakpointIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_i64).collect();
                    let traces: Vec<i64> = self.lines.values().flatten().filter(|b| b.log.is_some()).filter_map(|b| b.id).collect();
                    if !hits.is_empty() && hits.iter().all(|id| traces.contains(id)) {
                        self.pending_traces.push(body.clone());
                    }
                }
                "breakpoint" => {
                    if let (Some(id), Some(verified)) = (body.pointer("/breakpoint/id").and_then(Value::as_i64), body.pointer("/breakpoint/verified").and_then(Value::as_bool)) {
                        self.verified.insert(id, verified);
                    }
                }
                _ => {}
            }
        }
    }

    fn list(&self) -> Value {
        let verified = |id: Option<i64>| id.and_then(|id| self.verified.get(&id).copied()).unwrap_or(false);
        let lines: Vec<Value> = self
            .lines
            .iter()
            .flat_map(|(file, list)| list.iter().map(move |b| (file, b)))
            .map(|(file, b)| json!({ "file": file, "line": b.line, "condition": b.condition, "logMessage": b.log, "verified": verified(b.id) }))
            .collect();
        let functions: Vec<Value> = self.functions.iter().map(|f| json!({ "function": f.name, "condition": f.condition, "verified": verified(f.id) })).collect();
        json!({ "breakpoints": lines, "functions": functions })
    }

    fn trace_entries(&self, arguments: &Value) -> Value {
        let since = arguments.get("sinceCursor").and_then(Value::as_u64).unwrap_or(0);
        let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 1000) as usize;
        let newer: Vec<&Value> = self.trace.iter().filter(|e| e.get("cursor").and_then(Value::as_u64).unwrap_or(0) > since).collect();
        let truncated = newer.len() > limit;
        let entries: Vec<&Value> = newer.into_iter().take(limit).collect();
        let cursor = entries.last().and_then(|e| e.get("cursor").and_then(Value::as_u64)).unwrap_or(since);
        json!({ "entries": entries, "cursor": cursor, "truncated": truncated })
    }
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    let outcome = match name {
        "breakpoint_set" => set(ctx, arguments)?,
        "breakpoint_remove" => remove(ctx, arguments)?,
        "breakpoint_list" => Ok(ctx.breakpoints.list()),
        "breakpoint_clear" => clear(ctx)?,
        "trace_log" => Ok(ctx.breakpoints.trace_entries(arguments)),
        "exception_breakpoints" if ctx.mode != Mode::Standalone => Err(tr!("Choose which exceptions to stop on in the IDE (VS Code, for example).")),
        "exception_breakpoints" => exception_breakpoints(ctx, arguments)?,
        "break_on_write" if ctx.mode != Mode::Standalone => Err(tr!("Choose a field to break on writes in the IDE (in VS Code, Break on Value Change in the Variables view).")),
        "break_on_write" => break_on_write(ctx, arguments)?,
        _ => return Ok(None),
    };
    Ok(Some(Outcome::Done(outcome)))
}

/// `break_on_write`: フィールドを選んで、書き込みの見張りを置く（`clear` で外す）。
fn break_on_write(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    if arguments.get("clear").and_then(Value::as_bool) == Some(true) {
        return Ok(ctx.request("setDataBreakpoints", json!({ "breakpoints": [] }))?.map(|_| json!({ "watching": Value::Null })));
    }
    let Some(expression) = arguments.get("expression").and_then(Value::as_str).filter(|e| !e.trim().is_empty()) else {
        return Ok(Err(tr!("There is no expression.")));
    };
    let frame = match ctx.frame_id(arguments)? {
        Ok(frame) => frame,
        Err(e) => return Ok(Err(e)),
    };
    let info = match ctx.request("dataBreakpointInfo", json!({ "expression": expression, "frameId": frame }))? {
        Ok(info) => info,
        Err(e) => return Ok(Err(e)),
    };
    let Some(id) = info.get("dataId").and_then(Value::as_str) else {
        return Ok(Err(info.get("description").and_then(Value::as_str).unwrap_or_default().to_string()));
    };
    let set = match ctx.request("setDataBreakpoints", json!({ "breakpoints": [{ "dataId": id, "accessType": "write" }] }))? {
        Ok(set) => set,
        Err(e) => return Ok(Err(e)),
    };
    let answer = set.pointer("/breakpoints/0").cloned().unwrap_or(Value::Null);
    if answer.get("verified").and_then(Value::as_bool) != Some(true) {
        return Ok(Err(answer.get("message").and_then(Value::as_str).unwrap_or_default().to_string()));
    }
    Ok(Ok(json!({ "watching": expression, "note": info.get("description") })))
}
/// 起動のとき: 先に置かれたブレークポイントと例外の設定を、新しいセッションに置き直す。置けなかったブレークポイントは、
/// `breakpoint_list` で verified が false になる。失敗を返すのは、例外の設定を送れなかったときだけ。
pub(in crate::interface::mcp) fn apply_all(ctx: &mut Context) -> io::Result<Result<(), String>> {
    for file in ctx.breakpoints.lines.keys().cloned().collect::<Vec<_>>() {
        if let Err(e) = send_lines(ctx, &file)? {
            crate::log!("mcp: breakpoints of {file}: {e}");
        }
    }
    if !ctx.breakpoints.functions.is_empty() {
        if let Err(e) = send_functions(ctx)? {
            crate::log!("mcp: function breakpoints: {e}");
        }
    }
    if let Some(exceptions) = ctx.breakpoints.exceptions.clone() {
        if let Err(e) = ctx.request("setExceptionBreakpoints", exceptions)? {
            return Ok(Err(e));
        }
    }
    Ok(Ok(()))
}

fn set(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let condition = arguments.get("condition").and_then(Value::as_str).filter(|c| !c.trim().is_empty()).map(str::to_string);
    let log = arguments.get("logMessage").and_then(Value::as_str).filter(|m| !m.is_empty()).map(str::to_string);
    if let Some(function) = arguments.get("function").and_then(Value::as_str) {
        if log.is_some() {
            return Ok(Err(tr!("Put tracepoints (logMessage) on a file and line.")));
        }
        let functions = &mut ctx.breakpoints.functions;
        functions.retain(|f| f.name != function);
        functions.push(FunctionBreakpoint { name: function.to_string(), condition, id: None });
        return send_functions(ctx);
    }
    let (Some(file), Some(line)) = (arguments.get("file").and_then(Value::as_str), arguments.get("line").and_then(Value::as_u64)) else {
        return Ok(Err(tr!("Specify file and line (or function).")));
    };
    let list = ctx.breakpoints.lines.entry(file.to_string()).or_default();
    list.retain(|b| b.line != line as u32);
    list.push(LineBreakpoint { line: line as u32, condition, log, id: None });
    send_lines(ctx, file)
}

fn remove(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    if let Some(function) = arguments.get("function").and_then(Value::as_str) {
        let functions = &mut ctx.breakpoints.functions;
        let before = functions.len();
        functions.retain(|f| f.name != function);
        if before == functions.len() {
            return Ok(Err(tr!("There is no breakpoint on the function {function}.", function = function)));
        }
        return send_functions(ctx);
    }
    let (Some(file), Some(line)) = (arguments.get("file").and_then(Value::as_str), arguments.get("line").and_then(Value::as_u64)) else {
        return Ok(Err(tr!("Specify file and line (or function).")));
    };
    let Some(list) = ctx.breakpoints.lines.get_mut(file) else { return Ok(Err(tr!("There are no breakpoints in {file}.", file = file))) };
    let before = list.len();
    list.retain(|b| b.line != line as u32);
    if before == list.len() {
        return Ok(Err(tr!("There is no breakpoint on line {line} of {file}.", line = line, file = file)));
    }
    send_lines(ctx, file)
}

fn clear(ctx: &mut Context) -> io::Result<Result<Value, String>> {
    let files: Vec<String> = std::mem::take(&mut ctx.breakpoints.lines).into_keys().collect();
    for file in &files {
        if let Err(e) = send_lines(ctx, file)? {
            return Ok(Err(e));
        }
    }
    if !std::mem::take(&mut ctx.breakpoints.functions).is_empty() {
        if let Err(e) = send_functions(ctx)? {
            return Ok(Err(e));
        }
    }
    Ok(Ok(json!({ "cleared": true })))
}

/// ファイルの、AI のブレークポイントを全部送り直す（本体の setBreakpoints は置き換え）。
fn send_lines(ctx: &mut Context, file: &str) -> io::Result<Result<Value, String>> {
    let items: Vec<Value> = ctx.breakpoints.lines.get(file).into_iter().flatten().map(|b| json!({ "line": b.line, "condition": b.condition })).collect();
    let body = match ctx.request("setBreakpoints", json!({ "source": { "path": file }, "breakpoints": items }))? {
        Ok(body) => body,
        Err(e) => return Ok(Err(e)),
    };
    let answered: Vec<Value> = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
    let breakpoints = &mut *ctx.breakpoints;
    if let Some(list) = breakpoints.lines.get_mut(file) {
        for (breakpoint, answer) in list.iter_mut().zip(&answered) {
            breakpoint.id = answer.get("id").and_then(Value::as_i64);
            if let Some(id) = breakpoint.id {
                breakpoints.verified.insert(id, answer.get("verified").and_then(Value::as_bool) == Some(true));
            }
        }
    }
    Ok(Ok(json!({ "file": file, "breakpoints": answered })))
}

fn send_functions(ctx: &mut Context) -> io::Result<Result<Value, String>> {
    let items: Vec<Value> = ctx.breakpoints.functions.iter().map(|f| json!({ "name": f.name, "condition": f.condition })).collect();
    let body = match ctx.request("setFunctionBreakpoints", json!({ "breakpoints": items }))? {
        Ok(body) => body,
        Err(e) => return Ok(Err(e)),
    };
    let answered: Vec<Value> = body.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
    let breakpoints = &mut *ctx.breakpoints;
    for (function, answer) in breakpoints.functions.iter_mut().zip(&answered) {
        function.id = answer.get("id").and_then(Value::as_i64);
        if let Some(id) = function.id {
            breakpoints.verified.insert(id, answer.get("verified").and_then(Value::as_bool) == Some(true));
        }
    }
    Ok(Ok(json!({ "functions": answered })))
}

fn exception_breakpoints(ctx: &mut Context, arguments: &Value) -> io::Result<Result<Value, String>> {
    let types: Vec<String> = arguments.get("types").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    let (filters, options) = match arguments.get("mode").and_then(Value::as_str) {
        Some("none") => (json!([]), json!([])),
        Some("userUnhandled") => (json!(["user-unhandled"]), json!([])),
        Some("all") if types.is_empty() => (json!(["all"]), json!([])),
        // 型は `A, B`（その型だけ）か `!A, B`（その型以外）。拡張の例外設定と同じ書き方。
        Some("all") => {
            let condition = if types.iter().all(|t| t.starts_with('!')) {
                format!("!{}", types.iter().map(|t| t.trim_start_matches('!')).collect::<Vec<_>>().join(", "))
            } else {
                types.join(", ")
            };
            (json!([]), json!([{ "filterId": "all", "condition": condition }]))
        }
        _ => return Ok(Err(tr!("mode must be none, userUnhandled or all."))),
    };
    // 覚えておき、次の起動にも使う（終わったあとの起動は、新しいセッションになる）。
    let request = json!({ "filters": filters, "filterOptions": options });
    ctx.breakpoints.exceptions = Some(request.clone());
    Ok(ctx.request("setExceptionBreakpoints", request)?.map(|_| json!({ "mode": arguments.get("mode"), "types": types })))
}

/// トレースポイントで止まったものを記録し、続行する（入力を 1 つ処理するたびに呼ぶ）。
pub(in crate::interface::mcp) fn run_tracepoints(ctx: &mut Context) -> io::Result<()> {
    for body in std::mem::take(&mut ctx.breakpoints.pending_traces) {
        let thread = body.get("threadId").and_then(Value::as_i64).unwrap_or(0);
        let hit = body.pointer("/hitBreakpointIds/0").and_then(Value::as_i64);
        let template = ctx.breakpoints.lines.values().flatten().find(|b| b.id.is_some() && b.id == hit).and_then(|b| b.log.clone()).unwrap_or_default();
        let (frame, at) = match ctx.request("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": 1 }))? {
            Ok(trace) => (trace.pointer("/stackFrames/0/id").and_then(Value::as_i64), trace.pointer("/stackFrames/0").map(frame_summary).unwrap_or(Value::Null)),
            Err(_) => (None, Value::Null),
        };
        let mut message = String::new();
        for part in template_parts(&template) {
            match part {
                TemplatePart::Text(text) => message.push_str(&text),
                TemplatePart::Expression(expression) => match ctx.request("evaluate", json!({ "expression": expression, "frameId": frame, "context": "watch" }))? {
                    Ok(value) => message.push_str(value.get("result").and_then(Value::as_str).unwrap_or("")),
                    Err(e) => message.push_str(&format!("<{e}>")),
                },
            }
        }
        let breakpoints = &mut *ctx.breakpoints;
        if breakpoints.trace.len() == TRACE_ENTRIES {
            breakpoints.trace.pop_front();
        }
        breakpoints.trace_total += 1;
        let entry = json!({ "cursor": breakpoints.trace_total, "ms": breakpoints.started.elapsed().as_millis() as u64, "threadId": thread, "at": at, "message": message });
        breakpoints.trace.push_back(entry);
        if let Err(e) = ctx.request("continue", json!({ "threadId": thread }))? {
            crate::log!("mcp: tracepoint continue: {e}");
        }
    }
    Ok(())
}

/// トレースポイントの文言の部品。
#[derive(Debug, PartialEq)]
enum TemplatePart {
    Text(String),
    Expression(String),
}

/// `x={x}, n={items.Count}` を、文字と式に分ける。`{{` と `}}` は波かっこそのもの。式の中の入れ子の `{ }` は数えて、
/// いちばん外の `}` で式を閉じる。
fn template_parts(template: &str) -> Vec<TemplatePart> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut expression = String::new();
                let mut depth = 0;
                for c in chars.by_ref() {
                    match c {
                        '{' => depth += 1,
                        '}' if depth == 0 => break,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    expression.push(c);
                }
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                parts.push(TemplatePart::Expression(expression.trim().to_string()));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_is_split_into_text_and_expressions() {
        assert_eq!(
            template_parts("x={x}, n={items.Count}!"),
            vec![TemplatePart::Text("x=".into()), TemplatePart::Expression("x".into()), TemplatePart::Text(", n=".into()), TemplatePart::Expression("items.Count".into()), TemplatePart::Text("!".into())]
        );
    }

    #[test]
    fn doubled_braces_are_braces_and_nested_braces_stay_in_the_expression() {
        assert_eq!(template_parts("{{a}} {new { A = 1 }.A}"), vec![TemplatePart::Text("{a} ".into()), TemplatePart::Expression("new { A = 1 }.A".into())]);
        assert_eq!(template_parts("plain"), vec![TemplatePart::Text("plain".into())]);
    }

    #[test]
    fn a_stop_only_at_tracepoints_is_recorded_and_not_reported() {
        let mut breakpoints = Breakpoints::new();
        breakpoints.lines.insert("a.cs".into(), vec![LineBreakpoint { line: 3, condition: None, log: Some("x={x}".into()), id: Some(1) }, LineBreakpoint { line: 5, condition: None, log: None, id: Some(2) }]);
        breakpoints.observe(&[("stopped", json!({ "hitBreakpointIds": [1] }))]);
        assert_eq!(breakpoints.pending_traces.len(), 1);
        // ふつうのブレークポイントにも当たったなら、止まったまま（AI に知らせる）。
        breakpoints.observe(&[("stopped", json!({ "hitBreakpointIds": [1, 2] }))]);
        assert_eq!(breakpoints.pending_traces.len(), 1);
    }
}
