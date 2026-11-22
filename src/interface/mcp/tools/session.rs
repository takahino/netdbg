//! セッションのツール: 起動、アタッチ（`--mcp` だけ）、状態、続行、一時停止、ステップ、止まるのを待つ、終える。

use super::{breakpoints, tool, wait_ms, Context, Outcome, DEFAULT_WAIT_MS};
use crate::interface::mcp::server::Mode;
use crate::interface::mcp::state::Target;
use serde_json::{json, Value};
use std::io;
use std::time::Duration;

pub(in crate::interface::mcp) fn tools(standalone: bool) -> Vec<Value> {
    let mut tools = Vec::new();
    if standalone {
        tools.push(tool(
            "debug_launch",
            "Start a .NET or .NET Framework program under the debugger. Breakpoints set before are applied before it runs. \
             The engine debugs programs of its own bitness (x86 or x64); a mismatch is refused with the reason.",
            json!({
                "program": { "type": "string", "description": "Full path of the .exe (a .NET apphost or a .NET Framework exe), or of a framework-dependent .dll." },
                "args": { "type": "array", "items": { "type": "string" } },
                "cwd": { "type": "string" },
                "env": { "type": "object", "additionalProperties": { "type": "string" } },
                "stopAtEntry": { "type": "boolean", "description": "Stop at the first line of Main." },
                "coverage": { "type": "boolean", "description": "Count line and branch coverage for the whole run (see coverage)." },
                "buildProject": { "type": "string", "description": "Build this project (.csproj / .vbproj / .fsproj, or its folder) with dotnet build -c Debug first; the launch is refused with the build errors if it fails." },
                "waitMs": wait_ms("the first stop"),
            }),
            &["program"],
        ));
        tools.push(tool(
            "debug_attach",
            "Attach to a running .NET or .NET Framework process of the engine's bitness. observeOnly attaches only to observe (for a shared test environment): changing values, Edit and Continue, moving the next statement, running the program's functions (property getters, ToString, calls) and input to its windows are refused; fields, stacks, records (SQL, exceptions) and screenshots work.",
            json!({ "processId": { "type": "integer" }, "observeOnly": { "type": "boolean", "description": "Only observe: the debugger changes no data (default false)." }, "waitMs": wait_ms("a stop") }),
            &["processId"],
        ));
        tools.push(tool(
            "debug_open_dump",
            "Open a crash dump (.dmp with full memory) of a .NET or .NET Framework process. It shows as a stopped program: threads, \
             stack_trace, variables, evaluate (fields and locals only), exception and heap tools work; it cannot continue, step or run functions. \
             The engine opens dumps of its own bitness (an x86 process dumped by a 64-bit tool counts as x86).",
            json!({ "dumpPath": { "type": "string", "description": "Full path of the .dmp file." } }),
            &["dumpPath"],
        ));
    }
    tools.extend([
        tool(
            "debug_status",
            "State of the debug session: notStarted, running, stopped (with the thread, reason, top frame, and userFrame: the first frame with source when the top one has none) or exited (with the exit code); the process id.",
            json!({}),
            &[],
        ),
        tool("debug_terminate", "End the debugged program (a launched one) or detach (an attached one).", json!({}), &[]),
        tool("debug_continue", "Continue the stopped program.", json!({ "waitMs": wait_ms("the next stop (0: return at once)") }), &[]),
        tool("debug_pause", "Stop the running program (all threads).", json!({}), &[]),
        tool(
            "debug_step",
            "Step the stopped thread: over (next line), into (into the call on the line) or out (to the caller). Waits for the step to finish.",
            json!({
                "kind": { "type": "string", "enum": ["over", "into", "out"] },
                "threadId": super::thread_schema(),
                "waitMs": wait_ms("the step to finish"),
            }),
            &["kind"],
        ),
        tool(
            "debug_wait_for_stop",
            "Wait until the program stops (breakpoint, step, exception, pause) or exits. Returns at once if it is already stopped.",
            json!({ "waitMs": wait_ms("a stop") }),
            &[],
        ),
    ]);
    tools
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    let done = |r: Result<Value, String>| Ok(Some(Outcome::Done(r)));
    match name {
        "debug_launch" | "debug_attach" | "debug_open_dump" if ctx.mode != Mode::Standalone => done(Err(tr!("Launch and attach in the IDE (VS Code, for example). Over MCP you work with the session the IDE started."))),
        "debug_launch" => start(ctx, "launch", arguments).map(Some),
        // ダンプは launch の dumpPath で開く（エンジンの dumps.rs）。開くとすぐに止まった形になる。
        "debug_open_dump" => start(ctx, "launch", &json!({ "dumpPath": arguments.get("dumpPath").cloned().unwrap_or(Value::Null) })).map(Some),
        "debug_attach" => start(ctx, "attach", arguments).map(Some),
        "debug_status" => done(Ok(ctx.status()?)),
        "debug_terminate" => terminate(ctx).map(Some),
        "debug_continue" => {
            let thread = match ctx.stopped_thread(arguments) {
                Ok(thread) => thread,
                Err(e) => return done(Err(e)),
            };
            if let Err(e) = ctx.request("continue", json!({ "threadId": thread }))? {
                return done(Err(e));
            }
            ctx.wait_or_status(arguments, 0).map(Some)
        }
        "debug_pause" => {
            if !matches!(ctx.state.target, Target::Running) {
                return done(Err(ctx.state.not_running()));
            }
            match ctx.request("pause", json!({ "threadId": 0 }))? {
                Ok(_) => done(Ok(ctx.status()?)),
                Err(e) => done(Err(e)),
            }
        }
        "debug_step" => {
            let command = match arguments.get("kind").and_then(Value::as_str) {
                Some("over") => "next",
                Some("into") => "stepIn",
                Some("out") => "stepOut",
                _ => return done(Err(tr!("kind must be over, into or out."))),
            };
            let thread = match ctx.stopped_thread(arguments) {
                Ok(thread) => thread,
                Err(e) => return done(Err(e)),
            };
            if let Err(e) = ctx.request(command, json!({ "threadId": thread }))? {
                return done(Err(e));
            }
            ctx.wait_or_status(arguments, DEFAULT_WAIT_MS).map(Some)
        }
        "debug_wait_for_stop" => {
            if matches!(ctx.state.target, Target::NotStarted) {
                return done(Err(tr!("Not started yet. Start with debug_launch or debug_attach.")));
            }
            ctx.wait_or_status(arguments, DEFAULT_WAIT_MS).map(Some)
        }
        _ => Ok(None),
    }
}

/// `debug_launch` / `debug_attach`（`--mcp` だけ）。
fn start(ctx: &mut Context, request: &str, arguments: &Value) -> io::Result<Outcome> {
    if matches!(ctx.state.target, Target::Running | Target::Stopped(_)) {
        return Ok(Outcome::Done(Err(tr!("Already debugging. End it with debug_terminate first."))));
    }
    // 前のセッションが終わっていれば、新しいセッションで始める。
    if matches!(ctx.state.target, Target::Exited(_)) {
        ctx.backend.restart();
        ctx.state.restart();
        ctx.breakpoints.restart();
    }
    if let Some(project) = arguments.get("buildProject").and_then(Value::as_str) {
        if let Err(e) = build_project(project) {
            return Ok(Outcome::Done(Err(e)));
        }
    }
    if !ctx.state.initialized {
        if let Err(e) = ctx.request("initialize", json!({ "adapterID": "netdbg", "clientID": "mcp", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path" }))? {
            return Ok(Outcome::Done(Err(e)));
        }
        ctx.state.initialized = true;
    }
    let mut request_arguments = arguments.clone();
    if let Some(object) = request_arguments.as_object_mut() {
        object.remove("waitMs");
        object.remove("buildProject");
    }
    if let Err(e) = ctx.request(request, request_arguments)? {
        return Ok(Outcome::Done(Err(e)));
    }
    // 先に置かれたブレークポイントと例外の設定を、新しいセッションに置き直す。
    if let Err(e) = breakpoints::apply_all(ctx)? {
        return Ok(Outcome::Done(Err(e)));
    }
    if let Err(e) = ctx.request("configurationDone", json!({}))? {
        return Ok(Outcome::Done(Err(e)));
    }
    // ダンプは、configurationDone の答えと一緒に止まった知らせが届いている。
    if !matches!(ctx.state.target, Target::Stopped(_)) {
        ctx.state.target = Target::Running;
    }
    ctx.wait_or_status(arguments, 0)
}

fn terminate(ctx: &mut Context) -> io::Result<Outcome> {
    if !matches!(ctx.state.target, Target::Running | Target::Stopped(_)) {
        return Ok(Outcome::Done(Err(ctx.state.not_running())));
    }
    // 起動した対象は終わらせ、アタッチした対象は切り離す（エンジンは、MCP のために動き続ける）。
    if let Err(e) = ctx.request("terminate", json!({}))? {
        return Ok(Outcome::Done(Err(e)));
    }
    // 終わりの知らせ（exited）を待つ。それまでは、止まっていた状態のまま答えないように、動いているものとして扱う。
    ctx.state.target = Target::Running;
    Ok(Outcome::WaitForStop(Duration::from_millis(DEFAULT_WAIT_MS)))
}

/// `debug_launch` の `buildProject`: `dotnet build -c Debug`。失敗なら、出力の最後の行を理由にする。
fn build_project(project: &str) -> Result<(), String> {
    let output = std::process::Command::new("dotnet")
        .args(["build", project, "-c", "Debug", "-nologo", "-v", "q"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| tr!("Cannot start dotnet build: {error}. Check that the .NET SDK is installed and dotnet is on PATH.", error = e))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    Err(tr!("The build failed ({status}):\n{output}", status = output.status, output = tail[tail.len().saturating_sub(30)..].join("\n")))
}
