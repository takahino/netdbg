//! ICorDebugManagedCallback / 2 / 3 / 4 の実装。
//!
//! ランタイムのスレッドから呼ばれる。ここでは出来事をチャネルに積むだけで、対象の検査も続行もしない
//! （docs/DECISIONS.md D-008）。関数の型は生成した vtable のフィールドの型と照合されるので、引数の個数や
//! 大きさを間違えるとコンパイルが通らない。
//!
//! 3（CustomNotification）と 4（GC の前後、データブレークポイント）は、ランタイムが問い合わせてくる。どれも、頼まないと
//! 届かない出来事なので、受け取って記録し、続行するだけにしている（plan.html の段階 1）。

#![allow(non_snake_case)]

use crate::com::{ComPtr, IUnknown, IUnknownVtbl, Interface, E_NOINTERFACE, GUID, HRESULT, S_OK};
use crate::cordebug::*;
use crate::events::{post, DebugEvent, EventKind, Input};
use std::ffi::c_void;

/// コールバックのオブジェクト。インターフェイスごとに vtable へのポインターを一つ持つ。
#[repr(C)]
struct Callback {
    callback1: &'static ICorDebugManagedCallbackVtbl,
    callback2: &'static ICorDebugManagedCallback2Vtbl,
    callback3: &'static ICorDebugManagedCallback3Vtbl,
    callback4: &'static ICorDebugManagedCallback4Vtbl,
}

// プロセスに一つだけの、状態を持たないオブジェクト。参照カウントは数えない。
static CALLBACK: Callback = Callback { callback1: &VTBL1, callback2: &VTBL2, callback3: &VTBL3, callback4: &VTBL4 };

unsafe impl Sync for Callback {}

/// `ICorDebug::SetManagedHandler` に渡すポインター。
pub fn managed_callback() -> *mut c_void {
    &CALLBACK as *const Callback as *mut c_void
}

unsafe extern "system" fn query_interface(_this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let iid = *riid;
    if iid == IUnknown::IID || iid == ICorDebugManagedCallback::IID {
        *out = &CALLBACK.callback1 as *const _ as *mut c_void;
        S_OK
    } else if iid == ICorDebugManagedCallback2::IID {
        *out = &CALLBACK.callback2 as *const _ as *mut c_void;
        S_OK
    } else if iid == ICorDebugManagedCallback3::IID {
        *out = &CALLBACK.callback3 as *const _ as *mut c_void;
        S_OK
    } else if iid == ICorDebugManagedCallback4::IID {
        *out = &CALLBACK.callback4 as *const _ as *mut c_void;
        S_OK
    } else {
        *out = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
    2
}

unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
    1
}

const UNKNOWN: IUnknownVtbl = IUnknownVtbl { QueryInterface: query_interface, AddRef: add_ref, Release: release };

/// ICorDebugAppDomain と ICorDebugProcess はどちらも ICorDebugController から派生していて、vtable の先頭が
/// 同じ並びなので、そのまま Controller として持てる。
unsafe fn controller(pointer: *mut c_void) -> Option<ComPtr<ICorDebugController>> {
    ComPtr::from_borrowed(pointer)
}

unsafe fn send(controller_pointer: *mut c_void, kind: EventKind) -> HRESULT {
    post(Input::Debug(DebugEvent { controller: controller(controller_pointer), kind }));
    S_OK
}

/// 引数を使わず、名前だけを伝えるコールバック。
macro_rules! other {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        unsafe extern "system" fn $name(_this: *mut c_void, controller: *mut c_void $(, $arg: $ty)*) -> HRESULT {
            $(let _ = $arg;)*
            send(controller, EventKind::Other(stringify!($name)))
        }
    };
}

unsafe extern "system" fn Breakpoint(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void, breakpoint: *mut c_void) -> HRESULT {
    match (ComPtr::from_borrowed(thread), ComPtr::from_borrowed(breakpoint)) {
        (Some(thread), Some(breakpoint)) => send(appdomain, EventKind::Breakpoint { thread, breakpoint }),
        _ => send(appdomain, EventKind::Other("Breakpoint (null)")),
    }
}

unsafe extern "system" fn CreateProcess(_this: *mut c_void, process: *mut c_void) -> HRESULT {
    match ComPtr::from_borrowed(process) {
        Some(p) => send(process, EventKind::CreateProcess(p)),
        None => S_OK,
    }
}

unsafe extern "system" fn ExitProcess(_this: *mut c_void, _process: *mut c_void) -> HRESULT {
    // 対象はもう無いので、続行するものが無い。
    post(Input::Debug(DebugEvent { controller: None, kind: EventKind::ExitProcess }));
    S_OK
}

unsafe extern "system" fn CreateAppDomain(_this: *mut c_void, process: *mut c_void, appdomain: *mut c_void) -> HRESULT {
    match ComPtr::from_borrowed(appdomain) {
        Some(a) => send(process, EventKind::CreateAppDomain(a)),
        None => send(process, EventKind::Other("CreateAppDomain (null)")),
    }
}

/// 第 2 引数のオブジェクトを一つ運ぶだけのコールバック。
macro_rules! carries {
    ($name:ident => $variant:ident) => {
        unsafe extern "system" fn $name(_this: *mut c_void, appdomain: *mut c_void, object: *mut c_void) -> HRESULT {
            match ComPtr::from_borrowed(object) {
                Some(o) => send(appdomain, EventKind::$variant(o)),
                None => send(appdomain, EventKind::Other(stringify!($name))),
            }
        }
    };
}

carries!(LoadModule => LoadModule);
carries!(UnloadModule => UnloadModule);
carries!(CreateThread => CreateThread);
carries!(ExitThread => ExitThread);

/// 第 2 引数のスレッドを運ぶコールバック（variant がフィールドつき）。
macro_rules! carries_thread {
    ($name:ident => $variant:ident) => {
        unsafe extern "system" fn $name(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void) -> HRESULT {
            match ComPtr::from_borrowed(thread) {
                Some(thread) => send(appdomain, EventKind::$variant { thread }),
                None => send(appdomain, EventKind::Other(stringify!($name))),
            }
        }
    };
}

unsafe extern "system" fn StepComplete(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void, stepper: *mut c_void, reason: CorDebugStepReason) -> HRESULT {
    match ComPtr::from_borrowed(thread) {
        Some(thread) => send(appdomain, EventKind::StepComplete { thread, stepper: ComPtr::from_borrowed(stepper), reason }),
        None => send(appdomain, EventKind::Other("StepComplete (null)")),
    }
}

carries_thread!(Break => Break);
other!(Exception(thread: *mut c_void, unhandled: i32));
unsafe fn eval_finished(appdomain: *mut c_void, eval: *mut c_void, failed: bool) -> HRESULT {
    match ComPtr::from_borrowed(eval) {
        Some(eval) => send(appdomain, EventKind::EvalComplete { eval, failed }),
        None => send(appdomain, EventKind::Other("EvalComplete (null)")),
    }
}

unsafe extern "system" fn EvalComplete(_this: *mut c_void, appdomain: *mut c_void, _thread: *mut c_void, eval: *mut c_void) -> HRESULT {
    eval_finished(appdomain, eval, false)
}

unsafe extern "system" fn EvalException(_this: *mut c_void, appdomain: *mut c_void, _thread: *mut c_void, eval: *mut c_void) -> HRESULT {
    eval_finished(appdomain, eval, true)
}
other!(LoadClass(class: *mut c_void));
other!(UnloadClass(class: *mut c_void));
other!(DebuggerError(error: HRESULT, code: u32));
other!(LogMessage(thread: *mut c_void, level: i32, switch_name: *mut u16, message: *mut u16));
other!(LogSwitch(thread: *mut c_void, level: i32, reason: u32, switch_name: *mut u16, parent_name: *mut u16));
other!(ExitAppDomain(appdomain: *mut c_void));
other!(LoadAssembly(assembly: *mut c_void));
other!(UnloadAssembly(assembly: *mut c_void));
other!(ControlCTrap());
other!(NameChange(thread: *mut c_void));
other!(UpdateModuleSymbols(module: *mut c_void, stream: *mut c_void));
other!(EditAndContinueRemap(thread: *mut c_void, function: *mut c_void, accurate: i32));
other!(BreakpointSetError(thread: *mut c_void, breakpoint: *mut c_void, error: u32));

static VTBL1: ICorDebugManagedCallbackVtbl = ICorDebugManagedCallbackVtbl {
    base: UNKNOWN,
    Breakpoint,
    StepComplete,
    Break,
    Exception,
    EvalComplete,
    EvalException,
    CreateProcess,
    ExitProcess,
    CreateThread,
    ExitThread,
    LoadModule,
    UnloadModule,
    LoadClass,
    UnloadClass,
    DebuggerError,
    LogMessage,
    LogSwitch,
    CreateAppDomain,
    ExitAppDomain,
    LoadAssembly,
    UnloadAssembly,
    ControlCTrap,
    NameChange,
    UpdateModuleSymbols,
    EditAndContinueRemap,
    BreakpointSetError,
};

// Edit and Continue: 差分を当てたメソッドの古い版を実行しているスレッドが、系列点に来た。乗り換えるかは session が決める。
unsafe extern "system" fn FunctionRemapOpportunity(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void, old_function: *mut c_void, new_function: *mut c_void, old_il_offset: u32) -> HRESULT {
    match (ComPtr::from_borrowed(thread), ComPtr::from_borrowed(old_function), ComPtr::from_borrowed(new_function)) {
        (Some(thread), Some(old_function), Some(new_function)) => send(appdomain, EventKind::FunctionRemapOpportunity { thread, old_function, new_function, old_il_offset }),
        _ => send(appdomain, EventKind::Other("FunctionRemapOpportunity (null)")),
    }
}
other!(CreateConnection(connection: CONNID, name: *mut u16));
other!(ChangeConnection(connection: CONNID));
other!(DestroyConnection(connection: CONNID));
unsafe extern "system" fn Exception2(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void, frame: *mut c_void, _offset: u32, event_type: CorDebugExceptionCallbackType, _flags: u32) -> HRESULT {
    match ComPtr::from_borrowed(thread) {
        Some(thread) => send(appdomain, EventKind::Exception { thread, kind: event_type, frame: ComPtr::from_borrowed(frame) }),
        None => send(appdomain, EventKind::Other("Exception2 (null)")),
    }
}
other!(ExceptionUnwind(thread: *mut c_void, event_type: CorDebugExceptionUnwindCallbackType, flags: u32));
unsafe extern "system" fn FunctionRemapComplete(_this: *mut c_void, appdomain: *mut c_void, thread: *mut c_void, _function: *mut c_void) -> HRESULT {
    match ComPtr::from_borrowed(thread) {
        Some(thread) => send(appdomain, EventKind::FunctionRemapComplete { thread }),
        None => send(appdomain, EventKind::Other("FunctionRemapComplete (null)")),
    }
}
other!(MDANotification(thread: *mut c_void, mda: *mut c_void));

static VTBL2: ICorDebugManagedCallback2Vtbl = ICorDebugManagedCallback2Vtbl {
    base: UNKNOWN,
    FunctionRemapOpportunity,
    CreateConnection,
    ChangeConnection,
    DestroyConnection,
    Exception: Exception2,
    ExceptionUnwind,
    FunctionRemapComplete,
    MDANotification,
};

// ICorDebugManagedCallback3: 対象の CustomNotification。続行に使うのは、2 番目の引数のアプリケーションドメイン。
unsafe extern "system" fn CustomNotification(_this: *mut c_void, _thread: *mut c_void, appdomain: *mut c_void) -> HRESULT {
    send(appdomain, EventKind::Other("CustomNotification"))
}

static VTBL3: ICorDebugManagedCallback3Vtbl = ICorDebugManagedCallback3Vtbl { base: UNKNOWN, CustomNotification };

// ICorDebugManagedCallback4: GC の前後（ICorDebugProcess8::EnableGCNotificationEvents で頼んだときだけ）と、
// データブレークポイント（ICorDebugProcess::SetThreadContext で置いたときだけ）。書き込みの見張りで使う（D-093）。
unsafe extern "system" fn BeforeGarbageCollection(_this: *mut c_void, process: *mut c_void) -> HRESULT {
    send(process, EventKind::BeforeGarbageCollection)
}

unsafe extern "system" fn AfterGarbageCollection(_this: *mut c_void, process: *mut c_void) -> HRESULT {
    send(process, EventKind::AfterGarbageCollection)
}

unsafe extern "system" fn DataBreakpoint(_this: *mut c_void, process: *mut c_void, thread: *mut c_void, _context: *mut u8, _context_size: u32) -> HRESULT {
    match ComPtr::from_borrowed(thread) {
        Some(thread) => send(process, EventKind::DataBreakpoint { thread }),
        None => send(process, EventKind::Other("DataBreakpoint (null)")),
    }
}

static VTBL4: ICorDebugManagedCallback4Vtbl = ICorDebugManagedCallback4Vtbl { base: UNKNOWN, BeforeGarbageCollection, AfterGarbageCollection, DataBreakpoint };
