//! ステップ（next / stepIn / stepOut）。await をまたぐステップ、戻り値の表示、実行する場所の移動（goto）もここ。

use super::*;

impl Session {
    /// `next` / `stepIn` / `stepOut`。終わると `stopped`（reason: step）が届く。
    pub fn step(&mut self, arguments: &Value, kind: StepKind) -> Result<(), String> {
        let thread = arguments.get("threadId").and_then(Value::as_u64).ok_or_else(|| tr!("The step has no threadId."))? as u32;
        if self.state != State::Stopped {
            return Err(tr!("Stepping works only while the target is stopped."));
        }
        // async メソッドからの stepOut: 待っている呼び出し元（の続き）に戻れるよう、対象のコードを動かして準備する
        // （ステッパーを仕掛ける前に。関数評価は対象を動かすので）。
        let (async_out, runtime_async_out) = if kind == StepKind::Out { self.prepare_async_step_out(thread) } else { (None, false) };
        // async メソッドの続き（一度中断したあと）では、スタックの下にあるのは呼び出し元ではない（続きを走らせた仕組みや、
        // 待ちを終わらせた別のメソッド）。ふつうの stepOut は仕掛けず、待っている側の知らせだけを待つ。
        if async_out.is_none() {
            self.start_step(thread, kind)?;
        } else {
            self.cancel_step();
        }
        self.runtime_async_out = runtime_async_out;
        self.async_step_out = async_out;
        self.stepping_in = kind == StepKind::In;
        self.leaving_filtered = false;
        // 前の停止で見せていた戻り値は、ここまで。
        self.forget_return_values();
        self.arm_return_values(thread, kind);
        if kind != StepKind::Out {
            self.arm_awaits(thread);
        }
        let controller = self.stopped_controller.take().ok_or_else(|| tr!("There is nothing to continue."))?;
        unsafe { controller.Continue(0) }.ok().map_err(|hr| failed(tr_str!("Continuing"), hr))?;
        self.state = State::Running;
        self.forget_stop();
        Ok(())
    }

    /// スレッドにステッパーを仕掛ける（対象はまだ動かさない）。
    pub(super) fn start_step(&mut self, thread_id: u32, kind: StepKind) -> Result<(), String> {
        // 利用者の一つのステップの中で進め直すときも、async メソッドからの stepOut の待ち受けと、Step Into Target の
        // 一時的なブレークポイントは続ける。
        let async_out = self.async_step_out.take();
        let target_step = self.target_step.take();
        let watch_step = self.watch_step.take();
        self.cancel_step();
        self.async_step_out = async_out;
        self.target_step = target_step;
        self.watch_step = watch_step;
        // いまいる所が、まだ逆コンパイルしていないモジュールなら、文の範囲を知るために逆コンパイルする（D-060）。
        self.decompile_for_top_frame(thread_id);
        let process = self.process.as_ref().ok_or_else(|| tr!("There is no target process."))?;
        unsafe {
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread_id, out))?;
            let stepper = take::<ICorDebugStepper>("ICorDebugThread::CreateStepper", |out| thread.CreateStepper(out))?;
            // ソースに対応しないコード（プロローグなど）では止まらない。
            stepper.SetUnmappedStopMask(STOP_NONE);
            if let Ok(stepper2) = stepper.cast::<ICorDebugStepper2>() {
                stepper2.SetJMC(self.just_my_code as i32);
            }
            let started = match (kind, self.statement_range(thread_id)) {
                (StepKind::Out, _) => stepper.StepOut(),
                (_, Some(mut range)) => {
                    stepper.SetRangeIL(1);
                    stepper.StepRange((kind == StepKind::In) as i32, &mut range, 1)
                }
                // いまの場所にソースの情報が無い。一命令ずつ進めても意味が無いので、戻るまで進める。
                (_, None) => stepper.StepOut(),
            };
            started.ok().map_err(|hr| failed(tr_str!("Starting the step"), hr))?;
            self.stepper = Some(stepper);
        }
        Ok(())
    }

    /// スレッドのいちばん上のフレームで、いまいる文の IL の範囲。ソースの情報が無ければ `None`。
    pub(super) fn statement_range(&self, thread_id: u32) -> Option<COR_DEBUG_STEP_RANGE> {
        let frames = self.walk_frames(thread_id).ok()?;
        let frame = frames.first()?;
        unsafe {
            let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
            let pdb = self.frame_module(frame)?.pdb.as_ref()?;
            let (mut token, mut offset, mut mapping) = (0u32, 0u32, 0);
            frame.GetFunctionToken(&mut token);
            il_frame.GetIP(&mut offset, &mut mapping);
            let (start, end) = pdb.step_range(token, offset, frame_version(frame))?;
            let end = match end {
                Some(end) => end,
                // 最後の文は、メソッドの IL の終わりまで。
                None => {
                    let mut function = null_mut();
                    frame.GetFunction(&mut function);
                    let function = ComPtr::<ICorDebugFunction>::from_raw(function)?;
                    let mut code = null_mut();
                    function.GetILCode(&mut code);
                    let mut size = 0u32;
                    ComPtr::<ICorDebugCode>::from_raw(code)?.GetSize(&mut size);
                    size
                }
            };
            Some(COR_DEBUG_STEP_RANGE { startOffset: start, endOffset: end })
        }
    }

    /// ステップが終わった場所で止まってよいかを決める。止まれない場所なら、次のステップを仕掛けて `false`
    /// （呼び出し側が続行する）。止まれる場所は、ソースのある行の上。
    pub(super) fn settle_step(&mut self, thread_id: u32, reason: u32) -> bool {
        // プロパティや演算子から抜けた所。もとのステップインを、文の途中から続ける。
        if std::mem::take(&mut self.leaving_filtered) && self.start_step(thread_id, StepKind::In).is_ok() {
            return false;
        }
        // ステップインで、プロパティや演算子の中に入った。中では止まらず、いったん抜ける。
        const STEP_CALL: u32 = 2;
        if self.stepping_in && self.step_filtering && reason == STEP_CALL && self.top_is_property_or_operator(thread_id) && self.start_step(thread_id, StepKind::Out).is_ok() {
            self.leaving_filtered = true;
            return false;
        }
        // ステップで入った先が、PDB の無いアプリのモジュールなら、ここで逆コンパイルして止まれるようにする（D-060）。
        self.decompile_for_top_frame(thread_id);
        let stoppable = (|| {
            let frames = self.walk_frames(thread_id).ok()?;
            let frame = frames.first()?;
            unsafe {
                let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
                let pdb = self.frame_module(frame)?.pdb.as_ref()?;
                let (mut token, mut offset, mut mapping) = (0u32, 0u32, 0);
                frame.GetFunctionToken(&mut token);
                il_frame.GetIP(&mut offset, &mut mapping);
                Some(pdb.find_point(token, offset, frame_version(frame)).is_some_and(|p| !p.is_hidden()))
            }
        })();
        // async メソッドからの stepOut で、待っていた側に戻った所。await の続き（GetResult のあと）はコンパイラが作った
        // 隠し行だが、netcoredbg と同じく、ここで止まる（コールスタックでは await の行に見える）。
        let after_async_out = std::mem::take(&mut self.landing_after_async_out);
        let next = match stoppable {
            Some(true) => return true,
            Some(false) if after_async_out => return true,
            // 隠し行（コンパイラが作ったコード）の上。見える行に出るまで進める。
            Some(false) => StepKind::Over,
            // シンボルの無いコードに入った。自分のコードに戻るまで進める。
            None => StepKind::Out,
        };
        crate::log!("step landed where it cannot stop (reason {reason}): continue with {next:?}");
        match self.start_step(thread_id, next) {
            Ok(()) => {
                // 進め直した先の文にも await があり得る。
                if next != StepKind::Out {
                    self.arm_awaits(thread_id);
                }
                false
            }
            Err(e) => {
                // 仕掛けられなければ、ここで止まる（動かし続けるより、止まった方が利用者が状況を見られる）。
                crate::log!("could not continue the step: {e}");
                true
            }
        }
    }

    /// スレッドのいちばん上のフレームが、プロパティのアクセサーか演算子か。
    pub(super) fn top_is_property_or_operator(&self, thread_id: u32) -> bool {
        (|| {
            let frames = self.walk_frames(thread_id).ok()?;
            let frame = frames.first()?;
            let mut token = 0u32;
            unsafe { frame.GetFunctionToken(&mut token) };
            Some(unsafe { self.frame_module(frame) }?.metadata()?.is_property_or_operator(token & 0x00FF_FFFF))
        })()
        .unwrap_or(false)
    }

    /// 進行中のステップをやめる（ブレークポイントなど、ほかの理由で止まったとき）。
    pub(super) fn cancel_step(&mut self) {
        if let Some(stepper) = self.stepper.take() {
            unsafe { stepper.Deactivate() };
        }
        if let Some(breakpoint) = self.async_step_out.take() {
            unsafe { breakpoint.Activate(0) };
        }
        self.landing_after_async_out = false;
        self.runtime_async_out = false;
        self.remove_return_value_breakpoints();
        self.remove_await_breakpoints();
        self.end_target_step();
        self.watch_step = None;
    }

    /// async メソッドの中からの stepOut の準備（netcoredbg と同じやり方）。メソッドの Task に「待ちが終わったら
    /// デバッガーに知らせる」印を付け（ビルダーの `SetNotificationForWaitCompletion(true)`）、知らせる所
    /// （`Task.NotifyDebuggerOfWaitCompletion`。待っている側の `GetResult` が呼ぶ）にブレークポイントを置く。
    /// そこに当たったら、そこから stepOut すれば、待っている呼び出し元（`await` の行か、`GetResult()` の行）に着く。
    /// async メソッドでない、まだ一度も中断していない（呼び出し元に、ふつうの stepOut で戻れる）、準備できない、ときは
    /// `None`（ふつうの stepOut だけになる）。
    fn prepare_async_step_out(&mut self, thread: u32) -> (Option<ComPtr<ICorDebugFunctionBreakpoint>>, bool) {
        // いちばん上のフレームが、await を持つ async メソッドの本体（状態機械の MoveNext）で、続きの中か。
        // 最初の区間（中断する前）は、すぐ下が `AsyncMethodBuilderCore.Start` で、その下が呼び出し元。
        // PDB に async の情報が無いメソッドは、runtime-async かもしれない（続きの鎖の次のフレームに置く）。
        enum Which {
            /// PDB に await が無い。runtime-async なら続きの鎖へ置く。
            Runtime,
            /// 状態機械の続き。
            Machine,
            /// ふつうの stepOut（async でない、まだ一度も中断していない）。
            Plain,
        }
        let which = (|| {
            let frames = self.walk_frames(thread).ok()?;
            let frame = frames.first()?;
            let mut method = 0u32;
            unsafe { frame.GetFunctionToken(&mut method) };
            let awaits = unsafe { self.frame_module(frame) }.and_then(|module| module.pdb.as_ref()).map(|pdb| !pdb.awaits(method, frame_version(frame)).is_empty());
            if !awaits.unwrap_or(false) {
                return Some(Which::Runtime);
            }
            let caller = frames.get(1).and_then(|caller| {
                let mut token = 0u32;
                unsafe { caller.GetFunctionToken(&mut token) };
                unsafe { self.frame_module(caller) }?.metadata()?.method_name(token)
            });
            Some(if caller.is_some_and(|name| name.ends_with("AsyncMethodBuilderCore.Start")) { Which::Plain } else { Which::Machine })
        })()
        .unwrap_or(Which::Plain);
        if matches!(which, Which::Runtime) {
            return match self.runtime_async_step_out_breakpoint(thread) {
                Some(breakpoint) => (Some(breakpoint), true),
                None => (None, false),
            };
        }
        if matches!(which, Which::Plain) {
            return (None, false);
        }
        let Some(this) = self.top_this(thread) else { return (None, false) };
        let Some(builder) = (unsafe { value::field(&this, &self.modules, "<>t__builder") }) else { return (None, false) };
        if let Err(e) = self.evaluate_on(thread, &builder, "SetNotificationForWaitCompletion(true)") {
            crate::log!("async step out: SetNotificationForWaitCompletion: {e}");
            return (None, false);
        }
        // Task は System.Private.CoreLib（.NET Framework では mscorlib）にある。
        let found = self.modules.iter().find_map(|module| {
            let metadata = module.metadata()?;
            let task = metadata.find_type("System.Threading.Tasks.Task")?;
            let method = metadata.method_rows_named(task, "NotifyDebuggerOfWaitCompletion").into_iter().next()?;
            Some((module.module.clone(), (crate::debugger::metadata::METHOD_DEF as u32) << 24 | method))
        });
        let Some((module, method)) = found else {
            crate::log!("async step out: Task.NotifyDebuggerOfWaitCompletion not found");
            return (None, false);
        };
        (unsafe { break_at(&module, method, 0) }.map_err(|e| crate::log!("async step out: {e}")).ok(), false)
    }

    /// async メソッドからの stepOut で、待っている側が待ち終えた（`Task.NotifyDebuggerOfWaitCompletion` に当たった）。
    /// そこから stepOut して、待っている呼び出し元に着いたら止まる。
    pub(super) fn async_step_out_reached(&mut self, thread: u32) {
        self.cancel_step();
        match self.start_step(thread, StepKind::Out) {
            Ok(()) => self.landing_after_async_out = true,
            Err(e) => crate::log!("async step out: {e}"),
        }
    }

    pub(super) fn remove_await_breakpoints(&mut self) {
        for armed in self.await_breakpoints.drain(..) {
            unsafe { armed.breakpoint.Activate(0) };
        }
    }

    /// async メソッドの中でステップするとき、いまの文にある `await` の「中断する位置」にブレークポイントを置く。
    ///
    /// 待つものがまだ終わっていないと、メソッドはそこでいったん呼び出し元へ戻り、あとで（別のスレッドかもしれない）
    /// 続きが走る。ふつうのステップは「戻った」所で終わってしまうので、中断を捕まえて、続きの側でステップをやり直す。
    /// 待つものがもう終わっていれば、中断する位置は通らず、ふつうのステップのまま次の行に着く。
    pub(super) fn arm_awaits(&mut self, thread: u32) {
        let armed = (|| {
            let range = self.statement_range(thread)?;
            let frames = self.walk_frames(thread).ok()?;
            let frame = frames.first()?;
            let module = unsafe { self.frame_module(frame) }?;
            let (mut method, mut offset, mut mapping) = (0u32, 0u32, 0);
            unsafe {
                frame.GetFunctionToken(&mut method);
                frame.cast::<ICorDebugILFrame>().ok()?.GetIP(&mut offset, &mut mapping);
            }
            let version = frame_version(frame);
            let mut points = module.pdb.as_ref()?.awaits(method, version).to_vec();
            // runtime-async は状態機械を作らず、PDB に AsyncMethodSteppingInformation が無い。
            // 中断は `AsyncHelpers.Await` の呼び出し、再開はその次の命令（どちらも利用者のメソッド自身）。
            if points.is_empty() {
                let il = unsafe { method_il(frame) }?;
                points = runtime_async_awaits(&il, module.metadata()?);
            }
            let mut armed = Vec::new();
            for point in &points {
                // いまの文の、これから通る await だけ。
                if point.yield_offset < offset.max(range.startOffset) || point.yield_offset >= range.endOffset {
                    continue;
                }
                if let Ok(breakpoint) = unsafe { break_at(&module.module, method, point.yield_offset) } {
                    armed.push(AwaitBreakpoint { breakpoint, thread, module: module.module.clone(), method, resume_offset: Some(point.resume_offset), state_machine: None });
                }
            }
            Some(armed)
        })();
        self.await_breakpoints = armed.unwrap_or_default();
    }

    /// await の内部のブレークポイントに当たった。
    pub(super) fn await_reached(&mut self, thread: u32, breakpoint: &ComPtr<ICorDebugBreakpoint>) {
        let Some(index) = self.await_breakpoints.iter().position(|b| b.breakpoint.as_raw() == breakpoint.as_raw()) else { return };
        match self.await_breakpoints[index].resume_offset {
            // 中断する所。ステップしているスレッドなら、いまのステップをやめて、続きが走り出す所で待ち受ける。
            // どの呼び出しの続きかは、状態機械のオブジェクトで見分ける。
            Some(resume_offset) => {
                if self.await_breakpoints[index].thread != thread {
                    return;
                }
                let (module, method) = (self.await_breakpoints[index].module.clone(), self.await_breakpoints[index].method);
                let state_machine = self.state_machine_handle(thread);
                self.cancel_step();
                match unsafe { break_at(&module, method, resume_offset) } {
                    Ok(breakpoint) => self.await_breakpoints.push(AwaitBreakpoint { breakpoint, thread, module, method, resume_offset: None, state_machine }),
                    Err(e) => crate::log!("async step: {e}"),
                }
            }
            // 同じメソッドの、別の呼び出しの続き。何もせずに走らせる（待っている呼び出しの続きを待ち続ける）。
            None if self.continues_another_call(index, thread) => {
                crate::log!("async step: the continuation of another call of the same method (thread {thread}): keep waiting");
            }
            // 続きが走り出した所（await の文の途中）。ここから、次の行までステップする。
            None => {
                self.remove_await_breakpoints();
                match self.start_step(thread, StepKind::Over) {
                    // 同じ文に、まだ await が残っていることがある（`await A(await B())`）。
                    Ok(()) => self.arm_awaits(thread),
                    Err(e) => crate::log!("async step: {e}"),
                }
            }
        }
    }

    /// スレッドのいちばん上のフレームの `this`（async メソッドの本体なら、状態機械）。
    pub(super) fn top_this(&self, thread: u32) -> Option<ComPtr<ICorDebugValue>> {
        let frames = self.walk_frames(thread).ok()?;
        let il_frame = frames.first()?.cast::<ICorDebugILFrame>().ok()?;
        let mut raw = null_mut();
        unsafe { il_frame.GetArgument(0, &mut raw) };
        unsafe { ComPtr::from_raw(raw) }
    }

    /// 待っている呼び出しの状態機械への弱いハンドル。状態機械がクラス（デバッグのビルド）のときだけ作れる。
    fn state_machine_handle(&self, thread: u32) -> Option<ComPtr<ICorDebugHandleValue>> {
        let this = self.top_this(thread)?;
        let reference = this.cast::<ICorDebugReferenceValue>().ok()?;
        let object = unsafe { take::<ICorDebugValue>("ICorDebugReferenceValue::Dereference", |out| reference.Dereference(out)) }.ok()?;
        let heap: ComPtr<ICorDebugHeapValue2> = object.cast().ok()?;
        unsafe { take::<ICorDebugHandleValue>("ICorDebugHeapValue2::CreateHandle", |out| heap.CreateHandle(HANDLE_WEAK_TRACK_RESURRECTION, out)) }.ok()
    }

    /// 再開する位置のブレークポイント（`await_breakpoints[index]`）に当たったのが、待っている呼び出しとは別の呼び出しの続きか。
    /// 見分けられないとき（状態機械が構造体、番地が読めない）は、別とはみなさない。
    fn continues_another_call(&self, index: usize, thread: u32) -> bool {
        let Some(expected) = self.await_breakpoints[index].state_machine.as_ref().and_then(|handle| handle.cast::<ICorDebugValue>().ok()) else { return false };
        let expected = unsafe { inspect::object_address(&expected) };
        let current = self.top_this(thread).map_or(0, |this| unsafe { inspect::object_address(&this) });
        expected != 0 && current != 0 && expected != current
    }

    /// 戻り値を拾う準備をする。`step` が、ステッパーを仕掛けたあと、対象を動かす前に呼ぶ。
    /// うまくいかなくても、戻り値が出ないだけで、ステップには影響しない。
    pub(super) fn arm_return_values(&mut self, thread: u32, kind: StepKind) {
        if kind == StepKind::Out {
            self.stepping_out = Some(thread);
            return;
        }
        let armed = (|| {
            let range = self.statement_range(thread)?;
            let frames = self.walk_frames(thread).ok()?;
            let frame = frames.first()?;
            let metadata = unsafe { self.frame_module(frame) }?.metadata()?;
            let mut armed = Vec::new();
            unsafe {
                // 戻り値が残っている場所は、IL ではなく機械語の位置で分かる。ブレークポイントも機械語のコードに置く。
                let native_frame = frame.cast::<ICorDebugNativeFrame>().ok()?;
                let mut raw = null_mut();
                native_frame.GetCode(&mut raw);
                let native_code = ComPtr::<ICorDebugCode>::from_raw(raw)?;
                let native_code3 = native_code.cast::<ICorDebugCode3>().ok()?;
                let (mut frame_start, mut frame_end) = (0u64, 0u64);
                frame.GetStackRange(&mut frame_start, &mut frame_end).ok().ok()?;
                let il = method_il(frame)?;
                for (call_offset, token) in crate::debugger::il::find_calls(&il, range.startOffset, range.endOffset) {
                    let mut offsets = [0u32; 8];
                    let mut fetched = 0u32;
                    // 値を返さないメソッドでは失敗する。見せるものが無い。
                    if !native_code3.GetReturnValueLiveOffset(call_offset, offsets.len() as u32, &mut fetched, offsets.as_mut_ptr()).is_ok() {
                        continue;
                    }
                    for &offset in &offsets[..(fetched as usize).min(offsets.len())] {
                        let mut raw = null_mut();
                        native_code.CreateBreakpoint(offset, &mut raw);
                        if let Some(breakpoint) = ComPtr::<ICorDebugFunctionBreakpoint>::from_raw(raw) {
                            breakpoint.Activate(1);
                            armed.push(ReturnValueBreakpoint { breakpoint, call_offset, name: metadata.call_name(token), thread, frame_start });
                        }
                    }
                }
            }
            Some(armed)
        })();
        self.return_value_breakpoints = armed.unwrap_or_default();
    }

    pub(super) fn remove_return_value_breakpoints(&mut self) {
        for armed in self.return_value_breakpoints.drain(..) {
            unsafe { armed.breakpoint.Activate(0) };
        }
        self.stepping_out = None;
    }

    /// 内部のブレークポイントに当たった。ステップしているフレームでの当たりなら、戻り値を読んで覚える。
    pub(super) fn capture_return_value(&mut self, thread: u32, breakpoint: &ComPtr<ICorDebugBreakpoint>) {
        let Some(armed) = self.return_value_breakpoints.iter().find(|b| b.breakpoint.as_raw() == breakpoint.as_raw()) else { return };
        if armed.thread != thread {
            return;
        }
        let (call_offset, name, frame_start) = (armed.call_offset, armed.name.clone(), armed.frame_start);
        let Some(frame) = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().next()) else { return };
        unsafe {
            let (mut start, mut end) = (0u64, 0u64);
            frame.GetStackRange(&mut start, &mut end);
            if start != frame_start {
                return;
            }
            let Ok(il_frame3) = frame.cast::<ICorDebugILFrame3>() else { return };
            let mut raw = null_mut();
            il_frame3.GetReturnValueForILOffset(call_offset, &mut raw);
            if let Some(value) = ComPtr::<ICorDebugValue>::from_raw(raw) {
                self.remember_return_value(thread, name, &value);
            }
        }
    }

    /// `stepOut` で呼び出し元に戻った直後。どの呼び出しから戻ったかは分からないので、メソッドの中の呼び出しを
    /// 順に試す（ランタイムは、いまの場所が「戻った直後」になっている呼び出しにだけ値を返す）。
    pub(super) fn read_return_value_after_step_out(&mut self, thread: u32) {
        let Some(frame) = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().next()) else { return };
        unsafe {
            let Ok(il_frame3) = frame.cast::<ICorDebugILFrame3>() else { return };
            let Some(il) = method_il(&frame) else { return };
            for (call_offset, token) in crate::debugger::il::find_calls(&il, 0, il.len() as u32) {
                let mut raw = null_mut();
                if !il_frame3.GetReturnValueForILOffset(call_offset, &mut raw).is_ok() {
                    continue;
                }
                if let Some(value) = ComPtr::<ICorDebugValue>::from_raw(raw) {
                    let name = self.frame_module(&frame).and_then(|m| m.metadata()).map_or("?".into(), |m| m.call_name(token));
                    self.remember_return_value(thread, name, &value);
                    return;
                }
            }
        }
    }

    pub(super) unsafe fn remember_return_value(&mut self, thread: u32, name: String, value: &ComPtr<ICorDebugValue>) {
        let described = value::describe(value, &self.modules);
        let handle = if described.expandable { value::strong_handle(value) } else { None };
        self.return_values.push(ReturnValue { thread, name, value: described.value, type_name: described.type_name, handle });
    }

    pub(super) fn forget_return_values(&mut self) {
        for value in self.return_values.drain(..) {
            if let Some(handle) = value.handle {
                unsafe { value::dispose_handle(&handle) };
            }
        }
    }

    /// `gotoTargets`: 「次に実行する行」の候補を返す。行が実際に移せる場所かは、止まっているスレッドが分かる `goto` で確かめる。
    pub fn goto_targets(&mut self, arguments: &Value) -> Result<Value, String> {
        let path = arguments.pointer("/source/path").and_then(Value::as_str).ok_or_else(|| tr!("gotoTargets has no source.path."))?;
        let line = arguments.get("line").and_then(Value::as_u64).ok_or_else(|| tr!("gotoTargets has no line."))? as u32;
        self.goto_targets.push((path.to_string(), line));
        Ok(json!({ "targets": [{ "id": self.goto_targets.len(), "label": format!("line {line}"), "line": line }] }))
    }

    /// `goto`: スレッドのいちばん上のフレームの、次に実行する場所を動かす。対象は止まったまま。
    /// 止まった場所が変わったことの通知（`stopped`、reason: goto）は、拡張の側が出す（netcoredbg のときと同じ分担）。
    pub fn goto(&mut self, arguments: &Value) -> Result<(), String> {
        let thread = arguments.get("threadId").and_then(Value::as_u64).ok_or_else(|| tr!("goto has no threadId."))? as u32;
        let target = arguments.get("targetId").and_then(Value::as_u64).ok_or_else(|| tr!("goto has no targetId."))? as usize;
        let (path, line) = target.checked_sub(1).and_then(|index| self.goto_targets.get(index)).cloned().ok_or_else(|| tr!("That targetId was not given by gotoTargets."))?;
        if self.state != State::Stopped {
            return Err(tr!("The execution point can be moved only while the target is stopped."));
        }
        let frames = self.walk_frames(thread)?;
        let frame = frames.first().ok_or_else(|| tr!("This thread has no frame to move."))?;
        unsafe {
            let il_frame = frame.cast::<ICorDebugILFrame>().map_err(|hr| failed(tr_str!("Getting ICorDebugILFrame"), hr))?;
            let mut method = 0u32;
            frame.GetFunctionToken(&mut method);
            let pdb = self.frame_module(frame).and_then(|m| m.pdb.as_ref()).ok_or_else(|| tr!("The current method has no symbols, so it cannot be moved to a line."))?;
            let location = pdb
                .find_document(&path)
                .and_then(|document| pdb.resolve_line_in(document, line, method, frame_version(frame)))
                .ok_or_else(|| tr!("Line {line} is not in the method running now. It can be moved only within the same method.", line = line))?;
            // catch / finally の中など、ランタイムが安全に移せない場所は断られる。
            il_frame.CanSetIP(location.il_offset).ok().map_err(|hr| tr!("Cannot move to line {line} ({code}).", line = location.line, code = hr))?;
            il_frame.SetIP(location.il_offset).ok().map_err(|hr| failed(tr_str!("Moving the execution point"), hr))?;
        }
        // フレームと変数の番号は、動かす前の場所のもの。
        self.frames.clear();
        self.handles.clear();
        Ok(())
    }
}

/// runtime-async の中断と再開。`AsyncHelpers.Await` の呼び出しが中断で、その次の命令が再開。
/// 状態機械の PDB（`AsyncMethodSteppingInformation`）が付いているメソッドは、こちらを使わない。
pub(super) fn runtime_async_awaits(il: &[u8], metadata: &crate::debugger::metadata::Metadata) -> Vec<crate::debugger::pdb::Await> {
    crate::debugger::il::find_calls(il, 0, il.len() as u32)
        .into_iter()
        .filter(|(_, token)| metadata.call_name(*token) == "AsyncHelpers.Await")
        .map(|(offset, _)| crate::debugger::pdb::Await {
            yield_offset: offset,
            resume_offset: crate::debugger::il::decode(il, offset).map(|instruction| instruction.end()).unwrap_or(offset.saturating_add(5)),
        })
        .collect()
}
