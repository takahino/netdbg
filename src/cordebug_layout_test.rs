//! 生成したバインディングの検算。
//!
//! 右辺の番号は、spike（手書きの vtable 番号で x86 / x64 の実機を動かして確かめたもの）と同じ値。
//! 生成器や IDL を変えて並びがずれたら、ここで落ちる。

use crate::cordebug::*;
use crate::metahost::*;
use crate::oleacc::{IAccessibleVtbl, IDispatchVtbl, VARIANT};
use std::mem::{offset_of, size_of};

/// vtable の先頭から数えた番号（IUnknown の 3 つを含む）。
macro_rules! slot {
    ($vtbl:ty, $($field:tt)+) => {
        offset_of!($vtbl, $($field)+) / size_of::<usize>()
    };
}

#[test]
fn vtable_slots_match_the_ones_proven_on_real_runtimes() {
    assert_eq!(slot!(ICLRMetaHostVtbl, GetRuntime), 3);
    assert_eq!(slot!(ICLRRuntimeInfoVtbl, GetInterface), 9);
    assert_eq!(slot!(ICorDebugVtbl, Initialize), 3);
    assert_eq!(slot!(ICorDebugVtbl, SetManagedHandler), 5);
    assert_eq!(slot!(ICorDebugVtbl, CreateProcess), 7);
    assert_eq!(slot!(ICorDebugVtbl, DebugActiveProcess), 8);
    assert_eq!(slot!(ICorDebugControllerVtbl, Continue), 4);
    assert_eq!(slot!(ICorDebugProcessVtbl, GetID), 13);
    assert_eq!(slot!(ICorDebugAppDomainVtbl, Attach), 21);
    assert_eq!(slot!(ICorDebugModuleVtbl, GetName), 6);
    assert_eq!(slot!(ICorDebugModuleVtbl, GetFunctionFromToken), 9);
    assert_eq!(slot!(ICorDebugModuleVtbl, GetMetaDataInterface), 14);
    assert_eq!(slot!(ICorDebugFunctionVtbl, GetILCode), 6);
    assert_eq!(slot!(ICorDebugFunctionVtbl, CreateBreakpoint), 8);
    assert_eq!(slot!(ICorDebugCodeVtbl, CreateBreakpoint), 7);
    assert_eq!(slot!(ICorDebugBreakpointVtbl, Activate), 3);
    assert_eq!(slot!(ICorDebugThreadVtbl, GetID), 4);
    assert_eq!(slot!(ICorDebugThreadVtbl, GetActiveFrame), 15);
    assert_eq!(slot!(ICorDebugThreadVtbl, CreateEval), 17);
    assert_eq!(slot!(ICorDebugFrameVtbl, GetFunctionToken), 6);
    assert_eq!(slot!(ICorDebugILFrameVtbl, GetIP), 11);
    assert_eq!(slot!(ICorDebugILFrameVtbl, GetLocalVariable), 14);
    assert_eq!(slot!(ICorDebugILFrameVtbl, GetArgument), 16);
    assert_eq!(slot!(ICorDebugGenericValueVtbl, GetValue), 7);
    assert_eq!(slot!(ICorDebugGenericValueVtbl, SetValue), 8);
    assert_eq!(slot!(ICorDebugEvalVtbl, CallFunction), 3);
    assert_eq!(slot!(ICorDebugEvalVtbl, GetResult), 10);
    assert_eq!(slot!(ICorDebugEvalVtbl, CreateValue), 12);
    assert_eq!(slot!(ICorDebugProcess5Vtbl, EnumerateHeap), 4);
    assert_eq!(slot!(ICorDebugHeapEnumVtbl, Next), 7);
}

#[test]
fn callback_vtables_have_the_expected_number_of_slots() {
    assert_eq!(slot!(ICorDebugManagedCallbackVtbl, Breakpoint), 3);
    assert_eq!(slot!(ICorDebugManagedCallbackVtbl, BreakpointSetError), 28);
    assert_eq!(size_of::<ICorDebugManagedCallbackVtbl>(), 29 * size_of::<usize>());
    assert_eq!(slot!(ICorDebugManagedCallback2Vtbl, MDANotification), 10);
    assert_eq!(size_of::<ICorDebugManagedCallback2Vtbl>(), 11 * size_of::<usize>());
}

#[test]
fn structs_have_the_sizes_the_runtime_writes() {
    // ICorDebugHeapEnum::Next が書く 1 要素。spike では [u64; 4] として読んだ。
    assert_eq!(size_of::<COR_HEAPOBJECT>(), 32);
    assert_eq!(size_of::<COR_TYPEID>(), 16);
    assert_eq!(size_of::<COR_DEBUG_STEP_RANGE>(), 8);
    assert_eq!(size_of::<CORDB_ADDRESS>(), 8);
}

/// IAccessible（idl/oleacc.idl）。番号は文書の並び（IUnknown 3、IDispatch 4 のあと）。
#[test]
fn accessible_vtable_follows_the_documented_order() {
    assert_eq!(slot!(IDispatchVtbl, Invoke), 6);
    assert_eq!(slot!(IAccessibleVtbl, get_accParent), 7);
    assert_eq!(slot!(IAccessibleVtbl, get_accRole), 13);
    assert_eq!(slot!(IAccessibleVtbl, get_accState), 14);
    assert_eq!(slot!(IAccessibleVtbl, put_accValue), 27);
    // Windows の VARIANT は x86 で 16 バイト、x64 で 24 バイト（値の部分は 8 バイト目から）。
    assert_eq!(size_of::<VARIANT>(), 8 + 2 * size_of::<usize>());
    assert_eq!(offset_of!(VARIANT, data), 8);
}

/// UI Automation（idl/uiautomation.idl）。右辺は Windows SDK の UIAutomationClient.h の Vtbl の並びから数えた番号
/// （生成元の idl とは別の資料で確かめる）。
#[test]
fn ui_automation_vtables_follow_the_sdk_header() {
    use crate::uiautomation::*;
    assert_eq!(slot!(IUIAutomationElementVtbl, SetFocus), 3);
    assert_eq!(slot!(IUIAutomationElementVtbl, GetCurrentPatternAs), 14);
    assert_eq!(slot!(IUIAutomationElementVtbl, GetCachedChildren), 19);
    assert_eq!(slot!(IUIAutomationElementVtbl, get_CachedControlType), 53);
    assert_eq!(slot!(IUIAutomationElementVtbl, get_CachedBoundingRectangle), 75);
    assert_eq!(slot!(IUIAutomationElementVtbl, GetClickablePoint), 84);
    assert_eq!(slot!(IUIAutomation2Vtbl, base.ElementFromHandleBuildCache), 10);
    assert_eq!(slot!(IUIAutomation2Vtbl, base.CreateCacheRequest), 20);
    assert_eq!(slot!(IUIAutomation2Vtbl, base.CreatePropertyCondition), 23);
    assert_eq!(slot!(IUIAutomation2Vtbl, put_ConnectionTimeout), 61);
    assert_eq!(slot!(IUIAutomation2Vtbl, put_TransactionTimeout), 63);
    assert_eq!(slot!(IUIAutomationCacheRequestVtbl, AddProperty), 3);
    assert_eq!(slot!(IUIAutomationCacheRequestVtbl, put_TreeScope), 7);
    assert_eq!(slot!(IUIAutomationValuePatternVtbl, SetValue), 3);
    assert_eq!(slot!(IUIAutomationSelectionPatternVtbl, GetCurrentSelection), 3);
    assert_eq!(slot!(IUIAutomationSelectionItemPatternVtbl, Select), 3);
    assert_eq!(size_of::<crate::uiautomation::VARIANT>(), 8 + 2 * size_of::<usize>());
}