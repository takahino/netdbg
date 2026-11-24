// このファイルは tools/idlgen が idl/cordebug.idl から生成した。手で編集しない。
//     cargo run -p idlgen -- idl/cordebug.idl src/cordebug.rs
//
// メソッドは `extern "system"`: x86 では stdcall、x64 では通常の規約になる。
// インターフェイスへのポインターは `*mut c_void` で受け渡しし、呼び出し側が `ComPtr` に包む。
// 中身を知らずポインターとしてだけ扱っている型: IStream
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]

use crate::com::{ComPtr, IUnknownVtbl, Interface, GUID, HRESULT};
use std::ffi::c_void;

pub type mdToken = u32;
pub type mdModule = mdToken;
pub type mdScope = usize;
pub type mdTypeDef = mdToken;
pub type mdSourceFile = mdToken;
pub type mdMemberRef = mdToken;
pub type mdMethodDef = mdToken;
pub type mdFieldDef = mdToken;
pub type mdSignature = mdToken;
pub type CorElementType = u32;
pub type PCCOR_SIGNATURE = usize;
pub type LPDEBUG_EVENT = usize;
pub type LPSTARTUPINFOW = usize;
pub type LPPROCESS_INFORMATION = usize;
pub type LPCVOID = *mut c_void;
pub type HPROCESS = *mut c_void;
pub type HTHREAD = *mut c_void;
pub type TASKID = u64;
pub type CONNID = u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_IL_MAP {
    pub oldOffset: u32,
    pub newOffset: u32,
    pub fAccurate: i32,
}

pub type CorDebugIlToNativeMappingTypes = u32;
pub const NO_MAPPING: CorDebugIlToNativeMappingTypes = 0xffffffff;
pub const PROLOG: CorDebugIlToNativeMappingTypes = 0xfffffffe;
pub const EPILOG: CorDebugIlToNativeMappingTypes = 0xfffffffd;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_DEBUG_IL_TO_NATIVE_MAP {
    pub ilOffset: u32,
    pub nativeStartOffset: u32,
    pub nativeEndOffset: u32,
}

pub type CorDebugJITCompilerFlags = u32;
pub const CORDEBUG_JIT_DEFAULT: CorDebugJITCompilerFlags = 0x1;
pub const CORDEBUG_JIT_DISABLE_OPTIMIZATION: CorDebugJITCompilerFlags = 0x3;
pub const CORDEBUG_JIT_ENABLE_ENC: CorDebugJITCompilerFlags = 0x7;

pub type CorDebugJITCompilerFlagsDeprecated = u32;
pub const CORDEBUG_JIT_TRACK_DEBUG_INFO: CorDebugJITCompilerFlagsDeprecated = 0x1;

pub type CorDebugNGENPolicy = u32;
pub const DISABLE_LOCAL_NIC: CorDebugNGENPolicy = 0x1;
pub type CORDB_ADDRESS = u64;
pub type CORDB_REGISTER = u64;
pub type CORDB_CONTINUE_STATUS = u32;

pub type CorDebugBlockingReason = u32;
pub const BLOCKING_NONE: CorDebugBlockingReason = 0x0;
pub const BLOCKING_MONITOR_CRITICAL_SECTION: CorDebugBlockingReason = 0x1;
pub const BLOCKING_MONITOR_EVENT: CorDebugBlockingReason = 0x2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CorDebugBlockingObject {
    pub pBlockingObject: *mut c_void,
    pub dwTimeout: u32,
    pub blockingReason: CorDebugBlockingReason,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CorDebugExceptionObjectStackFrame {
    pub pModule: *mut c_void,
    pub ip: CORDB_ADDRESS,
    pub methodDef: mdMethodDef,
    pub isLastForeignExceptionFrame: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CorDebugGuidToTypeMapping {
    pub iid: GUID,
    pub pType: *mut c_void,
}

pub type CorDebugPlatform = u32;
pub const CORDB_PLATFORM_WINDOWS_X86: CorDebugPlatform = 0x0;
pub const CORDB_PLATFORM_WINDOWS_AMD64: CorDebugPlatform = 0x1;
pub const CORDB_PLATFORM_WINDOWS_IA64: CorDebugPlatform = 0x2;
pub const CORDB_PLATFORM_MAC_PPC: CorDebugPlatform = 0x3;
pub const CORDB_PLATFORM_MAC_X86: CorDebugPlatform = 0x4;
pub const CORDB_PLATFORM_WINDOWS_ARM: CorDebugPlatform = 0x5;
pub const CORDB_PLATFORM_MAC_AMD64: CorDebugPlatform = 0x6;
pub const CORDB_PLATFORM_WINDOWS_ARM64: CorDebugPlatform = 0x7;
pub const CORDB_PLATFORM_POSIX_AMD64: CorDebugPlatform = 0x8;
pub const CORDB_PLATFORM_POSIX_X86: CorDebugPlatform = 0x9;
pub const CORDB_PLATFORM_POSIX_ARM: CorDebugPlatform = 0xa;
pub const CORDB_PLATFORM_POSIX_ARM64: CorDebugPlatform = 0xb;
pub const CORDB_PLATFORM_POSIX_LOONGARCH64: CorDebugPlatform = 0xc;
pub const CORDB_PLATFORM_POSIX_RISCV64: CorDebugPlatform = 0xd;

pub type CorDebugStepReason = u32;
pub const STEP_NORMAL: CorDebugStepReason = 0x0;
pub const STEP_RETURN: CorDebugStepReason = 0x1;
pub const STEP_CALL: CorDebugStepReason = 0x2;
pub const STEP_EXCEPTION_FILTER: CorDebugStepReason = 0x3;
pub const STEP_EXCEPTION_HANDLER: CorDebugStepReason = 0x4;
pub const STEP_INTERCEPT: CorDebugStepReason = 0x5;
pub const STEP_EXIT: CorDebugStepReason = 0x6;

pub type LoggingLevelEnum = u32;
pub const LTraceLevel0: LoggingLevelEnum = 0x0;
pub const LTraceLevel1: LoggingLevelEnum = 0x1;
pub const LTraceLevel2: LoggingLevelEnum = 0x2;
pub const LTraceLevel3: LoggingLevelEnum = 0x3;
pub const LTraceLevel4: LoggingLevelEnum = 0x4;
pub const LStatusLevel0: LoggingLevelEnum = 0x14;
pub const LStatusLevel1: LoggingLevelEnum = 0x15;
pub const LStatusLevel2: LoggingLevelEnum = 0x16;
pub const LStatusLevel3: LoggingLevelEnum = 0x17;
pub const LStatusLevel4: LoggingLevelEnum = 0x18;
pub const LWarningLevel: LoggingLevelEnum = 0x28;
pub const LErrorLevel: LoggingLevelEnum = 0x32;
pub const LPanicLevel: LoggingLevelEnum = 0x64;

pub type LogSwitchCallReason = u32;
pub const SWITCH_CREATE: LogSwitchCallReason = 0x0;
pub const SWITCH_MODIFY: LogSwitchCallReason = 0x1;
pub const SWITCH_DELETE: LogSwitchCallReason = 0x2;

pub type CorDebugExceptionCallbackType = u32;
pub const DEBUG_EXCEPTION_FIRST_CHANCE: CorDebugExceptionCallbackType = 0x1;
pub const DEBUG_EXCEPTION_USER_FIRST_CHANCE: CorDebugExceptionCallbackType = 0x2;
pub const DEBUG_EXCEPTION_CATCH_HANDLER_FOUND: CorDebugExceptionCallbackType = 0x3;
pub const DEBUG_EXCEPTION_UNHANDLED: CorDebugExceptionCallbackType = 0x4;

pub type CorDebugExceptionFlags = u32;
pub const DEBUG_EXCEPTION_NONE: CorDebugExceptionFlags = 0x0;
pub const DEBUG_EXCEPTION_CAN_BE_INTERCEPTED: CorDebugExceptionFlags = 0x1;

pub type CorDebugExceptionUnwindCallbackType = u32;
pub const DEBUG_EXCEPTION_UNWIND_BEGIN: CorDebugExceptionUnwindCallbackType = 0x1;
pub const DEBUG_EXCEPTION_INTERCEPTED: CorDebugExceptionUnwindCallbackType = 0x2;

pub type CorDebugCreateProcessFlags = u32;
pub const DEBUG_NO_SPECIAL_OPTIONS: CorDebugCreateProcessFlags = 0x0;

pub type CorDebugHandleType = u32;
pub const HANDLE_STRONG: CorDebugHandleType = 0x1;
pub const HANDLE_WEAK_TRACK_RESURRECTION: CorDebugHandleType = 0x2;
pub const HANDLE_PINNED: CorDebugHandleType = 0x3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_VERSION {
    pub dwMajor: u32,
    pub dwMinor: u32,
    pub dwBuild: u32,
    pub dwSubBuild: u32,
}

pub type CorDebugInterfaceVersion = u32;
pub const CorDebugInvalidVersion: CorDebugInterfaceVersion = 0x0;
pub const CorDebugVersion_1_0: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugManagedCallback: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugUnmanagedCallback: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebug: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugController: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugAppDomain: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugAssembly: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugProcess: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugBreakpoint: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugFunctionBreakpoint: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugModuleBreakpoint: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugValueBreakpoint: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugStepper: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugRegisterSet: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugThread: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugChain: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugFrame: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugILFrame: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugNativeFrame: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugModule: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugFunction: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugCode: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugClass: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugEval: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugGenericValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugReferenceValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugHeapValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugObjectValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugBoxValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugStringValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugArrayValue: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugContext: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugObjectEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugBreakpointEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugStepperEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugProcessEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugThreadEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugFrameEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugChainEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugModuleEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugValueEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugCodeEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugTypeEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugErrorInfoEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugAppDomainEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugAssemblyEnum: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugEditAndContinueErrorInfo: CorDebugInterfaceVersion = 0x1;
pub const ver_ICorDebugEditAndContinueSnapshot: CorDebugInterfaceVersion = 0x1;
pub const CorDebugVersion_1_1: CorDebugInterfaceVersion = 0x2;
pub const CorDebugVersion_2_0: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugManagedCallback2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugAppDomain2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugAssembly2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugProcess2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugStepper2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugRegisterSet2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugThread2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugILFrame2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugInternalFrame: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugModule2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugFunction2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugCode2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugClass2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugValue2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugEval2: CorDebugInterfaceVersion = 0x3;
pub const ver_ICorDebugObjectValue2: CorDebugInterfaceVersion = 0x3;
pub const CorDebugVersion_4_0: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugThread3: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugThread4: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugStackWalk: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugNativeFrame2: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugInternalFrame2: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugRuntimeUnwindableFrame: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugHeapValue3: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugBlockingObjectEnum: CorDebugInterfaceVersion = 0x4;
pub const ver_ICorDebugValue3: CorDebugInterfaceVersion = 0x4;
pub const CorDebugVersion_4_5: CorDebugInterfaceVersion = 0x5;
pub const ver_ICorDebugComObjectValue: CorDebugInterfaceVersion = 0x5;
pub const ver_ICorDebugAppDomain3: CorDebugInterfaceVersion = 0x5;
pub const ver_ICorDebugCode3: CorDebugInterfaceVersion = 0x5;
pub const ver_ICorDebugILFrame3: CorDebugInterfaceVersion = 0x5;
pub const CorDebugLatestVersion: CorDebugInterfaceVersion = 0x5;

pub type CorDebugThreadState = u32;
pub const THREAD_RUN: CorDebugThreadState = 0x0;
pub const THREAD_SUSPEND: CorDebugThreadState = 0x1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_TYPEID {
    pub token1: u64,
    pub token2: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_HEAPOBJECT {
    pub address: CORDB_ADDRESS,
    pub size: u64,
    pub type_: COR_TYPEID,
}

pub type CorDebugGenerationTypes = u32;
pub const CorDebug_Gen0: CorDebugGenerationTypes = 0x0;
pub const CorDebug_Gen1: CorDebugGenerationTypes = 0x1;
pub const CorDebug_Gen2: CorDebugGenerationTypes = 0x2;
pub const CorDebug_LOH: CorDebugGenerationTypes = 0x3;
pub const CorDebug_POH: CorDebugGenerationTypes = 0x4;
pub const CorDebug_NonGC: CorDebugGenerationTypes = 0x7fffffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_SEGMENT {
    pub start: CORDB_ADDRESS,
    pub end: CORDB_ADDRESS,
    pub type_: CorDebugGenerationTypes,
    pub heap: u32,
}

pub type CorDebugGCType = u32;
pub const CorDebugWorkstationGC: CorDebugGCType = 0x0;
pub const CorDebugServerGC: CorDebugGCType = 0x1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_HEAPINFO {
    pub areGCStructuresValid: i32,
    pub pointerSize: u32,
    pub numHeaps: u32,
    pub concurrent: i32,
    pub gcType: CorDebugGCType,
}

pub type CorGCReferenceType = u32;
pub const CorHandleStrong: CorGCReferenceType = 0x1;
pub const CorHandleStrongPinning: CorGCReferenceType = 0x2;
pub const CorHandleWeakShort: CorGCReferenceType = 0x4;
pub const CorHandleWeakLong: CorGCReferenceType = 0x8;
pub const CorHandleWeakRefCount: CorGCReferenceType = 0x10;
pub const CorHandleStrongRefCount: CorGCReferenceType = 0x20;
pub const CorHandleStrongDependent: CorGCReferenceType = 0x40;
pub const CorHandleStrongAsyncPinned: CorGCReferenceType = 0x80;
pub const CorHandleStrongSizedByref: CorGCReferenceType = 0x100;
pub const CorHandleWeakNativeCom: CorGCReferenceType = 0x200;
pub const CorHandleWeakWinRT: CorGCReferenceType = 0x200;
pub const CorReferenceStack: CorGCReferenceType = 0x80000001;
pub const CorReferenceFinalizer: CorGCReferenceType = 0x4c4b402;
pub const CorHandleStrongOnly: CorGCReferenceType = 0x1e3;
pub const CorHandleWeakOnly: CorGCReferenceType = 0x21c;
pub const CorHandleAll: CorGCReferenceType = 0x7fffffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_GC_REFERENCE {
    pub Domain: *mut c_void,
    pub Location: *mut c_void,
    pub Type: CorGCReferenceType,
    pub ExtraData: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_ARRAY_LAYOUT {
    pub componentID: COR_TYPEID,
    pub componentType: CorElementType,
    pub firstElementOffset: u32,
    pub elementSize: u32,
    pub countOffset: u32,
    pub rankSize: u32,
    pub numRanks: u32,
    pub rankOffset: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_TYPE_LAYOUT {
    pub parentID: COR_TYPEID,
    pub objectSize: u32,
    pub numFields: u32,
    pub boxOffset: u32,
    pub type_: CorElementType,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_FIELD {
    pub token: mdFieldDef,
    pub offset: u32,
    pub id: COR_TYPEID,
    pub fieldType: CorElementType,
}

pub type CorDebugRecordFormat = u32;
pub const FORMAT_WINDOWS_EXCEPTIONRECORD32: CorDebugRecordFormat = 0x1;
pub const FORMAT_WINDOWS_EXCEPTIONRECORD64: CorDebugRecordFormat = 0x2;

pub type CorDebugDecodeEventFlagsWindows = u32;
pub const IS_FIRST_CHANCE: CorDebugDecodeEventFlagsWindows = 0x1;

pub type CorDebugDebugEventKind = u32;
pub const DEBUG_EVENT_KIND_MODULE_LOADED: CorDebugDebugEventKind = 0x1;
pub const DEBUG_EVENT_KIND_MODULE_UNLOADED: CorDebugDebugEventKind = 0x2;
pub const DEBUG_EVENT_KIND_MANAGED_EXCEPTION_FIRST_CHANCE: CorDebugDebugEventKind = 0x3;
pub const DEBUG_EVENT_KIND_MANAGED_EXCEPTION_USER_FIRST_CHANCE: CorDebugDebugEventKind = 0x4;
pub const DEBUG_EVENT_KIND_MANAGED_EXCEPTION_CATCH_HANDLER_FOUND: CorDebugDebugEventKind = 0x5;
pub const DEBUG_EVENT_KIND_MANAGED_EXCEPTION_UNHANDLED: CorDebugDebugEventKind = 0x6;

pub type CorDebugStateChange = u32;
pub const PROCESS_RUNNING: CorDebugStateChange = 0x1;
pub const FLUSH_ALL: CorDebugStateChange = 0x2;

pub type CorDebugCodeInvokeKind = u32;
pub const CODE_INVOKE_KIND_NONE: CorDebugCodeInvokeKind = 0x0;
pub const CODE_INVOKE_KIND_RETURN: CorDebugCodeInvokeKind = 0x1;
pub const CODE_INVOKE_KIND_TAILCALL: CorDebugCodeInvokeKind = 0x2;

pub type CorDebugCodeInvokePurpose = u32;
pub const CODE_INVOKE_PURPOSE_NONE: CorDebugCodeInvokePurpose = 0x0;
pub const CODE_INVOKE_PURPOSE_NATIVE_TO_MANAGED_TRANSITION: CorDebugCodeInvokePurpose = 0x1;
pub const CODE_INVOKE_PURPOSE_CLASS_INIT: CorDebugCodeInvokePurpose = 0x2;
pub const CODE_INVOKE_PURPOSE_INTERFACE_DISPATCH: CorDebugCodeInvokePurpose = 0x3;

pub type WriteableMetadataUpdateMode = u32;
pub const LegacyCompatPolicy: WriteableMetadataUpdateMode = 0x0;
pub const AlwaysShowUpdates: WriteableMetadataUpdateMode = 0x1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_MEMORY_RANGE {
    pub start: CORDB_ADDRESS,
    pub end: CORDB_ADDRESS,
}

pub type CorDebugIntercept = u32;
pub const INTERCEPT_NONE: CorDebugIntercept = 0x0;
pub const INTERCEPT_CLASS_INIT: CorDebugIntercept = 0x1;
pub const INTERCEPT_EXCEPTION_FILTER: CorDebugIntercept = 0x2;
pub const INTERCEPT_SECURITY: CorDebugIntercept = 0x4;
pub const INTERCEPT_CONTEXT_POLICY: CorDebugIntercept = 0x8;
pub const INTERCEPT_INTERCEPTION: CorDebugIntercept = 0x10;
pub const INTERCEPT_ALL: CorDebugIntercept = 0xffff;

pub type CorDebugUnmappedStop = u32;
pub const STOP_NONE: CorDebugUnmappedStop = 0x0;
pub const STOP_PROLOG: CorDebugUnmappedStop = 0x1;
pub const STOP_EPILOG: CorDebugUnmappedStop = 0x2;
pub const STOP_NO_MAPPING_INFO: CorDebugUnmappedStop = 0x4;
pub const STOP_OTHER_UNMAPPED: CorDebugUnmappedStop = 0x8;
pub const STOP_UNMANAGED: CorDebugUnmappedStop = 0x10;
pub const STOP_ALL: CorDebugUnmappedStop = 0xffff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_DEBUG_STEP_RANGE {
    pub startOffset: u32,
    pub endOffset: u32,
}

pub type CorDebugRegister = u32;
pub const REGISTER_INSTRUCTION_POINTER: CorDebugRegister = 0x0;
pub const REGISTER_STACK_POINTER: CorDebugRegister = 0x1;
pub const REGISTER_FRAME_POINTER: CorDebugRegister = 0x2;
pub const REGISTER_X86_EIP: CorDebugRegister = 0x0;
pub const REGISTER_X86_ESP: CorDebugRegister = 0x1;
pub const REGISTER_X86_EBP: CorDebugRegister = 0x2;
pub const REGISTER_X86_EAX: CorDebugRegister = 0x3;
pub const REGISTER_X86_ECX: CorDebugRegister = 0x4;
pub const REGISTER_X86_EDX: CorDebugRegister = 0x5;
pub const REGISTER_X86_EBX: CorDebugRegister = 0x6;
pub const REGISTER_X86_ESI: CorDebugRegister = 0x7;
pub const REGISTER_X86_EDI: CorDebugRegister = 0x8;
pub const REGISTER_X86_FPSTACK_0: CorDebugRegister = 0x9;
pub const REGISTER_X86_FPSTACK_1: CorDebugRegister = 0xa;
pub const REGISTER_X86_FPSTACK_2: CorDebugRegister = 0xb;
pub const REGISTER_X86_FPSTACK_3: CorDebugRegister = 0xc;
pub const REGISTER_X86_FPSTACK_4: CorDebugRegister = 0xd;
pub const REGISTER_X86_FPSTACK_5: CorDebugRegister = 0xe;
pub const REGISTER_X86_FPSTACK_6: CorDebugRegister = 0xf;
pub const REGISTER_X86_FPSTACK_7: CorDebugRegister = 0x10;
pub const REGISTER_AMD64_RIP: CorDebugRegister = 0x0;
pub const REGISTER_AMD64_RSP: CorDebugRegister = 0x1;
pub const REGISTER_AMD64_RBP: CorDebugRegister = 0x2;
pub const REGISTER_AMD64_RAX: CorDebugRegister = 0x3;
pub const REGISTER_AMD64_RCX: CorDebugRegister = 0x4;
pub const REGISTER_AMD64_RDX: CorDebugRegister = 0x5;
pub const REGISTER_AMD64_RBX: CorDebugRegister = 0x6;
pub const REGISTER_AMD64_RSI: CorDebugRegister = 0x7;
pub const REGISTER_AMD64_RDI: CorDebugRegister = 0x8;
pub const REGISTER_AMD64_R8: CorDebugRegister = 0x9;
pub const REGISTER_AMD64_R9: CorDebugRegister = 0xa;
pub const REGISTER_AMD64_R10: CorDebugRegister = 0xb;
pub const REGISTER_AMD64_R11: CorDebugRegister = 0xc;
pub const REGISTER_AMD64_R12: CorDebugRegister = 0xd;
pub const REGISTER_AMD64_R13: CorDebugRegister = 0xe;
pub const REGISTER_AMD64_R14: CorDebugRegister = 0xf;
pub const REGISTER_AMD64_R15: CorDebugRegister = 0x10;
pub const REGISTER_AMD64_XMM0: CorDebugRegister = 0x11;
pub const REGISTER_AMD64_XMM1: CorDebugRegister = 0x12;
pub const REGISTER_AMD64_XMM2: CorDebugRegister = 0x13;
pub const REGISTER_AMD64_XMM3: CorDebugRegister = 0x14;
pub const REGISTER_AMD64_XMM4: CorDebugRegister = 0x15;
pub const REGISTER_AMD64_XMM5: CorDebugRegister = 0x16;
pub const REGISTER_AMD64_XMM6: CorDebugRegister = 0x17;
pub const REGISTER_AMD64_XMM7: CorDebugRegister = 0x18;
pub const REGISTER_AMD64_XMM8: CorDebugRegister = 0x19;
pub const REGISTER_AMD64_XMM9: CorDebugRegister = 0x1a;
pub const REGISTER_AMD64_XMM10: CorDebugRegister = 0x1b;
pub const REGISTER_AMD64_XMM11: CorDebugRegister = 0x1c;
pub const REGISTER_AMD64_XMM12: CorDebugRegister = 0x1d;
pub const REGISTER_AMD64_XMM13: CorDebugRegister = 0x1e;
pub const REGISTER_AMD64_XMM14: CorDebugRegister = 0x1f;
pub const REGISTER_AMD64_XMM15: CorDebugRegister = 0x20;
pub const REGISTER_IA64_BSP: CorDebugRegister = 0x2;
pub const REGISTER_IA64_R0: CorDebugRegister = 0x3;
pub const REGISTER_IA64_F0: CorDebugRegister = 0x83;
pub const REGISTER_ARM_PC: CorDebugRegister = 0x0;
pub const REGISTER_ARM_SP: CorDebugRegister = 0x1;
pub const REGISTER_ARM_R0: CorDebugRegister = 0x2;
pub const REGISTER_ARM_R1: CorDebugRegister = 0x3;
pub const REGISTER_ARM_R2: CorDebugRegister = 0x4;
pub const REGISTER_ARM_R3: CorDebugRegister = 0x5;
pub const REGISTER_ARM_R4: CorDebugRegister = 0x6;
pub const REGISTER_ARM_R5: CorDebugRegister = 0x7;
pub const REGISTER_ARM_R6: CorDebugRegister = 0x8;
pub const REGISTER_ARM_R7: CorDebugRegister = 0x9;
pub const REGISTER_ARM_R8: CorDebugRegister = 0xa;
pub const REGISTER_ARM_R9: CorDebugRegister = 0xb;
pub const REGISTER_ARM_R10: CorDebugRegister = 0xc;
pub const REGISTER_ARM_R11: CorDebugRegister = 0xd;
pub const REGISTER_ARM_R12: CorDebugRegister = 0xe;
pub const REGISTER_ARM_LR: CorDebugRegister = 0xf;
pub const REGISTER_ARM_D0: CorDebugRegister = 0x10;
pub const REGISTER_ARM_D1: CorDebugRegister = 0x11;
pub const REGISTER_ARM_D2: CorDebugRegister = 0x12;
pub const REGISTER_ARM_D3: CorDebugRegister = 0x13;
pub const REGISTER_ARM_D4: CorDebugRegister = 0x14;
pub const REGISTER_ARM_D5: CorDebugRegister = 0x15;
pub const REGISTER_ARM_D6: CorDebugRegister = 0x16;
pub const REGISTER_ARM_D7: CorDebugRegister = 0x17;
pub const REGISTER_ARM_D8: CorDebugRegister = 0x18;
pub const REGISTER_ARM_D9: CorDebugRegister = 0x19;
pub const REGISTER_ARM_D10: CorDebugRegister = 0x1a;
pub const REGISTER_ARM_D11: CorDebugRegister = 0x1b;
pub const REGISTER_ARM_D12: CorDebugRegister = 0x1c;
pub const REGISTER_ARM_D13: CorDebugRegister = 0x1d;
pub const REGISTER_ARM_D14: CorDebugRegister = 0x1e;
pub const REGISTER_ARM_D15: CorDebugRegister = 0x1f;
pub const REGISTER_ARM_D16: CorDebugRegister = 0x20;
pub const REGISTER_ARM_D17: CorDebugRegister = 0x21;
pub const REGISTER_ARM_D18: CorDebugRegister = 0x22;
pub const REGISTER_ARM_D19: CorDebugRegister = 0x23;
pub const REGISTER_ARM_D20: CorDebugRegister = 0x24;
pub const REGISTER_ARM_D21: CorDebugRegister = 0x25;
pub const REGISTER_ARM_D22: CorDebugRegister = 0x26;
pub const REGISTER_ARM_D23: CorDebugRegister = 0x27;
pub const REGISTER_ARM_D24: CorDebugRegister = 0x28;
pub const REGISTER_ARM_D25: CorDebugRegister = 0x29;
pub const REGISTER_ARM_D26: CorDebugRegister = 0x2a;
pub const REGISTER_ARM_D27: CorDebugRegister = 0x2b;
pub const REGISTER_ARM_D28: CorDebugRegister = 0x2c;
pub const REGISTER_ARM_D29: CorDebugRegister = 0x2d;
pub const REGISTER_ARM_D30: CorDebugRegister = 0x2e;
pub const REGISTER_ARM_D31: CorDebugRegister = 0x2f;
pub const REGISTER_ARM64_PC: CorDebugRegister = 0x0;
pub const REGISTER_ARM64_SP: CorDebugRegister = 0x1;
pub const REGISTER_ARM64_FP: CorDebugRegister = 0x2;
pub const REGISTER_ARM64_X0: CorDebugRegister = 0x3;
pub const REGISTER_ARM64_X1: CorDebugRegister = 0x4;
pub const REGISTER_ARM64_X2: CorDebugRegister = 0x5;
pub const REGISTER_ARM64_X3: CorDebugRegister = 0x6;
pub const REGISTER_ARM64_X4: CorDebugRegister = 0x7;
pub const REGISTER_ARM64_X5: CorDebugRegister = 0x8;
pub const REGISTER_ARM64_X6: CorDebugRegister = 0x9;
pub const REGISTER_ARM64_X7: CorDebugRegister = 0xa;
pub const REGISTER_ARM64_X8: CorDebugRegister = 0xb;
pub const REGISTER_ARM64_X9: CorDebugRegister = 0xc;
pub const REGISTER_ARM64_X10: CorDebugRegister = 0xd;
pub const REGISTER_ARM64_X11: CorDebugRegister = 0xe;
pub const REGISTER_ARM64_X12: CorDebugRegister = 0xf;
pub const REGISTER_ARM64_X13: CorDebugRegister = 0x10;
pub const REGISTER_ARM64_X14: CorDebugRegister = 0x11;
pub const REGISTER_ARM64_X15: CorDebugRegister = 0x12;
pub const REGISTER_ARM64_X16: CorDebugRegister = 0x13;
pub const REGISTER_ARM64_X17: CorDebugRegister = 0x14;
pub const REGISTER_ARM64_X18: CorDebugRegister = 0x15;
pub const REGISTER_ARM64_X19: CorDebugRegister = 0x16;
pub const REGISTER_ARM64_X20: CorDebugRegister = 0x17;
pub const REGISTER_ARM64_X21: CorDebugRegister = 0x18;
pub const REGISTER_ARM64_X22: CorDebugRegister = 0x19;
pub const REGISTER_ARM64_X23: CorDebugRegister = 0x1a;
pub const REGISTER_ARM64_X24: CorDebugRegister = 0x1b;
pub const REGISTER_ARM64_X25: CorDebugRegister = 0x1c;
pub const REGISTER_ARM64_X26: CorDebugRegister = 0x1d;
pub const REGISTER_ARM64_X27: CorDebugRegister = 0x1e;
pub const REGISTER_ARM64_X28: CorDebugRegister = 0x1f;
pub const REGISTER_ARM64_LR: CorDebugRegister = 0x20;
pub const REGISTER_ARM64_V0: CorDebugRegister = 0x21;
pub const REGISTER_ARM64_V1: CorDebugRegister = 0x22;
pub const REGISTER_ARM64_V2: CorDebugRegister = 0x23;
pub const REGISTER_ARM64_V3: CorDebugRegister = 0x24;
pub const REGISTER_ARM64_V4: CorDebugRegister = 0x25;
pub const REGISTER_ARM64_V5: CorDebugRegister = 0x26;
pub const REGISTER_ARM64_V6: CorDebugRegister = 0x27;
pub const REGISTER_ARM64_V7: CorDebugRegister = 0x28;
pub const REGISTER_ARM64_V8: CorDebugRegister = 0x29;
pub const REGISTER_ARM64_V9: CorDebugRegister = 0x2a;
pub const REGISTER_ARM64_V10: CorDebugRegister = 0x2b;
pub const REGISTER_ARM64_V11: CorDebugRegister = 0x2c;
pub const REGISTER_ARM64_V12: CorDebugRegister = 0x2d;
pub const REGISTER_ARM64_V13: CorDebugRegister = 0x2e;
pub const REGISTER_ARM64_V14: CorDebugRegister = 0x2f;
pub const REGISTER_ARM64_V15: CorDebugRegister = 0x30;
pub const REGISTER_ARM64_V16: CorDebugRegister = 0x31;
pub const REGISTER_ARM64_V17: CorDebugRegister = 0x32;
pub const REGISTER_ARM64_V18: CorDebugRegister = 0x33;
pub const REGISTER_ARM64_V19: CorDebugRegister = 0x34;
pub const REGISTER_ARM64_V20: CorDebugRegister = 0x35;
pub const REGISTER_ARM64_V21: CorDebugRegister = 0x36;
pub const REGISTER_ARM64_V22: CorDebugRegister = 0x37;
pub const REGISTER_ARM64_V23: CorDebugRegister = 0x38;
pub const REGISTER_ARM64_V24: CorDebugRegister = 0x39;
pub const REGISTER_ARM64_V25: CorDebugRegister = 0x3a;
pub const REGISTER_ARM64_V26: CorDebugRegister = 0x3b;
pub const REGISTER_ARM64_V27: CorDebugRegister = 0x3c;
pub const REGISTER_ARM64_V28: CorDebugRegister = 0x3d;
pub const REGISTER_ARM64_V29: CorDebugRegister = 0x3e;
pub const REGISTER_ARM64_V30: CorDebugRegister = 0x3f;
pub const REGISTER_ARM64_V31: CorDebugRegister = 0x40;
pub const REGISTER_LOONGARCH64_PC: CorDebugRegister = 0x0;
pub const REGISTER_LOONGARCH64_SP: CorDebugRegister = 0x1;
pub const REGISTER_LOONGARCH64_FP: CorDebugRegister = 0x2;
pub const REGISTER_LOONGARCH64_RA: CorDebugRegister = 0x3;
pub const REGISTER_LOONGARCH64_TP: CorDebugRegister = 0x4;
pub const REGISTER_LOONGARCH64_A0: CorDebugRegister = 0x5;
pub const REGISTER_LOONGARCH64_A1: CorDebugRegister = 0x6;
pub const REGISTER_LOONGARCH64_A2: CorDebugRegister = 0x7;
pub const REGISTER_LOONGARCH64_A3: CorDebugRegister = 0x8;
pub const REGISTER_LOONGARCH64_A4: CorDebugRegister = 0x9;
pub const REGISTER_LOONGARCH64_A5: CorDebugRegister = 0xa;
pub const REGISTER_LOONGARCH64_A6: CorDebugRegister = 0xb;
pub const REGISTER_LOONGARCH64_A7: CorDebugRegister = 0xc;
pub const REGISTER_LOONGARCH64_T0: CorDebugRegister = 0xd;
pub const REGISTER_LOONGARCH64_T1: CorDebugRegister = 0xe;
pub const REGISTER_LOONGARCH64_T2: CorDebugRegister = 0xf;
pub const REGISTER_LOONGARCH64_T3: CorDebugRegister = 0x10;
pub const REGISTER_LOONGARCH64_T4: CorDebugRegister = 0x11;
pub const REGISTER_LOONGARCH64_T5: CorDebugRegister = 0x12;
pub const REGISTER_LOONGARCH64_T6: CorDebugRegister = 0x13;
pub const REGISTER_LOONGARCH64_T7: CorDebugRegister = 0x14;
pub const REGISTER_LOONGARCH64_T8: CorDebugRegister = 0x15;
pub const REGISTER_LOONGARCH64_X0: CorDebugRegister = 0x16;
pub const REGISTER_LOONGARCH64_S0: CorDebugRegister = 0x17;
pub const REGISTER_LOONGARCH64_S1: CorDebugRegister = 0x18;
pub const REGISTER_LOONGARCH64_S2: CorDebugRegister = 0x19;
pub const REGISTER_LOONGARCH64_S3: CorDebugRegister = 0x1a;
pub const REGISTER_LOONGARCH64_S4: CorDebugRegister = 0x1b;
pub const REGISTER_LOONGARCH64_S5: CorDebugRegister = 0x1c;
pub const REGISTER_LOONGARCH64_S6: CorDebugRegister = 0x1d;
pub const REGISTER_LOONGARCH64_S7: CorDebugRegister = 0x1e;
pub const REGISTER_LOONGARCH64_S8: CorDebugRegister = 0x1f;
pub const REGISTER_LOONGARCH64_F0: CorDebugRegister = 0x20;
pub const REGISTER_LOONGARCH64_F1: CorDebugRegister = 0x21;
pub const REGISTER_LOONGARCH64_F2: CorDebugRegister = 0x22;
pub const REGISTER_LOONGARCH64_F3: CorDebugRegister = 0x23;
pub const REGISTER_LOONGARCH64_F4: CorDebugRegister = 0x24;
pub const REGISTER_LOONGARCH64_F5: CorDebugRegister = 0x25;
pub const REGISTER_LOONGARCH64_F6: CorDebugRegister = 0x26;
pub const REGISTER_LOONGARCH64_F7: CorDebugRegister = 0x27;
pub const REGISTER_LOONGARCH64_F8: CorDebugRegister = 0x28;
pub const REGISTER_LOONGARCH64_F9: CorDebugRegister = 0x29;
pub const REGISTER_LOONGARCH64_F10: CorDebugRegister = 0x2a;
pub const REGISTER_LOONGARCH64_F11: CorDebugRegister = 0x2b;
pub const REGISTER_LOONGARCH64_F12: CorDebugRegister = 0x2c;
pub const REGISTER_LOONGARCH64_F13: CorDebugRegister = 0x2d;
pub const REGISTER_LOONGARCH64_F14: CorDebugRegister = 0x2e;
pub const REGISTER_LOONGARCH64_F15: CorDebugRegister = 0x2f;
pub const REGISTER_LOONGARCH64_F16: CorDebugRegister = 0x30;
pub const REGISTER_LOONGARCH64_F17: CorDebugRegister = 0x31;
pub const REGISTER_LOONGARCH64_F18: CorDebugRegister = 0x32;
pub const REGISTER_LOONGARCH64_F19: CorDebugRegister = 0x33;
pub const REGISTER_LOONGARCH64_F20: CorDebugRegister = 0x34;
pub const REGISTER_LOONGARCH64_F21: CorDebugRegister = 0x35;
pub const REGISTER_LOONGARCH64_F22: CorDebugRegister = 0x36;
pub const REGISTER_LOONGARCH64_F23: CorDebugRegister = 0x37;
pub const REGISTER_LOONGARCH64_F24: CorDebugRegister = 0x38;
pub const REGISTER_LOONGARCH64_F25: CorDebugRegister = 0x39;
pub const REGISTER_LOONGARCH64_F26: CorDebugRegister = 0x3a;
pub const REGISTER_LOONGARCH64_F27: CorDebugRegister = 0x3b;
pub const REGISTER_LOONGARCH64_F28: CorDebugRegister = 0x3c;
pub const REGISTER_LOONGARCH64_F29: CorDebugRegister = 0x3d;
pub const REGISTER_LOONGARCH64_F30: CorDebugRegister = 0x3e;
pub const REGISTER_LOONGARCH64_F31: CorDebugRegister = 0x3f;
pub const REGISTER_RISCV64_PC: CorDebugRegister = 0x0;
pub const REGISTER_RISCV64_SP: CorDebugRegister = 0x1;
pub const REGISTER_RISCV64_FP: CorDebugRegister = 0x2;
pub const REGISTER_RISCV64_RA: CorDebugRegister = 0x3;
pub const REGISTER_RISCV64_GP: CorDebugRegister = 0x4;
pub const REGISTER_RISCV64_TP: CorDebugRegister = 0x5;
pub const REGISTER_RISCV64_T0: CorDebugRegister = 0x6;
pub const REGISTER_RISCV64_T1: CorDebugRegister = 0x7;
pub const REGISTER_RISCV64_T2: CorDebugRegister = 0x8;
pub const REGISTER_RISCV64_S1: CorDebugRegister = 0x9;
pub const REGISTER_RISCV64_A0: CorDebugRegister = 0xa;
pub const REGISTER_RISCV64_A1: CorDebugRegister = 0xb;
pub const REGISTER_RISCV64_A2: CorDebugRegister = 0xc;
pub const REGISTER_RISCV64_A3: CorDebugRegister = 0xd;
pub const REGISTER_RISCV64_A4: CorDebugRegister = 0xe;
pub const REGISTER_RISCV64_A5: CorDebugRegister = 0xf;
pub const REGISTER_RISCV64_A6: CorDebugRegister = 0x10;
pub const REGISTER_RISCV64_A7: CorDebugRegister = 0x11;
pub const REGISTER_RISCV64_S2: CorDebugRegister = 0x12;
pub const REGISTER_RISCV64_S3: CorDebugRegister = 0x13;
pub const REGISTER_RISCV64_S4: CorDebugRegister = 0x14;
pub const REGISTER_RISCV64_S5: CorDebugRegister = 0x15;
pub const REGISTER_RISCV64_S6: CorDebugRegister = 0x16;
pub const REGISTER_RISCV64_S7: CorDebugRegister = 0x17;
pub const REGISTER_RISCV64_S8: CorDebugRegister = 0x18;
pub const REGISTER_RISCV64_S9: CorDebugRegister = 0x19;
pub const REGISTER_RISCV64_S10: CorDebugRegister = 0x1a;
pub const REGISTER_RISCV64_S11: CorDebugRegister = 0x1b;
pub const REGISTER_RISCV64_T3: CorDebugRegister = 0x1c;
pub const REGISTER_RISCV64_T4: CorDebugRegister = 0x1d;
pub const REGISTER_RISCV64_T5: CorDebugRegister = 0x1e;
pub const REGISTER_RISCV64_T6: CorDebugRegister = 0x1f;
pub const REGISTER_RISCV64_F0: CorDebugRegister = 0x20;
pub const REGISTER_RISCV64_F1: CorDebugRegister = 0x21;
pub const REGISTER_RISCV64_F2: CorDebugRegister = 0x22;
pub const REGISTER_RISCV64_F3: CorDebugRegister = 0x23;
pub const REGISTER_RISCV64_F4: CorDebugRegister = 0x24;
pub const REGISTER_RISCV64_F5: CorDebugRegister = 0x25;
pub const REGISTER_RISCV64_F6: CorDebugRegister = 0x26;
pub const REGISTER_RISCV64_F7: CorDebugRegister = 0x27;
pub const REGISTER_RISCV64_F8: CorDebugRegister = 0x28;
pub const REGISTER_RISCV64_F9: CorDebugRegister = 0x29;
pub const REGISTER_RISCV64_F10: CorDebugRegister = 0x2a;
pub const REGISTER_RISCV64_F11: CorDebugRegister = 0x2b;
pub const REGISTER_RISCV64_F12: CorDebugRegister = 0x2c;
pub const REGISTER_RISCV64_F13: CorDebugRegister = 0x2d;
pub const REGISTER_RISCV64_F14: CorDebugRegister = 0x2e;
pub const REGISTER_RISCV64_F15: CorDebugRegister = 0x2f;
pub const REGISTER_RISCV64_F16: CorDebugRegister = 0x30;
pub const REGISTER_RISCV64_F17: CorDebugRegister = 0x31;
pub const REGISTER_RISCV64_F18: CorDebugRegister = 0x32;
pub const REGISTER_RISCV64_F19: CorDebugRegister = 0x33;
pub const REGISTER_RISCV64_F20: CorDebugRegister = 0x34;
pub const REGISTER_RISCV64_F21: CorDebugRegister = 0x35;
pub const REGISTER_RISCV64_F22: CorDebugRegister = 0x36;
pub const REGISTER_RISCV64_F23: CorDebugRegister = 0x37;
pub const REGISTER_RISCV64_F24: CorDebugRegister = 0x38;
pub const REGISTER_RISCV64_F25: CorDebugRegister = 0x39;
pub const REGISTER_RISCV64_F26: CorDebugRegister = 0x3a;
pub const REGISTER_RISCV64_F27: CorDebugRegister = 0x3b;
pub const REGISTER_RISCV64_F28: CorDebugRegister = 0x3c;
pub const REGISTER_RISCV64_F29: CorDebugRegister = 0x3d;
pub const REGISTER_RISCV64_F30: CorDebugRegister = 0x3e;
pub const REGISTER_RISCV64_F31: CorDebugRegister = 0x3f;

pub type CorDebugUserState = u32;
pub const USER_STOP_REQUESTED: CorDebugUserState = 0x1;
pub const USER_SUSPEND_REQUESTED: CorDebugUserState = 0x2;
pub const USER_BACKGROUND: CorDebugUserState = 0x4;
pub const USER_UNSTARTED: CorDebugUserState = 0x8;
pub const USER_STOPPED: CorDebugUserState = 0x10;
pub const USER_WAIT_SLEEP_JOIN: CorDebugUserState = 0x20;
pub const USER_SUSPENDED: CorDebugUserState = 0x40;
pub const USER_UNSAFE_POINT: CorDebugUserState = 0x80;
pub const USER_THREADPOOL: CorDebugUserState = 0x100;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COR_ACTIVE_FUNCTION {
    pub pAppDomain: *mut c_void,
    pub pModule: *mut c_void,
    pub pFunction: *mut c_void,
    pub ilOffset: u32,
    pub flags: u32,
}

pub type CorDebugSetContextFlag = u32;
pub const SET_CONTEXT_FLAG_ACTIVE_FRAME: CorDebugSetContextFlag = 0x1;
pub const SET_CONTEXT_FLAG_UNWIND_FRAME: CorDebugSetContextFlag = 0x2;

pub type CorDebugChainReason = u32;
pub const CHAIN_NONE: CorDebugChainReason = 0x0;
pub const CHAIN_CLASS_INIT: CorDebugChainReason = 0x1;
pub const CHAIN_EXCEPTION_FILTER: CorDebugChainReason = 0x2;
pub const CHAIN_SECURITY: CorDebugChainReason = 0x4;
pub const CHAIN_CONTEXT_POLICY: CorDebugChainReason = 0x8;
pub const CHAIN_INTERCEPTION: CorDebugChainReason = 0x10;
pub const CHAIN_PROCESS_START: CorDebugChainReason = 0x20;
pub const CHAIN_THREAD_START: CorDebugChainReason = 0x40;
pub const CHAIN_ENTER_MANAGED: CorDebugChainReason = 0x80;
pub const CHAIN_ENTER_UNMANAGED: CorDebugChainReason = 0x100;
pub const CHAIN_DEBUGGER_EVAL: CorDebugChainReason = 0x200;
pub const CHAIN_CONTEXT_SWITCH: CorDebugChainReason = 0x400;
pub const CHAIN_FUNC_EVAL: CorDebugChainReason = 0x800;

pub type CorDebugInternalFrameType = u32;
pub const STUBFRAME_NONE: CorDebugInternalFrameType = 0x0;
pub const STUBFRAME_M2U: CorDebugInternalFrameType = 0x1;
pub const STUBFRAME_U2M: CorDebugInternalFrameType = 0x2;
pub const STUBFRAME_APPDOMAIN_TRANSITION: CorDebugInternalFrameType = 0x3;
pub const STUBFRAME_LIGHTWEIGHT_FUNCTION: CorDebugInternalFrameType = 0x4;
pub const STUBFRAME_FUNC_EVAL: CorDebugInternalFrameType = 0x5;
pub const STUBFRAME_INTERNALCALL: CorDebugInternalFrameType = 0x6;
pub const STUBFRAME_CLASS_INIT: CorDebugInternalFrameType = 0x7;
pub const STUBFRAME_EXCEPTION: CorDebugInternalFrameType = 0x8;
pub const STUBFRAME_SECURITY: CorDebugInternalFrameType = 0x9;
pub const STUBFRAME_JIT_COMPILATION: CorDebugInternalFrameType = 0xa;

pub type CorDebugMappingResult = u32;
pub const MAPPING_PROLOG: CorDebugMappingResult = 0x1;
pub const MAPPING_EPILOG: CorDebugMappingResult = 0x2;
pub const MAPPING_NO_INFO: CorDebugMappingResult = 0x4;
pub const MAPPING_UNMAPPED_ADDRESS: CorDebugMappingResult = 0x8;
pub const MAPPING_EXACT: CorDebugMappingResult = 0x10;
pub const MAPPING_APPROXIMATE: CorDebugMappingResult = 0x20;

pub type ILCodeKind = u32;
pub const ILCODE_ORIGINAL_IL: ILCodeKind = 0x1;
pub const ILCODE_REJIT_IL: ILCodeKind = 0x2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodeChunkInfo {
    pub startAddr: CORDB_ADDRESS,
    pub length: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CorDebugEHClause {
    pub Flags: u32,
    pub TryOffset: u32,
    pub TryLength: u32,
    pub HandlerOffset: u32,
    pub HandlerLength: u32,
    pub ClassToken: u32,
    pub FilterOffset: u32,
}

pub type VariableLocationType = u32;
pub const VLT_REGISTER: VariableLocationType = 0x0;
pub const VLT_REGISTER_RELATIVE: VariableLocationType = 0x1;
pub const VLT_INVALID: VariableLocationType = 0x2;

pub type CorDebugMDAFlags = u32;
pub const MDA_FLAG_SLIP: CorDebugMDAFlags = 0x2;

// ---- ICorDebugDataTarget : IUnknown
#[repr(C)]
pub struct ICorDebugDataTargetVtbl {
    pub base: IUnknownVtbl,
    pub GetPlatform: unsafe extern "system" fn(this: *mut c_void, pTargetPlatform: *mut CorDebugPlatform) -> HRESULT,
    pub ReadVirtual: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, pBuffer: *mut u8, bytesRequested: u32, pBytesRead: *mut u32) -> HRESULT,
    pub GetThreadContext: unsafe extern "system" fn(this: *mut c_void, dwThreadID: u32, contextFlags: u32, contextSize: u32, pContext: *mut u8) -> HRESULT,
}
pub enum ICorDebugDataTarget {}
unsafe impl Interface for ICorDebugDataTarget {
    const IID: GUID = GUID { data1: 0xFE06DC28, data2: 0x49FB, data3: 0x4636, data4: [0xA4, 0xA3, 0xE8, 0x0D, 0xB4, 0xAE, 0x11, 0x6C] };
    type Vtbl = ICorDebugDataTargetVtbl;
}
impl ComPtr<ICorDebugDataTarget> {
    #[inline]
    pub unsafe fn GetPlatform(&self, pTargetPlatform: *mut CorDebugPlatform) -> HRESULT {
        (self.vtbl().GetPlatform)(self.as_raw(), pTargetPlatform)
    }
    #[inline]
    pub unsafe fn ReadVirtual(&self, address: CORDB_ADDRESS, pBuffer: *mut u8, bytesRequested: u32, pBytesRead: *mut u32) -> HRESULT {
        (self.vtbl().ReadVirtual)(self.as_raw(), address, pBuffer, bytesRequested, pBytesRead)
    }
    #[inline]
    pub unsafe fn GetThreadContext(&self, dwThreadID: u32, contextFlags: u32, contextSize: u32, pContext: *mut u8) -> HRESULT {
        (self.vtbl().GetThreadContext)(self.as_raw(), dwThreadID, contextFlags, contextSize, pContext)
    }
}

// ---- ICorDebugStaticFieldSymbol : IUnknown
#[repr(C)]
pub struct ICorDebugStaticFieldSymbolVtbl {
    pub base: IUnknownVtbl,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcbSize: *mut u32) -> HRESULT,
    pub GetAddress: unsafe extern "system" fn(this: *mut c_void, pRVA: *mut CORDB_ADDRESS) -> HRESULT,
}
pub enum ICorDebugStaticFieldSymbol {}
unsafe impl Interface for ICorDebugStaticFieldSymbol {
    const IID: GUID = GUID { data1: 0xCBF9DA63, data2: 0xF68D, data3: 0x4BBB, data4: [0xA2, 0x1C, 0x15, 0xA4, 0x5E, 0xAA, 0xDF, 0x5B] };
    type Vtbl = ICorDebugStaticFieldSymbolVtbl;
}
impl ComPtr<ICorDebugStaticFieldSymbol> {
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcbSize: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcbSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pRVA: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetAddress)(self.as_raw(), pRVA)
    }
}

// ---- ICorDebugInstanceFieldSymbol : IUnknown
#[repr(C)]
pub struct ICorDebugInstanceFieldSymbolVtbl {
    pub base: IUnknownVtbl,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcbSize: *mut u32) -> HRESULT,
    pub GetOffset: unsafe extern "system" fn(this: *mut c_void, pcbOffset: *mut u32) -> HRESULT,
}
pub enum ICorDebugInstanceFieldSymbol {}
unsafe impl Interface for ICorDebugInstanceFieldSymbol {
    const IID: GUID = GUID { data1: 0xA074096B, data2: 0x3ADC, data3: 0x4485, data4: [0x81, 0xDA, 0x68, 0xC7, 0xA4, 0xEA, 0x52, 0xDB] };
    type Vtbl = ICorDebugInstanceFieldSymbolVtbl;
}
impl ComPtr<ICorDebugInstanceFieldSymbol> {
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcbSize: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcbSize)
    }
    #[inline]
    pub unsafe fn GetOffset(&self, pcbOffset: *mut u32) -> HRESULT {
        (self.vtbl().GetOffset)(self.as_raw(), pcbOffset)
    }
}

// ---- ICorDebugVariableSymbol : IUnknown
#[repr(C)]
pub struct ICorDebugVariableSymbolVtbl {
    pub base: IUnknownVtbl,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcbValue: *mut u32) -> HRESULT,
    pub GetValue: unsafe extern "system" fn(this: *mut c_void, offset: u32, cbContext: u32, context: *mut u8, cbValue: u32, pcbValue: *mut u32, pValue: *mut u8) -> HRESULT,
    pub SetValue: unsafe extern "system" fn(this: *mut c_void, offset: u32, threadID: u32, cbContext: u32, context: *mut u8, cbValue: u32, pValue: *mut u8) -> HRESULT,
    pub GetSlotIndex: unsafe extern "system" fn(this: *mut c_void, pSlotIndex: *mut u32) -> HRESULT,
}
pub enum ICorDebugVariableSymbol {}
unsafe impl Interface for ICorDebugVariableSymbol {
    const IID: GUID = GUID { data1: 0x707E8932, data2: 0x1163, data3: 0x48D9, data4: [0x8A, 0x93, 0xF5, 0xB1, 0xF4, 0x80, 0xFB, 0xB7] };
    type Vtbl = ICorDebugVariableSymbolVtbl;
}
impl ComPtr<ICorDebugVariableSymbol> {
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcbValue: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcbValue)
    }
    #[inline]
    pub unsafe fn GetValue(&self, offset: u32, cbContext: u32, context: *mut u8, cbValue: u32, pcbValue: *mut u32, pValue: *mut u8) -> HRESULT {
        (self.vtbl().GetValue)(self.as_raw(), offset, cbContext, context, cbValue, pcbValue, pValue)
    }
    #[inline]
    pub unsafe fn SetValue(&self, offset: u32, threadID: u32, cbContext: u32, context: *mut u8, cbValue: u32, pValue: *mut u8) -> HRESULT {
        (self.vtbl().SetValue)(self.as_raw(), offset, threadID, cbContext, context, cbValue, pValue)
    }
    #[inline]
    pub unsafe fn GetSlotIndex(&self, pSlotIndex: *mut u32) -> HRESULT {
        (self.vtbl().GetSlotIndex)(self.as_raw(), pSlotIndex)
    }
}

// ---- ICorDebugMemoryBuffer : IUnknown
#[repr(C)]
pub struct ICorDebugMemoryBufferVtbl {
    pub base: IUnknownVtbl,
    pub GetStartAddress: unsafe extern "system" fn(this: *mut c_void, address: *mut LPCVOID) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcbBufferLength: *mut u32) -> HRESULT,
}
pub enum ICorDebugMemoryBuffer {}
unsafe impl Interface for ICorDebugMemoryBuffer {
    const IID: GUID = GUID { data1: 0x677888B3, data2: 0xD160, data3: 0x4B8C, data4: [0xA7, 0x3B, 0xD7, 0x9E, 0x6A, 0xAA, 0x1D, 0x13] };
    type Vtbl = ICorDebugMemoryBufferVtbl;
}
impl ComPtr<ICorDebugMemoryBuffer> {
    #[inline]
    pub unsafe fn GetStartAddress(&self, address: *mut LPCVOID) -> HRESULT {
        (self.vtbl().GetStartAddress)(self.as_raw(), address)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcbBufferLength: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcbBufferLength)
    }
}

// ---- ICorDebugMergedAssemblyRecord : IUnknown
#[repr(C)]
pub struct ICorDebugMergedAssemblyRecordVtbl {
    pub base: IUnknownVtbl,
    pub GetSimpleName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetVersion: unsafe extern "system" fn(this: *mut c_void, pMajor: *mut u16, pMinor: *mut u16, pBuild: *mut u16, pRevision: *mut u16) -> HRESULT,
    pub GetCulture: unsafe extern "system" fn(this: *mut c_void, cchCulture: u32, pcchCulture: *mut u32, szCulture: *mut u16) -> HRESULT,
    pub GetPublicKey: unsafe extern "system" fn(this: *mut c_void, cbPublicKey: u32, pcbPublicKey: *mut u32, pbPublicKey: *mut u8) -> HRESULT,
    pub GetPublicKeyToken: unsafe extern "system" fn(this: *mut c_void, cbPublicKeyToken: u32, pcbPublicKeyToken: *mut u32, pbPublicKeyToken: *mut u8) -> HRESULT,
    pub GetIndex: unsafe extern "system" fn(this: *mut c_void, pIndex: *mut u32) -> HRESULT,
}
pub enum ICorDebugMergedAssemblyRecord {}
unsafe impl Interface for ICorDebugMergedAssemblyRecord {
    const IID: GUID = GUID { data1: 0xFAA8637B, data2: 0x3BBE, data3: 0x4671, data4: [0x8E, 0x26, 0x3B, 0x59, 0x87, 0x5B, 0x92, 0x2A] };
    type Vtbl = ICorDebugMergedAssemblyRecordVtbl;
}
impl ComPtr<ICorDebugMergedAssemblyRecord> {
    #[inline]
    pub unsafe fn GetSimpleName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetSimpleName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetVersion(&self, pMajor: *mut u16, pMinor: *mut u16, pBuild: *mut u16, pRevision: *mut u16) -> HRESULT {
        (self.vtbl().GetVersion)(self.as_raw(), pMajor, pMinor, pBuild, pRevision)
    }
    #[inline]
    pub unsafe fn GetCulture(&self, cchCulture: u32, pcchCulture: *mut u32, szCulture: *mut u16) -> HRESULT {
        (self.vtbl().GetCulture)(self.as_raw(), cchCulture, pcchCulture, szCulture)
    }
    #[inline]
    pub unsafe fn GetPublicKey(&self, cbPublicKey: u32, pcbPublicKey: *mut u32, pbPublicKey: *mut u8) -> HRESULT {
        (self.vtbl().GetPublicKey)(self.as_raw(), cbPublicKey, pcbPublicKey, pbPublicKey)
    }
    #[inline]
    pub unsafe fn GetPublicKeyToken(&self, cbPublicKeyToken: u32, pcbPublicKeyToken: *mut u32, pbPublicKeyToken: *mut u8) -> HRESULT {
        (self.vtbl().GetPublicKeyToken)(self.as_raw(), cbPublicKeyToken, pcbPublicKeyToken, pbPublicKeyToken)
    }
    #[inline]
    pub unsafe fn GetIndex(&self, pIndex: *mut u32) -> HRESULT {
        (self.vtbl().GetIndex)(self.as_raw(), pIndex)
    }
}

// ---- ICorDebugSymbolProvider : IUnknown
#[repr(C)]
pub struct ICorDebugSymbolProviderVtbl {
    pub base: IUnknownVtbl,
    pub GetStaticFieldSymbols: unsafe extern "system" fn(this: *mut c_void, cbSignature: u32, typeSig: *mut u8, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT,
    pub GetInstanceFieldSymbols: unsafe extern "system" fn(this: *mut c_void, cbSignature: u32, typeSig: *mut u8, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT,
    pub GetMethodLocalSymbols: unsafe extern "system" fn(this: *mut c_void, nativeRVA: u32, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT,
    pub GetMethodParameterSymbols: unsafe extern "system" fn(this: *mut c_void, nativeRVA: u32, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT,
    pub GetMergedAssemblyRecords: unsafe extern "system" fn(this: *mut c_void, cRequestedRecords: u32, pcFetchedRecords: *mut u32, pRecords: *mut *mut c_void) -> HRESULT,
    pub GetMethodProps: unsafe extern "system" fn(this: *mut c_void, codeRva: u32, pMethodToken: *mut mdToken, pcGenericParams: *mut u32, cbSignature: u32, pcbSignature: *mut u32, signature: *mut u8) -> HRESULT,
    pub GetTypeProps: unsafe extern "system" fn(this: *mut c_void, vtableRva: u32, cbSignature: u32, pcbSignature: *mut u32, signature: *mut u8) -> HRESULT,
    pub GetCodeRange: unsafe extern "system" fn(this: *mut c_void, codeRva: u32, pCodeStartAddress: *mut u32, pCodeSize: *mut u32) -> HRESULT,
    pub GetAssemblyImageBytes: unsafe extern "system" fn(this: *mut c_void, rva: CORDB_ADDRESS, length: u32, ppMemoryBuffer: *mut *mut c_void) -> HRESULT,
    pub GetObjectSize: unsafe extern "system" fn(this: *mut c_void, cbSignature: u32, typeSig: *mut u8, pObjectSize: *mut u32) -> HRESULT,
    pub GetAssemblyImageMetadata: unsafe extern "system" fn(this: *mut c_void, ppMemoryBuffer: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugSymbolProvider {}
unsafe impl Interface for ICorDebugSymbolProvider {
    const IID: GUID = GUID { data1: 0x3948A999, data2: 0xFD8A, data3: 0x4C38, data4: [0xA7, 0x08, 0x8A, 0x71, 0xE9, 0xB0, 0x4D, 0xBB] };
    type Vtbl = ICorDebugSymbolProviderVtbl;
}
impl ComPtr<ICorDebugSymbolProvider> {
    #[inline]
    pub unsafe fn GetStaticFieldSymbols(&self, cbSignature: u32, typeSig: *mut u8, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetStaticFieldSymbols)(self.as_raw(), cbSignature, typeSig, cRequestedSymbols, pcFetchedSymbols, pSymbols)
    }
    #[inline]
    pub unsafe fn GetInstanceFieldSymbols(&self, cbSignature: u32, typeSig: *mut u8, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetInstanceFieldSymbols)(self.as_raw(), cbSignature, typeSig, cRequestedSymbols, pcFetchedSymbols, pSymbols)
    }
    #[inline]
    pub unsafe fn GetMethodLocalSymbols(&self, nativeRVA: u32, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetMethodLocalSymbols)(self.as_raw(), nativeRVA, cRequestedSymbols, pcFetchedSymbols, pSymbols)
    }
    #[inline]
    pub unsafe fn GetMethodParameterSymbols(&self, nativeRVA: u32, cRequestedSymbols: u32, pcFetchedSymbols: *mut u32, pSymbols: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetMethodParameterSymbols)(self.as_raw(), nativeRVA, cRequestedSymbols, pcFetchedSymbols, pSymbols)
    }
    #[inline]
    pub unsafe fn GetMergedAssemblyRecords(&self, cRequestedRecords: u32, pcFetchedRecords: *mut u32, pRecords: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetMergedAssemblyRecords)(self.as_raw(), cRequestedRecords, pcFetchedRecords, pRecords)
    }
    #[inline]
    pub unsafe fn GetMethodProps(&self, codeRva: u32, pMethodToken: *mut mdToken, pcGenericParams: *mut u32, cbSignature: u32, pcbSignature: *mut u32, signature: *mut u8) -> HRESULT {
        (self.vtbl().GetMethodProps)(self.as_raw(), codeRva, pMethodToken, pcGenericParams, cbSignature, pcbSignature, signature)
    }
    #[inline]
    pub unsafe fn GetTypeProps(&self, vtableRva: u32, cbSignature: u32, pcbSignature: *mut u32, signature: *mut u8) -> HRESULT {
        (self.vtbl().GetTypeProps)(self.as_raw(), vtableRva, cbSignature, pcbSignature, signature)
    }
    #[inline]
    pub unsafe fn GetCodeRange(&self, codeRva: u32, pCodeStartAddress: *mut u32, pCodeSize: *mut u32) -> HRESULT {
        (self.vtbl().GetCodeRange)(self.as_raw(), codeRva, pCodeStartAddress, pCodeSize)
    }
    #[inline]
    pub unsafe fn GetAssemblyImageBytes(&self, rva: CORDB_ADDRESS, length: u32, ppMemoryBuffer: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAssemblyImageBytes)(self.as_raw(), rva, length, ppMemoryBuffer)
    }
    #[inline]
    pub unsafe fn GetObjectSize(&self, cbSignature: u32, typeSig: *mut u8, pObjectSize: *mut u32) -> HRESULT {
        (self.vtbl().GetObjectSize)(self.as_raw(), cbSignature, typeSig, pObjectSize)
    }
    #[inline]
    pub unsafe fn GetAssemblyImageMetadata(&self, ppMemoryBuffer: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAssemblyImageMetadata)(self.as_raw(), ppMemoryBuffer)
    }
}

// ---- ICorDebugSymbolProvider2 : IUnknown
#[repr(C)]
pub struct ICorDebugSymbolProvider2Vtbl {
    pub base: IUnknownVtbl,
    pub GetGenericDictionaryInfo: unsafe extern "system" fn(this: *mut c_void, ppMemoryBuffer: *mut *mut c_void) -> HRESULT,
    pub GetFrameProps: unsafe extern "system" fn(this: *mut c_void, codeRva: u32, pCodeStartRva: *mut u32, pParentFrameStartRva: *mut u32) -> HRESULT,
}
pub enum ICorDebugSymbolProvider2 {}
unsafe impl Interface for ICorDebugSymbolProvider2 {
    const IID: GUID = GUID { data1: 0xF9801807, data2: 0x4764, data3: 0x4330, data4: [0x9E, 0x67, 0x4F, 0x68, 0x50, 0x94, 0x16, 0x5E] };
    type Vtbl = ICorDebugSymbolProvider2Vtbl;
}
impl ComPtr<ICorDebugSymbolProvider2> {
    #[inline]
    pub unsafe fn GetGenericDictionaryInfo(&self, ppMemoryBuffer: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetGenericDictionaryInfo)(self.as_raw(), ppMemoryBuffer)
    }
    #[inline]
    pub unsafe fn GetFrameProps(&self, codeRva: u32, pCodeStartRva: *mut u32, pParentFrameStartRva: *mut u32) -> HRESULT {
        (self.vtbl().GetFrameProps)(self.as_raw(), codeRva, pCodeStartRva, pParentFrameStartRva)
    }
}

// ---- ICorDebugVirtualUnwinder : IUnknown
#[repr(C)]
pub struct ICorDebugVirtualUnwinderVtbl {
    pub base: IUnknownVtbl,
    pub GetContext: unsafe extern "system" fn(this: *mut c_void, contextFlags: u32, cbContextBuf: u32, contextSize: *mut u32, contextBuf: *mut u8) -> HRESULT,
    pub Next: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
}
pub enum ICorDebugVirtualUnwinder {}
unsafe impl Interface for ICorDebugVirtualUnwinder {
    const IID: GUID = GUID { data1: 0xF69126B7, data2: 0xC787, data3: 0x4F6B, data4: [0xAE, 0x96, 0xA5, 0x69, 0x78, 0x6F, 0xC6, 0x70] };
    type Vtbl = ICorDebugVirtualUnwinderVtbl;
}
impl ComPtr<ICorDebugVirtualUnwinder> {
    #[inline]
    pub unsafe fn GetContext(&self, contextFlags: u32, cbContextBuf: u32, contextSize: *mut u32, contextBuf: *mut u8) -> HRESULT {
        (self.vtbl().GetContext)(self.as_raw(), contextFlags, cbContextBuf, contextSize, contextBuf)
    }
    #[inline]
    pub unsafe fn Next(&self) -> HRESULT {
        (self.vtbl().Next)(self.as_raw())
    }
}

// ---- ICorDebugDataTarget2 : IUnknown
#[repr(C)]
pub struct ICorDebugDataTarget2Vtbl {
    pub base: IUnknownVtbl,
    pub GetImageFromPointer: unsafe extern "system" fn(this: *mut c_void, addr: CORDB_ADDRESS, pImageBase: *mut CORDB_ADDRESS, pSize: *mut u32) -> HRESULT,
    pub GetImageLocation: unsafe extern "system" fn(this: *mut c_void, baseAddress: CORDB_ADDRESS, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetSymbolProviderForImage: unsafe extern "system" fn(this: *mut c_void, imageBaseAddress: CORDB_ADDRESS, ppSymProvider: *mut *mut c_void) -> HRESULT,
    pub EnumerateThreadIDs: unsafe extern "system" fn(this: *mut c_void, cThreadIds: u32, pcThreadIds: *mut u32, pThreadIds: *mut u32) -> HRESULT,
    pub CreateVirtualUnwinder: unsafe extern "system" fn(this: *mut c_void, nativeThreadID: u32, contextFlags: u32, cbContext: u32, initialContext: *mut u8, ppUnwinder: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugDataTarget2 {}
unsafe impl Interface for ICorDebugDataTarget2 {
    const IID: GUID = GUID { data1: 0x2eb364da, data2: 0x605b, data3: 0x4e8d, data4: [0xb3, 0x33, 0x33, 0x94, 0xc4, 0x82, 0x8d, 0x41] };
    type Vtbl = ICorDebugDataTarget2Vtbl;
}
impl ComPtr<ICorDebugDataTarget2> {
    #[inline]
    pub unsafe fn GetImageFromPointer(&self, addr: CORDB_ADDRESS, pImageBase: *mut CORDB_ADDRESS, pSize: *mut u32) -> HRESULT {
        (self.vtbl().GetImageFromPointer)(self.as_raw(), addr, pImageBase, pSize)
    }
    #[inline]
    pub unsafe fn GetImageLocation(&self, baseAddress: CORDB_ADDRESS, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetImageLocation)(self.as_raw(), baseAddress, cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetSymbolProviderForImage(&self, imageBaseAddress: CORDB_ADDRESS, ppSymProvider: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetSymbolProviderForImage)(self.as_raw(), imageBaseAddress, ppSymProvider)
    }
    #[inline]
    pub unsafe fn EnumerateThreadIDs(&self, cThreadIds: u32, pcThreadIds: *mut u32, pThreadIds: *mut u32) -> HRESULT {
        (self.vtbl().EnumerateThreadIDs)(self.as_raw(), cThreadIds, pcThreadIds, pThreadIds)
    }
    #[inline]
    pub unsafe fn CreateVirtualUnwinder(&self, nativeThreadID: u32, contextFlags: u32, cbContext: u32, initialContext: *mut u8, ppUnwinder: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateVirtualUnwinder)(self.as_raw(), nativeThreadID, contextFlags, cbContext, initialContext, ppUnwinder)
    }
}

// ---- ICorDebugLoadedModule : IUnknown
#[repr(C)]
pub struct ICorDebugLoadedModuleVtbl {
    pub base: IUnknownVtbl,
    pub GetBaseAddress: unsafe extern "system" fn(this: *mut c_void, pAddress: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcBytes: *mut u32) -> HRESULT,
}
pub enum ICorDebugLoadedModule {}
unsafe impl Interface for ICorDebugLoadedModule {
    const IID: GUID = GUID { data1: 0x817F343A, data2: 0x6630, data3: 0x4578, data4: [0x96, 0xC5, 0xD1, 0x1B, 0xC0, 0xEC, 0x5E, 0xE2] };
    type Vtbl = ICorDebugLoadedModuleVtbl;
}
impl ComPtr<ICorDebugLoadedModule> {
    #[inline]
    pub unsafe fn GetBaseAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetBaseAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcBytes: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcBytes)
    }
}

// ---- ICorDebugDataTarget3 : IUnknown
#[repr(C)]
pub struct ICorDebugDataTarget3Vtbl {
    pub base: IUnknownVtbl,
    pub GetLoadedModules: unsafe extern "system" fn(this: *mut c_void, cRequestedModules: u32, pcFetchedModules: *mut u32, pLoadedModules: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugDataTarget3 {}
unsafe impl Interface for ICorDebugDataTarget3 {
    const IID: GUID = GUID { data1: 0xD05E60C3, data2: 0x848C, data3: 0x4E7D, data4: [0x89, 0x4E, 0x62, 0x33, 0x20, 0xFF, 0x6A, 0xFA] };
    type Vtbl = ICorDebugDataTarget3Vtbl;
}
impl ComPtr<ICorDebugDataTarget3> {
    #[inline]
    pub unsafe fn GetLoadedModules(&self, cRequestedModules: u32, pcFetchedModules: *mut u32, pLoadedModules: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLoadedModules)(self.as_raw(), cRequestedModules, pcFetchedModules, pLoadedModules)
    }
}

// ---- ICorDebugDataTarget4 : IUnknown
#[repr(C)]
pub struct ICorDebugDataTarget4Vtbl {
    pub base: IUnknownVtbl,
    pub VirtualUnwind: unsafe extern "system" fn(this: *mut c_void, threadId: u32, contextSize: u32, context: *mut u8) -> HRESULT,
}
pub enum ICorDebugDataTarget4 {}
unsafe impl Interface for ICorDebugDataTarget4 {
    const IID: GUID = GUID { data1: 0xE799DC06, data2: 0xE099, data3: 0x4713, data4: [0xBD, 0xD9, 0x90, 0x6D, 0x3C, 0xC0, 0x2C, 0xF2] };
    type Vtbl = ICorDebugDataTarget4Vtbl;
}
impl ComPtr<ICorDebugDataTarget4> {
    #[inline]
    pub unsafe fn VirtualUnwind(&self, threadId: u32, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().VirtualUnwind)(self.as_raw(), threadId, contextSize, context)
    }
}

// ---- ICorDebugMutableDataTarget : ICorDebugDataTarget
#[repr(C)]
pub struct ICorDebugMutableDataTargetVtbl {
    pub base: ICorDebugDataTargetVtbl,
    pub WriteVirtual: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, pBuffer: *mut u8, bytesRequested: u32) -> HRESULT,
    pub SetThreadContext: unsafe extern "system" fn(this: *mut c_void, dwThreadID: u32, contextSize: u32, pContext: *mut u8) -> HRESULT,
    pub ContinueStatusChanged: unsafe extern "system" fn(this: *mut c_void, dwThreadId: u32, continueStatus: CORDB_CONTINUE_STATUS) -> HRESULT,
}
pub enum ICorDebugMutableDataTarget {}
unsafe impl Interface for ICorDebugMutableDataTarget {
    const IID: GUID = GUID { data1: 0xA1B8A756, data2: 0x3CB6, data3: 0x4CCB, data4: [0x97, 0x9F, 0x3D, 0xF9, 0x99, 0x67, 0x3A, 0x59] };
    type Vtbl = ICorDebugMutableDataTargetVtbl;
}
impl ComPtr<ICorDebugMutableDataTarget> {
    #[inline]
    pub unsafe fn WriteVirtual(&self, address: CORDB_ADDRESS, pBuffer: *mut u8, bytesRequested: u32) -> HRESULT {
        (self.vtbl().WriteVirtual)(self.as_raw(), address, pBuffer, bytesRequested)
    }
    #[inline]
    pub unsafe fn SetThreadContext(&self, dwThreadID: u32, contextSize: u32, pContext: *mut u8) -> HRESULT {
        (self.vtbl().SetThreadContext)(self.as_raw(), dwThreadID, contextSize, pContext)
    }
    #[inline]
    pub unsafe fn ContinueStatusChanged(&self, dwThreadId: u32, continueStatus: CORDB_CONTINUE_STATUS) -> HRESULT {
        (self.vtbl().ContinueStatusChanged)(self.as_raw(), dwThreadId, continueStatus)
    }
    #[inline]
    pub unsafe fn GetPlatform(&self, pTargetPlatform: *mut CorDebugPlatform) -> HRESULT {
        (self.vtbl().base.GetPlatform)(self.as_raw(), pTargetPlatform)
    }
    #[inline]
    pub unsafe fn ReadVirtual(&self, address: CORDB_ADDRESS, pBuffer: *mut u8, bytesRequested: u32, pBytesRead: *mut u32) -> HRESULT {
        (self.vtbl().base.ReadVirtual)(self.as_raw(), address, pBuffer, bytesRequested, pBytesRead)
    }
    #[inline]
    pub unsafe fn GetThreadContext(&self, dwThreadID: u32, contextFlags: u32, contextSize: u32, pContext: *mut u8) -> HRESULT {
        (self.vtbl().base.GetThreadContext)(self.as_raw(), dwThreadID, contextFlags, contextSize, pContext)
    }
}

// ---- ICorDebugMetaDataLocator : IUnknown
#[repr(C)]
pub struct ICorDebugMetaDataLocatorVtbl {
    pub base: IUnknownVtbl,
    pub GetMetaData: unsafe extern "system" fn(this: *mut c_void, wszImagePath: *const u16, dwImageTimeStamp: u32, dwImageSize: u32, cchPathBuffer: u32, pcchPathBuffer: *mut u32, wszPathBuffer: *mut u16) -> HRESULT,
}
pub enum ICorDebugMetaDataLocator {}
unsafe impl Interface for ICorDebugMetaDataLocator {
    const IID: GUID = GUID { data1: 0x7cef8ba9, data2: 0x2ef7, data3: 0x42bf, data4: [0x97, 0x3f, 0x41, 0x71, 0x47, 0x4f, 0x87, 0xd9] };
    type Vtbl = ICorDebugMetaDataLocatorVtbl;
}
impl ComPtr<ICorDebugMetaDataLocator> {
    #[inline]
    pub unsafe fn GetMetaData(&self, wszImagePath: *const u16, dwImageTimeStamp: u32, dwImageSize: u32, cchPathBuffer: u32, pcchPathBuffer: *mut u32, wszPathBuffer: *mut u16) -> HRESULT {
        (self.vtbl().GetMetaData)(self.as_raw(), wszImagePath, dwImageTimeStamp, dwImageSize, cchPathBuffer, pcchPathBuffer, wszPathBuffer)
    }
}

// ---- ICorDebugManagedCallback : IUnknown
#[repr(C)]
pub struct ICorDebugManagedCallbackVtbl {
    pub base: IUnknownVtbl,
    pub Breakpoint: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pBreakpoint: *mut c_void) -> HRESULT,
    pub StepComplete: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pStepper: *mut c_void, reason: CorDebugStepReason) -> HRESULT,
    pub Break: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT,
    pub Exception: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, unhandled: i32) -> HRESULT,
    pub EvalComplete: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pEval: *mut c_void) -> HRESULT,
    pub EvalException: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pEval: *mut c_void) -> HRESULT,
    pub CreateProcess: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void) -> HRESULT,
    pub ExitProcess: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void) -> HRESULT,
    pub CreateThread: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT,
    pub ExitThread: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT,
    pub LoadModule: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pModule: *mut c_void) -> HRESULT,
    pub UnloadModule: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pModule: *mut c_void) -> HRESULT,
    pub LoadClass: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, c: *mut c_void) -> HRESULT,
    pub UnloadClass: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, c: *mut c_void) -> HRESULT,
    pub DebuggerError: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, errorHR: HRESULT, errorCode: u32) -> HRESULT,
    pub LogMessage: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, lLevel: i32, pLogSwitchName: *mut u16, pMessage: *mut u16) -> HRESULT,
    pub LogSwitch: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, lLevel: i32, ulReason: u32, pLogSwitchName: *mut u16, pParentName: *mut u16) -> HRESULT,
    pub CreateAppDomain: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, pAppDomain: *mut c_void) -> HRESULT,
    pub ExitAppDomain: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, pAppDomain: *mut c_void) -> HRESULT,
    pub LoadAssembly: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pAssembly: *mut c_void) -> HRESULT,
    pub UnloadAssembly: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pAssembly: *mut c_void) -> HRESULT,
    pub ControlCTrap: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void) -> HRESULT,
    pub NameChange: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void) -> HRESULT,
    pub UpdateModuleSymbols: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pModule: *mut c_void, pSymbolStream: *mut c_void) -> HRESULT,
    pub EditAndContinueRemap: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pFunction: *mut c_void, fAccurate: i32) -> HRESULT,
    pub BreakpointSetError: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pBreakpoint: *mut c_void, dwError: u32) -> HRESULT,
}
pub enum ICorDebugManagedCallback {}
unsafe impl Interface for ICorDebugManagedCallback {
    const IID: GUID = GUID { data1: 0x3d6f5f60, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef] };
    type Vtbl = ICorDebugManagedCallbackVtbl;
}
impl ComPtr<ICorDebugManagedCallback> {
    #[inline]
    pub unsafe fn Breakpoint(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pBreakpoint: *mut c_void) -> HRESULT {
        (self.vtbl().Breakpoint)(self.as_raw(), pAppDomain, pThread, pBreakpoint)
    }
    #[inline]
    pub unsafe fn StepComplete(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pStepper: *mut c_void, reason: CorDebugStepReason) -> HRESULT {
        (self.vtbl().StepComplete)(self.as_raw(), pAppDomain, pThread, pStepper, reason)
    }
    #[inline]
    pub unsafe fn Break(&self, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT {
        (self.vtbl().Break)(self.as_raw(), pAppDomain, thread)
    }
    #[inline]
    pub unsafe fn Exception(&self, pAppDomain: *mut c_void, pThread: *mut c_void, unhandled: i32) -> HRESULT {
        (self.vtbl().Exception)(self.as_raw(), pAppDomain, pThread, unhandled)
    }
    #[inline]
    pub unsafe fn EvalComplete(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pEval: *mut c_void) -> HRESULT {
        (self.vtbl().EvalComplete)(self.as_raw(), pAppDomain, pThread, pEval)
    }
    #[inline]
    pub unsafe fn EvalException(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pEval: *mut c_void) -> HRESULT {
        (self.vtbl().EvalException)(self.as_raw(), pAppDomain, pThread, pEval)
    }
    #[inline]
    pub unsafe fn CreateProcess(&self, pProcess: *mut c_void) -> HRESULT {
        (self.vtbl().CreateProcess)(self.as_raw(), pProcess)
    }
    #[inline]
    pub unsafe fn ExitProcess(&self, pProcess: *mut c_void) -> HRESULT {
        (self.vtbl().ExitProcess)(self.as_raw(), pProcess)
    }
    #[inline]
    pub unsafe fn CreateThread(&self, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT {
        (self.vtbl().CreateThread)(self.as_raw(), pAppDomain, thread)
    }
    #[inline]
    pub unsafe fn ExitThread(&self, pAppDomain: *mut c_void, thread: *mut c_void) -> HRESULT {
        (self.vtbl().ExitThread)(self.as_raw(), pAppDomain, thread)
    }
    #[inline]
    pub unsafe fn LoadModule(&self, pAppDomain: *mut c_void, pModule: *mut c_void) -> HRESULT {
        (self.vtbl().LoadModule)(self.as_raw(), pAppDomain, pModule)
    }
    #[inline]
    pub unsafe fn UnloadModule(&self, pAppDomain: *mut c_void, pModule: *mut c_void) -> HRESULT {
        (self.vtbl().UnloadModule)(self.as_raw(), pAppDomain, pModule)
    }
    #[inline]
    pub unsafe fn LoadClass(&self, pAppDomain: *mut c_void, c: *mut c_void) -> HRESULT {
        (self.vtbl().LoadClass)(self.as_raw(), pAppDomain, c)
    }
    #[inline]
    pub unsafe fn UnloadClass(&self, pAppDomain: *mut c_void, c: *mut c_void) -> HRESULT {
        (self.vtbl().UnloadClass)(self.as_raw(), pAppDomain, c)
    }
    #[inline]
    pub unsafe fn DebuggerError(&self, pProcess: *mut c_void, errorHR: HRESULT, errorCode: u32) -> HRESULT {
        (self.vtbl().DebuggerError)(self.as_raw(), pProcess, errorHR, errorCode)
    }
    #[inline]
    pub unsafe fn LogMessage(&self, pAppDomain: *mut c_void, pThread: *mut c_void, lLevel: i32, pLogSwitchName: *mut u16, pMessage: *mut u16) -> HRESULT {
        (self.vtbl().LogMessage)(self.as_raw(), pAppDomain, pThread, lLevel, pLogSwitchName, pMessage)
    }
    #[inline]
    pub unsafe fn LogSwitch(&self, pAppDomain: *mut c_void, pThread: *mut c_void, lLevel: i32, ulReason: u32, pLogSwitchName: *mut u16, pParentName: *mut u16) -> HRESULT {
        (self.vtbl().LogSwitch)(self.as_raw(), pAppDomain, pThread, lLevel, ulReason, pLogSwitchName, pParentName)
    }
    #[inline]
    pub unsafe fn CreateAppDomain(&self, pProcess: *mut c_void, pAppDomain: *mut c_void) -> HRESULT {
        (self.vtbl().CreateAppDomain)(self.as_raw(), pProcess, pAppDomain)
    }
    #[inline]
    pub unsafe fn ExitAppDomain(&self, pProcess: *mut c_void, pAppDomain: *mut c_void) -> HRESULT {
        (self.vtbl().ExitAppDomain)(self.as_raw(), pProcess, pAppDomain)
    }
    #[inline]
    pub unsafe fn LoadAssembly(&self, pAppDomain: *mut c_void, pAssembly: *mut c_void) -> HRESULT {
        (self.vtbl().LoadAssembly)(self.as_raw(), pAppDomain, pAssembly)
    }
    #[inline]
    pub unsafe fn UnloadAssembly(&self, pAppDomain: *mut c_void, pAssembly: *mut c_void) -> HRESULT {
        (self.vtbl().UnloadAssembly)(self.as_raw(), pAppDomain, pAssembly)
    }
    #[inline]
    pub unsafe fn ControlCTrap(&self, pProcess: *mut c_void) -> HRESULT {
        (self.vtbl().ControlCTrap)(self.as_raw(), pProcess)
    }
    #[inline]
    pub unsafe fn NameChange(&self, pAppDomain: *mut c_void, pThread: *mut c_void) -> HRESULT {
        (self.vtbl().NameChange)(self.as_raw(), pAppDomain, pThread)
    }
    #[inline]
    pub unsafe fn UpdateModuleSymbols(&self, pAppDomain: *mut c_void, pModule: *mut c_void, pSymbolStream: *mut c_void) -> HRESULT {
        (self.vtbl().UpdateModuleSymbols)(self.as_raw(), pAppDomain, pModule, pSymbolStream)
    }
    #[inline]
    pub unsafe fn EditAndContinueRemap(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pFunction: *mut c_void, fAccurate: i32) -> HRESULT {
        (self.vtbl().EditAndContinueRemap)(self.as_raw(), pAppDomain, pThread, pFunction, fAccurate)
    }
    #[inline]
    pub unsafe fn BreakpointSetError(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pBreakpoint: *mut c_void, dwError: u32) -> HRESULT {
        (self.vtbl().BreakpointSetError)(self.as_raw(), pAppDomain, pThread, pBreakpoint, dwError)
    }
}

// ---- ICorDebugManagedCallback3 : IUnknown
#[repr(C)]
pub struct ICorDebugManagedCallback3Vtbl {
    pub base: IUnknownVtbl,
    pub CustomNotification: unsafe extern "system" fn(this: *mut c_void, pThread: *mut c_void, pAppDomain: *mut c_void) -> HRESULT,
}
pub enum ICorDebugManagedCallback3 {}
unsafe impl Interface for ICorDebugManagedCallback3 {
    const IID: GUID = GUID { data1: 0x264EA0FC, data2: 0x2591, data3: 0x49AA, data4: [0x86, 0x8E, 0x83, 0x5E, 0x65, 0x15, 0x32, 0x3F] };
    type Vtbl = ICorDebugManagedCallback3Vtbl;
}
impl ComPtr<ICorDebugManagedCallback3> {
    #[inline]
    pub unsafe fn CustomNotification(&self, pThread: *mut c_void, pAppDomain: *mut c_void) -> HRESULT {
        (self.vtbl().CustomNotification)(self.as_raw(), pThread, pAppDomain)
    }
}

// ---- ICorDebugManagedCallback4 : IUnknown
#[repr(C)]
pub struct ICorDebugManagedCallback4Vtbl {
    pub base: IUnknownVtbl,
    pub BeforeGarbageCollection: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void) -> HRESULT,
    pub AfterGarbageCollection: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void) -> HRESULT,
    pub DataBreakpoint: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, pThread: *mut c_void, pContext: *mut u8, contextSize: u32) -> HRESULT,
}
pub enum ICorDebugManagedCallback4 {}
unsafe impl Interface for ICorDebugManagedCallback4 {
    const IID: GUID = GUID { data1: 0x322911AE, data2: 0x16A5, data3: 0x49BA, data4: [0x84, 0xA3, 0xED, 0x69, 0x67, 0x81, 0x38, 0xA3] };
    type Vtbl = ICorDebugManagedCallback4Vtbl;
}
impl ComPtr<ICorDebugManagedCallback4> {
    #[inline]
    pub unsafe fn BeforeGarbageCollection(&self, pProcess: *mut c_void) -> HRESULT {
        (self.vtbl().BeforeGarbageCollection)(self.as_raw(), pProcess)
    }
    #[inline]
    pub unsafe fn AfterGarbageCollection(&self, pProcess: *mut c_void) -> HRESULT {
        (self.vtbl().AfterGarbageCollection)(self.as_raw(), pProcess)
    }
    #[inline]
    pub unsafe fn DataBreakpoint(&self, pProcess: *mut c_void, pThread: *mut c_void, pContext: *mut u8, contextSize: u32) -> HRESULT {
        (self.vtbl().DataBreakpoint)(self.as_raw(), pProcess, pThread, pContext, contextSize)
    }
}

// ---- ICorDebugManagedCallback2 : IUnknown
#[repr(C)]
pub struct ICorDebugManagedCallback2Vtbl {
    pub base: IUnknownVtbl,
    pub FunctionRemapOpportunity: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pOldFunction: *mut c_void, pNewFunction: *mut c_void, oldILOffset: u32) -> HRESULT,
    pub CreateConnection: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, dwConnectionId: CONNID, pConnName: *mut u16) -> HRESULT,
    pub ChangeConnection: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, dwConnectionId: CONNID) -> HRESULT,
    pub DestroyConnection: unsafe extern "system" fn(this: *mut c_void, pProcess: *mut c_void, dwConnectionId: CONNID) -> HRESULT,
    pub Exception: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pFrame: *mut c_void, nOffset: u32, dwEventType: CorDebugExceptionCallbackType, dwFlags: u32) -> HRESULT,
    pub ExceptionUnwind: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, dwEventType: CorDebugExceptionUnwindCallbackType, dwFlags: u32) -> HRESULT,
    pub FunctionRemapComplete: unsafe extern "system" fn(this: *mut c_void, pAppDomain: *mut c_void, pThread: *mut c_void, pFunction: *mut c_void) -> HRESULT,
    pub MDANotification: unsafe extern "system" fn(this: *mut c_void, pController: *mut c_void, pThread: *mut c_void, pMDA: *mut c_void) -> HRESULT,
}
pub enum ICorDebugManagedCallback2 {}
unsafe impl Interface for ICorDebugManagedCallback2 {
    const IID: GUID = GUID { data1: 0x250E5EEA, data2: 0xDB5C, data3: 0x4C76, data4: [0xB6, 0xF3, 0x8C, 0x46, 0xF1, 0x2E, 0x32, 0x03] };
    type Vtbl = ICorDebugManagedCallback2Vtbl;
}
impl ComPtr<ICorDebugManagedCallback2> {
    #[inline]
    pub unsafe fn FunctionRemapOpportunity(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pOldFunction: *mut c_void, pNewFunction: *mut c_void, oldILOffset: u32) -> HRESULT {
        (self.vtbl().FunctionRemapOpportunity)(self.as_raw(), pAppDomain, pThread, pOldFunction, pNewFunction, oldILOffset)
    }
    #[inline]
    pub unsafe fn CreateConnection(&self, pProcess: *mut c_void, dwConnectionId: CONNID, pConnName: *mut u16) -> HRESULT {
        (self.vtbl().CreateConnection)(self.as_raw(), pProcess, dwConnectionId, pConnName)
    }
    #[inline]
    pub unsafe fn ChangeConnection(&self, pProcess: *mut c_void, dwConnectionId: CONNID) -> HRESULT {
        (self.vtbl().ChangeConnection)(self.as_raw(), pProcess, dwConnectionId)
    }
    #[inline]
    pub unsafe fn DestroyConnection(&self, pProcess: *mut c_void, dwConnectionId: CONNID) -> HRESULT {
        (self.vtbl().DestroyConnection)(self.as_raw(), pProcess, dwConnectionId)
    }
    #[inline]
    pub unsafe fn Exception(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pFrame: *mut c_void, nOffset: u32, dwEventType: CorDebugExceptionCallbackType, dwFlags: u32) -> HRESULT {
        (self.vtbl().Exception)(self.as_raw(), pAppDomain, pThread, pFrame, nOffset, dwEventType, dwFlags)
    }
    #[inline]
    pub unsafe fn ExceptionUnwind(&self, pAppDomain: *mut c_void, pThread: *mut c_void, dwEventType: CorDebugExceptionUnwindCallbackType, dwFlags: u32) -> HRESULT {
        (self.vtbl().ExceptionUnwind)(self.as_raw(), pAppDomain, pThread, dwEventType, dwFlags)
    }
    #[inline]
    pub unsafe fn FunctionRemapComplete(&self, pAppDomain: *mut c_void, pThread: *mut c_void, pFunction: *mut c_void) -> HRESULT {
        (self.vtbl().FunctionRemapComplete)(self.as_raw(), pAppDomain, pThread, pFunction)
    }
    #[inline]
    pub unsafe fn MDANotification(&self, pController: *mut c_void, pThread: *mut c_void, pMDA: *mut c_void) -> HRESULT {
        (self.vtbl().MDANotification)(self.as_raw(), pController, pThread, pMDA)
    }
}

// ---- ICorDebugUnmanagedCallback : IUnknown
#[repr(C)]
pub struct ICorDebugUnmanagedCallbackVtbl {
    pub base: IUnknownVtbl,
    pub DebugEvent: unsafe extern "system" fn(this: *mut c_void, pDebugEvent: LPDEBUG_EVENT, fOutOfBand: i32) -> HRESULT,
}
pub enum ICorDebugUnmanagedCallback {}
unsafe impl Interface for ICorDebugUnmanagedCallback {
    const IID: GUID = GUID { data1: 0x5263E909, data2: 0x8CB5, data3: 0x11d3, data4: [0xBD, 0x2F, 0x00, 0x00, 0xF8, 0x08, 0x49, 0xBD] };
    type Vtbl = ICorDebugUnmanagedCallbackVtbl;
}
impl ComPtr<ICorDebugUnmanagedCallback> {
    #[inline]
    pub unsafe fn DebugEvent(&self, pDebugEvent: LPDEBUG_EVENT, fOutOfBand: i32) -> HRESULT {
        (self.vtbl().DebugEvent)(self.as_raw(), pDebugEvent, fOutOfBand)
    }
}

// ---- ICorDebug : IUnknown
#[repr(C)]
pub struct ICorDebugVtbl {
    pub base: IUnknownVtbl,
    pub Initialize: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub Terminate: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub SetManagedHandler: unsafe extern "system" fn(this: *mut c_void, pCallback: *mut c_void) -> HRESULT,
    pub SetUnmanagedHandler: unsafe extern "system" fn(this: *mut c_void, pCallback: *mut c_void) -> HRESULT,
    pub CreateProcess: unsafe extern "system" fn(this: *mut c_void, lpApplicationName: *const u16, lpCommandLine: *mut u16, lpProcessAttributes: *mut c_void, lpThreadAttributes: *mut c_void, bInheritHandles: i32, dwCreationFlags: u32, lpEnvironment: *mut c_void, lpCurrentDirectory: *const u16, lpStartupInfo: LPSTARTUPINFOW, lpProcessInformation: LPPROCESS_INFORMATION, debuggingFlags: CorDebugCreateProcessFlags, ppProcess: *mut *mut c_void) -> HRESULT,
    pub DebugActiveProcess: unsafe extern "system" fn(this: *mut c_void, id: u32, win32Attach: i32, ppProcess: *mut *mut c_void) -> HRESULT,
    pub EnumerateProcesses: unsafe extern "system" fn(this: *mut c_void, ppProcess: *mut *mut c_void) -> HRESULT,
    pub GetProcess: unsafe extern "system" fn(this: *mut c_void, dwProcessId: u32, ppProcess: *mut *mut c_void) -> HRESULT,
    pub CanLaunchOrAttach: unsafe extern "system" fn(this: *mut c_void, dwProcessId: u32, win32DebuggingEnabled: i32) -> HRESULT,
}
pub enum ICorDebug {}
unsafe impl Interface for ICorDebug {
    const IID: GUID = GUID { data1: 0x3d6f5f61, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef] };
    type Vtbl = ICorDebugVtbl;
}
impl ComPtr<ICorDebug> {
    #[inline]
    pub unsafe fn Initialize(&self) -> HRESULT {
        (self.vtbl().Initialize)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Terminate(&self) -> HRESULT {
        (self.vtbl().Terminate)(self.as_raw())
    }
    #[inline]
    pub unsafe fn SetManagedHandler(&self, pCallback: *mut c_void) -> HRESULT {
        (self.vtbl().SetManagedHandler)(self.as_raw(), pCallback)
    }
    #[inline]
    pub unsafe fn SetUnmanagedHandler(&self, pCallback: *mut c_void) -> HRESULT {
        (self.vtbl().SetUnmanagedHandler)(self.as_raw(), pCallback)
    }
    #[inline]
    pub unsafe fn CreateProcess(&self, lpApplicationName: *const u16, lpCommandLine: *mut u16, lpProcessAttributes: *mut c_void, lpThreadAttributes: *mut c_void, bInheritHandles: i32, dwCreationFlags: u32, lpEnvironment: *mut c_void, lpCurrentDirectory: *const u16, lpStartupInfo: LPSTARTUPINFOW, lpProcessInformation: LPPROCESS_INFORMATION, debuggingFlags: CorDebugCreateProcessFlags, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateProcess)(self.as_raw(), lpApplicationName, lpCommandLine, lpProcessAttributes, lpThreadAttributes, bInheritHandles, dwCreationFlags, lpEnvironment, lpCurrentDirectory, lpStartupInfo, lpProcessInformation, debuggingFlags, ppProcess)
    }
    #[inline]
    pub unsafe fn DebugActiveProcess(&self, id: u32, win32Attach: i32, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().DebugActiveProcess)(self.as_raw(), id, win32Attach, ppProcess)
    }
    #[inline]
    pub unsafe fn EnumerateProcesses(&self, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateProcesses)(self.as_raw(), ppProcess)
    }
    #[inline]
    pub unsafe fn GetProcess(&self, dwProcessId: u32, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcess)(self.as_raw(), dwProcessId, ppProcess)
    }
    #[inline]
    pub unsafe fn CanLaunchOrAttach(&self, dwProcessId: u32, win32DebuggingEnabled: i32) -> HRESULT {
        (self.vtbl().CanLaunchOrAttach)(self.as_raw(), dwProcessId, win32DebuggingEnabled)
    }
}

// ---- ICorDebugRemoteTarget : IUnknown
#[repr(C)]
pub struct ICorDebugRemoteTargetVtbl {
    pub base: IUnknownVtbl,
    pub GetHostName: unsafe extern "system" fn(this: *mut c_void, cchHostName: u32, pcchHostName: *mut u32, szHostName: *mut u16) -> HRESULT,
}
pub enum ICorDebugRemoteTarget {}
unsafe impl Interface for ICorDebugRemoteTarget {
    const IID: GUID = GUID { data1: 0xC3ED8383, data2: 0x5A49, data3: 0x4cf5, data4: [0xB4, 0xB7, 0x01, 0x86, 0x4D, 0x9E, 0x58, 0x2D] };
    type Vtbl = ICorDebugRemoteTargetVtbl;
}
impl ComPtr<ICorDebugRemoteTarget> {
    #[inline]
    pub unsafe fn GetHostName(&self, cchHostName: u32, pcchHostName: *mut u32, szHostName: *mut u16) -> HRESULT {
        (self.vtbl().GetHostName)(self.as_raw(), cchHostName, pcchHostName, szHostName)
    }
}

// ---- ICorDebugRemote : IUnknown
#[repr(C)]
pub struct ICorDebugRemoteVtbl {
    pub base: IUnknownVtbl,
    pub CreateProcessEx: unsafe extern "system" fn(this: *mut c_void, pRemoteTarget: *mut c_void, lpApplicationName: *const u16, lpCommandLine: *mut u16, lpProcessAttributes: *mut c_void, lpThreadAttributes: *mut c_void, bInheritHandles: i32, dwCreationFlags: u32, lpEnvironment: *mut c_void, lpCurrentDirectory: *const u16, lpStartupInfo: LPSTARTUPINFOW, lpProcessInformation: LPPROCESS_INFORMATION, debuggingFlags: CorDebugCreateProcessFlags, ppProcess: *mut *mut c_void) -> HRESULT,
    pub DebugActiveProcessEx: unsafe extern "system" fn(this: *mut c_void, pRemoteTarget: *mut c_void, dwProcessId: u32, fWin32Attach: i32, ppProcess: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugRemote {}
unsafe impl Interface for ICorDebugRemote {
    const IID: GUID = GUID { data1: 0xD5EBB8E2, data2: 0x7BBE, data3: 0x4c1d, data4: [0x98, 0xA6, 0xA3, 0xC0, 0x4C, 0xBD, 0xEF, 0x64] };
    type Vtbl = ICorDebugRemoteVtbl;
}
impl ComPtr<ICorDebugRemote> {
    #[inline]
    pub unsafe fn CreateProcessEx(&self, pRemoteTarget: *mut c_void, lpApplicationName: *const u16, lpCommandLine: *mut u16, lpProcessAttributes: *mut c_void, lpThreadAttributes: *mut c_void, bInheritHandles: i32, dwCreationFlags: u32, lpEnvironment: *mut c_void, lpCurrentDirectory: *const u16, lpStartupInfo: LPSTARTUPINFOW, lpProcessInformation: LPPROCESS_INFORMATION, debuggingFlags: CorDebugCreateProcessFlags, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateProcessEx)(self.as_raw(), pRemoteTarget, lpApplicationName, lpCommandLine, lpProcessAttributes, lpThreadAttributes, bInheritHandles, dwCreationFlags, lpEnvironment, lpCurrentDirectory, lpStartupInfo, lpProcessInformation, debuggingFlags, ppProcess)
    }
    #[inline]
    pub unsafe fn DebugActiveProcessEx(&self, pRemoteTarget: *mut c_void, dwProcessId: u32, fWin32Attach: i32, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().DebugActiveProcessEx)(self.as_raw(), pRemoteTarget, dwProcessId, fWin32Attach, ppProcess)
    }
}

// ---- ICorDebug2 : IUnknown
#[repr(C)]
pub struct ICorDebug2Vtbl {
    pub base: IUnknownVtbl,
}
pub enum ICorDebug2 {}
unsafe impl Interface for ICorDebug2 {
    const IID: GUID = GUID { data1: 0xECCCCF2E, data2: 0xB286, data3: 0x4b3e, data4: [0xA9, 0x83, 0x86, 0x0A, 0x87, 0x93, 0xD1, 0x05] };
    type Vtbl = ICorDebug2Vtbl;
}
impl ComPtr<ICorDebug2> {
}

// ---- ICorDebugController : IUnknown
#[repr(C)]
pub struct ICorDebugControllerVtbl {
    pub base: IUnknownVtbl,
    pub Stop: unsafe extern "system" fn(this: *mut c_void, dwTimeoutIgnored: u32) -> HRESULT,
    pub Continue: unsafe extern "system" fn(this: *mut c_void, fIsOutOfBand: i32) -> HRESULT,
    pub IsRunning: unsafe extern "system" fn(this: *mut c_void, pbRunning: *mut i32) -> HRESULT,
    pub HasQueuedCallbacks: unsafe extern "system" fn(this: *mut c_void, pThread: *mut c_void, pbQueued: *mut i32) -> HRESULT,
    pub EnumerateThreads: unsafe extern "system" fn(this: *mut c_void, ppThreads: *mut *mut c_void) -> HRESULT,
    pub SetAllThreadsDebugState: unsafe extern "system" fn(this: *mut c_void, state: CorDebugThreadState, pExceptThisThread: *mut c_void) -> HRESULT,
    pub Detach: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub Terminate: unsafe extern "system" fn(this: *mut c_void, exitCode: u32) -> HRESULT,
    pub CanCommitChanges: unsafe extern "system" fn(this: *mut c_void, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT,
    pub CommitChanges: unsafe extern "system" fn(this: *mut c_void, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugController {}
unsafe impl Interface for ICorDebugController {
    const IID: GUID = GUID { data1: 0x3d6f5f62, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef] };
    type Vtbl = ICorDebugControllerVtbl;
}
impl ComPtr<ICorDebugController> {
    #[inline]
    pub unsafe fn Stop(&self, dwTimeoutIgnored: u32) -> HRESULT {
        (self.vtbl().Stop)(self.as_raw(), dwTimeoutIgnored)
    }
    #[inline]
    pub unsafe fn Continue(&self, fIsOutOfBand: i32) -> HRESULT {
        (self.vtbl().Continue)(self.as_raw(), fIsOutOfBand)
    }
    #[inline]
    pub unsafe fn IsRunning(&self, pbRunning: *mut i32) -> HRESULT {
        (self.vtbl().IsRunning)(self.as_raw(), pbRunning)
    }
    #[inline]
    pub unsafe fn HasQueuedCallbacks(&self, pThread: *mut c_void, pbQueued: *mut i32) -> HRESULT {
        (self.vtbl().HasQueuedCallbacks)(self.as_raw(), pThread, pbQueued)
    }
    #[inline]
    pub unsafe fn EnumerateThreads(&self, ppThreads: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateThreads)(self.as_raw(), ppThreads)
    }
    #[inline]
    pub unsafe fn SetAllThreadsDebugState(&self, state: CorDebugThreadState, pExceptThisThread: *mut c_void) -> HRESULT {
        (self.vtbl().SetAllThreadsDebugState)(self.as_raw(), state, pExceptThisThread)
    }
    #[inline]
    pub unsafe fn Detach(&self) -> HRESULT {
        (self.vtbl().Detach)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Terminate(&self, exitCode: u32) -> HRESULT {
        (self.vtbl().Terminate)(self.as_raw(), exitCode)
    }
    #[inline]
    pub unsafe fn CanCommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CanCommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
    #[inline]
    pub unsafe fn CommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
}

// ---- ICorDebugAppDomain : ICorDebugController
#[repr(C)]
pub struct ICorDebugAppDomainVtbl {
    pub base: ICorDebugControllerVtbl,
    pub GetProcess: unsafe extern "system" fn(this: *mut c_void, ppProcess: *mut *mut c_void) -> HRESULT,
    pub EnumerateAssemblies: unsafe extern "system" fn(this: *mut c_void, ppAssemblies: *mut *mut c_void) -> HRESULT,
    pub GetModuleFromMetaDataInterface: unsafe extern "system" fn(this: *mut c_void, pIMetaData: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT,
    pub EnumerateBreakpoints: unsafe extern "system" fn(this: *mut c_void, ppBreakpoints: *mut *mut c_void) -> HRESULT,
    pub EnumerateSteppers: unsafe extern "system" fn(this: *mut c_void, ppSteppers: *mut *mut c_void) -> HRESULT,
    pub IsAttached: unsafe extern "system" fn(this: *mut c_void, pbAttached: *mut i32) -> HRESULT,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetObject: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
    pub Attach: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub GetID: unsafe extern "system" fn(this: *mut c_void, pId: *mut u32) -> HRESULT,
}
pub enum ICorDebugAppDomain {}
unsafe impl Interface for ICorDebugAppDomain {
    const IID: GUID = GUID { data1: 0x3d6f5f63, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef] };
    type Vtbl = ICorDebugAppDomainVtbl;
}
impl ComPtr<ICorDebugAppDomain> {
    #[inline]
    pub unsafe fn GetProcess(&self, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcess)(self.as_raw(), ppProcess)
    }
    #[inline]
    pub unsafe fn EnumerateAssemblies(&self, ppAssemblies: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateAssemblies)(self.as_raw(), ppAssemblies)
    }
    #[inline]
    pub unsafe fn GetModuleFromMetaDataInterface(&self, pIMetaData: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModuleFromMetaDataInterface)(self.as_raw(), pIMetaData, ppModule)
    }
    #[inline]
    pub unsafe fn EnumerateBreakpoints(&self, ppBreakpoints: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateBreakpoints)(self.as_raw(), ppBreakpoints)
    }
    #[inline]
    pub unsafe fn EnumerateSteppers(&self, ppSteppers: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateSteppers)(self.as_raw(), ppSteppers)
    }
    #[inline]
    pub unsafe fn IsAttached(&self, pbAttached: *mut i32) -> HRESULT {
        (self.vtbl().IsAttached)(self.as_raw(), pbAttached)
    }
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetObject(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObject)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn Attach(&self) -> HRESULT {
        (self.vtbl().Attach)(self.as_raw())
    }
    #[inline]
    pub unsafe fn GetID(&self, pId: *mut u32) -> HRESULT {
        (self.vtbl().GetID)(self.as_raw(), pId)
    }
    #[inline]
    pub unsafe fn Stop(&self, dwTimeoutIgnored: u32) -> HRESULT {
        (self.vtbl().base.Stop)(self.as_raw(), dwTimeoutIgnored)
    }
    #[inline]
    pub unsafe fn Continue(&self, fIsOutOfBand: i32) -> HRESULT {
        (self.vtbl().base.Continue)(self.as_raw(), fIsOutOfBand)
    }
    #[inline]
    pub unsafe fn IsRunning(&self, pbRunning: *mut i32) -> HRESULT {
        (self.vtbl().base.IsRunning)(self.as_raw(), pbRunning)
    }
    #[inline]
    pub unsafe fn HasQueuedCallbacks(&self, pThread: *mut c_void, pbQueued: *mut i32) -> HRESULT {
        (self.vtbl().base.HasQueuedCallbacks)(self.as_raw(), pThread, pbQueued)
    }
    #[inline]
    pub unsafe fn EnumerateThreads(&self, ppThreads: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.EnumerateThreads)(self.as_raw(), ppThreads)
    }
    #[inline]
    pub unsafe fn SetAllThreadsDebugState(&self, state: CorDebugThreadState, pExceptThisThread: *mut c_void) -> HRESULT {
        (self.vtbl().base.SetAllThreadsDebugState)(self.as_raw(), state, pExceptThisThread)
    }
    #[inline]
    pub unsafe fn Detach(&self) -> HRESULT {
        (self.vtbl().base.Detach)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Terminate(&self, exitCode: u32) -> HRESULT {
        (self.vtbl().base.Terminate)(self.as_raw(), exitCode)
    }
    #[inline]
    pub unsafe fn CanCommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CanCommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
    #[inline]
    pub unsafe fn CommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
}

// ---- ICorDebugAppDomain2 : IUnknown
#[repr(C)]
pub struct ICorDebugAppDomain2Vtbl {
    pub base: IUnknownVtbl,
    pub GetArrayOrPointerType: unsafe extern "system" fn(this: *mut c_void, elementType: CorElementType, nRank: u32, pTypeArg: *mut c_void, ppType: *mut *mut c_void) -> HRESULT,
    pub GetFunctionPointerType: unsafe extern "system" fn(this: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugAppDomain2 {}
unsafe impl Interface for ICorDebugAppDomain2 {
    const IID: GUID = GUID { data1: 0x096E81D5, data2: 0xECDA, data3: 0x4202, data4: [0x83, 0xF5, 0xC6, 0x59, 0x80, 0xA9, 0xEF, 0x75] };
    type Vtbl = ICorDebugAppDomain2Vtbl;
}
impl ComPtr<ICorDebugAppDomain2> {
    #[inline]
    pub unsafe fn GetArrayOrPointerType(&self, elementType: CorElementType, nRank: u32, pTypeArg: *mut c_void, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetArrayOrPointerType)(self.as_raw(), elementType, nRank, pTypeArg, ppType)
    }
    #[inline]
    pub unsafe fn GetFunctionPointerType(&self, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunctionPointerType)(self.as_raw(), nTypeArgs, ppTypeArgs, ppType)
    }
}

// ---- ICorDebugGuidToTypeEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugGuidToTypeEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut CorDebugGuidToTypeMapping, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugGuidToTypeEnum {}
unsafe impl Interface for ICorDebugGuidToTypeEnum {
    const IID: GUID = GUID { data1: 0x6164D242, data2: 0x1015, data3: 0x4BD6, data4: [0x8C, 0xBE, 0xD0, 0xDB, 0xD4, 0xB8, 0x27, 0x5A] };
    type Vtbl = ICorDebugGuidToTypeEnumVtbl;
}
impl ComPtr<ICorDebugGuidToTypeEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut CorDebugGuidToTypeMapping, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugAppDomain3 : IUnknown
#[repr(C)]
pub struct ICorDebugAppDomain3Vtbl {
    pub base: IUnknownVtbl,
    pub GetCachedWinRTTypesForIIDs: unsafe extern "system" fn(this: *mut c_void, cReqTypes: u32, iidsToResolve: *mut GUID, ppTypesEnum: *mut *mut c_void) -> HRESULT,
    pub GetCachedWinRTTypes: unsafe extern "system" fn(this: *mut c_void, ppGuidToTypeEnum: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugAppDomain3 {}
unsafe impl Interface for ICorDebugAppDomain3 {
    const IID: GUID = GUID { data1: 0x8CB96A16, data2: 0xB588, data3: 0x42E2, data4: [0xB7, 0x1C, 0xDD, 0x84, 0x9F, 0xC2, 0xEC, 0xCC] };
    type Vtbl = ICorDebugAppDomain3Vtbl;
}
impl ComPtr<ICorDebugAppDomain3> {
    #[inline]
    pub unsafe fn GetCachedWinRTTypesForIIDs(&self, cReqTypes: u32, iidsToResolve: *mut GUID, ppTypesEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedWinRTTypesForIIDs)(self.as_raw(), cReqTypes, iidsToResolve, ppTypesEnum)
    }
    #[inline]
    pub unsafe fn GetCachedWinRTTypes(&self, ppGuidToTypeEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedWinRTTypes)(self.as_raw(), ppGuidToTypeEnum)
    }
}

// ---- ICorDebugAppDomain4 : IUnknown
#[repr(C)]
pub struct ICorDebugAppDomain4Vtbl {
    pub base: IUnknownVtbl,
    pub GetObjectForCCW: unsafe extern "system" fn(this: *mut c_void, ccwPointer: CORDB_ADDRESS, ppManagedObject: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugAppDomain4 {}
unsafe impl Interface for ICorDebugAppDomain4 {
    const IID: GUID = GUID { data1: 0xFB99CC40, data2: 0x83BE, data3: 0x4724, data4: [0xAB, 0x3B, 0x76, 0x8E, 0x79, 0x6E, 0xBA, 0xC2] };
    type Vtbl = ICorDebugAppDomain4Vtbl;
}
impl ComPtr<ICorDebugAppDomain4> {
    #[inline]
    pub unsafe fn GetObjectForCCW(&self, ccwPointer: CORDB_ADDRESS, ppManagedObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObjectForCCW)(self.as_raw(), ccwPointer, ppManagedObject)
    }
}

// ---- ICorDebugAssembly : IUnknown
#[repr(C)]
pub struct ICorDebugAssemblyVtbl {
    pub base: IUnknownVtbl,
    pub GetProcess: unsafe extern "system" fn(this: *mut c_void, ppProcess: *mut *mut c_void) -> HRESULT,
    pub GetAppDomain: unsafe extern "system" fn(this: *mut c_void, ppAppDomain: *mut *mut c_void) -> HRESULT,
    pub EnumerateModules: unsafe extern "system" fn(this: *mut c_void, ppModules: *mut *mut c_void) -> HRESULT,
    pub GetCodeBase: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
}
pub enum ICorDebugAssembly {}
unsafe impl Interface for ICorDebugAssembly {
    const IID: GUID = GUID { data1: 0xdf59507c, data2: 0xd47a, data3: 0x459e, data4: [0xbc, 0xe2, 0x64, 0x27, 0xea, 0xc8, 0xfd, 0x06] };
    type Vtbl = ICorDebugAssemblyVtbl;
}
impl ComPtr<ICorDebugAssembly> {
    #[inline]
    pub unsafe fn GetProcess(&self, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcess)(self.as_raw(), ppProcess)
    }
    #[inline]
    pub unsafe fn GetAppDomain(&self, ppAppDomain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAppDomain)(self.as_raw(), ppAppDomain)
    }
    #[inline]
    pub unsafe fn EnumerateModules(&self, ppModules: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateModules)(self.as_raw(), ppModules)
    }
    #[inline]
    pub unsafe fn GetCodeBase(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetCodeBase)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
}

// ---- ICorDebugAssembly2 : IUnknown
#[repr(C)]
pub struct ICorDebugAssembly2Vtbl {
    pub base: IUnknownVtbl,
    pub IsFullyTrusted: unsafe extern "system" fn(this: *mut c_void, pbFullyTrusted: *mut i32) -> HRESULT,
}
pub enum ICorDebugAssembly2 {}
unsafe impl Interface for ICorDebugAssembly2 {
    const IID: GUID = GUID { data1: 0x426d1f9e, data2: 0x6dd4, data3: 0x44c8, data4: [0xae, 0xc7, 0x26, 0xcd, 0xba, 0xf4, 0xe3, 0x98] };
    type Vtbl = ICorDebugAssembly2Vtbl;
}
impl ComPtr<ICorDebugAssembly2> {
    #[inline]
    pub unsafe fn IsFullyTrusted(&self, pbFullyTrusted: *mut i32) -> HRESULT {
        (self.vtbl().IsFullyTrusted)(self.as_raw(), pbFullyTrusted)
    }
}

// ---- ICorDebugAssembly3 : IUnknown
#[repr(C)]
pub struct ICorDebugAssembly3Vtbl {
    pub base: IUnknownVtbl,
    pub GetContainerAssembly: unsafe extern "system" fn(this: *mut c_void, ppAssembly: *mut *mut c_void) -> HRESULT,
    pub EnumerateContainedAssemblies: unsafe extern "system" fn(this: *mut c_void, ppAssemblies: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugAssembly3 {}
unsafe impl Interface for ICorDebugAssembly3 {
    const IID: GUID = GUID { data1: 0x76361AB2, data2: 0x8C86, data3: 0x4FE9, data4: [0x96, 0xF2, 0xF7, 0x3D, 0x88, 0x43, 0x57, 0x0A] };
    type Vtbl = ICorDebugAssembly3Vtbl;
}
impl ComPtr<ICorDebugAssembly3> {
    #[inline]
    pub unsafe fn GetContainerAssembly(&self, ppAssembly: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetContainerAssembly)(self.as_raw(), ppAssembly)
    }
    #[inline]
    pub unsafe fn EnumerateContainedAssemblies(&self, ppAssemblies: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateContainedAssemblies)(self.as_raw(), ppAssemblies)
    }
}

// ---- ICorDebugHeapEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugHeapEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, objects: *mut COR_HEAPOBJECT, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugHeapEnum {}
unsafe impl Interface for ICorDebugHeapEnum {
    const IID: GUID = GUID { data1: 0x76D7DAB8, data2: 0xD044, data3: 0x11DF, data4: [0x9A, 0x15, 0x7E, 0x29, 0xDF, 0xD7, 0x20, 0x85] };
    type Vtbl = ICorDebugHeapEnumVtbl;
}
impl ComPtr<ICorDebugHeapEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, objects: *mut COR_HEAPOBJECT, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, objects, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugHeapSegmentEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugHeapSegmentEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, segments: *mut COR_SEGMENT, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugHeapSegmentEnum {}
unsafe impl Interface for ICorDebugHeapSegmentEnum {
    const IID: GUID = GUID { data1: 0xA2FA0F8E, data2: 0xD045, data3: 0x11DF, data4: [0xAC, 0x8E, 0xCE, 0x2A, 0xDF, 0xD7, 0x20, 0x85] };
    type Vtbl = ICorDebugHeapSegmentEnumVtbl;
}
impl ComPtr<ICorDebugHeapSegmentEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, segments: *mut COR_SEGMENT, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, segments, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugGCReferenceEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugGCReferenceEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, roots: *mut COR_GC_REFERENCE, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugGCReferenceEnum {}
unsafe impl Interface for ICorDebugGCReferenceEnum {
    const IID: GUID = GUID { data1: 0x7F3C24D3, data2: 0x7E1D, data3: 0x4245, data4: [0xAC, 0x3A, 0xF7, 0x2F, 0x88, 0x59, 0xC8, 0x0C] };
    type Vtbl = ICorDebugGCReferenceEnumVtbl;
}
impl ComPtr<ICorDebugGCReferenceEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, roots: *mut COR_GC_REFERENCE, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, roots, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugProcess : ICorDebugController
#[repr(C)]
pub struct ICorDebugProcessVtbl {
    pub base: ICorDebugControllerVtbl,
    pub GetID: unsafe extern "system" fn(this: *mut c_void, pdwProcessId: *mut u32) -> HRESULT,
    pub GetHandle: unsafe extern "system" fn(this: *mut c_void, phProcessHandle: *mut HPROCESS) -> HRESULT,
    pub GetThread: unsafe extern "system" fn(this: *mut c_void, dwThreadId: u32, ppThread: *mut *mut c_void) -> HRESULT,
    pub EnumerateObjects: unsafe extern "system" fn(this: *mut c_void, ppObjects: *mut *mut c_void) -> HRESULT,
    pub IsTransitionStub: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, pbTransitionStub: *mut i32) -> HRESULT,
    pub IsOSSuspended: unsafe extern "system" fn(this: *mut c_void, threadID: u32, pbSuspended: *mut i32) -> HRESULT,
    pub GetThreadContext: unsafe extern "system" fn(this: *mut c_void, threadID: u32, contextSize: u32, context: *mut u8) -> HRESULT,
    pub SetThreadContext: unsafe extern "system" fn(this: *mut c_void, threadID: u32, contextSize: u32, context: *mut u8) -> HRESULT,
    pub ReadMemory: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, size: u32, buffer: *mut u8, read: *mut usize) -> HRESULT,
    pub WriteMemory: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, size: u32, buffer: *mut u8, written: *mut usize) -> HRESULT,
    pub ClearCurrentException: unsafe extern "system" fn(this: *mut c_void, threadID: u32) -> HRESULT,
    pub EnableLogMessages: unsafe extern "system" fn(this: *mut c_void, fOnOff: i32) -> HRESULT,
    pub ModifyLogSwitch: unsafe extern "system" fn(this: *mut c_void, pLogSwitchName: *mut u16, lLevel: i32) -> HRESULT,
    pub EnumerateAppDomains: unsafe extern "system" fn(this: *mut c_void, ppAppDomains: *mut *mut c_void) -> HRESULT,
    pub GetObject: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
    pub ThreadForFiberCookie: unsafe extern "system" fn(this: *mut c_void, fiberCookie: u32, ppThread: *mut *mut c_void) -> HRESULT,
    pub GetHelperThreadID: unsafe extern "system" fn(this: *mut c_void, pThreadID: *mut u32) -> HRESULT,
}
pub enum ICorDebugProcess {}
unsafe impl Interface for ICorDebugProcess {
    const IID: GUID = GUID { data1: 0x3d6f5f64, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef] };
    type Vtbl = ICorDebugProcessVtbl;
}
impl ComPtr<ICorDebugProcess> {
    #[inline]
    pub unsafe fn GetID(&self, pdwProcessId: *mut u32) -> HRESULT {
        (self.vtbl().GetID)(self.as_raw(), pdwProcessId)
    }
    #[inline]
    pub unsafe fn GetHandle(&self, phProcessHandle: *mut HPROCESS) -> HRESULT {
        (self.vtbl().GetHandle)(self.as_raw(), phProcessHandle)
    }
    #[inline]
    pub unsafe fn GetThread(&self, dwThreadId: u32, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetThread)(self.as_raw(), dwThreadId, ppThread)
    }
    #[inline]
    pub unsafe fn EnumerateObjects(&self, ppObjects: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateObjects)(self.as_raw(), ppObjects)
    }
    #[inline]
    pub unsafe fn IsTransitionStub(&self, address: CORDB_ADDRESS, pbTransitionStub: *mut i32) -> HRESULT {
        (self.vtbl().IsTransitionStub)(self.as_raw(), address, pbTransitionStub)
    }
    #[inline]
    pub unsafe fn IsOSSuspended(&self, threadID: u32, pbSuspended: *mut i32) -> HRESULT {
        (self.vtbl().IsOSSuspended)(self.as_raw(), threadID, pbSuspended)
    }
    #[inline]
    pub unsafe fn GetThreadContext(&self, threadID: u32, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().GetThreadContext)(self.as_raw(), threadID, contextSize, context)
    }
    #[inline]
    pub unsafe fn SetThreadContext(&self, threadID: u32, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().SetThreadContext)(self.as_raw(), threadID, contextSize, context)
    }
    #[inline]
    pub unsafe fn ReadMemory(&self, address: CORDB_ADDRESS, size: u32, buffer: *mut u8, read: *mut usize) -> HRESULT {
        (self.vtbl().ReadMemory)(self.as_raw(), address, size, buffer, read)
    }
    #[inline]
    pub unsafe fn WriteMemory(&self, address: CORDB_ADDRESS, size: u32, buffer: *mut u8, written: *mut usize) -> HRESULT {
        (self.vtbl().WriteMemory)(self.as_raw(), address, size, buffer, written)
    }
    #[inline]
    pub unsafe fn ClearCurrentException(&self, threadID: u32) -> HRESULT {
        (self.vtbl().ClearCurrentException)(self.as_raw(), threadID)
    }
    #[inline]
    pub unsafe fn EnableLogMessages(&self, fOnOff: i32) -> HRESULT {
        (self.vtbl().EnableLogMessages)(self.as_raw(), fOnOff)
    }
    #[inline]
    pub unsafe fn ModifyLogSwitch(&self, pLogSwitchName: *mut u16, lLevel: i32) -> HRESULT {
        (self.vtbl().ModifyLogSwitch)(self.as_raw(), pLogSwitchName, lLevel)
    }
    #[inline]
    pub unsafe fn EnumerateAppDomains(&self, ppAppDomains: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateAppDomains)(self.as_raw(), ppAppDomains)
    }
    #[inline]
    pub unsafe fn GetObject(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObject)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn ThreadForFiberCookie(&self, fiberCookie: u32, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ThreadForFiberCookie)(self.as_raw(), fiberCookie, ppThread)
    }
    #[inline]
    pub unsafe fn GetHelperThreadID(&self, pThreadID: *mut u32) -> HRESULT {
        (self.vtbl().GetHelperThreadID)(self.as_raw(), pThreadID)
    }
    #[inline]
    pub unsafe fn Stop(&self, dwTimeoutIgnored: u32) -> HRESULT {
        (self.vtbl().base.Stop)(self.as_raw(), dwTimeoutIgnored)
    }
    #[inline]
    pub unsafe fn Continue(&self, fIsOutOfBand: i32) -> HRESULT {
        (self.vtbl().base.Continue)(self.as_raw(), fIsOutOfBand)
    }
    #[inline]
    pub unsafe fn IsRunning(&self, pbRunning: *mut i32) -> HRESULT {
        (self.vtbl().base.IsRunning)(self.as_raw(), pbRunning)
    }
    #[inline]
    pub unsafe fn HasQueuedCallbacks(&self, pThread: *mut c_void, pbQueued: *mut i32) -> HRESULT {
        (self.vtbl().base.HasQueuedCallbacks)(self.as_raw(), pThread, pbQueued)
    }
    #[inline]
    pub unsafe fn EnumerateThreads(&self, ppThreads: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.EnumerateThreads)(self.as_raw(), ppThreads)
    }
    #[inline]
    pub unsafe fn SetAllThreadsDebugState(&self, state: CorDebugThreadState, pExceptThisThread: *mut c_void) -> HRESULT {
        (self.vtbl().base.SetAllThreadsDebugState)(self.as_raw(), state, pExceptThisThread)
    }
    #[inline]
    pub unsafe fn Detach(&self) -> HRESULT {
        (self.vtbl().base.Detach)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Terminate(&self, exitCode: u32) -> HRESULT {
        (self.vtbl().base.Terminate)(self.as_raw(), exitCode)
    }
    #[inline]
    pub unsafe fn CanCommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CanCommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
    #[inline]
    pub unsafe fn CommitChanges(&self, cSnapshots: u32, pSnapshots: *mut *mut c_void, pError: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CommitChanges)(self.as_raw(), cSnapshots, pSnapshots, pError)
    }
}

// ---- ICorDebugProcess2 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess2Vtbl {
    pub base: IUnknownVtbl,
    pub GetThreadForTaskID: unsafe extern "system" fn(this: *mut c_void, taskid: TASKID, ppThread: *mut *mut c_void) -> HRESULT,
    pub GetVersion: unsafe extern "system" fn(this: *mut c_void, version: *mut COR_VERSION) -> HRESULT,
    pub SetUnmanagedBreakpoint: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, bufsize: u32, buffer: *mut u8, bufLen: *mut u32) -> HRESULT,
    pub ClearUnmanagedBreakpoint: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS) -> HRESULT,
    pub SetDesiredNGENCompilerFlags: unsafe extern "system" fn(this: *mut c_void, pdwFlags: u32) -> HRESULT,
    pub GetDesiredNGENCompilerFlags: unsafe extern "system" fn(this: *mut c_void, pdwFlags: *mut u32) -> HRESULT,
    pub GetReferenceValueFromGCHandle: unsafe extern "system" fn(this: *mut c_void, handle: usize, pOutValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugProcess2 {}
unsafe impl Interface for ICorDebugProcess2 {
    const IID: GUID = GUID { data1: 0xAD1B3588, data2: 0x0EF0, data3: 0x4744, data4: [0xA4, 0x96, 0xAA, 0x09, 0xA9, 0xF8, 0x03, 0x71] };
    type Vtbl = ICorDebugProcess2Vtbl;
}
impl ComPtr<ICorDebugProcess2> {
    #[inline]
    pub unsafe fn GetThreadForTaskID(&self, taskid: TASKID, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetThreadForTaskID)(self.as_raw(), taskid, ppThread)
    }
    #[inline]
    pub unsafe fn GetVersion(&self, version: *mut COR_VERSION) -> HRESULT {
        (self.vtbl().GetVersion)(self.as_raw(), version)
    }
    #[inline]
    pub unsafe fn SetUnmanagedBreakpoint(&self, address: CORDB_ADDRESS, bufsize: u32, buffer: *mut u8, bufLen: *mut u32) -> HRESULT {
        (self.vtbl().SetUnmanagedBreakpoint)(self.as_raw(), address, bufsize, buffer, bufLen)
    }
    #[inline]
    pub unsafe fn ClearUnmanagedBreakpoint(&self, address: CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().ClearUnmanagedBreakpoint)(self.as_raw(), address)
    }
    #[inline]
    pub unsafe fn SetDesiredNGENCompilerFlags(&self, pdwFlags: u32) -> HRESULT {
        (self.vtbl().SetDesiredNGENCompilerFlags)(self.as_raw(), pdwFlags)
    }
    #[inline]
    pub unsafe fn GetDesiredNGENCompilerFlags(&self, pdwFlags: *mut u32) -> HRESULT {
        (self.vtbl().GetDesiredNGENCompilerFlags)(self.as_raw(), pdwFlags)
    }
    #[inline]
    pub unsafe fn GetReferenceValueFromGCHandle(&self, handle: usize, pOutValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetReferenceValueFromGCHandle)(self.as_raw(), handle, pOutValue)
    }
}

// ---- ICorDebugProcess3 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess3Vtbl {
    pub base: IUnknownVtbl,
    pub SetEnableCustomNotification: unsafe extern "system" fn(this: *mut c_void, pClass: *mut c_void, fEnable: i32) -> HRESULT,
}
pub enum ICorDebugProcess3 {}
unsafe impl Interface for ICorDebugProcess3 {
    const IID: GUID = GUID { data1: 0x2EE06488, data2: 0xC0D4, data3: 0x42B1, data4: [0xB2, 0x6D, 0xF3, 0x79, 0x5E, 0xF6, 0x06, 0xFB] };
    type Vtbl = ICorDebugProcess3Vtbl;
}
impl ComPtr<ICorDebugProcess3> {
    #[inline]
    pub unsafe fn SetEnableCustomNotification(&self, pClass: *mut c_void, fEnable: i32) -> HRESULT {
        (self.vtbl().SetEnableCustomNotification)(self.as_raw(), pClass, fEnable)
    }
}

// ---- ICorDebugProcess5 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess5Vtbl {
    pub base: IUnknownVtbl,
    pub GetGCHeapInformation: unsafe extern "system" fn(this: *mut c_void, pHeapInfo: *mut COR_HEAPINFO) -> HRESULT,
    pub EnumerateHeap: unsafe extern "system" fn(this: *mut c_void, ppObjects: *mut *mut c_void) -> HRESULT,
    pub EnumerateHeapRegions: unsafe extern "system" fn(this: *mut c_void, ppRegions: *mut *mut c_void) -> HRESULT,
    pub GetObject: unsafe extern "system" fn(this: *mut c_void, addr: CORDB_ADDRESS, pObject: *mut *mut c_void) -> HRESULT,
    pub EnumerateGCReferences: unsafe extern "system" fn(this: *mut c_void, enumerateWeakReferences: i32, ppEnum: *mut *mut c_void) -> HRESULT,
    pub EnumerateHandles: unsafe extern "system" fn(this: *mut c_void, types: CorGCReferenceType, ppEnum: *mut *mut c_void) -> HRESULT,
    pub GetTypeID: unsafe extern "system" fn(this: *mut c_void, obj: CORDB_ADDRESS, pId: *mut COR_TYPEID) -> HRESULT,
    pub GetTypeForTypeID: unsafe extern "system" fn(this: *mut c_void, id: COR_TYPEID, ppType: *mut *mut c_void) -> HRESULT,
    pub GetArrayLayout: unsafe extern "system" fn(this: *mut c_void, id: COR_TYPEID, pLayout: *mut COR_ARRAY_LAYOUT) -> HRESULT,
    pub GetTypeLayout: unsafe extern "system" fn(this: *mut c_void, id: COR_TYPEID, pLayout: *mut COR_TYPE_LAYOUT) -> HRESULT,
    pub GetTypeFields: unsafe extern "system" fn(this: *mut c_void, id: COR_TYPEID, celt: u32, fields: *mut COR_FIELD, pceltNeeded: *mut u32) -> HRESULT,
    pub EnableNGENPolicy: unsafe extern "system" fn(this: *mut c_void, ePolicy: CorDebugNGENPolicy) -> HRESULT,
}
pub enum ICorDebugProcess5 {}
unsafe impl Interface for ICorDebugProcess5 {
    const IID: GUID = GUID { data1: 0x21e9d9c0, data2: 0xfcb8, data3: 0x11df, data4: [0x8c, 0xff, 0x08, 0x00, 0x20, 0x0c, 0x9a, 0x66] };
    type Vtbl = ICorDebugProcess5Vtbl;
}
impl ComPtr<ICorDebugProcess5> {
    #[inline]
    pub unsafe fn GetGCHeapInformation(&self, pHeapInfo: *mut COR_HEAPINFO) -> HRESULT {
        (self.vtbl().GetGCHeapInformation)(self.as_raw(), pHeapInfo)
    }
    #[inline]
    pub unsafe fn EnumerateHeap(&self, ppObjects: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateHeap)(self.as_raw(), ppObjects)
    }
    #[inline]
    pub unsafe fn EnumerateHeapRegions(&self, ppRegions: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateHeapRegions)(self.as_raw(), ppRegions)
    }
    #[inline]
    pub unsafe fn GetObject(&self, addr: CORDB_ADDRESS, pObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObject)(self.as_raw(), addr, pObject)
    }
    #[inline]
    pub unsafe fn EnumerateGCReferences(&self, enumerateWeakReferences: i32, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateGCReferences)(self.as_raw(), enumerateWeakReferences, ppEnum)
    }
    #[inline]
    pub unsafe fn EnumerateHandles(&self, types: CorGCReferenceType, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateHandles)(self.as_raw(), types, ppEnum)
    }
    #[inline]
    pub unsafe fn GetTypeID(&self, obj: CORDB_ADDRESS, pId: *mut COR_TYPEID) -> HRESULT {
        (self.vtbl().GetTypeID)(self.as_raw(), obj, pId)
    }
    #[inline]
    pub unsafe fn GetTypeForTypeID(&self, id: COR_TYPEID, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetTypeForTypeID)(self.as_raw(), id, ppType)
    }
    #[inline]
    pub unsafe fn GetArrayLayout(&self, id: COR_TYPEID, pLayout: *mut COR_ARRAY_LAYOUT) -> HRESULT {
        (self.vtbl().GetArrayLayout)(self.as_raw(), id, pLayout)
    }
    #[inline]
    pub unsafe fn GetTypeLayout(&self, id: COR_TYPEID, pLayout: *mut COR_TYPE_LAYOUT) -> HRESULT {
        (self.vtbl().GetTypeLayout)(self.as_raw(), id, pLayout)
    }
    #[inline]
    pub unsafe fn GetTypeFields(&self, id: COR_TYPEID, celt: u32, fields: *mut COR_FIELD, pceltNeeded: *mut u32) -> HRESULT {
        (self.vtbl().GetTypeFields)(self.as_raw(), id, celt, fields, pceltNeeded)
    }
    #[inline]
    pub unsafe fn EnableNGENPolicy(&self, ePolicy: CorDebugNGENPolicy) -> HRESULT {
        (self.vtbl().EnableNGENPolicy)(self.as_raw(), ePolicy)
    }
}

// ---- ICorDebugDebugEvent : IUnknown
#[repr(C)]
pub struct ICorDebugDebugEventVtbl {
    pub base: IUnknownVtbl,
    pub GetEventKind: unsafe extern "system" fn(this: *mut c_void, pDebugEventKind: *mut CorDebugDebugEventKind) -> HRESULT,
    pub GetThread: unsafe extern "system" fn(this: *mut c_void, ppThread: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugDebugEvent {}
unsafe impl Interface for ICorDebugDebugEvent {
    const IID: GUID = GUID { data1: 0x41BD395D, data2: 0xDE99, data3: 0x48F1, data4: [0xBF, 0x7A, 0xCC, 0x0F, 0x44, 0xA6, 0xD2, 0x81] };
    type Vtbl = ICorDebugDebugEventVtbl;
}
impl ComPtr<ICorDebugDebugEvent> {
    #[inline]
    pub unsafe fn GetEventKind(&self, pDebugEventKind: *mut CorDebugDebugEventKind) -> HRESULT {
        (self.vtbl().GetEventKind)(self.as_raw(), pDebugEventKind)
    }
    #[inline]
    pub unsafe fn GetThread(&self, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetThread)(self.as_raw(), ppThread)
    }
}

// ---- ICorDebugProcess6 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess6Vtbl {
    pub base: IUnknownVtbl,
    pub DecodeEvent: unsafe extern "system" fn(this: *mut c_void, pRecord: *mut u8, countBytes: u32, format: CorDebugRecordFormat, dwFlags: u32, dwThreadId: u32, ppEvent: *mut *mut c_void) -> HRESULT,
    pub ProcessStateChanged: unsafe extern "system" fn(this: *mut c_void, change: CorDebugStateChange) -> HRESULT,
    pub GetCode: unsafe extern "system" fn(this: *mut c_void, codeAddress: CORDB_ADDRESS, ppCode: *mut *mut c_void) -> HRESULT,
    pub EnableVirtualModuleSplitting: unsafe extern "system" fn(this: *mut c_void, enableSplitting: i32) -> HRESULT,
    pub MarkDebuggerAttached: unsafe extern "system" fn(this: *mut c_void, fIsAttached: i32) -> HRESULT,
    pub GetExportStepInfo: unsafe extern "system" fn(this: *mut c_void, pszExportName: *const u16, pInvokeKind: *mut CorDebugCodeInvokeKind, pInvokePurpose: *mut CorDebugCodeInvokePurpose) -> HRESULT,
}
pub enum ICorDebugProcess6 {}
unsafe impl Interface for ICorDebugProcess6 {
    const IID: GUID = GUID { data1: 0x11588775, data2: 0x7205, data3: 0x4CEB, data4: [0xA4, 0x1A, 0x93, 0x75, 0x3C, 0x31, 0x53, 0xE9] };
    type Vtbl = ICorDebugProcess6Vtbl;
}
impl ComPtr<ICorDebugProcess6> {
    #[inline]
    pub unsafe fn DecodeEvent(&self, pRecord: *mut u8, countBytes: u32, format: CorDebugRecordFormat, dwFlags: u32, dwThreadId: u32, ppEvent: *mut *mut c_void) -> HRESULT {
        (self.vtbl().DecodeEvent)(self.as_raw(), pRecord, countBytes, format, dwFlags, dwThreadId, ppEvent)
    }
    #[inline]
    pub unsafe fn ProcessStateChanged(&self, change: CorDebugStateChange) -> HRESULT {
        (self.vtbl().ProcessStateChanged)(self.as_raw(), change)
    }
    #[inline]
    pub unsafe fn GetCode(&self, codeAddress: CORDB_ADDRESS, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCode)(self.as_raw(), codeAddress, ppCode)
    }
    #[inline]
    pub unsafe fn EnableVirtualModuleSplitting(&self, enableSplitting: i32) -> HRESULT {
        (self.vtbl().EnableVirtualModuleSplitting)(self.as_raw(), enableSplitting)
    }
    #[inline]
    pub unsafe fn MarkDebuggerAttached(&self, fIsAttached: i32) -> HRESULT {
        (self.vtbl().MarkDebuggerAttached)(self.as_raw(), fIsAttached)
    }
    #[inline]
    pub unsafe fn GetExportStepInfo(&self, pszExportName: *const u16, pInvokeKind: *mut CorDebugCodeInvokeKind, pInvokePurpose: *mut CorDebugCodeInvokePurpose) -> HRESULT {
        (self.vtbl().GetExportStepInfo)(self.as_raw(), pszExportName, pInvokeKind, pInvokePurpose)
    }
}

// ---- ICorDebugProcess7 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess7Vtbl {
    pub base: IUnknownVtbl,
    pub SetWriteableMetadataUpdateMode: unsafe extern "system" fn(this: *mut c_void, flags: WriteableMetadataUpdateMode) -> HRESULT,
}
pub enum ICorDebugProcess7 {}
unsafe impl Interface for ICorDebugProcess7 {
    const IID: GUID = GUID { data1: 0x9B2C54E4, data2: 0x119F, data3: 0x4D6F, data4: [0xB4, 0x02, 0x52, 0x76, 0x03, 0x26, 0x6D, 0x69] };
    type Vtbl = ICorDebugProcess7Vtbl;
}
impl ComPtr<ICorDebugProcess7> {
    #[inline]
    pub unsafe fn SetWriteableMetadataUpdateMode(&self, flags: WriteableMetadataUpdateMode) -> HRESULT {
        (self.vtbl().SetWriteableMetadataUpdateMode)(self.as_raw(), flags)
    }
}

// ---- ICorDebugProcess8 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess8Vtbl {
    pub base: IUnknownVtbl,
    pub EnableExceptionCallbacksOutsideOfMyCode: unsafe extern "system" fn(this: *mut c_void, enableExceptionsOutsideOfJMC: i32) -> HRESULT,
}
pub enum ICorDebugProcess8 {}
unsafe impl Interface for ICorDebugProcess8 {
    const IID: GUID = GUID { data1: 0x2E6F28C1, data2: 0x85EB, data3: 0x4141, data4: [0x80, 0xAD, 0x0A, 0x90, 0x94, 0x4B, 0x96, 0x39] };
    type Vtbl = ICorDebugProcess8Vtbl;
}
impl ComPtr<ICorDebugProcess8> {
    #[inline]
    pub unsafe fn EnableExceptionCallbacksOutsideOfMyCode(&self, enableExceptionsOutsideOfJMC: i32) -> HRESULT {
        (self.vtbl().EnableExceptionCallbacksOutsideOfMyCode)(self.as_raw(), enableExceptionsOutsideOfJMC)
    }
}

// ---- ICorDebugProcess10 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess10Vtbl {
    pub base: IUnknownVtbl,
    pub EnableGCNotificationEvents: unsafe extern "system" fn(this: *mut c_void, fEnable: i32) -> HRESULT,
}
pub enum ICorDebugProcess10 {}
unsafe impl Interface for ICorDebugProcess10 {
    const IID: GUID = GUID { data1: 0x8F378F6F, data2: 0x1017, data3: 0x4461, data4: [0x98, 0x90, 0xEC, 0xF6, 0x4C, 0x54, 0x07, 0x9F] };
    type Vtbl = ICorDebugProcess10Vtbl;
}
impl ComPtr<ICorDebugProcess10> {
    #[inline]
    pub unsafe fn EnableGCNotificationEvents(&self, fEnable: i32) -> HRESULT {
        (self.vtbl().EnableGCNotificationEvents)(self.as_raw(), fEnable)
    }
}

// ---- ICorDebugMemoryRangeEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugMemoryRangeEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, objects: *mut COR_MEMORY_RANGE, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugMemoryRangeEnum {}
unsafe impl Interface for ICorDebugMemoryRangeEnum {
    const IID: GUID = GUID { data1: 0xD1A0BCFC, data2: 0x5865, data3: 0x4437, data4: [0xBE, 0x3F, 0x36, 0xF0, 0x22, 0x95, 0x1F, 0x8A] };
    type Vtbl = ICorDebugMemoryRangeEnumVtbl;
}
impl ComPtr<ICorDebugMemoryRangeEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, objects: *mut COR_MEMORY_RANGE, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, objects, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugProcess11 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess11Vtbl {
    pub base: IUnknownVtbl,
    pub EnumerateLoaderHeapMemoryRegions: unsafe extern "system" fn(this: *mut c_void, ppRanges: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugProcess11 {}
unsafe impl Interface for ICorDebugProcess11 {
    const IID: GUID = GUID { data1: 0x344B37AA, data2: 0xF2C0, data3: 0x4D3B, data4: [0x99, 0x09, 0x91, 0xCC, 0xF7, 0x87, 0xDA, 0x8C] };
    type Vtbl = ICorDebugProcess11Vtbl;
}
impl ComPtr<ICorDebugProcess11> {
    #[inline]
    pub unsafe fn EnumerateLoaderHeapMemoryRegions(&self, ppRanges: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateLoaderHeapMemoryRegions)(self.as_raw(), ppRanges)
    }
}

// ---- ICorDebugProcess12 : IUnknown
#[repr(C)]
pub struct ICorDebugProcess12Vtbl {
    pub base: IUnknownVtbl,
    pub GetAsyncStack: unsafe extern "system" fn(this: *mut c_void, continuationAddress: CORDB_ADDRESS, ppStackWalk: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugProcess12 {}
unsafe impl Interface for ICorDebugProcess12 {
    const IID: GUID = GUID { data1: 0x4DCD6FB9, data2: 0x3CF0, data3: 0x43F0, data4: [0x9E, 0xDF, 0xE8, 0x33, 0x07, 0x0F, 0xE6, 0x44] };
    type Vtbl = ICorDebugProcess12Vtbl;
}
impl ComPtr<ICorDebugProcess12> {
    #[inline]
    pub unsafe fn GetAsyncStack(&self, continuationAddress: CORDB_ADDRESS, ppStackWalk: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAsyncStack)(self.as_raw(), continuationAddress, ppStackWalk)
    }
}

// ---- ICorDebugModuleDebugEvent : ICorDebugDebugEvent
#[repr(C)]
pub struct ICorDebugModuleDebugEventVtbl {
    pub base: ICorDebugDebugEventVtbl,
    pub GetModule: unsafe extern "system" fn(this: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugModuleDebugEvent {}
unsafe impl Interface for ICorDebugModuleDebugEvent {
    const IID: GUID = GUID { data1: 0x51A15E8D, data2: 0x9FFF, data3: 0x4864, data4: [0x9B, 0x87, 0xF4, 0xFB, 0xDE, 0xA7, 0x47, 0xA2] };
    type Vtbl = ICorDebugModuleDebugEventVtbl;
}
impl ComPtr<ICorDebugModuleDebugEvent> {
    #[inline]
    pub unsafe fn GetModule(&self, ppModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModule)(self.as_raw(), ppModule)
    }
    #[inline]
    pub unsafe fn GetEventKind(&self, pDebugEventKind: *mut CorDebugDebugEventKind) -> HRESULT {
        (self.vtbl().base.GetEventKind)(self.as_raw(), pDebugEventKind)
    }
    #[inline]
    pub unsafe fn GetThread(&self, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetThread)(self.as_raw(), ppThread)
    }
}

// ---- ICorDebugExceptionDebugEvent : ICorDebugDebugEvent
#[repr(C)]
pub struct ICorDebugExceptionDebugEventVtbl {
    pub base: ICorDebugDebugEventVtbl,
    pub GetStackPointer: unsafe extern "system" fn(this: *mut c_void, pStackPointer: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetNativeIP: unsafe extern "system" fn(this: *mut c_void, pIP: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetFlags: unsafe extern "system" fn(this: *mut c_void, pdwFlags: *mut CorDebugExceptionFlags) -> HRESULT,
}
pub enum ICorDebugExceptionDebugEvent {}
unsafe impl Interface for ICorDebugExceptionDebugEvent {
    const IID: GUID = GUID { data1: 0xAF79EC94, data2: 0x4752, data3: 0x419C, data4: [0xA6, 0x26, 0x5F, 0xB1, 0xCC, 0x1A, 0x5A, 0xB7] };
    type Vtbl = ICorDebugExceptionDebugEventVtbl;
}
impl ComPtr<ICorDebugExceptionDebugEvent> {
    #[inline]
    pub unsafe fn GetStackPointer(&self, pStackPointer: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetStackPointer)(self.as_raw(), pStackPointer)
    }
    #[inline]
    pub unsafe fn GetNativeIP(&self, pIP: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetNativeIP)(self.as_raw(), pIP)
    }
    #[inline]
    pub unsafe fn GetFlags(&self, pdwFlags: *mut CorDebugExceptionFlags) -> HRESULT {
        (self.vtbl().GetFlags)(self.as_raw(), pdwFlags)
    }
    #[inline]
    pub unsafe fn GetEventKind(&self, pDebugEventKind: *mut CorDebugDebugEventKind) -> HRESULT {
        (self.vtbl().base.GetEventKind)(self.as_raw(), pDebugEventKind)
    }
    #[inline]
    pub unsafe fn GetThread(&self, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetThread)(self.as_raw(), ppThread)
    }
}

// ---- ICorDebugBreakpoint : IUnknown
#[repr(C)]
pub struct ICorDebugBreakpointVtbl {
    pub base: IUnknownVtbl,
    pub Activate: unsafe extern "system" fn(this: *mut c_void, bActive: i32) -> HRESULT,
    pub IsActive: unsafe extern "system" fn(this: *mut c_void, pbActive: *mut i32) -> HRESULT,
}
pub enum ICorDebugBreakpoint {}
unsafe impl Interface for ICorDebugBreakpoint {
    const IID: GUID = GUID { data1: 0xCC7BCAE8, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugBreakpointVtbl;
}
impl ComPtr<ICorDebugBreakpoint> {
    #[inline]
    pub unsafe fn Activate(&self, bActive: i32) -> HRESULT {
        (self.vtbl().Activate)(self.as_raw(), bActive)
    }
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().IsActive)(self.as_raw(), pbActive)
    }
}

// ---- ICorDebugFunctionBreakpoint : ICorDebugBreakpoint
#[repr(C)]
pub struct ICorDebugFunctionBreakpointVtbl {
    pub base: ICorDebugBreakpointVtbl,
    pub GetFunction: unsafe extern "system" fn(this: *mut c_void, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetOffset: unsafe extern "system" fn(this: *mut c_void, pnOffset: *mut u32) -> HRESULT,
}
pub enum ICorDebugFunctionBreakpoint {}
unsafe impl Interface for ICorDebugFunctionBreakpoint {
    const IID: GUID = GUID { data1: 0xCC7BCAE9, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugFunctionBreakpointVtbl;
}
impl ComPtr<ICorDebugFunctionBreakpoint> {
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetOffset(&self, pnOffset: *mut u32) -> HRESULT {
        (self.vtbl().GetOffset)(self.as_raw(), pnOffset)
    }
    #[inline]
    pub unsafe fn Activate(&self, bActive: i32) -> HRESULT {
        (self.vtbl().base.Activate)(self.as_raw(), bActive)
    }
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().base.IsActive)(self.as_raw(), pbActive)
    }
}

// ---- ICorDebugModuleBreakpoint : ICorDebugBreakpoint
#[repr(C)]
pub struct ICorDebugModuleBreakpointVtbl {
    pub base: ICorDebugBreakpointVtbl,
    pub GetModule: unsafe extern "system" fn(this: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugModuleBreakpoint {}
unsafe impl Interface for ICorDebugModuleBreakpoint {
    const IID: GUID = GUID { data1: 0xCC7BCAEA, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugModuleBreakpointVtbl;
}
impl ComPtr<ICorDebugModuleBreakpoint> {
    #[inline]
    pub unsafe fn GetModule(&self, ppModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModule)(self.as_raw(), ppModule)
    }
    #[inline]
    pub unsafe fn Activate(&self, bActive: i32) -> HRESULT {
        (self.vtbl().base.Activate)(self.as_raw(), bActive)
    }
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().base.IsActive)(self.as_raw(), pbActive)
    }
}

// ---- ICorDebugValueBreakpoint : ICorDebugBreakpoint
#[repr(C)]
pub struct ICorDebugValueBreakpointVtbl {
    pub base: ICorDebugBreakpointVtbl,
    pub GetValue: unsafe extern "system" fn(this: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugValueBreakpoint {}
unsafe impl Interface for ICorDebugValueBreakpoint {
    const IID: GUID = GUID { data1: 0xCC7BCAEB, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugValueBreakpointVtbl;
}
impl ComPtr<ICorDebugValueBreakpoint> {
    #[inline]
    pub unsafe fn GetValue(&self, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetValue)(self.as_raw(), ppValue)
    }
    #[inline]
    pub unsafe fn Activate(&self, bActive: i32) -> HRESULT {
        (self.vtbl().base.Activate)(self.as_raw(), bActive)
    }
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().base.IsActive)(self.as_raw(), pbActive)
    }
}

// ---- ICorDebugStepper : IUnknown
#[repr(C)]
pub struct ICorDebugStepperVtbl {
    pub base: IUnknownVtbl,
    pub IsActive: unsafe extern "system" fn(this: *mut c_void, pbActive: *mut i32) -> HRESULT,
    pub Deactivate: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub SetInterceptMask: unsafe extern "system" fn(this: *mut c_void, mask: CorDebugIntercept) -> HRESULT,
    pub SetUnmappedStopMask: unsafe extern "system" fn(this: *mut c_void, mask: CorDebugUnmappedStop) -> HRESULT,
    pub Step: unsafe extern "system" fn(this: *mut c_void, bStepIn: i32) -> HRESULT,
    pub StepRange: unsafe extern "system" fn(this: *mut c_void, bStepIn: i32, ranges: *mut COR_DEBUG_STEP_RANGE, cRangeCount: u32) -> HRESULT,
    pub StepOut: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub SetRangeIL: unsafe extern "system" fn(this: *mut c_void, bIL: i32) -> HRESULT,
}
pub enum ICorDebugStepper {}
unsafe impl Interface for ICorDebugStepper {
    const IID: GUID = GUID { data1: 0xCC7BCAEC, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugStepperVtbl;
}
impl ComPtr<ICorDebugStepper> {
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().IsActive)(self.as_raw(), pbActive)
    }
    #[inline]
    pub unsafe fn Deactivate(&self) -> HRESULT {
        (self.vtbl().Deactivate)(self.as_raw())
    }
    #[inline]
    pub unsafe fn SetInterceptMask(&self, mask: CorDebugIntercept) -> HRESULT {
        (self.vtbl().SetInterceptMask)(self.as_raw(), mask)
    }
    #[inline]
    pub unsafe fn SetUnmappedStopMask(&self, mask: CorDebugUnmappedStop) -> HRESULT {
        (self.vtbl().SetUnmappedStopMask)(self.as_raw(), mask)
    }
    #[inline]
    pub unsafe fn Step(&self, bStepIn: i32) -> HRESULT {
        (self.vtbl().Step)(self.as_raw(), bStepIn)
    }
    #[inline]
    pub unsafe fn StepRange(&self, bStepIn: i32, ranges: *mut COR_DEBUG_STEP_RANGE, cRangeCount: u32) -> HRESULT {
        (self.vtbl().StepRange)(self.as_raw(), bStepIn, ranges, cRangeCount)
    }
    #[inline]
    pub unsafe fn StepOut(&self) -> HRESULT {
        (self.vtbl().StepOut)(self.as_raw())
    }
    #[inline]
    pub unsafe fn SetRangeIL(&self, bIL: i32) -> HRESULT {
        (self.vtbl().SetRangeIL)(self.as_raw(), bIL)
    }
}

// ---- ICorDebugStepper2 : IUnknown
#[repr(C)]
pub struct ICorDebugStepper2Vtbl {
    pub base: IUnknownVtbl,
    pub SetJMC: unsafe extern "system" fn(this: *mut c_void, fIsJMCStepper: i32) -> HRESULT,
}
pub enum ICorDebugStepper2 {}
unsafe impl Interface for ICorDebugStepper2 {
    const IID: GUID = GUID { data1: 0xC5B6E9C3, data2: 0xE7D1, data3: 0x4a8e, data4: [0x87, 0x3B, 0x7F, 0x04, 0x7F, 0x07, 0x06, 0xF7] };
    type Vtbl = ICorDebugStepper2Vtbl;
}
impl ComPtr<ICorDebugStepper2> {
    #[inline]
    pub unsafe fn SetJMC(&self, fIsJMCStepper: i32) -> HRESULT {
        (self.vtbl().SetJMC)(self.as_raw(), fIsJMCStepper)
    }
}

// ---- ICorDebugRegisterSet : IUnknown
#[repr(C)]
pub struct ICorDebugRegisterSetVtbl {
    pub base: IUnknownVtbl,
    pub GetRegistersAvailable: unsafe extern "system" fn(this: *mut c_void, pAvailable: *mut u64) -> HRESULT,
    pub GetRegisters: unsafe extern "system" fn(this: *mut c_void, mask: u64, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT,
    pub SetRegisters: unsafe extern "system" fn(this: *mut c_void, mask: u64, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT,
    pub GetThreadContext: unsafe extern "system" fn(this: *mut c_void, contextSize: u32, context: *mut u8) -> HRESULT,
    pub SetThreadContext: unsafe extern "system" fn(this: *mut c_void, contextSize: u32, context: *mut u8) -> HRESULT,
}
pub enum ICorDebugRegisterSet {}
unsafe impl Interface for ICorDebugRegisterSet {
    const IID: GUID = GUID { data1: 0xCC7BCB0B, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugRegisterSetVtbl;
}
impl ComPtr<ICorDebugRegisterSet> {
    #[inline]
    pub unsafe fn GetRegistersAvailable(&self, pAvailable: *mut u64) -> HRESULT {
        (self.vtbl().GetRegistersAvailable)(self.as_raw(), pAvailable)
    }
    #[inline]
    pub unsafe fn GetRegisters(&self, mask: u64, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT {
        (self.vtbl().GetRegisters)(self.as_raw(), mask, regCount, regBuffer)
    }
    #[inline]
    pub unsafe fn SetRegisters(&self, mask: u64, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT {
        (self.vtbl().SetRegisters)(self.as_raw(), mask, regCount, regBuffer)
    }
    #[inline]
    pub unsafe fn GetThreadContext(&self, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().GetThreadContext)(self.as_raw(), contextSize, context)
    }
    #[inline]
    pub unsafe fn SetThreadContext(&self, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().SetThreadContext)(self.as_raw(), contextSize, context)
    }
}

// ---- ICorDebugRegisterSet2 : IUnknown
#[repr(C)]
pub struct ICorDebugRegisterSet2Vtbl {
    pub base: IUnknownVtbl,
    pub GetRegistersAvailable: unsafe extern "system" fn(this: *mut c_void, numChunks: u32, availableRegChunks: *mut u8) -> HRESULT,
    pub GetRegisters: unsafe extern "system" fn(this: *mut c_void, maskCount: u32, mask: *mut u8, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT,
    pub SetRegisters: unsafe extern "system" fn(this: *mut c_void, maskCount: u32, mask: *mut u8, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT,
}
pub enum ICorDebugRegisterSet2 {}
unsafe impl Interface for ICorDebugRegisterSet2 {
    const IID: GUID = GUID { data1: 0x6DC7BA3F, data2: 0x89BA, data3: 0x4459, data4: [0x9E, 0xC1, 0x9D, 0x60, 0x93, 0x7B, 0x46, 0x8D] };
    type Vtbl = ICorDebugRegisterSet2Vtbl;
}
impl ComPtr<ICorDebugRegisterSet2> {
    #[inline]
    pub unsafe fn GetRegistersAvailable(&self, numChunks: u32, availableRegChunks: *mut u8) -> HRESULT {
        (self.vtbl().GetRegistersAvailable)(self.as_raw(), numChunks, availableRegChunks)
    }
    #[inline]
    pub unsafe fn GetRegisters(&self, maskCount: u32, mask: *mut u8, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT {
        (self.vtbl().GetRegisters)(self.as_raw(), maskCount, mask, regCount, regBuffer)
    }
    #[inline]
    pub unsafe fn SetRegisters(&self, maskCount: u32, mask: *mut u8, regCount: u32, regBuffer: *mut CORDB_REGISTER) -> HRESULT {
        (self.vtbl().SetRegisters)(self.as_raw(), maskCount, mask, regCount, regBuffer)
    }
}

// ---- ICorDebugThread : IUnknown
#[repr(C)]
pub struct ICorDebugThreadVtbl {
    pub base: IUnknownVtbl,
    pub GetProcess: unsafe extern "system" fn(this: *mut c_void, ppProcess: *mut *mut c_void) -> HRESULT,
    pub GetID: unsafe extern "system" fn(this: *mut c_void, pdwThreadId: *mut u32) -> HRESULT,
    pub GetHandle: unsafe extern "system" fn(this: *mut c_void, phThreadHandle: *mut HTHREAD) -> HRESULT,
    pub GetAppDomain: unsafe extern "system" fn(this: *mut c_void, ppAppDomain: *mut *mut c_void) -> HRESULT,
    pub SetDebugState: unsafe extern "system" fn(this: *mut c_void, state: CorDebugThreadState) -> HRESULT,
    pub GetDebugState: unsafe extern "system" fn(this: *mut c_void, pState: *mut CorDebugThreadState) -> HRESULT,
    pub GetUserState: unsafe extern "system" fn(this: *mut c_void, pState: *mut CorDebugUserState) -> HRESULT,
    pub GetCurrentException: unsafe extern "system" fn(this: *mut c_void, ppExceptionObject: *mut *mut c_void) -> HRESULT,
    pub ClearCurrentException: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub CreateStepper: unsafe extern "system" fn(this: *mut c_void, ppStepper: *mut *mut c_void) -> HRESULT,
    pub EnumerateChains: unsafe extern "system" fn(this: *mut c_void, ppChains: *mut *mut c_void) -> HRESULT,
    pub GetActiveChain: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub GetActiveFrame: unsafe extern "system" fn(this: *mut c_void, ppFrame: *mut *mut c_void) -> HRESULT,
    pub GetRegisterSet: unsafe extern "system" fn(this: *mut c_void, ppRegisters: *mut *mut c_void) -> HRESULT,
    pub CreateEval: unsafe extern "system" fn(this: *mut c_void, ppEval: *mut *mut c_void) -> HRESULT,
    pub GetObject: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugThread {}
unsafe impl Interface for ICorDebugThread {
    const IID: GUID = GUID { data1: 0x938c6d66, data2: 0x7fb6, data3: 0x4f69, data4: [0xb3, 0x89, 0x42, 0x5b, 0x89, 0x87, 0x32, 0x9b] };
    type Vtbl = ICorDebugThreadVtbl;
}
impl ComPtr<ICorDebugThread> {
    #[inline]
    pub unsafe fn GetProcess(&self, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcess)(self.as_raw(), ppProcess)
    }
    #[inline]
    pub unsafe fn GetID(&self, pdwThreadId: *mut u32) -> HRESULT {
        (self.vtbl().GetID)(self.as_raw(), pdwThreadId)
    }
    #[inline]
    pub unsafe fn GetHandle(&self, phThreadHandle: *mut HTHREAD) -> HRESULT {
        (self.vtbl().GetHandle)(self.as_raw(), phThreadHandle)
    }
    #[inline]
    pub unsafe fn GetAppDomain(&self, ppAppDomain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAppDomain)(self.as_raw(), ppAppDomain)
    }
    #[inline]
    pub unsafe fn SetDebugState(&self, state: CorDebugThreadState) -> HRESULT {
        (self.vtbl().SetDebugState)(self.as_raw(), state)
    }
    #[inline]
    pub unsafe fn GetDebugState(&self, pState: *mut CorDebugThreadState) -> HRESULT {
        (self.vtbl().GetDebugState)(self.as_raw(), pState)
    }
    #[inline]
    pub unsafe fn GetUserState(&self, pState: *mut CorDebugUserState) -> HRESULT {
        (self.vtbl().GetUserState)(self.as_raw(), pState)
    }
    #[inline]
    pub unsafe fn GetCurrentException(&self, ppExceptionObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCurrentException)(self.as_raw(), ppExceptionObject)
    }
    #[inline]
    pub unsafe fn ClearCurrentException(&self) -> HRESULT {
        (self.vtbl().ClearCurrentException)(self.as_raw())
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateStepper)(self.as_raw(), ppStepper)
    }
    #[inline]
    pub unsafe fn EnumerateChains(&self, ppChains: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateChains)(self.as_raw(), ppChains)
    }
    #[inline]
    pub unsafe fn GetActiveChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetActiveChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetActiveFrame(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetActiveFrame)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetRegisterSet(&self, ppRegisters: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRegisterSet)(self.as_raw(), ppRegisters)
    }
    #[inline]
    pub unsafe fn CreateEval(&self, ppEval: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateEval)(self.as_raw(), ppEval)
    }
    #[inline]
    pub unsafe fn GetObject(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObject)(self.as_raw(), ppObject)
    }
}

// ---- ICorDebugThread2 : IUnknown
#[repr(C)]
pub struct ICorDebugThread2Vtbl {
    pub base: IUnknownVtbl,
    pub GetActiveFunctions: unsafe extern "system" fn(this: *mut c_void, cFunctions: u32, pcFunctions: *mut u32, pFunctions: *mut COR_ACTIVE_FUNCTION) -> HRESULT,
    pub GetConnectionID: unsafe extern "system" fn(this: *mut c_void, pdwConnectionId: *mut CONNID) -> HRESULT,
    pub GetTaskID: unsafe extern "system" fn(this: *mut c_void, pTaskId: *mut TASKID) -> HRESULT,
    pub GetVolatileOSThreadID: unsafe extern "system" fn(this: *mut c_void, pdwTid: *mut u32) -> HRESULT,
    pub InterceptCurrentException: unsafe extern "system" fn(this: *mut c_void, pFrame: *mut c_void) -> HRESULT,
}
pub enum ICorDebugThread2 {}
unsafe impl Interface for ICorDebugThread2 {
    const IID: GUID = GUID { data1: 0x2BD956D9, data2: 0x7B07, data3: 0x4bef, data4: [0x8A, 0x98, 0x12, 0xAA, 0x86, 0x24, 0x17, 0xC5] };
    type Vtbl = ICorDebugThread2Vtbl;
}
impl ComPtr<ICorDebugThread2> {
    #[inline]
    pub unsafe fn GetActiveFunctions(&self, cFunctions: u32, pcFunctions: *mut u32, pFunctions: *mut COR_ACTIVE_FUNCTION) -> HRESULT {
        (self.vtbl().GetActiveFunctions)(self.as_raw(), cFunctions, pcFunctions, pFunctions)
    }
    #[inline]
    pub unsafe fn GetConnectionID(&self, pdwConnectionId: *mut CONNID) -> HRESULT {
        (self.vtbl().GetConnectionID)(self.as_raw(), pdwConnectionId)
    }
    #[inline]
    pub unsafe fn GetTaskID(&self, pTaskId: *mut TASKID) -> HRESULT {
        (self.vtbl().GetTaskID)(self.as_raw(), pTaskId)
    }
    #[inline]
    pub unsafe fn GetVolatileOSThreadID(&self, pdwTid: *mut u32) -> HRESULT {
        (self.vtbl().GetVolatileOSThreadID)(self.as_raw(), pdwTid)
    }
    #[inline]
    pub unsafe fn InterceptCurrentException(&self, pFrame: *mut c_void) -> HRESULT {
        (self.vtbl().InterceptCurrentException)(self.as_raw(), pFrame)
    }
}

// ---- ICorDebugThread3 : IUnknown
#[repr(C)]
pub struct ICorDebugThread3Vtbl {
    pub base: IUnknownVtbl,
    pub CreateStackWalk: unsafe extern "system" fn(this: *mut c_void, ppStackWalk: *mut *mut c_void) -> HRESULT,
    pub GetActiveInternalFrames: unsafe extern "system" fn(this: *mut c_void, cInternalFrames: u32, pcInternalFrames: *mut u32, ppInternalFrames: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugThread3 {}
unsafe impl Interface for ICorDebugThread3 {
    const IID: GUID = GUID { data1: 0xF8544EC3, data2: 0x5E4E, data3: 0x46c7, data4: [0x8D, 0x3E, 0xA5, 0x2B, 0x84, 0x05, 0xB1, 0xF5] };
    type Vtbl = ICorDebugThread3Vtbl;
}
impl ComPtr<ICorDebugThread3> {
    #[inline]
    pub unsafe fn CreateStackWalk(&self, ppStackWalk: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateStackWalk)(self.as_raw(), ppStackWalk)
    }
    #[inline]
    pub unsafe fn GetActiveInternalFrames(&self, cInternalFrames: u32, pcInternalFrames: *mut u32, ppInternalFrames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetActiveInternalFrames)(self.as_raw(), cInternalFrames, pcInternalFrames, ppInternalFrames)
    }
}

// ---- ICorDebugThread4 : IUnknown
#[repr(C)]
pub struct ICorDebugThread4Vtbl {
    pub base: IUnknownVtbl,
    pub HasUnhandledException: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub GetBlockingObjects: unsafe extern "system" fn(this: *mut c_void, ppBlockingObjectEnum: *mut *mut c_void) -> HRESULT,
    pub GetCurrentCustomDebuggerNotification: unsafe extern "system" fn(this: *mut c_void, ppNotificationObject: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugThread4 {}
unsafe impl Interface for ICorDebugThread4 {
    const IID: GUID = GUID { data1: 0x1A1F204B, data2: 0x1C66, data3: 0x4637, data4: [0x82, 0x3F, 0x3E, 0xE6, 0xC7, 0x44, 0xA6, 0x9C] };
    type Vtbl = ICorDebugThread4Vtbl;
}
impl ComPtr<ICorDebugThread4> {
    #[inline]
    pub unsafe fn HasUnhandledException(&self) -> HRESULT {
        (self.vtbl().HasUnhandledException)(self.as_raw())
    }
    #[inline]
    pub unsafe fn GetBlockingObjects(&self, ppBlockingObjectEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetBlockingObjects)(self.as_raw(), ppBlockingObjectEnum)
    }
    #[inline]
    pub unsafe fn GetCurrentCustomDebuggerNotification(&self, ppNotificationObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCurrentCustomDebuggerNotification)(self.as_raw(), ppNotificationObject)
    }
}

// ---- ICorDebugThread5 : IUnknown
#[repr(C)]
pub struct ICorDebugThread5Vtbl {
    pub base: IUnknownVtbl,
    pub GetBytesAllocated: unsafe extern "system" fn(this: *mut c_void, pSohAllocatedBytes: *mut u64, pUohAllocatedBytes: *mut u64) -> HRESULT,
}
pub enum ICorDebugThread5 {}
unsafe impl Interface for ICorDebugThread5 {
    const IID: GUID = GUID { data1: 0xF98421C4, data2: 0xE506, data3: 0x4D24, data4: [0x91, 0x6F, 0x02, 0x37, 0xEE, 0x85, 0x3E, 0xC6] };
    type Vtbl = ICorDebugThread5Vtbl;
}
impl ComPtr<ICorDebugThread5> {
    #[inline]
    pub unsafe fn GetBytesAllocated(&self, pSohAllocatedBytes: *mut u64, pUohAllocatedBytes: *mut u64) -> HRESULT {
        (self.vtbl().GetBytesAllocated)(self.as_raw(), pSohAllocatedBytes, pUohAllocatedBytes)
    }
}

// ---- ICorDebugStackWalk : IUnknown
#[repr(C)]
pub struct ICorDebugStackWalkVtbl {
    pub base: IUnknownVtbl,
    pub GetContext: unsafe extern "system" fn(this: *mut c_void, contextFlags: u32, contextBufSize: u32, contextSize: *mut u32, contextBuf: *mut u8) -> HRESULT,
    pub SetContext: unsafe extern "system" fn(this: *mut c_void, flag: CorDebugSetContextFlag, contextSize: u32, context: *mut u8) -> HRESULT,
    pub Next: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub GetFrame: unsafe extern "system" fn(this: *mut c_void, pFrame: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugStackWalk {}
unsafe impl Interface for ICorDebugStackWalk {
    const IID: GUID = GUID { data1: 0xA0647DE9, data2: 0x55DE, data3: 0x4816, data4: [0x92, 0x9C, 0x38, 0x52, 0x71, 0xC6, 0x4C, 0xF7] };
    type Vtbl = ICorDebugStackWalkVtbl;
}
impl ComPtr<ICorDebugStackWalk> {
    #[inline]
    pub unsafe fn GetContext(&self, contextFlags: u32, contextBufSize: u32, contextSize: *mut u32, contextBuf: *mut u8) -> HRESULT {
        (self.vtbl().GetContext)(self.as_raw(), contextFlags, contextBufSize, contextSize, contextBuf)
    }
    #[inline]
    pub unsafe fn SetContext(&self, flag: CorDebugSetContextFlag, contextSize: u32, context: *mut u8) -> HRESULT {
        (self.vtbl().SetContext)(self.as_raw(), flag, contextSize, context)
    }
    #[inline]
    pub unsafe fn Next(&self) -> HRESULT {
        (self.vtbl().Next)(self.as_raw())
    }
    #[inline]
    pub unsafe fn GetFrame(&self, pFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFrame)(self.as_raw(), pFrame)
    }
}

// ---- ICorDebugChain : IUnknown
#[repr(C)]
pub struct ICorDebugChainVtbl {
    pub base: IUnknownVtbl,
    pub GetThread: unsafe extern "system" fn(this: *mut c_void, ppThread: *mut *mut c_void) -> HRESULT,
    pub GetStackRange: unsafe extern "system" fn(this: *mut c_void, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetContext: unsafe extern "system" fn(this: *mut c_void, ppContext: *mut *mut c_void) -> HRESULT,
    pub GetCaller: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub GetCallee: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub GetPrevious: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub GetNext: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub IsManaged: unsafe extern "system" fn(this: *mut c_void, pManaged: *mut i32) -> HRESULT,
    pub EnumerateFrames: unsafe extern "system" fn(this: *mut c_void, ppFrames: *mut *mut c_void) -> HRESULT,
    pub GetActiveFrame: unsafe extern "system" fn(this: *mut c_void, ppFrame: *mut *mut c_void) -> HRESULT,
    pub GetRegisterSet: unsafe extern "system" fn(this: *mut c_void, ppRegisters: *mut *mut c_void) -> HRESULT,
    pub GetReason: unsafe extern "system" fn(this: *mut c_void, pReason: *mut CorDebugChainReason) -> HRESULT,
}
pub enum ICorDebugChain {}
unsafe impl Interface for ICorDebugChain {
    const IID: GUID = GUID { data1: 0xCC7BCAEE, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugChainVtbl;
}
impl ComPtr<ICorDebugChain> {
    #[inline]
    pub unsafe fn GetThread(&self, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetThread)(self.as_raw(), ppThread)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetContext(&self, ppContext: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetContext)(self.as_raw(), ppContext)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCaller)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCallee)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetPrevious(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetPrevious)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetNext(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetNext)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn IsManaged(&self, pManaged: *mut i32) -> HRESULT {
        (self.vtbl().IsManaged)(self.as_raw(), pManaged)
    }
    #[inline]
    pub unsafe fn EnumerateFrames(&self, ppFrames: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateFrames)(self.as_raw(), ppFrames)
    }
    #[inline]
    pub unsafe fn GetActiveFrame(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetActiveFrame)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetRegisterSet(&self, ppRegisters: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRegisterSet)(self.as_raw(), ppRegisters)
    }
    #[inline]
    pub unsafe fn GetReason(&self, pReason: *mut CorDebugChainReason) -> HRESULT {
        (self.vtbl().GetReason)(self.as_raw(), pReason)
    }
}

// ---- ICorDebugFrame : IUnknown
#[repr(C)]
pub struct ICorDebugFrameVtbl {
    pub base: IUnknownVtbl,
    pub GetChain: unsafe extern "system" fn(this: *mut c_void, ppChain: *mut *mut c_void) -> HRESULT,
    pub GetCode: unsafe extern "system" fn(this: *mut c_void, ppCode: *mut *mut c_void) -> HRESULT,
    pub GetFunction: unsafe extern "system" fn(this: *mut c_void, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetFunctionToken: unsafe extern "system" fn(this: *mut c_void, pToken: *mut mdMethodDef) -> HRESULT,
    pub GetStackRange: unsafe extern "system" fn(this: *mut c_void, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetCaller: unsafe extern "system" fn(this: *mut c_void, ppFrame: *mut *mut c_void) -> HRESULT,
    pub GetCallee: unsafe extern "system" fn(this: *mut c_void, ppFrame: *mut *mut c_void) -> HRESULT,
    pub CreateStepper: unsafe extern "system" fn(this: *mut c_void, ppStepper: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugFrame {}
unsafe impl Interface for ICorDebugFrame {
    const IID: GUID = GUID { data1: 0xCC7BCAEF, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugFrameVtbl;
}
impl ComPtr<ICorDebugFrame> {
    #[inline]
    pub unsafe fn GetChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionToken(&self, pToken: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().GetFunctionToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCaller)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCallee)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateStepper)(self.as_raw(), ppStepper)
    }
}

// ---- ICorDebugInternalFrame : ICorDebugFrame
#[repr(C)]
pub struct ICorDebugInternalFrameVtbl {
    pub base: ICorDebugFrameVtbl,
    pub GetFrameType: unsafe extern "system" fn(this: *mut c_void, pType: *mut CorDebugInternalFrameType) -> HRESULT,
}
pub enum ICorDebugInternalFrame {}
unsafe impl Interface for ICorDebugInternalFrame {
    const IID: GUID = GUID { data1: 0xB92CC7F7, data2: 0x9D2D, data3: 0x45c4, data4: [0xBC, 0x2B, 0x62, 0x1F, 0xCC, 0x9D, 0xFB, 0xF4] };
    type Vtbl = ICorDebugInternalFrameVtbl;
}
impl ComPtr<ICorDebugInternalFrame> {
    #[inline]
    pub unsafe fn GetFrameType(&self, pType: *mut CorDebugInternalFrameType) -> HRESULT {
        (self.vtbl().GetFrameType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionToken(&self, pToken: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().base.GetFunctionToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCaller)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCallee)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateStepper)(self.as_raw(), ppStepper)
    }
}

// ---- ICorDebugInternalFrame2 : IUnknown
#[repr(C)]
pub struct ICorDebugInternalFrame2Vtbl {
    pub base: IUnknownVtbl,
    pub GetAddress: unsafe extern "system" fn(this: *mut c_void, pAddress: *mut CORDB_ADDRESS) -> HRESULT,
    pub IsCloserToLeaf: unsafe extern "system" fn(this: *mut c_void, pFrameToCompare: *mut c_void, pIsCloser: *mut i32) -> HRESULT,
}
pub enum ICorDebugInternalFrame2 {}
unsafe impl Interface for ICorDebugInternalFrame2 {
    const IID: GUID = GUID { data1: 0xC0815BDC, data2: 0xCFAB, data3: 0x447e, data4: [0xA7, 0x79, 0xC1, 0x16, 0xB4, 0x54, 0xEB, 0x5B] };
    type Vtbl = ICorDebugInternalFrame2Vtbl;
}
impl ComPtr<ICorDebugInternalFrame2> {
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn IsCloserToLeaf(&self, pFrameToCompare: *mut c_void, pIsCloser: *mut i32) -> HRESULT {
        (self.vtbl().IsCloserToLeaf)(self.as_raw(), pFrameToCompare, pIsCloser)
    }
}

// ---- ICorDebugILFrame : ICorDebugFrame
#[repr(C)]
pub struct ICorDebugILFrameVtbl {
    pub base: ICorDebugFrameVtbl,
    pub GetIP: unsafe extern "system" fn(this: *mut c_void, pnOffset: *mut u32, pMappingResult: *mut CorDebugMappingResult) -> HRESULT,
    pub SetIP: unsafe extern "system" fn(this: *mut c_void, nOffset: u32) -> HRESULT,
    pub EnumerateLocalVariables: unsafe extern "system" fn(this: *mut c_void, ppValueEnum: *mut *mut c_void) -> HRESULT,
    pub GetLocalVariable: unsafe extern "system" fn(this: *mut c_void, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT,
    pub EnumerateArguments: unsafe extern "system" fn(this: *mut c_void, ppValueEnum: *mut *mut c_void) -> HRESULT,
    pub GetArgument: unsafe extern "system" fn(this: *mut c_void, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetStackDepth: unsafe extern "system" fn(this: *mut c_void, pDepth: *mut u32) -> HRESULT,
    pub GetStackValue: unsafe extern "system" fn(this: *mut c_void, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT,
    pub CanSetIP: unsafe extern "system" fn(this: *mut c_void, nOffset: u32) -> HRESULT,
}
pub enum ICorDebugILFrame {}
unsafe impl Interface for ICorDebugILFrame {
    const IID: GUID = GUID { data1: 0x03E26311, data2: 0x4F76, data3: 0x11d3, data4: [0x88, 0xC6, 0x00, 0x60, 0x97, 0x94, 0x54, 0x18] };
    type Vtbl = ICorDebugILFrameVtbl;
}
impl ComPtr<ICorDebugILFrame> {
    #[inline]
    pub unsafe fn GetIP(&self, pnOffset: *mut u32, pMappingResult: *mut CorDebugMappingResult) -> HRESULT {
        (self.vtbl().GetIP)(self.as_raw(), pnOffset, pMappingResult)
    }
    #[inline]
    pub unsafe fn SetIP(&self, nOffset: u32) -> HRESULT {
        (self.vtbl().SetIP)(self.as_raw(), nOffset)
    }
    #[inline]
    pub unsafe fn EnumerateLocalVariables(&self, ppValueEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateLocalVariables)(self.as_raw(), ppValueEnum)
    }
    #[inline]
    pub unsafe fn GetLocalVariable(&self, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalVariable)(self.as_raw(), dwIndex, ppValue)
    }
    #[inline]
    pub unsafe fn EnumerateArguments(&self, ppValueEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateArguments)(self.as_raw(), ppValueEnum)
    }
    #[inline]
    pub unsafe fn GetArgument(&self, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetArgument)(self.as_raw(), dwIndex, ppValue)
    }
    #[inline]
    pub unsafe fn GetStackDepth(&self, pDepth: *mut u32) -> HRESULT {
        (self.vtbl().GetStackDepth)(self.as_raw(), pDepth)
    }
    #[inline]
    pub unsafe fn GetStackValue(&self, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetStackValue)(self.as_raw(), dwIndex, ppValue)
    }
    #[inline]
    pub unsafe fn CanSetIP(&self, nOffset: u32) -> HRESULT {
        (self.vtbl().CanSetIP)(self.as_raw(), nOffset)
    }
    #[inline]
    pub unsafe fn GetChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionToken(&self, pToken: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().base.GetFunctionToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCaller)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCallee)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateStepper)(self.as_raw(), ppStepper)
    }
}

// ---- ICorDebugILFrame2 : IUnknown
#[repr(C)]
pub struct ICorDebugILFrame2Vtbl {
    pub base: IUnknownVtbl,
    pub RemapFunction: unsafe extern "system" fn(this: *mut c_void, newILOffset: u32) -> HRESULT,
    pub EnumerateTypeParameters: unsafe extern "system" fn(this: *mut c_void, ppTyParEnum: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugILFrame2 {}
unsafe impl Interface for ICorDebugILFrame2 {
    const IID: GUID = GUID { data1: 0x5D88A994, data2: 0x6C30, data3: 0x479b, data4: [0x89, 0x0F, 0xBC, 0xEF, 0x88, 0xB1, 0x29, 0xA5] };
    type Vtbl = ICorDebugILFrame2Vtbl;
}
impl ComPtr<ICorDebugILFrame2> {
    #[inline]
    pub unsafe fn RemapFunction(&self, newILOffset: u32) -> HRESULT {
        (self.vtbl().RemapFunction)(self.as_raw(), newILOffset)
    }
    #[inline]
    pub unsafe fn EnumerateTypeParameters(&self, ppTyParEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateTypeParameters)(self.as_raw(), ppTyParEnum)
    }
}

// ---- ICorDebugILFrame3 : IUnknown
#[repr(C)]
pub struct ICorDebugILFrame3Vtbl {
    pub base: IUnknownVtbl,
    pub GetReturnValueForILOffset: unsafe extern "system" fn(this: *mut c_void, ILoffset: u32, ppReturnValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugILFrame3 {}
unsafe impl Interface for ICorDebugILFrame3 {
    const IID: GUID = GUID { data1: 0x9A9E2ED6, data2: 0x04DF, data3: 0x4FE0, data4: [0xBB, 0x50, 0xCA, 0xB6, 0x41, 0x26, 0xAD, 0x24] };
    type Vtbl = ICorDebugILFrame3Vtbl;
}
impl ComPtr<ICorDebugILFrame3> {
    #[inline]
    pub unsafe fn GetReturnValueForILOffset(&self, ILoffset: u32, ppReturnValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetReturnValueForILOffset)(self.as_raw(), ILoffset, ppReturnValue)
    }
}

// ---- ICorDebugILFrame4 : IUnknown
#[repr(C)]
pub struct ICorDebugILFrame4Vtbl {
    pub base: IUnknownVtbl,
    pub EnumerateLocalVariablesEx: unsafe extern "system" fn(this: *mut c_void, flags: ILCodeKind, ppValueEnum: *mut *mut c_void) -> HRESULT,
    pub GetLocalVariableEx: unsafe extern "system" fn(this: *mut c_void, flags: ILCodeKind, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetCodeEx: unsafe extern "system" fn(this: *mut c_void, flags: ILCodeKind, ppCode: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugILFrame4 {}
unsafe impl Interface for ICorDebugILFrame4 {
    const IID: GUID = GUID { data1: 0xAD914A30, data2: 0xC6D1, data3: 0x4AC5, data4: [0x9C, 0x5E, 0x57, 0x7F, 0x3B, 0xAA, 0x8A, 0x45] };
    type Vtbl = ICorDebugILFrame4Vtbl;
}
impl ComPtr<ICorDebugILFrame4> {
    #[inline]
    pub unsafe fn EnumerateLocalVariablesEx(&self, flags: ILCodeKind, ppValueEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateLocalVariablesEx)(self.as_raw(), flags, ppValueEnum)
    }
    #[inline]
    pub unsafe fn GetLocalVariableEx(&self, flags: ILCodeKind, dwIndex: u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalVariableEx)(self.as_raw(), flags, dwIndex, ppValue)
    }
    #[inline]
    pub unsafe fn GetCodeEx(&self, flags: ILCodeKind, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCodeEx)(self.as_raw(), flags, ppCode)
    }
}

// ---- ICorDebugNativeFrame : ICorDebugFrame
#[repr(C)]
pub struct ICorDebugNativeFrameVtbl {
    pub base: ICorDebugFrameVtbl,
    pub GetIP: unsafe extern "system" fn(this: *mut c_void, pnOffset: *mut u32) -> HRESULT,
    pub SetIP: unsafe extern "system" fn(this: *mut c_void, nOffset: u32) -> HRESULT,
    pub GetRegisterSet: unsafe extern "system" fn(this: *mut c_void, ppRegisters: *mut *mut c_void) -> HRESULT,
    pub GetLocalRegisterValue: unsafe extern "system" fn(this: *mut c_void, reg: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetLocalDoubleRegisterValue: unsafe extern "system" fn(this: *mut c_void, highWordReg: CorDebugRegister, lowWordReg: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetLocalMemoryValue: unsafe extern "system" fn(this: *mut c_void, address: CORDB_ADDRESS, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetLocalRegisterMemoryValue: unsafe extern "system" fn(this: *mut c_void, highWordReg: CorDebugRegister, lowWordAddress: CORDB_ADDRESS, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetLocalMemoryRegisterValue: unsafe extern "system" fn(this: *mut c_void, highWordAddress: CORDB_ADDRESS, lowWordRegister: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT,
    pub CanSetIP: unsafe extern "system" fn(this: *mut c_void, nOffset: u32) -> HRESULT,
}
pub enum ICorDebugNativeFrame {}
unsafe impl Interface for ICorDebugNativeFrame {
    const IID: GUID = GUID { data1: 0x03E26314, data2: 0x4F76, data3: 0x11d3, data4: [0x88, 0xC6, 0x00, 0x60, 0x97, 0x94, 0x54, 0x18] };
    type Vtbl = ICorDebugNativeFrameVtbl;
}
impl ComPtr<ICorDebugNativeFrame> {
    #[inline]
    pub unsafe fn GetIP(&self, pnOffset: *mut u32) -> HRESULT {
        (self.vtbl().GetIP)(self.as_raw(), pnOffset)
    }
    #[inline]
    pub unsafe fn SetIP(&self, nOffset: u32) -> HRESULT {
        (self.vtbl().SetIP)(self.as_raw(), nOffset)
    }
    #[inline]
    pub unsafe fn GetRegisterSet(&self, ppRegisters: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRegisterSet)(self.as_raw(), ppRegisters)
    }
    #[inline]
    pub unsafe fn GetLocalRegisterValue(&self, reg: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalRegisterValue)(self.as_raw(), reg, cbSigBlob, pvSigBlob, ppValue)
    }
    #[inline]
    pub unsafe fn GetLocalDoubleRegisterValue(&self, highWordReg: CorDebugRegister, lowWordReg: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalDoubleRegisterValue)(self.as_raw(), highWordReg, lowWordReg, cbSigBlob, pvSigBlob, ppValue)
    }
    #[inline]
    pub unsafe fn GetLocalMemoryValue(&self, address: CORDB_ADDRESS, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalMemoryValue)(self.as_raw(), address, cbSigBlob, pvSigBlob, ppValue)
    }
    #[inline]
    pub unsafe fn GetLocalRegisterMemoryValue(&self, highWordReg: CorDebugRegister, lowWordAddress: CORDB_ADDRESS, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalRegisterMemoryValue)(self.as_raw(), highWordReg, lowWordAddress, cbSigBlob, pvSigBlob, ppValue)
    }
    #[inline]
    pub unsafe fn GetLocalMemoryRegisterValue(&self, highWordAddress: CORDB_ADDRESS, lowWordRegister: CorDebugRegister, cbSigBlob: u32, pvSigBlob: PCCOR_SIGNATURE, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetLocalMemoryRegisterValue)(self.as_raw(), highWordAddress, lowWordRegister, cbSigBlob, pvSigBlob, ppValue)
    }
    #[inline]
    pub unsafe fn CanSetIP(&self, nOffset: u32) -> HRESULT {
        (self.vtbl().CanSetIP)(self.as_raw(), nOffset)
    }
    #[inline]
    pub unsafe fn GetChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionToken(&self, pToken: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().base.GetFunctionToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCaller)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCallee)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateStepper)(self.as_raw(), ppStepper)
    }
}

// ---- ICorDebugNativeFrame2 : IUnknown
#[repr(C)]
pub struct ICorDebugNativeFrame2Vtbl {
    pub base: IUnknownVtbl,
    pub IsChild: unsafe extern "system" fn(this: *mut c_void, pIsChild: *mut i32) -> HRESULT,
    pub IsMatchingParentFrame: unsafe extern "system" fn(this: *mut c_void, pPotentialParentFrame: *mut c_void, pIsParent: *mut i32) -> HRESULT,
    pub GetStackParameterSize: unsafe extern "system" fn(this: *mut c_void, pSize: *mut u32) -> HRESULT,
}
pub enum ICorDebugNativeFrame2 {}
unsafe impl Interface for ICorDebugNativeFrame2 {
    const IID: GUID = GUID { data1: 0x35389FF1, data2: 0x3684, data3: 0x4c55, data4: [0xA2, 0xEE, 0x21, 0x0F, 0x26, 0xC6, 0x0E, 0x5E] };
    type Vtbl = ICorDebugNativeFrame2Vtbl;
}
impl ComPtr<ICorDebugNativeFrame2> {
    #[inline]
    pub unsafe fn IsChild(&self, pIsChild: *mut i32) -> HRESULT {
        (self.vtbl().IsChild)(self.as_raw(), pIsChild)
    }
    #[inline]
    pub unsafe fn IsMatchingParentFrame(&self, pPotentialParentFrame: *mut c_void, pIsParent: *mut i32) -> HRESULT {
        (self.vtbl().IsMatchingParentFrame)(self.as_raw(), pPotentialParentFrame, pIsParent)
    }
    #[inline]
    pub unsafe fn GetStackParameterSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().GetStackParameterSize)(self.as_raw(), pSize)
    }
}

// ---- ICorDebugModule3 : IUnknown
#[repr(C)]
pub struct ICorDebugModule3Vtbl {
    pub base: IUnknownVtbl,
    pub CreateReaderForInMemorySymbols: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, ppObj: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugModule3 {}
unsafe impl Interface for ICorDebugModule3 {
    const IID: GUID = GUID { data1: 0x86F012BF, data2: 0xFF15, data3: 0x4372, data4: [0xBD, 0x30, 0xB6, 0xF1, 0x1C, 0xAA, 0xE1, 0xDD] };
    type Vtbl = ICorDebugModule3Vtbl;
}
impl ComPtr<ICorDebugModule3> {
    #[inline]
    pub unsafe fn CreateReaderForInMemorySymbols(&self, riid: *const GUID, ppObj: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateReaderForInMemorySymbols)(self.as_raw(), riid, ppObj)
    }
}

// ---- ICorDebugModule4 : IUnknown
#[repr(C)]
pub struct ICorDebugModule4Vtbl {
    pub base: IUnknownVtbl,
    pub IsMappedLayout: unsafe extern "system" fn(this: *mut c_void, pIsMapped: *mut i32) -> HRESULT,
}
pub enum ICorDebugModule4 {}
unsafe impl Interface for ICorDebugModule4 {
    const IID: GUID = GUID { data1: 0xFF8B8EAF, data2: 0x25CD, data3: 0x4316, data4: [0x88, 0x59, 0x84, 0x41, 0x6D, 0xE4, 0x40, 0x2E] };
    type Vtbl = ICorDebugModule4Vtbl;
}
impl ComPtr<ICorDebugModule4> {
    #[inline]
    pub unsafe fn IsMappedLayout(&self, pIsMapped: *mut i32) -> HRESULT {
        (self.vtbl().IsMappedLayout)(self.as_raw(), pIsMapped)
    }
}

// ---- ICorDebugRuntimeUnwindableFrame : ICorDebugFrame
#[repr(C)]
pub struct ICorDebugRuntimeUnwindableFrameVtbl {
    pub base: ICorDebugFrameVtbl,
}
pub enum ICorDebugRuntimeUnwindableFrame {}
unsafe impl Interface for ICorDebugRuntimeUnwindableFrame {
    const IID: GUID = GUID { data1: 0x879CAC0A, data2: 0x4A53, data3: 0x4668, data4: [0xB8, 0xE3, 0xCB, 0x84, 0x73, 0xCB, 0x18, 0x7F] };
    type Vtbl = ICorDebugRuntimeUnwindableFrameVtbl;
}
impl ComPtr<ICorDebugRuntimeUnwindableFrame> {
    #[inline]
    pub unsafe fn GetChain(&self, ppChain: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetChain)(self.as_raw(), ppChain)
    }
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionToken(&self, pToken: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().base.GetFunctionToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetStackRange(&self, pStart: *mut CORDB_ADDRESS, pEnd: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetStackRange)(self.as_raw(), pStart, pEnd)
    }
    #[inline]
    pub unsafe fn GetCaller(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCaller)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn GetCallee(&self, ppFrame: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetCallee)(self.as_raw(), ppFrame)
    }
    #[inline]
    pub unsafe fn CreateStepper(&self, ppStepper: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateStepper)(self.as_raw(), ppStepper)
    }
}

// ---- ICorDebugModule : IUnknown
#[repr(C)]
pub struct ICorDebugModuleVtbl {
    pub base: IUnknownVtbl,
    pub GetProcess: unsafe extern "system" fn(this: *mut c_void, ppProcess: *mut *mut c_void) -> HRESULT,
    pub GetBaseAddress: unsafe extern "system" fn(this: *mut c_void, pAddress: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetAssembly: unsafe extern "system" fn(this: *mut c_void, ppAssembly: *mut *mut c_void) -> HRESULT,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub EnableJITDebugging: unsafe extern "system" fn(this: *mut c_void, bTrackJITInfo: i32, bAllowJitOpts: i32) -> HRESULT,
    pub EnableClassLoadCallbacks: unsafe extern "system" fn(this: *mut c_void, bClassLoadCallbacks: i32) -> HRESULT,
    pub GetFunctionFromToken: unsafe extern "system" fn(this: *mut c_void, methodDef: mdMethodDef, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetFunctionFromRVA: unsafe extern "system" fn(this: *mut c_void, rva: CORDB_ADDRESS, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetClassFromToken: unsafe extern "system" fn(this: *mut c_void, typeDef: mdTypeDef, ppClass: *mut *mut c_void) -> HRESULT,
    pub CreateBreakpoint: unsafe extern "system" fn(this: *mut c_void, ppBreakpoint: *mut *mut c_void) -> HRESULT,
    pub GetEditAndContinueSnapshot: unsafe extern "system" fn(this: *mut c_void, ppEditAndContinueSnapshot: *mut *mut c_void) -> HRESULT,
    pub GetMetaDataInterface: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, ppObj: *mut *mut c_void) -> HRESULT,
    pub GetToken: unsafe extern "system" fn(this: *mut c_void, pToken: *mut mdModule) -> HRESULT,
    pub IsDynamic: unsafe extern "system" fn(this: *mut c_void, pDynamic: *mut i32) -> HRESULT,
    pub GetGlobalVariableValue: unsafe extern "system" fn(this: *mut c_void, fieldDef: mdFieldDef, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcBytes: *mut u32) -> HRESULT,
    pub IsInMemory: unsafe extern "system" fn(this: *mut c_void, pInMemory: *mut i32) -> HRESULT,
}
pub enum ICorDebugModule {}
unsafe impl Interface for ICorDebugModule {
    const IID: GUID = GUID { data1: 0xdba2d8c1, data2: 0xe5c5, data3: 0x4069, data4: [0x8c, 0x13, 0x10, 0xa7, 0xc6, 0xab, 0xf4, 0x3d] };
    type Vtbl = ICorDebugModuleVtbl;
}
impl ComPtr<ICorDebugModule> {
    #[inline]
    pub unsafe fn GetProcess(&self, ppProcess: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcess)(self.as_raw(), ppProcess)
    }
    #[inline]
    pub unsafe fn GetBaseAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetBaseAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn GetAssembly(&self, ppAssembly: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetAssembly)(self.as_raw(), ppAssembly)
    }
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn EnableJITDebugging(&self, bTrackJITInfo: i32, bAllowJitOpts: i32) -> HRESULT {
        (self.vtbl().EnableJITDebugging)(self.as_raw(), bTrackJITInfo, bAllowJitOpts)
    }
    #[inline]
    pub unsafe fn EnableClassLoadCallbacks(&self, bClassLoadCallbacks: i32) -> HRESULT {
        (self.vtbl().EnableClassLoadCallbacks)(self.as_raw(), bClassLoadCallbacks)
    }
    #[inline]
    pub unsafe fn GetFunctionFromToken(&self, methodDef: mdMethodDef, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunctionFromToken)(self.as_raw(), methodDef, ppFunction)
    }
    #[inline]
    pub unsafe fn GetFunctionFromRVA(&self, rva: CORDB_ADDRESS, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunctionFromRVA)(self.as_raw(), rva, ppFunction)
    }
    #[inline]
    pub unsafe fn GetClassFromToken(&self, typeDef: mdTypeDef, ppClass: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetClassFromToken)(self.as_raw(), typeDef, ppClass)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetEditAndContinueSnapshot(&self, ppEditAndContinueSnapshot: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetEditAndContinueSnapshot)(self.as_raw(), ppEditAndContinueSnapshot)
    }
    #[inline]
    pub unsafe fn GetMetaDataInterface(&self, riid: *const GUID, ppObj: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetMetaDataInterface)(self.as_raw(), riid, ppObj)
    }
    #[inline]
    pub unsafe fn GetToken(&self, pToken: *mut mdModule) -> HRESULT {
        (self.vtbl().GetToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn IsDynamic(&self, pDynamic: *mut i32) -> HRESULT {
        (self.vtbl().IsDynamic)(self.as_raw(), pDynamic)
    }
    #[inline]
    pub unsafe fn GetGlobalVariableValue(&self, fieldDef: mdFieldDef, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetGlobalVariableValue)(self.as_raw(), fieldDef, ppValue)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcBytes: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcBytes)
    }
    #[inline]
    pub unsafe fn IsInMemory(&self, pInMemory: *mut i32) -> HRESULT {
        (self.vtbl().IsInMemory)(self.as_raw(), pInMemory)
    }
}

// ---- ICorDebugModule2 : IUnknown
#[repr(C)]
pub struct ICorDebugModule2Vtbl {
    pub base: IUnknownVtbl,
    pub SetJMCStatus: unsafe extern "system" fn(this: *mut c_void, bIsJustMyCode: i32, cTokens: u32, pTokens: *mut mdToken) -> HRESULT,
    pub ApplyChanges: unsafe extern "system" fn(this: *mut c_void, cbMetadata: u32, pbMetadata: *mut u8, cbIL: u32, pbIL: *mut u8) -> HRESULT,
    pub SetJITCompilerFlags: unsafe extern "system" fn(this: *mut c_void, dwFlags: u32) -> HRESULT,
    pub GetJITCompilerFlags: unsafe extern "system" fn(this: *mut c_void, pdwFlags: *mut u32) -> HRESULT,
    pub ResolveAssembly: unsafe extern "system" fn(this: *mut c_void, tkAssemblyRef: mdToken, ppAssembly: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugModule2 {}
unsafe impl Interface for ICorDebugModule2 {
    const IID: GUID = GUID { data1: 0x7FCC5FB5, data2: 0x49C0, data3: 0x41de, data4: [0x99, 0x38, 0x3B, 0x88, 0xB5, 0xB9, 0xAD, 0xD7] };
    type Vtbl = ICorDebugModule2Vtbl;
}
impl ComPtr<ICorDebugModule2> {
    #[inline]
    pub unsafe fn SetJMCStatus(&self, bIsJustMyCode: i32, cTokens: u32, pTokens: *mut mdToken) -> HRESULT {
        (self.vtbl().SetJMCStatus)(self.as_raw(), bIsJustMyCode, cTokens, pTokens)
    }
    #[inline]
    pub unsafe fn ApplyChanges(&self, cbMetadata: u32, pbMetadata: *mut u8, cbIL: u32, pbIL: *mut u8) -> HRESULT {
        (self.vtbl().ApplyChanges)(self.as_raw(), cbMetadata, pbMetadata, cbIL, pbIL)
    }
    #[inline]
    pub unsafe fn SetJITCompilerFlags(&self, dwFlags: u32) -> HRESULT {
        (self.vtbl().SetJITCompilerFlags)(self.as_raw(), dwFlags)
    }
    #[inline]
    pub unsafe fn GetJITCompilerFlags(&self, pdwFlags: *mut u32) -> HRESULT {
        (self.vtbl().GetJITCompilerFlags)(self.as_raw(), pdwFlags)
    }
    #[inline]
    pub unsafe fn ResolveAssembly(&self, tkAssemblyRef: mdToken, ppAssembly: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ResolveAssembly)(self.as_raw(), tkAssemblyRef, ppAssembly)
    }
}

// ---- ICorDebugFunction : IUnknown
#[repr(C)]
pub struct ICorDebugFunctionVtbl {
    pub base: IUnknownVtbl,
    pub GetModule: unsafe extern "system" fn(this: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT,
    pub GetClass: unsafe extern "system" fn(this: *mut c_void, ppClass: *mut *mut c_void) -> HRESULT,
    pub GetToken: unsafe extern "system" fn(this: *mut c_void, pMethodDef: *mut mdMethodDef) -> HRESULT,
    pub GetILCode: unsafe extern "system" fn(this: *mut c_void, ppCode: *mut *mut c_void) -> HRESULT,
    pub GetNativeCode: unsafe extern "system" fn(this: *mut c_void, ppCode: *mut *mut c_void) -> HRESULT,
    pub CreateBreakpoint: unsafe extern "system" fn(this: *mut c_void, ppBreakpoint: *mut *mut c_void) -> HRESULT,
    pub GetLocalVarSigToken: unsafe extern "system" fn(this: *mut c_void, pmdSig: *mut mdSignature) -> HRESULT,
    pub GetCurrentVersionNumber: unsafe extern "system" fn(this: *mut c_void, pnCurrentVersion: *mut u32) -> HRESULT,
}
pub enum ICorDebugFunction {}
unsafe impl Interface for ICorDebugFunction {
    const IID: GUID = GUID { data1: 0xCC7BCAF3, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugFunctionVtbl;
}
impl ComPtr<ICorDebugFunction> {
    #[inline]
    pub unsafe fn GetModule(&self, ppModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModule)(self.as_raw(), ppModule)
    }
    #[inline]
    pub unsafe fn GetClass(&self, ppClass: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetClass)(self.as_raw(), ppClass)
    }
    #[inline]
    pub unsafe fn GetToken(&self, pMethodDef: *mut mdMethodDef) -> HRESULT {
        (self.vtbl().GetToken)(self.as_raw(), pMethodDef)
    }
    #[inline]
    pub unsafe fn GetILCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetILCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetNativeCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetNativeCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetLocalVarSigToken(&self, pmdSig: *mut mdSignature) -> HRESULT {
        (self.vtbl().GetLocalVarSigToken)(self.as_raw(), pmdSig)
    }
    #[inline]
    pub unsafe fn GetCurrentVersionNumber(&self, pnCurrentVersion: *mut u32) -> HRESULT {
        (self.vtbl().GetCurrentVersionNumber)(self.as_raw(), pnCurrentVersion)
    }
}

// ---- ICorDebugFunction2 : IUnknown
#[repr(C)]
pub struct ICorDebugFunction2Vtbl {
    pub base: IUnknownVtbl,
    pub SetJMCStatus: unsafe extern "system" fn(this: *mut c_void, bIsJustMyCode: i32) -> HRESULT,
    pub GetJMCStatus: unsafe extern "system" fn(this: *mut c_void, pbIsJustMyCode: *mut i32) -> HRESULT,
    pub EnumerateNativeCode: unsafe extern "system" fn(this: *mut c_void, ppCodeEnum: *mut *mut c_void) -> HRESULT,
    pub GetVersionNumber: unsafe extern "system" fn(this: *mut c_void, pnVersion: *mut u32) -> HRESULT,
}
pub enum ICorDebugFunction2 {}
unsafe impl Interface for ICorDebugFunction2 {
    const IID: GUID = GUID { data1: 0xEF0C490B, data2: 0x94C3, data3: 0x4e4d, data4: [0xB6, 0x29, 0xDD, 0xC1, 0x34, 0xC5, 0x32, 0xD8] };
    type Vtbl = ICorDebugFunction2Vtbl;
}
impl ComPtr<ICorDebugFunction2> {
    #[inline]
    pub unsafe fn SetJMCStatus(&self, bIsJustMyCode: i32) -> HRESULT {
        (self.vtbl().SetJMCStatus)(self.as_raw(), bIsJustMyCode)
    }
    #[inline]
    pub unsafe fn GetJMCStatus(&self, pbIsJustMyCode: *mut i32) -> HRESULT {
        (self.vtbl().GetJMCStatus)(self.as_raw(), pbIsJustMyCode)
    }
    #[inline]
    pub unsafe fn EnumerateNativeCode(&self, ppCodeEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateNativeCode)(self.as_raw(), ppCodeEnum)
    }
    #[inline]
    pub unsafe fn GetVersionNumber(&self, pnVersion: *mut u32) -> HRESULT {
        (self.vtbl().GetVersionNumber)(self.as_raw(), pnVersion)
    }
}

// ---- ICorDebugFunction3 : IUnknown
#[repr(C)]
pub struct ICorDebugFunction3Vtbl {
    pub base: IUnknownVtbl,
    pub GetActiveReJitRequestILCode: unsafe extern "system" fn(this: *mut c_void, ppReJitedILCode: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugFunction3 {}
unsafe impl Interface for ICorDebugFunction3 {
    const IID: GUID = GUID { data1: 0x09B70F28, data2: 0xE465, data3: 0x482D, data4: [0x99, 0xE0, 0x81, 0xA1, 0x65, 0xEB, 0x05, 0x32] };
    type Vtbl = ICorDebugFunction3Vtbl;
}
impl ComPtr<ICorDebugFunction3> {
    #[inline]
    pub unsafe fn GetActiveReJitRequestILCode(&self, ppReJitedILCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetActiveReJitRequestILCode)(self.as_raw(), ppReJitedILCode)
    }
}

// ---- ICorDebugFunction4 : IUnknown
#[repr(C)]
pub struct ICorDebugFunction4Vtbl {
    pub base: IUnknownVtbl,
    pub CreateNativeBreakpoint: unsafe extern "system" fn(this: *mut c_void, ppBreakpoint: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugFunction4 {}
unsafe impl Interface for ICorDebugFunction4 {
    const IID: GUID = GUID { data1: 0x72965963, data2: 0x34fd, data3: 0x46e9, data4: [0x94, 0x34, 0xb8, 0x17, 0xfe, 0x6e, 0x7f, 0x43] };
    type Vtbl = ICorDebugFunction4Vtbl;
}
impl ComPtr<ICorDebugFunction4> {
    #[inline]
    pub unsafe fn CreateNativeBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateNativeBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugFunction5 : IUnknown
#[repr(C)]
pub struct ICorDebugFunction5Vtbl {
    pub base: IUnknownVtbl,
    pub DisableOptimizations: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub AreOptimizationsDisabled: unsafe extern "system" fn(this: *mut c_void, pOptimizationsDisabled: *mut i32) -> HRESULT,
}
pub enum ICorDebugFunction5 {}
unsafe impl Interface for ICorDebugFunction5 {
    const IID: GUID = GUID { data1: 0x9D4DAB7B, data2: 0x3401, data3: 0x4F37, data4: [0xBD, 0x08, 0xCA, 0x09, 0xF3, 0xFD, 0xF1, 0x0F] };
    type Vtbl = ICorDebugFunction5Vtbl;
}
impl ComPtr<ICorDebugFunction5> {
    #[inline]
    pub unsafe fn DisableOptimizations(&self) -> HRESULT {
        (self.vtbl().DisableOptimizations)(self.as_raw())
    }
    #[inline]
    pub unsafe fn AreOptimizationsDisabled(&self, pOptimizationsDisabled: *mut i32) -> HRESULT {
        (self.vtbl().AreOptimizationsDisabled)(self.as_raw(), pOptimizationsDisabled)
    }
}

// ---- ICorDebugCode : IUnknown
#[repr(C)]
pub struct ICorDebugCodeVtbl {
    pub base: IUnknownVtbl,
    pub IsIL: unsafe extern "system" fn(this: *mut c_void, pbIL: *mut i32) -> HRESULT,
    pub GetFunction: unsafe extern "system" fn(this: *mut c_void, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetAddress: unsafe extern "system" fn(this: *mut c_void, pStart: *mut CORDB_ADDRESS) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pcBytes: *mut u32) -> HRESULT,
    pub CreateBreakpoint: unsafe extern "system" fn(this: *mut c_void, offset: u32, ppBreakpoint: *mut *mut c_void) -> HRESULT,
    pub GetCode: unsafe extern "system" fn(this: *mut c_void, startOffset: u32, endOffset: u32, cBufferAlloc: u32, buffer: *mut u8, pcBufferSize: *mut u32) -> HRESULT,
    pub GetVersionNumber: unsafe extern "system" fn(this: *mut c_void, nVersion: *mut u32) -> HRESULT,
    pub GetILToNativeMapping: unsafe extern "system" fn(this: *mut c_void, cMap: u32, pcMap: *mut u32, map: *mut COR_DEBUG_IL_TO_NATIVE_MAP) -> HRESULT,
    pub GetEnCRemapSequencePoints: unsafe extern "system" fn(this: *mut c_void, cMap: u32, pcMap: *mut u32, offsets: *mut u32) -> HRESULT,
}
pub enum ICorDebugCode {}
unsafe impl Interface for ICorDebugCode {
    const IID: GUID = GUID { data1: 0xCC7BCAF4, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugCodeVtbl;
}
impl ComPtr<ICorDebugCode> {
    #[inline]
    pub unsafe fn IsIL(&self, pbIL: *mut i32) -> HRESULT {
        (self.vtbl().IsIL)(self.as_raw(), pbIL)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunction)(self.as_raw(), ppFunction)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pStart: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetAddress)(self.as_raw(), pStart)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pcBytes: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pcBytes)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, offset: u32, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateBreakpoint)(self.as_raw(), offset, ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetCode(&self, startOffset: u32, endOffset: u32, cBufferAlloc: u32, buffer: *mut u8, pcBufferSize: *mut u32) -> HRESULT {
        (self.vtbl().GetCode)(self.as_raw(), startOffset, endOffset, cBufferAlloc, buffer, pcBufferSize)
    }
    #[inline]
    pub unsafe fn GetVersionNumber(&self, nVersion: *mut u32) -> HRESULT {
        (self.vtbl().GetVersionNumber)(self.as_raw(), nVersion)
    }
    #[inline]
    pub unsafe fn GetILToNativeMapping(&self, cMap: u32, pcMap: *mut u32, map: *mut COR_DEBUG_IL_TO_NATIVE_MAP) -> HRESULT {
        (self.vtbl().GetILToNativeMapping)(self.as_raw(), cMap, pcMap, map)
    }
    #[inline]
    pub unsafe fn GetEnCRemapSequencePoints(&self, cMap: u32, pcMap: *mut u32, offsets: *mut u32) -> HRESULT {
        (self.vtbl().GetEnCRemapSequencePoints)(self.as_raw(), cMap, pcMap, offsets)
    }
}

// ---- ICorDebugCode2 : IUnknown
#[repr(C)]
pub struct ICorDebugCode2Vtbl {
    pub base: IUnknownVtbl,
    pub GetCodeChunks: unsafe extern "system" fn(this: *mut c_void, cbufSize: u32, pcnumChunks: *mut u32, chunks: *mut CodeChunkInfo) -> HRESULT,
    pub GetCompilerFlags: unsafe extern "system" fn(this: *mut c_void, pdwFlags: *mut u32) -> HRESULT,
}
pub enum ICorDebugCode2 {}
unsafe impl Interface for ICorDebugCode2 {
    const IID: GUID = GUID { data1: 0x5F696509, data2: 0x452F, data3: 0x4436, data4: [0xA3, 0xFE, 0x4D, 0x11, 0xFE, 0x7E, 0x23, 0x47] };
    type Vtbl = ICorDebugCode2Vtbl;
}
impl ComPtr<ICorDebugCode2> {
    #[inline]
    pub unsafe fn GetCodeChunks(&self, cbufSize: u32, pcnumChunks: *mut u32, chunks: *mut CodeChunkInfo) -> HRESULT {
        (self.vtbl().GetCodeChunks)(self.as_raw(), cbufSize, pcnumChunks, chunks)
    }
    #[inline]
    pub unsafe fn GetCompilerFlags(&self, pdwFlags: *mut u32) -> HRESULT {
        (self.vtbl().GetCompilerFlags)(self.as_raw(), pdwFlags)
    }
}

// ---- ICorDebugCode3 : IUnknown
#[repr(C)]
pub struct ICorDebugCode3Vtbl {
    pub base: IUnknownVtbl,
    pub GetReturnValueLiveOffset: unsafe extern "system" fn(this: *mut c_void, ILoffset: u32, bufferSize: u32, pFetched: *mut u32, pOffsets: *mut u32) -> HRESULT,
}
pub enum ICorDebugCode3 {}
unsafe impl Interface for ICorDebugCode3 {
    const IID: GUID = GUID { data1: 0xD13D3E88, data2: 0xE1F2, data3: 0x4020, data4: [0xAA, 0x1D, 0x3D, 0x16, 0x2D, 0xCB, 0xE9, 0x66] };
    type Vtbl = ICorDebugCode3Vtbl;
}
impl ComPtr<ICorDebugCode3> {
    #[inline]
    pub unsafe fn GetReturnValueLiveOffset(&self, ILoffset: u32, bufferSize: u32, pFetched: *mut u32, pOffsets: *mut u32) -> HRESULT {
        (self.vtbl().GetReturnValueLiveOffset)(self.as_raw(), ILoffset, bufferSize, pFetched, pOffsets)
    }
}

// ---- ICorDebugCode4 : IUnknown
#[repr(C)]
pub struct ICorDebugCode4Vtbl {
    pub base: IUnknownVtbl,
    pub EnumerateVariableHomes: unsafe extern "system" fn(this: *mut c_void, ppEnum: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugCode4 {}
unsafe impl Interface for ICorDebugCode4 {
    const IID: GUID = GUID { data1: 0x18221fa4, data2: 0x20cb, data3: 0x40fa, data4: [0xb1, 0x9d, 0x9f, 0x91, 0xc4, 0xfa, 0x8c, 0x14] };
    type Vtbl = ICorDebugCode4Vtbl;
}
impl ComPtr<ICorDebugCode4> {
    #[inline]
    pub unsafe fn EnumerateVariableHomes(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateVariableHomes)(self.as_raw(), ppEnum)
    }
}

// ---- ICorDebugILCode : IUnknown
#[repr(C)]
pub struct ICorDebugILCodeVtbl {
    pub base: IUnknownVtbl,
    pub GetEHClauses: unsafe extern "system" fn(this: *mut c_void, cClauses: u32, pcClauses: *mut u32, clauses: *mut CorDebugEHClause) -> HRESULT,
}
pub enum ICorDebugILCode {}
unsafe impl Interface for ICorDebugILCode {
    const IID: GUID = GUID { data1: 0x598D46C2, data2: 0xC877, data3: 0x42A7, data4: [0x89, 0xD2, 0x3D, 0x0C, 0x7F, 0x1C, 0x12, 0x64] };
    type Vtbl = ICorDebugILCodeVtbl;
}
impl ComPtr<ICorDebugILCode> {
    #[inline]
    pub unsafe fn GetEHClauses(&self, cClauses: u32, pcClauses: *mut u32, clauses: *mut CorDebugEHClause) -> HRESULT {
        (self.vtbl().GetEHClauses)(self.as_raw(), cClauses, pcClauses, clauses)
    }
}

// ---- ICorDebugILCode2 : IUnknown
#[repr(C)]
pub struct ICorDebugILCode2Vtbl {
    pub base: IUnknownVtbl,
    pub GetLocalVarSigToken: unsafe extern "system" fn(this: *mut c_void, pmdSig: *mut mdSignature) -> HRESULT,
    pub GetInstrumentedILMap: unsafe extern "system" fn(this: *mut c_void, cMap: u32, pcMap: *mut u32, map: *mut COR_IL_MAP) -> HRESULT,
}
pub enum ICorDebugILCode2 {}
unsafe impl Interface for ICorDebugILCode2 {
    const IID: GUID = GUID { data1: 0x46586093, data2: 0xD3F5, data3: 0x4DB6, data4: [0xAC, 0xDB, 0x95, 0x5B, 0xCE, 0x22, 0x8C, 0x15] };
    type Vtbl = ICorDebugILCode2Vtbl;
}
impl ComPtr<ICorDebugILCode2> {
    #[inline]
    pub unsafe fn GetLocalVarSigToken(&self, pmdSig: *mut mdSignature) -> HRESULT {
        (self.vtbl().GetLocalVarSigToken)(self.as_raw(), pmdSig)
    }
    #[inline]
    pub unsafe fn GetInstrumentedILMap(&self, cMap: u32, pcMap: *mut u32, map: *mut COR_IL_MAP) -> HRESULT {
        (self.vtbl().GetInstrumentedILMap)(self.as_raw(), cMap, pcMap, map)
    }
}

// ---- ICorDebugClass : IUnknown
#[repr(C)]
pub struct ICorDebugClassVtbl {
    pub base: IUnknownVtbl,
    pub GetModule: unsafe extern "system" fn(this: *mut c_void, pModule: *mut *mut c_void) -> HRESULT,
    pub GetToken: unsafe extern "system" fn(this: *mut c_void, pTypeDef: *mut mdTypeDef) -> HRESULT,
    pub GetStaticFieldValue: unsafe extern "system" fn(this: *mut c_void, fieldDef: mdFieldDef, pFrame: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugClass {}
unsafe impl Interface for ICorDebugClass {
    const IID: GUID = GUID { data1: 0xCC7BCAF5, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugClassVtbl;
}
impl ComPtr<ICorDebugClass> {
    #[inline]
    pub unsafe fn GetModule(&self, pModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModule)(self.as_raw(), pModule)
    }
    #[inline]
    pub unsafe fn GetToken(&self, pTypeDef: *mut mdTypeDef) -> HRESULT {
        (self.vtbl().GetToken)(self.as_raw(), pTypeDef)
    }
    #[inline]
    pub unsafe fn GetStaticFieldValue(&self, fieldDef: mdFieldDef, pFrame: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetStaticFieldValue)(self.as_raw(), fieldDef, pFrame, ppValue)
    }
}

// ---- ICorDebugClass2 : IUnknown
#[repr(C)]
pub struct ICorDebugClass2Vtbl {
    pub base: IUnknownVtbl,
    pub GetParameterizedType: unsafe extern "system" fn(this: *mut c_void, elementType: CorElementType, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT,
    pub SetJMCStatus: unsafe extern "system" fn(this: *mut c_void, bIsJustMyCode: i32) -> HRESULT,
}
pub enum ICorDebugClass2 {}
unsafe impl Interface for ICorDebugClass2 {
    const IID: GUID = GUID { data1: 0xB008EA8D, data2: 0x7AB1, data3: 0x43f7, data4: [0xBB, 0x20, 0xFB, 0xB5, 0xA0, 0x40, 0x38, 0xAE] };
    type Vtbl = ICorDebugClass2Vtbl;
}
impl ComPtr<ICorDebugClass2> {
    #[inline]
    pub unsafe fn GetParameterizedType(&self, elementType: CorElementType, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetParameterizedType)(self.as_raw(), elementType, nTypeArgs, ppTypeArgs, ppType)
    }
    #[inline]
    pub unsafe fn SetJMCStatus(&self, bIsJustMyCode: i32) -> HRESULT {
        (self.vtbl().SetJMCStatus)(self.as_raw(), bIsJustMyCode)
    }
}

// ---- ICorDebugEval : IUnknown
#[repr(C)]
pub struct ICorDebugEvalVtbl {
    pub base: IUnknownVtbl,
    pub CallFunction: unsafe extern "system" fn(this: *mut c_void, pFunction: *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT,
    pub NewObject: unsafe extern "system" fn(this: *mut c_void, pConstructor: *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT,
    pub NewObjectNoConstructor: unsafe extern "system" fn(this: *mut c_void, pClass: *mut c_void) -> HRESULT,
    pub NewString: unsafe extern "system" fn(this: *mut c_void, string: *const u16) -> HRESULT,
    pub NewArray: unsafe extern "system" fn(this: *mut c_void, elementType: CorElementType, pElementClass: *mut c_void, rank: u32, dims: *mut u32, lowBounds: *mut u32) -> HRESULT,
    pub IsActive: unsafe extern "system" fn(this: *mut c_void, pbActive: *mut i32) -> HRESULT,
    pub Abort: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub GetResult: unsafe extern "system" fn(this: *mut c_void, ppResult: *mut *mut c_void) -> HRESULT,
    pub GetThread: unsafe extern "system" fn(this: *mut c_void, ppThread: *mut *mut c_void) -> HRESULT,
    pub CreateValue: unsafe extern "system" fn(this: *mut c_void, elementType: CorElementType, pElementClass: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugEval {}
unsafe impl Interface for ICorDebugEval {
    const IID: GUID = GUID { data1: 0xCC7BCAF6, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugEvalVtbl;
}
impl ComPtr<ICorDebugEval> {
    #[inline]
    pub unsafe fn CallFunction(&self, pFunction: *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CallFunction)(self.as_raw(), pFunction, nArgs, ppArgs)
    }
    #[inline]
    pub unsafe fn NewObject(&self, pConstructor: *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT {
        (self.vtbl().NewObject)(self.as_raw(), pConstructor, nArgs, ppArgs)
    }
    #[inline]
    pub unsafe fn NewObjectNoConstructor(&self, pClass: *mut c_void) -> HRESULT {
        (self.vtbl().NewObjectNoConstructor)(self.as_raw(), pClass)
    }
    #[inline]
    pub unsafe fn NewString(&self, string: *const u16) -> HRESULT {
        (self.vtbl().NewString)(self.as_raw(), string)
    }
    #[inline]
    pub unsafe fn NewArray(&self, elementType: CorElementType, pElementClass: *mut c_void, rank: u32, dims: *mut u32, lowBounds: *mut u32) -> HRESULT {
        (self.vtbl().NewArray)(self.as_raw(), elementType, pElementClass, rank, dims, lowBounds)
    }
    #[inline]
    pub unsafe fn IsActive(&self, pbActive: *mut i32) -> HRESULT {
        (self.vtbl().IsActive)(self.as_raw(), pbActive)
    }
    #[inline]
    pub unsafe fn Abort(&self) -> HRESULT {
        (self.vtbl().Abort)(self.as_raw())
    }
    #[inline]
    pub unsafe fn GetResult(&self, ppResult: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetResult)(self.as_raw(), ppResult)
    }
    #[inline]
    pub unsafe fn GetThread(&self, ppThread: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetThread)(self.as_raw(), ppThread)
    }
    #[inline]
    pub unsafe fn CreateValue(&self, elementType: CorElementType, pElementClass: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateValue)(self.as_raw(), elementType, pElementClass, ppValue)
    }
}

// ---- ICorDebugEval2 : IUnknown
#[repr(C)]
pub struct ICorDebugEval2Vtbl {
    pub base: IUnknownVtbl,
    pub CallParameterizedFunction: unsafe extern "system" fn(this: *mut c_void, pFunction: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT,
    pub CreateValueForType: unsafe extern "system" fn(this: *mut c_void, pType: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
    pub NewParameterizedObject: unsafe extern "system" fn(this: *mut c_void, pConstructor: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT,
    pub NewParameterizedObjectNoConstructor: unsafe extern "system" fn(this: *mut c_void, pClass: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void) -> HRESULT,
    pub NewParameterizedArray: unsafe extern "system" fn(this: *mut c_void, pElementType: *mut c_void, rank: u32, dims: *mut u32, lowBounds: *mut u32) -> HRESULT,
    pub NewStringWithLength: unsafe extern "system" fn(this: *mut c_void, string: *const u16, uiLength: u32) -> HRESULT,
    pub RudeAbort: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
}
pub enum ICorDebugEval2 {}
unsafe impl Interface for ICorDebugEval2 {
    const IID: GUID = GUID { data1: 0xFB0D9CE7, data2: 0xBE66, data3: 0x4683, data4: [0x9D, 0x32, 0xA4, 0x2A, 0x04, 0xE2, 0xFD, 0x91] };
    type Vtbl = ICorDebugEval2Vtbl;
}
impl ComPtr<ICorDebugEval2> {
    #[inline]
    pub unsafe fn CallParameterizedFunction(&self, pFunction: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CallParameterizedFunction)(self.as_raw(), pFunction, nTypeArgs, ppTypeArgs, nArgs, ppArgs)
    }
    #[inline]
    pub unsafe fn CreateValueForType(&self, pType: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateValueForType)(self.as_raw(), pType, ppValue)
    }
    #[inline]
    pub unsafe fn NewParameterizedObject(&self, pConstructor: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void, nArgs: u32, ppArgs: *mut *mut c_void) -> HRESULT {
        (self.vtbl().NewParameterizedObject)(self.as_raw(), pConstructor, nTypeArgs, ppTypeArgs, nArgs, ppArgs)
    }
    #[inline]
    pub unsafe fn NewParameterizedObjectNoConstructor(&self, pClass: *mut c_void, nTypeArgs: u32, ppTypeArgs: *mut *mut c_void) -> HRESULT {
        (self.vtbl().NewParameterizedObjectNoConstructor)(self.as_raw(), pClass, nTypeArgs, ppTypeArgs)
    }
    #[inline]
    pub unsafe fn NewParameterizedArray(&self, pElementType: *mut c_void, rank: u32, dims: *mut u32, lowBounds: *mut u32) -> HRESULT {
        (self.vtbl().NewParameterizedArray)(self.as_raw(), pElementType, rank, dims, lowBounds)
    }
    #[inline]
    pub unsafe fn NewStringWithLength(&self, string: *const u16, uiLength: u32) -> HRESULT {
        (self.vtbl().NewStringWithLength)(self.as_raw(), string, uiLength)
    }
    #[inline]
    pub unsafe fn RudeAbort(&self) -> HRESULT {
        (self.vtbl().RudeAbort)(self.as_raw())
    }
}

// ---- ICorDebugValue : IUnknown
#[repr(C)]
pub struct ICorDebugValueVtbl {
    pub base: IUnknownVtbl,
    pub GetType: unsafe extern "system" fn(this: *mut c_void, pType: *mut CorElementType) -> HRESULT,
    pub GetSize: unsafe extern "system" fn(this: *mut c_void, pSize: *mut u32) -> HRESULT,
    pub GetAddress: unsafe extern "system" fn(this: *mut c_void, pAddress: *mut CORDB_ADDRESS) -> HRESULT,
    pub CreateBreakpoint: unsafe extern "system" fn(this: *mut c_void, ppBreakpoint: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugValue {}
unsafe impl Interface for ICorDebugValue {
    const IID: GUID = GUID { data1: 0xCC7BCAF7, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugValueVtbl;
}
impl ComPtr<ICorDebugValue> {
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugValue2 : IUnknown
#[repr(C)]
pub struct ICorDebugValue2Vtbl {
    pub base: IUnknownVtbl,
    pub GetExactType: unsafe extern "system" fn(this: *mut c_void, ppType: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugValue2 {}
unsafe impl Interface for ICorDebugValue2 {
    const IID: GUID = GUID { data1: 0x5E0B54E7, data2: 0xD88A, data3: 0x4626, data4: [0x94, 0x20, 0xA6, 0x91, 0xE0, 0xA7, 0x8B, 0x49] };
    type Vtbl = ICorDebugValue2Vtbl;
}
impl ComPtr<ICorDebugValue2> {
    #[inline]
    pub unsafe fn GetExactType(&self, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetExactType)(self.as_raw(), ppType)
    }
}

// ---- ICorDebugValue3 : IUnknown
#[repr(C)]
pub struct ICorDebugValue3Vtbl {
    pub base: IUnknownVtbl,
    pub GetSize64: unsafe extern "system" fn(this: *mut c_void, pSize: *mut u64) -> HRESULT,
}
pub enum ICorDebugValue3 {}
unsafe impl Interface for ICorDebugValue3 {
    const IID: GUID = GUID { data1: 0x565005FC, data2: 0x0F8A, data3: 0x4F3E, data4: [0x9E, 0xDB, 0x83, 0x10, 0x2B, 0x15, 0x65, 0x95] };
    type Vtbl = ICorDebugValue3Vtbl;
}
impl ComPtr<ICorDebugValue3> {
    #[inline]
    pub unsafe fn GetSize64(&self, pSize: *mut u64) -> HRESULT {
        (self.vtbl().GetSize64)(self.as_raw(), pSize)
    }
}

// ---- ICorDebugGenericValue : ICorDebugValue
#[repr(C)]
pub struct ICorDebugGenericValueVtbl {
    pub base: ICorDebugValueVtbl,
    pub GetValue: unsafe extern "system" fn(this: *mut c_void, pTo: *mut c_void) -> HRESULT,
    pub SetValue: unsafe extern "system" fn(this: *mut c_void, pFrom: *mut c_void) -> HRESULT,
}
pub enum ICorDebugGenericValue {}
unsafe impl Interface for ICorDebugGenericValue {
    const IID: GUID = GUID { data1: 0xCC7BCAF8, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugGenericValueVtbl;
}
impl ComPtr<ICorDebugGenericValue> {
    #[inline]
    pub unsafe fn GetValue(&self, pTo: *mut c_void) -> HRESULT {
        (self.vtbl().GetValue)(self.as_raw(), pTo)
    }
    #[inline]
    pub unsafe fn SetValue(&self, pFrom: *mut c_void) -> HRESULT {
        (self.vtbl().SetValue)(self.as_raw(), pFrom)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugReferenceValue : ICorDebugValue
#[repr(C)]
pub struct ICorDebugReferenceValueVtbl {
    pub base: ICorDebugValueVtbl,
    pub IsNull: unsafe extern "system" fn(this: *mut c_void, pbNull: *mut i32) -> HRESULT,
    pub GetValue: unsafe extern "system" fn(this: *mut c_void, pValue: *mut CORDB_ADDRESS) -> HRESULT,
    pub SetValue: unsafe extern "system" fn(this: *mut c_void, value: CORDB_ADDRESS) -> HRESULT,
    pub Dereference: unsafe extern "system" fn(this: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
    pub DereferenceStrong: unsafe extern "system" fn(this: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugReferenceValue {}
unsafe impl Interface for ICorDebugReferenceValue {
    const IID: GUID = GUID { data1: 0xCC7BCAF9, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugReferenceValueVtbl;
}
impl ComPtr<ICorDebugReferenceValue> {
    #[inline]
    pub unsafe fn IsNull(&self, pbNull: *mut i32) -> HRESULT {
        (self.vtbl().IsNull)(self.as_raw(), pbNull)
    }
    #[inline]
    pub unsafe fn GetValue(&self, pValue: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetValue)(self.as_raw(), pValue)
    }
    #[inline]
    pub unsafe fn SetValue(&self, value: CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().SetValue)(self.as_raw(), value)
    }
    #[inline]
    pub unsafe fn Dereference(&self, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().Dereference)(self.as_raw(), ppValue)
    }
    #[inline]
    pub unsafe fn DereferenceStrong(&self, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().DereferenceStrong)(self.as_raw(), ppValue)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugHeapValue : ICorDebugValue
#[repr(C)]
pub struct ICorDebugHeapValueVtbl {
    pub base: ICorDebugValueVtbl,
    pub IsValid: unsafe extern "system" fn(this: *mut c_void, pbValid: *mut i32) -> HRESULT,
    pub CreateRelocBreakpoint: unsafe extern "system" fn(this: *mut c_void, ppBreakpoint: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugHeapValue {}
unsafe impl Interface for ICorDebugHeapValue {
    const IID: GUID = GUID { data1: 0xCC7BCAFA, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugHeapValueVtbl;
}
impl ComPtr<ICorDebugHeapValue> {
    #[inline]
    pub unsafe fn IsValid(&self, pbValid: *mut i32) -> HRESULT {
        (self.vtbl().IsValid)(self.as_raw(), pbValid)
    }
    #[inline]
    pub unsafe fn CreateRelocBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateRelocBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugHeapValue2 : IUnknown
#[repr(C)]
pub struct ICorDebugHeapValue2Vtbl {
    pub base: IUnknownVtbl,
    pub CreateHandle: unsafe extern "system" fn(this: *mut c_void, type_: CorDebugHandleType, ppHandle: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugHeapValue2 {}
unsafe impl Interface for ICorDebugHeapValue2 {
    const IID: GUID = GUID { data1: 0xE3AC4D6C, data2: 0x9CB7, data3: 0x43e6, data4: [0x96, 0xCC, 0xB2, 0x15, 0x40, 0xE5, 0x08, 0x3C] };
    type Vtbl = ICorDebugHeapValue2Vtbl;
}
impl ComPtr<ICorDebugHeapValue2> {
    #[inline]
    pub unsafe fn CreateHandle(&self, type_: CorDebugHandleType, ppHandle: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreateHandle)(self.as_raw(), type_, ppHandle)
    }
}

// ---- ICorDebugHeapValue3 : IUnknown
#[repr(C)]
pub struct ICorDebugHeapValue3Vtbl {
    pub base: IUnknownVtbl,
    pub GetThreadOwningMonitorLock: unsafe extern "system" fn(this: *mut c_void, ppThread: *mut *mut c_void, pAcquisitionCount: *mut u32) -> HRESULT,
    pub GetMonitorEventWaitList: unsafe extern "system" fn(this: *mut c_void, ppThreadEnum: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugHeapValue3 {}
unsafe impl Interface for ICorDebugHeapValue3 {
    const IID: GUID = GUID { data1: 0xA69ACAD8, data2: 0x2374, data3: 0x46e9, data4: [0x9F, 0xF8, 0xB1, 0xF1, 0x41, 0x20, 0xD2, 0x96] };
    type Vtbl = ICorDebugHeapValue3Vtbl;
}
impl ComPtr<ICorDebugHeapValue3> {
    #[inline]
    pub unsafe fn GetThreadOwningMonitorLock(&self, ppThread: *mut *mut c_void, pAcquisitionCount: *mut u32) -> HRESULT {
        (self.vtbl().GetThreadOwningMonitorLock)(self.as_raw(), ppThread, pAcquisitionCount)
    }
    #[inline]
    pub unsafe fn GetMonitorEventWaitList(&self, ppThreadEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetMonitorEventWaitList)(self.as_raw(), ppThreadEnum)
    }
}

// ---- ICorDebugHeapValue4 : IUnknown
#[repr(C)]
pub struct ICorDebugHeapValue4Vtbl {
    pub base: IUnknownVtbl,
    pub CreatePinnedHandle: unsafe extern "system" fn(this: *mut c_void, ppHandle: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugHeapValue4 {}
unsafe impl Interface for ICorDebugHeapValue4 {
    const IID: GUID = GUID { data1: 0xB35DD495, data2: 0xA555, data3: 0x463B, data4: [0x9B, 0xE9, 0xC5, 0x53, 0x38, 0x48, 0x6B, 0xB8] };
    type Vtbl = ICorDebugHeapValue4Vtbl;
}
impl ComPtr<ICorDebugHeapValue4> {
    #[inline]
    pub unsafe fn CreatePinnedHandle(&self, ppHandle: *mut *mut c_void) -> HRESULT {
        (self.vtbl().CreatePinnedHandle)(self.as_raw(), ppHandle)
    }
}

// ---- ICorDebugObjectValue : ICorDebugValue
#[repr(C)]
pub struct ICorDebugObjectValueVtbl {
    pub base: ICorDebugValueVtbl,
    pub GetClass: unsafe extern "system" fn(this: *mut c_void, ppClass: *mut *mut c_void) -> HRESULT,
    pub GetFieldValue: unsafe extern "system" fn(this: *mut c_void, pClass: *mut c_void, fieldDef: mdFieldDef, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetVirtualMethod: unsafe extern "system" fn(this: *mut c_void, memberRef: mdMemberRef, ppFunction: *mut *mut c_void) -> HRESULT,
    pub GetContext: unsafe extern "system" fn(this: *mut c_void, ppContext: *mut *mut c_void) -> HRESULT,
    pub IsValueClass: unsafe extern "system" fn(this: *mut c_void, pbIsValueClass: *mut i32) -> HRESULT,
    pub GetManagedCopy: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
    pub SetFromManagedCopy: unsafe extern "system" fn(this: *mut c_void, pObject: *mut c_void) -> HRESULT,
}
pub enum ICorDebugObjectValue {}
unsafe impl Interface for ICorDebugObjectValue {
    const IID: GUID = GUID { data1: 0x18AD3D6E, data2: 0xB7D2, data3: 0x11d2, data4: [0xBD, 0x04, 0x00, 0x00, 0xF8, 0x08, 0x49, 0xBD] };
    type Vtbl = ICorDebugObjectValueVtbl;
}
impl ComPtr<ICorDebugObjectValue> {
    #[inline]
    pub unsafe fn GetClass(&self, ppClass: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetClass)(self.as_raw(), ppClass)
    }
    #[inline]
    pub unsafe fn GetFieldValue(&self, pClass: *mut c_void, fieldDef: mdFieldDef, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFieldValue)(self.as_raw(), pClass, fieldDef, ppValue)
    }
    #[inline]
    pub unsafe fn GetVirtualMethod(&self, memberRef: mdMemberRef, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetVirtualMethod)(self.as_raw(), memberRef, ppFunction)
    }
    #[inline]
    pub unsafe fn GetContext(&self, ppContext: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetContext)(self.as_raw(), ppContext)
    }
    #[inline]
    pub unsafe fn IsValueClass(&self, pbIsValueClass: *mut i32) -> HRESULT {
        (self.vtbl().IsValueClass)(self.as_raw(), pbIsValueClass)
    }
    #[inline]
    pub unsafe fn GetManagedCopy(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetManagedCopy)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn SetFromManagedCopy(&self, pObject: *mut c_void) -> HRESULT {
        (self.vtbl().SetFromManagedCopy)(self.as_raw(), pObject)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugObjectValue2 : IUnknown
#[repr(C)]
pub struct ICorDebugObjectValue2Vtbl {
    pub base: IUnknownVtbl,
    pub GetVirtualMethodAndType: unsafe extern "system" fn(this: *mut c_void, memberRef: mdMemberRef, ppFunction: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugObjectValue2 {}
unsafe impl Interface for ICorDebugObjectValue2 {
    const IID: GUID = GUID { data1: 0x49E4A320, data2: 0x4A9B, data3: 0x4eca, data4: [0xB1, 0x05, 0x22, 0x9F, 0xB7, 0xD5, 0x00, 0x9F] };
    type Vtbl = ICorDebugObjectValue2Vtbl;
}
impl ComPtr<ICorDebugObjectValue2> {
    #[inline]
    pub unsafe fn GetVirtualMethodAndType(&self, memberRef: mdMemberRef, ppFunction: *mut *mut c_void, ppType: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetVirtualMethodAndType)(self.as_raw(), memberRef, ppFunction, ppType)
    }
}

// ---- ICorDebugDelegateObjectValue : IUnknown
#[repr(C)]
pub struct ICorDebugDelegateObjectValueVtbl {
    pub base: IUnknownVtbl,
    pub GetTarget: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
    pub GetFunction: unsafe extern "system" fn(this: *mut c_void, ppFunction: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugDelegateObjectValue {}
unsafe impl Interface for ICorDebugDelegateObjectValue {
    const IID: GUID = GUID { data1: 0x3AF70CC7, data2: 0x6047, data3: 0x47F6, data4: [0xA5, 0xC5, 0x09, 0x0A, 0x1A, 0x62, 0x26, 0x38] };
    type Vtbl = ICorDebugDelegateObjectValueVtbl;
}
impl ComPtr<ICorDebugDelegateObjectValue> {
    #[inline]
    pub unsafe fn GetTarget(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetTarget)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn GetFunction(&self, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFunction)(self.as_raw(), ppFunction)
    }
}

// ---- ICorDebugBoxValue : ICorDebugHeapValue
#[repr(C)]
pub struct ICorDebugBoxValueVtbl {
    pub base: ICorDebugHeapValueVtbl,
    pub GetObject: unsafe extern "system" fn(this: *mut c_void, ppObject: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugBoxValue {}
unsafe impl Interface for ICorDebugBoxValue {
    const IID: GUID = GUID { data1: 0xCC7BCAFC, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugBoxValueVtbl;
}
impl ComPtr<ICorDebugBoxValue> {
    #[inline]
    pub unsafe fn GetObject(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetObject)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn IsValid(&self, pbValid: *mut i32) -> HRESULT {
        (self.vtbl().base.IsValid)(self.as_raw(), pbValid)
    }
    #[inline]
    pub unsafe fn CreateRelocBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateRelocBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugStringValue : ICorDebugHeapValue
#[repr(C)]
pub struct ICorDebugStringValueVtbl {
    pub base: ICorDebugHeapValueVtbl,
    pub GetLength: unsafe extern "system" fn(this: *mut c_void, pcchString: *mut u32) -> HRESULT,
    pub GetString: unsafe extern "system" fn(this: *mut c_void, cchString: u32, pcchString: *mut u32, szString: *mut u16) -> HRESULT,
}
pub enum ICorDebugStringValue {}
unsafe impl Interface for ICorDebugStringValue {
    const IID: GUID = GUID { data1: 0xCC7BCAFD, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugStringValueVtbl;
}
impl ComPtr<ICorDebugStringValue> {
    #[inline]
    pub unsafe fn GetLength(&self, pcchString: *mut u32) -> HRESULT {
        (self.vtbl().GetLength)(self.as_raw(), pcchString)
    }
    #[inline]
    pub unsafe fn GetString(&self, cchString: u32, pcchString: *mut u32, szString: *mut u16) -> HRESULT {
        (self.vtbl().GetString)(self.as_raw(), cchString, pcchString, szString)
    }
    #[inline]
    pub unsafe fn IsValid(&self, pbValid: *mut i32) -> HRESULT {
        (self.vtbl().base.IsValid)(self.as_raw(), pbValid)
    }
    #[inline]
    pub unsafe fn CreateRelocBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateRelocBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugArrayValue : ICorDebugHeapValue
#[repr(C)]
pub struct ICorDebugArrayValueVtbl {
    pub base: ICorDebugHeapValueVtbl,
    pub GetElementType: unsafe extern "system" fn(this: *mut c_void, pType: *mut CorElementType) -> HRESULT,
    pub GetRank: unsafe extern "system" fn(this: *mut c_void, pnRank: *mut u32) -> HRESULT,
    pub GetCount: unsafe extern "system" fn(this: *mut c_void, pnCount: *mut u32) -> HRESULT,
    pub GetDimensions: unsafe extern "system" fn(this: *mut c_void, cdim: u32, dims: *mut u32) -> HRESULT,
    pub HasBaseIndicies: unsafe extern "system" fn(this: *mut c_void, pbHasBaseIndicies: *mut i32) -> HRESULT,
    pub GetBaseIndicies: unsafe extern "system" fn(this: *mut c_void, cdim: u32, indices: *mut u32) -> HRESULT,
    pub GetElement: unsafe extern "system" fn(this: *mut c_void, cdim: u32, indices: *mut u32, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetElementAtPosition: unsafe extern "system" fn(this: *mut c_void, nPosition: u32, ppValue: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugArrayValue {}
unsafe impl Interface for ICorDebugArrayValue {
    const IID: GUID = GUID { data1: 0x0405B0DF, data2: 0xA660, data3: 0x11d2, data4: [0xBD, 0x02, 0x00, 0x00, 0xF8, 0x08, 0x49, 0xBD] };
    type Vtbl = ICorDebugArrayValueVtbl;
}
impl ComPtr<ICorDebugArrayValue> {
    #[inline]
    pub unsafe fn GetElementType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().GetElementType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetRank(&self, pnRank: *mut u32) -> HRESULT {
        (self.vtbl().GetRank)(self.as_raw(), pnRank)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pnCount: *mut u32) -> HRESULT {
        (self.vtbl().GetCount)(self.as_raw(), pnCount)
    }
    #[inline]
    pub unsafe fn GetDimensions(&self, cdim: u32, dims: *mut u32) -> HRESULT {
        (self.vtbl().GetDimensions)(self.as_raw(), cdim, dims)
    }
    #[inline]
    pub unsafe fn HasBaseIndicies(&self, pbHasBaseIndicies: *mut i32) -> HRESULT {
        (self.vtbl().HasBaseIndicies)(self.as_raw(), pbHasBaseIndicies)
    }
    #[inline]
    pub unsafe fn GetBaseIndicies(&self, cdim: u32, indices: *mut u32) -> HRESULT {
        (self.vtbl().GetBaseIndicies)(self.as_raw(), cdim, indices)
    }
    #[inline]
    pub unsafe fn GetElement(&self, cdim: u32, indices: *mut u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetElement)(self.as_raw(), cdim, indices, ppValue)
    }
    #[inline]
    pub unsafe fn GetElementAtPosition(&self, nPosition: u32, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetElementAtPosition)(self.as_raw(), nPosition, ppValue)
    }
    #[inline]
    pub unsafe fn IsValid(&self, pbValid: *mut i32) -> HRESULT {
        (self.vtbl().base.IsValid)(self.as_raw(), pbValid)
    }
    #[inline]
    pub unsafe fn CreateRelocBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.CreateRelocBreakpoint)(self.as_raw(), ppBreakpoint)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugVariableHome : IUnknown
#[repr(C)]
pub struct ICorDebugVariableHomeVtbl {
    pub base: IUnknownVtbl,
    pub GetCode: unsafe extern "system" fn(this: *mut c_void, ppCode: *mut *mut c_void) -> HRESULT,
    pub GetSlotIndex: unsafe extern "system" fn(this: *mut c_void, pSlotIndex: *mut u32) -> HRESULT,
    pub GetArgumentIndex: unsafe extern "system" fn(this: *mut c_void, pArgumentIndex: *mut u32) -> HRESULT,
    pub GetLiveRange: unsafe extern "system" fn(this: *mut c_void, pStartOffset: *mut u32, pEndOffset: *mut u32) -> HRESULT,
    pub GetLocationType: unsafe extern "system" fn(this: *mut c_void, pLocationType: *mut VariableLocationType) -> HRESULT,
    pub GetRegister: unsafe extern "system" fn(this: *mut c_void, pRegister: *mut CorDebugRegister) -> HRESULT,
    pub GetOffset: unsafe extern "system" fn(this: *mut c_void, pOffset: *mut i32) -> HRESULT,
}
pub enum ICorDebugVariableHome {}
unsafe impl Interface for ICorDebugVariableHome {
    const IID: GUID = GUID { data1: 0x50847b8d, data2: 0xf43f, data3: 0x41b0, data4: [0x92, 0x4c, 0x63, 0x83, 0xa5, 0xf2, 0x27, 0x8b] };
    type Vtbl = ICorDebugVariableHomeVtbl;
}
impl ComPtr<ICorDebugVariableHome> {
    #[inline]
    pub unsafe fn GetCode(&self, ppCode: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCode)(self.as_raw(), ppCode)
    }
    #[inline]
    pub unsafe fn GetSlotIndex(&self, pSlotIndex: *mut u32) -> HRESULT {
        (self.vtbl().GetSlotIndex)(self.as_raw(), pSlotIndex)
    }
    #[inline]
    pub unsafe fn GetArgumentIndex(&self, pArgumentIndex: *mut u32) -> HRESULT {
        (self.vtbl().GetArgumentIndex)(self.as_raw(), pArgumentIndex)
    }
    #[inline]
    pub unsafe fn GetLiveRange(&self, pStartOffset: *mut u32, pEndOffset: *mut u32) -> HRESULT {
        (self.vtbl().GetLiveRange)(self.as_raw(), pStartOffset, pEndOffset)
    }
    #[inline]
    pub unsafe fn GetLocationType(&self, pLocationType: *mut VariableLocationType) -> HRESULT {
        (self.vtbl().GetLocationType)(self.as_raw(), pLocationType)
    }
    #[inline]
    pub unsafe fn GetRegister(&self, pRegister: *mut CorDebugRegister) -> HRESULT {
        (self.vtbl().GetRegister)(self.as_raw(), pRegister)
    }
    #[inline]
    pub unsafe fn GetOffset(&self, pOffset: *mut i32) -> HRESULT {
        (self.vtbl().GetOffset)(self.as_raw(), pOffset)
    }
}

// ---- ICorDebugHandleValue : ICorDebugReferenceValue
#[repr(C)]
pub struct ICorDebugHandleValueVtbl {
    pub base: ICorDebugReferenceValueVtbl,
    pub GetHandleType: unsafe extern "system" fn(this: *mut c_void, pType: *mut CorDebugHandleType) -> HRESULT,
    pub Dispose: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
}
pub enum ICorDebugHandleValue {}
unsafe impl Interface for ICorDebugHandleValue {
    const IID: GUID = GUID { data1: 0x029596E8, data2: 0x276B, data3: 0x46a1, data4: [0x98, 0x21, 0x73, 0x2E, 0x96, 0xBB, 0xB0, 0x0B] };
    type Vtbl = ICorDebugHandleValueVtbl;
}
impl ComPtr<ICorDebugHandleValue> {
    #[inline]
    pub unsafe fn GetHandleType(&self, pType: *mut CorDebugHandleType) -> HRESULT {
        (self.vtbl().GetHandleType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn Dispose(&self) -> HRESULT {
        (self.vtbl().Dispose)(self.as_raw())
    }
    #[inline]
    pub unsafe fn IsNull(&self, pbNull: *mut i32) -> HRESULT {
        (self.vtbl().base.IsNull)(self.as_raw(), pbNull)
    }
    #[inline]
    pub unsafe fn GetValue(&self, pValue: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.GetValue)(self.as_raw(), pValue)
    }
    #[inline]
    pub unsafe fn SetValue(&self, value: CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.SetValue)(self.as_raw(), value)
    }
    #[inline]
    pub unsafe fn Dereference(&self, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Dereference)(self.as_raw(), ppValue)
    }
    #[inline]
    pub unsafe fn DereferenceStrong(&self, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.DereferenceStrong)(self.as_raw(), ppValue)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugContext : ICorDebugObjectValue
#[repr(C)]
pub struct ICorDebugContextVtbl {
    pub base: ICorDebugObjectValueVtbl,
}
pub enum ICorDebugContext {}
unsafe impl Interface for ICorDebugContext {
    const IID: GUID = GUID { data1: 0xCC7BCB00, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugContextVtbl;
}
impl ComPtr<ICorDebugContext> {
    #[inline]
    pub unsafe fn GetClass(&self, ppClass: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetClass)(self.as_raw(), ppClass)
    }
    #[inline]
    pub unsafe fn GetFieldValue(&self, pClass: *mut c_void, fieldDef: mdFieldDef, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetFieldValue)(self.as_raw(), pClass, fieldDef, ppValue)
    }
    #[inline]
    pub unsafe fn GetVirtualMethod(&self, memberRef: mdMemberRef, ppFunction: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetVirtualMethod)(self.as_raw(), memberRef, ppFunction)
    }
    #[inline]
    pub unsafe fn GetContext(&self, ppContext: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetContext)(self.as_raw(), ppContext)
    }
    #[inline]
    pub unsafe fn IsValueClass(&self, pbIsValueClass: *mut i32) -> HRESULT {
        (self.vtbl().base.IsValueClass)(self.as_raw(), pbIsValueClass)
    }
    #[inline]
    pub unsafe fn GetManagedCopy(&self, ppObject: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.GetManagedCopy)(self.as_raw(), ppObject)
    }
    #[inline]
    pub unsafe fn SetFromManagedCopy(&self, pObject: *mut c_void) -> HRESULT {
        (self.vtbl().base.SetFromManagedCopy)(self.as_raw(), pObject)
    }
    #[inline]
    pub unsafe fn GetType(&self, pType: *mut CorElementType) -> HRESULT {
        (self.vtbl().base.base.GetType)(self.as_raw(), pType)
    }
    #[inline]
    pub unsafe fn GetSize(&self, pSize: *mut u32) -> HRESULT {
        (self.vtbl().base.base.GetSize)(self.as_raw(), pSize)
    }
    #[inline]
    pub unsafe fn GetAddress(&self, pAddress: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().base.base.GetAddress)(self.as_raw(), pAddress)
    }
    #[inline]
    pub unsafe fn CreateBreakpoint(&self, ppBreakpoint: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.base.CreateBreakpoint)(self.as_raw(), ppBreakpoint)
    }
}

// ---- ICorDebugComObjectValue : IUnknown
#[repr(C)]
pub struct ICorDebugComObjectValueVtbl {
    pub base: IUnknownVtbl,
    pub GetCachedInterfaceTypes: unsafe extern "system" fn(this: *mut c_void, bIInspectableOnly: i32, ppInterfacesEnum: *mut *mut c_void) -> HRESULT,
    pub GetCachedInterfacePointers: unsafe extern "system" fn(this: *mut c_void, bIInspectableOnly: i32, celt: u32, pcEltFetched: *mut u32, ptrs: *mut CORDB_ADDRESS) -> HRESULT,
}
pub enum ICorDebugComObjectValue {}
unsafe impl Interface for ICorDebugComObjectValue {
    const IID: GUID = GUID { data1: 0x5F69C5E5, data2: 0x3E12, data3: 0x42DF, data4: [0xB3, 0x71, 0xF9, 0xD7, 0x61, 0xD6, 0xEE, 0x24] };
    type Vtbl = ICorDebugComObjectValueVtbl;
}
impl ComPtr<ICorDebugComObjectValue> {
    #[inline]
    pub unsafe fn GetCachedInterfaceTypes(&self, bIInspectableOnly: i32, ppInterfacesEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetCachedInterfaceTypes)(self.as_raw(), bIInspectableOnly, ppInterfacesEnum)
    }
    #[inline]
    pub unsafe fn GetCachedInterfacePointers(&self, bIInspectableOnly: i32, celt: u32, pcEltFetched: *mut u32, ptrs: *mut CORDB_ADDRESS) -> HRESULT {
        (self.vtbl().GetCachedInterfacePointers)(self.as_raw(), bIInspectableOnly, celt, pcEltFetched, ptrs)
    }
}

// ---- ICorDebugEnum : IUnknown
#[repr(C)]
pub struct ICorDebugEnumVtbl {
    pub base: IUnknownVtbl,
    pub Skip: unsafe extern "system" fn(this: *mut c_void, celt: u32) -> HRESULT,
    pub Reset: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub Clone: unsafe extern "system" fn(this: *mut c_void, ppEnum: *mut *mut c_void) -> HRESULT,
    pub GetCount: unsafe extern "system" fn(this: *mut c_void, pcelt: *mut u32) -> HRESULT,
}
pub enum ICorDebugEnum {}
unsafe impl Interface for ICorDebugEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB01, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugEnumVtbl;
}
impl ComPtr<ICorDebugEnum> {
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugObjectEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugObjectEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, objects: *mut CORDB_ADDRESS, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugObjectEnum {}
unsafe impl Interface for ICorDebugObjectEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB02, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugObjectEnumVtbl;
}
impl ComPtr<ICorDebugObjectEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, objects: *mut CORDB_ADDRESS, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, objects, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugBreakpointEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugBreakpointEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, breakpoints: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugBreakpointEnum {}
unsafe impl Interface for ICorDebugBreakpointEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB03, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugBreakpointEnumVtbl;
}
impl ComPtr<ICorDebugBreakpointEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, breakpoints: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, breakpoints, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugStepperEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugStepperEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, steppers: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugStepperEnum {}
unsafe impl Interface for ICorDebugStepperEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB04, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugStepperEnumVtbl;
}
impl ComPtr<ICorDebugStepperEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, steppers: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, steppers, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugProcessEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugProcessEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, processes: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugProcessEnum {}
unsafe impl Interface for ICorDebugProcessEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB05, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugProcessEnumVtbl;
}
impl ComPtr<ICorDebugProcessEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, processes: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, processes, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugThreadEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugThreadEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, threads: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugThreadEnum {}
unsafe impl Interface for ICorDebugThreadEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB06, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugThreadEnumVtbl;
}
impl ComPtr<ICorDebugThreadEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, threads: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, threads, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugFrameEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugFrameEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, frames: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugFrameEnum {}
unsafe impl Interface for ICorDebugFrameEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB07, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugFrameEnumVtbl;
}
impl ComPtr<ICorDebugFrameEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, frames: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, frames, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugChainEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugChainEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, chains: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugChainEnum {}
unsafe impl Interface for ICorDebugChainEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB08, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugChainEnumVtbl;
}
impl ComPtr<ICorDebugChainEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, chains: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, chains, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugModuleEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugModuleEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, modules: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugModuleEnum {}
unsafe impl Interface for ICorDebugModuleEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB09, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugModuleEnumVtbl;
}
impl ComPtr<ICorDebugModuleEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, modules: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, modules, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugValueEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugValueEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugValueEnum {}
unsafe impl Interface for ICorDebugValueEnum {
    const IID: GUID = GUID { data1: 0xCC7BCB0A, data2: 0x8A68, data3: 0x11d2, data4: [0x98, 0x3C, 0x00, 0x00, 0xF8, 0x08, 0x34, 0x2D] };
    type Vtbl = ICorDebugValueEnumVtbl;
}
impl ComPtr<ICorDebugValueEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugVariableHomeEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugVariableHomeEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, homes: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugVariableHomeEnum {}
unsafe impl Interface for ICorDebugVariableHomeEnum {
    const IID: GUID = GUID { data1: 0xe76b7a57, data2: 0x4f7a, data3: 0x4309, data4: [0x85, 0xa7, 0x5d, 0x91, 0x8c, 0x3d, 0xea, 0xf7] };
    type Vtbl = ICorDebugVariableHomeEnumVtbl;
}
impl ComPtr<ICorDebugVariableHomeEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, homes: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, homes, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugCodeEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugCodeEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugCodeEnum {}
unsafe impl Interface for ICorDebugCodeEnum {
    const IID: GUID = GUID { data1: 0x55E96461, data2: 0x9645, data3: 0x45e4, data4: [0xA2, 0xFF, 0x03, 0x67, 0x87, 0x7A, 0xBC, 0xDE] };
    type Vtbl = ICorDebugCodeEnumVtbl;
}
impl ComPtr<ICorDebugCodeEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugTypeEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugTypeEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugTypeEnum {}
unsafe impl Interface for ICorDebugTypeEnum {
    const IID: GUID = GUID { data1: 0x10F27499, data2: 0x9DF2, data3: 0x43ce, data4: [0x83, 0x33, 0xA3, 0x21, 0xD7, 0xC9, 0x9C, 0xB4] };
    type Vtbl = ICorDebugTypeEnumVtbl;
}
impl ComPtr<ICorDebugTypeEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugType : IUnknown
#[repr(C)]
pub struct ICorDebugTypeVtbl {
    pub base: IUnknownVtbl,
    pub GetType: unsafe extern "system" fn(this: *mut c_void, ty: *mut CorElementType) -> HRESULT,
    pub GetClass: unsafe extern "system" fn(this: *mut c_void, ppClass: *mut *mut c_void) -> HRESULT,
    pub EnumerateTypeParameters: unsafe extern "system" fn(this: *mut c_void, ppTyParEnum: *mut *mut c_void) -> HRESULT,
    pub GetFirstTypeParameter: unsafe extern "system" fn(this: *mut c_void, value: *mut *mut c_void) -> HRESULT,
    pub GetBase: unsafe extern "system" fn(this: *mut c_void, pBase: *mut *mut c_void) -> HRESULT,
    pub GetStaticFieldValue: unsafe extern "system" fn(this: *mut c_void, fieldDef: mdFieldDef, pFrame: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT,
    pub GetRank: unsafe extern "system" fn(this: *mut c_void, pnRank: *mut u32) -> HRESULT,
}
pub enum ICorDebugType {}
unsafe impl Interface for ICorDebugType {
    const IID: GUID = GUID { data1: 0xD613F0BB, data2: 0xACE1, data3: 0x4c19, data4: [0xBD, 0x72, 0xE4, 0xC0, 0x8D, 0x5D, 0xA7, 0xF5] };
    type Vtbl = ICorDebugTypeVtbl;
}
impl ComPtr<ICorDebugType> {
    #[inline]
    pub unsafe fn GetType(&self, ty: *mut CorElementType) -> HRESULT {
        (self.vtbl().GetType)(self.as_raw(), ty)
    }
    #[inline]
    pub unsafe fn GetClass(&self, ppClass: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetClass)(self.as_raw(), ppClass)
    }
    #[inline]
    pub unsafe fn EnumerateTypeParameters(&self, ppTyParEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateTypeParameters)(self.as_raw(), ppTyParEnum)
    }
    #[inline]
    pub unsafe fn GetFirstTypeParameter(&self, value: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetFirstTypeParameter)(self.as_raw(), value)
    }
    #[inline]
    pub unsafe fn GetBase(&self, pBase: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetBase)(self.as_raw(), pBase)
    }
    #[inline]
    pub unsafe fn GetStaticFieldValue(&self, fieldDef: mdFieldDef, pFrame: *mut c_void, ppValue: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetStaticFieldValue)(self.as_raw(), fieldDef, pFrame, ppValue)
    }
    #[inline]
    pub unsafe fn GetRank(&self, pnRank: *mut u32) -> HRESULT {
        (self.vtbl().GetRank)(self.as_raw(), pnRank)
    }
}

// ---- ICorDebugType2 : IUnknown
#[repr(C)]
pub struct ICorDebugType2Vtbl {
    pub base: IUnknownVtbl,
    pub GetTypeID: unsafe extern "system" fn(this: *mut c_void, id: *mut COR_TYPEID) -> HRESULT,
}
pub enum ICorDebugType2 {}
unsafe impl Interface for ICorDebugType2 {
    const IID: GUID = GUID { data1: 0xe6e91d79, data2: 0x693d, data3: 0x48bc, data4: [0xb4, 0x17, 0x82, 0x84, 0xb4, 0xf1, 0x0f, 0xb5] };
    type Vtbl = ICorDebugType2Vtbl;
}
impl ComPtr<ICorDebugType2> {
    #[inline]
    pub unsafe fn GetTypeID(&self, id: *mut COR_TYPEID) -> HRESULT {
        (self.vtbl().GetTypeID)(self.as_raw(), id)
    }
}

// ---- ICorDebugErrorInfoEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugErrorInfoEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, errors: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugErrorInfoEnum {}
unsafe impl Interface for ICorDebugErrorInfoEnum {
    const IID: GUID = GUID { data1: 0xF0E18809, data2: 0x72B5, data3: 0x11d2, data4: [0x97, 0x6F, 0x00, 0xA0, 0xC9, 0xB4, 0xD5, 0x0C] };
    type Vtbl = ICorDebugErrorInfoEnumVtbl;
}
impl ComPtr<ICorDebugErrorInfoEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, errors: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, errors, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugAppDomainEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugAppDomainEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugAppDomainEnum {}
unsafe impl Interface for ICorDebugAppDomainEnum {
    const IID: GUID = GUID { data1: 0x63ca1b24, data2: 0x4359, data3: 0x4883, data4: [0xbd, 0x57, 0x13, 0xf8, 0x15, 0xf5, 0x87, 0x44] };
    type Vtbl = ICorDebugAppDomainEnumVtbl;
}
impl ComPtr<ICorDebugAppDomainEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugAssemblyEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugAssemblyEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugAssemblyEnum {}
unsafe impl Interface for ICorDebugAssemblyEnum {
    const IID: GUID = GUID { data1: 0x4a2a1ec9, data2: 0x85ec, data3: 0x4bfb, data4: [0x9f, 0x15, 0xa8, 0x9f, 0xdf, 0xe0, 0xfe, 0x83] };
    type Vtbl = ICorDebugAssemblyEnumVtbl;
}
impl ComPtr<ICorDebugAssemblyEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut *mut c_void, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugBlockingObjectEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugBlockingObjectEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut CorDebugBlockingObject, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugBlockingObjectEnum {}
unsafe impl Interface for ICorDebugBlockingObjectEnum {
    const IID: GUID = GUID { data1: 0x976A6278, data2: 0x134A, data3: 0x4a81, data4: [0x81, 0xA3, 0x8F, 0x27, 0x79, 0x43, 0xF4, 0xC3] };
    type Vtbl = ICorDebugBlockingObjectEnumVtbl;
}
impl ComPtr<ICorDebugBlockingObjectEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut CorDebugBlockingObject, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugMDA : IUnknown
#[repr(C)]
pub struct ICorDebugMDAVtbl {
    pub base: IUnknownVtbl,
    pub GetName: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetDescription: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetXML: unsafe extern "system" fn(this: *mut c_void, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT,
    pub GetFlags: unsafe extern "system" fn(this: *mut c_void, pFlags: *mut CorDebugMDAFlags) -> HRESULT,
    pub GetOSThreadId: unsafe extern "system" fn(this: *mut c_void, pOsTid: *mut u32) -> HRESULT,
}
pub enum ICorDebugMDA {}
unsafe impl Interface for ICorDebugMDA {
    const IID: GUID = GUID { data1: 0xCC726F2F, data2: 0x1DB7, data3: 0x459b, data4: [0xB0, 0xEC, 0x05, 0xF0, 0x1D, 0x84, 0x1B, 0x42] };
    type Vtbl = ICorDebugMDAVtbl;
}
impl ComPtr<ICorDebugMDA> {
    #[inline]
    pub unsafe fn GetName(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetName)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetDescription(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetDescription)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetXML(&self, cchName: u32, pcchName: *mut u32, szName: *mut u16) -> HRESULT {
        (self.vtbl().GetXML)(self.as_raw(), cchName, pcchName, szName)
    }
    #[inline]
    pub unsafe fn GetFlags(&self, pFlags: *mut CorDebugMDAFlags) -> HRESULT {
        (self.vtbl().GetFlags)(self.as_raw(), pFlags)
    }
    #[inline]
    pub unsafe fn GetOSThreadId(&self, pOsTid: *mut u32) -> HRESULT {
        (self.vtbl().GetOSThreadId)(self.as_raw(), pOsTid)
    }
}

// ---- ICorDebugEditAndContinueErrorInfo : IUnknown
#[repr(C)]
pub struct ICorDebugEditAndContinueErrorInfoVtbl {
    pub base: IUnknownVtbl,
    pub GetModule: unsafe extern "system" fn(this: *mut c_void, ppModule: *mut *mut c_void) -> HRESULT,
    pub GetToken: unsafe extern "system" fn(this: *mut c_void, pToken: *mut mdToken) -> HRESULT,
    pub GetErrorCode: unsafe extern "system" fn(this: *mut c_void, pHr: *mut HRESULT) -> HRESULT,
    pub GetString: unsafe extern "system" fn(this: *mut c_void, cchString: u32, pcchString: *mut u32, szString: *mut u16) -> HRESULT,
}
pub enum ICorDebugEditAndContinueErrorInfo {}
unsafe impl Interface for ICorDebugEditAndContinueErrorInfo {
    const IID: GUID = GUID { data1: 0x8D600D41, data2: 0xF4F6, data3: 0x4cb3, data4: [0xB7, 0xEC, 0x7B, 0xD1, 0x64, 0x94, 0x40, 0x36] };
    type Vtbl = ICorDebugEditAndContinueErrorInfoVtbl;
}
impl ComPtr<ICorDebugEditAndContinueErrorInfo> {
    #[inline]
    pub unsafe fn GetModule(&self, ppModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetModule)(self.as_raw(), ppModule)
    }
    #[inline]
    pub unsafe fn GetToken(&self, pToken: *mut mdToken) -> HRESULT {
        (self.vtbl().GetToken)(self.as_raw(), pToken)
    }
    #[inline]
    pub unsafe fn GetErrorCode(&self, pHr: *mut HRESULT) -> HRESULT {
        (self.vtbl().GetErrorCode)(self.as_raw(), pHr)
    }
    #[inline]
    pub unsafe fn GetString(&self, cchString: u32, pcchString: *mut u32, szString: *mut u16) -> HRESULT {
        (self.vtbl().GetString)(self.as_raw(), cchString, pcchString, szString)
    }
}

// ---- ICorDebugEditAndContinueSnapshot : IUnknown
#[repr(C)]
pub struct ICorDebugEditAndContinueSnapshotVtbl {
    pub base: IUnknownVtbl,
    pub CopyMetaData: unsafe extern "system" fn(this: *mut c_void, pIStream: *mut c_void, pMvid: *mut GUID) -> HRESULT,
    pub GetMvid: unsafe extern "system" fn(this: *mut c_void, pMvid: *mut GUID) -> HRESULT,
    pub GetRoDataRVA: unsafe extern "system" fn(this: *mut c_void, pRoDataRVA: *mut u32) -> HRESULT,
    pub GetRwDataRVA: unsafe extern "system" fn(this: *mut c_void, pRwDataRVA: *mut u32) -> HRESULT,
    pub SetPEBytes: unsafe extern "system" fn(this: *mut c_void, pIStream: *mut c_void) -> HRESULT,
    pub SetILMap: unsafe extern "system" fn(this: *mut c_void, mdFunction: mdToken, cMapSize: u32, map: *mut COR_IL_MAP) -> HRESULT,
    pub SetPESymbolBytes: unsafe extern "system" fn(this: *mut c_void, pIStream: *mut c_void) -> HRESULT,
}
pub enum ICorDebugEditAndContinueSnapshot {}
unsafe impl Interface for ICorDebugEditAndContinueSnapshot {
    const IID: GUID = GUID { data1: 0x6DC3FA01, data2: 0xD7CB, data3: 0x11d2, data4: [0x8A, 0x95, 0x00, 0x80, 0xC7, 0x92, 0xE5, 0xD8] };
    type Vtbl = ICorDebugEditAndContinueSnapshotVtbl;
}
impl ComPtr<ICorDebugEditAndContinueSnapshot> {
    #[inline]
    pub unsafe fn CopyMetaData(&self, pIStream: *mut c_void, pMvid: *mut GUID) -> HRESULT {
        (self.vtbl().CopyMetaData)(self.as_raw(), pIStream, pMvid)
    }
    #[inline]
    pub unsafe fn GetMvid(&self, pMvid: *mut GUID) -> HRESULT {
        (self.vtbl().GetMvid)(self.as_raw(), pMvid)
    }
    #[inline]
    pub unsafe fn GetRoDataRVA(&self, pRoDataRVA: *mut u32) -> HRESULT {
        (self.vtbl().GetRoDataRVA)(self.as_raw(), pRoDataRVA)
    }
    #[inline]
    pub unsafe fn GetRwDataRVA(&self, pRwDataRVA: *mut u32) -> HRESULT {
        (self.vtbl().GetRwDataRVA)(self.as_raw(), pRwDataRVA)
    }
    #[inline]
    pub unsafe fn SetPEBytes(&self, pIStream: *mut c_void) -> HRESULT {
        (self.vtbl().SetPEBytes)(self.as_raw(), pIStream)
    }
    #[inline]
    pub unsafe fn SetILMap(&self, mdFunction: mdToken, cMapSize: u32, map: *mut COR_IL_MAP) -> HRESULT {
        (self.vtbl().SetILMap)(self.as_raw(), mdFunction, cMapSize, map)
    }
    #[inline]
    pub unsafe fn SetPESymbolBytes(&self, pIStream: *mut c_void) -> HRESULT {
        (self.vtbl().SetPESymbolBytes)(self.as_raw(), pIStream)
    }
}

// ---- ICorDebugExceptionObjectCallStackEnum : ICorDebugEnum
#[repr(C)]
pub struct ICorDebugExceptionObjectCallStackEnumVtbl {
    pub base: ICorDebugEnumVtbl,
    pub Next: unsafe extern "system" fn(this: *mut c_void, celt: u32, values: *mut CorDebugExceptionObjectStackFrame, pceltFetched: *mut u32) -> HRESULT,
}
pub enum ICorDebugExceptionObjectCallStackEnum {}
unsafe impl Interface for ICorDebugExceptionObjectCallStackEnum {
    const IID: GUID = GUID { data1: 0xED775530, data2: 0x4DC4, data3: 0x41F7, data4: [0x86, 0xD0, 0x9E, 0x2D, 0xEF, 0x7D, 0xFC, 0x66] };
    type Vtbl = ICorDebugExceptionObjectCallStackEnumVtbl;
}
impl ComPtr<ICorDebugExceptionObjectCallStackEnum> {
    #[inline]
    pub unsafe fn Next(&self, celt: u32, values: *mut CorDebugExceptionObjectStackFrame, pceltFetched: *mut u32) -> HRESULT {
        (self.vtbl().Next)(self.as_raw(), celt, values, pceltFetched)
    }
    #[inline]
    pub unsafe fn Skip(&self, celt: u32) -> HRESULT {
        (self.vtbl().base.Skip)(self.as_raw(), celt)
    }
    #[inline]
    pub unsafe fn Reset(&self) -> HRESULT {
        (self.vtbl().base.Reset)(self.as_raw())
    }
    #[inline]
    pub unsafe fn Clone(&self, ppEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().base.Clone)(self.as_raw(), ppEnum)
    }
    #[inline]
    pub unsafe fn GetCount(&self, pcelt: *mut u32) -> HRESULT {
        (self.vtbl().base.GetCount)(self.as_raw(), pcelt)
    }
}

// ---- ICorDebugExceptionObjectValue : IUnknown
#[repr(C)]
pub struct ICorDebugExceptionObjectValueVtbl {
    pub base: IUnknownVtbl,
    pub EnumerateExceptionCallStack: unsafe extern "system" fn(this: *mut c_void, ppCallStackEnum: *mut *mut c_void) -> HRESULT,
}
pub enum ICorDebugExceptionObjectValue {}
unsafe impl Interface for ICorDebugExceptionObjectValue {
    const IID: GUID = GUID { data1: 0xAE4CA65D, data2: 0x59DD, data3: 0x42A2, data4: [0x83, 0xA5, 0x57, 0xE8, 0xA0, 0x8D, 0x87, 0x19] };
    type Vtbl = ICorDebugExceptionObjectValueVtbl;
}
impl ComPtr<ICorDebugExceptionObjectValue> {
    #[inline]
    pub unsafe fn EnumerateExceptionCallStack(&self, ppCallStackEnum: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateExceptionCallStack)(self.as_raw(), ppCallStackEnum)
    }
}

// ---- ICorDebugExceptionObjectValue2 : IUnknown
#[repr(C)]
pub struct ICorDebugExceptionObjectValue2Vtbl {
    pub base: IUnknownVtbl,
    pub ForceCatchHandlerFoundEvents: unsafe extern "system" fn(this: *mut c_void, enableEvents: i32) -> HRESULT,
}
pub enum ICorDebugExceptionObjectValue2 {}
unsafe impl Interface for ICorDebugExceptionObjectValue2 {
    const IID: GUID = GUID { data1: 0xe3b2f332, data2: 0xcc46, data3: 0x4f1e, data4: [0xab, 0x4e, 0x54, 0x00, 0xe3, 0x32, 0x19, 0x5e] };
    type Vtbl = ICorDebugExceptionObjectValue2Vtbl;
}
impl ComPtr<ICorDebugExceptionObjectValue2> {
    #[inline]
    pub unsafe fn ForceCatchHandlerFoundEvents(&self, enableEvents: i32) -> HRESULT {
        (self.vtbl().ForceCatchHandlerFoundEvents)(self.as_raw(), enableEvents)
    }
}
