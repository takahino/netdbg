// このファイルは tools/idlgen が idl/oleacc.idl から生成した。手で編集しない。
//     cargo run -p idlgen -- idl/oleacc.idl src/oleacc.rs
//
// メソッドは `extern "system"`: x86 では stdcall、x64 では通常の規約になる。
// インターフェイスへのポインターは `*mut c_void` で受け渡しし、呼び出し側が `ComPtr` に包む。
// 中身を知らずポインターとしてだけ扱っている型: DISPPARAMS, EXCEPINFO, ITypeInfo
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]

use crate::com::{ComPtr, IUnknownVtbl, Interface, GUID, HRESULT};
use std::ffi::c_void;

pub type BSTR = *mut u16;
pub type LPOLESTR = *mut u16;
pub type LCID = u32;
pub type DISPID = i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VARIANT {
    pub vt: u16,
    pub wReserved1: u16,
    pub wReserved2: u16,
    pub wReserved3: u16,
    pub data: [usize; 2],
}

// ---- IDispatch : IUnknown
#[repr(C)]
pub struct IDispatchVtbl {
    pub base: IUnknownVtbl,
    pub GetTypeInfoCount: unsafe extern "system" fn(this: *mut c_void, pctinfo: *mut u32) -> HRESULT,
    pub GetTypeInfo: unsafe extern "system" fn(this: *mut c_void, iTInfo: u32, lcid: LCID, ppTInfo: *mut *mut c_void) -> HRESULT,
    pub GetIDsOfNames: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, rgszNames: *mut LPOLESTR, cNames: u32, lcid: LCID, rgDispId: *mut DISPID) -> HRESULT,
    pub Invoke: unsafe extern "system" fn(this: *mut c_void, dispIdMember: DISPID, riid: *const GUID, lcid: LCID, wFlags: u16, pDispParams: *mut c_void, pVarResult: *mut VARIANT, pExcepInfo: *mut c_void, puArgErr: *mut u32) -> HRESULT,
}
pub enum IDispatch {}
unsafe impl Interface for IDispatch {
    const IID: GUID = GUID { data1: 0x00020400, data2: 0x0000, data3: 0x0000, data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46] };
    type Vtbl = IDispatchVtbl;
}
impl ComPtr<IDispatch> {
    #[inline]
    pub unsafe fn GetTypeInfoCount(&self, pctinfo: *mut u32) -> HRESULT {
        (self.vtbl().GetTypeInfoCount)(self.as_raw(), pctinfo)
    }
    #[inline]
    pub unsafe fn GetTypeInfo(&self, iTInfo: u32, lcid: LCID, ppTInfo: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetTypeInfo)(self.as_raw(), iTInfo, lcid, ppTInfo)
    }
    #[inline]
    pub unsafe fn GetIDsOfNames(&self, riid: *const GUID, rgszNames: *mut LPOLESTR, cNames: u32, lcid: LCID, rgDispId: *mut DISPID) -> HRESULT {
        (self.vtbl().GetIDsOfNames)(self.as_raw(), riid, rgszNames, cNames, lcid, rgDispId)
    }
    #[inline]
    pub unsafe fn Invoke(&self, dispIdMember: DISPID, riid: *const GUID, lcid: LCID, wFlags: u16, pDispParams: *mut c_void, pVarResult: *mut VARIANT, pExcepInfo: *mut c_void, puArgErr: *mut u32) -> HRESULT {
        (self.vtbl().Invoke)(self.as_raw(), dispIdMember, riid, lcid, wFlags, pDispParams, pVarResult, pExcepInfo, puArgErr)
    }
}

// ---- IAccessible : IDispatch
#[repr(C)]
pub struct IAccessibleVtbl {
    pub base: IDispatchVtbl,
    pub get_accParent: unsafe extern "system" fn(this: *mut c_void, ppdispParent: *mut *mut c_void) -> HRESULT,
    pub get_accChildCount: unsafe extern "system" fn(this: *mut c_void, pcountChildren: *mut i32) -> HRESULT,
    pub get_accChild: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, ppdispChild: *mut *mut c_void) -> HRESULT,
    pub get_accName: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszName: *mut BSTR) -> HRESULT,
    pub get_accValue: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszValue: *mut BSTR) -> HRESULT,
    pub get_accDescription: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszDescription: *mut BSTR) -> HRESULT,
    pub get_accRole: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pvarRole: *mut VARIANT) -> HRESULT,
    pub get_accState: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pvarState: *mut VARIANT) -> HRESULT,
    pub get_accHelp: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszHelp: *mut BSTR) -> HRESULT,
    pub get_accHelpTopic: unsafe extern "system" fn(this: *mut c_void, pszHelpFile: *mut BSTR, varChild: VARIANT, pidTopic: *mut i32) -> HRESULT,
    pub get_accKeyboardShortcut: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszKeyboardShortcut: *mut BSTR) -> HRESULT,
    pub get_accFocus: unsafe extern "system" fn(this: *mut c_void, pvarChild: *mut VARIANT) -> HRESULT,
    pub get_accSelection: unsafe extern "system" fn(this: *mut c_void, pvarChildren: *mut VARIANT) -> HRESULT,
    pub get_accDefaultAction: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, pszDefaultAction: *mut BSTR) -> HRESULT,
    pub accSelect: unsafe extern "system" fn(this: *mut c_void, flagsSelect: i32, varChild: VARIANT) -> HRESULT,
    pub accLocation: unsafe extern "system" fn(this: *mut c_void, pxLeft: *mut i32, pyTop: *mut i32, pcxWidth: *mut i32, pcyHeight: *mut i32, varChild: VARIANT) -> HRESULT,
    pub accNavigate: unsafe extern "system" fn(this: *mut c_void, navDir: i32, varStart: VARIANT, pvarEndUpAt: *mut VARIANT) -> HRESULT,
    pub accHitTest: unsafe extern "system" fn(this: *mut c_void, xLeft: i32, yTop: i32, pvarChild: *mut VARIANT) -> HRESULT,
    pub accDoDefaultAction: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT) -> HRESULT,
    pub put_accName: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, szName: BSTR) -> HRESULT,
    pub put_accValue: unsafe extern "system" fn(this: *mut c_void, varChild: VARIANT, szValue: BSTR) -> HRESULT,
}
pub enum IAccessible {}
unsafe impl Interface for IAccessible {
    const IID: GUID = GUID { data1: 0x618736E0, data2: 0x3C3D, data3: 0x11CF, data4: [0x81, 0x0C, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71] };
    type Vtbl = IAccessibleVtbl;
}
impl ComPtr<IAccessible> {
    #[inline]
    pub unsafe fn get_accParent(&self, ppdispParent: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_accParent)(self.as_raw(), ppdispParent)
    }
    #[inline]
    pub unsafe fn get_accChildCount(&self, pcountChildren: *mut i32) -> HRESULT {
        (self.vtbl().get_accChildCount)(self.as_raw(), pcountChildren)
    }
    #[inline]
    pub unsafe fn get_accChild(&self, varChild: VARIANT, ppdispChild: *mut *mut c_void) -> HRESULT {
        (self.vtbl().get_accChild)(self.as_raw(), varChild, ppdispChild)
    }
    #[inline]
    pub unsafe fn get_accName(&self, varChild: VARIANT, pszName: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accName)(self.as_raw(), varChild, pszName)
    }
    #[inline]
    pub unsafe fn get_accValue(&self, varChild: VARIANT, pszValue: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accValue)(self.as_raw(), varChild, pszValue)
    }
    #[inline]
    pub unsafe fn get_accDescription(&self, varChild: VARIANT, pszDescription: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accDescription)(self.as_raw(), varChild, pszDescription)
    }
    #[inline]
    pub unsafe fn get_accRole(&self, varChild: VARIANT, pvarRole: *mut VARIANT) -> HRESULT {
        (self.vtbl().get_accRole)(self.as_raw(), varChild, pvarRole)
    }
    #[inline]
    pub unsafe fn get_accState(&self, varChild: VARIANT, pvarState: *mut VARIANT) -> HRESULT {
        (self.vtbl().get_accState)(self.as_raw(), varChild, pvarState)
    }
    #[inline]
    pub unsafe fn get_accHelp(&self, varChild: VARIANT, pszHelp: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accHelp)(self.as_raw(), varChild, pszHelp)
    }
    #[inline]
    pub unsafe fn get_accHelpTopic(&self, pszHelpFile: *mut BSTR, varChild: VARIANT, pidTopic: *mut i32) -> HRESULT {
        (self.vtbl().get_accHelpTopic)(self.as_raw(), pszHelpFile, varChild, pidTopic)
    }
    #[inline]
    pub unsafe fn get_accKeyboardShortcut(&self, varChild: VARIANT, pszKeyboardShortcut: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accKeyboardShortcut)(self.as_raw(), varChild, pszKeyboardShortcut)
    }
    #[inline]
    pub unsafe fn get_accFocus(&self, pvarChild: *mut VARIANT) -> HRESULT {
        (self.vtbl().get_accFocus)(self.as_raw(), pvarChild)
    }
    #[inline]
    pub unsafe fn get_accSelection(&self, pvarChildren: *mut VARIANT) -> HRESULT {
        (self.vtbl().get_accSelection)(self.as_raw(), pvarChildren)
    }
    #[inline]
    pub unsafe fn get_accDefaultAction(&self, varChild: VARIANT, pszDefaultAction: *mut BSTR) -> HRESULT {
        (self.vtbl().get_accDefaultAction)(self.as_raw(), varChild, pszDefaultAction)
    }
    #[inline]
    pub unsafe fn accSelect(&self, flagsSelect: i32, varChild: VARIANT) -> HRESULT {
        (self.vtbl().accSelect)(self.as_raw(), flagsSelect, varChild)
    }
    #[inline]
    pub unsafe fn accLocation(&self, pxLeft: *mut i32, pyTop: *mut i32, pcxWidth: *mut i32, pcyHeight: *mut i32, varChild: VARIANT) -> HRESULT {
        (self.vtbl().accLocation)(self.as_raw(), pxLeft, pyTop, pcxWidth, pcyHeight, varChild)
    }
    #[inline]
    pub unsafe fn accNavigate(&self, navDir: i32, varStart: VARIANT, pvarEndUpAt: *mut VARIANT) -> HRESULT {
        (self.vtbl().accNavigate)(self.as_raw(), navDir, varStart, pvarEndUpAt)
    }
    #[inline]
    pub unsafe fn accHitTest(&self, xLeft: i32, yTop: i32, pvarChild: *mut VARIANT) -> HRESULT {
        (self.vtbl().accHitTest)(self.as_raw(), xLeft, yTop, pvarChild)
    }
    #[inline]
    pub unsafe fn accDoDefaultAction(&self, varChild: VARIANT) -> HRESULT {
        (self.vtbl().accDoDefaultAction)(self.as_raw(), varChild)
    }
    #[inline]
    pub unsafe fn put_accName(&self, varChild: VARIANT, szName: BSTR) -> HRESULT {
        (self.vtbl().put_accName)(self.as_raw(), varChild, szName)
    }
    #[inline]
    pub unsafe fn put_accValue(&self, varChild: VARIANT, szValue: BSTR) -> HRESULT {
        (self.vtbl().put_accValue)(self.as_raw(), varChild, szValue)
    }
    #[inline]
    pub unsafe fn GetTypeInfoCount(&self, pctinfo: *mut u32) -> HRESULT {
        (self.vtbl().base.GetTypeInfoCount)(self.as_raw(), pctinfo)
    }
    #[inline]
    pub unsafe fn GetTypeInfo(&self, iTInfo: u32, lcid: LCID, ppTInfo: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetTypeInfo)(self.as_raw(), iTInfo, lcid, ppTInfo)
    }
    #[inline]
    pub unsafe fn GetIDsOfNames(&self, riid: *const GUID, rgszNames: *mut LPOLESTR, cNames: u32, lcid: LCID, rgDispId: *mut DISPID) -> HRESULT {
        (self.vtbl().base.GetIDsOfNames)(self.as_raw(), riid, rgszNames, cNames, lcid, rgDispId)
    }
    #[inline]
    pub unsafe fn Invoke(&self, dispIdMember: DISPID, riid: *const GUID, lcid: LCID, wFlags: u16, pDispParams: *mut c_void, pVarResult: *mut VARIANT, pExcepInfo: *mut c_void, puArgErr: *mut u32) -> HRESULT {
        (self.vtbl().base.Invoke)(self.as_raw(), dispIdMember, riid, lcid, wFlags, pDispParams, pVarResult, pExcepInfo, puArgErr)
    }
}
