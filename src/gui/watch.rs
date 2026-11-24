//! 画面の木を、動いている間に読み続ける（停止のあいだの変化。D-087）。
//!
//! WinForms のコントロールの文字とチェックは、対象が止まっている間は読めない（メッセージに答えない）。そこで、動いている
//! 間に画面の変化の知らせ（WinEvent。対象の中に入らない形なので、対象の協力は要らない）を受けるたびに、少し待ってから
//! 木を読み直しておく。止まったら、「前の停止のあと最初に読んだ木」と「止まる前に最後に読んだ木」を比べる。止まる直前に
//! 変わって、まだ読めていないコントロールは「変わった（値は止まっている間は読めない）」として別に返す。
//!
//! - 知らせのフック（`SetWinEventHook`）は専用のスレッドに置き、そのスレッドでメッセージのループを回す。フックの中は
//!   窓を積むだけにする。
//! - 読むのは別のスレッド（GUI のスレッド。`super::spawn`）。窓が答えないとき（止まっている、忙しい）は読まずに待つ。

use super::ffi::*;
use crate::win32::HANDLE;
use crate::report::tree::{self, Control};
use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 知らせが続く間は読まない（最後の知らせから、この時間たってから読む）。
const SETTLE: Duration = Duration::from_millis(50);
/// 知らせが来なくても読み直す間隔（WinForms のチェックや WPF の文字のように、知らせの無い変化を拾う）。
const POLL: Duration = Duration::from_secs(1);
/// 窓が答えないときに、読み直すまで待つ時間。
const RETRY: Duration = Duration::from_millis(500);
/// 木が一度も読めていない間の間隔。速い起動では、最初の停止が最初の読みより先になる。
const STARTUP: Duration = Duration::from_millis(50);

/// 次の読みまで待つ時間。`None` は待たずに読む（見始めた直後）。
/// 木が一度も無い間は短く繰り返す。1 秒待つと、その間に止まった変化が、あとの最初の読みへ吸い込まれて消える。
fn pause_before_read(reads: u32, have_tree: bool, retry: bool) -> Option<Duration> {
    if reads == 0 {
        return None;
    }
    Some(if !have_tree { STARTUP } else if retry { RETRY } else { POLL })
}
/// 止まっている間は、中身（文字、チェック、選んだ項目）を読めないコントロールの種類。
const UNREADABLE_WHILE_STOPPED: &[&str] = &["TextBox", "RichTextBox", "ComboBox", "ListBox", "CheckBox", "RadioButton"];
/// 読むコントロールの数の上限。
const MAX_CONTROLS: usize = 3000;

/// 読んだ木（コントロールと、窓のハンドル → `controls` の添字）。
#[derive(Clone, Default)]
struct Read {
    controls: Vec<Control>,
    index: HashMap<usize, usize>,
    /// 窓はあるのに中を読めなかった窓（WPF の UI Automation が答えなかった）の見出し。前に読んだ中身を引き継ぐ。
    unreadable: Vec<String>,
    /// メッセージが時間切れになり、木が途中までしか無い。覚えずに前の木を残す。
    failed: bool,
}

#[derive(Default)]
struct State {
    /// 止まる前に最後に読んだ木。
    last: Option<Read>,
    /// 比べる元（前の停止のときの木）。
    baseline: Option<Read>,
    /// 前の停止で読めなかったコントロール（窓のハンドル）。次に読めたら、比べる元のその分だけを読んだ値にする
    /// （止まる前の変化を、次の停止でまた出さない。ほかの分は入れ直さない: 続行したあとの変化が消える）。
    fix_baseline: BTreeSet<usize>,
    /// 最後に読んだあとで変わった窓（まだ読めていない）。
    dirty: BTreeSet<usize>,
}

/// 止まったときの変化。
#[derive(Debug, Default)]
pub struct AtStop {
    pub changes: Vec<tree::Change>,
    /// 止まる直前に変わり、値をまだ読めていないコントロール（見せる名前）。
    pub unread: Vec<String>,
    /// 比べる元がある（最初の停止では無い）。
    pub compared: bool,
}

/// 知らせのフックが窓を渡す先（同時に 1 つ。新しく見始めたら差し替える）と、その世代（前のフックのスレッドが
/// 終わるときに、新しい渡す先を外さない）。
static SENDER: Mutex<Option<(u64, Sender<usize>)>> = Mutex::new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

unsafe extern "system" fn on_event(_hook: HANDLE, event: u32, hwnd: HWND, object: i32, _child: i32, _thread: u32, _time: u32) {
    let wanted = matches!(event, EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE | EVENT_OBJECT_STATECHANGE | EVENT_OBJECT_NAMECHANGE | EVENT_OBJECT_VALUECHANGE);
    if !wanted || hwnd.is_null() || matches!(object, OBJID_CARET | OBJID_CURSOR) {
        return;
    }
    if let Some((_, sender)) = SENDER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(hwnd as usize);
    }
}

/// 見ている最中。落とすと止める。
pub struct Watcher {
    process_id: u32,
    hook_thread: u32,
    state: Arc<Mutex<State>>,
}

/// 対象のプロセス `process_id` の画面を見始める（始めに 1 回読む）。
pub fn start(process_id: u32) -> Result<Watcher, String> {
    let (sender, receiver) = channel::<usize>();
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    *SENDER.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, sender.clone()));
    let release = move || {
        let mut slot = SENDER.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|(g, _)| *g == generation) {
            slot.take();
        }
    };
    let (ready_sender, ready_receiver) = channel::<Result<u32, String>>();
    std::thread::spawn(move || unsafe {
        let hook = SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_VALUECHANGE, std::ptr::null_mut(), on_event, process_id, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        if hook.is_null() {
            let _ = ready_sender.send(Err(tr!("Could not watch the program's windows (SetWinEventHook failed).")));
        } else {
            let _ = ready_sender.send(Ok(GetCurrentThreadId()));
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {}
            UnhookWinEvent(hook);
        }
        // 渡す先を外すと、読む側の受け口が閉じ、読む側も終わる。
        release();
    });
    let hook_thread = match ready_receiver.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(thread)) => thread,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            release();
            return Err(tr!("Could not watch the program's windows (SetWinEventHook failed)."));
        }
    };
    let state = Arc::new(Mutex::new(State::default()));
    let shared = state.clone();
    super::spawn(move || {
        // 始めに待たずに 1 回読む。その後は、知らせのあと（続く知らせをまとめてから）と、知らせが無くても間隔を置いて読む。
        // 木が一度も無い間は短く繰り返す（窓がまだ無い、答えられない）。読めたあとは、答えられなければ `RETRY`、そうでなければ `POLL`。
        let mut reads = 0u32;
        let mut have_tree = false;
        let mut retry = true;
        loop {
            if let Some(wait) = pause_before_read(reads, have_tree, retry) {
                match receiver.recv_timeout(wait) {
                    Ok(handle) => {
                        lock(&shared).dirty.insert(handle);
                        while let Ok(handle) = receiver.recv_timeout(SETTLE) {
                            lock(&shared).dirty.insert(handle);
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            reads += 1;
            let read = read_into(process_id, &shared);
            if read {
                have_tree = true;
            }
            retry = !read;
        }
    });
    drop(sender);
    Ok(Watcher { process_id, hook_thread, state })
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// 木を読み、覚える。窓が答えなければ読まずに false（あとで読み直す）。
fn read_into(process_id: u32, state: &Mutex<State>) -> bool {
    // 止まっている対象を読みにいくと、コントロールごとに待ってしまう。先に、窓が答えるかを見る。
    let windows = super::snapshot(process_id, false, MAX_CONTROLS);
    // 窓がまだ無い回を成功にすると、次は 1 秒待ちになる。その間に最初の停止が来ると、比べる元が空のまま残る。
    let Some(top) = windows.elements.iter().find(|e| e.depth == 0) else { return false };
    if !super::responds(top.id) {
        return false;
    }
    let read = read_tree(process_id);
    // 止まっている UI スレッドへの読み取りは、名前の無い木になる。覚えずに、前の木を残す。
    if read.failed {
        return false;
    }
    lock(state).read(read);
    true
}

/// 画面の木を読む（動いている対象。止まっている間は文字とチェックが読めない）。
pub fn read_controls(process_id: u32) -> Vec<Control> {
    read_tree(process_id).controls
}

fn read_tree(process_id: u32) -> Read {
    let snapshot = super::snapshot(process_id, true, MAX_CONTROLS);
    let titles: HashMap<usize, String> = snapshot.elements.iter().filter(|e| e.depth == 0).map(|e| (e.window, e.text.clone().unwrap_or_default())).collect();
    let mut read = Read { failed: snapshot.failed, ..Read::default() };
    // UI Automation で読む窓（WPF）で、読めなかったか、中が 1 つも無かったもの（読み損ね）。
    for &window in &snapshot.automation_windows {
        let empty = !snapshot.elements.iter().any(|e| e.depth > 0 && e.window == window);
        if empty || snapshot.unread.iter().any(|(w, _)| *w == window) {
            read.unreadable.push(titles.get(&window).cloned().unwrap_or_default());
        }
    }
    for e in snapshot.elements.iter().filter(|e| e.depth > 0) {
        read.index.insert(e.id, read.controls.len());
        read.controls.push(Control {
            window: titles.get(&e.window).cloned().unwrap_or_default(),
            name: e.name.clone(),
            kind: e.kind.to_string(),
            text: e.text.clone(),
            checked: e.checked,
            enabled: e.enabled,
            visible: e.visible,
        });
    }
    read
}

/// 止まっている間に、メッセージを送らずに読めた値（窓のコントロールだけ）。
struct Now {
    enabled: bool,
    visible: bool,
    /// 窓が持っている文字（ラベルやボタン）。
    text: String,
}

impl State {
    /// 動いている間に読めた木を覚える。
    fn read(&mut self, mut read: Read) {
        // 中を読めなかった窓は、前に読んだ中身を引き継ぐ（読み損ねを「窓が閉じた」にしない）。
        // 見出しのメッセージが空で返ったときは、前に見えていた窓が 1 つだけなら、その見出しのままにする。
        if let Some(previous) = &self.last {
            let mut titles: Vec<String> = previous.controls.iter().map(|c| c.window.clone()).filter(|t| !t.is_empty()).collect();
            titles.sort();
            titles.dedup();
            if titles.len() == 1 {
                for title in &mut read.unreadable {
                    if title.is_empty() {
                        *title = titles[0].clone();
                    }
                }
            }
            for (handle, &index) in &previous.index {
                let control = &previous.controls[index];
                if read.unreadable.contains(&control.window) && !read.index.contains_key(handle) {
                    read.index.insert(*handle, read.controls.len());
                    read.controls.push(control.clone());
                }
            }
        }
        // 比べる元が無いか空（窓がまだ開いていなかった）なら、いま読んだものを元にする。
        if self.baseline.as_ref().is_none_or(|b| b.controls.is_empty()) {
            self.baseline = Some(read.clone());
        }
        // 止まる前に名前を読めなかった木へ、動いている間に読めた名前と、止まっている間は読めない値を写す。
        // ラベルの文字は写さない（止まったときの文字が、次の停止との比べる元）。
        self.adopt_details(&read);
        // 前の停止で読めなかった分は、いま読めた値を比べる元にする（止まる前の変化を、次の停止でまた出さない。ほかの分を
        // 入れ直すと、続行したあとの変化が消えるので入れ直さない）。
        let fix = std::mem::take(&mut self.fix_baseline);
        if let Some(baseline) = self.baseline.as_mut() {
            for handle in fix {
                if let (Some(&b), Some(&r)) = (baseline.index.get(&handle), read.index.get(&handle)) {
                    baseline.controls[b] = read.controls[r].clone();
                }
            }
        }
        self.last = Some(read);
        self.dirty.clear();
    }

    /// まだ一度も読めていない停止で、メッセージを送らずに取れる木を比べる元にする。
    /// 名前は空のまま。次に動いている間に読めた木から `adopt_details` が名前だけを足す。
    fn seed_stopped(&mut self, process_id: u32) {
        let snapshot = super::snapshot(process_id, false, MAX_CONTROLS);
        let titles: HashMap<usize, String> = snapshot.elements.iter().filter(|e| e.depth == 0).map(|e| (e.window, super::text_without_messages(e.id))).collect();
        if titles.is_empty() {
            return;
        }
        let mut read = Read::default();
        for e in snapshot.elements.iter().filter(|e| e.depth > 0) {
            let kind = e.kind.to_string();
            let text = if UNREADABLE_WHILE_STOPPED.contains(&kind.as_str()) { None } else { Some(super::text_without_messages(e.id)) };
            read.index.insert(e.id, read.controls.len());
            read.controls.push(Control {
                window: titles.get(&e.window).cloned().unwrap_or_default(),
                name: None,
                kind,
                text,
                checked: None,
                enabled: e.enabled,
                visible: e.visible,
            });
        }
        if read.controls.is_empty() {
            return;
        }
        self.baseline = Some(read.clone());
        self.last = Some(read);
    }

    /// 名前の無い比べる元へ、あとから読めた名前と、止まっている間は読めない値（文字、チェック）を写す。
    fn adopt_details(&mut self, read: &Read) {
        let Some(baseline) = self.baseline.as_mut() else { return };
        let pairs: Vec<(usize, usize)> = baseline.index.iter().filter_map(|(handle, &b)| read.index.get(handle).map(|&r| (b, r))).collect();
        for (b, r) in pairs {
            let source = &read.controls[r];
            let dest = &mut baseline.controls[b];
            if dest.name.is_none() {
                dest.name.clone_from(&source.name);
            }
            if dest.checked.is_none() {
                dest.checked = source.checked;
            }
            if dest.text.is_none() && UNREADABLE_WHILE_STOPPED.contains(&dest.kind.as_str()) {
                dest.text.clone_from(&source.text);
            }
        }
    }

    /// 止まった: 前の停止からの変化を返し、いまの木を次の比べる元にする。`now` は、止まっていても読めるコントロールの
    /// いまの値（変化の知らせは、止まったあとに届くことがある）。読めない分（テキストボックスの文字、チェック、選んだ
    /// 項目）のうち、最後に読んだあとで変わったものは「読めない」として返す。
    fn at_stop(&mut self, now: &dyn Fn(usize) -> Option<Now>) -> AtStop {
        let Some(mut last) = self.last.clone() else { return AtStop::default() };
        let mut unread = BTreeSet::new();
        let mut unread_handles = BTreeSet::new();
        for (&handle, &index) in &last.index {
            let Some(now) = now(handle) else { continue };
            let control = &mut last.controls[index];
            control.enabled = now.enabled;
            control.visible = now.visible;
            if !UNREADABLE_WHILE_STOPPED.contains(&control.kind.as_str()) {
                control.text = Some(now.text);
            } else if self.dirty.contains(&handle) {
                unread.insert(control.name.clone().unwrap_or_else(|| control.kind.clone()));
                unread_handles.insert(handle);
            }
        }
        let unread: Vec<String> = unread.into_iter().collect();
        let result = match &self.baseline {
            Some(baseline) => AtStop { changes: tree::diff(&baseline.controls, &last.controls), unread, compared: true },
            None => AtStop { unread, ..AtStop::default() },
        };
        self.fix_baseline = unread_handles;
        self.baseline = Some(last);
        result
    }
}

impl Watcher {
    /// 止まった: 前の停止からの変化を返し、いまの木を次の比べる元にする。
    pub fn at_stop(&self) -> AtStop {
        let process_id = self.process_id;
        {
            let mut state = lock(&self.state);
            // 起動が速く、最初の読みより先に止まると、あとの読みが比べる元になり、その前の変化が消える。
            if state.last.is_none() {
                state.seed_stopped(process_id);
            }
        }
        lock(&self.state).at_stop(&|handle| {
            // WPF のコントロール（窓ではない。番号は UI Automation のもの）は読み直さない。
            (super::process_of(handle) == process_id).then(|| {
                let (enabled, visible) = super::window_state(handle);
                Now { enabled, visible, text: super::text_without_messages(handle) }
            })
        })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.hook_thread, WM_QUIT, 0, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(name: &str, kind: &str, text: &str, checked: Option<bool>) -> Control {
        Control { window: "Smoke GUI".into(), name: Some(name.into()), kind: kind.into(), text: Some(text.into()), checked, enabled: true, visible: true }
    }

    /// 窓のハンドル 1 = nameBox、2 = agreeBox、3 = resultLabel。
    fn read(name: &str, agree: bool, label: &str) -> Read {
        let controls = vec![control("nameBox", "TextBox", name, None), control("agreeBox", "CheckBox", "Agree", Some(agree)), control("resultLabel", "Label", label, None)];
        Read { controls, index: [(1, 0), (2, 1), (3, 2)].into_iter().collect(), ..Read::default() }
    }

    /// 止まっている間に読める値（ラベルの文字は `label`）。
    fn stopped(label: &'static str) -> impl Fn(usize) -> Option<Now> {
        move |handle| Some(Now { enabled: true, visible: true, text: if handle == 3 { label.into() } else { String::new() } })
    }

    fn whats(found: &AtStop) -> Vec<(String, String, String)> {
        found.changes.iter().map(|c| (c.control.clone(), c.what.clone(), c.after.clone())).collect()
    }

    #[test]
    fn the_first_stop_compares_with_the_first_read_that_had_windows() {
        let mut state = State::default();
        state.read(Read::default());
        state.read(read("", false, ""));
        state.read(read("Ada", true, ""));
        let found = state.at_stop(&stopped(""));
        assert!(found.compared);
        assert_eq!(whats(&found), [("nameBox".into(), "text".into(), "Ada".into()), ("agreeBox".into(), "checked".into(), "入".into())]);
    }

    #[test]
    fn a_label_changed_just_before_the_stop_is_read_while_stopped() {
        let mut state = State::default();
        state.read(read("Ada", false, ""));
        state.at_stop(&stopped(""));
        // 続行して、ハンドラーがラベルを書き、読む前に止まった。
        state.dirty.insert(3);
        let found = state.at_stop(&stopped("Hello, Ada"));
        assert_eq!(whats(&found), [("resultLabel".into(), "text".into(), "Hello, Ada".into())]);
        assert!(found.unread.is_empty());
    }

    #[test]
    fn a_text_box_changed_just_before_the_stop_is_unread_and_not_reported_twice() {
        let mut state = State::default();
        state.read(read("", false, ""));
        state.dirty.insert(1);
        let found = state.at_stop(&stopped(""));
        assert_eq!(found.unread, ["nameBox"]);
        assert!(found.changes.is_empty());
        // 続行して読めた: 文字は止まる前の変化なので、比べる元に入る。続行のあとに変わったラベルは、次の停止の変化。
        state.read(read("Ada", false, "Hello, Ada"));
        let found = state.at_stop(&stopped("Hello, Ada"));
        assert_eq!(whats(&found), [("resultLabel".into(), "text".into(), "Hello, Ada".into())]);
    }

    #[test]
    fn an_unreadable_window_whose_title_was_empty_is_not_closed() {
        let mut state = State::default();
        state.read(read("Ada", true, ""));
        state.read(Read { unreadable: vec![String::new()], ..Read::default() });
        let found = state.at_stop(&|_| None);
        assert!(found.changes.iter().all(|c| c.what != "closed"), "{:?}", found.changes);
        assert!(found.changes.is_empty(), "{:?}", found.changes);
    }

    #[test]
    fn a_window_whose_inside_could_not_be_read_keeps_what_was_read_before() {
        let mut state = State::default();
        state.read(read("Ada", true, ""));
        state.read(Read { unreadable: vec!["Smoke GUI".into()], ..Read::default() });
        let found = state.at_stop(&stopped(""));
        assert!(found.changes.is_empty(), "{:?}", found.changes);
    }

    #[test]
    fn nothing_is_compared_before_anything_was_read() {
        let mut state = State::default();
        let found = state.at_stop(&stopped(""));
        assert!(!found.compared && found.changes.is_empty());
    }

    /// 名前の無い、止まった瞬間の木（ラベルの文字は空）。
    fn seeded(label: &str) -> Read {
        let controls = vec![
            Control { window: "Smoke GUI".into(), name: None, kind: "TextBox".into(), text: None, checked: None, enabled: true, visible: true },
            Control { window: "Smoke GUI".into(), name: None, kind: "CheckBox".into(), text: None, checked: None, enabled: true, visible: true },
            Control { window: "Smoke GUI".into(), name: None, kind: "Label".into(), text: Some(label.into()), checked: None, enabled: true, visible: true },
        ];
        Read { controls, index: [(1, 0), (2, 1), (3, 2)].into_iter().collect(), ..Read::default() }
    }

    #[test]
    fn the_first_read_waits_for_nothing_and_an_unread_tree_is_retried_quickly() {
        assert_eq!(pause_before_read(0, false, true), None);
        assert_eq!(pause_before_read(1, false, true), Some(STARTUP));
        assert_eq!(pause_before_read(2, true, true), Some(RETRY));
        assert_eq!(pause_before_read(2, true, false), Some(POLL));
    }

    #[test]
    fn a_stop_before_any_read_keeps_that_labels_text_as_the_baseline() {
        let mut state = State::default();
        let seed = seeded("");
        state.baseline = Some(seed.clone());
        state.last = Some(seed);
        let found = state.at_stop(&stopped(""));
        assert!(found.changes.is_empty(), "{:?}", found.changes);
        // 続行したあと、ハンドラーがラベルを書いてから、初めて名前つきで読めた。
        let later = read("", false, "Hello");
        state.read(later);
        let found = state.at_stop(&stopped("Hello"));
        assert_eq!(whats(&found), [("resultLabel".into(), "text".into(), "Hello".into())]);
    }
}