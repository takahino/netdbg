//! 対象の見張りのツール（段階 11）。`process_usage`: 対象のプロセスの今の使用量（メモリ、CPU、GDI / USER、ハンドル、
//! 32bit のアドレス空間）。`ui_freeze`: UI のフリーズで止める設定（既定で入っていて、2 秒答えない UI スレッドで止まる）。
//! `retention_path`: オブジェクトを保持している経路（GC のルートから、どの参照で生きているか。段階 12）。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![
        tool(
            "process_usage",
            "The program's process usage now: privateBytes, workingSet, cpuMs (CPU time since the start; compare two calls for \
             the rate), gdiObjects and userObjects against their per-process quotas (10,000 by default: a leak of window handles \
             or GDI objects ends at the quota), handles, and for a 32-bit process on 64-bit Windows its address space \
             (addressLimit, addressUsed, largestFree: an OutOfMemoryException of a 32-bit program usually means largestFree got \
             small, not that memory ran out). Works while running and while stopped.",
            json!({}),
            &[],
        ),
        tool(
            "ui_freeze",
            "The UI freeze watch: while the program runs (not while stepping), a UI thread that does not answer window messages for \
             thresholdMs (default 2000) pauses the program on that thread, with the reason \"UI not responding\" (the stack shows \
             what holds the UI up). It is on by default; enabled: false turns it off. Returns the setting now.",
            json!({
                "enabled": { "type": "boolean", "description": "Watch for UI freezes (true) or not (false). Omit to keep." },
                "thresholdMs": { "type": "integer", "minimum": 500, "description": "How long a UI thread may not answer before the program is paused." },
            }),
            &[],
        ),
        tool(
            "retention_path",
            "Why an object is still alive (a memory leak): the shortest chain of references from a GC root to the object of a C# \
             expression, evaluated in a frame of the stopped program. Each step gives the type and the field or element that \
             leads to the next (\".items\", \"[3]\"); the root is the stack (with the local variable when it is one), a strong \
             handle (static fields, GCHandle), a pinned handle or the finalizer queue. Not found means only weak references keep \
             it, or the next GC collects it. Follows at most maxDepth references (default 20).",
            json!({
                "expression": { "type": "string", "description": "A C# expression for the object, e.g. \"form.orders[0]\"." },
                "threadId": { "type": "integer", "description": "The thread (default: the stopped one)." },
                "frameIndex": { "type": "integer", "minimum": 0, "description": "The frame to evaluate in (0 = top, default)." },
                "maxDepth": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Longest chain to look for (default 20)." },
            }),
            &["expression"],
        ),
    ]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    let (command, request) = match name {
        "retention_path" => {
            let frame = match ctx.frame_id(arguments)? {
                Ok(frame) => frame,
                Err(e) => return Ok(Some(Outcome::Done(Err(e)))),
            };
            let mut request = json!({ "expression": arguments.get("expression").cloned().unwrap_or(Value::Null), "frameId": frame });
            if let Some(depth) = arguments.get("maxDepth") {
                request["maxDepth"] = depth.clone();
            }
            ("netdbg.retentionPath", request)
        }
        "process_usage" => ("netdbg.processUsage", json!({})),
        "ui_freeze" => ("netdbg.setUiFreeze", arguments.clone()),
        _ => return Ok(None),
    };
    Ok(Some(Outcome::Done(ctx.request(command, request)?)))
}
