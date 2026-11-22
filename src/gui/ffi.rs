//! GUI の操作に使う Win32 API の宣言（user32、gdi32、dwmapi と、対象のメモリを読み書きする kernel32）。COM ではないので
//! 手で宣言する（D-011）。構造体の大きさは、x86 と x64 で試験する（下の `tests`）。

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use crate::win32::HANDLE;
use std::ffi::c_void;

pub type HWND = *mut c_void;
pub type HDC = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type LPARAM = isize;
pub type WPARAM = usize;
pub type LRESULT = isize;
pub type WNDENUMPROC = unsafe extern "system" fn(HWND, LPARAM) -> i32;
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUSEINPUT {
    pub dx: i32,
    pub dy: i32,
    pub mouseData: u32,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KEYBDINPUT {
    pub wVk: u16,
    pub wScan: u16,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union INPUT_UNION {
    pub mi: MOUSEINPUT,
    pub ki: KEYBDINPUT,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct INPUT {
    pub kind: u32,
    pub u: INPUT_UNION,
}

pub const INPUT_MOUSE: u32 = 0;
pub const INPUT_KEYBOARD: u32 = 1;
pub const KEYEVENTF_KEYUP: u32 = 0x2;
pub const KEYEVENTF_UNICODE: u32 = 0x4;
pub const MOUSEEVENTF_MOVE: u32 = 0x1;
pub const MOUSEEVENTF_LEFTDOWN: u32 = 0x2;
pub const MOUSEEVENTF_LEFTUP: u32 = 0x4;
pub const MOUSEEVENTF_RIGHTDOWN: u32 = 0x8;
pub const MOUSEEVENTF_RIGHTUP: u32 = 0x10;
pub const MOUSEEVENTF_WHEEL: u32 = 0x800;
pub const MOUSEEVENTF_VIRTUALDESK: u32 = 0x4000;
pub const MOUSEEVENTF_ABSOLUTE: u32 = 0x8000;

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HANDLE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HANDLE,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: [i32; 2],
    pub lPrivate: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}

#[repr(C)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}

#[repr(C)]
pub struct DWM_THUMBNAIL_PROPERTIES {
    pub dwFlags: u32,
    pub rcDestination: RECT,
    pub rcSource: RECT,
    pub opacity: u8,
    pub fVisible: i32,
    pub fSourceClientAreaOnly: i32,
}

pub const DWM_TNP_RECTDESTINATION: u32 = 0x1;
pub const DWM_TNP_RECTSOURCE: u32 = 0x2;
pub const DWM_TNP_OPACITY: u32 = 0x4;
pub const DWM_TNP_VISIBLE: u32 = 0x8;
pub const DWM_TNP_SOURCECLIENTAREAONLY: u32 = 0x10;
pub const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;

pub const GWL_STYLE: i32 = -16;
pub const SMTO_ABORTIFHUNG: u32 = 0x2;
pub const WM_SETTEXT: u32 = 0x000C;
pub const WM_GETTEXT: u32 = 0x000D;
pub const WM_GETTEXTLENGTH: u32 = 0x000E;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_COMMAND: u32 = 0x0111;
pub const BM_CLICK: u32 = 0x00F5;
pub const CB_GETCOUNT: u32 = 0x0146;
pub const CB_GETLBTEXT: u32 = 0x0148;
pub const CB_GETLBTEXTLEN: u32 = 0x0149;
pub const CB_SETCURSEL: u32 = 0x014E;
pub const CBN_SELCHANGE: u32 = 1;
pub const LB_SETCURSEL: u32 = 0x0186;
pub const LB_GETCURSEL: u32 = 0x0188;
pub const LB_GETTEXT: u32 = 0x0189;
pub const LB_GETTEXTLEN: u32 = 0x018A;
pub const LB_GETCOUNT: u32 = 0x018B;
pub const LBN_SELCHANGE: u32 = 1;
pub const SW_RESTORE: i32 = 9;
pub const SW_SHOWNOACTIVATE: i32 = 4;
pub const SWP_NOZORDER: u32 = 0x4;
pub const SWP_NOACTIVATE: u32 = 0x10;
/// 相手のスレッドの答えを待たない（止まっている窓でも固まらない）。
pub const SWP_ASYNCWINDOWPOS: u32 = 0x4000;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
pub const SM_XVIRTUALSCREEN: i32 = 76;
pub const SM_YVIRTUALSCREEN: i32 = 77;
pub const SM_CXVIRTUALSCREEN: i32 = 78;
pub const SM_CYVIRTUALSCREEN: i32 = 79;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_EX_TOPMOST: u32 = 0x8;
pub const WS_EX_TOOLWINDOW: u32 = 0x80;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
pub const WS_EX_TRANSPARENT: u32 = 0x20;
pub const PM_REMOVE: u32 = 0x1;
pub const SRCCOPY: u32 = 0x00CC_0020;
pub const DIB_RGB_COLORS: u32 = 0;
pub const TRANSPARENT: i32 = 1;
pub const FW_BOLD: i32 = 700;
pub const ANTIALIASED_QUALITY: u32 = 4;
pub const BLACK_BRUSH: i32 = 4;
/// SetThreadDpiAwarenessContext の PER_MONITOR_AWARE_V2。座標をすべて物理ピクセルにする。
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;

#[link(name = "user32")]
extern "system" {
    pub fn EnumWindows(callback: WNDENUMPROC, lparam: LPARAM) -> i32;
    pub fn EnumChildWindows(parent: HWND, callback: WNDENUMPROC, lparam: LPARAM) -> i32;
    pub fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut u32) -> u32;
    pub fn GetClassNameW(hwnd: HWND, name: *mut u16, size: i32) -> i32;
    pub fn GetParent(hwnd: HWND) -> HWND;
    pub fn IsWindowVisible(hwnd: HWND) -> i32;
    pub fn IsWindowEnabled(hwnd: HWND) -> i32;
    pub fn IsIconic(hwnd: HWND) -> i32;
    pub fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> i32;
    pub fn GetDlgCtrlID(hwnd: HWND) -> i32;
    pub fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
    pub fn GetDpiForWindow(hwnd: HWND) -> u32;
    pub fn SendMessageTimeoutW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, flags: u32, timeout: u32, result: *mut usize) -> LRESULT;
    pub fn PostMessageW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> i32;
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub fn SetForegroundWindow(hwnd: HWND) -> i32;
    pub fn GetForegroundWindow() -> HWND;
    pub fn BringWindowToTop(hwnd: HWND) -> i32;
    pub fn ShowWindow(hwnd: HWND, command: i32) -> i32;
    pub fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    pub fn SendInput(count: u32, inputs: *const INPUT, size: i32) -> u32;
    pub fn GetSystemMetrics(index: i32) -> i32;
    pub fn MonitorFromWindow(hwnd: HWND, flags: u32) -> HANDLE;
    pub fn GetMonitorInfoW(monitor: HANDLE, info: *mut MONITORINFO) -> i32;
    pub fn AttachThreadInput(attach: u32, to: u32, on: i32) -> i32;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn SetThreadDpiAwarenessContext(context: isize) -> isize;
    pub fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    pub fn VkKeyScanW(ch: u16) -> i16;
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn CreateWindowExW(ex_style: u32, class: *const u16, name: *const u16, style: u32, x: i32, y: i32, width: i32, height: i32, parent: HWND, menu: HANDLE, instance: HANDLE, param: *mut c_void) -> HWND;
    pub fn DefWindowProcW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn DestroyWindow(hwnd: HWND) -> i32;
    pub fn PeekMessageW(msg: *mut MSG, hwnd: HWND, first: u32, last: u32, remove: u32) -> i32;
    pub fn TranslateMessage(msg: *const MSG) -> i32;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn FrameRect(dc: HDC, rect: *const RECT, brush: HGDIOBJ) -> i32;
    pub fn FillRect(dc: HDC, rect: *const RECT, brush: HGDIOBJ) -> i32;
    // 手の操作の記録（recorder.rs）。
    pub fn SetWindowsHookExW(id: i32, procedure: HOOKPROC, module: HANDLE, thread: u32) -> HANDLE;
    pub fn UnhookWindowsHookEx(hook: HANDLE) -> i32;
    pub fn CallNextHookEx(hook: HANDLE, code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn GetMessageW(msg: *mut MSG, hwnd: HWND, first: u32, last: u32) -> i32;
    pub fn PostThreadMessageW(thread: u32, message: u32, wparam: WPARAM, lparam: LPARAM) -> i32;
    pub fn WindowFromPoint(point: POINT) -> HWND;
    pub fn GetGUIThreadInfo(thread: u32, info: *mut GUITHREADINFO) -> i32;
    pub fn GetAncestor(hwnd: HWND, flags: u32) -> HWND;
    pub fn GetAsyncKeyState(key: i32) -> i16;
    // 画面の変化の知らせ（watch.rs）。
    pub fn SetWinEventHook(event_min: u32, event_max: u32, module: HANDLE, procedure: WINEVENTPROC, process: u32, thread: u32, flags: u32) -> HANDLE;
    pub fn UnhookWinEvent(hook: HANDLE) -> i32;
    /// 窓が持っている文字を、メッセージを送らずに読む（止まっている対象でも読める。テキストボックスの中身は読めない）。
    pub fn InternalGetWindowText(hwnd: HWND, text: *mut u16, max: i32) -> i32;
}

pub type HOOKPROC = unsafe extern "system" fn(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
pub type WINEVENTPROC = unsafe extern "system" fn(hook: HANDLE, event: u32, hwnd: HWND, object: i32, child: i32, thread: u32, time: u32);
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
pub const EVENT_OBJECT_STATECHANGE: u32 = 0x800A;
pub const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
pub const EVENT_OBJECT_VALUECHANGE: u32 = 0x800E;
/// 知らせを、対象のプロセスに入らずに受ける（自分のスレッドのメッセージのループで受ける）。
pub const WINEVENT_OUTOFCONTEXT: u32 = 0x0;
pub const WINEVENT_SKIPOWNPROCESS: u32 = 0x2;
pub const OBJID_CARET: i32 = -8;
pub const OBJID_CURSOR: i32 = -9;
pub const WH_KEYBOARD_LL: i32 = 13;
pub const WH_MOUSE_LL: i32 = 14;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_LBUTTONUP: u32 = 0x0202;
/// 低レベルのフックで、合成された（SendInput などで注入された）入力の印。
pub const LLMHF_INJECTED: u32 = 0x1;
pub const LLKHF_INJECTED: u32 = 0x10;
pub const LLKHF_ALTDOWN: u32 = 0x20;
pub const GA_ROOT: u32 = 2;
/// テキストボックスのスタイル: パスワードの欄。
pub const ES_PASSWORD: u32 = 0x20;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,
    pub mouseData: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,
    pub scanCode: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct GUITHREADINFO {
    pub cbSize: u32,
    pub flags: u32,
    pub hwndActive: HWND,
    pub hwndFocus: HWND,
    pub hwndCapture: HWND,
    pub hwndMenuOwner: HWND,
    pub hwndMoveSize: HWND,
    pub hwndCaret: HWND,
    pub rcCaret: RECT,
}

#[link(name = "shcore")]
extern "system" {
    pub fn GetDpiForMonitor(monitor: HANDLE, kind: u32, dpi_x: *mut u32, dpi_y: *mut u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn CreateCompatibleBitmap(dc: HDC, width: i32, height: i32) -> HGDIOBJ;
    pub fn SelectObject(dc: HDC, object: HGDIOBJ) -> HGDIOBJ;
    pub fn BitBlt(dest: HDC, x: i32, y: i32, width: i32, height: i32, source: HDC, sx: i32, sy: i32, op: u32) -> i32;
    pub fn GetDIBits(dc: HDC, bitmap: HGDIOBJ, start: u32, lines: u32, bits: *mut c_void, info: *mut BITMAPINFOHEADER, usage: u32) -> i32;
    pub fn DeleteObject(object: HGDIOBJ) -> i32;
    pub fn DeleteDC(dc: HDC) -> i32;
    pub fn CreateSolidBrush(color: u32) -> HGDIOBJ;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn SetTextColor(dc: HDC, color: u32) -> u32;
    pub fn TextOutW(dc: HDC, x: i32, y: i32, text: *const u16, length: i32) -> i32;
    pub fn GetStockObject(index: i32) -> HGDIOBJ;
    pub fn CreateFontW(height: i32, width: i32, escapement: i32, orientation: i32, weight: i32, italic: u32, underline: u32, strike_out: u32, charset: u32, precision: u32, clip_precision: u32, quality: u32, pitch_and_family: u32, face: *const u16) -> HGDIOBJ;
    pub fn GetTextExtentPoint32W(dc: HDC, text: *const u16, length: i32, size: *mut [i32; 2]) -> i32;
    pub fn ExcludeClipRect(dc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    pub fn SaveDC(dc: HDC) -> i32;
    pub fn RestoreDC(dc: HDC, saved: i32) -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    pub fn DwmRegisterThumbnail(destination: HWND, source: HWND, thumbnail: *mut isize) -> i32;
    pub fn DwmUnregisterThumbnail(thumbnail: isize) -> i32;
    pub fn DwmUpdateThumbnailProperties(thumbnail: isize, properties: *const DWM_THUMBNAIL_PROPERTIES) -> i32;
    pub fn DwmFlush() -> i32;
    pub fn DwmGetWindowAttribute(hwnd: HWND, attribute: u32, value: *mut c_void, size: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn VirtualAllocEx(process: HANDLE, address: *mut c_void, size: usize, allocation: u32, protection: u32) -> *mut c_void;
    pub fn VirtualFreeEx(process: HANDLE, address: *mut c_void, size: usize, free: u32) -> i32;
    pub fn ReadProcessMemory(process: HANDLE, address: *const c_void, buffer: *mut c_void, size: usize, read: *mut usize) -> i32;
    pub fn GetModuleHandleW(name: *const u16) -> HANDLE;
    pub fn GetCurrentThreadId() -> u32;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoCreateInstance(clsid: *const crate::com::GUID, outer: *mut c_void, context: u32, iid: *const crate::com::GUID, out: *mut *mut c_void) -> i32;
}

// BSTR、VARIANT、SAFEARRAY（UI Automation の値の受け渡し）。
#[link(name = "oleaut32")]
extern "system" {
    pub fn SysAllocString(text: *const u16) -> *mut u16;
    pub fn SysFreeString(text: *mut u16);
    pub fn SysStringLen(text: *const u16) -> u32;
    pub fn VariantClear(value: *mut c_void) -> i32;
    pub fn SafeArrayGetLBound(array: *mut c_void, dimension: u32, bound: *mut i32) -> i32;
    pub fn SafeArrayGetUBound(array: *mut c_void, dimension: u32, bound: *mut i32) -> i32;
    pub fn SafeArrayAccessData(array: *mut c_void, data: *mut *mut c_void) -> i32;
    pub fn SafeArrayUnaccessData(array: *mut c_void) -> i32;
}

pub const CLSCTX_INPROC_SERVER: u32 = 0x1;

pub const PROCESS_VM_OPERATION: u32 = 0x0008;
pub const PROCESS_VM_READ: u32 = 0x0010;
pub const PROCESS_VM_WRITE: u32 = 0x0020;
pub const MEM_COMMIT_RESERVE: u32 = 0x3000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const PAGE_READWRITE: u32 = 0x04;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_the_windows_headers() {
        // SendInput は大きさを引数で確かめる。x86 では 28、x64 では 40（MOUSEINPUT の ULONG_PTR の分）。
        assert_eq!(std::mem::size_of::<INPUT>(), if usize::BITS == 32 { 28 } else { 40 });
        assert_eq!(std::mem::size_of::<DWM_THUMBNAIL_PROPERTIES>(), 48);
        assert_eq!(std::mem::size_of::<BITMAPINFOHEADER>(), 40);
        assert_eq!(std::mem::size_of::<MSG>(), if usize::BITS == 32 { 32 } else { 48 });
        assert_eq!(std::mem::size_of::<WNDCLASSEXW>(), if usize::BITS == 32 { 48 } else { 80 });
    }
}
