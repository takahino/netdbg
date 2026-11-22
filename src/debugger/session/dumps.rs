//! クラッシュダンプのセッション（段階 12）: 起動の引数 `dumpPath` でダンプを開き、止まったプロセスとして見せる。
//! スレッド、コール スタック、ローカル変数、例外、モジュール、ヒープの統計、保持している経路は、生きたプロセスと
//! 同じ道で読む（ICorDebug が、ダンプの上に止まったプロセスを作る。dump.rs）。
//!
//! 対象は動いていないので、続行、ステップ、ブレークポイント、関数の評価（プロパティの getter など）、値の書き換えは
//! できない。頼まれたら、その理由を返す（`READ_ONLY`）。止めずに読む記録（SQL ログなど）と UI のフリーズの監視も
//! 行わない。

use super::*;
use crate::debugger::dump;

/// ダンプで断る要求と、その理由。
pub(super) const REFUSED_FOR_DUMPS: [&str; 12] =
    ["continue", "next", "stepIn", "stepOut", "pause", "goto", "setVariable", "setExpression", "netdbg.applyChanges", "netdbg.freezeThread", "netdbg.makeObjectId", "netdbg.processUsage"];
pub(super) fn read_only() -> String { tr!("A crash dump is read only. Continuing, stepping, evaluating functions and changing values are not possible.") }

#[derive(Default)]
pub(super) struct DumpState {
    /// 起動の引数 `dumpPath`。
    path: Option<String>,
    /// 開いたダンプ（DAC と、ダンプを読む口を持ち続ける）。
    opened: Option<dump::OpenedDump>,
}

impl Session {
    /// ダンプのセッションか（`launch` に `dumpPath` が来た）。
    pub fn is_dump(&self) -> bool {
        self.dump.path.is_some()
    }

    /// ダンプで断る要求なら、その理由。
    pub fn refuse_for_dump(&self, command: &str) -> Option<String> {
        (self.is_dump() && REFUSED_FOR_DUMPS.contains(&command)).then(read_only)
    }

    /// `launch` の `dumpPath`: ファイルを確かめて覚える。開くのは `configurationDone`。
    pub(super) fn launch_dump(&mut self, path: &str, arguments: &Value) -> Result<(), String> {
        if !Path::new(path).is_file() {
            return Err(tr!("The dump file does not exist: {path}", path = path));
        }
        self.read_debug_options(arguments)?;
        // 対象のコードは実行できないので、プロパティは開いたときに読まない（クリックで評価する形。評価は断られる）。
        self.implicit_evaluation_allowed = false;
        self.dump.path = Some(path.to_string());
        Ok(())
    }

    /// `configurationDone`: ダンプを開き、モジュール、プロセス、止まった知らせを送る。
    pub(super) fn open_dump(&mut self) -> Result<(), String> {
        if self.dump.opened.is_some() {
            return Ok(());
        }
        let path = self.dump.path.clone().ok_or_else(|| tr!("There is no dump."))?;
        let opened = dump::open(Path::new(&path))?;
        let process = opened.process.clone();
        self.process = Some(process.clone());
        self.state = State::Stopped;
        let modules = unsafe { all_modules(&process) };
        // アプリのフォルダー（逆コンパイルの対象を決める）は、最初の exe のモジュールのフォルダー。
        if let Some(program) = modules.iter().map(module_path).find(|p| p.to_lowercase().ends_with(".exe")) {
            self.decompile.set_program(&program);
        }
        for module in modules {
            let events = self.module_loaded(module);
            self.deferred.extend(events);
        }
        // ランタイムの数え上げの順は、作られた順ではない。ダンプのスレッドの順に並べ、最初のものを Main Thread とする。
        let mut threads = unsafe { all_threads(&process) };
        threads.sort_by_key(|id| opened.thread_order.iter().position(|t| t == id).unwrap_or(usize::MAX));
        self.main_thread = threads.first().copied();
        self.threads = threads;
        let name = self.modules.iter().map(|m| m.path.clone()).find(|p| p.to_lowercase().ends_with(".exe")).unwrap_or_else(|| path.clone());
        // 番号は、ダンプを書いたときのもの。いまの機械のプロセスではないので、systemProcessId には入れない（拡張が、
        // その番号のプロセスの見張りを始めてしまう）。
        self.deferred.push(("process", json!({ "name": name, "isLocalProcess": false, "startMethod": "attach", "dumpProcessId": opened.process_id })));
        let kind = if opened.framework { ".NET Framework" } else { ".NET" };
        let bits = if std::mem::size_of::<usize>() == 4 { if opened.wow64 { "x86, written as a 64-bit dump" } else { "x86" } } else { "x64" };
        self.deferred.push(("output", json!({ "category": "console", "output": format!("Opened the crash dump {path} ({kind}, {bits}). It is read only: the program cannot run, step or evaluate functions.\n") })));
        let stopped = self.dump_stop(opened.exception);
        self.deferred.push(stopped);
        self.dump.opened = Some(opened);
        Ok(())
    }

    /// 止まった知らせ: 例外を投げている途中のスレッド（ダンプを書いたときの例外のスレッドを先に）があれば例外で、
    /// 無ければ一時停止として。
    fn dump_stop(&mut self, exception: Option<(u32, u32)>) -> Event {
        let candidates: Vec<u32> = exception.map(|(thread, _)| thread).into_iter().chain(self.threads.iter().copied()).collect();
        let thrown = candidates.into_iter().find_map(|thread| self.current_exception(thread).map(|e| (thread, e)));
        let Some((thread, exception)) = thrown else {
            let thread = exception.map(|(thread, _)| thread).filter(|t| self.threads.contains(t)).or(self.main_thread).unwrap_or(0);
            return ("stopped", json!({ "reason": "pause", "description": "Crash dump", "threadId": thread, "allThreadsStopped": true }));
        };
        let type_name = unsafe { value::describe(&exception, &self.modules) }.type_name;
        // 場所は、利用者のコード（シンボルのあるモジュール）の最初のフレーム。ダンプの一番上は、待ちや例外の処理の
        // 中（.NET の部品）であることが多い。
        let frames = self.walk_frames(thread).unwrap_or_default();
        let user = frames.iter().find(|frame| unsafe { self.frame_module(frame) }.is_some_and(|m| m.pdb.is_some())).or(frames.first());
        let module = user.and_then(|frame| unsafe { self.frame_module(frame) }.map(|m| file_name(&m.path))).unwrap_or_else(|| "<unknown module>".into());
        // 文言は、生きたプロセスの未処理の例外と同じ（exceptionInfo もそれに合わせる）。
        let text = format!("An unhandled exception of type '{type_name}' occurred in {module}");
        self.stopped_exception = Some(StoppedException { thread, break_mode: "unhandled", text: text.clone() });
        ("stopped", json!({ "reason": "exception", "description": "Crash dump", "threadId": thread, "text": text, "allThreadsStopped": true }))
    }

    /// ダンプを閉じる（`disconnect`）。
    pub(super) fn close_dump(&mut self) {
        self.forget_process();
        self.dump.opened = None;
    }
}

/// プロセスのすべてのモジュール（アプリケーション ドメイン、アセンブリの順）。生きたプロセスでは読み込みの知らせで
/// 届くものを、ダンプでは数え上げる。
unsafe fn all_modules(process: &ComPtr<ICorDebugProcess>) -> Vec<ComPtr<ICorDebugModule>> {
    let mut modules = Vec::new();
    let Ok(domains) = take::<ICorDebugAppDomainEnum>("ICorDebugProcess::EnumerateAppDomains", |out| process.EnumerateAppDomains(out)) else { return modules };
    for domain in each::<ICorDebugAppDomain>(|out, fetched| domains.Next(1, out, fetched)) {
        let Ok(assemblies) = take::<ICorDebugAssemblyEnum>("ICorDebugAppDomain::EnumerateAssemblies", |out| domain.EnumerateAssemblies(out)) else { continue };
        for assembly in each::<ICorDebugAssembly>(|out, fetched| assemblies.Next(1, out, fetched)) {
            let Ok(enumerator) = take::<ICorDebugModuleEnum>("ICorDebugAssembly::EnumerateModules", |out| assembly.EnumerateModules(out)) else { continue };
            modules.extend(each::<ICorDebugModule>(|out, fetched| enumerator.Next(1, out, fetched)));
        }
    }
    modules
}

/// マネージドのスレッドの番号（ランタイムが知っている順）。
unsafe fn all_threads(process: &ComPtr<ICorDebugProcess>) -> Vec<u32> {
    let Ok(threads) = take::<ICorDebugThreadEnum>("ICorDebugProcess::EnumerateThreads", |out| process.EnumerateThreads(out)) else { return Vec::new() };
    each::<ICorDebugThread>(|out, fetched| threads.Next(1, out, fetched)).iter().map(thread_id).collect()
}

/// 列挙のオブジェクトから 1 つずつ取り出す（`next` は `Next(1, …)`）。ダンプのほか、切り離しの前の後始末も使う。
pub(super) unsafe fn each<I: Interface>(next: impl Fn(*mut *mut c_void, *mut u32) -> HRESULT) -> Vec<ComPtr<I>> {
    let mut items = Vec::new();
    // 壊れたダンプで回り続けないように、数を限る。
    for _ in 0..100_000 {
        let (mut raw, mut fetched) = (null_mut(), 0u32);
        if next(&mut raw, &mut fetched).0 != 0 || fetched == 0 {
            break;
        }
        match ComPtr::<I>::from_raw(raw) {
            Some(item) => items.push(item),
            None => break,
        }
    }
    items
}
