//! 本体の要求の口。IF の層（`crate::interface` の DAP、MCP）は、要求（コマンドと引数。語彙は DAP のもの）をここに
//! 渡し、答え（`Reply`）を受け取る。答えをどの相手にどう書くかは、IF の層が決める。
//!
//! 本体のスレッドは 1 本（入力のループ）。対象の出来事（`crate::events` の `Debug` など）も、ここで本体に渡す。

use super::session::{Event, Owner, Session, StepKind};
use crate::events::DebugEvent;
use crate::com::{ComPtr, IUnknown, HRESULT};
use serde_json::{json, Value};

/// 要求や出来事を処理したあと、入力のループを続けるかどうか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// 対象の終了を待ってから終わる（`disconnect` で対象を止めたとき）。
    ExitAfterProcess,
    Exit,
}

/// 要求への答え。
pub struct Reply {
    /// 成功なら本文（無ければ null）、失敗なら利用者に見せる理由。
    pub body: Result<Value, String>,
    /// 答えのあとに伝える出来事（`initialized`、`continued`、止まったこと、処理の途中で起きたこと）。
    pub events: Vec<Event>,
    /// 対象の値を変え得る操作が、対象に触れた（変数の欄やウォッチを取り直す目安。DAP の `invalidated`）。
    pub changed_values: bool,
    pub flow: Flow,
}

impl Reply {
    pub fn new(body: Result<Value, String>) -> Reply {
        Reply { body, events: Vec::new(), changed_values: false, flow: Flow::Continue }
    }
}

pub struct Engine {
    session: Session,
}

impl Engine {
    pub fn new() -> Engine {
        Engine { session: Session::new() }
    }

    /// 対象が生きているか（起動・アタッチしてから、終わるまで）。
    pub fn is_live(&self) -> bool {
        self.session.is_live()
    }

    /// 試験しているプログラム（起動したものか、アタッチした先の exe）。成績書が使う。
    pub fn program(&self) -> Option<String> {
        self.session.program_path()
    }

    /// 対象のプロセスの番号（動いているときだけ）。
    pub fn target_process_id(&self) -> Option<u32> {
        self.session.target_process_id().filter(|_| self.is_live())
    }

    /// 対象が止まっているか（止まっていると、画面の名前と文字は読めない）。
    pub fn is_stopped(&self) -> bool {
        self.session.is_stopped()
    }

    /// 要求を 1 つ処理する。`seq` は取り消し（`cancel`）の照合に、`owner` はブレークポイントを置いた側に使う。
    pub fn request(&mut self, seq: i64, command: &str, arguments: &Value, owner: Owner) -> Reply {
        // 評価の途中で `cancel` が届いたら、その評価を中断する（session が入力の列を覗く）。
        self.session.current_request = Some(seq);
        self.session.touched_target = false;
        self.session.eval_stats = Default::default();
        let started = std::time::Instant::now();
        let mut reply = self.dispatch(command, arguments, owner);
        // 遅さの調べ物のため: 要求ごとの時間と、そのうちの関数評価（対象のコードの実行）の回数と時間。
        let (evals, eval_time) = self.session.eval_stats;
        crate::log!("{command}: {:.1} ms, {evals} evals ({:.1} ms)", started.elapsed().as_secs_f64() * 1000.0, eval_time.as_secs_f64() * 1000.0);
        reply.changed_values = reply.body.is_ok() && self.session.touched_target && changes_values(command, arguments);
        // 要求の処理の途中（関数評価の間）に起きた出来事は、答えのあとに伝える。
        reply.events.extend(self.session.take_deferred());
        reply
    }

    fn dispatch(&mut self, command: &str, arguments: &Value, owner: Owner) -> Reply {
        let session = &mut self.session;
        // クラッシュダンプは動かせない（読むだけ）。
        if let Some(refused) = session.refuse_for_dump(command) {
            return Reply::new(Err(refused));
        }
        // 観察専用のアタッチは、データを変えない（D-089）。
        if let Some(refused) = session.refuse_while_observing(command) {
            return Reply::new(Err(refused));
        }
        let body: Result<Value, String> = match command {
            // 取り消しは、取り消す相手の処理の中で行う（ここに来るのは、その相手が終わったあと）。
            "cancel" => Ok(Value::Null),
            "initialize" => {
                // 子プロセスを追うときに、子のセッションを頼めるか（children.rs）。
                if owner == Owner::Client {
                    // 表示の言語は、クライアントの言語に合わせる（VS Code は自分の表示言語を送る。D-084）。
                    crate::i18n::from_client(arguments.get("locale").and_then(Value::as_str));
                    session.set_client_starts_debugging(arguments.get("supportsStartDebuggingRequest").and_then(Value::as_bool) == Some(true));
                }
                // 相手はこのイベントを待ってからブレークポイントを送ってくる。
                let mut reply = Reply::new(Ok(capabilities()));
                reply.events.push(("initialized", Value::Null));
                return reply;
            }
            "launch" => session.launch(arguments).map(|()| Value::Null),
            "attach" => session.attach(arguments).map(|()| Value::Null),
            "configurationDone" => session.configuration_done().map(|()| Value::Null),
            "setBreakpoints" => session.set_breakpoints(owner, arguments),
            "gotoTargets" => session.goto_targets(arguments),
            "goto" => session.goto(arguments).map(|()| Value::Null),
            "setFunctionBreakpoints" => session.set_function_breakpoints(owner, arguments),
            "setExceptionBreakpoints" => session.set_exception_breakpoints(arguments),
            "exceptionInfo" => session.exception_info(arguments),
            "threads" => session.threads(),
            "stackTrace" => session.stack_trace(arguments),
            "scopes" => session.scopes(arguments),
            // 変数の木のプロパティと式は、利用者が頼んだ評価（観察専用では、その中の関数の評価を断る）。
            "variables" => session.as_user_evaluation(|s| s.variables(arguments)),
            "evaluate" => session.as_user_evaluation(|s| s.evaluate(arguments)),
            "setVariable" => session.set_variable(arguments),
            // 書き込みの見張り（D-093）。
            "dataBreakpointInfo" => session.data_breakpoint_info(arguments),
            "setDataBreakpoints" => session.set_data_breakpoints(arguments),
            "setExpression" => session.set_expression(arguments),
            // 拡張（VSCodedotnetX86Debugger）の独自の要求（段階 5）。
            "netdbg.makeObjectId" => session.make_object_id(arguments),
            "netdbg.freezeThread" => session.freeze_thread(arguments),
            "netdbg.heapStats" => session.heap_statistics(),
            "netdbg.handleStats" => session.handle_statistics(arguments),
            // Edit and Continue（D-059）: 書き換えたソースの差分を当てる。
            "netdbg.applyChanges" => session.apply_changes(arguments),
            // PDB の無いモジュールを逆コンパイルして、ソースのファイルの一覧を返す（D-060）。
            "netdbg.decompile" => session.decompile_request(arguments),
            // PDB が知っているソースのファイル（止まる前にソースを開いて、ブレークポイントを置くため。段階 13）。
            "loadedSources" => session.loaded_sources(),
            // 止めずに読む記録（段階 11、D-063）: SQL ログの切り替えと、出来事の列。
            "netdbg.setSqlLog" => session.set_sql_log(arguments),
            "netdbg.setExceptionLog" => session.set_exception_log(arguments),
            "netdbg.setEventTrace" => session.set_event_trace(arguments),
            "netdbg.journal" => session.read_journal(arguments),
            "continue" => {
                let mut reply = Reply::new(session.resume());
                if reply.body.is_ok() {
                    // netcoredbg と同じく、答えのあとに continued も伝える。
                    reply.events.push(("continued", json!({ "threadId": arguments.get("threadId").cloned().unwrap_or(json!(0)), "allThreadsContinued": true })));
                }
                return reply;
            }
            "next" => session.step(arguments, StepKind::Over).map(|()| Value::Null),
            // `targetId` があれば、選んだ呼び出しの中に入る（Step Into Target。段階 11）。
            "stepIn" if arguments.get("targetId").is_some() => session.step_into_target(arguments).map(|()| Value::Null),
            "stepIn" => session.step(arguments, StepKind::In).map(|()| Value::Null),
            "stepInTargets" => session.step_in_targets(arguments),
            // 値の変化で止める（書き込みの行は拡張が探して渡す。段階 11）。
            "netdbg.setValueWatches" => session.set_value_watches(arguments),
            // UI のフリーズの監視と、プロセスの使用量（段階 11）。
            "netdbg.setUiFreeze" => session.set_ui_freeze(arguments),
            // SQL で止める（段階 12）。
            "netdbg.setSqlBreakpoints" => session.set_sql_breakpoints(arguments),
            // オブジェクトを保持している経路（段階 12）。
            "netdbg.retentionPath" => session.retention_path(arguments),
            // ダイアログを出した箇所（段階 12）。
            "netdbg.setDialogLog" => session.set_dialog_log(arguments),
            // 開いているトランザクション（段階 12）。
            "netdbg.transactions" => session.open_transactions(),
            "netdbg.processUsage" => session.process_usage(),
            // 行と分岐のカバレッジ（B1、D-079）。
            "netdbg.setCoverage" => session.set_coverage(arguments),
            "netdbg.coverage" => session.coverage_request(arguments),
            // 差し替わった exe の差分と、再試験の範囲（B2、D-080）。
            "netdbg.diffAssemblies" => session.diff_assemblies(arguments),
            "stepOut" => session.step(arguments, StepKind::Out).map(|()| Value::Null),
            "pause" => match session.pause() {
                // 答えを先に返してから、止まったことを伝える。
                Ok(events) => return Reply { events, ..Reply::new(Ok(Value::Null)) },
                Err(message) => Err(message),
            },
            // ダンプには終わらせる対象が無い。閉じて、終わったと伝える。
            "terminate" if session.is_dump() => {
                let _ = session.detach();
                return Reply { events: vec![("terminated", Value::Null)], ..Reply::new(Ok(Value::Null)) };
            }
            "terminate" => {
                // 終了の知らせ（exited / terminated）は、対象が実際に終わったときに届く。
                session.terminate();
                Ok(Value::Null)
            }
            "disconnect" => {
                // 指定が無ければ、起動した対象は終わらせ、アタッチした対象は切り離す（DAP の既定）。
                let terminate = arguments.get("terminateDebuggee").and_then(Value::as_bool).unwrap_or(!session.is_attached());
                let mut reply = Reply::new(Ok(Value::Null));
                reply.flow = Flow::Exit;
                if !terminate {
                    // 切り離せなかったら、利用者に知らせる（対象が止まったまま残っているかもしれない）。
                    if let Err(message) = session.detach() {
                        reply.body = Err(message);
                    }
                } else if session.is_live() {
                    // 答えは、対象が終わってから（IF の層が保留する）。
                    session.terminate();
                    reply.flow = Flow::ExitAfterProcess;
                }
                return reply;
            }
            other => Err(tr!("This request is not supported yet: {command}", command = other)),
        };
        Reply::new(body)
    }

    /// 要求を送ってくる相手がいなくなった。起動した対象は残さない。アタッチした対象は、切り離して動かし続ける。
    pub fn client_closed(&mut self) -> Flow {
        if self.session.is_attached() {
            if let Err(message) = self.session.detach() {
                crate::log!("{message}");
            }
            return Flow::Exit;
        }
        if self.session.is_live() {
            self.session.terminate();
            return Flow::ExitAfterProcess;
        }
        Flow::Exit
    }

    /// dbgshim の起動通知（ランタイムが読み込まれた）。
    pub fn runtime_started(&mut self, result: Result<ComPtr<IUnknown>, HRESULT>) -> Vec<Event> {
        self.session.runtime_started(result)
    }

    /// 起動した対象のプロセスが終わった（ランタイムを読み込まないまま終わったかもしれない）。
    pub fn target_exited(&mut self, process_id: u32, exit_code: u32) -> Vec<Event> {
        self.session.exited_before_runtime(process_id, exit_code)
    }

    /// ICorDebug のコールバックで届いた出来事。
    pub fn debug_event(&mut self, event: DebugEvent) -> Vec<Event> {
        // 出来事を処理している間、対象は止まっている。UI のフリーズとして数えない。
        self.session.hold_freeze_watch();
        self.session.debug_event(event)
    }

    /// UI のフリーズの監視が、答えない UI スレッドを知らせてきた。
    pub fn ui_frozen(&mut self, process_id: u32, thread: u32, ms: u32, generation: u64) -> Vec<Event> {
        self.session.ui_frozen(process_id, thread, ms, generation)
    }

    /// カバレッジの印が変わった（時のスレッドから）。
    pub fn coverage_tick(&mut self) -> Vec<Event> {
        self.session.coverage_tick()
    }

    /// 入力を 1 つ処理し終えた: 対象が走っているかを、UI のフリーズの監視に伝える。
    pub fn settle(&mut self) {
        self.session.sync_freeze_watch();
    }

    /// 対象の標準出力・標準エラー。
    pub fn output(&mut self, category: &'static str, text: String) -> Vec<Event> {
        vec![("output", json!({ "category": category, "output": text }))]
    }
}

/// 値を変え得る要求か。変数の欄やウォッチ、ホバーの評価は除く（取り直しのたびにまた評価して、`invalidated` が
/// 止まらなくなる）。
fn changes_values(command: &str, arguments: &Value) -> bool {
    match command {
        "setVariable" | "setExpression" | "netdbg.applyChanges" => true,
        "evaluate" => !matches!(arguments.get("context").and_then(Value::as_str), Some("watch" | "hover" | "variables")),
        _ => false,
    }
}

/// `initialize` の答え。実装した機能だけを true にしていく（docs/DECISIONS.md D-013）。
fn capabilities() -> Value {
    json!({
        "supportsConfigurationDoneRequest": true,
        "supportsFunctionBreakpoints": true,
        "supportsConditionalBreakpoints": true,
        "supportsEvaluateForHovers": true,
        "supportsSetVariable": true,
        "supportsSetExpression": true,
        "supportsExceptionInfoRequest": true,
        "supportsExceptionFilterOptions": true,
        "supportsGotoTargetsRequest": true,
        // ログポイントとヒットカウント（段階 11。拡張の中継から移した。hits.rs）。
        "supportsLogPoints": true,
        // 書き込みの見張り（フィールドのデータ ブレークポイント。.NET だけ。D-093）。
        "supportsDataBreakpoints": true,
        // Step Into Target（`stepInTargets` と `stepIn` の `targetId`。段階 11）。
        "supportsStepInTargetsRequest": true,
        "supportsHitConditionalBreakpoints": true,
        // 独自: Edit and Continue（`netdbg.applyChanges`。D-059）。
        "netdbgSupportsApplyChanges": true,
        // 独自: PDB の無いモジュールの逆コンパイル（`netdbg.decompile`。D-060）。
        "netdbgSupportsDecompile": true,
        // 独自: 止めずに読む記録（`netdbg.setSqlLog`、`netdbg.setExceptionLog`、`netdbg.journal`、`netdbg.sqlExecuted`、
        // `netdbg.exceptionThrown`。D-063）。
        "netdbgSupportsJournal": true,
        "netdbgSupportsValueWatches": true,
        "netdbgSupportsUiFreeze": true,
        "netdbgSupportsProcessUsage": true,
        "netdbgSupportsSqlBreakpoints": true,
        // WinForms のデータバインディングの失敗を、出来事の列（種類 binding）と netdbg.bindingFailed で渡す（段階 12）。
        "netdbgSupportsBindingLog": true,
        "netdbgSupportsRetentionPath": true,
        "netdbgSupportsDialogLog": true,
        "netdbgSupportsTransactions": true,
        // 独自: クラッシュダンプを開く（launch の dumpPath。読むだけ。段階 12）。
        "netdbgSupportsDumps": true,
        "supportTerminateDebuggee": true,
        "supportsTerminateRequest": true,
            "supportsLoadedSourcesRequest": true,
        "supportsCancelRequest": true,
        // ラベルは netcoredbg と同じ生の名前。拡張が、フィルターの id を見て利用者向けの文言に差し替える。
        "exceptionBreakpointFilters": [{ "filter": "all", "label": "all" }, { "filter": "user-unhandled", "label": "user-unhandled" }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_answers_with_capabilities_then_initialized() {
        let reply = Engine::new().request(1, "initialize", &Value::Null, Owner::Client);
        assert!(reply.body.unwrap().is_object());
        assert_eq!(reply.events[0].0, "initialized");
    }

    #[test]
    fn an_unknown_request_fails_without_ending_the_session() {
        let reply = Engine::new().request(1, "noSuchRequest", &Value::Null, Owner::Client);
        assert!(reply.body.unwrap_err().contains("noSuchRequest"));
        assert_eq!(reply.flow, Flow::Continue);
    }

    #[test]
    fn disconnect_without_a_debuggee_ends_the_session() {
        let reply = Engine::new().request(1, "disconnect", &Value::Null, Owner::Client);
        assert!(reply.body.is_ok());
        assert_eq!(reply.flow, Flow::Exit);
    }

    #[test]
    fn launch_without_a_program_says_what_to_set() {
        assert!(Engine::new().request(1, "launch", &json!({}), Owner::Client).body.unwrap_err().contains("program"));
    }

    #[test]
    fn launch_of_a_missing_file_fails_before_anything_starts() {
        assert!(Engine::new().request(1, "launch", &json!({ "program": "C:\\no\\such\\app.exe" }), Owner::Client).body.unwrap_err().contains("app.exe"));
    }

    #[test]
    fn breakpoints_set_before_launch_are_pending() {
        let body = Engine::new().request(1, "setBreakpoints", &json!({ "source": { "path": "C:\\src\\a.cs" }, "breakpoints": [{ "line": 3 }, { "line": 9 }] }), Owner::Client).body.unwrap();
        let breakpoints = body["breakpoints"].as_array().unwrap();
        assert_eq!(breakpoints.len(), 2);
        assert_eq!(breakpoints[0]["verified"], false);
        assert_eq!(breakpoints[1]["line"], 9);
        assert_ne!(breakpoints[0]["id"], breakpoints[1]["id"]);
    }

    #[test]
    fn target_output_becomes_an_output_event() {
        let events = Engine::new().output("stdout", "hello\n".into());
        assert_eq!(events, vec![("output", json!({ "category": "stdout", "output": "hello\n" }))]);
    }

    #[test]
    fn a_closed_client_without_a_debuggee_ends_the_session() {
        assert_eq!(Engine::new().client_closed(), Flow::Exit);
    }
}
