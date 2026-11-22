//! カバレッジのツール（B1、D-079）。`coverage`: デバッグしているアプリの、通った行と分かれ道の向き。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "coverage",
        "Line and branch coverage of the program being debugged (its own code with symbols; the .NET libraries are not counted): \
         which lines ran and, for each branch (if, while, ?:, &&, ||, switch), which ways it went. Turn it on with enabled: true \
         (from then on; for the whole run, start the program with coverage); it is written as Cobertura XML (ReportGenerator, \
         GitHub, Azure Pipelines, GitLab read it) when the program ends, or now with write: true. file gives the state of each line \
         of one source file (hit, and per branch the ways seen). A line counts as run once (no hit counts). Branches whose ways \
         cannot be told apart are left out and counted in uncountableBranches.",
        json!({
            "enabled": { "type": "boolean", "description": "Start (true) or stop (false) counting. Omit to only read." },
            "output": { "type": "string", "description": "With enabled: the Cobertura XML file to write (default: under %LOCALAPPDATA%\\netdbg\\coverage)." },
            "folder": { "type": "string", "description": "The folder (absolute) to write <program>-<time>.cobertura.xml into, now and later (default %LOCALAPPDATA%\\netdbg\\coverage)." },
            "write": { "type": "boolean", "description": "Write the Cobertura XML now and return its path." },
            "file": { "type": "string", "description": "A source file: return the state of each of its lines." },
        }),
        &[],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    if name != "coverage" {
        return Ok(None);
    }
    if let Some(enabled) = arguments.get("enabled").and_then(Value::as_bool) {
        let mut request = json!({ "enabled": enabled });
        for key in ["output", "folder"] {
            if let Some(value) = arguments.get(key) {
                request[key] = value.clone();
            }
        }
        if let Err(e) = ctx.request("netdbg.setCoverage", request)? {
            return Ok(Some(Outcome::Done(Err(e))));
        }
    }
    let mut request = json!({});
    for key in ["folder", "write", "file"] {
        if let Some(value) = arguments.get(key) {
            request[key] = value.clone();
        }
    }
    Ok(Some(Outcome::Done(ctx.request("netdbg.coverage", request)?)))
}
