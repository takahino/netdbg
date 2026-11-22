//! 入力のループ（本体のスレッド）へ届く入力の列。
//!
//! IF の層からの入力（DAP の要求、MCP の呼び出し）、デバッグ対象の出来事（ランタイムのコールバックのスレッド）、
//! 対象の標準出力（パイプを読むスレッド）を、一本の列にまとめる。状態を触るのは受け取る側のスレッドだけにする
//! （docs/DECISIONS.md D-008）。IF の層からの入力の中身は、IF の層（`crate::interface::ClientInput`）が決める。

use crate::com::{ComPtr, IUnknown, HRESULT};
use crate::cordebug::*;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

pub enum Input {
    /// IF の層（DAP、MCP）からの入力。
    Client(crate::interface::ClientInput),
    /// dbgshim の起動通知。ランタイムが読み込まれた。
    RuntimeStarted(Result<ComPtr<IUnknown>, HRESULT>),
    /// 起動した対象のプロセスが終わった（ランタイムを読み込まないまま終わったかは、受け取る側が判断する）。
    TargetExited { process_id: u32, exit_code: u32 },
    Debug(DebugEvent),
    /// デバッグ対象の標準出力・標準エラー。
    Output { category: &'static str, text: String },
    /// 対象の UI スレッドが、ウィンドウメッセージに `ms` ミリ秒答えていない（UI のフリーズの監視のスレッドから）。
    /// `generation` は、監視が数え始めた走りの番号（止めて続けたあとの古い知らせを捨てるため）。
    UiFrozen { process_id: u32, thread: u32, ms: u32, generation: u64 },
    /// カバレッジの印が変わった（時のスレッドから、間を置いて）。変わった行を知らせる。
    CoverageTick,
    /// 人の手の操作（成績書の記録。`gui::recorder` の読む側のスレッドから）。
    HandOperation(crate::report::operations::Recorded),
}

/// ICorDebugManagedCallback で届いた出来事。対象は、`controller` を `Continue` するまで止まっている。
pub struct DebugEvent {
    /// 続行に使う。ExitProcess のときだけ `None`。
    pub controller: Option<ComPtr<ICorDebugController>>,
    pub kind: EventKind,
}

pub enum EventKind {
    CreateProcess(ComPtr<ICorDebugProcess>),
    ExitProcess,
    CreateAppDomain(ComPtr<ICorDebugAppDomain>),
    LoadModule(ComPtr<ICorDebugModule>),
    UnloadModule(ComPtr<ICorDebugModule>),
    CreateThread(ComPtr<ICorDebugThread>),
    ExitThread(ComPtr<ICorDebugThread>),
    Breakpoint { thread: ComPtr<ICorDebugThread>, breakpoint: ComPtr<ICorDebugBreakpoint> },
    /// ステップが終わった。`reason` は CorDebugStepReason（STEP_NORMAL、STEP_RETURN、STEP_CALL など）。
    /// `stepper` は終わったステッパー（利用者のステップか、エンジンが計時に使うものかを見分ける）。
    StepComplete { thread: ComPtr<ICorDebugThread>, stepper: Option<ComPtr<ICorDebugStepper>>, reason: u32 },
    /// 関数評価が終わった。`failed` なら、評価したコードが例外を投げた（結果は、その例外のオブジェクト）。
    EvalComplete { eval: ComPtr<ICorDebugEval>, failed: bool },
    /// 例外の通知（ICorDebugManagedCallback2）。`kind` は CorDebugExceptionCallbackType（投げられた、捕まえる所が見つかった、誰も捕まえない）。
    /// `frame` は、通知の種類による: 捕まえる場所が見つかった（CATCH_HANDLER_FOUND）なら、捕まえるフレーム。
    Exception { thread: ComPtr<ICorDebugThread>, kind: u32, frame: Option<ComPtr<ICorDebugFrame>> },
    /// 対象が `Debugger.Break()` を呼んだ。
    Break { thread: ComPtr<ICorDebugThread> },
    /// Edit and Continue: 差分を当てたメソッドの古い版で、スレッドが系列点に来た（`old_il_offset` は古い版の IL の位置）。
    /// 続行する前に、葉のフレームで `RemapFunction` を呼べば、新しい版の対応する位置に乗り換える。
    FunctionRemapOpportunity { thread: ComPtr<ICorDebugThread>, old_function: ComPtr<ICorDebugFunction>, new_function: ComPtr<ICorDebugFunction>, old_il_offset: u32 },
    /// Edit and Continue: 乗り換えが終わった。
    FunctionRemapComplete { thread: ComPtr<ICorDebugThread> },
    /// GC の前と後（ICorDebugProcess8::EnableGCNotificationEvents で頼んだとき。書き込みの見張りの番地を置き直す。D-093）。
    BeforeGarbageCollection,
    AfterGarbageCollection,
    /// 書き込みの見張り（データ ブレークポイント）に当たった（D-093）。
    DataBreakpoint { thread: ComPtr<ICorDebugThread> },
    /// まだ扱っていないコールバック。名前はログ用。
    Other(&'static str),
}

/// 届いた入力の列。チャネルではなく自前の列にしてあるのは、関数評価の間だけ「デバッグ対象の出来事だけを先に取り出す」
/// 必要があるから（評価は要求の処理の途中で対象を動かし、終わりのコールバックを待つ。その間に届いた要求は、順番を
/// 保ったまま後で処理する）。
static QUEUE: Mutex<VecDeque<Input>> = Mutex::new(VecDeque::new());
static ARRIVED: Condvar = Condvar::new();

fn queue() -> MutexGuard<'static, VecDeque<Input>> {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 列の中に、要求 `seq` の取り消しが届いているか。取り出さずに見るだけ（取り消しの要求にも、あとで順番どおりに答える）。
/// 何が取り消しかは、IF の層が決める。
pub fn cancel_requested(seq: i64) -> bool {
    queue().iter().any(|input| matches!(input, Input::Client(client) if client.cancels(seq)))
}

/// どのスレッドからでも呼べる。
pub fn post(input: Input) {
    queue().push_back(input);
    ARRIVED.notify_all();
}

/// 条件に合う最初の入力を取り出す。`deadline` までに届かなければ `None`。
fn take(deadline: Option<Instant>, wanted: impl Fn(&Input) -> bool) -> Option<Input> {
    let mut inputs = queue();
    loop {
        if let Some(index) = inputs.iter().position(&wanted) {
            return inputs.remove(index);
        }
        inputs = match deadline {
            None => ARRIVED.wait(inputs).unwrap_or_else(|e| e.into_inner()),
            Some(deadline) => {
                let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
                ARRIVED.wait_timeout(inputs, left).unwrap_or_else(|e| e.into_inner()).0
            }
        };
    }
}

/// 次の入力（届いた順）。
pub fn next(deadline: Option<Instant>) -> Option<Input> {
    take(deadline, |_| true)
}

/// 次の「デバッグ対象の出来事」。ほかの入力は列に残す。関数評価の待ち合わせ用。
pub fn next_debug(deadline: Instant) -> Option<DebugEvent> {
    match take(Some(deadline), |input| matches!(input, Input::Debug(_))) {
        Some(Input::Debug(event)) => Some(event),
        _ => None,
    }
}