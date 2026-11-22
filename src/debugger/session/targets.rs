//! Step Into Target（Visual Studio の「特定の呼び出しにステップ イン」。段階 11）: いまの文の中の呼び出しを IL から並べ
//! （`stepInTargets`）、選んだ呼び出しの中でステップを終える（`stepIn` の `targetId`）。拡張の中継は文の字句をソースから
//! 読んでいたが、エンジンは IL の call / callvirt を、実行する順に並べる（逆コンパイルしたソースでも同じ）。
//!
//! 選んだ呼び出しへは、中継と同じく、呼び先の名前に一時的な関数ブレークポイントを置いてステップ オーバーする。呼び先に
//! 着けばそこで止まり（理由は step）、行がその呼び出しをしなければ、次の行で止まる。仮想の呼び出しは実体の型が分からない
//! ので、名前（型を付けない）で、利用者のコード（シンボルのあるモジュールと、逆コンパイルの対象）に置く。

use super::*;

/// 進行中の Step Into Target: 呼び先に置いた一時的なブレークポイント。
pub(super) struct TargetStep {
    pub(super) breakpoints: Vec<ComPtr<ICorDebugFunctionBreakpoint>>,
}

/// `stepInTargets` で渡した候補（番号は添字 + 1）。
#[derive(Debug, Clone)]
pub(super) struct StepTarget {
    /// 一時的なブレークポイントを置く名前（メソッドの名前だけ）。
    method: String,
}

/// 文の中の 1 つの呼び出し。
struct Call {
    /// 見出し（`Order.Save`、プロパティは `Order.Total.get`）。
    label: String,
    /// 一時的なブレークポイントを置く名前（メソッドの名前だけ）。
    method: String,
}

impl Session {
    /// `stepInTargets { frameId }`: フレームのいまの文の中の呼び出し（実行する順）。
    pub fn step_in_targets(&mut self, arguments: &Value) -> Result<Value, String> {
        let (thread, depth) = self.frame_of(arguments)?;
        let frames = self.walk_frames(thread)?;
        let frame = frames.get(depth).ok_or_else(|| tr!("There is no frame."))?;
        self.step_targets.clear();
        // ソースの情報が無いフレームでは、候補は無い。
        let Some((calls, line)) = (unsafe { self.statement_calls(frame) }) else { return Ok(json!({ "targets": [] })) };
        let mut targets = Vec::new();
        for Call { label, method } in calls {
            self.step_targets.push(StepTarget { method });
            let mut target = json!({ "id": self.step_targets.len(), "label": label });
            if let Some(line) = line {
                target["line"] = json!(line);
            }
            targets.push(target);
        }
        Ok(json!({ "targets": targets }))
    }

    /// フレームのいまの文の、いまの位置から先の呼び出しと、文の行。
    unsafe fn statement_calls(&self, frame: &ComPtr<ICorDebugFrame>) -> Option<(Vec<Call>, Option<u32>)> {
        let module = self.frame_module(frame)?;
        let (pdb, metadata) = (module.pdb.as_ref()?, module.metadata()?);
        let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
        let (mut token, mut offset, mut mapping) = (0u32, 0u32, 0);
        frame.GetFunctionToken(&mut token);
        il_frame.GetIP(&mut offset, &mut mapping);
        let il = method_il(frame)?;
        let (start, end) = pdb.step_range(token, offset, frame_version(frame))?;
        let end = end.unwrap_or(il.len() as u32);
        let line = pdb.find_visible_point(token, offset, frame_version(frame)).map(|p| p.start_line);
        // 文の先頭からではなく、いまの場所から（文の途中で止まっていれば、もう呼んだものは除く）。
        let calls = crate::debugger::il::find_calls(&il, start.max(offset), end).into_iter().map(|(_, callee)| target_names(metadata, callee)).collect();
        Some((calls, line))
    }

    /// `stepIn { threadId, targetId }`: 選んだ呼び出しの中に入る。候補に無い番号なら、ふつうのステップ イン。
    pub fn step_into_target(&mut self, arguments: &Value) -> Result<(), String> {
        let target = arguments.get("targetId").and_then(Value::as_u64).and_then(|id| self.step_targets.get(id.checked_sub(1)? as usize)).cloned();
        let Some(target) = target else { return self.step(arguments, StepKind::In) };
        let mut breakpoints = Vec::new();
        let modules = &self.modules;
        self.while_synchronized(|| {
            for module in modules.iter().filter(|m| m.pdb.is_some() || m.decompile != Decompile::NotTarget) {
                let Some(metadata) = module.metadata() else { continue };
                for method in metadata.find_methods(&target.method) {
                    if let Ok(created) = unsafe { break_at(&module.module, method, 0) } {
                        breakpoints.push(created);
                    }
                }
            }
        });
        crate::log!("step into target {}: {} breakpoints", target.method, breakpoints.len());
        // 一時的なブレークポイントを覚えてから、行をステップ オーバーする（`step` の中の cancel_step で消えないように、後で入れる）。
        let result = self.step(arguments, StepKind::Over);
        if result.is_ok() {
            self.target_step = Some(TargetStep { breakpoints });
        } else {
            remove_breakpoints(&breakpoints);
        }
        result
    }

    /// ブレークポイントが、Step Into Target の一時的なものなら、ステップを終えて止まる（理由は step）。
    pub(super) fn target_step_arrived(&mut self, thread: u32, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Option<Outcome> {
        let raw = breakpoint.as_raw();
        let arrived = self.target_step.as_ref()?.breakpoints.iter().any(|b| b.as_raw() == raw);
        if !arrived {
            return None;
        }
        self.cancel_step();
        self.patched_thread = Some(thread);
        Some(Outcome::Stop(("stopped", json!({ "reason": "step", "threadId": thread, "allThreadsStopped": true }))))
    }

    /// Step Into Target の一時的なブレークポイントを外す（着いた、行を越えた、ほかの理由で止まった）。
    pub(super) fn end_target_step(&mut self) {
        if let Some(step) = self.target_step.take() {
            remove_breakpoints(&step.breakpoints);
        }
    }
}

fn remove_breakpoints(breakpoints: &[ComPtr<ICorDebugFunctionBreakpoint>]) {
    for breakpoint in breakpoints {
        unsafe { breakpoint.Activate(0) };
    }
}

/// 呼び出し命令のトークンが指す呼び先。
fn target_names(metadata: &Metadata, callee: u32) -> Call {
    let full = metadata.call_name(callee);
    let (owner, method) = full.rsplit_once('.').map_or((None, full.as_str()), |(owner, method)| (Some(owner), method));
    let label_method = match (method.strip_prefix("get_"), method.strip_prefix("set_")) {
        (Some(property), _) => format!("{property}.get"),
        (_, Some(property)) => format!("{property}.set"),
        _ => method.to_string(),
    };
    let label = owner.map_or_else(|| label_method.clone(), |owner| format!("{owner}.{label_method}"));
    Call { label, method: method.to_string() }
}
