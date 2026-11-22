//! 差し替わった exe の差分のツール（B2、D-080）。`assembly_diff`: 前後の exe のメソッドを比べ、再試験の範囲を出す。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "assembly_diff",
        "Which methods changed between two builds of the program (exe, dll or their folders; no symbols needed, so delivered \
         builds work), compared by their IL with names instead of metadata tokens, so unrelated additions do not show up. \
         Async bodies, lambdas and local functions count as the method that contains them. Kinds: changed, added, removed. \
         With coverage (see the coverage tool) of the new build, each changed or added method gets executed: true/false, and \
         retest: true when it changed but the test did not run it (the retest range). new defaults to the program being \
         debugged, and then the session's coverage is used; coverage can also be a Cobertura XML file. Compare builds of the \
         same configuration (Debug against Release shows nearly everything as changed).",
        json!({
            "old": { "type": "string", "description": "The previous build: an exe, a dll or a folder." },
            "new": { "type": "string", "description": "The new build (default: the program being debugged, or its folder when old is a folder)." },
            "coverage": { "type": "string", "description": "A Cobertura XML file of a test of the new build (default: this session's coverage)." },
            "retestOnly": { "type": "boolean", "description": "Return only the retest range." },
        }),
        &["old"],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    if name != "assembly_diff" {
        return Ok(None);
    }
    let mut request = json!({});
    for key in ["old", "new", "coverage"] {
        if let Some(value) = arguments.get(key) {
            request[key] = value.clone();
        }
    }
    let answer = ctx.request("netdbg.diffAssemblies", request)?;
    let retest_only = arguments.get("retestOnly").and_then(Value::as_bool) == Some(true);
    Ok(Some(Outcome::Done(answer.map(|mut body| {
        if retest_only {
            let kept: Vec<Value> = body["changes"].as_array().into_iter().flatten().filter(|c| c["retest"] == true).cloned().collect();
            body["changes"] = json!(kept);
        }
        body
    }))))
}
