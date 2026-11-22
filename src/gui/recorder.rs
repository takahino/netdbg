//! 人の手の操作の記録（成績書。B3、D-081）: 低レベルのマウスとキーボードのフックで、対象のプロセスの窓への入力を拾い、
//! 押した・打った先のコントロールを読んで、操作の言葉（`report::operations`）にする。
//!
//! - フック（`WH_MOUSE_LL` / `WH_KEYBOARD_LL`）は専用のスレッドに置き、そのスレッドでメッセージのループを回す。フックの
//!   中は位置とキーを積むだけにし、すぐ返す（遅いと Windows がフックを外す）。
//! - 読むのは別のスレッド（GUI のスレッド。座標が物理ピクセル、MSAA が使える）。押したあとの状態（チェックが入ったか）を
//!   読むため、押してから少し待つ。
//! - 合成された入力（`SendInput` など。AI の `ui_mouse` / `ui_keys`）は手の操作にしない。AI の操作は、ハブが AI の操作の
//!   知らせから「AI」として入れる。
//! - フックは、このプロセスが合成した入力では確かめられない（合成した入力は印が付くので捨てる）。解釈は
//!   `report::operations` の単体試験で、フックそのものは手で確かめる。

use super::ffi::*;
use crate::report::operations::{Input, Interpreter, Key, Recorded, Target};
use crate::report::Spot;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

/// フックから読む側へ渡す入力（まだコントロールを読んでいない）。
enum Raw {
    Click { x: i32, y: i32 },
    Key { vk: u32, ctrl: bool, alt: bool },
}

/// フックの手続きが入力を渡す先（記録は同時に 1 つ）。
static SENDER: Mutex<Option<Sender<Raw>>> = Mutex::new(None);

/// 押してから状態を読むまで待つ時間（チェックボックスやコンボボックスが変わり終える）。
const SETTLE: Duration = Duration::from_millis(120);
/// 止めるときに、残りの操作を待つ上限。
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

fn send(raw: Raw) {
    if let Some(sender) = SENDER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = sender.send(raw);
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam as u32 == WM_LBUTTONUP {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 {
            send(Raw::Click { x: info.pt.x, y: info.pt.y });
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.flags & LLKHF_INJECTED == 0 {
            let ctrl = GetAsyncKeyState(VK_CONTROL as i32) < 0;
            send(Raw::Key { vk: info.vkCode, ctrl, alt: info.flags & LLKHF_ALTDOWN != 0 });
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// 記録中。`stop` で止める（止めると、打っている途中の入力も操作にして返す）。
pub struct Recording {
    hook_thread: u32,
    finished: Receiver<Vec<Recorded>>,
}

/// 対象のプロセス `process_id` への手の操作を記録し始める。操作の言葉ができるたびに `on_operation` を呼ぶ（読む側の
/// スレッドから）。
pub fn start(process_id: u32, on_operation: impl Fn(Recorded) + Send + 'static) -> Result<Recording, String> {
    let (raw_sender, raw_receiver) = channel::<Raw>();
    {
        let mut slot = SENDER.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err(tr!("Hand operations are already being recorded."));
        }
        *slot = Some(raw_sender);
    }
    // フックのスレッド。
    let (ready_sender, ready_receiver) = channel::<Result<u32, String>>();
    std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, mouse_hook, module, 0);
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, keyboard_hook, module, 0);
        if mouse.is_null() || keyboard.is_null() {
            let _ = ready_sender.send(Err(tr!("Could not set the mouse and keyboard hooks.")));
        } else {
            let _ = ready_sender.send(Ok(GetCurrentThreadId()));
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {}
        }
        if !mouse.is_null() {
            UnhookWindowsHookEx(mouse);
        }
        if !keyboard.is_null() {
            UnhookWindowsHookEx(keyboard);
        }
        // 渡す先を外すと、読む側の受け口が閉じ、残りをまとめて終わる。
        SENDER.lock().unwrap_or_else(|e| e.into_inner()).take();
    });
    let hook_thread = match ready_receiver.recv_timeout(STOP_TIMEOUT) {
        Ok(Ok(thread)) => thread,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            SENDER.lock().unwrap_or_else(|e| e.into_inner()).take();
            return Err(tr!("The mouse and keyboard hooks did not start."));
        }
    };
    // 読む側のスレッド。
    let (finished_sender, finished) = channel();
    super::spawn(move || {
        let mut interpreter = Interpreter::default();
        let mut reread = |id: usize| target_of(process_id, id);
        while let Ok(raw) = raw_receiver.recv() {
            let Some(input) = resolve(process_id, raw) else { continue };
            for recorded in interpreter.feed(input, &mut reread) {
                on_operation(recorded);
            }
        }
        let _ = finished_sender.send(interpreter.finish(&mut reread));
    });
    Ok(Recording { hook_thread, finished })
}

impl Recording {
    /// 止める。打っている途中の入力があれば、その操作を返す。
    pub fn stop(self) -> Vec<Recorded> {
        unsafe { PostThreadMessageW(self.hook_thread, WM_QUIT, 0, 0) };
        self.finished.recv_timeout(STOP_TIMEOUT).unwrap_or_default()
    }
}

/// フックの入力を、対象のコントロールへの入力にする（対象の窓でなければ `None`）。
fn resolve(process_id: u32, raw: Raw) -> Option<Input> {
    match raw {
        Raw::Click { x, y } => {
            // 押した先は、すぐに読む（ダイアログの OK のように、押すと窓が閉じるものも残す）。状態（チェックなど）は、
            // 変わり終えてから読み直す。
            let handle = super::control_at(process_id, x, y)?;
            let pressed = target_of(process_id, handle)?;
            std::thread::sleep(SETTLE);
            Some(Input::Click(target_of(process_id, handle).unwrap_or(pressed)))
        }
        Raw::Key { vk, ctrl, alt } => {
            let key = key_of(vk, ctrl, alt)?;
            let handle = super::focused_control(process_id)?;
            Some(Input::Key { key, target: target_of(process_id, handle)? })
        }
    }
}

fn target_of(process_id: u32, handle: usize) -> Option<Target> {
    let element = super::element_of(process_id, handle)?;
    let (rect, window) = (element.rect, element.window);
    Some(Target {
        window,
        id: handle,
        name: element.name,
        kind: element.kind.to_string(),
        text: element.text,
        checked: element.checked,
        password: super::is_password(handle),
        rect: Some(Spot { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }),
    })
}

/// 仮想キーを、記録のキーの種類にする。修飾キーだけの押しは `None`。
fn key_of(vk: u32, ctrl: bool, alt: bool) -> Option<Key> {
    const VK_TAB: u32 = 0x09;
    const VK_RETURN: u32 = 0x0D;
    const VK_ESCAPE: u32 = 0x1B;
    const VK_F1: u32 = 0x70;
    const VK_F24: u32 = 0x87;
    Some(match vk {
        // Shift、Ctrl、Alt、左右の Shift / Ctrl / Alt、Windows キー。
        0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C => return None,
        VK_RETURN if !ctrl && !alt => Key::Enter,
        VK_TAB if !ctrl && !alt => Key::Tab,
        VK_ESCAPE => Key::Named("Esc".into()),
        VK_F1..=VK_F24 => Key::Named(format!("{}F{}", modifiers(ctrl, alt), vk - VK_F1 + 1)),
        _ if ctrl || alt => Key::Named(format!("{}{}", modifiers(ctrl, alt), key_name(vk))),
        _ => Key::Edit,
    })
}

fn modifiers(ctrl: bool, alt: bool) -> String {
    format!("{}{}", if ctrl { "Ctrl+" } else { "" }, if alt { "Alt+" } else { "" })
}

/// キーの名前（文字と数字はそのまま、ほかは仮想キーの番号）。
fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from).unwrap_or_default(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x2E => "Delete".into(),
        _ => format!("VK{vk:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_classified_like_a_test_log() {
        assert_eq!(key_of(0x41, false, false), Some(Key::Edit), "A");
        assert_eq!(key_of(0x0D, false, false), Some(Key::Enter));
        assert_eq!(key_of(0x09, false, false), Some(Key::Tab));
        assert_eq!(key_of(0x53, true, false), Some(Key::Named("Ctrl+S".into())));
        assert_eq!(key_of(0x74, false, false), Some(Key::Named("F5".into())));
        assert_eq!(key_of(0x73, false, true), Some(Key::Named("Alt+F4".into())));
        assert_eq!(key_of(0x11, true, false), None, "Ctrl だけ");
    }
}
