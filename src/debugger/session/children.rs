//! 子プロセス（段階 12）: 対象が `Process.Start` で起動したプロセスを記録し、追う設定なら、子を止めたまま、
//! クライアントに子へのアタッチを頼む（DAP の逆向きの要求 `startDebugging`）。子のビット数に合うエンジンは、クライアント
//! （拡張）がアタッチと同じ道で選ぶ（ビット数が違う子も追える）。子のエンジンは、ランタイムの起動を待つ登録をしてから
//! 子を動かす（`__resumeChild`）ので、子の最初のコードから止められる（.NET Framework の子は、ランタイムが読み込まれて
//! からつなぐ）。
//!
//! プローブの機能 `Child`（`System.Diagnostics.Process.Start`）。読むのはフィールドだけで、関数の評価はしない（だから
//! プローブを置いたモジュールの最適化は切らない）。子の番号は、`Start` から戻ったとき（stepOut の見張り）に読む。
//! 起動の引数 `childProcesses`: `"record"`（既定。記録だけ）、`"follow"`（追う）、`"off"`。

use super::*;

const METHOD: &str = "System.Diagnostics.Process.Start";
/// 呼び出し元として探すフレームの数。
const CALLER_SEARCH: usize = 40;

/// 子プロセスの扱い。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum ChildMode {
    Off,
    #[default]
    Record,
    Follow,
}

/// `Start` から戻るのを待っている起動。
struct Pending {
    stepper: ComPtr<ICorDebugStepper>,
    /// `Process` のオブジェクト（弱いハンドル）。戻ったら番号を読む。
    process: ComPtr<ICorDebugHandleValue>,
    body: Value,
}

#[derive(Default)]
pub(super) struct ChildState {
    pub mode: ChildMode,
    pending: Vec<Pending>,
    /// クライアントが `startDebugging` を受けられる（`initialize` の `supportsStartDebuggingRequest`）。
    pub client_starts_debugging: bool,
}

impl ChildState {
    pub(super) fn forget_target(&mut self) {
        self.pending.clear();
    }
}

impl Session {
    /// `initialize` の `supportsStartDebuggingRequest`（クライアントが子のデバッグのセッションを始められるか）。
    pub fn set_client_starts_debugging(&mut self, supported: bool) {
        self.children.client_starts_debugging = supported;
    }

    /// 起動の引数 `childProcesses`。プローブを置き直す。
    pub(super) fn read_child_options(&mut self, arguments: &Value) {
        self.children.mode = match arguments.get("childProcesses").and_then(Value::as_str) {
            Some("off") => ChildMode::Off,
            Some("follow") => ChildMode::Follow,
            _ => ChildMode::Record,
        };
        let names = if self.children.mode == ChildMode::Off { Vec::new() } else { vec![METHOD.to_string()] };
        self.replace_probes(probes::Feature::Child, names);
    }

    /// `Process.Start` の入口（止めた形にしてある）: 起動するファイルと引数を読み、戻るのを見張る。
    pub(super) fn record_child_start(&mut self, thread: u32) -> Vec<Event> {
        // 静的な `Process.Start(...)` は、中でインスタンスの `Start()` を呼ぶ。`this` が Process のものだけを扱う。
        let Some(this) = self.top_this(thread) else { return Vec::new() };
        if unsafe { value::runtime_type_name(&this, &self.modules) }.as_deref() != Some("System.Diagnostics.Process") {
            return Vec::new();
        }
        let caller = stack::user_caller(&self.describe_top_frames(thread, CALLER_SEARCH), 1).unwrap_or(Value::Null);
        let (file, arguments, shell) = unsafe {
            let info = value::field(&this, &self.modules, "_startInfo").or_else(|| value::field(&this, &self.modules, "startInfo"));
            let text = |names: &[&str]| info.as_ref().and_then(|i| value::string_field(i, &self.modules, names)).unwrap_or_default();
            let shell = info.as_ref().and_then(|i| value::field(i, &self.modules, "_useShellExecute").or_else(|| value::field(i, &self.modules, "useShellExecute"))).is_some_and(|v| value::describe(&v, &self.modules).value == "true");
            (text(&["_fileName", "fileName"]), text(&["_arguments", "arguments"]), shell)
        };
        let body = json!({ "fileName": file, "arguments": arguments, "useShellExecute": shell, "caller": caller, "threadId": thread });
        let watched = unsafe { super::transactions::weak_handle(&this) }.and_then(|process| self.step_out_watch(thread).map(|stepper| (stepper, process)));
        match watched {
            Some((stepper, process)) => {
                self.children.pending.push(Pending { stepper, process, body });
                Vec::new()
            }
            // 戻るのを見張れなければ、番号なしで記録する。
            None => vec![("netdbg.childProcess", self.probes.journal.push("process", body).to_json())],
        }
    }

    /// ステップが終わった: 子の起動を見張っていたステッパーなら、子の番号を読んで記録し、追う設定なら子を止めて
    /// `startDebugging` を頼む。ほかのステッパーなら `None`。
    pub(super) fn child_started(&mut self, stepper: &ComPtr<ICorDebugStepper>, reason: u32) -> Option<Vec<Event>> {
        const STEP_EXCEPTION_FILTER: u32 = 3;
        const STEP_EXCEPTION_HANDLER: u32 = 4;
        let index = self.children.pending.iter().position(|p| p.stepper.as_raw() == stepper.as_raw())?;
        let Pending { process, mut body, .. } = self.children.pending.swap_remove(index);
        let mut out = Vec::new();
        if matches!(reason, STEP_EXCEPTION_FILTER | STEP_EXCEPTION_HANDLER) {
            body["failed"] = json!(true);
            out.push(("netdbg.childProcess", self.probes.journal.push("process", body).to_json()));
            return Some(out);
        }
        let process_id = process.cast::<ICorDebugValue>().ok().and_then(|v| unsafe {
            let id = value::field(&v, &self.modules, "_processId").or_else(|| value::field(&v, &self.modules, "processId"))?;
            value::describe(&id, &self.modules).value.parse::<u32>().ok()
        });
        let file = body["fileName"].as_str().unwrap_or("").to_string();
        if let Some(pid) = process_id.filter(|&p| p != 0) {
            body["processId"] = json!(pid);
            let kind = child_kind(pid);
            body["kind"] = json!(kind);
            if let Some(bitness) = child_bitness(pid) {
                body["bitness"] = json!(bitness);
            }
            let request = self.follow_child(pid, &file, kind, &mut body);
            body["followed"] = json!(request.is_some());
            let note = if request.is_some() { " — attaching" } else { "" };
            out.push(("output", json!({ "category": "console", "output": format!("Child process started: {file} (pid {pid}, {}, {kind}){note}\n", body["bitness"].as_str().unwrap_or("?")) })));
            out.extend(request);
        } else {
            // シェルで開いた（文書を開いた、すでに動いているプロセスに渡した）ときは、番号が無い。
            out.push(("output", json!({ "category": "console", "output": format!("Child process started: {file} (no process id)\n") })));
        }
        out.insert(0, ("netdbg.childProcess", self.probes.journal.push("process", body).to_json()));
        Some(out)
    }

    /// 追う設定なら、子を止めて、クライアントにアタッチを頼む要求を返す。頼めなければ `None`（理由は `body` に書く）。
    fn follow_child(&mut self, pid: u32, file: &str, kind: &str, body: &mut Value) -> Option<Event> {
        if self.children.mode != ChildMode::Follow {
            return None;
        }
        let reason = if kind == "native" {
            Some("not a .NET program")
        } else if !self.children.client_starts_debugging {
            Some("the client cannot start child debug sessions (startDebugging)")
        } else {
            None
        };
        if let Some(reason) = reason {
            body["notFollowed"] = json!(reason);
            return None;
        }
        // 子は止めたまま渡す（デバッガー無しで先へ進ませない）。子のエンジンが、ランタイムを待つ登録をしてから動かす。
        if !win32::suspend_process(pid, true) {
            body["notFollowed"] = json!("could not suspend the child process");
            return None;
        }
        let name = Path::new(file).file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
        let configuration = json!({ "name": format!("{name} (pid {pid})"), "processId": pid, "__resumeChild": true });
        Some((REVERSE_REQUEST_START_DEBUGGING, json!({ "request": "attach", "configuration": configuration })))
    }
}

/// 逆向きの要求 `startDebugging` の出来事の名前（`REVERSE_REQUEST` + 要求の名前）。
const REVERSE_REQUEST_START_DEBUGGING: &str = "request:startDebugging";

/// 子の種類: `.NET Framework`（exe に CLR ヘッダー）、`.NET`（隣に `.runtimeconfig.json` か同名の dll がある）、`native`。
fn child_kind(pid: u32) -> &'static str {
    let Some(image) = target::process_image_path(pid) else { return "native" };
    let path = Path::new(&image);
    if std::fs::read(path).ok().and_then(|data| target::inspect(&data)).is_some_and(|e| e.managed) {
        return ".NET Framework";
    }
    let config = path.with_extension("runtimeconfig.json");
    let dll = path.with_extension("dll");
    if config.is_file() || dll.is_file() {
        ".NET"
    } else {
        "native"
    }
}

/// 子のビット数。WOW64 で動いていれば x86。そうでなければ、64bit の Windows なら x64、32bit の Windows なら x86。
fn child_bitness(pid: u32) -> Option<&'static str> {
    let handle = win32::open_process_for_query(pid)?;
    let mut wow64 = 0i32;
    if unsafe { win32::IsWow64Process(handle.raw(), &mut wow64) } == 0 {
        return None;
    }
    Some(if wow64 != 0 || !windows_is_64bit() { "x86" } else { "x64" })
}

/// Windows が 64bit か（64bit のエンジンなら 64bit。32bit のエンジンなら、自分が WOW64 で動いているか）。
fn windows_is_64bit() -> bool {
    let mut wow64 = 0i32;
    std::mem::size_of::<usize>() == 8 || (unsafe { win32::IsWow64Process(win32::GetCurrentProcess(), &mut wow64) } != 0 && wow64 != 0)
}
