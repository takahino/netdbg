//! 関数評価: 止まっている対象のスレッドで、対象のメソッドを実行する。
//!
//! プロパティの値、`ToString()`、例外の `Message` などは、対象のコードを動かさないと得られない。評価の間は
//! 対象を続行し、終わりのコールバック（EvalComplete / EvalException）を待つ。待っている間に届くほかの出来事
//! （モジュールの読み込みなど）は、ここで受けて続行する。DAP の要求は列に残り、評価が終わってから処理される。
//!
//! 評価は対象の状態を変え得る（副作用のあるプロパティ）。呼ぶのは、利用者が見ようとしたものだけにする。

use super::*;
use crate::events;
use std::time::{Duration, Instant};

/// 評価を待つ時間。越えたら中断を頼む（netcoredbg の既定と同じ 5 秒）。
const EVAL_TIMEOUT: Duration = Duration::from_secs(5);
/// ほかのスレッドを動かしてから、中断を頼んでから、それぞれ待つ時間。
const ABORT_TIMEOUT: Duration = Duration::from_secs(3);
/// 取り消されたとき、ほかのスレッドを動かしてから、中断を頼むまでに待つ時間。
const CANCEL_GRACE: Duration = Duration::from_millis(500);
/// 評価を待つ間に、取り消し（DAP の cancel）が届いていないかを見に行く間隔。
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// ほかのスレッドの状態を変えるために、対象を止めるのを待つ時間（ミリ秒）。
const STOP_TIMEOUT_MS: u32 = 3000;

/// 止まった場所のせいで評価を始められないときの、理由と直し方。それ以外の失敗なら `None`。
/// ICorDebug は、評価を仕込む呼び出し（`CallParameterizedFunction` など）でこれらを返す（CorError.h）。
fn unavailable_reason(hr: HRESULT) -> Option<String> {
    let reason = match hr.0 as u32 {
        // CORDBG_E_ILLEGAL_AT_GC_UNSAFE_POINT: `pause` で止めたスレッドが、ランタイムやネイティブのコード（`Thread.Sleep` の中など）にいる。
        0x8013_1C23 => tr_str!("This thread is not stopped at a GC-safe place (as when stopped with pause), so expressions that run the target's code cannot be evaluated. "),
        // CORDBG_E_ILLEGAL_IN_PROLOG
        0x8013_1C24 => tr_str!("This thread is stopped at a method's entry (its prologue), so expressions that run the target's code cannot be evaluated. "),
        // CORDBG_E_ILLEGAL_IN_NATIVE_CODE
        0x8013_1C25 => tr_str!("This thread is stopped in native code, so expressions that run the target's code cannot be evaluated. "),
        // CORDBG_E_ILLEGAL_IN_OPTIMIZED_CODE
        0x8013_1C26 => tr_str!("This thread is stopped in optimized code, so expressions that run the target's code cannot be evaluated. "),
        // CORDBG_E_ILLEGAL_IN_STACK_OVERFLOW
        0x8013_1C22 => tr_str!("It is stopped with the stack overflowed, so expressions that run the target's code cannot be evaluated. "),
        // CORDBG_E_FUNC_EVAL_BAD_START_POINT: 例外の処理の途中など。
        0x8013_1313 => tr_str!("The target's code cannot run from where this thread stopped (in the middle of exception handling, for example). "),
        // CORDBG_E_CANT_CALL_ON_THIS_THREAD、CORDBG_E_THREAD_NOT_SCHEDULED、CORDBG_E_BAD_THREAD_STATE
        0x8013_1C09 | 0x8013_1C00 | 0x8013_132D => tr_str!("This thread cannot run the target's code right now. "),
        // CORDBG_E_FUNC_EVAL_NOT_COMPLETE: 前の評価が、まだ終わっていない（中断できなかった評価が残っている）。
        0x8013_1315 => return Some(tr!("An earlier evaluation has not finished, so the target's code cannot run. Continue, then try again. ({code})", code = hr)),
        _ => return None,
    };
    Some(tr!("{reason}Stop on a line of your code with a breakpoint or a step, then try again. ({code})", reason = reason, code = hr))
}

/// 評価を仕込む呼び出しの失敗。止まった場所のせいなら `Unavailable`（理由と直し方）。
fn start_failed(what: &str, hr: HRESULT) -> EvalFailure {
    match unavailable_reason(hr) {
        Some(reason) => EvalFailure::Unavailable(reason),
        None => EvalFailure::Message(failed(what, hr)),
    }
}

/// 評価がうまくいかなかった理由。
pub(super) enum EvalFailure {
    /// 評価したコードが例外を投げた。中身は、その例外のオブジェクト。
    Threw(ComPtr<ICorDebugValue>),
    /// 評価を始められなかった、など。利用者に見せる文。
    Message(String),
    /// 時間切れで中断した（中断できなかったときも）。利用者に見せる文。
    TimedOut(String),
    /// 止まった場所のせいで、この停止では対象のコードを実行できない（`pause` で止めたときなど）。理由と直し方。
    Unavailable(String),
}

impl From<String> for EvalFailure {
    fn from(message: String) -> Self {
        EvalFailure::Message(message)
    }
}

impl EvalFailure {
    /// 利用者に見せる文。
    pub(super) fn describe(&self, session: &Session) -> String {
        match self {
            EvalFailure::Threw(exception) => tr!("The evaluated code threw an exception: {type}", type = unsafe { value::describe(exception, &session.modules) }.type_name),
            EvalFailure::Message(message) | EvalFailure::TimedOut(message) | EvalFailure::Unavailable(message) => message.clone(),
        }
    }
}

/// 評価の終わりを待つ段。時間切れのたびに一段進む。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WaitStage {
    /// ふつうに待っている（ほかのスレッドは止めたまま）。
    Waiting,
    /// 止めていたほかのスレッドも動かした。
    OthersRunning,
    /// 中断（Abort）を頼んだ。
    AbortRequested,
    /// 強い中断（RudeAbort）を頼んだ。
    RudeAbortRequested,
}
impl Session {
    /// `thread_id` のスレッドで `callable` を呼び、戻り値を返す（値を返さないメソッドなら `None`）。
    /// 対象が止まっているときだけ呼べる。終わったとき、対象はまた止まっている。
    ///
    /// 値型の戻り値は、ボックス化されて返ってくる。戻り値の型が `object` やインターフェイスでなければ、プリミティブ型の
    /// 値は中身に作り直す（`list.Count` は `int`。ボックスのままだと、`object` として扱ってしまい、計算できない）。
    pub(super) fn call(&mut self, thread_id: u32, callable: &value::Callable, arguments: &[ComPtr<ICorDebugValue>]) -> Result<Option<ComPtr<ICorDebugValue>>, EvalFailure> {
        let Some(result) = self.call_raw(thread_id, callable, arguments)? else { return Ok(None) };
        let declared = unsafe {
            let mut token = 0;
            callable.function.GetToken(&mut token);
            let mut module = null_mut();
            callable.function.GetModule(&mut module);
            ComPtr::<ICorDebugModule>::from_raw(module).and_then(|module| value::Modules::metadata(&self.modules, &module)?.return_kind(token))
        };
        use crate::debugger::metadata::ParameterKind;
        let stays_boxed = matches!(declared, None | Some(ParameterKind::Object | ParameterKind::Reference | ParameterKind::String));
        Ok(Some(if stays_boxed { result } else { self.unboxed(thread_id, result) }))
    }

    fn call_raw(&mut self, thread_id: u32, callable: &value::Callable, arguments: &[ComPtr<ICorDebugValue>]) -> Result<Option<ComPtr<ICorDebugValue>>, EvalFailure> {
        self.run_eval(thread_id, |eval| unsafe {
            let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
            let mut type_arguments: Vec<*mut c_void> = callable.type_arguments.iter().map(|t| t.as_raw()).collect();
            let mut raw_arguments: Vec<*mut c_void> = arguments.iter().map(|a| a.as_raw()).collect();
            eval2
                .CallParameterizedFunction(callable.function.as_raw(), type_arguments.len() as u32, type_arguments.as_mut_ptr(), raw_arguments.len() as u32, raw_arguments.as_mut_ptr())
                .ok()
                .map_err(|hr| start_failed(tr_str!("Calling the method"), hr))
        })
    }

    /// 対象の中に文字列を作る（メソッドに引数として渡すため）。文字列はヒープのオブジェクトなので、作るのにも対象を動かす。
    pub(super) fn new_string(&mut self, thread_id: u32, text: &str) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        self.new_string_of_units(thread_id, &text.encode_utf16().collect::<Vec<u16>>())
    }

    /// UTF-16 の符号単位の並びから文字列を作る（組にならないサロゲートを含む文字列は、Rust の文字列では持てない）。
    /// 途中に 0 があると、そこで切れる（`ICorDebugEval::NewString` が、0 で終わる文字列を受け取るため）。
    pub(super) fn new_string_of_units(&mut self, thread_id: u32, units: &[u16]) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        let mut wide = units.to_vec();
        wide.push(0);
        let created = self.run_eval(thread_id, |eval| unsafe { eval.NewString(wide.as_ptr()).ok().map_err(|hr| start_failed(tr_str!("Creating the string"), hr)) })?;
        created.ok_or_else(|| EvalFailure::Message(tr!("Could not create the string.")))
    }

    /// 対象の中に、型のオブジェクトをコンストラクターを呼ばずに作る（中身は 0）。値型なら、ボックス化されたものになる。
    pub(super) fn new_object_without_constructor(&mut self, thread_id: u32, class: &ComPtr<ICorDebugClass>) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        let created = self.run_eval(thread_id, |eval| unsafe {
            let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
            eval2.NewParameterizedObjectNoConstructor(class.as_raw(), 0, null_mut()).ok().map_err(|hr| start_failed(tr_str!("Creating the object"), hr))
        })?;
        created.ok_or_else(|| EvalFailure::Message(tr!("Could not create the object.")))
    }

    /// 対象の中に、型（型引数つきでもよい）のオブジェクトをコンストラクターを呼ばずに作る。
    pub(super) fn new_object_of_type(&mut self, thread_id: u32, ty: &ComPtr<ICorDebugType>) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        let created = self.run_eval(thread_id, |eval| unsafe {
            let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
            let class = take::<ICorDebugClass>("ICorDebugType::GetClass", |out| ty.GetClass(out))?;
            let mut arguments: Vec<*mut c_void> = value::type_arguments_of(ty).iter().map(|argument| argument.as_raw()).collect();
            eval2.NewParameterizedObjectNoConstructor(class.as_raw(), arguments.len() as u32, arguments.as_mut_ptr()).ok().map_err(|hr| start_failed(tr_str!("Creating the object"), hr))
        })?;
        created.ok_or_else(|| EvalFailure::Message(tr!("Could not create the object.")))
    }

    /// 対象の中で、コンストラクターを実行してオブジェクトを作る。`callable` は、コンストラクターと、型の型引数。
    pub(super) fn new_object(&mut self, thread_id: u32, callable: &value::Callable, arguments: &[ComPtr<ICorDebugValue>]) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        let created = self.run_eval(thread_id, |eval| unsafe {
            let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
            let mut type_arguments: Vec<*mut c_void> = callable.type_arguments.iter().map(|t| t.as_raw()).collect();
            let mut raw_arguments: Vec<*mut c_void> = arguments.iter().map(|a| a.as_raw()).collect();
            eval2
                .NewParameterizedObject(callable.function.as_raw(), type_arguments.len() as u32, type_arguments.as_mut_ptr(), raw_arguments.len() as u32, raw_arguments.as_mut_ptr())
                .ok()
                .map_err(|hr| start_failed(tr_str!("Creating the object"), hr))
        })?;
        created.ok_or_else(|| EvalFailure::Message(tr!("Could not create the object.")))
    }

    /// 対象の中に、配列を作る（要素は、どれも 0 か null）。`dimensions` は、次元ごとの大きさ。
    pub(super) fn new_array(&mut self, thread_id: u32, element: &ComPtr<ICorDebugType>, dimensions: &[u32]) -> Result<ComPtr<ICorDebugValue>, EvalFailure> {
        let created = self.run_eval(thread_id, |eval| unsafe {
            let eval2: ComPtr<ICorDebugEval2> = eval.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugEval2"), hr))?;
            let mut sizes = dimensions.to_vec();
            let mut lower_bounds = vec![0u32; dimensions.len()];
            eval2.NewParameterizedArray(element.as_raw(), sizes.len() as u32, sizes.as_mut_ptr(), lower_bounds.as_mut_ptr()).ok().map_err(|hr| start_failed(tr_str!("Creating the array"), hr))
        })?;
        created.ok_or_else(|| EvalFailure::Message(tr!("Could not create the array.")))
    }

    /// 評価を一つ実行する。`setup` が、何を実行するかを評価のオブジェクトに仕込む。
    fn run_eval(&mut self, thread_id: u32, setup: impl FnOnce(&ComPtr<ICorDebugEval>) -> Result<(), EvalFailure>) -> Result<Option<ComPtr<ICorDebugValue>>, EvalFailure> {
        let message = |text: String| EvalFailure::Message(text);
        if self.is_dump() {
            return Err(message(dumps::read_only()));
        }
        if self.evaluation_refused() {
            return Err(message(observe::refusal()));
        }
        if self.state != State::Stopped {
            return Err(message(tr!("Expressions can be evaluated only while the target is stopped.")));
        }
        let process = self.process.clone().ok_or_else(|| message(tr!("There is no target process.")))?;
        let controller = self.stopped_controller.clone().ok_or_else(|| message(tr!("There is nothing to continue.")))?;
        let eval = unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread_id, out)).map_err(message)?;
            // 止まった場所によっては評価できない（`pause` で止めたスレッドが、ランタイムの中にいるときなど）。
            take::<ICorDebugEval>("ICorDebugThread::CreateEval", |out| thread.CreateEval(out)).map_err(|e| message(tr!("This thread cannot run the target's code right now: {error}", error = e)))?
        };
        setup(&eval)?;

        // 評価の間、ほかのスレッドは動かさない（評価のせいで、ほかのスレッドが先へ進んでしまうのを防ぐ）。
        // `await` の待ちだけは別で、待っている Task を終わらせるのがほかのスレッドなので、はじめから動かしておく（D-034）。
        if self.awaiting == 0 {
            self.set_other_threads(&process, thread_id, THREAD_SUSPEND);
        }
        // 凍結したスレッドで評価するときは、評価の間だけ、そのスレッドを動かす。
        let frozen = self.frozen_threads.contains(&thread_id);
        if frozen {
            self.set_thread_state(&process, thread_id, THREAD_RUN);
        }
        let started = Instant::now();
        let resumed = unsafe { controller.Continue(0) };
        if !resumed.is_ok() {
            self.set_other_threads(&process, thread_id, THREAD_RUN);
            return Err(message(failed(tr_str!("Starting the evaluation"), resumed)));
        }
        let outcome = self.wait_for_eval(&eval, &process, thread_id);
        if self.process.is_some() {
            self.set_other_threads(&process, thread_id, THREAD_RUN);
            if frozen {
                self.set_thread_state(&process, thread_id, THREAD_SUSPEND);
            }
        }
        self.eval_stats.0 += 1;
        self.eval_stats.1 += started.elapsed();
        let threw = outcome?;

        let mut raw = null_mut();
        unsafe { eval.GetResult(&mut raw) };
        let result = unsafe { ComPtr::<ICorDebugValue>::from_raw(raw) };
        match (threw, result) {
            (true, Some(exception)) => Err(EvalFailure::Threw(exception)),
            (true, None) => Err(message(tr!("The evaluated code threw an exception."))),
            (false, result) => Ok(result),
        }
    }

    /// 評価の途中で、止めていたほかのスレッドを動かす。スレッドの状態は、対象が止まっている間にしか変えられないので、
    /// いったん止めて、変えて、続ける。
    fn let_other_threads_run(&self, process: &ComPtr<ICorDebugProcess>, thread_id: u32) {
        unsafe {
            if process.Stop(STOP_TIMEOUT_MS).is_ok() {
                self.set_other_threads(process, thread_id, THREAD_RUN);
                process.Continue(0);
            }
        }
    }

    fn set_thread_state(&self, process: &ComPtr<ICorDebugProcess>, thread_id: u32, state: u32) {
        unsafe {
            let mut raw = null_mut();
            process.GetThread(thread_id, &mut raw);
            if let Some(thread) = ComPtr::<ICorDebugThread>::from_raw(raw) {
                thread.SetDebugState(state);
            }
        }
    }

    /// ほかのスレッドの状態を変える。利用者が凍結したスレッドは、動かさない（止めたまま）。
    fn set_other_threads(&self, process: &ComPtr<ICorDebugProcess>, except: u32, state: u32) {
        for &id in self.threads.iter().filter(|&&id| id != except && !(state == THREAD_RUN && self.frozen_threads.contains(&id))) {
            unsafe {
                let mut raw = null_mut();
                process.GetThread(id, &mut raw);
                if let Some(thread) = ComPtr::<ICorDebugThread>::from_raw(raw) {
                    thread.SetDebugState(state);
                }
            }
        }
    }

    /// 評価の終わりを待つ。`Ok(true)` は「例外で終わった」。待つ間に届いたほかの出来事は、受けて続行する。
    ///
    /// 時間切れになったら、まず、止めていたほかのスレッドも動かして、もう少し待つ。評価したコードが、ほかのスレッドの
    /// 持っているロックを待っている（GC がファイナライザーのスレッドを待つ、なども）と、ほかのスレッドを止めたままでは
    /// 終わらない（Visual Studio が「すべてのスレッドの実行が必要です」と出す場面）。それでも終わらなければ、中断する。
    fn wait_for_eval(&mut self, eval: &ComPtr<ICorDebugEval>, process: &ComPtr<ICorDebugProcess>, thread_id: u32) -> Result<bool, EvalFailure> {
        // 対象のコードを動かしたので、値が変わったかもしれない。
        self.touched_target = true;
        let mut deadline = Instant::now() + EVAL_TIMEOUT;
        let mut stage = WaitStage::Waiting;
        let mut cancelled = false;
        loop {
            // 利用者が評価を取り消したら（DAP の cancel）、すぐに中断を頼む。待つのは短く区切って、取り消しを見に行く。
            if !cancelled && stage < WaitStage::AbortRequested && self.current_request.is_some_and(events::cancel_requested) {
                cancelled = true;
                // 中断はなるべく避ける。ロックを待っている途中のスレッドを中断すると、そのロックを誰も取れなくなることがある
                // （.NET 11 の x86 で起きた）。先にほかのスレッドを動かして少しだけ待ち、それでも終わらなければ中断する
                // （時間切れの段を進めて、次の段で Abort する）。どちらでも、応答は「取り消した」。
                if stage == WaitStage::Waiting {
                    crate::log!("eval cancelled by the client: letting the other threads run");
                    self.let_other_threads_run(process, thread_id);
                    stage = WaitStage::OthersRunning;
                    deadline = Instant::now() + CANCEL_GRACE;
                } else {
                    deadline = Instant::now();
                }
            }
            let slice = deadline.min(Instant::now() + CANCEL_POLL);
            let Some(event) = events::next_debug(slice) else {
                if Instant::now() >= deadline {
                    stage = self.escalate(stage, eval, process, thread_id)?;
                    deadline = Instant::now() + ABORT_TIMEOUT;
                }
                continue;
            };
            match event.kind {
                EventKind::EvalComplete { eval: finished, failed } if finished.as_raw() == eval.as_raw() => {
                    // このコールバックで対象はまた止まっている。続行に使う相手を取り替える。
                    if let Some(controller) = event.controller {
                        self.stopped_controller = Some(controller);
                    }
                    return self.eval_finished(stage, cancelled, failed);
                }
                EventKind::ExitProcess => {
                    let events = self.debug_event(DebugEvent { controller: None, kind: EventKind::ExitProcess });
                    self.deferred.extend(events);
                    return Err(EvalFailure::Message(tr!("The target ended during the evaluation.")));
                }
                // モジュールやスレッドの出入りは、ふだんと同じように記録する（知らせるのは、要求に答えたあと）。
                kind @ (EventKind::LoadModule(_) | EventKind::UnloadModule(_) | EventKind::CreateThread(_) | EventKind::ExitThread(_) | EventKind::CreateAppDomain(_)) => {
                    // debug_event は、止まる出来事でなければ自分で続行する。状態は Running として扱わせる。
                    self.state = State::Running;
                    let events = self.debug_event(DebugEvent { controller: event.controller, kind });
                    self.state = State::Stopped;
                    self.deferred.extend(events);
                }
                // 評価したコードの中のブレークポイントや例外では止まらない。
                _ => {
                    if let Some(controller) = event.controller {
                        unsafe { controller.Continue(0) };
                    }
                }
            }
        }
    }

    /// 時間切れ: 待ち方を一段強める。最後の段でも終わらなければ、待つのをやめる（対象は動いたまま）。
    fn escalate(&mut self, stage: WaitStage, eval: &ComPtr<ICorDebugEval>, process: &ComPtr<ICorDebugProcess>, thread_id: u32) -> Result<WaitStage, EvalFailure> {
        match stage {
            WaitStage::Waiting => {
                crate::log!("eval timed out: letting the other threads run");
                self.let_other_threads_run(process, thread_id);
                Ok(WaitStage::OthersRunning)
            }
            WaitStage::OthersRunning => {
                crate::log!("eval timed out: Abort");
                unsafe { eval.Abort() };
                Ok(WaitStage::AbortRequested)
            }
            WaitStage::AbortRequested => {
                crate::log!("eval did not abort: RudeAbort");
                if let Ok(eval2) = eval.cast::<ICorDebugEval2>() {
                    unsafe { eval2.RudeAbort() };
                }
                Ok(WaitStage::RudeAbortRequested)
            }
            WaitStage::RudeAbortRequested => {
                // 止められなかった。対象は動いたままなので、状態もそれに合わせる。
                self.state = State::Running;
                self.stopped_controller = None;
                self.forget_stop();
                self.deferred.push(("continued", json!({ "threadId": 0, "allThreadsContinued": true })));
                Err(EvalFailure::TimedOut(tr!("The evaluation did not finish and could not be aborted. The target keeps running.")))
            }
        }
    }

    /// 評価が終わった。`failed` は、評価したコードが例外を投げたか。どの段で終わったかで、結果と知らせることが変わる。
    fn eval_finished(&mut self, stage: WaitStage, cancelled: bool, failed: bool) -> Result<bool, EvalFailure> {
        match stage {
            _ if cancelled => Err(EvalFailure::Message("cancelled".into())),
            WaitStage::Waiting => Ok(failed),
            // ほかのスレッドを動かしたら終わった。止めていたスレッドが動いたので、その間に状態が変わったかもしれない
            // ことを、利用者に知らせる（Visual Studio の「すべてのスレッドを実行して評価しました」に当たる）。
            WaitStage::OthersRunning => {
                crate::log!("eval finished after letting the other threads run");
                self.deferred.push((
                    "output",
                    json!({ "category": "important", "output": tr!("The evaluation did not finish in {seconds} seconds, so the other stopped threads were run to finish it. The other threads' state may have changed meanwhile.\n", seconds = EVAL_TIMEOUT.as_secs()) }),
                ));
                Ok(failed)
            }
            _ if self.awaiting > 0 => Err(EvalFailure::TimedOut(tr!(
                "The awaited work did not finish in {seconds} seconds, so waiting stopped. Work that continues on this stopped thread (such as the UI thread) cannot finish during an evaluation.",
                seconds = (EVAL_TIMEOUT + ABORT_TIMEOUT).as_secs()
            ))),
            _ => Err(EvalFailure::TimedOut(tr!("The evaluation did not finish in {seconds} seconds, so it was aborted (it did not finish even with the other threads running).", seconds = EVAL_TIMEOUT.as_secs()))),
        }
    }
    /// 対象の中に `int` の値を作る（メソッドに引数として渡すため）。
    pub(super) fn new_int(&self, thread_id: u32, number: i32) -> Result<ComPtr<ICorDebugValue>, String> {
        const ELEMENT_TYPE_I4: u32 = 0x08;
        self.new_primitive(thread_id, ELEMENT_TYPE_I4, &number.to_le_bytes())
    }

    /// 対象の中に `bool` の値を作る。
    pub(super) fn new_bool(&self, thread_id: u32, flag: bool) -> Result<ComPtr<ICorDebugValue>, String> {
        const ELEMENT_TYPE_BOOLEAN: u32 = 0x02;
        self.new_primitive(thread_id, ELEMENT_TYPE_BOOLEAN, &[flag as u8])
    }

    /// 対象の中に `null` の参照を作る。
    pub(super) fn new_null(&self, thread_id: u32) -> Result<ComPtr<ICorDebugValue>, String> {
        const ELEMENT_TYPE_CLASS: u32 = 0x12;
        self.new_primitive(thread_id, ELEMENT_TYPE_CLASS, &[])
    }

    /// プリミティブ型の値を作り、`bytes`（リトルエンディアン）を書き込む。`bytes` が空なら、作ったまま（0 や null）。
    pub(super) fn new_primitive(&self, thread_id: u32, element_type: u32, bytes: &[u8]) -> Result<ComPtr<ICorDebugValue>, String> {
        let process = self.process.as_ref().ok_or_else(|| tr!("There is no target process."))?;
        unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread_id, out))?;
            let eval = take::<ICorDebugEval>("ICorDebugThread::CreateEval", |out| thread.CreateEval(out))?;
            let value = take::<ICorDebugValue>("ICorDebugEval::CreateValue", |out| eval.CreateValue(element_type, null_mut(), out))?;
            if !bytes.is_empty() {
                let generic: ComPtr<ICorDebugGenericValue> = value.cast().map_err(|hr| failed(tr_str!("Getting ICorDebugGenericValue"), hr))?;
                // SetValue は、型の大きさの分だけ読む。8 バイトの作業領域に写してから渡す。
                let mut buffer = [0u8; 8];
                buffer[..bytes.len()].copy_from_slice(bytes);
                generic.SetValue(buffer.as_mut_ptr() as *mut c_void).ok().map_err(|hr| failed(tr_str!("Writing the value"), hr))?;
            }
            Ok(value)
        }
    }

    /// 値の、引数の無いメソッド（プロパティの `get_X` など）を呼ぶ。メソッドが無ければ `None`。
    pub(super) fn call_parameterless(&mut self, thread_id: u32, target: &ComPtr<ICorDebugValue>, name: &str) -> Option<Result<Option<ComPtr<ICorDebugValue>>, EvalFailure>> {
        let callable = unsafe { value::find_parameterless_method(target, &self.modules, name) }?;
        Some(self.call(thread_id, &callable, std::slice::from_ref(target)))
    }

    /// 値の、文字列を返すプロパティを読む。読めなければ `None`（理由はログに残す）。
    pub(super) fn string_property(&mut self, thread_id: u32, target: &ComPtr<ICorDebugValue>, getter: &str) -> Option<String> {
        match self.call_parameterless(thread_id, target, getter)? {
            Ok(Some(result)) => unsafe { value::as_string(&result) },
            Ok(None) => None,
            Err(failure) => {
                crate::log!("{getter}: {}", failure.describe(self));
                None
            }
        }
    }
}