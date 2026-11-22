//! 対象のアプリの GUI（WinForms。WPF の窓の中は `uia`）を、外から読んで操作する。MCP の `ui_*` のツールが使う（D-053、D-054、D-055）。
//!
//! WinForms のコントロールは、1 つずつが Win32 の窓（HWND）なので、Win32 だけで扱える（UI Automation の COM は使わない）。
//! - 名前（Control.Name）は、WinForms が答える登録メッセージ `WM_GETCONTROLNAME` で読む（対象のメモリに置いた入れ物に
//!   書いてもらい、読み戻す）。文字は `WM_GETTEXT`。どちらも、対象が動いているときだけ答えが返る。止まっている間は、
//!   呼ぶ側（mcp/front.rs）がデバッガーの式で読む。
//! - 押す・入力・選ぶは、ボタンやテキストボックスへのメッセージ（BM_CLICK、WM_SETTEXT、CB_SETCURSEL と通知）。
//!   座標でのマウスとキーは、実際の入力（SendInput）。
//! - 座標はすべて物理ピクセル（Per-Monitor DPI 対応にしたスレッドから呼ぶ。`dpi_aware`）。

pub mod capture;
pub mod ffi;
pub mod png;
pub mod recorder;
mod accessible;
pub mod uia;
pub mod watch;

use crate::win32::{wide, OwnedHandle, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use ffi::*;
use serde_json::{json, Value};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;

/// メッセージの答えを待つ時間（ミリ秒）。止まっている対象で長く待たない。
const MESSAGE_TIMEOUT: u32 = 300;

// このスレッドで、メッセージが時間切れになった（それ以降のメッセージは送らない）。`snapshot` の始めに戻す。
thread_local! {
    static MESSAGE_FAILED: Cell<bool> = const { Cell::new(false) };
}

fn reset_message_failure() {
    MESSAGE_FAILED.with(|failed| failed.set(false));
}

fn message_failed() -> bool {
    MESSAGE_FAILED.with(|failed| failed.get())
}

/// コントロール（またはトップレベルの窓）1 つ。
#[derive(Clone, Debug)]
pub struct Element {
    /// 窓のハンドル（WinForms のコントロールと窓）、または UI Automation の要素の番号（WPF のコントロール。`uia::element_id`）。
    pub id: usize,
    /// トップレベルの窓。
    pub window: usize,
    pub depth: usize,
    pub kind: &'static str,
    /// WinForms のコントロールなら、.NET の型の名前（`System.Windows.Forms.CheckBox` など。アセンブリの名前は除く）。
    pub type_name: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub visible: bool,
    pub enabled: bool,
    pub checked: Option<bool>,
    /// 画面の座標（物理ピクセル）。
    pub rect: RECT,
}

impl Element {
    /// MCP に返す形。`window_rect` は、トップレベルの窓の見えている範囲（`ui_screenshot` の窓の画像の左上）。
    pub fn describe(&self, mark: usize, window_rect: RECT) -> Value {
        let mut value = json!({
            "mark": mark,
            "id": self.id,
            "kind": self.kind,
            "enabled": self.enabled,
            "screen": rect_json(self.rect),
            "inWindow": rect_json(RECT { left: self.rect.left - window_rect.left, top: self.rect.top - window_rect.top, right: self.rect.right - window_rect.left, bottom: self.rect.bottom - window_rect.top }),
        });
        // 無いもの（読めなかった名前、空の文字）は出さない（AI に渡す量を減らす）。
        if let Some(name) = &self.name {
            value["name"] = json!(name);
        }
        if let Some(type_name) = &self.type_name {
            value["type"] = json!(type_name);
        }
        if let Some(text) = self.text.as_ref().filter(|t| !t.is_empty()) {
            value["text"] = json!(text);
        }
        if let Some(checked) = self.checked {
            value["checked"] = json!(checked);
        }
        if self.depth > 0 {
            value["depth"] = json!(self.depth);
        }
        value
    }
}

pub fn rect_json(rect: RECT) -> Value {
    json!({ "x": rect.left, "y": rect.top, "width": rect.right - rect.left, "height": rect.bottom - rect.top })
}

/// GUI の処理を、座標が物理ピクセルのスレッドで行う（待たない。終わりは `work` が知らせる）。写すための窓も、
/// このスレッドで作って壊す。MSAA（COM）は、このスレッドの中だけで使う。
pub fn spawn(work: impl FnOnce() + Send + 'static) {
    // プロセスも Per-Monitor DPI 対応にする。スレッドだけでは、GetDpiForMonitor がプロセスの設定（既定は非対応）を見て、
    // どのモニターも 96 DPI と答える。エンジンは窓を持たない（写すための窓だけ）ので、ほかに影響しない。
    static PROCESS_AWARE: std::sync::Once = std::sync::Once::new();
    PROCESS_AWARE.call_once(|| unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
    std::thread::spawn(move || {
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let com = accessible::Apartment::enter();
        work();
        drop(com);
    });
}

fn hwnd(handle: usize) -> HWND {
    handle as HWND
}

fn class_name(handle: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(handle, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// クラス名とスタイルから、コントロールの種類（WinForms の型に近い名前）。
pub fn kind_of(class: &str, style: u32, top_level: bool) -> &'static str {
    let upper = class.to_ascii_uppercase();
    // WinForms のクラス名は `WindowsForms10.<もとのクラス>.app.…`。
    let base = upper.strip_prefix("WINDOWSFORMS10.").map(|rest| rest.split('.').next().unwrap_or("")).unwrap_or(upper.as_str());
    match base {
        "BUTTON" => match style & 0xF {
            0x2 | 0x3 | 0x5 | 0x6 => "CheckBox",
            0x4 | 0x9 => "RadioButton",
            0x7 => "GroupBox",
            _ => "Button",
        },
        "EDIT" => "TextBox",
        "COMBOBOX" => "ComboBox",
        "LISTBOX" => "ListBox",
        "STATIC" => "Label",
        "SYSLISTVIEW32" => "ListView",
        "SYSTREEVIEW32" => "TreeView",
        "SYSTABCONTROL32" => "TabControl",
        "MSCTLS_PROGRESS32" => "ProgressBar",
        "MSCTLS_TRACKBAR32" => "TrackBar",
        "MSCTLS_UPDOWN32" => "UpDown",
        "SCROLLBAR" => "ScrollBar",
        "SYSDATETIMEPICK32" => "DateTimePicker",
        "#32770" => "Dialog",
        _ if base.starts_with("RICHEDIT") => "RichTextBox",
        "WINDOW" if top_level => "Form",
        "WINDOW" => "Control",
        _ if top_level => "Window",
        _ => "Other",
    }
}

/// 対象のプロセスの、見えているトップレベルの窓。
pub fn top_windows(process_id: u32) -> Vec<usize> {
    struct Search {
        process_id: u32,
        found: Vec<usize>,
    }
    unsafe extern "system" fn each(handle: HWND, data: LPARAM) -> i32 {
        let search = &mut *(data as *mut Search);
        let mut owner = 0u32;
        GetWindowThreadProcessId(handle, &mut owner);
        if owner == search.process_id && IsWindowVisible(handle) != 0 {
            search.found.push(handle as usize);
        }
        1
    }
    let mut search = Search { process_id, found: Vec::new() };
    unsafe { EnumWindows(each, &mut search as *mut Search as LPARAM) };
    search.found
}

fn children(parent: usize) -> Vec<usize> {
    unsafe extern "system" fn each(handle: HWND, data: LPARAM) -> i32 {
        (*(data as *mut Vec<usize>)).push(handle as usize);
        1
    }
    let mut all = Vec::new();
    unsafe { EnumChildWindows(hwnd(parent), each, &mut all as *mut Vec<usize> as LPARAM) };
    // EnumChildWindows は孫も返す。直接の子だけにする（木の形は呼ぶ側でたどる）。
    all.into_iter().filter(|&child| unsafe { GetParent(hwnd(child)) } as usize == parent).collect()
}

pub fn window_rect(handle: usize) -> RECT {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd(handle), &mut rect) };
    rect
}

/// 画面の一覧と、その中の UI Automation の要素の参照（一覧を読んだ GUI のスレッドの中で、操作に使う）。
pub struct Snapshot {
    pub elements: Vec<Element>,
    pub automation: Option<uia::Automation>,
    pub handles: uia::Handles,
    /// UI Automation で中を読む窓（WPF の窓。止まっていて読まなかったものも）。
    pub automation_windows: Vec<usize>,
    /// UI Automation で中を読めなかった窓と、その理由。
    pub unread: Vec<(usize, String)>,
    /// メッセージが時間切れになった（木は途中まで。見張りは、この回を覚えずに前の木を残す）。
    pub failed: bool,
}

impl Snapshot {
    /// 要素の UI Automation の参照（WPF のコントロールなら）。
    pub fn automation_element(&self, id: usize) -> Option<&crate::com::ComPtr<crate::uiautomation::IUIAutomationElement>> {
        self.handles.get(&id)
    }
}

/// 対象のプロセスの窓とコントロールを、木の順（深さつき）で並べる。`read_messages` が false なら、名前と文字を
/// メッセージで読まず、WPF の窓の中も読まない（対象が止まっていて答えられないとき）。
pub fn snapshot(process_id: u32, read_messages: bool, max: usize) -> Snapshot {
    reset_message_failure();
    let reader = if read_messages { WinFormsReader::open(process_id) } else { None };
    let mut list = Vec::new();
    let (mut automation, mut handles, mut unread, mut automation_windows) = (None::<uia::Automation>, uia::Handles::new(), Vec::new(), Vec::new());
    'windows: for window in top_windows(process_id) {
        let mut stack = vec![(window, 0usize)];
        while let Some((handle, depth)) = stack.pop() {
            if list.len() >= max {
                break 'windows;
            }
            let class = class_name(hwnd(handle));
            list.push(read_element(handle, window, depth, reader.as_ref(), read_messages));
            // WPF の窓の中は、窓ではない。UI Automation で読む（動いているときだけ）。
            if depth == 0 && uia::is_automation_window(&class) {
                automation_windows.push(handle);
                // 見出しのメッセージが時間切れなら、UI Automation も待たない（止まったスレッドへの 2 秒）。
                if read_messages && !message_failed() {
                    if automation.is_none() {
                        match uia::Automation::new() {
                            Ok(created) => automation = Some(created),
                            Err(e) => unread.push((handle, e)),
                        }
                    }
                    if let Some(automation) = &automation {
                        if let Err(e) = automation.read_window(handle, max, &mut list, &mut handles) {
                            unread.push((handle, e));
                        }
                    }
                }
                continue;
            }
            // 子は、見た目の並び（上から、左から）で。スタックなので逆に積む。
            let mut kids: Vec<usize> = children(handle);
            kids.sort_by_key(|&k| {
                let r = window_rect(k);
                (r.top, r.left)
            });
            for kid in kids.into_iter().rev() {
                stack.push((kid, depth + 1));
            }
        }
    }
    Snapshot { elements: list, automation, handles, automation_windows, unread, failed: message_failed() }
}

/// コントロール（か窓）1 つを読む。`read_messages` が false なら、名前と文字を読まない（止まっている対象）。
fn read_element(handle: usize, window: usize, depth: usize, reader: Option<&WinFormsReader>, read_messages: bool) -> Element {
    let h = hwnd(handle);
    let class = class_name(h);
    let style = unsafe { GetWindowLongW(h, GWL_STYLE) } as u32;
    let mut kind = kind_of(&class, style, depth == 0);
    let mut checked = None;
    // ボタンの仲間は、スタイルだけでは見分けられない（WinForms はオーナー描画にする）。MSAA の役割と状態で見る。
    // 先に答えがあるかを見る。止まっている UI スレッドへの MSAA は、時間切れが無くて長く待つ。
    if read_messages && !message_failed() && matches!(kind, "Button" | "CheckBox" | "RadioButton") && responds(handle) {
        if let Some((role, state)) = accessible::role_and_state(handle) {
            kind = accessible::button_kind(role).unwrap_or(kind);
            checked = matches!(kind, "CheckBox" | "RadioButton").then_some(state & accessible::STATE_SYSTEM_CHECKED != 0);
        }
    }
    Element {
        id: handle,
        window,
        depth,
        kind,
        name: reader.and_then(|r| r.read(handle, r.name_message)),
        type_name: reader.and_then(|r| r.read(handle, r.type_message)).map(|t| t.split(',').next().unwrap_or_default().to_string()),
        text: if read_messages { text_of(handle, kind) } else { None },
        visible: unsafe { IsWindowVisible(h) } != 0,
        enabled: unsafe { IsWindowEnabled(h) } != 0,
        checked,
        rect: window_rect(handle),
    }
}

/// 窓のプロセスの番号。
pub fn process_of(handle: usize) -> u32 {
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd(handle), &mut process_id) };
    process_id
}

/// 対象のプロセスのコントロール 1 つを、動いている前提で読む（手の操作の記録。名前、種類、文字、チェック）。
pub fn element_of(process_id: u32, handle: usize) -> Option<Element> {
    if handle == 0 || process_of(handle) != process_id {
        return None;
    }
    let window = unsafe { GetAncestor(hwnd(handle), GA_ROOT) } as usize;
    let reader = WinFormsReader::open(process_id);
    Some(read_element(handle, window, usize::from(window != handle), reader.as_ref(), true))
}

/// パスワードの欄か（文字を記録に残さない）。
pub fn is_password(handle: usize) -> bool {
    let style = unsafe { GetWindowLongW(hwnd(handle), GWL_STYLE) } as u32;
    class_name(hwnd(handle)).to_ascii_uppercase().contains("EDIT") && style & ES_PASSWORD != 0
}

/// いま手前にある窓が対象のものなら、その中のフォーカスのあるコントロール。
pub fn focused_control(process_id: u32) -> Option<usize> {
    let foreground = unsafe { GetForegroundWindow() } as usize;
    if foreground == 0 || process_of(foreground) != process_id {
        return None;
    }
    let thread = unsafe { GetWindowThreadProcessId(hwnd(foreground), null_mut()) };
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 || info.hwndFocus.is_null() {
        return Some(foreground);
    }
    Some(info.hwndFocus as usize)
}

/// 画面の座標にある対象のコントロール。コンボボックスの開いた一覧（`ComboLBox`）を押したなら、そのコンボボックス
/// （閉じたあとフォーカスを持つ）。対象の窓でなければ `None`。
pub fn control_at(process_id: u32, x: i32, y: i32) -> Option<usize> {
    let handle = unsafe { WindowFromPoint(POINT { x, y }) } as usize;
    if handle == 0 || process_of(handle) != process_id {
        return None;
    }
    if class_name(hwnd(handle)).eq_ignore_ascii_case("ComboLBox") {
        return focused_control(process_id);
    }
    Some(handle)
}

/// GUI のスレッド（`spawn`。座標が物理ピクセル）で `work` を動かし、答えを待つ。
pub fn on_gui_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, timeout: std::time::Duration) -> Result<T, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    spawn(move || {
        let _ = sender.send(work());
    });
    receiver.recv_timeout(timeout).map_err(|_| tr!("The target's windows do not answer (its UI thread may be stopped)."))
}

/// 窓が持っている文字（メッセージを送らないので、止まっている対象でも読める。ラベルやボタンの文字。テキストボックスの
/// 中身は、コントロールの中にあるので読めない）。
pub fn text_without_messages(window: usize) -> String {
    let mut buffer = [0u16; 1024];
    let length = unsafe { InternalGetWindowText(hwnd(window), buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// 窓が有効か、表示されているか（メッセージを送らない）。
pub fn window_state(window: usize) -> (bool, bool) {
    unsafe { (IsWindowEnabled(hwnd(window)) != 0, IsWindowVisible(hwnd(window)) != 0) }
}

/// 窓の UI のスレッドが答えるか（ブレークポイントで止まっている、忙しいなら false）。`WM_NULL` は何もしないメッセージ。
pub fn responds(window: usize) -> bool {
    const WM_NULL: u32 = 0;
    send(window, WM_NULL, 0, 0).is_some()
}

/// メッセージを送り、答えを返す（時間切れ・答えられないなら `None`）。
fn send(handle: usize, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<usize> {
    // 1 つ時間切れになったら、残りのコントロールにも送らない（止まっている UI スレッドへ、コントロールの数だけ待たない）。
    if message_failed() {
        return None;
    }
    let mut result = 0usize;
    let ok = unsafe { SendMessageTimeoutW(hwnd(handle), message, wparam, lparam, SMTO_ABORTIFHUNG, MESSAGE_TIMEOUT, &mut result) };
    if ok == 0 {
        MESSAGE_FAILED.with(|failed| failed.set(true));
        return None;
    }
    Some(result)
}

/// コントロールの文字。コンボボックスとリストは、選ばれた項目の文字。
fn text_of(handle: usize, kind: &str) -> Option<String> {
    let read = |length_message: u32, text_message: u32, index: usize| -> Option<String> {
        let length = send(handle, length_message, index, 0)?;
        if length == 0 || length > 1 << 20 {
            return Some(String::new());
        }
        let mut buffer = vec![0u16; length + 1];
        // WM_GETTEXT は、別のプロセスの窓でも、入れ物を Windows が受け渡してくれる。
        let (wparam, lparam) = if text_message == WM_GETTEXT { (buffer.len(), buffer.as_mut_ptr() as LPARAM) } else { (index, buffer.as_mut_ptr() as LPARAM) };
        let copied = send(handle, text_message, wparam, lparam)?;
        Some(String::from_utf16_lossy(&buffer[..copied.min(length)]))
    };
    match kind {
        "ListBox" => {
            let index = send(handle, LB_GETCURSEL, 0, 0)?;
            if index as isize == -1 {
                return Some(String::new());
            }
            read(LB_GETTEXTLEN, LB_GETTEXT, index)
        }
        _ => read(WM_GETTEXTLENGTH, WM_GETTEXT, 0),
    }
}

/// WinForms が答える登録メッセージで、コントロールの Name（`WM_GETCONTROLNAME`）と型（`WM_GETCONTROLTYPE`）を読む。
/// 答えを書く入れ物は、対象のプロセスのメモリに置く（このメッセージの lParam は、対象の側の番地）。
struct WinFormsReader {
    process: OwnedHandle,
    remote: *mut c_void,
    name_message: u32,
    type_message: u32,
}

const NAME_BUFFER_CHARS: usize = 1024;

impl WinFormsReader {
    fn open(process_id: u32) -> Option<WinFormsReader> {
        let access = PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_LIMITED_INFORMATION;
        let process = OwnedHandle::new(unsafe { OpenProcess(access, 0, process_id) })?;
        let remote = unsafe { VirtualAllocEx(process.raw(), null_mut(), NAME_BUFFER_CHARS * 2, MEM_COMMIT_RESERVE, PAGE_READWRITE) };
        if remote.is_null() {
            return None;
        }
        let name_message = unsafe { RegisterWindowMessageW(wide("WM_GETCONTROLNAME").as_ptr()) };
        let type_message = unsafe { RegisterWindowMessageW(wide("WM_GETCONTROLTYPE").as_ptr()) };
        Some(WinFormsReader { process, remote, name_message, type_message })
    }

    fn read(&self, handle: usize, message: u32) -> Option<String> {
        // 答えは、書いた文字の数（終わりの 0 を含む）。WinForms でなければ 0。
        let written = send(handle, message, NAME_BUFFER_CHARS, self.remote as LPARAM)?;
        if written == 0 || written > NAME_BUFFER_CHARS {
            return None;
        }
        let mut buffer = vec![0u16; written];
        let mut read = 0usize;
        let ok = unsafe { ReadProcessMemory(self.process.raw(), self.remote, buffer.as_mut_ptr() as *mut c_void, written * 2, &mut read) };
        if ok == 0 {
            return None;
        }
        let name = String::from_utf16_lossy(&buffer).trim_end_matches('\0').to_string();
        (!name.is_empty()).then_some(name)
    }
}

impl Drop for WinFormsReader {
    fn drop(&mut self) {
        unsafe { VirtualFreeEx(self.process.raw(), self.remote, 0, MEM_RELEASE) };
    }
}

/// 押す（ボタン、チェックボックス、ラジオボタン）。答えを待たずに置く（ハンドラーがブレークポイントで止まっても、
/// こちらは固まらない）。
pub fn click(handle: usize) -> Result<(), String> {
    if unsafe { PostMessageW(hwnd(handle), BM_CLICK, 0, 0) } == 0 {
        return Err(tr!("Could not press it (the window may be gone)."));
    }
    Ok(())
}

/// 文字を入れる（テキストボックス）。WinForms は WM_SETTEXT で Text を変え、TextChanged を起こす。
pub fn set_text(handle: usize, text: &str) -> Result<(), String> {
    let text = wide(text);
    // 答えを待ちすぎない（TextChanged のハンドラーがブレークポイントで止まると、答えが返らない）。止まったかは呼ぶ側が見る。
    let mut result = 0usize;
    unsafe { SendMessageTimeoutW(hwnd(handle), WM_SETTEXT, 0, text.as_ptr() as LPARAM, SMTO_ABORTIFHUNG, 1000, &mut result) };
    Ok(())
}

/// コンボボックス・リストの項目を選び、親に変わったことを知らせる（WinForms の SelectedIndexChanged が起きる）。
pub fn select(handle: usize, kind: &str, item: Option<&str>, index: Option<usize>) -> Result<(), String> {
    let (count_message, length_message, text_message, set_message, notify) = match kind {
        "ComboBox" => (CB_GETCOUNT, CB_GETLBTEXTLEN, CB_GETLBTEXT, CB_SETCURSEL, CBN_SELCHANGE),
        "ListBox" => (LB_GETCOUNT, LB_GETTEXTLEN, LB_GETTEXT, LB_SETCURSEL, LBN_SELCHANGE),
        other => return Err(tr!("{kind} cannot be selected from. Point at a combo box or list box.", kind = other)),
    };
    let count = send(handle, count_message, 0, 0).ok_or_else(|| tr!("Cannot read the number of items (is the target stopped?)."))?;
    let mut items = Vec::new();
    for i in 0..count.min(1000) {
        let length = send(handle, length_message, i, 0).unwrap_or(0);
        let mut buffer = vec![0u16; length + 1];
        let copied = send(handle, text_message, i, buffer.as_mut_ptr() as LPARAM).unwrap_or(0);
        items.push(String::from_utf16_lossy(&buffer[..copied.min(length)]));
    }
    let chosen = match (item, index) {
        (_, Some(index)) if index < items.len() => index,
        (Some(item), _) => items.iter().position(|i| i == item).ok_or_else(|| tr!("There is no item '{item}'. Items: {items}", item = item, items = items.join(", ")))?,
        _ => return Err(tr!("Specify item (the item's text) or index. Items: {items}", items = items.join(", "))),
    };
    send(handle, set_message, chosen, 0);
    let parent = unsafe { GetParent(hwnd(handle)) };
    let id = unsafe { GetDlgCtrlID(hwnd(handle)) } as usize & 0xFFFF;
    unsafe { PostMessageW(parent, WM_COMMAND, (notify as usize) << 16 | id, handle as LPARAM) };
    Ok(())
}

/// 窓を前に出す。ほかのプロセスの窓は、ふつうは前に出せない（前にいるプロセスだけが許される）ので、いま前にある
/// 窓のスレッドの入力に一時的につないでから頼む（Alt キーを送るやり方は、前の窓のメニューを開いてしまう）。
pub fn bring_to_front(window: usize) -> Result<(), String> {
    let h = hwnd(window);
    unsafe {
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        let foreground = GetForegroundWindow();
        if foreground == h {
            return Ok(());
        }
        let me = GetCurrentThreadId();
        let theirs = GetWindowThreadProcessId(foreground, null_mut());
        let attached = theirs != 0 && theirs != me && AttachThreadInput(me, theirs, 1) != 0;
        SetForegroundWindow(h);
        BringWindowToTop(h);
        if attached {
            AttachThreadInput(me, theirs, 0);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        if GetForegroundWindow() != h {
            return Err(tr!("Could not bring the window to the front (Windows did not allow it). Click the window once and try again."));
        }
    }
    Ok(())
}

/// 窓を動かす。位置と大きさは、見えている範囲（`ui_tree` の `screen`、画像と同じ）で指す。窓の矩形は、目に見えない
/// 枠（大きさを変えるつまみ）の分だけ外に広いので、その分を足して渡す。
pub fn move_window(window: usize, x: Option<i32>, y: Option<i32>, width: Option<i32>, height: Option<i32>) -> Result<(), String> {
    let (frame, visible) = (window_rect(window), capture::visible_rect(window));
    let (left, top) = (visible.left - frame.left, visible.top - frame.top);
    let (right, bottom) = (frame.right - visible.right, frame.bottom - visible.bottom);
    let (x, y) = (x.unwrap_or(visible.left) - left, y.unwrap_or(visible.top) - top);
    let width = width.unwrap_or(visible.right - visible.left) + left + right;
    let height = height.unwrap_or(visible.bottom - visible.top) + top + bottom;
    if unsafe { SetWindowPos(hwnd(window), null_mut(), x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS) } == 0 {
        return Err(tr!("Could not move the window (it cannot be moved while the target is stopped)."));
    }
    Ok(())
}

pub fn close_window(window: usize) -> Result<(), String> {
    if unsafe { PostMessageW(hwnd(window), WM_CLOSE, 0, 0) } == 0 {
        return Err(tr!("Could not close it (the window may be gone)."));
    }
    Ok(())
}

/// キーの入力先を、コントロールにする（窓は前に出してから）。SetFocus は、相手のスレッドの入力につないで呼ぶ。
pub fn focus(control: usize) -> Result<(), String> {
    unsafe {
        let me = GetCurrentThreadId();
        let theirs = GetWindowThreadProcessId(hwnd(control), null_mut());
        if AttachThreadInput(me, theirs, 1) == 0 {
            return Err(tr!("Could not move the focus to the control."));
        }
        SetFocus(hwnd(control));
        AttachThreadInput(me, theirs, 0);
    }
    Ok(())
}

/// 窓のあるモニターの範囲（画面の座標）。
pub fn monitor_rect(window: usize) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: RECT::default(), rcWork: RECT::default(), dwFlags: 0 };
    unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd(window), MONITOR_DEFAULTTONEAREST), &mut info) };
    info.rcMonitor
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(tr!("Could not send the input (another window is blocking input, or an administrator window is in front)."));
    }
    Ok(())
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT { kind: INPUT_KEYBOARD, u: INPUT_UNION { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

/// キーの並びの部品。
#[derive(Debug, PartialEq)]
pub enum Key {
    /// 文字（そのまま打つ）。
    Text(String),
    /// 名前つきのキー（仮想キー）と、押しながらのキー（Ctrl、Shift、Alt）。
    Named { vk: u16, ctrl: bool, shift: bool, alt: bool },
}

/// `ui_keys` の書き方: ふつうの文字はそのまま、`{Enter}` `{Tab}` `{Esc}` `{F5}` `{Ctrl+A}` `{Shift+Tab}` `{Alt+F4}` などは
/// 中かっこで。`{{` と `}}` は中かっこそのもの。
pub fn parse_keys(keys: &str) -> Result<Vec<Key>, String> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = keys.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    name.push(c);
                }
                if !text.is_empty() {
                    parts.push(Key::Text(std::mem::take(&mut text)));
                }
                parts.push(named_key(&name)?);
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(Key::Text(text));
    }
    Ok(parts)
}

fn named_key(spec: &str) -> Result<Key, String> {
    let (mut ctrl, mut shift, mut alt) = (false, false, false);
    let pieces: Vec<&str> = spec.split('+').map(str::trim).collect();
    let (modifiers, key) = pieces.split_at(pieces.len().saturating_sub(1));
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            other => return Err(tr!("Unknown modifier key: {key} (Ctrl, Shift, Alt)", key = other)),
        }
    }
    let key = key.first().copied().unwrap_or("");
    let lower = key.to_ascii_lowercase();
    let vk: u16 = match lower.as_str() {
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "space" => 0x20,
        "backspace" | "bs" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        _ if lower.len() > 1 && lower.starts_with('f') && lower[1..].parse::<u16>().is_ok_and(|n| (1..=24).contains(&n)) => 0x6F + lower[1..].parse::<u16>().unwrap_or(1),
        // 1 文字（Ctrl+A など）: その文字のキー。
        _ if key.chars().count() == 1 => {
            let scanned = unsafe { VkKeyScanW(key.encode_utf16().next().unwrap_or(0)) };
            if scanned == -1 {
                return Err(tr!("Cannot type the key '{key}'.", key = key));
            }
            (scanned & 0xFF) as u16
        }
        _ => return Err(tr!("Unknown key: {{{key}}} (Enter, Tab, Esc, F1 to F24, arrows, Ctrl+A and so on)", key = spec)),
    };
    Ok(Key::Named { vk, ctrl, shift, alt })
}

/// キーを打つ（いま前にある窓へ）。
pub fn send_keys(keys: &[Key]) -> Result<(), String> {
    let mut inputs = Vec::new();
    for key in keys {
        match key {
            Key::Text(text) => {
                for unit in text.encode_utf16() {
                    inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
                    inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
            Key::Named { vk, ctrl, shift, alt } => {
                let modifiers: Vec<u16> = [(*ctrl, VK_CONTROL), (*shift, VK_SHIFT), (*alt, VK_MENU)].into_iter().filter(|(on, _)| *on).map(|(_, vk)| vk).collect();
                inputs.extend(modifiers.iter().map(|&m| key_input(m, 0, 0)));
                inputs.push(key_input(*vk, 0, 0));
                inputs.push(key_input(*vk, 0, KEYEVENTF_KEYUP));
                inputs.extend(modifiers.iter().rev().map(|&m| key_input(m, 0, KEYEVENTF_KEYUP)));
            }
        }
    }
    send_inputs(&inputs)
}

/// マウスの操作。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseAction {
    Click,
    DoubleClick,
    RightClick,
    Move,
    Down,
    Up,
    Drag { to_x: i32, to_y: i32 },
    Wheel(i32),
}

/// 画面の座標（物理ピクセル）を、SendInput の絶対座標（仮想画面全体を 0〜65535）にする。
pub fn absolute(x: i32, y: i32, virtual_screen: RECT) -> (i32, i32) {
    let width = (virtual_screen.right - virtual_screen.left).max(1) as i64;
    let height = (virtual_screen.bottom - virtual_screen.top).max(1) as i64;
    // ピクセルの中心に当たるように丸める。
    let nx = (((x - virtual_screen.left) as i64 * 65535 + width / 2) / (width - 1).max(1)).clamp(0, 65535);
    let ny = (((y - virtual_screen.top) as i64 * 65535 + height / 2) / (height - 1).max(1)).clamp(0, 65535);
    (nx as i32, ny as i32)
}

fn virtual_screen() -> RECT {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        RECT { left: x, top: y, right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN), bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN) }
    }
}

/// 画面の座標で、マウスを操作する。
pub fn mouse(action: MouseAction, x: i32, y: i32) -> Result<(), String> {
    let screen = virtual_screen();
    let at = |x: i32, y: i32, flags: u32, data: u32| {
        let (ax, ay) = absolute(x, y, screen);
        INPUT { kind: INPUT_MOUSE, u: INPUT_UNION { mi: MOUSEINPUT { dx: ax, dy: ay, mouseData: data, dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, time: 0, dwExtraInfo: 0 } } }
    };
    let mut inputs = vec![at(x, y, MOUSEEVENTF_MOVE, 0)];
    match action {
        MouseAction::Move => {}
        MouseAction::Click => inputs.extend([at(x, y, MOUSEEVENTF_LEFTDOWN, 0), at(x, y, MOUSEEVENTF_LEFTUP, 0)]),
        MouseAction::DoubleClick => inputs.extend([at(x, y, MOUSEEVENTF_LEFTDOWN, 0), at(x, y, MOUSEEVENTF_LEFTUP, 0), at(x, y, MOUSEEVENTF_LEFTDOWN, 0), at(x, y, MOUSEEVENTF_LEFTUP, 0)]),
        MouseAction::RightClick => inputs.extend([at(x, y, MOUSEEVENTF_RIGHTDOWN, 0), at(x, y, MOUSEEVENTF_RIGHTUP, 0)]),
        MouseAction::Down => inputs.push(at(x, y, MOUSEEVENTF_LEFTDOWN, 0)),
        MouseAction::Up => inputs.push(at(x, y, MOUSEEVENTF_LEFTUP, 0)),
        MouseAction::Wheel(clicks) => inputs.push(at(x, y, MOUSEEVENTF_WHEEL, (clicks * 120) as u32)),
        MouseAction::Drag { to_x, to_y } => {
            inputs.push(at(x, y, MOUSEEVENTF_LEFTDOWN, 0));
            // 途中を通る（アプリが動きを見て、ドラッグを始められるように）。
            for step in 1..=8 {
                inputs.push(at(x + (to_x - x) * step / 8, y + (to_y - y) * step / 8, MOUSEEVENTF_MOVE, 0));
            }
            inputs.push(at(to_x, to_y, MOUSEEVENTF_LEFTUP, 0));
        }
    }
    send_inputs(&inputs)
}

/// 窓のあるモニターの DPI の倍率（96 が 1.0）と、対象のプログラムの 1 ピクセルが画面の何ピクセルか。
/// DPI に対応していない（または System aware で、別の DPI のモニターにある）窓は、Windows が引き伸ばして描くので、
/// プログラムの中の座標（`MouseEventArgs.X` など）は、画面のピクセル ÷ 2 番目の値になる。
pub fn dpi_scales(window: usize) -> (f64, f64) {
    let (mut monitor_dpi, mut unused) = (0u32, 0u32);
    // MDT_EFFECTIVE_DPI
    let ok = unsafe { GetDpiForMonitor(MonitorFromWindow(hwnd(window), MONITOR_DEFAULTTONEAREST), 0, &mut monitor_dpi, &mut unused) } == 0;
    let monitor_dpi = if ok && monitor_dpi > 0 { monitor_dpi } else { 96 };
    let window_dpi = match unsafe { GetDpiForWindow(hwnd(window)) } {
        0 => monitor_dpi,
        dpi => dpi,
    };
    (monitor_dpi as f64 / 96.0, monitor_dpi as f64 / window_dpi as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_the_winforms_class_and_the_button_style() {
        assert_eq!(kind_of("WindowsForms10.BUTTON.app.0.141b42a_r7_ad1", 0x5001_0000, false), "Button");
        assert_eq!(kind_of("WindowsForms10.Button.app.0.2360855_r3_ad1", 0x5001_0003, false), "CheckBox");
        assert_eq!(kind_of("WindowsForms10.BUTTON.app.0.1", 0x5001_0009, false), "RadioButton");
        assert_eq!(kind_of("WindowsForms10.Edit.app.0.1", 0, false), "TextBox");
        assert_eq!(kind_of("WindowsForms10.Window.8.app.0.1", 0, true), "Form");
        assert_eq!(kind_of("WindowsForms10.Window.8.app.0.1", 0, false), "Control");
        assert_eq!(kind_of("#32770", 0, true), "Dialog");
        assert_eq!(kind_of("RichEdit20W", 0, false), "RichTextBox");
    }

    #[test]
    fn keys_are_text_and_named_keys_in_braces() {
        assert_eq!(
            parse_keys("abc{Enter}{{x}}").unwrap(),
            vec![Key::Text("abc".into()), Key::Named { vk: 0x0D, ctrl: false, shift: false, alt: false }, Key::Text("{x}".into())]
        );
        assert_eq!(parse_keys("{Shift+Tab}").unwrap(), vec![Key::Named { vk: 0x09, ctrl: false, shift: true, alt: false }]);
        assert_eq!(parse_keys("{F5}").unwrap(), vec![Key::Named { vk: 0x74, ctrl: false, shift: false, alt: false }]);
        assert!(parse_keys("{Hyper+X}").is_err());
        assert!(parse_keys("{NoSuchKey}").is_err());
    }

    #[test]
    fn screen_points_map_onto_the_absolute_range() {
        let screen = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert_eq!(absolute(0, 0, screen), (0, 0));
        assert_eq!(absolute(1919, 1079, screen), (65535, 65535));
        // 2 枚目のモニターが左にある (仮想画面の左が負)。
        let two = RECT { left: -1920, top: 0, right: 1920, bottom: 1080 };
        assert_eq!(absolute(-1920, 0, two).0, 0);
        assert!((absolute(0, 0, two).0 - 32768).abs() < 20);
    }
}
