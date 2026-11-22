//! COM の最小限の土台。生成したバインディング（cordebug.rs）と手書きの宣言が共有する。

// フィールド名とメソッド名、型の名前は COM の名前（QueryInterface、HRESULT、GUID など）のままにする。
#![allow(non_snake_case, dead_code, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl fmt::Debug for GUID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = &self.data4;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1, self.data2, self.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        )
    }
}

/// COM の戻り値。負なら失敗。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HRESULT(pub i32);

pub const S_OK: HRESULT = HRESULT(0);
pub const S_FALSE: HRESULT = HRESULT(1);
pub const E_NOINTERFACE: HRESULT = HRESULT(0x8000_4002_u32 as i32);
pub const E_POINTER: HRESULT = HRESULT(0x8000_4003_u32 as i32);
pub const E_FAIL: HRESULT = HRESULT(0x8000_4005_u32 as i32);

impl HRESULT {
    pub fn is_ok(self) -> bool {
        self.0 >= 0
    }

    /// 失敗を `Err` にする。`?` で呼び出し元へ返すために使う。
    pub fn ok(self) -> Result<(), HRESULT> {
        if self.is_ok() {
            Ok(())
        } else {
            Err(self)
        }
    }
}

impl fmt::Debug for HRESULT {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08X}", self.0 as u32)
    }
}

impl fmt::Display for HRESULT {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[repr(C)]
pub struct IUnknownVtbl {
    pub QueryInterface: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub Release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

/// COM インターフェイスの目印。
///
/// # Safety
/// `Vtbl` は、`IID` で得たオブジェクトの vtable の並びと一致していなければならない。
pub unsafe trait Interface {
    const IID: GUID;
    type Vtbl;
}

pub enum IUnknown {}
unsafe impl Interface for IUnknown {
    const IID: GUID = GUID { data1: 0, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
    type Vtbl = IUnknownVtbl;
}

/// 参照カウントを一つ持つ COM ポインター。捨てると `Release` する。
pub struct ComPtr<I: Interface> {
    ptr: NonNull<c_void>,
    interface: PhantomData<I>,
}

// ICorDebug のオブジェクトはフリースレッドで、コールバックのスレッドで受け取ったものをセッションのスレッドで使う。
unsafe impl<I: Interface> Send for ComPtr<I> {}

impl<I: Interface> ComPtr<I> {
    /// 出力引数で受け取ったポインターを包む。参照カウントはそのまま引き継ぐ。null なら `None`。
    ///
    /// # Safety
    /// `ptr` は `I` を実装したオブジェクトで、呼び出し側が参照を一つ持っていること。
    pub unsafe fn from_raw(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| ComPtr { ptr, interface: PhantomData })
    }

    /// 借りているポインター（コールバックの引数など）を包む。`AddRef` して自分の参照を持つ。
    ///
    /// # Safety
    /// `ptr` は `I` を実装した生きているオブジェクトであること。
    pub unsafe fn from_borrowed(ptr: *mut c_void) -> Option<Self> {
        let ptr = NonNull::new(ptr)?;
        let unknown = &**(ptr.as_ptr() as *const *const IUnknownVtbl);
        (unknown.AddRef)(ptr.as_ptr());
        Some(ComPtr { ptr, interface: PhantomData })
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// # Safety
    /// 生成したメソッドから呼ぶためのもの。`Interface` の約束が守られていれば正しい。
    pub unsafe fn vtbl(&self) -> &I::Vtbl {
        &**(self.ptr.as_ptr() as *const *const I::Vtbl)
    }

    fn unknown(&self) -> &IUnknownVtbl {
        // どのインターフェイスも先頭の 3 つは IUnknown。
        unsafe { &**(self.ptr.as_ptr() as *const *const IUnknownVtbl) }
    }

    /// `QueryInterface`。
    pub fn cast<J: Interface>(&self) -> Result<ComPtr<J>, HRESULT> {
        let mut out = std::ptr::null_mut();
        unsafe {
            (self.unknown().QueryInterface)(self.as_raw(), &J::IID, &mut out).ok()?;
            ComPtr::from_raw(out).ok_or(E_POINTER)
        }
    }
}

impl<I: Interface> Clone for ComPtr<I> {
    fn clone(&self) -> Self {
        unsafe { (self.unknown().AddRef)(self.as_raw()) };
        ComPtr { ptr: self.ptr, interface: PhantomData }
    }
}

impl<I: Interface> Drop for ComPtr<I> {
    fn drop(&mut self) {
        unsafe { (self.unknown().Release)(self.as_raw()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // 参照カウントだけを数える偽のオブジェクト。
    #[repr(C)]
    struct Fake {
        vtbl: *const IUnknownVtbl,
        count: AtomicU32,
    }
    unsafe extern "system" fn query(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
        if *riid == IUnknown::IID {
            add_ref(this);
            *out = this;
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
    unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
        (*(this as *const Fake)).count.fetch_add(1, Ordering::SeqCst) + 1
    }
    unsafe extern "system" fn release(this: *mut c_void) -> u32 {
        (*(this as *const Fake)).count.fetch_sub(1, Ordering::SeqCst) - 1
    }
    static VTBL: IUnknownVtbl = IUnknownVtbl { QueryInterface: query, AddRef: add_ref, Release: release };

    pub enum INothing {}
    unsafe impl Interface for INothing {
        const IID: GUID = GUID { data1: 1, data2: 2, data3: 3, data4: [0; 8] };
        type Vtbl = IUnknownVtbl;
    }

    #[test]
    fn counts_references_across_borrow_clone_cast_and_drop() {
        let fake = Fake { vtbl: &VTBL, count: AtomicU32::new(1) };
        let raw = &fake as *const Fake as *mut c_void;
        {
            let borrowed = unsafe { ComPtr::<IUnknown>::from_borrowed(raw) }.unwrap();
            assert_eq!(fake.count.load(Ordering::SeqCst), 2);
            let cloned = borrowed.clone();
            let cast = cloned.cast::<IUnknown>().unwrap();
            assert_eq!(fake.count.load(Ordering::SeqCst), 4);
            assert_eq!(cast.as_raw(), raw);
            assert_eq!(borrowed.cast::<INothing>().err(), Some(E_NOINTERFACE));
        }
        assert_eq!(fake.count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_null_pointer_is_none() {
        assert!(unsafe { ComPtr::<IUnknown>::from_raw(std::ptr::null_mut()) }.is_none());
    }

    #[test]
    fn hresult_reports_failure_in_hex() {
        assert!(S_FALSE.is_ok());
        assert_eq!(E_NOINTERFACE.ok(), Err(E_NOINTERFACE));
        assert_eq!(format!("{E_NOINTERFACE}"), "0x80004002");
    }
}
