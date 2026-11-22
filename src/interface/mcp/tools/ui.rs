//! 対象のアプリの画面のツール（`ui_*`）: WinForms、WPF の画面を読み、操作し、撮る（D-053、D-054、D-055）。画面の処理は `crate::gui`。
//!
//! - 操作は、対象が動いているときだけ（止まっている窓はメッセージに答えない）。一覧と画像は、止まっていてもできる。
//! - 操作のあと、少し（`waitMs`）待つ。その間にブレークポイントで止まれば、止まった場所を返す。
//! - 相手は `id`（窓のハンドル、または UI Automation の要素の番号）、`mark`（`ui_tree` / `ui_find` / 印つきの `ui_screenshot` の番号）、`name`（Control.Name、x:Name）、
//!   `text`（表示の文字。名前の無いメッセージボックスのボタンなど）のどれかで指す。

use super::{tool, Context, Outcome, MAX_WAIT_MS};
use crate::gui::{self, capture, ffi::RECT, rect_json, Element, Snapshot};
use crate::interface::mcp::state::Target;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    // コントロールの指し方（どれか 1 つ）。
    let target = |extra: Value| {
        let mut properties = json!({
            "id": { "type": "integer", "description": "Control id from ui_tree / ui_find." },
            "mark": { "type": "integer", "minimum": 1, "description": "Number shown by the latest ui_tree, ui_find or annotated ui_screenshot." },
            "name": { "type": "string", "description": "The control's name in code (WinForms Control.Name, WPF x:Name / AutomationId), e.g. okButton." },
            "text": { "type": "string", "description": "The control's displayed text, for controls without a name (e.g. the OK button of a message box)." },
            "window": { "type": "integer", "description": "Limit the search to this window (window id from ui_tree)." },
        });
        if let (Some(properties), Value::Object(extra)) = (properties.as_object_mut(), extra) {
            properties.extend(extra);
        }
        properties
    };
    let wait = json!({ "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS, "description": "Milliseconds to watch for a breakpoint hit after the action (default 500). If the program stops, the result has stopped: true and where." });
    vec![
        tool(
            "ui_tree",
            "The debugged program's visible windows (front first) and their controls: kind, name, text, enabled, checked, and the rectangle in screen pixels and inside the window (the same pixels as a window ui_screenshot). \
             Each control has a mark number to use with the other ui_* tools. Works while stopped too (names from earlier, no text).",
            json!({
                "window": { "type": "integer", "description": "Only this window." },
                "maxElements": { "type": "integer", "minimum": 1, "maximum": 2000, "description": "Stop listing after this many (default 300)." },
            }),
            &[],
        ),
        tool(
            "ui_find",
            "Find controls by name (exact), text (contains, case-insensitive) or kind (Button, TextBox, CheckBox, RadioButton, ComboBox, ListBox, Label, ...).",
            json!({ "name": { "type": "string" }, "text": { "type": "string" }, "kind": { "type": "string" }, "window": { "type": "integer" } }),
            &[],
        ),
        tool(
            "ui_screenshot",
            "Picture (PNG) of the program's screen, also while it is stopped at a breakpoint (shows its last drawing, not the 'Not Responding' ghost). \
             scope: window (default: the front window, or the window of the given control), control, app (all its visible windows, dialogs included), screen (the whole monitor). \
             annotate draws each control's mark number on the picture. The result gives the screen point of the picture's top-left corner.",
            target(json!({
                "scope": { "type": "string", "enum": ["window", "control", "app", "screen"] },
                "annotate": { "type": "boolean", "description": "Draw the mark numbers of the controls (default false)." },
            })),
            &[],
        ),
        tool(
            "ui_invoke",
            "Press a button, check box or radio button (as a click would, without moving the mouse). The program must be running.",
            target(json!({ "waitMs": wait })),
            &[],
        ),
        tool(
            "ui_set_text",
            "Replace the text of a text box (raises TextChanged).",
            target(json!({ "value": { "type": "string", "description": "The new text." }, "waitMs": wait })),
            &["value"],
        ),
        tool(
            "ui_select",
            "Select an item of a combo box or list box by its text or index (raises SelectedIndexChanged). An unknown item is refused with the list of items.",
            target(json!({ "item": { "type": "string" }, "index": { "type": "integer", "minimum": 0 }, "waitMs": wait })),
            &[],
        ),
        tool(
            "ui_toggle",
            "Check or uncheck a check box. With checked, does nothing when it is already in that state.",
            target(json!({ "checked": { "type": "boolean" }, "waitMs": wait })),
            &[],
        ),
        tool(
            "ui_keys",
            "Type keys into the program: brings its window to the front, focuses the given control (if any), then types. \
             Plain characters are typed as they are; special keys in braces: {Enter} {Tab} {Esc} {Backspace} {Delete} {Home} {End} {Up} {Down} {Left} {Right} {PageUp} {PageDown} {F1}..{F24} {Space}, \
             with modifiers {Ctrl+A} {Shift+Tab} {Alt+F4}; {{ and }} for braces.",
            target(json!({ "keys": { "type": "string" }, "waitMs": wait })),
            &["keys"],
        ),
        tool(
            "ui_mouse",
            "Real mouse input at a point: brings the window to the front, then click / double_click / right_click / move / down / up / drag (to toX, toY) / wheel (clicks, negative = down). \
             relativeTo: window (default; the pixels of a window ui_screenshot), control (the given control; without x, y its center), screen.",
            target(json!({
                "action": { "type": "string", "enum": ["click", "double_click", "right_click", "move", "down", "up", "drag", "wheel"] },
                "x": { "type": "integer" },
                "y": { "type": "integer" },
                "relativeTo": { "type": "string", "enum": ["window", "control", "screen"] },
                "toX": { "type": "integer" },
                "toY": { "type": "integer" },
                "clicks": { "type": "integer", "description": "Wheel notches (default -1)." },
                "waitMs": wait,
            })),
            &[],
        ),
        tool(
            "ui_window",
            "Bring a window of the program to the front, move or resize it (x, y, width, height in screen pixels), or close it.",
            json!({
                "window": { "type": "integer", "description": "Window id from ui_tree (default: the front window)." },
                "action": { "type": "string", "enum": ["front", "move", "close"] },
                "x": { "type": "integer" },
                "y": { "type": "integer" },
                "width": { "type": "integer" },
                "height": { "type": "integer" },
                "waitMs": wait,
            }),
            &["action"],
        ),
        tool(
            "ui_wait_for",
            "Wait until a control appears, becomes enabled, or disappears (e.g. a dialog after a click). Answers early if the program stops at a breakpoint.",
            json!({
                "name": { "type": "string" },
                "text": { "type": "string" },
                "kind": { "type": "string" },
                "window": { "type": "integer" },
                "state": { "type": "string", "enum": ["appear", "enabled", "disappear"] },
                "timeoutMs": { "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS, "description": "Default 10000." },
            }),
            &[],
        ),
        tool(
            "ui_changes",
            "Controls whose text, checked state, enabled state or visibility changed between the previous stop and this one (breakpoint, pause or step), and windows that opened or closed. \
             The windows are read while the program runs (a stopped WinForms program cannot answer), so controls that changed just before the stop are listed in unread: changed, value not readable until the program continues.",
            json!({}),
            &[],
        ),
        tool(
            "ui_replay",            "Replay recorded steps on the running program, e.g. on a newly delivered exe: the steps file that test_report writes (.steps.json; both the AI's ui_* actions and hand operations are recorded), or steps given directly. \
             Each step is a ui_invoke / ui_set_text / ui_select / ui_toggle / ui_keys call that points at a control by name (or text), e.g. {\"tool\": \"ui_set_text\", \"name\": \"nameBox\", \"value\": \"Taro\"}. \
             Waits for each control to appear and be enabled (timeoutMs per step, default 5000) and pauses between steps (pauseMs, default 300). \
             Stops at the first step that fails, or when the program stops (breakpoint) or does not answer; the result lists each step's outcome.",
            json!({
                "steps": { "type": "array", "items": { "type": "object" }, "description": "Steps to replay (instead of file)." },
                "file": { "type": "string", "description": "A steps file written by test_report (absolute path)." },
                "item": { "type": "string", "description": "With file: replay only this test item's steps (default: all items in order)." },
                "timeoutMs": { "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS, "description": "How long to wait for each step's control (default 5000)." },
                "pauseMs": { "type": "integer", "minimum": 0, "maximum": 10000, "description": "Pause after each step (default 300)." },
            }),
            &[],
        ),
    ]
}
/// このモジュールのツールなら、GUI のスレッドで始める。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> Option<Outcome> {
    if !name.starts_with("ui_") {
        return None;
    }
    // 停止のあいだの変化は、ハブが見ている（GUI のスレッドで読まない。D-087）。
    if name == "ui_changes" {
        return Some(Outcome::Done(ctx.request("netdbg.treeChanges", json!({})).unwrap_or_else(|e| Err(e.to_string()))));
    }
    // 観察専用のアタッチでは、画面に入力せず、窓も動かさない・閉じない（データと画面の状態を変えないため。D-089）。
    // 読む・撮る・待つはできる。
    if ctx.state.observe_only && matches!(name, "ui_invoke" | "ui_set_text" | "ui_select" | "ui_toggle" | "ui_keys" | "ui_mouse" | "ui_replay" | "ui_window") {
        return Some(Outcome::Done(Err(tr!("This session only observes (attached with observeOnly): input to the program's windows and moving or closing them are refused, so that the debugger changes no data. Reading, screenshots and waiting work."))));
    }
    // 手順を流すのは長くかかる（手ごとに相手が出るのを待つ）。
    let timeout = (name == "ui_replay").then_some(REPLAY_TIMEOUT);
    Some(match spawn(ctx, UiWork::Tool(name.to_string(), arguments.clone())) {
        Ok(job) => Outcome::Background(job, timeout),
        Err(e) => Outcome::Done(Err(e)),
    })
}

/// 一覧に出すコントロールの数の上限（既定）。
const MAX_ELEMENTS: usize = 300;
/// 操作のあと、ブレークポイントで止まらないか待つ時間（既定）。
const ACTION_WAIT_MS: u64 = 500;
/// `ui_wait_for` の既定の期限。
const UI_WAIT_MS: u64 = 10_000;
/// `ui_replay` の全体を待つ上限、手ごとに相手が出るのを待つ既定の時間、手のあとの既定の間。
const REPLAY_TIMEOUT: Duration = Duration::from_secs(600);
const STEP_TIMEOUT_MS: u64 = 5000;
const STEP_PAUSE_MS: u64 = 300;
/// `ui_wait_for` で、画面を見直す間隔。
pub(in crate::interface::mcp) const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// `ui_wait_for` の条件。
#[derive(Clone)]
pub(in crate::interface::mcp) struct UiQuery {
    name: Option<String>,
    text: Option<String>,
    kind: Option<String>,
    window: Option<usize>,
    /// appear / enabled / disappear
    state: String,
}

impl UiQuery {
    fn from(arguments: &Value) -> Result<UiQuery, String> {
        let get = |key: &str| arguments.get(key).and_then(Value::as_str).map(str::to_string);
        let query = UiQuery { name: get("name"), text: get("text"), kind: get("kind"), window: arguments.get("window").and_then(Value::as_u64).map(|w| w as usize), state: get("state").unwrap_or_else(|| "appear".into()) };
        if query.name.is_none() && query.text.is_none() && query.kind.is_none() {
            return Err(tr!("Specify at least one of name, text and kind."));
        }
        if !matches!(query.state.as_str(), "appear" | "enabled" | "disappear") {
            return Err(tr!("state must be appear, enabled or disappear."));
        }
        Ok(query)
    }

    fn matches(&self, element: &Element) -> bool {
        element.visible
            && element.depth > 0
            && self.window.is_none_or(|w| element.window == w)
            && self.name.as_ref().is_none_or(|n| element.name.as_ref() == Some(n))
            && self.kind.as_ref().is_none_or(|k| element.kind.eq_ignore_ascii_case(k))
            && self.text.as_ref().is_none_or(|t| element.text.as_ref().is_some_and(|text| text.to_lowercase().contains(&t.to_lowercase())))
    }
}

fn number(arguments: &Value, key: &str) -> Option<i32> {
    arguments.get(key).and_then(Value::as_i64).map(|v| v as i32)
}

fn center(rect: RECT) -> (i32, i32) {
    ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2)
}

/// 画面について覚えておくこと（入力のループの側）。GUI のスレッドの処理には写しを渡し、終わったら書き戻す。
#[derive(Default)]
pub(in crate::interface::mcp) struct UiMemory {
    /// `ui_tree` などの番号（mark）の順のコントロール（`Element::id`）。番号 n は `marks[n - 1]`。
    marks: Vec<usize>,
    /// 動いている間に読んだコントロールの名前。止まっている間（メッセージに答えない）の一覧に使う。
    names: HashMap<usize, String>,
    /// 動いている間に読んだ WPF の窓の中（UI Automation）。止まっている間の一覧に使う。
    automation_cache: HashMap<usize, Vec<Element>>,
    next_job: u64,
}

/// GUI のスレッドでの処理の材料（ループの側の状態の写し）。
struct UiSession {
    pid: u32,
    running: bool,
    marks: Vec<usize>,
    names: HashMap<usize, String>,
    automation_cache: HashMap<usize, Vec<Element>>,
}

/// GUI のスレッドで終わった処理。入力の列（`ClientInput::Ui`）でループに返す。
pub struct UiDone {
    job: u64,
    session: UiSession,
    answer: UiAnswer,
}

pub(in crate::interface::mcp) enum UiAnswer {
    Tool(Outcome),
    /// `ui_wait_for` の見直し。条件が満たされていれば `Some`。
    Poll(Result<Option<Value>, String>),
}

pub(in crate::interface::mcp) enum UiWork {
    Tool(String, Value),
    Poll(UiQuery),
}

/// 処理を GUI のスレッドで始め、処理の番号を返す。対象が動いているか止まっているときだけ。ループは待たない: 対象の UI の
/// スレッドは、デバッガーのイベント（モジュールの読み込みなど）のたびに、エンジンが続行するまで止まる。ループが GUI の
/// 答えを待つと、その続行も止まり、互いに待ってしまう。
pub(in crate::interface::mcp) fn spawn(ctx: &mut Context, work: UiWork) -> Result<u64, String> {
    let running = match ctx.state.target {
        Target::Running => true,
        Target::Stopped(_) => false,
        _ => return Err(tr!("{state}The windows can be used only while debugging.", state = ctx.state.not_running())),
    };
    let pid = ctx.state.process_id.ok_or_else(|| tr!("The target process is not known yet (it is starting). Wait a little and try again."))?;
    let memory = &mut *ctx.ui;
    let mut session = UiSession { pid: pid as u32, running, marks: memory.marks.clone(), names: memory.names.clone(), automation_cache: memory.automation_cache.clone() };
    let job = memory.next_job;
    memory.next_job += 1;
    gui::spawn(move || {
        let answer = match work {
            UiWork::Tool(name, arguments) => UiAnswer::Tool(session.run_tool(&name, &arguments)),
            UiWork::Poll(query) => UiAnswer::Poll(session.ui_poll(&query)),
        };
        crate::interface::post(crate::interface::ClientInput::Ui(Box::new(UiDone { job, session, answer })));
    });
    Ok(job)
}

/// GUI のスレッドの処理が終わった。番号、名前、WPF の窓の中を書き戻し、処理の番号と答えを返す。
pub(in crate::interface::mcp) fn finished(memory: &mut UiMemory, done: UiDone) -> (u64, UiAnswer) {
    let UiDone { job, session, answer } = done;
    memory.marks = session.marks;
    memory.names = session.names;
    memory.automation_cache = session.automation_cache;
    (job, answer)
}

impl UiSession {
    fn run_tool(&mut self, name: &str, arguments: &Value) -> Outcome {
        let outcome = match name {
            "ui_tree" => self.ui_tree(arguments, false).map(|v| Outcome::Done(Ok(v))),
            "ui_find" => self.ui_tree(arguments, true).map(|v| Outcome::Done(Ok(v))),
            "ui_screenshot" => self.ui_screenshot(arguments),
            "ui_wait_for" => self.ui_wait_for(arguments),
            "ui_invoke" | "ui_set_text" | "ui_select" | "ui_toggle" | "ui_keys" | "ui_mouse" | "ui_window" => self.ui_action(name, arguments),
            "ui_replay" => self.ui_replay(arguments),
            other => Err(tr!("Unknown tool: {name}", name = other)),
        };
        outcome.unwrap_or_else(|e| Outcome::Done(Err(e)))
    }

    /// 対象の窓とコントロール（前の窓から順）。番号（mark）を振り直し、名前と WPF の窓の中を覚えておく（止まっている間に使う）。
    fn ui_elements(&mut self, limit: usize) -> Result<(Snapshot, bool), String> {
        let (pid, running) = (self.pid, self.running);
        let mut shot = gui::snapshot(pid, running, limit);
        if running {
            self.names = shot.elements.iter().filter_map(|e| e.name.clone().map(|n| (e.id, n))).collect();
            self.automation_cache = shot.automation_windows.iter().map(|&w| (w, shot.elements.iter().filter(|e| e.depth > 0 && e.window == w).cloned().collect())).collect();
        } else {
            for element in &mut shot.elements {
                element.name = self.names.get(&element.id).cloned();
            }
            // WPF の窓の中は、前に読んだもの（座標は、止まってから窓が動いていなければ正しい）。
            let mut merged = Vec::with_capacity(shot.elements.len());
            for element in shot.elements.drain(..) {
                let cached = (element.depth == 0).then(|| self.automation_cache.get(&element.id)).flatten();
                merged.push(element);
                merged.extend(cached.into_iter().flatten().cloned());
            }
            shot.elements = merged;
        }
        shot.elements.retain(|e| e.visible);
        self.marks = shot.elements.iter().filter(|e| e.depth > 0).map(|e| e.id).collect();
        Ok((shot, running))
    }

    fn mark_of(&self, handle: usize) -> usize {
        self.marks.iter().position(|&h| h == handle).map_or(0, |i| i + 1)
    }

    /// `id` / `mark` / `name` / `text` で指されたコントロール（無ければ `None`）。
    fn ui_target(&self, list: &[Element], arguments: &Value, running: bool) -> Result<Option<Element>, String> {
        let window = arguments.get("window").and_then(Value::as_u64).map(|w| w as usize);
        let in_window = |e: &&Element| window.is_none_or(|w| e.window == w);
        let found = if let Some(id) = arguments.get("id").and_then(Value::as_u64) {
            list.iter().find(|e| e.id == id as usize).ok_or_else(|| tr!("There is no control with id {id} (it may have closed). Check with ui_tree again.", id = id))?
        } else if let Some(mark) = arguments.get("mark").and_then(Value::as_u64) {
            let handle = self.marks.get((mark as usize).wrapping_sub(1)).ok_or_else(|| tr!("There is no mark {mark}. Check with ui_tree again.", mark = mark))?;
            list.iter().find(|e| e.id == *handle).ok_or_else(|| tr!("The control with mark {mark} is gone.", mark = mark))?
        } else if let Some(name) = arguments.get("name").and_then(Value::as_str) {
            if !running && self.names.is_empty() {
                return Err(tr!("Names cannot be read while stopped. Point with id or mark, or debug_continue first."));
            }
            list.iter().filter(in_window).find(|e| e.depth > 0 && e.name.as_deref() == Some(name)).ok_or_else(|| {
                let names: Vec<&str> = list.iter().filter_map(|e| e.name.as_deref()).collect();
                tr!("There is no control named '{name}'. Names: {names}", name = name, names = names.join(", "))
            })?
        } else if let Some(text) = arguments.get("text").and_then(Value::as_str) {
            list.iter().filter(in_window).find(|e| e.depth > 0 && e.text.as_deref() == Some(text)).ok_or_else(|| tr!("There is no control with the text '{text}'.", text = text))?
        } else {
            return Ok(None);
        };
        Ok(Some(found.clone()))
    }

    /// 指定の窓（`window`）、またはいちばん前の窓。
    fn ui_window_of(&self, list: &[Element], arguments: &Value) -> Result<usize, String> {
        if let Some(window) = arguments.get("window").and_then(Value::as_u64) {
            return list.iter().find(|e| e.depth == 0 && e.id == window as usize).map(|e| e.id).ok_or_else(|| tr!("There is no window {window}. Check with ui_tree again.", window = window));
        }
        list.iter().find(|e| e.depth == 0).map(|e| e.id).ok_or_else(|| tr!("The target's window is not visible (not open yet, or minimized)."))
    }

    fn describe(&self, element: &Element) -> Value {
        element.describe(self.mark_of(element.id), capture::visible_rect(element.window))
    }

    fn ui_tree(&mut self, arguments: &Value, find: bool) -> Result<Value, String> {
        let limit = arguments.get("maxElements").and_then(Value::as_u64).map_or(MAX_ELEMENTS, |n| n.clamp(1, 2000) as usize);
        let (shot, running) = self.ui_elements(limit)?;
        let list = &shot.elements;
        let mut answer = if find {
            let query = UiQuery::from(&json!({ "name": arguments.get("name"), "text": arguments.get("text"), "kind": arguments.get("kind"), "window": arguments.get("window") }))?;
            json!({ "matches": list.iter().filter(|e| query.matches(e)).map(|e| self.describe(e)).collect::<Vec<_>>() })
        } else {
            let only = arguments.get("window").and_then(Value::as_u64).map(|w| w as usize);
            let windows: Vec<Value> = list
                .iter()
                .filter(|w| w.depth == 0 && only.is_none_or(|o| o == w.id))
                .map(|w| {
                    let visible = capture::visible_rect(w.id);
                    let (dpi_scale, program_scale) = gui::dpi_scales(w.id);
                    json!({
                        "window": w.id,
                        "title": w.text,
                        "kind": w.kind,
                        "name": w.name,
                        "screen": rect_json(visible),
                        "dpiScale": dpi_scale,
                        "programScale": program_scale,
                        "elements": list.iter().filter(|e| e.depth > 0 && e.window == w.id).map(|e| self.describe(e)).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({ "windows": windows })
        };
        if list.len() >= limit {
            answer["truncated"] = json!(tr!("Cut off at {limit}. Raise maxElements, or narrow it to a window with window.", limit = limit));
        }
        if !running {
            answer["note"] = json!(tr!("The target is stopped. Names and the inside of WPF windows are from an earlier read; WinForms text and checked cannot be read. Coordinates and pictures can be used."));
        }
        if !shot.unread.is_empty() {
            answer["unread"] = json!(shot.unread.iter().map(|(window, reason)| json!({ "window": window, "reason": reason })).collect::<Vec<_>>());
        }
        Ok(answer)
    }

    fn ui_screenshot(&mut self, arguments: &Value) -> Result<Outcome, String> {
        let (shot, _) = self.ui_elements(MAX_ELEMENTS)?;
        let list = &shot.elements;
        let scope = arguments.get("scope").and_then(Value::as_str).unwrap_or("window");
        let target = self.ui_target(list, arguments, true).ok().flatten();
        let window = match &target {
            Some(t) => t.window,
            None => self.ui_window_of(list, arguments)?,
        };
        let (windows, crop): (Vec<usize>, Option<RECT>) = match scope {
            "window" => (vec![window], None),
            "app" => (list.iter().filter(|e| e.depth == 0).map(|e| e.id).collect(), None),
            "control" => {
                let target = target.as_ref().ok_or_else(|| tr!("With scope control, point at a control with id, mark, name or text."))?;
                (vec![target.window], Some(target.rect))
            }
            "screen" => (Vec::new(), Some(gui::monitor_rect(window))),
            other => return Err(tr!("scope must be window, control, app or screen ({value}).", value = other)),
        };
        let annotate = arguments.get("annotate").and_then(Value::as_bool).unwrap_or(false);
        let marks: Vec<capture::Mark> = if annotate {
            list.iter()
                .filter(|e| e.depth > 0 && (windows.is_empty() || windows.contains(&e.window)))
                .filter(|e| crop.is_none_or(|c| capture::intersect(c, e.rect).is_some() && e.id != target.as_ref().map_or(0, |t| t.id)))
                .map(|e| capture::Mark {
                    rect: e.rect,
                    label: self.mark_of(e.id).to_string(),
                    // 一覧の窓は前から順。この窓より前の窓は、この窓の印を隠す。
                    hidden_by: list.iter().filter(|w| w.depth == 0).take_while(|w| w.id != e.window).map(|w| capture::visible_rect(w.id)).collect(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let shot = capture::capture(&windows, crop, &marks, gui::dpi_scales(window).0)?;
        let info = json!({
            "scope": scope,
            "window": window,
            "origin": { "x": shot.origin.0, "y": shot.origin.1 },
            "width": shot.width,
            "height": shot.height,
            "dpiScale": gui::dpi_scales(window).0,
            "programScale": gui::dpi_scales(window).1,
            "coordinates": "Pixel (x, y) in this image is screen point (origin.x + x, origin.y + y). With scope window, use ui_mouse relativeTo window with the image coordinates as they are.",
        });
        Ok(Outcome::Content(json!({
            "content": [
                { "type": "image", "data": crate::base64::encode(&shot.png), "mimeType": "image/png" },
                { "type": "text", "text": serde_json::to_string_pretty(&info).unwrap_or_default() },
            ]
        })))
    }

    fn ui_wait_for(&mut self, arguments: &Value) -> Result<Outcome, String> {
        let query = UiQuery::from(arguments)?;
        if !self.running {
            return Err(tr!("The target is stopped. debug_continue before waiting."));
        }
        if let Some(found) = self.ui_poll(&query)? {
            return Ok(Outcome::Done(Ok(found)));
        }
        let wait = arguments.get("timeoutMs").and_then(Value::as_u64).unwrap_or(UI_WAIT_MS).min(MAX_WAIT_MS);
        Ok(Outcome::WaitForUi(Duration::from_millis(wait), query))
    }

    /// 条件が満たされていれば、その答え。
    fn ui_poll(&mut self, query: &UiQuery) -> Result<Option<Value>, String> {
        let (shot, _) = self.ui_elements(MAX_ELEMENTS)?;
        let list = &shot.elements;
        let found: Vec<&Element> = list.iter().filter(|e| query.matches(e)).collect();
        let satisfied = match query.state.as_str() {
            "disappear" => found.is_empty(),
            "enabled" => found.iter().any(|e| e.enabled),
            _ => !found.is_empty(),
        };
        Ok(satisfied.then(|| json!({ "found": query.state != "disappear", "state": query.state, "elements": found.iter().map(|e| self.describe(e)).collect::<Vec<_>>() })))
    }

    /// 画面を操作するツール。操作のあと `waitMs` 待ち、止まればその場所を返す。
    fn ui_action(&mut self, name: &str, arguments: &Value) -> Result<Outcome, String> {
        let (shot, running) = self.ui_elements(MAX_ELEMENTS)?;
        let list = &shot.elements;
        if !running {
            return Err(tr!("The target is stopped, so its windows cannot be operated (stopped windows do not answer input). debug_continue first."));
        }
        let target = self.ui_target(list, arguments, running)?;
        let need_target = || target.clone().ok_or_else(|| tr!("Point at a control with id, mark, name or text."));
        // WPF のコントロール（窓ではない）は、UI Automation で操作する。
        let automation = target.as_ref().and_then(|t| shot.automation_element(t.id));
        let mut done = json!({ "action": name });
        match name {
            "ui_invoke" => {
                let t = need_target()?;
                if !t.enabled {
                    return Err(tr!("The control is disabled (Enabled = false), so it cannot be pressed."));
                }
                match automation {
                    Some(element) => gui::uia::invoke(element)?,
                    None if matches!(t.kind, "Button" | "CheckBox" | "RadioButton") => gui::click(t.id)?,
                    None => {
                        let (x, y) = center(t.rect);
                        return Err(tr!("{kind} is not a control that can be pressed. Click it with ui_mouse (its center is ({x}, {y}) on the screen).", kind = t.kind, x = x, y = y));
                    }
                }
                done["target"] = self.describe(&t);
            }
            "ui_set_text" => {
                let t = need_target()?;
                let text = arguments.get("value").and_then(Value::as_str).ok_or_else(|| tr!("Specify value (the text to set)."))?;
                match automation {
                    Some(element) => gui::uia::set_text(element, text)?,
                    None => gui::set_text(t.id, text)?,
                }
                done["target"] = self.describe(&t);
            }
            "ui_select" => {
                let t = need_target()?;
                let item = arguments.get("item").and_then(Value::as_str).map(str::to_string);
                let index = arguments.get("index").and_then(Value::as_u64).map(|i| i as usize);
                match (automation, &shot.automation) {
                    (Some(element), Some(client)) => client.select(element, item.as_deref(), index)?,
                    _ => gui::select(t.id, t.kind, item.as_deref(), index)?,
                }
                done["target"] = self.describe(&t);
            }
            "ui_toggle" => {
                let t = need_target()?;
                if !matches!(t.kind, "CheckBox" | "RadioButton") {
                    return Err(tr!("{kind} is not a check box.", kind = t.kind));
                }
                let wanted = arguments.get("checked").and_then(Value::as_bool);
                if wanted.is_some() && wanted == t.checked {
                    return Ok(Outcome::Done(Ok(json!({ "action": name, "changed": false, "target": self.describe(&t) }))));
                }
                match automation {
                    Some(element) => gui::uia::toggle(element)?,
                    None => gui::click(t.id)?,
                }
                done["target"] = self.describe(&t);
            }
            "ui_keys" => {
                let keys = gui::parse_keys(arguments.get("keys").and_then(Value::as_str).ok_or_else(|| tr!("Specify keys."))?)?;
                let window = match &target {
                    Some(t) => t.window,
                    None => self.ui_window_of(list, arguments)?,
                };
                gui::bring_to_front(window)?;
                match (automation, &target) {
                    (Some(element), _) => gui::uia::focus(element)?,
                    (None, Some(t)) => gui::focus(t.id)?,
                    (None, None) => {}
                }
                gui::send_keys(&keys)?;
            }
            "ui_mouse" => {
                let action = match arguments.get("action").and_then(Value::as_str).unwrap_or("click") {
                    "click" => gui::MouseAction::Click,
                    "double_click" => gui::MouseAction::DoubleClick,
                    "right_click" => gui::MouseAction::RightClick,
                    "move" => gui::MouseAction::Move,
                    "down" => gui::MouseAction::Down,
                    "up" => gui::MouseAction::Up,
                    "wheel" => gui::MouseAction::Wheel(number(arguments, "clicks").unwrap_or(-1)),
                    "drag" => gui::MouseAction::Drag { to_x: 0, to_y: 0 },
                    other => return Err(tr!("action must be click, double_click, right_click, move, down, up, drag or wheel ({value}).", value = other)),
                };
                let relative = arguments.get("relativeTo").and_then(Value::as_str).unwrap_or(if target.is_some() { "control" } else { "window" });
                let window = match &target {
                    Some(t) => t.window,
                    None => self.ui_window_of(list, arguments)?,
                };
                let base = match relative {
                    "window" => capture::visible_rect(window),
                    "control" => need_target()?.rect,
                    "screen" => RECT::default(),
                    other => return Err(tr!("relativeTo must be window, control or screen ({value}).", value = other)),
                };
                // 座標が無ければ、コントロールの中心。
                let (x, y) = match (number(arguments, "x"), number(arguments, "y")) {
                    (Some(x), Some(y)) => (base.left + x, base.top + y),
                    (None, None) if relative == "control" => center(base),
                    _ => return Err(tr!("Specify x and y (with a control, leave them out for its center).")),
                };
                let action = match action {
                    gui::MouseAction::Drag { .. } => {
                        let (Some(to_x), Some(to_y)) = (number(arguments, "toX"), number(arguments, "toY")) else {
                            return Err(tr!("drag needs toX and toY (relative to the same base as x and y)."));
                        };
                        gui::MouseAction::Drag { to_x: base.left + to_x, to_y: base.top + to_y }
                    }
                    other => other,
                };
                gui::bring_to_front(window)?;
                gui::mouse(action, x, y)?;
                done["screen"] = json!({ "x": x, "y": y });
            }
            "ui_window" => {
                let window = self.ui_window_of(list, arguments)?;
                let action = arguments.get("action").and_then(Value::as_str).unwrap_or("front").to_string();
                let (x, y, width, height) = (number(arguments, "x"), number(arguments, "y"), number(arguments, "width"), number(arguments, "height"));
                match action.as_str() {
                    "front" => gui::bring_to_front(window),
                    "move" => gui::move_window(window, x, y, width, height),
                    "close" => gui::close_window(window),
                    other => Err(tr!("action must be front, move or close ({value}).", value = other)),
                }?;
                done["window"] = json!(window);
            }
            _ => unreachable!(),
        }
        let wait = arguments.get("waitMs").and_then(Value::as_u64).unwrap_or(ACTION_WAIT_MS).min(MAX_WAIT_MS);
        Ok(Outcome::AfterAction(Duration::from_millis(wait), done))
    }

    /// 手順を流す（D-086）。手ごとに相手が出て押せるようになるのを待ち、`ui_action` と同じ処理で操作する。
    fn ui_replay(&mut self, arguments: &Value) -> Result<Outcome, String> {
        let steps = replay_steps(arguments)?;
        if steps.is_empty() {
            return Err(tr!("There are no steps to replay."));
        }
        if !self.running {
            return Err(tr!("The target is stopped, so its windows cannot be operated (stopped windows do not answer input). debug_continue first."));
        }
        let timeout = Duration::from_millis(arguments.get("timeoutMs").and_then(Value::as_u64).unwrap_or(STEP_TIMEOUT_MS).min(MAX_WAIT_MS));
        let pause = Duration::from_millis(arguments.get("pauseMs").and_then(Value::as_u64).unwrap_or(STEP_PAUSE_MS).min(10_000));
        let mut results = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            if let Err(error) = self.replay_step(step, timeout) {
                results.push(json!({ "step": index + 1, "action": step, "ok": false, "error": error }));
                let done = json!({ "action": "ui_replay", "completed": index, "total": steps.len(), "results": results });
                return Err(tr!("Step {n} of {total} failed: {error}\n{results}", n = index + 1, total = steps.len(), error = error, results = serde_json::to_string_pretty(&done).unwrap_or_default()));
            }
            results.push(json!({ "step": index + 1, "action": step, "ok": true }));
            std::thread::sleep(pause);
        }
        // 最後の手のあとも少し見て、止まればその場所を返す（`ui_action` と同じ）。
        Ok(Outcome::AfterAction(pause, json!({ "action": "ui_replay", "completed": steps.len(), "total": steps.len(), "results": results })))
    }

    /// 手順 1 つ。相手が見つかり、押せる（Enabled）まで待ってから操作する。窓が答えなければ（止まった、忙しい）やめる。
    fn replay_step(&mut self, step: &Value, timeout: Duration) -> Result<(), String> {
        let (tool, mut arguments) = crate::report::steps::split(step)?;
        // 止まったかは、手ごとに窓が答えるかで見る（操作のあとに待たない）。
        arguments["waitMs"] = json!(0);
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let (shot, _) = self.ui_elements(MAX_ELEMENTS)?;
            let found = self.ui_target(&shot.elements, &arguments, true);
            let ready = match &found {
                Ok(Some(target)) => target.enabled,
                Ok(None) => true,
                Err(_) => false,
            };
            if ready {
                let window = match &found {
                    Ok(Some(target)) => Some(target.window),
                    _ => shot.elements.iter().find(|e| e.depth == 0).map(|e| e.id),
                };
                if window.is_some_and(|w| !gui::responds(w)) {
                    return Err(tr!("The program's window does not answer (it may have stopped at a breakpoint, or be busy)."));
                }
                return self.ui_action(&tool, &arguments).map(|_| ());
            }
            if std::time::Instant::now() >= deadline {
                return Err(match found {
                    Err(e) => e,
                    _ => tr!("The control is disabled (Enabled = false), so it cannot be pressed."),
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// 流す手順（`steps`、または `file` の手順。`item` があればその項目だけ）。
fn replay_steps(arguments: &Value) -> Result<Vec<Value>, String> {
    if let Some(steps) = arguments.get("steps").and_then(Value::as_array) {
        return Ok(steps.clone());
    }
    let path = arguments.get("file").and_then(Value::as_str).ok_or_else(|| tr!("Give steps, or file (a steps file written by test_report)."))?;
    let text = std::fs::read_to_string(path).map_err(|e| tr!("Cannot read {path}: {error}", path = path, error = e))?;
    let file: Value = serde_json::from_str(&text).map_err(|e| tr!("{path} is not a steps file: {error}", path = path, error = e))?;
    crate::report::steps::pick(&file, arguments.get("item").and_then(Value::as_str))
}