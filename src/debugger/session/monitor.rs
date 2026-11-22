//! 対象の見張り（段階 11）: UI のフリーズと、プロセスの使用量（netdbg.processUsage。読むのは usage.rs）。
//!
//! UI のフリーズ: 対象の UI スレッドがウィンドウメッセージに答えない時間が閾値を超えたら、そのスレッドで
//! 止めて理由を出す（Visual Studio には無い）。拡張の補助（PowerShell の uiFreezeWatch.ps1）が行っていた監視を、
//! エンジンの中のスレッドで行う。
//!
//! 監視のスレッドは Win32 だけを使い（見えているトップレベルの窓を、スレッドごとに 1 つ選んで `WM_NULL` を送る）、
//! 答えないスレッドを入力の列（`Input::UiFrozen`）で知らせるだけ。止めるかを決めるのは、入力のループのスレッド
//! （セッションの状態を触るのはそこだけ。D-008）。数えるのは、対象が続行で走っている間だけ。止まっている間とステップの
//! 間は数えない（遅い呼び出しをステップで越えるのは、フリーズではない。拡張と同じ）。数え直しは、走り出すたび。

use super::*;
use crate::events::{post, Input};
use crate::gui::ffi::{GetWindowThreadProcessId, SendMessageTimeoutW, HWND, SMTO_ABORTIFHUNG};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) const DEFAULT_THRESHOLD_MS: u32 = 2000;
/// 閾値の下限（短すぎると、ふつうの重い処理で止まる）。
const MIN_THRESHOLD_MS: u32 = 500;
/// 窓に問い合わせる間隔と、1 回の問い合わせで答えを待つ時間。
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const ANSWER_TIMEOUT_MS: u32 = 200;
const WM_NULL: u32 = 0;

/// 監視の設定（起動の引数 `uiFreeze`、要求 `netdbg.setUiFreeze`）。
#[derive(Clone, Copy, Debug)]
pub(super) struct FreezeSettings {
    pub enabled: bool,
    pub threshold_ms: u32,
}

impl Default for FreezeSettings {
    fn default() -> FreezeSettings {
        FreezeSettings { enabled: true, threshold_ms: DEFAULT_THRESHOLD_MS }
    }
}

impl FreezeSettings {
    /// `{ enabled, thresholdMs }`（`true` / `false` だけでもよい）を、今の設定に重ねる。
    fn apply(&mut self, value: &Value) {
        if let Some(enabled) = value.as_bool().or_else(|| value.get("enabled").and_then(Value::as_bool)) {
            self.enabled = enabled;
        }
        if let Some(ms) = value.get("thresholdMs").and_then(Value::as_u64) {
            self.threshold_ms = (ms.min(u32::MAX as u64) as u32).max(MIN_THRESHOLD_MS);
        }
    }

    fn to_json(self) -> Value {
        json!({ "enabled": self.enabled, "thresholdMs": self.threshold_ms })
    }
}

/// 監視のスレッドと分け合う状態。
struct Shared {
    /// 数えてよいか（対象が続行で走っている）。
    counting: AtomicBool,
    /// 走り出した回数。変わったら、前の数えを捨てる（止まっていた間に答えなかった分を数えない）。
    generation: AtomicU64,
    threshold_ms: AtomicU32,
    stop: AtomicBool,
}

/// 1 つの対象の監視。落とすと、監視のスレッドも終わる。
pub(super) struct FreezeWatch {
    process_id: u32,
    shared: Arc<Shared>,
}

impl FreezeWatch {
    fn start(process_id: u32, threshold_ms: u32) -> FreezeWatch {
        let shared = Arc::new(Shared { counting: AtomicBool::new(false), generation: AtomicU64::new(0), threshold_ms: AtomicU32::new(threshold_ms), stop: AtomicBool::new(false) });
        let watched = shared.clone();
        let started = std::thread::Builder::new().name("ui-freeze".into()).spawn(move || watch(process_id, &watched));
        if let Err(e) = started {
            crate::log!("ui freeze: could not start the watch: {e}");
        }
        FreezeWatch { process_id, shared }
    }

    fn set_counting(&self, counting: bool) {
        let was = self.shared.counting.swap(counting, Ordering::SeqCst);
        if counting && !was {
            self.shared.generation.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }
}

impl Drop for FreezeWatch {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
    }
}

/// 監視のスレッド。答えないスレッドは、フリーズ 1 回につき 1 度だけ知らせる（答えたら、また数え直す）。
fn watch(process_id: u32, shared: &Shared) {
    let mut since: HashMap<u32, Instant> = HashMap::new();
    let mut reported: HashSet<u32> = HashSet::new();
    let mut seen_generation = u64::MAX;
    while !shared.stop.load(Ordering::SeqCst) {
        std::thread::sleep(POLL_INTERVAL);
        let generation = shared.generation.load(Ordering::SeqCst);
        if generation != seen_generation || !shared.counting.load(Ordering::SeqCst) {
            since.clear();
            reported.clear();
            seen_generation = generation;
        }
        if !shared.counting.load(Ordering::SeqCst) {
            continue;
        }
        for (thread, window) in ui_thread_windows(process_id) {
            let mut result = 0usize;
            let answered = unsafe { SendMessageTimeoutW(window as HWND, WM_NULL, 0, 0, SMTO_ABORTIFHUNG, ANSWER_TIMEOUT_MS, &mut result) } != 0;
            // 待っている間に、デバッガーが対象を止めた（答えないのは、そのせい）。
            if !shared.counting.load(Ordering::SeqCst) || shared.generation.load(Ordering::SeqCst) != seen_generation {
                break;
            }
            if answered {
                since.remove(&thread);
                reported.remove(&thread);
                continue;
            }
            // 1 回目の問い合わせで待った分も、答えなかった時間に入れる。
            let first = *since.entry(thread).or_insert_with(|| Instant::now() - Duration::from_millis(ANSWER_TIMEOUT_MS as u64));
            let ms = first.elapsed().as_millis().min(u32::MAX as u128) as u32;
            if ms >= shared.threshold_ms.load(Ordering::SeqCst) && reported.insert(thread) {
                post(Input::UiFrozen { process_id, thread, ms, generation: seen_generation });
            }
        }
    }
}

/// プロセスの UI スレッドと、そのスレッドの見えているトップレベルの窓（1 つあれば、そのスレッドに問い合わせられる）。
fn ui_thread_windows(process_id: u32) -> Vec<(u32, usize)> {
    let mut windows: Vec<(u32, usize)> = Vec::new();
    for window in crate::gui::top_windows(process_id) {
        let thread = unsafe { GetWindowThreadProcessId(window as HWND, std::ptr::null_mut()) };
        if !windows.iter().any(|(t, _)| *t == thread) {
            windows.push((thread, window));
        }
    }
    windows
}

impl Session {
    /// 起動の引数 `uiFreeze`（`{ enabled, thresholdMs }`、または `true` / `false`）を、今の設定に重ねる。起動の前に
    /// `netdbg.setUiFreeze`（MCP の `ui_freeze`）で変えた設定は残す（SQL ログと同じく、エンジンが終わるまで続く）。
    pub(super) fn read_ui_freeze_options(&mut self, arguments: &Value) {
        if let Some(value) = arguments.get("uiFreeze") {
            self.ui_freeze.apply(value);
        }
    }

    /// 対象のプロセスができた: 監視を始める（設定で切っていなければ）。
    pub(super) fn start_freeze_watch(&mut self, process_id: u32) {
        self.freeze_watch = self.ui_freeze.enabled.then(|| FreezeWatch::start(process_id, self.ui_freeze.threshold_ms));
    }

    /// `netdbg.setUiFreeze { enabled, thresholdMs }`: 監視の設定を変える（走っている対象にもすぐ効く）。今の設定を返す。
    pub fn set_ui_freeze(&mut self, arguments: &Value) -> Result<Value, String> {
        self.ui_freeze.apply(arguments);
        match (&self.freeze_watch, self.ui_freeze.enabled) {
            (Some(watch), true) => watch.shared.threshold_ms.store(self.ui_freeze.threshold_ms, Ordering::SeqCst),
            (Some(_), false) => self.freeze_watch = None,
            (None, true) => {
                if let Some(process_id) = self.process_id() {
                    self.start_freeze_watch(process_id);
                }
            }
            (None, false) => {}
        }
        self.sync_freeze_watch();
        Ok(self.ui_freeze.to_json())
    }

    /// 入力を処理したあと: 対象が続行で走っているかを、監視に伝える。
    pub fn sync_freeze_watch(&self) {
        if let Some(watch) = &self.freeze_watch {
            watch.set_counting(self.freeze_counts());
        }
    }

    /// 対象の出来事を処理する間（対象は止まっている）は、数えない。
    pub fn hold_freeze_watch(&self) {
        if let Some(watch) = &self.freeze_watch {
            watch.set_counting(false);
        }
    }

    /// 数えてよいか: 対象が走っていて、ステップ（利用者のものも、エンジンが仕掛けたものも）の途中でない。
    fn freeze_counts(&self) -> bool {
        self.state == State::Running && self.stepper.is_none() && self.async_step_out.is_none() && self.target_step.is_none() && self.watch_step.is_none()
    }

    /// 監視のスレッドが、答えない UI スレッドを知らせてきた。まだ同じ走りの中なら、止めて理由を出す。
    pub fn ui_frozen(&mut self, process_id: u32, thread: u32, ms: u32, generation: u64) -> Vec<Event> {
        let current = self.freeze_watch.as_ref().is_some_and(|w| w.process_id == process_id && w.generation() == generation);
        if !current || !self.freeze_counts() {
            return Vec::new();
        }
        if let Err(e) = self.pause() {
            crate::log!("ui freeze: could not pause: {e}");
            return Vec::new();
        }
        crate::log!("ui freeze: thread {thread} did not answer for {ms} ms: paused");
        let description = format!("UI not responding for {:.1} s", f64::from(ms) / 1000.0);
        self.probes.journal.push("uiFreeze", json!({ "threadId": thread, "ms": ms }));
        vec![
            // 拡張が自分の言葉で見せられるよう、数のままも渡す。
            ("netdbg.uiFrozen", json!({ "threadId": thread, "ms": ms })),
            ("output", json!({ "category": "console", "output": format!("{description}\n") })),
            ("stopped", json!({ "reason": "pause", "threadId": thread, "description": description, "text": description, "allThreadsStopped": true })),
        ]
    }

    /// `netdbg.processUsage`: 対象のプロセスの今の使用量（usage.rs）。
    pub fn process_usage(&self) -> Result<Value, String> {
        if self.process_handle.is_null() || matches!(self.state, State::NotStarted | State::Exited) {
            return Err(tr!("The target is not running."));
        }
        Ok(unsafe { crate::debugger::usage::sample(self.process_handle) }.to_json())
    }
}
