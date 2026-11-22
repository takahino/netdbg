//! Edit and Continue のツール（`debug_apply_changes`。D-059）。AI がソースを直したあと、デバッグを続けたまま当てる。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "debug_apply_changes",
        "Apply the edits saved to the program's C# source files to the running program (Edit and Continue), without restarting. \
         Changed methods run the new code from their next call; a method that is stopped or running switches to the new code at its \
         next statement when the line it is on was not changed. Works for a Debug build started by the debugger (not attached). \
         Edits that cannot be applied (a changed signature, a removed member) and compile errors are reported with file and line; \
         restart the session then. Returns status applied / noChanges / rude / errors / failed.",
        json!({
            "files": { "type": "array", "items": { "type": "string" }, "description": "The changed source files (full paths). Omit to check every source file of the program." },
        }),
        &[],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    if name != "debug_apply_changes" {
        return Ok(None);
    }
    let mut request = json!({});
    if let Some(files) = arguments.get("files").filter(|f| f.is_array()) {
        request["files"] = files.clone();
    }
    Ok(Some(Outcome::Done(ctx.request("netdbg.applyChanges", request)?)))
}
