//! 止めずに読む記録（段階 11、D-063）: 止めずに読むブレークポイント（プローブ）、例外ログ、出来事の列。
//!
//! プローブは、エンジンの機能（SQL ログなど）がメソッドの入口に置く関数ブレークポイント。シンボルの無いモジュールにも
//! 置く。当たったら、そのスレッドの上で式を評価して記録し、`stopped` を出さずに続行する。クライアント（VS Code、AI）には
//! 番号も停止も見せない。利用者が同じメソッドにブレークポイントを置いていれば、それは別のブレークポイントとして当たり、
//! ふだんどおり止まる（ICorDebug は、同じ場所のブレークポイントを一つずつ知らせる）。

use super::*;
use crate::debugger::journal::{Journal, Record};
use crate::debugger::sqllog;
use std::collections::HashMap;

/// プローブを置いた機能。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Feature {
    /// SQL ログ: ADO.NET のコマンドの `Execute*`。
    Sql,
    /// イベント トレース: イベントのハンドラー。
    Event,
    /// ダイアログ: `MessageBox.Show` と `ShowDialog`（dialogs.rs）。引数を読むだけで、関数の評価はしない。
    Dialog,
    /// トランザクション: `BeginTransaction`、`Commit`、`Rollback`（transactions.rs）。SQL ログと一緒に入る。
    Transaction,
    /// 子プロセス: `Process.Start`（children.rs）。フィールドを読むだけで、関数の評価はしない。
    Child,
}

/// 対象を止めた形にして読み、記録するもの。
#[derive(Debug)]
pub(super) enum Recording {
    /// SQL ログのプローブに当たった（メソッドの名前）。
    Sql { method: String },
    /// イベント トレースのハンドラーに入った（ハンドラーの名前）。
    Event { handler: String },
    /// 例外が投げられた（例外ログ）。`stops` は、このあと止まるか。
    Exception { stops: bool },
    /// WinForms のバインディングが例外を飲み込む（捕まえるメソッドの名前。bindings.rs）。
    Binding { method: String },
    /// ダイアログを出す（メソッドの名前。dialogs.rs）。
    Dialog { method: String },
    /// トランザクションを始める・終える（メソッドの名前。transactions.rs）。
    Transaction { method: String },
    /// 子プロセスを起動する（children.rs）。
    Child,
}

/// 例外ログのスナップショット（拡張の exceptionSnapshotCore と同じ上限）。
mod snapshot {
    /// 残すコール スタックのフレームの数。
    pub const FRAMES: usize = 30;
    /// ローカル変数を読む、利用者のコード（ソースのある）のフレームの数。
    pub const FRAMES_WITH_LOCALS: usize = 3;
    /// 1 つのフレームで残すローカル変数の数（オブジェクトは開かない）。
    pub const LOCALS: usize = 30;
    /// 残す値の文字数。
    pub const VALUE_LENGTH: usize = 200;
    /// 1 回の対象で取るスナップショットの数。これより後の例外は、スナップショットなしで記録する。
    pub const PER_TARGET: usize = 200;
}

/// 1 つのメソッド名（多重定義はすべて）に置いたプローブ。
struct Probe {
    feature: Feature,
    /// `Namespace.Type.Method`。
    name: String,
    bound: Vec<ComPtr<ICorDebugFunctionBreakpoint>>,
    /// イベント トレース: このハンドラーにつないだイベントの見出し、イベントの名前、コントロールの名前（拡張の画面が渡す）。
    traced: Traced,
}

#[derive(Default)]
struct Traced {
    labels: std::collections::BTreeSet<String>,
    events: std::collections::BTreeSet<String>,
    controls: std::collections::BTreeSet<String>,
}

/// SQL ログのプローブを読むときに、呼び出し元として残すフレームの数（プロバイダーのフレームを除く）。
const SQL_CALLER_FRAMES: usize = 11;

#[derive(Default)]
pub(super) struct ProbeState {
    probes: Vec<Probe>,
    pub journal: Journal,
    /// 同じ文（空白の違いを除く）が実行された回数。
    sql_repeats: HashMap<String, usize>,
    /// 例外ログ: 投げられた例外を、止まらないものも含めて記録する。
    pub exception_log: bool,
    /// 例外ログで、スタックとローカル変数のスナップショットを取る。
    exception_snapshot: bool,
    /// この対象で取ったスナップショットの数。
    snapshots_taken: usize,
    /// 記録の処理が決めた停止（SQL で止める、ダイアログで止める）。記録のあと（`finish`）で、続行の代わりに使う。
    pub pending_stop: Option<Event>,
}

impl ProbeState {
    /// 対象を起動し直した（同じエンジンで、もう一度 launch / attach）: 前の対象の記録と繰り返しの数を捨てる。置いた
    /// プローブ（SQL ログの設定）はそのまま。
    pub(super) fn new_target(&mut self) {
        self.journal.clear();
        self.sql_repeats.clear();
        self.snapshots_taken = 0;
    }
}

impl Session {
    /// 起動とアタッチの引数 `sqlLog`（`true`、`false`、`{ "methods": [...] }`）。無ければ何もしない。
    pub(super) fn read_probe_options(&mut self, arguments: &Value) {
        match arguments.get("sqlLog") {
            Some(Value::Bool(true)) => self.configure_sql_log(true, None, None),
            Some(Value::Object(options)) if options.get("enabled").and_then(Value::as_bool) != Some(false) => {
                let list = |key: &str| options.get(key).and_then(Value::as_array).map(|m| m.iter().filter_map(Value::as_str).map(str::to_string).collect());
                self.configure_sql_log(true, list("methods"), list("transactionMethods"));
            }
            _ => {}
        }
        // ダイアログの記録は既定で入れる（`dialogLog: false` で切る）。
        let dialogs = arguments.get("dialogLog").is_none_or(|v| v.as_bool().unwrap_or_else(|| v.get("enabled").and_then(Value::as_bool) != Some(false)));
        self.configure_dialog_log(dialogs);
        match arguments.get("exceptionLog") {
            Some(Value::Bool(true)) => self.configure_exception_log(true, true),
            Some(Value::Object(options)) if options.get("enabled").and_then(Value::as_bool) != Some(false) => {
                self.configure_exception_log(true, options.get("snapshot").and_then(Value::as_bool).unwrap_or(true));
            }
            _ => {}
        }
    }

    /// `netdbg.setExceptionLog { enabled, snapshot? }`: 例外ログを始めるか止める。
    pub fn set_exception_log(&mut self, arguments: &Value) -> Result<Value, String> {
        let enabled = arguments.get("enabled").and_then(Value::as_bool).ok_or_else(|| tr!("netdbg.setExceptionLog has no enabled."))?;
        let snapshot = arguments.get("snapshot").and_then(Value::as_bool).unwrap_or(true);
        self.configure_exception_log(enabled, snapshot);
        Ok(json!({ "enabled": enabled, "snapshot": enabled && snapshot }))
    }

    fn configure_exception_log(&mut self, enabled: bool, snapshot: bool) {
        self.probes.exception_log = enabled;
        self.probes.exception_snapshot = enabled && snapshot;
        crate::log!("exception log: {}", if enabled { "on" } else { "off" });
    }

    /// 投げられた例外を記録する（例外ログ）。拡張の中継の `netdbg.exceptionThrown` と同じ中身。
    fn record_exception(&mut self, thread: u32, stops: bool) -> Vec<Event> {
        let Some(exception) = self.current_exception(thread) else { return Vec::new() };
        let type_name = unsafe { value::describe(&exception, &self.modules) }.type_name;
        let message = unsafe { value::string_field(&exception, &self.modules, &["_message"]) }.unwrap_or_default();
        // 止まるなら、止まる理由の判定（break mode）は止まる側が持つ。ここでは、投げられた（first chance）の記録。
        let break_mode = "always";
        let frames = self.walk_frames(thread).unwrap_or_default();
        let mut described = Vec::new();
        for frame in frames.iter().take(snapshot::FRAMES) {
            self.decompile_for_frame(frame);
            described.push(unsafe { self.describe_frame(0, frame) });
        }
        // 投げられた所が .NET の中なら、ソースのある最初のフレーム（利用者のコード）を、例外の場所として見せる。
        let user = described.iter().find(|f| f.pointer("/source/path").is_some()).cloned();
        let mut body = json!({ "threadId": thread, "typeName": type_name, "message": message, "breakMode": break_mode, "stopped": stops });
        if let Some(user) = &user {
            body["functionName"] = user["name"].clone();
            body["source"] = user["source"].clone();
            body["line"] = user["line"].clone();
        }
        // アセンブリを読めなかった例外なら、要求と読み込み済みの版、バインディング リダイレクト（D-091）。
        let load_failure = self.assembly_load_details(&type_name, &exception);
        if let Some((details, _)) = &load_failure {
            body["assemblyLoad"] = details.clone();
        }
        // 例外に至るまでの流れと、環境（fault.rs）。
        body["context"] = self.exception_context(thread);
        self.mark_environment(&mut body);
        if self.probes.exception_snapshot && self.probes.snapshots_taken < snapshot::PER_TARGET {
            self.probes.snapshots_taken += 1;
            body["snapshot"] = self.exception_snapshot(thread, &described, &exception);
        }
        let record = self.probes.journal.push("exception", body).to_json();
        // デバッグ コンソールにも 1 行（Visual Studio の「例外がスローされました」と同じ）。
        let place = user.as_ref().and_then(|u| Some(format!(" at {}:{}", u.pointer("/source/name")?.as_str()?, u["line"])));
        let text = if message.is_empty() { format!("Exception thrown: '{type_name}'") } else { format!("Exception thrown: '{type_name}': {message}") };
        let extra = load_failure.map(|(_, line)| format!("{line}\n")).unwrap_or_default();
        let mut output = json!({ "category": "console", "output": format!("{text}{}\n{extra}", place.unwrap_or_default()) });
        if let Some(user) = &user {
            output["source"] = user["source"].clone();
            output["line"] = user["line"].clone();
        }
        vec![("netdbg.exceptionThrown", record), ("output", output)]
    }

    /// 例外が投げられた瞬間のスタックと、利用者のコードの先頭のフレームのローカル変数。
    fn exception_snapshot(&mut self, thread: u32, frames: &[Value], exception: &ComPtr<ICorDebugValue>) -> Value {
        let mut with_locals = 0;
        let mut out = Vec::new();
        for (depth, frame) in frames.iter().enumerate() {
            let mut entry = json!({ "name": frame["name"], "line": frame["line"] });
            if let Some(source) = frame.get("source") {
                entry["source"] = source.clone();
            }
            // 拡張の形（exceptionSnapshotCore.ts）と同じく、ローカル変数を読むフレームに印を付ける。
            let read_locals = frame.pointer("/source/path").is_some() && with_locals < snapshot::FRAMES_WITH_LOCALS;
            entry["readLocals"] = json!(read_locals);
            if read_locals {
                with_locals += 1;
                let locals: Vec<Value> = self
                    .frame_variables(thread, depth)
                    .unwrap_or_default()
                    .into_iter()
                    .take(snapshot::LOCALS)
                    .map(|(name, value)| {
                        let described = unsafe { value::describe(&value, &self.modules) };
                        let mut shown = described.value;
                        if shown.chars().count() > snapshot::VALUE_LENGTH {
                            shown = shown.chars().take(snapshot::VALUE_LENGTH).collect::<String>() + "…";
                        }
                        json!({ "name": name, "value": shown, "type": described.type_name })
                    })
                    .collect();
                entry["locals"] = json!(locals);
            }
            out.push(entry);
        }
        let mut snapshot = json!({ "frames": out });
        // 投げられた瞬間の StackTrace は、そこまでの分（対象のコードを実行して読む）。
        if let Some(stack_trace) = self.string_property(thread, exception, "get_StackTrace") {
            snapshot["stackTrace"] = json!(stack_trace);
        }
        snapshot
    }

    /// `netdbg.setSqlLog { enabled, methods?, transactionMethods? }`: SQL ログ（とトランザクション）を始めるか止める。
    /// `methods` は捕まえるメソッドの全部、`transactionMethods` はトランザクションのメソッドの全部（無ければ既定の一覧）。
    pub fn set_sql_log(&mut self, arguments: &Value) -> Result<Value, String> {
        let enabled = arguments.get("enabled").and_then(Value::as_bool).ok_or_else(|| tr!("netdbg.setSqlLog has no enabled."))?;
        let list = |key: &str| arguments.get(key).and_then(Value::as_array).map(|m| m.iter().filter_map(Value::as_str).map(str::to_string).collect());
        self.configure_sql_log(enabled, list("methods"), list("transactionMethods"));
        let bound = self.probes.probes.iter().filter(|p| p.feature == Feature::Sql && !p.bound.is_empty()).map(|p| p.name.clone()).collect::<Vec<_>>();
        Ok(json!({ "enabled": enabled, "methods": self.probes.probes.iter().filter(|p| p.feature == Feature::Sql).map(|p| p.name.clone()).collect::<Vec<_>>(), "bound": bound }))
    }

    /// SQL ログのプローブを置き直す。読み込み済みのモジュールにはすぐ置き、これから読み込むものには読み込みのときに置く。
    pub(super) fn configure_sql_log(&mut self, enabled: bool, methods: Option<Vec<String>>, transactions: Option<Vec<String>>) {
        let names = if enabled { methods.unwrap_or_else(|| sqllog::DEFAULT_METHODS.iter().map(|m| m.to_string()).collect()) } else { Vec::new() };
        self.replace_probes(Feature::Sql, names);
        let names = if enabled { transactions.unwrap_or_else(|| super::transactions::DEFAULT_METHODS.iter().map(|m| m.to_string()).collect()) } else { Vec::new() };
        self.replace_probes(Feature::Transaction, names);
        crate::log!("sql log: {}", if enabled { "on" } else { "off" });
    }

    /// 機能のプローブを、`names`（`Namespace.Type.Method`。引数の並びは見ない）で置き換える。空なら外すだけ。
    pub(super) fn replace_probes(&mut self, feature: Feature, names: Vec<String>) {
        let (old, kept): (Vec<Probe>, Vec<Probe>) = std::mem::take(&mut self.probes.probes).into_iter().partition(|p| p.feature == feature);
        self.probes.probes = kept;
        let mut fresh: Vec<Probe> = names
            .into_iter()
            .map(|name| name.split('(').next().unwrap_or("").trim().to_string())
            .filter(|n| !n.is_empty())
            .map(|name| Probe { feature, name, bound: Vec::new(), traced: Traced::default() })
            .collect();
        let modules = &self.modules;
        self.while_synchronized(|| {
            for bound in old.iter().flat_map(|p| &p.bound) {
                unsafe { bound.Activate(0) };
            }
            for probe in fresh.iter_mut() {
                for module in modules {
                    bind_probe(module, probe);
                }
            }
        });
        self.probes.probes.extend(fresh);
    }

    /// 機能のプローブがあるか（記録が入っているか）。
    pub(super) fn probe_feature_on(&self, feature: Feature) -> bool {
        self.probes.probes.iter().any(|p| p.feature == feature)
    }

    /// 読み込んだモジュールに、待っていたプローブを置く。置いたら、そのモジュールの JIT の最適化を切る。最適化された
    /// コード（.NET Framework の GAC の System.Data など）の入口では、ランタイムが関数の評価を断る（0x80131C26）ため。
    /// 読み込みの通知の中でしか切れないので、あとから始めた記録は、すでに読み込まれた最適化されたモジュールでは読めない
    /// ことがある。
    pub(super) fn bind_probes(&mut self, index: usize) {
        let module = &self.modules[index];
        let mut bound = false;
        for probe in self.probes.probes.iter_mut() {
            // ダイアログと子プロセスは引数とフィールドを読むだけで関数を評価しないので、最適化は切らない（WinForms / WPF、
            // System.dll 全体が遅くなる）。
            bound |= bind_probe(module, probe) && !matches!(probe.feature, Feature::Dialog | Feature::Child);
        }
        if bound && module.pdb.is_none() {
            if let Ok(module2) = module.module.cast::<ICorDebugModule2>() {
                let hr = unsafe { module2.SetJITCompilerFlags(CORDEBUG_JIT_DISABLE_OPTIMIZATION) };
                crate::log!("probe module {}: SetJITCompilerFlags {hr}", module.path);
            }
        }
    }

    /// 当たったブレークポイントと同じ場所（メソッドと IL オフセット）にあるプローブの、機能とメソッドの名前。同じ場所に
    /// 利用者のブレークポイントもあると、ランタイムは片方しか知らせてこないので、オブジェクトではなく場所で比べる。
    pub(super) fn probe_at(&self, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Option<Recording> {
        let raw = breakpoint.as_raw();
        let recording = |probe: &Probe| match probe.feature {
            Feature::Sql => Recording::Sql { method: probe.name.clone() },
            Feature::Event => Recording::Event { handler: probe.name.clone() },
            Feature::Dialog => Recording::Dialog { method: probe.name.clone() },
            Feature::Transaction => Recording::Transaction { method: probe.name.clone() },
            Feature::Child => Recording::Child,
        };
        if let Some(probe) = self.probes.probes.iter().find(|p| p.bound.iter().any(|b| b.as_raw() == raw)) {
            return Some(recording(probe));
        }
        if self.probes.probes.is_empty() {
            return None;
        }
        let function_breakpoint = breakpoint.cast::<ICorDebugFunctionBreakpoint>().ok()?;
        let location = unsafe { location_of(&function_breakpoint) }?;
        self.probes.probes.iter().find(|p| p.bound.iter().any(|b| unsafe { location_of(b) }.as_ref() == Some(&location))).map(recording)
    }

    /// 置いたプローブのブレークポイントすべて（カバレッジが、同じ場所のものを探すのに使う）。
    pub(super) fn probe_breakpoints(&self) -> impl Iterator<Item = &ComPtr<ICorDebugFunctionBreakpoint>> {
        self.probes.probes.iter().flat_map(|p| &p.bound)
    }

    /// 当たったブレークポイントがプローブのとき、同じ場所にある利用者のブレークポイントの「番号、条件」。
    pub(super) fn user_breakpoints_at(&self, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Vec<i64> {
        let Some(location) = breakpoint.cast::<ICorDebugFunctionBreakpoint>().ok().and_then(|b| unsafe { location_of(&b) }) else { return Vec::new() };
        self.bindings().filter(|(_, bound)| bound.iter().any(|b| unsafe { location_of(b) }.as_ref() == Some(&location))).map(|(id, _)| id).collect()
    }

    /// 対象を止めた形にして読み、記録する。送る出来事を返す。続行するか止まるかは、呼び出し側が決める。
    pub(super) fn run_record(&mut self, thread: u32, what: &Recording, controller: &ComPtr<ICorDebugController>) -> Vec<Event> {
        let started = std::time::Instant::now();
        // 例外で止まるなら、その情報は残す。
        self.hold_for_judging(controller);
        let stopped_exception = self.stopped_exception.take();
        let mut out = match what {
            Recording::Sql { method } => self.record_sql(thread, method),
            Recording::Exception { stops } => self.record_exception(thread, *stops),
            Recording::Event { handler } => self.record_event(thread, handler),
            Recording::Binding { method } => self.record_binding(thread, method),
            Recording::Dialog { method } => self.record_dialog(thread, method),
            Recording::Transaction { method } => self.record_transaction(thread, method),
            Recording::Child => self.record_child_start(thread),
        };
        // 評価で知ったフレームや値は捨てる（このあと止まるなら、利用者には新しい停止として見せる）。
        self.forget_stop();
        self.stopped_exception = stopped_exception;
        crate::log!("record {what:?} on thread {thread}: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        out.extend(self.take_deferred());
        out
    }

    /// SQL ログ: 止めたフレームのコマンドを読み、値を埋めた文と繰り返しの数を足して、列と出来事にする。
    fn record_sql(&mut self, thread: u32, method: &str) -> Vec<Event> {
        let Ok(frames) = self.walk_frames(thread) else { return Vec::new() };
        // 呼び出し元のフレームは、式を評価する前に読む（評価で対象のコードを動かすと、フレームのオブジェクトは使えなくなる）。
        // PDB の無いアプリのモジュールなら、逆コンパイルしてソースの場所を付ける（D-060）。
        let mut callers = Vec::new();
        for frame in frames.iter().skip(1).take(SQL_CALLER_FRAMES) {
            self.decompile_for_frame(frame);
            let described = unsafe { self.describe_frame(0, frame) };
            let mut caller = json!({ "name": described["name"], "line": described["line"] });
            if let Some(source) = described.get("source") {
                caller["source"] = source.clone();
            }
            callers.push(caller);
        }
        drop(frames);
        // `ExecuteReader()` は同じ文の `ExecuteReader(behavior)` を呼ぶことが多い。内側の呼び出しは記録しない。
        if callers.first().and_then(|c| c["name"].as_str()).is_some_and(|caller| self.probes.probes.iter().any(|p| p.feature == Feature::Sql && is_method(caller, &p.name))) {
            return Vec::new();
        }
        let type_name = method.rsplit_once('.').map_or(method, |(owner, _)| owner).to_string();
        let command = {
            let mut reader = FrameReader { session: self, thread };
            sqllog::capture(&mut reader, &type_name)
        };
        let Some(command) = command else { return Vec::new() };
        let sql = sqllog::substitute(&command, sqllog::dialect_of(&type_name));
        let repeat = {
            let count = self.probes.sql_repeats.entry(sqllog::normalize(&command.command_text)).or_insert(0);
            *count += 1;
            *count
        };
        let mut body = command.to_json();
        body["threadId"] = json!(thread);
        body["method"] = json!(method);
        body["typeName"] = json!(type_name);
        body["sql"] = json!(sql);
        body["repeat"] = json!(repeat);
        if repeat >= sqllog::REPEAT_WARNING {
            body["repeated"] = json!(true);
        }
        if let Some(caller) = caller_of(&callers) {
            body["caller"] = caller.clone();
        }
        body["frames"] = json!(callers);
        // 開いているトランザクションの中なら、その番号（transactions.rs）。
        let transaction = self.transaction_of_command(thread);
        if let Some(id) = transaction {
            body["transactionId"] = json!(id);
        }
        // SQL で止める（sqlbreak.rs）。止まらない文は、実行時間を計る。
        let stops = self.sql_breakpoints_hit(thread, &command, &sql, &mut body);
        let record = self.probes.journal.push("sql", body);
        let (seq, event) = (record.seq, record.to_json());
        if let Some(id) = transaction {
            self.add_transaction_statement(id, seq);
        }
        if !stops {
            self.time_sql(thread, seq);
        }
        vec![("netdbg.sqlExecuted", event)]
    }

    /// いちばん新しい出来事が、種類 `kind` で `within_ms` 以内に積まれ、`same` が真か。多重定義が多重定義を呼ぶとき
    /// （x64 の JIT は末尾呼び出しにして外側のフレームを消すので、呼び出し元では見分けられない）の、2 つめを捨てるのに使う。
    pub(super) fn repeats_newest_record(&self, kind: &str, within_ms: u64, same: impl Fn(&Value) -> bool) -> bool {
        self.probes.journal.newest().is_some_and(|last| last.kind == kind && crate::debugger::journal::now_ms().saturating_sub(last.time) < within_ms && same(&last.body))
    }

    /// SQL ログが入っているか（SQL のプローブがある）。
    pub(super) fn sql_log_on(&self) -> bool {
        self.probe_feature_on(Feature::Sql)
    }

    /// `netdbg.setEventTrace { handlers?: [{ breakpointName, label?, event?, control? }], clear? }`: イベントのハンドラーの
    /// 入口で、止めずに `sender` を記録する（拡張の中継と同じ要求）。ハンドラーは、すでにトレースしているものに足す。
    pub fn set_event_trace(&mut self, arguments: &Value) -> Result<Value, String> {
        let mut removed = Vec::new();
        if arguments.get("clear").and_then(Value::as_bool) == Some(true) {
            let (old, kept): (Vec<Probe>, Vec<Probe>) = std::mem::take(&mut self.probes.probes).into_iter().partition(|p| p.feature == Feature::Event);
            self.probes.probes = kept;
            removed = old;
        }
        // 新しく置くハンドラーは、置き終えてから一覧に入れる（すでにあるハンドラーには、見出しなどを足すだけ）。
        let mut fresh: Vec<Probe> = Vec::new();
        for handler in arguments.get("handlers").and_then(Value::as_array).into_iter().flatten() {
            let Some(name) = handler.get("breakpointName").and_then(Value::as_str).map(|n| n.split('(').next().unwrap_or("").trim().to_string()).filter(|n| !n.is_empty()) else { continue };
            let existing = self.probes.probes.iter_mut().chain(fresh.iter_mut()).find(|p| p.feature == Feature::Event && p.name == name);
            let probe = match existing {
                Some(probe) => probe,
                None => {
                    fresh.push(Probe { feature: Feature::Event, name, bound: Vec::new(), traced: Traced::default() });
                    fresh.last_mut().expect("pushed just now")
                }
            };
            for (key, set) in [("label", &mut probe.traced.labels), ("event", &mut probe.traced.events), ("control", &mut probe.traced.controls)] {
                if let Some(value) = handler.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    set.insert(value.to_string());
                }
            }
        }
        let modules = &self.modules;
        self.while_synchronized(|| {
            for bound in removed.iter().flat_map(|p| &p.bound) {
                unsafe { bound.Activate(0) };
            }
            for probe in fresh.iter_mut() {
                for module in modules {
                    bind_probe(module, probe);
                }
            }
        });
        self.probes.probes.extend(fresh);
        let count = self.probes.probes.iter().filter(|p| p.feature == Feature::Event).count();
        Ok(json!({ "count": count }))
    }

    /// イベント トレース: ハンドラーに入った。どのコントロールのイベントか（`sender` の名前、無ければ型）を記録する。
    fn record_event(&mut self, thread: u32, handler: &str) -> Vec<Event> {
        let frame = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().next());
        let described = frame.map(|frame| {
            self.decompile_for_frame(&frame);
            unsafe { self.describe_frame(0, &frame) }
        });
        let sender = {
            let mut reader = FrameReader { session: self, thread };
            use sqllog::Reader;
            match reader.evaluate("sender.Name").map(|s| sqllog::unquote(&s.value)).filter(|name| !name.is_empty() && name != "null") {
                Some(name) => Some(name),
                None => reader.evaluate("sender.GetType().Name").map(|s| format!("({})", sqllog::unquote(&s.value))),
            }
        };
        let Some(probe) = self.probes.probes.iter().find(|p| p.feature == Feature::Event && p.name == handler) else { return Vec::new() };
        let mut body = json!({
            "threadId": thread,
            "events": probe.traced.events,
            "labels": probe.traced.labels,
            "controls": probe.traced.controls,
            "handler": described.as_ref().map_or(Value::Null, |d| d["name"].clone()),
        });
        if let Some(sender) = sender {
            body["sender"] = json!(sender);
        }
        if let Some(described) = &described {
            if let Some(source) = described.get("source") {
                body["source"] = source.clone();
            }
            body["line"] = described["line"].clone();
        }
        let record = self.probes.journal.push("event", body);
        vec![("netdbg.controlEvent", record.to_json())]
    }

    /// `netdbg.journal { after?, kinds?, limit? }`: 出来事の列を、番号 `after` より後から古い順に読む。
    pub fn read_journal(&self, arguments: &Value) -> Result<Value, String> {
        let after = arguments.get("after").and_then(Value::as_u64).unwrap_or(0);
        let kinds: Vec<String> = arguments.get("kinds").and_then(Value::as_array).map(|k| k.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let limit = arguments.get("limit").and_then(Value::as_u64).map_or(1000, |l| l as usize);
        let journal = &self.probes.journal;
        let records: Vec<Value> = journal.read(after, &kinds, limit).into_iter().map(Record::to_json).collect();
        Ok(json!({ "records": records, "last": journal.last_seq(), "dropped": journal.dropped() }))
    }
}

/// 止めたスレッドのいちばん上のフレームで評価する、SQL ログの読み手。
struct FrameReader<'a> {
    session: &'a mut Session,
    thread: u32,
}

impl sqllog::Reader for FrameReader<'_> {
    fn evaluate(&mut self, expression: &str) -> Option<sqllog::Shown> {
        let value = match self.session.evaluate_expression(self.thread, 0, expression) {
            Ok(eval::Evaluated::Value(value) | eval::Evaluated::Nullable(value)) => value,
            Ok(_) => return None,
            Err(e) => {
                crate::log!("sql log: {expression}: {e}");
                return None;
            }
        };
        let described = unsafe { value::describe(&value, &self.session.modules) };
        Some(sqllog::Shown { value: described.value, type_name: described.type_name })
    }
}

/// フレームの名前（`Ns.Type.Method()`、型引数つきもある）が、メソッド `Ns.Type.Method` か。
pub(super) fn is_method(frame: &str, method: &str) -> bool {
    let name = frame.split('(').next().unwrap_or(frame);
    // 型引数（`List<int>.Add`）を外して比べる。
    let mut plain = String::with_capacity(name.len());
    let mut depth = 0;
    for c in name.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain == method
}

/// 文を実行させた呼び出し元として見せるフレーム: 利用者のコード（ソースがある）の最初のもの、無ければ .NET とプロバイダーの
/// 外の最初のもの（シンボルの無い自作のデータアクセスの部品）。拡張の `callerOf` と同じ。
fn caller_of(frames: &[Value]) -> Option<&Value> {
    const FRAMEWORK: [&str; 5] = ["System.", "Microsoft.", "Oracle.", "MS.", "Windows."];
    frames
        .iter()
        .find(|f| f.pointer("/source/path").is_some())
        .or_else(|| frames.iter().find(|f| !FRAMEWORK.iter().any(|p| f["name"].as_str().unwrap_or("").starts_with(p))))
        .or_else(|| frames.first())
}

/// プローブを、モジュールの中の名前に合うメソッドすべての入口に置く（シンボルが無くても置く）。置けたら true。
fn bind_probe(module: &Module, probe: &mut Probe) -> bool {
    let Some(metadata) = module.metadata() else { return false };
    let mut bound = false;
    for method in metadata.find_methods(&probe.name) {
        match unsafe { break_at(&module.module, method, 0) } {
            Ok(created) => {
                probe.bound.push(created);
                bound = true;
            }
            Err(e) => crate::log!("probe {} at 0x{method:08X} in {}: {e}", probe.name, module.path),
        }
    }
    bound
}

/// 関数ブレークポイントの場所: モジュールのパス、メソッドのトークン、IL オフセット。
pub(super) unsafe fn location_of(breakpoint: &ComPtr<ICorDebugFunctionBreakpoint>) -> Option<(String, u32, u32)> {
    let function = take::<ICorDebugFunction>("GetFunction", |out| breakpoint.GetFunction(out)).ok()?;
    let mut token = 0u32;
    function.GetToken(&mut token);
    let module = take::<ICorDebugModule>("GetModule", |out| function.GetModule(out)).ok()?;
    let mut offset = 0u32;
    breakpoint.GetOffset(&mut offset);
    Some((module_path(&module), token, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_names_match_methods_without_their_type_arguments() {
        assert!(is_method("System.Data.SqlClient.SqlCommand.ExecuteReader()", "System.Data.SqlClient.SqlCommand.ExecuteReader"));
        assert!(is_method("Ns.Repo<int>.Run()", "Ns.Repo.Run"));
        assert!(!is_method("Ns.Repo.RunAll()", "Ns.Repo.Run"));
    }

    #[test]
    fn the_caller_is_the_first_frame_of_the_users_code_else_outside_the_framework() {
        let frames = vec![json!({ "name": "MyDal.Db.Query()" }), json!({ "name": "OrderForm.Load()", "source": { "path": "C:\\app\\OrderForm.cs" }, "line": 12 })];
        assert_eq!(caller_of(&frames).unwrap()["name"], "OrderForm.Load()");
        let frames = vec![json!({ "name": "System.Data.Common.DbDataAdapter.Fill()" }), json!({ "name": "Oracle.X.Y()" }), json!({ "name": "Company.Dal.Run()" })];
        assert_eq!(caller_of(&frames).unwrap()["name"], "Company.Dal.Run()");
        assert_eq!(caller_of(&[json!({ "name": "System.A()" })]).unwrap()["name"], "System.A()");
        assert!(caller_of(&[]).is_none());
    }
}
