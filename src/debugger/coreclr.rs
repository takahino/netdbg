//! dbgshim: .NET（CoreCLR）のプロセスを起動し、ランタイムが読み込まれたら ICorDebug を受け取る。
//!
//! dbgshim.dll はエンジンの exe と同じフォルダーに置く（x86 と x64 で別のファイル）。ヘッダーには呼び出し規約の
//! 指定が無いが、x86 のバイナリは stdcall なので `extern "system"` で宣言する（docs/DECISIONS.md D-009）。

use crate::com::{ComPtr, HRESULT};
use crate::events::{post, Input};
use crate::debugger::libraries::library_provider;
use crate::win32::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

type StartupCallback = unsafe extern "system" fn(cordb: *mut c_void, parameter: *mut c_void, hr: HRESULT);

// dbgshim.dll の関数の型（dbgshim.h の宣言と一致させる。x86 は stdcall なので、引数を一つ間違えるとスタックが壊れる）。
type CreateProcessForLaunch = unsafe extern "system" fn(*mut u16, i32, *mut c_void, *const u16, *mut u32, *mut HANDLE) -> HRESULT;
type ResumeProcess = unsafe extern "system" fn(HANDLE) -> HRESULT;
type CloseResumeHandle = unsafe extern "system" fn(HANDLE) -> HRESULT;
/// RegisterForRuntimeStartup3（プロセス、アプリケーショングループ、ライブラリの提供者、通知、引数、登録）。
type RegisterForRuntimeStartup3 = unsafe extern "system" fn(u32, *const u16, *mut c_void, StartupCallback, *mut c_void, *mut *mut c_void) -> HRESULT;
type UnregisterForRuntimeStartup = unsafe extern "system" fn(*mut c_void) -> HRESULT;

pub struct Dbgshim {
    create_process_for_launch: CreateProcessForLaunch,
    resume_process: ResumeProcess,
    close_resume_handle: CloseResumeHandle,
    register_for_runtime_startup: RegisterForRuntimeStartup3,
    unregister_for_runtime_startup: UnregisterForRuntimeStartup,
}

/// 起動した対象。ランタイムの読み込みを待っている間の登録を持つ。
pub struct Launched {
    pub process_id: u32,
    registration: *mut c_void,
}

impl Launched {
    /// 起動通知を待たない対象（.NET Framework。デバッガーが直接起動する）。
    pub fn without_registration(process_id: u32) -> Launched {
        Launched { process_id, registration: null_mut() }
    }
}

/// ランタイムが読み込まれたときに dbgshim のスレッドから呼ばれる。ICorDebug の初期化はセッションのスレッドで行う。
unsafe extern "system" fn on_runtime_startup(cordb: *mut c_void, _parameter: *mut c_void, hr: HRESULT) {
    let result = match hr.ok() {
        // cordb はこの呼び出しの間だけ借りているので、自分の参照を持つ。
        Ok(()) => ComPtr::from_borrowed(cordb).ok_or(crate::com::E_POINTER),
        Err(e) => Err(e),
    };
    post(Input::RuntimeStarted(result));
}

/// 起動した対象のプロセスの終わりを見張り、終わったら `Input::TargetExited` を送る。ランタイムを読み込まないまま
/// 終わる対象（ネイティブの exe など）では、ICorDebug からは何も届かないので、これが無いとセッションが待ち続ける。
fn watch_exit(process_id: u32) {
    let process = unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        crate::log!("cannot watch pid {process_id} (Win32 error {})", unsafe { GetLastError() });
        return;
    }
    // HANDLE はスレッドをまたいで渡せない型なので、数として渡す。
    let raw = process as usize;
    std::thread::spawn(move || unsafe {
        let process = raw as HANDLE;
        WaitForSingleObject(process, INFINITE);
        let mut exit_code = 0u32;
        GetExitCodeProcess(process, &mut exit_code);
        CloseHandle(process);
        post(Input::TargetExited { process_id, exit_code });
    });
}

impl Dbgshim {
    /// エンジンの exe の隣の dbgshim.dll を読み込む。
    pub fn load() -> Result<Dbgshim, String> {
        let exe = std::env::current_exe().map_err(|e| tr!("Cannot tell where this executable is: {error}", error = e))?;
        let path = exe.with_file_name("dbgshim.dll");
        let module = unsafe { LoadLibraryW(wide(&path.to_string_lossy()).as_ptr()) };
        if module.is_null() {
            return Err(tr!(
                "Cannot load dbgshim.dll: {path} (Win32 error {code}). Put a dbgshim.dll of the engine's bitness next to the exe (build.ps1 places it).",
                path = path.display(),
                code = unsafe { GetLastError() }
            ));
        }
        let find = |name: &'static str| -> Result<*mut c_void, String> {
            let symbol = format!("{name}\0");
            let address = unsafe { GetProcAddress(module, symbol.as_ptr()) };
            if address.is_null() {
                Err(tr!("dbgshim.dll has no {name}. It may be a dbgshim.dll of a different version: {path}", name = name, path = path.display()))
            } else {
                Ok(address)
            }
        };
        // 関数ポインターへの変換。型は上の宣言（dbgshim.h の定義と一致させてある）。
        use std::mem::transmute;
        unsafe {
            Ok(Dbgshim {
                create_process_for_launch: transmute::<*mut c_void, CreateProcessForLaunch>(find("CreateProcessForLaunch")?),
                resume_process: transmute::<*mut c_void, ResumeProcess>(find("ResumeProcess")?),
                close_resume_handle: transmute::<*mut c_void, CloseResumeHandle>(find("CloseResumeHandle")?),
                // mscordbi.dll / mscordaccore.dll をランタイムの隣以外からも渡せる版（単一ファイルのアプリ。libraries.rs）。
                register_for_runtime_startup: transmute::<*mut c_void, RegisterForRuntimeStartup3>(find("RegisterForRuntimeStartup3")?),
                unregister_for_runtime_startup: transmute::<*mut c_void, UnregisterForRuntimeStartup>(find("UnregisterForRuntimeStartup")?),
            })
        }
    }

    /// 対象を止めた状態で作り、ランタイムの読み込みの通知を登録してから動かす。
    /// 通知は `Input::RuntimeStarted` として届く。
    ///
    /// `environment` は対象に足す（値が `None` なら消す）環境変数。dbgshim は環境変数のブロックを ANSI として
    /// 渡すので、ブロックは作らない。起動の間だけ自分の環境変数を書き換え、対象に引き継がせてから元に戻す
    /// （docs/DECISIONS.md D-015）。セッションのスレッドから呼ぶこと。
    pub fn launch(&self, command_line: &str, environment: &[(String, Option<String>)], cwd: Option<&str>) -> Result<Launched, String> {
        let mut command = wide(command_line);
        let cwd_wide = cwd.map(wide);
        let (mut process_id, mut resume_handle) = (0u32, null_mut());
        unsafe {
            let created = with_environment(environment, || {
                (self.create_process_for_launch)(command.as_mut_ptr(), 1, null_mut(), cwd_wide.as_ref().map_or(null(), |c| c.as_ptr()), &mut process_id, &mut resume_handle)
            });
            created.ok().map_err(|hr| tr!("Cannot launch the target ({code}): {command}. Check program and cwd.", code = hr, command = command_line))?;

            let mut registration = null_mut();
            let registered = (self.register_for_runtime_startup)(process_id, null(), library_provider(), on_runtime_startup, null_mut(), &mut registration);
            // 登録に失敗しても、止めたままの対象を残さない。
            (self.resume_process)(resume_handle);
            (self.close_resume_handle)(resume_handle);
            registered.ok().map_err(|hr| tr!("Cannot register for the runtime start notification ({code}).", code = hr))?;
            watch_exit(process_id);
            Ok(Launched { process_id, registration })
        }
    }

    /// 実行中のプロセスにつなぐ。ランタイムがもう読み込まれていれば、通知（`Input::RuntimeStarted`）はすぐに届く。
    /// まだなら、読み込まれるまで待つ。
    pub fn attach(&self, process_id: u32) -> Result<Launched, String> {
        let mut registration = null_mut();
        let registered = unsafe { (self.register_for_runtime_startup)(process_id, null(), library_provider(), on_runtime_startup, null_mut(), &mut registration) };
        registered.ok().map_err(|hr| tr!("Cannot find the runtime of process {pid} ({code}).", pid = process_id, code = hr))?;
        Ok(Launched { process_id, registration })
    }

    /// 起動通知の登録を解く。通知のコールバックの中から呼んではいけない。
    pub fn unregister(&self, launched: &mut Launched) {
        if !launched.registration.is_null() {
            unsafe { (self.unregister_for_runtime_startup)(launched.registration) };
            launched.registration = null_mut();
        }
    }
}
