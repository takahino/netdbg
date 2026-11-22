//! 逆コンパイルのツール（`decompile`。D-060）。PDB の無いモジュールを、エンジンが C# に戻したソースで読む。
//! ソースの無いフレームは、止まったときにエンジンが自分で逆コンパイルする。このツールは、まだ止まっていない所や、
//! 呼び出し先の中身を、ブレークポイントを置く前に読むためのもの。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![tool(
        "decompile",
        "Read the C# source the debugger decompiled from an assembly that has no PDB (a program shipped without symbols). \
         Without arguments, lists the loaded assemblies that are decompiled on demand. With module, decompiles it (if not yet) and \
         lists the source files. With module and type, returns the numbered lines of that type's file; add method for only that \
         member (all overloads). The files are real paths: breakpoint_set and source_context work on them, and frames in those \
         assemblies point at them. Needs a debug session with the assembly loaded.",
        json!({
            "module": { "type": "string", "description": "Assembly file name (MyApp.dll) or full path." },
            "type": { "type": "string", "description": "Type name: Namespace.Type, Type, or Outer.Inner / Outer+Inner for a nested type." },
            "method": { "type": "string", "description": "Member name in the type (method, constructor = type name, property, event)." },
            "maxLines": { "type": "integer", "minimum": 1, "description": "Most lines to return (default 400)." },
        }),
        &[],
    )]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    if name != "decompile" {
        return Ok(None);
    }
    if arguments.get("method").is_some() && arguments.get("type").is_none() {
        return Ok(Some(Outcome::Done(Err(tr!("Specify type as well as method.")))));
    }
    let mut request = json!({});
    for key in ["module", "type", "method", "maxLines"] {
        if let Some(value) = arguments.get(key) {
            request[key] = value.clone();
        }
    }
    Ok(Some(Outcome::Done(ctx.request("netdbg.decompile", request)?)))
}