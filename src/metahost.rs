// このファイルは tools/idlgen が idl/metahost.idl から生成した。手で編集しない。
//     cargo run -p idlgen -- idl/metahost.idl src/metahost.rs
//
// メソッドは `extern "system"`: x86 では stdcall、x64 では通常の規約になる。
// インターフェイスへのポインターは `*mut c_void` で受け渡しし、呼び出し側が `ComPtr` に包む。
// 中身を知らずポインターとしてだけ扱っている型: IEnumUnknown
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]

use crate::com::{ComPtr, IUnknownVtbl, Interface, GUID, HRESULT};
use std::ffi::c_void;

pub type CallbackThreadSetFnPtr = *mut c_void;
pub type CallbackThreadUnsetFnPtr = *mut c_void;
pub type RuntimeLoadedCallbackFnPtr = *mut c_void;

pub type LIBRARY_PROVIDER_INDEX_TYPE = u32;
pub const UnknownIndex: LIBRARY_PROVIDER_INDEX_TYPE = 0x0;
pub const Identity: LIBRARY_PROVIDER_INDEX_TYPE = 0x1;
pub const Runtime: LIBRARY_PROVIDER_INDEX_TYPE = 0x2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CLR_DEBUGGING_VERSION {
    pub wStructVersion: u16,
    pub wMajor: u16,
    pub wMinor: u16,
    pub wBuild: u16,
    pub wRevision: u16,
}

pub type CLR_DEBUGGING_PROCESS_FLAGS = u32;
pub const CLR_DEBUGGING_MANAGED_EVENT_PENDING: CLR_DEBUGGING_PROCESS_FLAGS = 0x1;
pub const CLR_DEBUGGING_MANAGED_EVENT_DEBUGGER_LAUNCH: CLR_DEBUGGING_PROCESS_FLAGS = 0x2;

// ---- ICLRMetaHost : IUnknown
#[repr(C)]
pub struct ICLRMetaHostVtbl {
    pub base: IUnknownVtbl,
    pub GetRuntime: unsafe extern "system" fn(this: *mut c_void, pwzVersion: *const u16, riid: *const GUID, ppRuntime: *mut *mut c_void) -> HRESULT,
    pub GetVersionFromFile: unsafe extern "system" fn(this: *mut c_void, pwzFilePath: *const u16, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT,
    pub EnumerateInstalledRuntimes: unsafe extern "system" fn(this: *mut c_void, ppEnumerator: *mut *mut c_void) -> HRESULT,
    pub EnumerateLoadedRuntimes: unsafe extern "system" fn(this: *mut c_void, hndProcess: *mut c_void, ppEnumerator: *mut *mut c_void) -> HRESULT,
    pub RequestRuntimeLoadedNotification: unsafe extern "system" fn(this: *mut c_void, pCallbackFunction: RuntimeLoadedCallbackFnPtr) -> HRESULT,
    pub QueryLegacyV2RuntimeBinding: unsafe extern "system" fn(this: *mut c_void, riid: *const GUID, ppUnk: *mut *mut c_void) -> HRESULT,
    pub ExitProcess: unsafe extern "system" fn(this: *mut c_void, iExitCode: i32) -> HRESULT,
}
pub enum ICLRMetaHost {}
unsafe impl Interface for ICLRMetaHost {
    const IID: GUID = GUID { data1: 0xD332DB9E, data2: 0xB9B3, data3: 0x4125, data4: [0x82, 0x07, 0xA1, 0x48, 0x84, 0xF5, 0x32, 0x16] };
    type Vtbl = ICLRMetaHostVtbl;
}
impl ComPtr<ICLRMetaHost> {
    #[inline]
    pub unsafe fn GetRuntime(&self, pwzVersion: *const u16, riid: *const GUID, ppRuntime: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetRuntime)(self.as_raw(), pwzVersion, riid, ppRuntime)
    }
    #[inline]
    pub unsafe fn GetVersionFromFile(&self, pwzFilePath: *const u16, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT {
        (self.vtbl().GetVersionFromFile)(self.as_raw(), pwzFilePath, pwzBuffer, pcchBuffer)
    }
    #[inline]
    pub unsafe fn EnumerateInstalledRuntimes(&self, ppEnumerator: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateInstalledRuntimes)(self.as_raw(), ppEnumerator)
    }
    #[inline]
    pub unsafe fn EnumerateLoadedRuntimes(&self, hndProcess: *mut c_void, ppEnumerator: *mut *mut c_void) -> HRESULT {
        (self.vtbl().EnumerateLoadedRuntimes)(self.as_raw(), hndProcess, ppEnumerator)
    }
    #[inline]
    pub unsafe fn RequestRuntimeLoadedNotification(&self, pCallbackFunction: RuntimeLoadedCallbackFnPtr) -> HRESULT {
        (self.vtbl().RequestRuntimeLoadedNotification)(self.as_raw(), pCallbackFunction)
    }
    #[inline]
    pub unsafe fn QueryLegacyV2RuntimeBinding(&self, riid: *const GUID, ppUnk: *mut *mut c_void) -> HRESULT {
        (self.vtbl().QueryLegacyV2RuntimeBinding)(self.as_raw(), riid, ppUnk)
    }
    #[inline]
    pub unsafe fn ExitProcess(&self, iExitCode: i32) -> HRESULT {
        (self.vtbl().ExitProcess)(self.as_raw(), iExitCode)
    }
}

// ---- ICLRDebuggingLibraryProvider : IUnknown
#[repr(C)]
pub struct ICLRDebuggingLibraryProviderVtbl {
    pub base: IUnknownVtbl,
    pub ProvideLibrary: unsafe extern "system" fn(this: *mut c_void, pwszFileName: *mut u16, dwTimestamp: u32, dwSizeOfImage: u32, phModule: *mut *mut c_void) -> HRESULT,
}
pub enum ICLRDebuggingLibraryProvider {}
unsafe impl Interface for ICLRDebuggingLibraryProvider {
    const IID: GUID = GUID { data1: 0x3151C08D, data2: 0x4D09, data3: 0x4f9b, data4: [0x88, 0x38, 0x28, 0x80, 0xBF, 0x18, 0xFE, 0x51] };
    type Vtbl = ICLRDebuggingLibraryProviderVtbl;
}
impl ComPtr<ICLRDebuggingLibraryProvider> {
    #[inline]
    pub unsafe fn ProvideLibrary(&self, pwszFileName: *mut u16, dwTimestamp: u32, dwSizeOfImage: u32, phModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().ProvideLibrary)(self.as_raw(), pwszFileName, dwTimestamp, dwSizeOfImage, phModule)
    }
}

// ---- ICLRDebuggingLibraryProvider2 : IUnknown
#[repr(C)]
pub struct ICLRDebuggingLibraryProvider2Vtbl {
    pub base: IUnknownVtbl,
    pub ProvideLibrary2: unsafe extern "system" fn(this: *mut c_void, pwszFileName: *mut u16, dwTimestamp: u32, dwSizeOfImage: u32, ppResolvedModulePath: *mut *mut u16) -> HRESULT,
}
pub enum ICLRDebuggingLibraryProvider2 {}
unsafe impl Interface for ICLRDebuggingLibraryProvider2 {
    const IID: GUID = GUID { data1: 0xE04E2FF1, data2: 0xDCFD, data3: 0x45D5, data4: [0xBC, 0xD1, 0x16, 0xFF, 0xF2, 0xFA, 0xF7, 0xBA] };
    type Vtbl = ICLRDebuggingLibraryProvider2Vtbl;
}
impl ComPtr<ICLRDebuggingLibraryProvider2> {
    #[inline]
    pub unsafe fn ProvideLibrary2(&self, pwszFileName: *mut u16, dwTimestamp: u32, dwSizeOfImage: u32, ppResolvedModulePath: *mut *mut u16) -> HRESULT {
        (self.vtbl().ProvideLibrary2)(self.as_raw(), pwszFileName, dwTimestamp, dwSizeOfImage, ppResolvedModulePath)
    }
}

// ---- ICLRDebuggingLibraryProvider3 : IUnknown
#[repr(C)]
pub struct ICLRDebuggingLibraryProvider3Vtbl {
    pub base: IUnknownVtbl,
    pub ProvideWindowsLibrary: unsafe extern "system" fn(this: *mut c_void, pwszFileName: *mut u16, pwszRuntimeModule: *mut u16, indexType: LIBRARY_PROVIDER_INDEX_TYPE, dwTimestamp: u32, dwSizeOfImage: u32, ppResolvedModulePath: *mut *mut u16) -> HRESULT,
    pub ProvideUnixLibrary: unsafe extern "system" fn(this: *mut c_void, pwszFileName: *mut u16, pwszRuntimeModule: *mut u16, indexType: LIBRARY_PROVIDER_INDEX_TYPE, pbBuildId: *mut u8, iBuildIdSize: i32, ppResolvedModulePath: *mut *mut u16) -> HRESULT,
}
pub enum ICLRDebuggingLibraryProvider3 {}
unsafe impl Interface for ICLRDebuggingLibraryProvider3 {
    const IID: GUID = GUID { data1: 0xDE3AAB18, data2: 0x46A0, data3: 0x48B4, data4: [0xBF, 0x0D, 0x2C, 0x33, 0x6E, 0x69, 0xEA, 0x1B] };
    type Vtbl = ICLRDebuggingLibraryProvider3Vtbl;
}
impl ComPtr<ICLRDebuggingLibraryProvider3> {
    #[inline]
    pub unsafe fn ProvideWindowsLibrary(&self, pwszFileName: *mut u16, pwszRuntimeModule: *mut u16, indexType: LIBRARY_PROVIDER_INDEX_TYPE, dwTimestamp: u32, dwSizeOfImage: u32, ppResolvedModulePath: *mut *mut u16) -> HRESULT {
        (self.vtbl().ProvideWindowsLibrary)(self.as_raw(), pwszFileName, pwszRuntimeModule, indexType, dwTimestamp, dwSizeOfImage, ppResolvedModulePath)
    }
    #[inline]
    pub unsafe fn ProvideUnixLibrary(&self, pwszFileName: *mut u16, pwszRuntimeModule: *mut u16, indexType: LIBRARY_PROVIDER_INDEX_TYPE, pbBuildId: *mut u8, iBuildIdSize: i32, ppResolvedModulePath: *mut *mut u16) -> HRESULT {
        (self.vtbl().ProvideUnixLibrary)(self.as_raw(), pwszFileName, pwszRuntimeModule, indexType, pbBuildId, iBuildIdSize, ppResolvedModulePath)
    }
}

// ---- ICLRDebugging : IUnknown
#[repr(C)]
pub struct ICLRDebuggingVtbl {
    pub base: IUnknownVtbl,
    pub OpenVirtualProcess: unsafe extern "system" fn(this: *mut c_void, moduleBaseAddress: u64, pDataTarget: *mut c_void, pLibraryProvider: *mut c_void, pMaxDebuggerSupportedVersion: *mut CLR_DEBUGGING_VERSION, riidProcess: *const GUID, ppProcess: *mut *mut c_void, pVersion: *mut CLR_DEBUGGING_VERSION, pdwFlags: *mut CLR_DEBUGGING_PROCESS_FLAGS) -> HRESULT,
    pub CanUnloadNow: unsafe extern "system" fn(this: *mut c_void, hModule: *mut c_void) -> HRESULT,
}
pub enum ICLRDebugging {}
unsafe impl Interface for ICLRDebugging {
    const IID: GUID = GUID { data1: 0xD28F3C5A, data2: 0x9634, data3: 0x4206, data4: [0xA5, 0x09, 0x47, 0x75, 0x52, 0xEE, 0xFB, 0x10] };
    type Vtbl = ICLRDebuggingVtbl;
}
impl ComPtr<ICLRDebugging> {
    #[inline]
    pub unsafe fn OpenVirtualProcess(&self, moduleBaseAddress: u64, pDataTarget: *mut c_void, pLibraryProvider: *mut c_void, pMaxDebuggerSupportedVersion: *mut CLR_DEBUGGING_VERSION, riidProcess: *const GUID, ppProcess: *mut *mut c_void, pVersion: *mut CLR_DEBUGGING_VERSION, pdwFlags: *mut CLR_DEBUGGING_PROCESS_FLAGS) -> HRESULT {
        (self.vtbl().OpenVirtualProcess)(self.as_raw(), moduleBaseAddress, pDataTarget, pLibraryProvider, pMaxDebuggerSupportedVersion, riidProcess, ppProcess, pVersion, pdwFlags)
    }
    #[inline]
    pub unsafe fn CanUnloadNow(&self, hModule: *mut c_void) -> HRESULT {
        (self.vtbl().CanUnloadNow)(self.as_raw(), hModule)
    }
}

// ---- ICLRRuntimeInfo : IUnknown
#[repr(C)]
pub struct ICLRRuntimeInfoVtbl {
    pub base: IUnknownVtbl,
    pub GetVersionString: unsafe extern "system" fn(this: *mut c_void, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT,
    pub GetRuntimeDirectory: unsafe extern "system" fn(this: *mut c_void, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT,
    pub IsLoaded: unsafe extern "system" fn(this: *mut c_void, hndProcess: *mut c_void, pbLoaded: *mut i32) -> HRESULT,
    pub LoadErrorString: unsafe extern "system" fn(this: *mut c_void, iResourceID: u32, pwzBuffer: *mut u16, pcchBuffer: *mut u32, iLocaleID: i32) -> HRESULT,
    pub LoadLibrary: unsafe extern "system" fn(this: *mut c_void, pwzDllName: *const u16, phndModule: *mut *mut c_void) -> HRESULT,
    pub GetProcAddress: unsafe extern "system" fn(this: *mut c_void, pszProcName: *const u8, ppProc: *mut *mut c_void) -> HRESULT,
    pub GetInterface: unsafe extern "system" fn(this: *mut c_void, rclsid: *const GUID, riid: *const GUID, ppUnk: *mut *mut c_void) -> HRESULT,
    pub IsLoadable: unsafe extern "system" fn(this: *mut c_void, pbLoadable: *mut i32) -> HRESULT,
    pub SetDefaultStartupFlags: unsafe extern "system" fn(this: *mut c_void, dwStartupFlags: u32, pwzHostConfigFile: *const u16) -> HRESULT,
    pub GetDefaultStartupFlags: unsafe extern "system" fn(this: *mut c_void, pdwStartupFlags: *mut u32, pwzHostConfigFile: *mut u16, pcchHostConfigFile: *mut u32) -> HRESULT,
    pub BindAsLegacyV2Runtime: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    pub IsStarted: unsafe extern "system" fn(this: *mut c_void, pbStarted: *mut i32, pdwStartupFlags: *mut u32) -> HRESULT,
}
pub enum ICLRRuntimeInfo {}
unsafe impl Interface for ICLRRuntimeInfo {
    const IID: GUID = GUID { data1: 0xBD39D1D2, data2: 0xBA2F, data3: 0x486a, data4: [0x89, 0xB0, 0xB4, 0xB0, 0xCB, 0x46, 0x68, 0x91] };
    type Vtbl = ICLRRuntimeInfoVtbl;
}
impl ComPtr<ICLRRuntimeInfo> {
    #[inline]
    pub unsafe fn GetVersionString(&self, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT {
        (self.vtbl().GetVersionString)(self.as_raw(), pwzBuffer, pcchBuffer)
    }
    #[inline]
    pub unsafe fn GetRuntimeDirectory(&self, pwzBuffer: *mut u16, pcchBuffer: *mut u32) -> HRESULT {
        (self.vtbl().GetRuntimeDirectory)(self.as_raw(), pwzBuffer, pcchBuffer)
    }
    #[inline]
    pub unsafe fn IsLoaded(&self, hndProcess: *mut c_void, pbLoaded: *mut i32) -> HRESULT {
        (self.vtbl().IsLoaded)(self.as_raw(), hndProcess, pbLoaded)
    }
    #[inline]
    pub unsafe fn LoadErrorString(&self, iResourceID: u32, pwzBuffer: *mut u16, pcchBuffer: *mut u32, iLocaleID: i32) -> HRESULT {
        (self.vtbl().LoadErrorString)(self.as_raw(), iResourceID, pwzBuffer, pcchBuffer, iLocaleID)
    }
    #[inline]
    pub unsafe fn LoadLibrary(&self, pwzDllName: *const u16, phndModule: *mut *mut c_void) -> HRESULT {
        (self.vtbl().LoadLibrary)(self.as_raw(), pwzDllName, phndModule)
    }
    #[inline]
    pub unsafe fn GetProcAddress(&self, pszProcName: *const u8, ppProc: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetProcAddress)(self.as_raw(), pszProcName, ppProc)
    }
    #[inline]
    pub unsafe fn GetInterface(&self, rclsid: *const GUID, riid: *const GUID, ppUnk: *mut *mut c_void) -> HRESULT {
        (self.vtbl().GetInterface)(self.as_raw(), rclsid, riid, ppUnk)
    }
    #[inline]
    pub unsafe fn IsLoadable(&self, pbLoadable: *mut i32) -> HRESULT {
        (self.vtbl().IsLoadable)(self.as_raw(), pbLoadable)
    }
    #[inline]
    pub unsafe fn SetDefaultStartupFlags(&self, dwStartupFlags: u32, pwzHostConfigFile: *const u16) -> HRESULT {
        (self.vtbl().SetDefaultStartupFlags)(self.as_raw(), dwStartupFlags, pwzHostConfigFile)
    }
    #[inline]
    pub unsafe fn GetDefaultStartupFlags(&self, pdwStartupFlags: *mut u32, pwzHostConfigFile: *mut u16, pcchHostConfigFile: *mut u32) -> HRESULT {
        (self.vtbl().GetDefaultStartupFlags)(self.as_raw(), pdwStartupFlags, pwzHostConfigFile, pcchHostConfigFile)
    }
    #[inline]
    pub unsafe fn BindAsLegacyV2Runtime(&self) -> HRESULT {
        (self.vtbl().BindAsLegacyV2Runtime)(self.as_raw())
    }
    #[inline]
    pub unsafe fn IsStarted(&self, pbStarted: *mut i32, pdwStartupFlags: *mut u32) -> HRESULT {
        (self.vtbl().IsStarted)(self.as_raw(), pbStarted, pdwStartupFlags)
    }
}
