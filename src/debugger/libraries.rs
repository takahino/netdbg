//! ランタイムに合う mscordbi.dll / mscordaccore.dll を探して dbgshim に渡す（`ICLRDebuggingLibraryProvider3`）。
//!
//! ふつうの .NET のアプリでは、ランタイム（coreclr.dll）の隣にある。単一ファイルで発行した自己完結のアプリは、ランタイムを
//! exe の中に持ち、この 2 つは同梱されない。そこで、ランタイムが求める版（ファイルのヘッダーの TimeDateStamp と
//! SizeOfImage）と同じものを、インストールされた共有ランタイムと NuGet のランタイムパックからも探す（D-050）。

#![allow(non_snake_case)]

use crate::com::{IUnknown, IUnknownVtbl, Interface, E_NOINTERFACE, GUID, HRESULT, S_OK};
use crate::metahost::*;
use crate::win32::CoTaskMemAlloc;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// 見つからなかった（dbgshim は、この失敗を「デバッグを始められない」として返す）。
const E_FAIL: HRESULT = HRESULT(0x8000_4005u32 as i32);

#[repr(C)]
struct Provider {
    vtable: &'static ICLRDebuggingLibraryProvider3Vtbl,
}

// プロセスに一つだけの、状態を持たないオブジェクト。参照カウントは数えない（callback.rs と同じ）。
static PROVIDER: Provider = Provider { vtable: &VTBL };

unsafe impl Sync for Provider {}

/// `RegisterForRuntimeStartup3` に渡すポインター。
pub fn library_provider() -> *mut c_void {
    &PROVIDER as *const Provider as *mut c_void
}

unsafe extern "system" fn query_interface(_this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let iid = *riid;
    if iid == IUnknown::IID || iid == ICLRDebuggingLibraryProvider3::IID {
        *out = &PROVIDER as *const Provider as *mut c_void;
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


unsafe extern "system" fn ProvideWindowsLibrary(
    _this: *mut c_void,
    file_name: *mut u16,
    runtime_module: *mut u16,
    index_type: LIBRARY_PROVIDER_INDEX_TYPE,
    timestamp: u32,
    size_of_image: u32,
    resolved: *mut *mut u16,
) -> HRESULT {
    let (file_name, runtime_module) = (crate::win32::from_wide_ptr(file_name), crate::win32::from_wide_ptr(runtime_module));
    // Identity: 求められているのは、そのファイル自身の版。Runtime: 版はランタイムのもので、ファイルの版では確かめられない。
    let wanted = (index_type == Identity).then_some((timestamp, size_of_image));
    let Some(found) = find_library(&file_name, Path::new(&runtime_module), wanted) else {
        crate::log!("library provider: {file_name} for {runtime_module} not found");
        return E_FAIL;
    };
    crate::log!("library provider: {file_name} -> {}", found.display());
    let mut text: Vec<u16> = found.as_os_str().to_string_lossy().encode_utf16().collect();
    text.push(0);
    let buffer = CoTaskMemAlloc(text.len() * 2) as *mut u16;
    if buffer.is_null() {
        return E_FAIL;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len());
    *resolved = buffer;
    S_OK
}

unsafe extern "system" fn ProvideUnixLibrary(_this: *mut c_void, _file_name: *mut u16, _runtime_module: *mut u16, _index_type: LIBRARY_PROVIDER_INDEX_TYPE, _build_id: *mut u8, _build_id_size: i32, _resolved: *mut *mut u16) -> HRESULT {
    E_FAIL
}

static VTBL: ICLRDebuggingLibraryProvider3Vtbl = ICLRDebuggingLibraryProvider3Vtbl {
    base: IUnknownVtbl { QueryInterface: query_interface, AddRef: add_ref, Release: release },
    ProvideWindowsLibrary,
    ProvideUnixLibrary,
};

/// `file_name` を探す順: ランタイムのモジュールの隣、エンジンと同じビット数のインストールされた共有ランタイム、
/// NuGet のランタイムパック。`wanted` があれば、その版（TimeDateStamp と SizeOfImage）のものだけ。
fn find_library(file_name: &str, runtime_module: &Path, wanted: Option<(u32, u32)>) -> Option<PathBuf> {
    let matches = |path: &Path| path.is_file() && wanted.is_none_or(|wanted| identity(path) == Some(wanted));
    let beside = runtime_module.parent().map(|dir| dir.join(file_name));
    if let Some(path) = beside.filter(|path| matches(path)) {
        return Some(path);
    }
    // 版で確かめられないときは、隣に無ければ探しようがない（どの版でもよいわけではない）。
    wanted?;
    candidate_directories().into_iter().map(|dir| dir.join(file_name)).find(|path| matches(path))
}

/// ファイルの先頭のヘッダーから、TimeDateStamp と SizeOfImage。
pub(crate) fn identity(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut headers = Vec::new();
    std::fs::File::open(path).ok()?.take(4096).read_to_end(&mut headers).ok()?;
    crate::debugger::target::image_identity(&headers)
}

/// 共有ランタイムとランタイムパックの、ネイティブのファイルが入ったフォルダー。
pub(crate) fn candidate_directories() -> Vec<PathBuf> {
    // エンジンと同じビット数の .NET（dotnet.exe を探すのと同じ場所）の共有ランタイム。
    let mut directories: Vec<PathBuf> = crate::debugger::target::dotnet_roots().iter().flat_map(|root| subdirectories(&root.join("shared").join("Microsoft.NETCore.App"))).collect();
    let arch = crate::debugger::target::Bits::of_this_process().name();
    // NuGet のランタイムパック（自己完結の発行に使ったもの）。
    let packages = std::env::var_os("NUGET_PACKAGES").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".nuget").join("packages")));
    if let Some(packages) = packages {
        let pack = packages.join(format!("microsoft.netcore.app.runtime.win-{arch}"));
        directories.extend(subdirectories(&pack).into_iter().map(|version| version.join("runtimes").join(format!("win-{arch}")).join("native")));
    }
    directories
}

fn subdirectories(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path).map(|entries| entries.filter_map(|entry| entry.ok()).map(|entry| entry.path()).filter(|path| path.is_dir()).collect()).unwrap_or_default()
}
