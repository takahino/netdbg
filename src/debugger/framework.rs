//! .NET Framework（デスクトップ CLR）の ICorDebug。
//!
//! .NET（CoreCLR）は dbgshim から ICorDebug を受け取るが、.NET Framework は Windows に入っている
//! mscoree.dll の `ICLRMetaHost` から受け取る。こちらは .NET のランタイムも dbgshim も要らないので、
//! x86 の .NET が入っていない機械でも .NET Framework のアプリをデバッグできる（docs/plan.html 段階 2）。
//!
//! 対象は、デバッガーが `ICorDebug::CreateProcess` で起動する（.NET のように「起動してから通知を待つ」形ではない）。

use crate::com::{ComPtr, Interface, GUID, HRESULT};
use crate::cordebug::{ICorDebug, ICorDebugProcess};
use crate::metahost::{ICLRMetaHost, ICLRRuntimeInfo};
use crate::win32::*;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

// metahost.idl では cpp_quote の中にあり、生成されない。値は同じファイルから写した。
const CLSID_CLR_META_HOST: GUID = GUID { data1: 0x9280188D, data2: 0x0E8E, data3: 0x4867, data4: [0xB3, 0x0C, 0x7F, 0xA8, 0x38, 0x84, 0xE8, 0xDE] };
const CLSID_CLR_DEBUGGING_LEGACY: GUID = GUID { data1: 0xDF8395B5, data2: 0xA4BA, data3: 0x450B, data4: [0xA7, 0x7C, 0xA9, 0xA4, 0x77, 0x62, 0xC5, 0x20] };

/// .NET Framework 4.x のランタイムの版。4.0 から 4.8.1 まで、どれもこの名前で入っている。
const VERSION_4: &str = "v4.0.30319";

fn failed(what: &str, hr: HRESULT) -> String {
    tr!("{what} failed ({code})", what = what, code = hr)
}

/// 実行ファイルが求めるランタイムの版が、扱える .NET Framework 4 以降かを確かめる（ファイルのメタデータを読むだけで、
/// 実行はしない）。2.0〜3.5 のランタイム（v2.0.50727）は ICorDebug の受け取り方が別で、対応していない。
unsafe fn check_version(host: &ComPtr<ICLRMetaHost>, program: &Path) -> Result<(), String> {
    let mut buffer = [0u16; 64];
    let mut length = buffer.len() as u32;
    host.GetVersionFromFile(wide(&program.to_string_lossy()).as_ptr(), buffer.as_mut_ptr(), &mut length)
        .ok()
        .map_err(|hr| tr!("Cannot read the .NET Framework version that {program} needs ({code}).", program = program.display(), code = hr))?;
    let wanted = from_wide(&buffer);
    if !wanted.starts_with("v4.") {
        return Err(tr!("{program} is for the .NET Framework runtime {wanted}. Only .NET Framework 4 or later ({supported}) is supported.", program = program.display(), wanted = wanted, supported = VERSION_4));
    }
    Ok(())
}

/// `launch` の時点で、.NET Framework のアプリが求めるランタイムの版を確かめる（起動まで待たずに断る）。
pub fn check_program(program: &Path) -> Result<(), String> {
    create_debugger(Some(program)).map(|_| ())
}

/// .NET Framework 用の ICorDebug を作る。`program` を渡すと、その実行ファイルが求めるランタイムの版を確かめる。
pub fn create_debugger(program: Option<&Path>) -> Result<ComPtr<ICorDebug>, String> {
    unsafe {
        let mscoree = LoadLibraryW(wide("mscoree.dll").as_ptr());
        if mscoree.is_null() {
            return Err(tr!("Cannot load mscoree.dll (Win32 error {code}). Check that .NET Framework 4 is installed.", code = GetLastError()));
        }
        let address = GetProcAddress(mscoree, c"CLRCreateInstance".as_ptr().cast());
        if address.is_null() {
            return Err(tr!("mscoree.dll has no CLRCreateInstance. .NET Framework 4 or later is needed."));
        }
        // 宣言は metahost.h の CLRCreateInstance と一致させてある。
        type ClrCreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
        let create = std::mem::transmute::<*mut c_void, ClrCreateInstance>(address);

        let mut raw = null_mut();
        create(&CLSID_CLR_META_HOST, &ICLRMetaHost::IID, &mut raw).ok().map_err(|hr| failed("CLRCreateInstance", hr))?;
        let host = ComPtr::<ICLRMetaHost>::from_raw(raw).ok_or_else(|| tr!("CLRCreateInstance returned null"))?;

        if let Some(program) = program {
            check_version(&host, program)?;
        }

        let mut raw = null_mut();
        host.GetRuntime(wide(VERSION_4).as_ptr(), &ICLRRuntimeInfo::IID, &mut raw)
            .ok()
            .map_err(|hr| tr!("The .NET Framework 4 runtime is not found ({code}). Install .NET Framework 4.x.", code = hr))?;
        let runtime = ComPtr::<ICLRRuntimeInfo>::from_raw(raw).ok_or_else(|| tr!("ICLRMetaHost::GetRuntime returned null"))?;

        let mut raw = null_mut();
        runtime.GetInterface(&CLSID_CLR_DEBUGGING_LEGACY, &ICorDebug::IID, &mut raw).ok().map_err(|hr| failed(tr_str!("Getting ICorDebug of .NET Framework"), hr))?;
        ComPtr::<ICorDebug>::from_raw(raw).ok_or_else(|| tr!("ICLRRuntimeInfo::GetInterface returned null"))
    }
}

/// 対象をデバッガーの下で起動する。`ICorDebug::Initialize` と `SetManagedHandler` を済ませてから呼ぶこと。
/// 環境変数は、呼び出し側が `win32::with_environment` で自分の環境変数を書き換えて引き継がせる（D-015 と同じ）。
pub fn launch(cordebug: &ComPtr<ICorDebug>, command_line: &str, cwd: Option<&str>) -> Result<(ComPtr<ICorDebugProcess>, u32), String> {
    let mut command = wide(command_line);
    let cwd_wide = cwd.map(wide);
    unsafe {
        // 標準入出力は、付け替えてあるエンジンのもの（対象の出力を受けるパイプ）をそのまま渡す。
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESTDHANDLES;
        startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
        let mut information: PROCESS_INFORMATION = std::mem::zeroed();
        let mut raw = null_mut();
        let hr = cordebug.CreateProcess(
            null(),
            command.as_mut_ptr(),
            null_mut(),
            null_mut(),
            1,
            0,
            null_mut(),
            cwd_wide.as_ref().map_or(null(), |c| c.as_ptr()),
            &mut startup as *mut STARTUPINFOW as usize,
            &mut information as *mut PROCESS_INFORMATION as usize,
            0,
            &mut raw,
        );
        hr.ok().map_err(|hr| tr!("Cannot launch the target ({code}): {command}. Check program and cwd.", code = hr, command = command_line))?;
        // プロセスの終了コードは ICorDebugProcess::GetHandle のハンドルで読むので、こちらは要らない。
        CloseHandle(information.hThread);
        CloseHandle(information.hProcess);
        let process = ComPtr::<ICorDebugProcess>::from_raw(raw).ok_or_else(|| tr!("ICorDebug::CreateProcess returned null"))?;
        Ok((process, information.dwProcessId))
    }
}

/// 動いているプロセスが .NET Framework のランタイム（clr.dll）を読み込んでいるか。
pub fn is_loaded_in(process_id: u32) -> bool {
    process_modules(process_id).iter().any(|name| name.eq_ignore_ascii_case("clr.dll"))
}

/// 動いているプロセスが、.NET か .NET Framework のランタイム（coreclr.dll / clr.dll）を読み込んでいるか。
/// モジュールの一覧を読めなければ（権限など）、読み込んでいるものとみなす（アタッチを試させる）。
pub fn has_runtime(process_id: u32) -> bool {
    let modules = process_modules(process_id);
    modules.is_empty() || modules.iter().any(|name| name.eq_ignore_ascii_case("clr.dll") || name.eq_ignore_ascii_case("coreclr.dll"))
}
