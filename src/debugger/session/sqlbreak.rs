//! SQL で止める（段階 12）と、SQL の実行時間。どちらも SQL ログ（probes.rs）の入口の上に載る。
//!
//! 止める: SQL ログの入口で読んだ文が、どれかの条件（`sqllog::SqlCondition`）に合えば、記録したうえで止まる（理由は
//! `breakpoint`、`description` に文の頭）。止まる場所は、プロバイダーのメソッドの入口（まだ実行していない）。合わない
//! ものは記録して続ける。
//!
//! 時間: 止まらない文は、そのスレッドに計時用のステッパー（stepOut。Just My Code は使わない）を仕掛け、呼び出しが
//! 戻った（例外で抜けたなら、捕まえる所に着いた）ときに、記録に `durationMs` を足して `netdbg.sqlCompleted` を送る。
//! 利用者のステップとは別のステッパーなので、ステップの状態には触らない。時間には、その間のデバッガーの処理（中の
//! プローブなど）も入る。止まった文は、止まっていた時間が混ざるので計らない。

use super::*;
use crate::debugger::sqllog::{self, Command, SqlCondition};
use std::time::Instant;

/// `stopped` の説明に入れる、文の頭の文字数。
const SUMMARY_LENGTH: usize = 80;

#[derive(Default)]
pub(super) struct SqlBreakState {
    /// 番号と条件。
    breakpoints: Vec<(i64, SqlCondition)>,
    /// 計っている文。
    timers: Vec<Timer>,
}

struct Timer {
    stepper: ComPtr<ICorDebugStepper>,
    /// 出来事の列の番号（SQL ログの記録）。
    seq: u64,
    started: Instant,
}

impl SqlBreakState {
    /// 対象が終わった: 計っていたものは捨てる（条件は残す）。
    pub(super) fn forget_target(&mut self) {
        self.timers.clear();
    }
}

impl Session {
    /// `netdbg.setSqlBreakpoints { breakpoints: [{ text?, parameter?, value? }] }`: SQL で止める条件を、送られてきた
    /// 一覧で置き換える。条件があるのに SQL ログが切れていれば、入れる（条件は SQL ログの入口で判定する）。
    pub fn set_sql_breakpoints(&mut self, arguments: &Value) -> Result<Value, String> {
        let items = arguments.get("breakpoints").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut answers = Vec::new();
        self.sql_break.breakpoints.clear();
        for item in &items {
            match SqlCondition::from_json(item) {
                Some(condition) => {
                    let id = self.next_breakpoint_id;
                    self.next_breakpoint_id += 1;
                    answers.push(json!({ "id": id, "verified": true, "condition": condition.to_json() }));
                    self.sql_break.breakpoints.push((id, condition));
                }
                None => answers.push(json!({ "verified": false, "message": "Give text, parameter or value: an empty SQL breakpoint would stop at every statement." })),
            }
        }
        if !self.sql_break.breakpoints.is_empty() && !self.sql_log_on() {
            self.configure_sql_log(true, None, None);
        }
        Ok(json!({ "breakpoints": answers, "sqlLog": self.sql_log_on() }))
    }

    /// 記録する文が、どれかの条件に合うか。合えば、記録に印を付け、止まる通知を用意する（`take_sql_stop`）。
    pub(super) fn sql_breakpoints_hit(&mut self, thread: u32, command: &Command, sql: &str, body: &mut Value) -> bool {
        let hit: Vec<i64> = self.sql_break.breakpoints.iter().filter(|(_, c)| c.matches(command, sql)).map(|(id, _)| *id).collect();
        if hit.is_empty() {
            return false;
        }
        body["stopped"] = json!(true);
        body["breakpointIds"] = json!(hit);
        let mut summary: String = sqllog::normalize(sql).chars().take(SUMMARY_LENGTH).collect();
        if sqllog::normalize(sql).chars().count() > SUMMARY_LENGTH {
            summary.push('…');
        }
        let description = format!("SQL breakpoint: {summary}");
        self.probes.pending_stop = Some(("stopped", json!({ "reason": "breakpoint", "threadId": thread, "allThreadsStopped": true, "hitBreakpointIds": hit, "description": description, "text": description })));
        true
    }

    /// 記録のあと: 記録の処理が止まると決めたなら（SQL、ダイアログの条件）、その通知。
    pub(super) fn take_record_stop(&mut self) -> Option<Event> {
        self.probes.pending_stop.take()
    }

    /// 文の実行時間を計り始める（呼び出しが戻ったら `sql_timer_finished`）。
    pub(super) fn time_sql(&mut self, thread_id: u32, seq: u64) {
        match self.step_out_watch(thread_id) {
            Some(stepper) => self.sql_break.timers.push(Timer { stepper, seq, started: Instant::now() }),
            None => crate::log!("sql timer: could not step out on thread {thread_id}"),
        }
    }

    /// いまのメソッドから戻るのを見張るステッパー（利用者のステップとは別。Just My Code は使わないので、呼び出し元が
    /// 利用者のコードでなくても、戻った所で終わる）。仕掛けられなければ `None`。
    pub(super) fn step_out_watch(&self, thread_id: u32) -> Option<ComPtr<ICorDebugStepper>> {
        unsafe {
            let process = self.process.as_ref()?;
            let thread = take::<ICorDebugThread>(tr_str!("Getting the thread"), |out| process.GetThread(thread_id, out)).ok()?;
            let stepper = take::<ICorDebugStepper>("ICorDebugThread::CreateStepper", |out| thread.CreateStepper(out)).ok()?;
            if let Ok(stepper2) = stepper.cast::<ICorDebugStepper2>() {
                stepper2.SetJMC(0);
            }
            stepper.StepOut().ok().ok()?;
            Some(stepper)
        }
    }

    /// ステップが終わった: 計時のステッパーなら、記録に時間を足して `netdbg.sqlCompleted` を返す（利用者には
    /// 見せない停止なので、呼び出し側は続行する）。ほかのステッパーなら `None`。
    pub(super) fn sql_timer_finished(&mut self, stepper: &ComPtr<ICorDebugStepper>, reason: u32) -> Option<Vec<Event>> {
        // CorDebugStepReason: 例外で抜けて、捕まえる所に着いた。
        const STEP_EXCEPTION_FILTER: u32 = 3;
        const STEP_EXCEPTION_HANDLER: u32 = 4;
        let index = self.sql_break.timers.iter().position(|t| t.stepper.as_raw() == stepper.as_raw())?;
        let timer = self.sql_break.timers.swap_remove(index);
        let ms = timer.started.elapsed().as_secs_f64() * 1000.0;
        let duration = (ms * 10.0).round() / 10.0;
        let failed = matches!(reason, STEP_EXCEPTION_FILTER | STEP_EXCEPTION_HANDLER);
        self.probes.journal.update(timer.seq, |body| {
            body["durationMs"] = json!(duration);
            if failed {
                body["failed"] = json!(true);
            }
        });
        let mut completed = json!({ "seq": timer.seq, "durationMs": duration });
        if failed {
            completed["failed"] = json!(true);
        }
        Some(vec![("netdbg.sqlCompleted", completed)])
    }
}
