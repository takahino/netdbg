//! Active Accessibility（MSAA）で、コントロールの役割（押しボタン、チェックボックスなど）と状態（チェックされているか）を
//! 読む。WinForms のボタンの仲間はオーナー描画で、スタイルやメッセージ（BM_GETCHECK）からは分からないため。
//! IAccessible のバインディングは `idl/oleacc.idl` から `tools/idlgen` が生成した `crate::oleacc`。
//!
//! 呼び出しは対象の UI のスレッドへの COM の呼び出しになる。対象が止まっていると返らないので、動いているときだけ、
//! `gui::spawn` の GUI 用のスレッドから呼ぶ。

use crate::com::{ComPtr, Interface, GUID, HRESULT};
use crate::oleacc::{IAccessible, VARIANT};
use std::ffi::c_void;

const OBJID_CLIENT: i32 = -4;
const VT_I4: u16 = 3;
const CHILDID_SELF: usize = 0;
const COINIT_APARTMENTTHREADED: u32 = 0x2;

pub const STATE_SYSTEM_CHECKED: u32 = 0x10;
const ROLE_SYSTEM_GROUPING: u32 = 0x14;
const ROLE_SYSTEM_PUSHBUTTON: u32 = 0x2B;
const ROLE_SYSTEM_CHECKBUTTON: u32 = 0x2C;
const ROLE_SYSTEM_RADIOBUTTON: u32 = 0x2D;

#[link(name = "oleacc")]
extern "system" {
    fn AccessibleObjectFromWindow(hwnd: *mut c_void, object: i32, riid: *const GUID, out: *mut *mut c_void) -> HRESULT;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> HRESULT;
    fn CoUninitialize();
}

/// このスレッドで COM を使う間の印。捨てると CoUninitialize する（初期化できたときだけ）。
pub struct Apartment(bool);

impl Apartment {
    pub fn enter() -> Apartment {
        Apartment(unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn self_child() -> VARIANT {
    VARIANT { vt: VT_I4, wReserved1: 0, wReserved2: 0, wReserved3: 0, data: [CHILDID_SELF, 0] }
}

/// VT_I4 の値（値の部分の先頭の 4 バイト）。
fn as_i4(value: &VARIANT) -> Option<u32> {
    (value.vt == VT_I4).then_some(value.data[0] as u32)
}

/// 窓の役割と状態。MSAA に答えなければ `None`。
pub fn role_and_state(handle: usize) -> Option<(u32, u32)> {
    let mut raw = std::ptr::null_mut();
    unsafe { AccessibleObjectFromWindow(handle as *mut c_void, OBJID_CLIENT, &IAccessible::IID, &mut raw) }.ok().ok()?;
    let accessible: ComPtr<IAccessible> = unsafe { ComPtr::from_raw(raw) }?;
    let empty = VARIANT { vt: 0, wReserved1: 0, wReserved2: 0, wReserved3: 0, data: [0, 0] };
    let (mut role, mut state) = (empty, empty);
    unsafe { accessible.get_accRole(self_child(), &mut role) }.ok().ok()?;
    unsafe { accessible.get_accState(self_child(), &mut state) }.ok().ok()?;
    Some((as_i4(&role)?, as_i4(&state)?))
}

/// ボタンの仲間の、役割から見た種類。
pub fn button_kind(role: u32) -> Option<&'static str> {
    Some(match role {
        ROLE_SYSTEM_PUSHBUTTON => "Button",
        ROLE_SYSTEM_CHECKBUTTON => "CheckBox",
        ROLE_SYSTEM_RADIOBUTTON => "RadioButton",
        ROLE_SYSTEM_GROUPING => "GroupBox",
        _ => return None,
    })
}
