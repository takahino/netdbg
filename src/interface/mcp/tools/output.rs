//! 対象の出力のツール（`debug_output`）。行は、本体の出来事から `State` が覚えている。

use super::{tool, Context, Outcome};
use crate::interface::mcp::state::State;
use serde_json::{json, Value};

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "debug_output",
        "The program's output (stdout / stderr) and the debugger's notes. Pass the cursor of the previous call to get only the new lines; search keeps only the lines that contain the text.",
        json!({
            "lines": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Lines to return, the latest ones (default 50)." },
            "sinceCursor": { "type": "integer", "minimum": 0, "description": "Return the lines written after this cursor." },
            "search": { "type": "string", "description": "Keep only the lines containing this text (case-insensitive)." },
        }),
        &[],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> Option<Outcome> {
    (name == "debug_output").then(|| Outcome::Done(Ok(output_lines(ctx.state, arguments))))
}

/// 最後の N 行、カーソルより後の行、文字列を含む行。
fn output_lines(state: &State, arguments: &Value) -> Value {
    let limit = arguments.get("lines").and_then(Value::as_u64).unwrap_or(50).clamp(1, 1000) as usize;
    let first = state.output_total - state.output.len() as u64;
    let since = arguments.get("sinceCursor").and_then(Value::as_u64);
    let search = arguments.get("search").and_then(Value::as_str).map(str::to_lowercase);
    let lines: Vec<&String> = state
        .output
        .iter()
        .enumerate()
        .filter(|(index, _)| since.is_none_or(|since| first + *index as u64 >= since))
        .map(|(_, line)| line)
        .filter(|line| search.as_ref().is_none_or(|search| line.to_lowercase().contains(search)))
        .collect();
    let truncated = lines.len() > limit || since.is_some_and(|since| since < first);
    let shown: Vec<&String> = lines[lines.len().saturating_sub(limit)..].to_vec();
    json!({ "output": shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"), "cursor": state.output_total, "truncated": truncated })
}
