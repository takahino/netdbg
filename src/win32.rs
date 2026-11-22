//! 使う Win32 API の宣言。必要な分だけを自前で持つ（docs/DECISIONS.md D-011）。

// 名前は Win32 の名前（HANDLE、STARTUPINFOW など）のままにする。
#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;

pub const STD_INPUT_HANDLE: u32 = -10i32 as u32;
pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
pub const HANDLE_FLAG_INHERIT: u32 = 1;
pub const DUPLICATE_SAME_ACCESS: u32 = 2;
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

#[repr(C)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: u32,
    pub lpSecurityDescriptor: *mut c_void,
    pub bInheritHandle: i32,
}

pub const STARTF_USESTDHANDLES: u32 = 0x100;

#[repr(C)]
pub struct STARTUPINFOW {
    pub cb: u32,
    pub lpReserved: *mut u16,
    pub lpDesktop: *mut u16,
    pub lpTitle: *mut u16,
    pub dwX: u32,
    pub dwY: u32,
    pub dwXSize: u32,
    pub dwYSize: u32,
    pub dwXCountChars: u32,
    pub dwYCountChars: u32,
    pub dwFillAttribute: u32,
    pub dwFlags: u32,
    pub wShowWindow: u16,
    pub cbReserved2: u16,
    pub lpReserved2: *mut u8,
    pub hStdInput: HANDLE,
    pub hStdOutput: HANDLE,
    pub hStdError: HANDLE,
}

#[repr(C)]
pub struct PROCESS_INFORMATION {
    pub hProcess: HANDLE,
    pub hThread: HANDLE,
    pub dwProcessId: u32,
    pub dwThreadId: u32,
}

#[repr(C)]
pub struct MODULEENTRY32W {
    pub dwSize: u32,
    pub th32ModuleID: u32,
    pub th32ProcessID: u32,
    pub GlblcntUsage: u32,
    pub ProccntUsage: u32,
    pub modBaseAddr: *mut u8,
    pub modBaseSize: u32,
    pub hModule: HANDLE,
    pub szModule: [u16; 256],
    pub szExePath: [u16; 260],
}

#[repr(C)]
pub struct THREADENTRY32 {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ThreadID: u32,
    pub th32OwnerProcessID: u32,
    pub tpBasePri: i32,
    pub tpDeltaPri: i32,
    pub dwFlags: u32,
}

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ProcessID: u32,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: u32,
    pub cntThreads: u32,
    pub th32ParentProcessID: u32,
    pub pcPriClassBase: i32,
    pub dwFlags: u32,
    pub szExeFile: [u16; 260],
}

pub const TH32CS_SNAPPROCESS: u32 = 0x2;
pub const TH32CS_SNAPTHREAD: u32 = 0x4;
pub const THREAD_QUERY_LIMITED_INFORMATION: u32 = 0x0800;
pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
pub const SYNCHRONIZE: u32 = 0x0010_0000;
pub const INFINITE: u32 = 0xFFFF_FFFF;

/// プロセスのメモリの数（`K32GetProcessMemoryInfo`）。大きさは `usize`（ビット数で変わる）。
#[repr(C)]
#[derive(Default)]
pub struct PROCESS_MEMORY_COUNTERS_EX {
    pub cb: u32,
    pub PageFaultCount: u32,
    pub PeakWorkingSetSize: usize,
    pub WorkingSetSize: usize,
    pub QuotaPeakPagedPoolUsage: usize,
    pub QuotaPagedPoolUsage: usize,
    pub QuotaPeakNonPagedPoolUsage: usize,
    pub QuotaNonPagedPoolUsage: usize,
    pub PagefileUsage: usize,
    pub PeakPagefileUsage: usize,
    pub PrivateUsage: usize,
}

/// アドレス空間の 1 区画（`VirtualQueryEx`）。64bit の Windows の SDK は `AllocationProtect` のあとに `PartitionId`（u16）を
/// 置くが、そこは `RegionSize` の境界合わせの詰め物に収まるので書かない（書くと、32bit で `RegionSize` の位置がずれる）。
#[repr(C)]
pub struct MEMORY_BASIC_INFORMATION {
    pub BaseAddress: usize,
    pub AllocationBase: usize,
    pub AllocationProtect: u32,
    pub RegionSize: usize,
    pub State: u32,
    pub Protect: u32,
    pub Type: u32,
}

pub const MEM_FREE: u32 = 0x1_0000;
pub const HKEY_LOCAL_MACHINE: HANDLE = 0x8000_0002u32 as i32 as isize as HANDLE;
pub const RRF_RT_REG_DWORD: u32 = 0x10;
pub const RRF_RT_REG_SZ: u32 = 0x2;
pub const GR_GDIOBJECTS: u32 = 0;
pub const GR_USEROBJECTS: u32 = 1;

#[link(name = "user32")]
extern "system" {
    /// プロセスが持つ GDI / USER のオブジェクトの数。
    pub fn GetGuiResources(process: HANDLE, flags: u32) -> u32;
}

#[link(name = "version")]
extern "system" {
    pub fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
    pub fn GetFileVersionInfoW(path: *const u16, handle: u32, length: u32, data: *mut c_void) -> i32;
    pub fn VerQueryValueW(block: *const c_void, sub_block: *const u16, buffer: *mut *mut c_void, length: *mut u32) -> i32;
}

/// ファイルの版（`VS_FIXEDFILEINFO` の FileVersion。`1.2.3.4`）。版の情報が無ければ `None`。成績書が exe の版を書くのに使う。
pub fn file_version(path: &std::path::Path) -> Option<String> {
    let name = wide(&path.to_string_lossy());
    unsafe {
        let size = GetFileVersionInfoSizeW(name.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(name.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        let mut info: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        // VS_FIXEDFILEINFO: 署名、構造の版、FileVersionMS、FileVersionLS、…（どれも u32）。
        if VerQueryValueW(data.as_ptr() as *const c_void, wide("\\").as_ptr(), &mut info, &mut length) == 0 || info.is_null() || length < 16 {
            return None;
        }
        let fixed = std::slice::from_raw_parts(info as *const u32, 4);
        let (ms, ls) = (fixed[2], fixed[3]);
        Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF))
    }
}

/// 時刻を、地方時の `2026-11-21 09:41:44` にする。
pub fn local_time_text(time: std::time::SystemTime) -> String {
    // FILETIME は 1601-01-01 からの 100 ナノ秒。
    const UNIX_TO_FILETIME: u64 = 11_644_473_600 * 10_000_000;
    let since = time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let utc = UNIX_TO_FILETIME + since.as_secs() * 10_000_000 + u64::from(since.subsec_nanos()) / 100;
    let (mut local, mut parts) = (0u64, [0u16; 8]);
    unsafe {
        if FileTimeToLocalFileTime(&utc, &mut local) == 0 || FileTimeToSystemTime(&local, &mut parts) == 0 {
            return String::new();
        }
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", parts[0], parts[1], parts[3], parts[4], parts[5], parts[6])
}

#[link(name = "shell32")]
extern "system" {
    /// 既定の関連付けで開く（専用の画面の URL をブラウザで）。戻り値が 32 以下ならエラー。
    pub fn ShellExecuteW(window: HANDLE, verb: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> HANDLE;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegGetValueW(key: HANDLE, sub_key: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, size: *mut u32) -> i32;
}

pub const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

#[link(name = "ntdll")]
extern "system" {
    /// プロセスのスレッドをすべて止める / 動かす（戻り値は NTSTATUS。0 が成功）。
    pub fn NtSuspendProcess(process: HANDLE) -> i32;
    pub fn NtResumeProcess(process: HANDLE) -> i32;
}

/// プロセスを止める（`true`）か動かす（`false`）。開けなければ `false` を返す。
pub fn suspend_process(process_id: u32, suspend: bool) -> bool {
    let Some(handle) = OwnedHandle::new(unsafe { OpenProcess(PROCESS_SUSPEND_RESUME, 0, process_id) }) else { return false };
    (unsafe { if suspend { NtSuspendProcess(handle.raw()) } else { NtResumeProcess(handle.raw()) } }) == 0
}

#[link(name = "ole32")]
extern "system" {
    /// 呼び出し側（dbgshim）が CoTaskMemFree で解放する文字列を作る。
    pub fn CoTaskMemAlloc(size: usize) -> *mut c_void;
}

#[link(name = "bcrypt")]
extern "system" {
    /// OS の乱数（`algorithm` が null なら BCRYPT_USE_SYSTEM_PREFERRED_RNG を渡す）。戻り値は NTSTATUS（0 が成功）。
    pub fn BCryptGenRandom(algorithm: *mut c_void, buffer: *mut u8, size: u32, flags: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
    pub fn K32GetProcessMemoryInfo(process: HANDLE, counters: *mut PROCESS_MEMORY_COUNTERS_EX, size: u32) -> i32;
    pub fn GetProcessHandleCount(process: HANDLE, count: *mut u32) -> i32;
    pub fn GetUserDefaultLocaleName(name: *mut u16, length: i32) -> i32;
    pub fn GetUserDefaultUILanguage() -> u16;
    pub fn LCIDToLocaleName(locale: u32, name: *mut u16, length: i32, flags: u32) -> i32;
    pub fn VirtualQueryEx(process: HANDLE, address: usize, info: *mut MEMORY_BASIC_INFORMATION, length: usize) -> usize;
    pub fn Thread32First(snapshot: HANDLE, entry: *mut THREADENTRY32) -> i32;
    pub fn Thread32Next(snapshot: HANDLE, entry: *mut THREADENTRY32) -> i32;
    pub fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> HANDLE;
    /// 時刻は FILETIME（100 ナノ秒単位）を u64 で受け取る（並びは同じ）。
    pub fn GetThreadTimes(thread: HANDLE, creation: *mut u64, exit: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
    pub fn GetProcessTimes(process: HANDLE, creation: *mut u64, exit: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
    pub fn LoadLibraryW(name: *const u16) -> HANDLE;
    pub fn GetProcAddress(module: HANDLE, name: *const u8) -> *mut c_void;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(handle: HANDLE) -> i32;
    pub fn GetExitCodeProcess(process: HANDLE, exit_code: *mut u32) -> i32;
    /// 地方時。SYSTEMTIME は 16 ビットの値 8 つ（年、月、曜日、日、時、分、秒、ミリ秒）。
    pub fn GetLocalTime(time: *mut [u16; 8]);
    /// FILETIME は u64 で受け渡す（並びは同じ）。SYSTEMTIME は 16 ビットの値 8 つ。
    pub fn FileTimeToLocalFileTime(utc: *const u64, local: *mut u64) -> i32;
    pub fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut [u16; 8]) -> i32;
    pub fn GetStdHandle(which: u32) -> HANDLE;
    pub fn GetConsoleOutputCP() -> u32;
    pub fn GetACP() -> u32;
    pub fn IsDBCSLeadByteEx(code_page: u32, byte: u8) -> i32;
    pub fn MultiByteToWideChar(code_page: u32, flags: u32, text: *const u8, length: i32, wide: *mut u16, wide_length: i32) -> i32;
    pub fn SetStdHandle(which: u32, handle: HANDLE) -> i32;
    pub fn CreatePipe(read: *mut HANDLE, write: *mut HANDLE, attributes: *const SECURITY_ATTRIBUTES, size: u32) -> i32;
    pub fn SetHandleInformation(handle: HANDLE, mask: u32, flags: u32) -> i32;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> HANDLE;
    pub fn IsWow64Process(process: HANDLE, is_wow64: *mut i32) -> i32;
    pub fn QueryFullProcessImageNameW(process: HANDLE, flags: u32, name: *mut u16, size: *mut u32) -> i32;
    pub fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> HANDLE;
    pub fn Module32FirstW(snapshot: HANDLE, entry: *mut MODULEENTRY32W) -> i32;
    pub fn Module32NextW(snapshot: HANDLE, entry: *mut MODULEENTRY32W) -> i32;
    pub fn Process32FirstW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
    pub fn Process32NextW(snapshot: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
    pub fn ProcessIdToSessionId(process_id: u32, session: *mut u32) -> i32;
    /// ドライブのビットの並び（A: が最下位）。
    pub fn GetLogicalDrives() -> u32;
    pub fn DuplicateHandle(
        source_process: HANDLE,
        source: HANDLE,
        target_process: HANDLE,
        target: *mut HANDLE,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    pub fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const SECURITY_ATTRIBUTES,
        disposition: u32,
        flags: u32,
        template: HANDLE,
    ) -> HANDLE;
}

/// NUL で終わる UTF-16 にする。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// NUL までの UTF-16 を文字列にする。
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// NUL で終わる UTF-16 のポインター（COM の呼び出しで渡されたもの）を文字列にする。null なら空。
///
/// # Safety
/// `text` は null か、NUL で終わる UTF-16 を指していること。
pub unsafe fn from_wide_ptr(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let length = (0..).take_while(|&i| *text.add(i) != 0).count();
    String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
}

/// 引数一つを、Windows のコマンドラインの規則（CommandLineToArgvW が元に戻せる形）で引用する。
pub fn quote_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // 引用符の前のバックスラッシュは倍にし、引用符自体もエスケープする。
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                quoted.push('"');
                continue;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            quoted.push(c);
        }
    }
    // 閉じる引用符の前のバックスラッシュも倍にする。
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// プログラムと引数から、一本のコマンドラインを作る。
pub fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program).chain(args.iter().map(String::as_str)).map(quote_argument).collect::<Vec<_>>().join(" ")
}

/// 子プロセスに引き継がせる環境変数を足して（値が `None` なら消して）`work` を行い、元に戻す。
///
/// 環境変数のブロックを作って渡す方法は、dbgshim が ANSI として扱うので使えない（docs/DECISIONS.md D-015）。
/// 自分の環境変数を一時的に書き換えるので、ほかのスレッドが環境変数を読まないセッションのスレッドから呼ぶこと。
pub fn with_environment<T>(environment: &[(String, Option<String>)], work: impl FnOnce() -> T) -> T {
    let saved: Vec<(&String, Option<std::ffi::OsString>)> = environment.iter().map(|(name, _)| (name, std::env::var_os(name))).collect();
    let apply = |name: &String, value: Option<&std::ffi::OsStr>| match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    };
    for (name, value) in environment {
        apply(name, value.as_ref().map(|v| v.as_ref()));
    }
    let result = work();
    for (name, value) in saved {
        apply(name, value.as_deref());
    }
    result
}

/// スコープを出るときに閉じるハンドル（途中で return しても閉じ忘れない）。
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// 開くのに失敗した値（null、INVALID_HANDLE_VALUE）なら `None`。
    pub fn new(handle: HANDLE) -> Option<OwnedHandle> {
        (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(OwnedHandle(handle))
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// プロセスを、情報を読むためだけに開く。開けなければ（権限が無い、もう終わっている）`None`。
pub fn open_process_for_query(process_id: u32) -> Option<OwnedHandle> {
    OwnedHandle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) })
}

/// プロセスが読み込んでいるモジュール 1 つ（名前と、読み込まれた範囲）。
pub struct ModuleRange {
    pub name: String,
    pub base: usize,
    pub size: usize,
}

/// プロセスが読み込んでいるモジュールのファイル名。調べられなければ空（権限が無い、ビット数が違う、もう終わっている）。
pub fn process_modules(process_id: u32) -> Vec<String> {
    process_module_ranges(process_id).into_iter().map(|m| m.name).collect()
}

/// プロセスが読み込んでいるモジュールと、その範囲。調べられなければ空。
pub fn process_module_ranges(process_id: u32) -> Vec<ModuleRange> {
    const TH32CS_SNAPMODULE: u32 = 0x8;
    const TH32CS_SNAPMODULE32: u32 = 0x10;
    const ERROR_BAD_LENGTH: u32 = 24;
    let mut modules = Vec::new();
    unsafe {
        // 対象がモジュールを読み込んでいる最中だと ERROR_BAD_LENGTH で失敗する。やり直せば取れる。
        let mut snapshot = None;
        for _ in 0..10 {
            snapshot = OwnedHandle::new(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id));
            if snapshot.is_some() || GetLastError() != ERROR_BAD_LENGTH {
                break;
            }
        }
        let Some(snapshot) = snapshot else { return modules };
        let mut entry: MODULEENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut more = Module32FirstW(snapshot.raw(), &mut entry);
        while more != 0 {
            modules.push(ModuleRange { name: from_wide(&entry.szModule), base: entry.modBaseAddr as usize, size: entry.modBaseSize as usize });
            more = Module32NextW(snapshot.raw(), &mut entry);
        }
    }
    modules
}

/// このエンジンのビット数の CONTEXT（スタックウォークで読むレジスター）の形。ビット数で形が違うので、バインディングと
/// 同じく、ここに閉じ込める（使う側は `CONTEXT_SIZE`、`CONTEXT_CONTROL`、`context_ip` だけを見る）。
#[cfg(target_arch = "x86")]
mod context_layout {
    pub const SIZE: usize = 716;
    /// Eip の位置。
    pub const IP: usize = 0xB8;
    /// CONTEXT_i386 | CONTEXT_CONTROL。
    pub const CONTROL: u32 = 0x0001_0001;
    /// CONTEXT_i386 | CONTEXT_DEBUG_REGISTERS と、ContextFlags、Dr0、Dr6、Dr7 の位置。
    pub const DEBUG: u32 = 0x0001_0010;
    pub const FLAGS: usize = 0x00;
    pub const DR0: usize = 0x04;
    pub const DR6: usize = 0x14;
    pub const DR7: usize = 0x18;
    /// ネイティブのコードの中で書き込みの見張りに当たると、ランタイムが処理できずに対象が落ちる（D-093。x64 は止まる）。
    pub const NATIVE_WRITE_ENDS_TARGET: bool = true;
}
#[cfg(target_arch = "x86_64")]
mod context_layout {
    pub const SIZE: usize = 1232;
    /// Rip の位置。
    pub const IP: usize = 0xF8;
    /// CONTEXT_AMD64 | CONTEXT_CONTROL。
    pub const CONTROL: u32 = 0x0010_0001;
    /// CONTEXT_AMD64 | CONTEXT_DEBUG_REGISTERS と、ContextFlags、Dr0、Dr6、Dr7 の位置。
    pub const DEBUG: u32 = 0x0010_0010;
    pub const FLAGS: usize = 0x30;
    pub const DR0: usize = 0x48;
    pub const DR6: usize = 0x68;
    pub const DR7: usize = 0x70;
    /// ネイティブのコードの中で当たっても、呼んだマネージドのフレームで止まる（D-093）。
    pub const NATIVE_WRITE_ENDS_TARGET: bool = false;
}
pub const CONTEXT_SIZE: usize = context_layout::SIZE;
pub const CONTEXT_CONTROL: u32 = context_layout::CONTROL;

/// 書き込みの見張りに、ネイティブのコードから書かれると対象が落ちるか（D-093）。
pub const NATIVE_WRITE_ENDS_TARGET: bool = context_layout::NATIVE_WRITE_ENDS_TARGET;

/// デバッグ レジスターを読み書きする CONTEXT（ContextFlags を入れたもの）。
pub fn debug_context() -> Vec<u8> {
    let mut context = vec![0u8; CONTEXT_SIZE];
    context[context_layout::FLAGS..context_layout::FLAGS + 4].copy_from_slice(&context_layout::DEBUG.to_le_bytes());
    context
}

fn put_usize(context: &mut [u8], at: usize, value: usize) {
    context[at..at + std::mem::size_of::<usize>()].copy_from_slice(&value.to_le_bytes());
}

fn get_usize(context: &[u8], at: usize) -> usize {
    usize::from_le_bytes(context[at..at + std::mem::size_of::<usize>()].try_into().unwrap_or_default())
}

/// ハードウェアの書き込みの見張り（Dr0 と Dr7 の 0 番）を置くか（`Some((番地, 大きさ))`）、外す（`None`）。大きさは 1、2、4、8
/// （8 は 64bit だけ）。ほかの番（Dr1〜Dr3）は触らない。
pub fn set_write_watch(context: &mut [u8], watch: Option<(usize, usize)>) {
    // Dr7: L0 (bit 0)、RW0 (bit 16-17。01 = 書き込み)、LEN0 (bit 18-19。00 = 1、01 = 2、11 = 4、10 = 8)。
    let mut dr7 = get_usize(context, context_layout::DR7) & !(0b1 | (0b1111 << 16));
    match watch {
        Some((address, size)) => {
            let length = match size {
                1 => 0b00,
                2 => 0b01,
                8 => 0b10,
                _ => 0b11,
            };
            put_usize(context, context_layout::DR0, address);
            dr7 |= 0b1 | (0b01 << 16) | (length << 18);
        }
        None => put_usize(context, context_layout::DR0, 0),
    }
    put_usize(context, context_layout::DR7, dr7);
    put_usize(context, context_layout::DR6, 0);
}

/// CONTEXT の命令の番地。
pub fn context_ip(context: &[u8]) -> Option<usize> {
    let bytes = context.get(context_layout::IP..context_layout::IP + std::mem::size_of::<usize>())?;
    Some(usize::from_le_bytes(bytes.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_arguments_are_not_quoted() {
        assert_eq!(quote_argument(r"C:\app\a.exe"), r"C:\app\a.exe");
    }

    #[test]
    fn spaces_quotes_and_trailing_backslashes_are_quoted() {
        assert_eq!(quote_argument(r"C:\Program Files\a.exe"), r#""C:\Program Files\a.exe""#);
        assert_eq!(quote_argument(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_argument(r"C:\my dir\"), r#""C:\my dir\\""#);
        assert_eq!(quote_argument(r#"a\"b c"#), r#""a\\\"b c""#);
        assert_eq!(quote_argument(""), r#""""#);
    }

    #[test]
    fn a_command_line_joins_the_quoted_parts() {
        assert_eq!(command_line(r"C:\a b\x.exe", &["1".into(), "two words".into()]), r#""C:\a b\x.exe" 1 "two words""#);
    }

    #[test]
    fn wide_strings_round_trip() {
        assert_eq!(from_wide(&wide("日本語 abc")), "日本語 abc");
    }
}