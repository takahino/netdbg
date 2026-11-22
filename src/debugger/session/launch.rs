//! 対象の起動、アタッチ、切り離し、終了。

use super::*;

impl Session {
    /// `launch` と `attach` に共通の、デバッグの仕方の設定。名前は、C# 拡張（vsdbg）の launch.json と同じ。
    pub(super) fn read_debug_options(&mut self, arguments: &Value) -> Result<(), String> {
        self.just_my_code = arguments.get("justMyCode").and_then(Value::as_bool).unwrap_or(true);
        self.step_filtering = arguments.get("enableStepFiltering").and_then(Value::as_bool).unwrap_or(true);
        // 変数の欄で、プロパティの getter などを自動では実行しない（副作用のある getter への備え。D-043）。既定は実行する（netcoredbg と同じ）。
        self.implicit_evaluation_allowed =
            arguments.get("expressionEvaluationOptions").and_then(|options| options.get("allowImplicitFuncEval")).and_then(Value::as_bool).unwrap_or(true);
        // PDB の無いアプリのモジュールを、エンジンの中で逆コンパイルする（D-060）。`off` 以外は有効（既定は `app`）。
        self.decompile.enabled = arguments.get("decompile").and_then(Value::as_str) != Some("off");
        // 止めずに読む記録（SQL ログ。段階 11）。新しい対象なので、前の記録は捨てる。
        self.probes.new_target();
        self.hit_state.new_target();
        self.environment_seq = None;
        self.read_child_options(arguments);
        self.read_ui_freeze_options(arguments);
        self.read_probe_options(arguments);
        self.read_coverage_options(arguments)
    }

    /// `launch`: 引数を確かめて覚える。対象を起動するのは `configurationDone`（ブレークポイントが揃ってから）。
    pub fn launch(&mut self, arguments: &Value) -> Result<(), String> {
        // クラッシュダンプを開く（dumps.rs）。
        if let Some(path) = arguments.get("dumpPath").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            return self.launch_dump(path, arguments);
        }
        let program = arguments.get("program").and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or_else(|| tr!("launch has no program. Set the path of the executable in launch.json."))?;
        let executable = target::check_launch(Path::new(program))?;
        self.read_debug_options(arguments)?;
        // Edit and Continue（D-059）。既定は有効（Debug ビルドのモジュールだけが、その形になる）。
        self.enc.enabled = arguments.get("editAndContinue").and_then(Value::as_bool).unwrap_or(true);
        let mut args: Vec<String> = match arguments.get("args") {
            Some(Value::Array(items)) => items.iter().map(|a| a.as_str().map(str::to_string).unwrap_or_else(|| a.to_string())).collect(),
            _ => Vec::new(),
        };
        // `program` が dll なら、.NET のアプリの本体。`dotnet <dll> 引数…` として起動する（netcoredbg と同じ）。
        let is_dll = Path::new(program).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dll"));
        let host = if is_dll {
            args.insert(0, program.to_string());
            Some(target::dotnet_host()?)
        } else {
            None
        };
        // CLR ヘッダーを持つ exe は .NET Framework のアプリ（.NET の exe はネイティブのホストで、本体は dll にある）。
        let framework = !is_dll && (executable.managed || arguments.get("__dotnetFramework").and_then(Value::as_bool) == Some(true));
        let mut extra = Vec::new();
        if let Some(Value::Object(env)) = arguments.get("env") {
            for (name, value) in env {
                // null は「その変数を消す」。文字列以外は JSON の表記のまま渡す。
                extra.push((name.clone(), match value {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                }));
            }
        }
        if !framework {
            self.dbgshim = Some(Dbgshim::load()?);
        } else if executable.managed {
            // .NET Framework 2.0〜3.5 用のアプリは、起動まで待たずに、ここで断る。
            framework::check_program(Path::new(program))?;
        }
        self.decompile.set_program(program);
        self.config = Some(LaunchConfig {
            program: program.to_string(),
            framework,
            command_line: win32::command_line(host.as_deref().unwrap_or(program), &args),
            environment: extra,
            cwd: arguments.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string),
            stop_at_entry: arguments.get("stopAtEntry").and_then(Value::as_bool).unwrap_or(false),
        });
        Ok(())
    }

    /// `attach`: 相手を確かめて覚える。つなぐのは `configurationDone`（`launch` と同じ流れにする）。
    pub fn attach(&mut self, arguments: &Value) -> Result<(), String> {
        // processId は数値でも文字列でも来る（VS Code の `${command:pickProcess}` は文字列を返す）。
        let process_id = match arguments.get("processId") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
        .and_then(|id| u32::try_from(id).ok())
        .ok_or_else(|| tr!("attach has no processId. Set the target's process ID in launch.json."))?;
        target::check_attach(process_id)?;
        // 親のエンジンが止めて渡した子プロセス（children.rs）。まだランタイムが無いので、起動を待って（.NET）、または
        // 読み込まれるのを待って（.NET Framework）つなぐ。子を動かすのは、つなぐ準備ができてから。
        self.resume_child = arguments.get("__resumeChild").and_then(Value::as_bool) == Some(true);
        // ランタイムを持たないプロセス（ネイティブ）につなぐと、読み込まれるまで待ち続けてしまう。先に断る。
        if !self.resume_child && !framework::has_runtime(process_id) {
            return Err(tr!("Process {pid} has no .NET runtime (coreclr.dll / clr.dll) loaded. Choose the process of a .NET app.", pid = process_id));
        }
        self.read_debug_options(arguments)?;
        // 観察専用（D-089）: データを変えない。プロパティも開いたときに実行しない（クリックで評価する形。評価は断られる）。
        self.observe_only = arguments.get("observeOnly").and_then(Value::as_bool) == Some(true);
        if self.observe_only {
            self.implicit_evaluation_allowed = false;
        }
        self.attach_framework = if self.resume_child { child_is_framework(process_id) } else { framework::is_loaded_in(process_id) };
        if !self.attach_framework {
            self.dbgshim = Some(Dbgshim::load()?);
        }
        self.attach_to = Some(process_id);
        // アタッチでは、プロセスの exe のフォルダーをアプリのフォルダーとする（dotnet app.dll なら dotnet のフォルダーになり、
        // アプリのモジュールは対象にならない。.NET の部品は名前で除く）。
        if let Some(image) = target::process_image_path(process_id) {
            self.decompile.set_program(&image);
        }
        Ok(())
    }

    /// `attach` でつないだ（つなごうとしている）セッションか。切断のとき、対象を終わらせずに切り離す。
    pub fn is_attached(&self) -> bool {
        // ダンプも、終わらせる対象が無いので、切り離す側で閉じる。
        self.attach_to.is_some() || self.is_dump()
    }

    /// `configurationDone`: 対象を起動する、または対象につなぐ。できたら `process` の通知を送る（netcoredbg と同じ形と
    /// 順番。拡張は、この `systemProcessId` で、メモリのグラフと UI のフリーズの見張りを始める）。
    pub fn configuration_done(&mut self) -> Result<(), String> {
        if self.is_dump() {
            return self.open_dump();
        }
        if self.launched.is_some() {
            return Ok(());
        }
        self.start_target()?;
        let process_id = self.launched.as_ref().map(|launched| launched.process_id).unwrap_or_default();
        let (name, start_method) = match &self.config {
            Some(config) if self.attach_to.is_none() => (config.program.clone(), "launch"),
            _ => (target::process_image_path(process_id).unwrap_or_default(), "attach"),
        };
        let mut body = json!({ "name": name, "systemProcessId": process_id, "isLocalProcess": true, "startMethod": start_method });
        // 観察専用なら印を付ける（MCP の画面の入力と、専用の画面が見る。D-089）。
        if self.observe_only {
            body["netdbgObserveOnly"] = json!(true);
        }
        self.deferred.push(("process", body));
        Ok(())
    }

    fn start_target(&mut self) -> Result<(), String> {
        if let Some(process_id) = self.attach_to {
            crate::log!("attaching to pid {process_id} (framework: {})", self.attach_framework);
            if self.attach_framework {
                // 止めてある .NET Framework の子: ランタイムが読み込まれる前にはつなげないので、動かして待つ（その間の
                // コードは止められない）。
                if std::mem::take(&mut self.resume_child) {
                    win32::suspend_process(process_id, false);
                    wait_for_framework(process_id)?;
                }
                let cordebug = Self::framework_debugger(None)?;
                self.process = Some(unsafe { take::<ICorDebugProcess>("ICorDebug::DebugActiveProcess", |out| cordebug.DebugActiveProcess(process_id, 0, out)) }?);
                self.cordebug = Some(cordebug);
                self.launched = Some(Launched::without_registration(process_id));
            } else {
                let dbgshim = self.dbgshim.as_ref().ok_or_else(|| tr!("dbgshim is not loaded."))?;
                self.launched = Some(dbgshim.attach(process_id)?);
                // 止めてある .NET の子: ランタイムの起動を待つ登録ができたので、動かす（最初のコードから止められる）。
                if std::mem::take(&mut self.resume_child) {
                    win32::suspend_process(process_id, false);
                }
            }
            self.state = State::Running;
            return Ok(());
        }
        let Some(config) = self.config.as_ref() else {
            return Err(tr!("launch or attach is needed before configurationDone."));
        };
        if config.framework {
            let cordebug = Self::framework_debugger(Some(Path::new(&config.program)))?;
            let (process, process_id) = win32::with_environment(&config.environment, || framework::launch(&cordebug, &config.command_line, config.cwd.as_deref()))?;
            crate::log!("launched pid {process_id} (.NET Framework): {}", config.command_line);
            self.process = Some(process);
            self.cordebug = Some(cordebug);
            self.launched = Some(Launched::without_registration(process_id));
            self.seeking_entry = config.stop_at_entry;
            self.state = State::Running;
            return Ok(());
        }
        let dbgshim = self.dbgshim.as_ref().ok_or_else(|| tr!("dbgshim is not loaded."))?;
        let launched = dbgshim.launch(&config.command_line, &config.environment, config.cwd.as_deref())?;
        crate::log!("launched pid {}: {}", launched.process_id, config.command_line);
        self.launched = Some(launched);
        self.seeking_entry = config.stop_at_entry;
        self.state = State::Running;
        Ok(())
    }

    /// .NET Framework 用の ICorDebug を作り、コールバックを受け取れる状態にする。
    pub(super) fn framework_debugger(program: Option<&Path>) -> Result<ComPtr<ICorDebug>, String> {
        let cordebug = framework::create_debugger(program)?;
        unsafe {
            cordebug.Initialize().ok().map_err(|hr| failed("ICorDebug::Initialize", hr))?;
            cordebug.SetManagedHandler(managed_callback()).ok().map_err(|hr| failed("ICorDebug::SetManagedHandler", hr))?;
        }
        Ok(cordebug)
    }

    /// dbgshim の起動通知。ここで ICorDebug を初期化して対象につなぐ。
    pub fn runtime_started(&mut self, result: Result<ComPtr<IUnknown>, HRESULT>) -> Vec<Event> {
        let attached = (|| -> Result<(), String> {
            let unknown = result.map_err(|hr| failed(tr_str!("Detecting the runtime start"), hr))?;
            let Some(launched) = self.launched.as_mut() else { return Err(tr!("A notification for a target that was not launched")) };
            if let Some(dbgshim) = self.dbgshim.as_ref() {
                dbgshim.unregister(launched);
            }
            let cordebug: ComPtr<ICorDebug> = unknown.cast().map_err(|hr| failed(tr_str!("Getting ICorDebug"), hr))?;
            unsafe {
                cordebug.Initialize().ok().map_err(|hr| failed("ICorDebug::Initialize", hr))?;
                cordebug.SetManagedHandler(managed_callback()).ok().map_err(|hr| failed("ICorDebug::SetManagedHandler", hr))?;
                let process_id = launched.process_id;
                let process = take::<ICorDebugProcess>("ICorDebug::DebugActiveProcess", |out| cordebug.DebugActiveProcess(process_id, 0, out))?;
                self.process = Some(process);
            }
            self.cordebug = Some(cordebug);
            Ok(())
        })();
        match attached {
            Ok(()) => Vec::new(),
            Err(message) => {
                crate::log!("attach failed: {message}");
                self.state = State::Exited;
                vec![("output", json!({ "category": "stderr", "output": tr!("Cannot start debugging: {message}\n", message = message) })), ("terminated", Value::Null)]
            }
        }
    }

    /// 起動した対象のプロセスが終わった。ランタイムが読み込まれていれば、終わりは ICorDebug（ExitProcess）が知らせる
    /// ので何もしない。読み込まれないまま終わった（ネイティブの exe など）なら、理由と終了コードを知らせて終える。
    pub fn exited_before_runtime(&mut self, process_id: u32, exit_code: u32) -> Vec<Event> {
        let waiting = self.process.is_none() && self.state == State::Running && self.launched.as_ref().is_some_and(|launched| launched.process_id == process_id);
        if !waiting {
            return Vec::new();
        }
        if let (Some(dbgshim), Some(launched)) = (self.dbgshim.as_ref(), self.launched.as_mut()) {
            dbgshim.unregister(launched);
        }
        self.state = State::Exited;
        crate::log!("pid {process_id} exited without loading the runtime: {exit_code}");
        vec![
            ("output", json!({ "category": "stderr", "output": tr!("The target ended without loading the .NET runtime (exit code {code}). Set a .NET app (exe or dll) as program.\n", code = exit_code as i32) })),
            ("exited", json!({ "exitCode": exit_code as i32 })),
            ("terminated", Value::Null),
        ]
    }

    /// 対象を切り離す（デバッグをやめ、対象は動かし続ける）。切り離せなかったら、その理由（対象は止まったままかも
    /// しれない）。
    pub fn detach(&mut self) -> Result<(), String> {
        if self.is_dump() {
            self.close_dump();
            return Ok(());
        }
        // ランタイムの読み込みを待っている途中なら、待つのをやめるだけ。
        if let (Some(dbgshim), Some(launched)) = (self.dbgshim.as_ref(), self.launched.as_mut()) {
            dbgshim.unregister(launched);
        }
        let Some(process) = self.process.clone() else { return Ok(()) };
        // カバレッジは、切り離す前に書き出す（知らせは要求の答えのあと）。
        let written = self.finish_coverage();
        self.deferred.extend(written);
        // 書き込みの見張りは、切り離す前に外す（D-093）。
        self.clear_data_watch();
        let detached = unsafe {
            // 切り離しは、対象を止めた状態でないとできない。置いたブレークポイントは残さない。
            if self.state == State::Running {
                process.Stop(0);
            }
            deactivate_all(&process);
            // ブレークポイント（やステップ）で止まっているスレッドは、再開の途中でデバッガーの手助けを要る。止めたまま
            // 切り離すと、x64 の対象が 0xC0000409 で落ちることがある（12 回中 7 回）。そのスレッドを 1 命令だけ進め、
            // 進み終えた（StepComplete）のを待ってから切り離す（docs/DECISIONS.md D-019 の追記）。
            if let Some(controller) = self.stopped_controller.take() {
                let settled = self.patched_thread.take().is_some_and(|thread| self.step_off_patch(&process, &controller, thread));
                if !settled {
                    // 確かめられなかったときは、これまでどおり、続行して少し待ってから止め直す。
                    controller.Continue(0);
                    std::thread::sleep(DETACH_SETTLE_TIME);
                    process.Stop(0);
                }
            }
            let hr = process.Detach();
            crate::log!("ICorDebugProcess::Detach: {hr}");
            hr
        };
        self.forget_process();
        if detached.0 != 0 {
            return Err(tr!("Could not detach from the target ({code}). The target may still be stopped, or may have ended.", code = detached));
        }
        Ok(())
    }

    /// 切り離す前に、ブレークポイントの場所で止まっているスレッドを 1 命令だけ進める。進み終えて対象が止まったら true。
    /// 待つ間に届いたほかの出来事（モジュールの読み込みなど）は、そのまま続行する。
    unsafe fn step_off_patch(&mut self, process: &ComPtr<ICorDebugProcess>, controller: &ComPtr<ICorDebugController>, stopped: u32) -> bool {
        const WAIT: std::time::Duration = std::time::Duration::from_secs(2);
        let Ok(thread) = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(stopped, out)) else { return false };
        let Ok(stepper) = take::<ICorDebugStepper>("ICorDebugThread::CreateStepper", |out| thread.CreateStepper(out)) else { return false };
        stepper.SetUnmappedStopMask(STOP_NONE);
        if !stepper.Step(0).is_ok() || !controller.Continue(0).is_ok() {
            return false;
        }
        let deadline = std::time::Instant::now() + WAIT;
        while let Some(event) = crate::events::next_debug(deadline) {
            match event.kind {
                EventKind::StepComplete { thread, .. } if thread_id(&thread) == stopped => return true,
                EventKind::ExitProcess => return false,
                _ => {
                    if let Some(other) = event.controller {
                        other.Continue(0);
                    }
                }
            }
        }
        crate::log!("detach: the step off the breakpoint did not finish in time");
        stepper.Deactivate();
        process.Stop(0);
        true
    }

    /// 対象を終わらせる。終了の通知（ExitProcess）はあとから届く。
    pub fn terminate(&mut self) {
        let Some(process) = self.process.as_ref() else { return };
        unsafe {
            if self.state == State::Running {
                process.Stop(0);
            }
            let hr = process.Terminate(0);
            crate::log!("ICorDebugProcess::Terminate: {hr}");
        }
        self.stopped_controller = None;
        self.state = State::Running;
    }
}

/// 子プロセスの exe が .NET Framework のものか（CLR ヘッダーを持つ）。.NET の exe はネイティブのホスト。
fn child_is_framework(process_id: u32) -> bool {
    target::process_image_path(process_id).and_then(|image| std::fs::read(image).ok()).and_then(|data| target::inspect(&data)).is_some_and(|e| e.managed)
}

/// .NET Framework の子で、clr.dll が読み込まれるのを待つ（10 秒まで）。
fn wait_for_framework(process_id: u32) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if framework::is_loaded_in(process_id) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(tr!("The .NET Framework runtime was not loaded into the child process {pid} (waited 10 seconds).", pid = process_id))
}

/// 対象に置いたブレークポイントとステッパーを、すべて外す。利用者のものだけでなく、止めずに読む入口（段階 11、12 の
/// プローブ）や、ステップの内部のもの（戻り値、await）も。一つでも残っていると、切り離しは
/// CORDBG_E_DETACH_FAILED_OUTSTANDING_BREAKPOINTS（0x80131C21）で失敗し、対象は止まったまま残る。種類ごとの一覧を
/// たどらず、ランタイムに数え上げてもらう（種類が増えても漏れない）。
unsafe fn deactivate_all(process: &ComPtr<ICorDebugProcess>) {
    let Ok(domains) = take::<ICorDebugAppDomainEnum>("ICorDebugProcess::EnumerateAppDomains", |out| process.EnumerateAppDomains(out)) else { return };
    for domain in dumps::each::<ICorDebugAppDomain>(|out, fetched| domains.Next(1, out, fetched)) {
        if let Ok(breakpoints) = take::<ICorDebugBreakpointEnum>("ICorDebugAppDomain::EnumerateBreakpoints", |out| domain.EnumerateBreakpoints(out)) {
            for breakpoint in dumps::each::<ICorDebugBreakpoint>(|out, fetched| breakpoints.Next(1, out, fetched)) {
                breakpoint.Activate(0);
            }
        }
        if let Ok(steppers) = take::<ICorDebugStepperEnum>("ICorDebugAppDomain::EnumerateSteppers", |out| domain.EnumerateSteppers(out)) {
            for stepper in dumps::each::<ICorDebugStepper>(|out, fetched| steppers.Next(1, out, fetched)) {
                stepper.Deactivate();
            }
        }
    }
}