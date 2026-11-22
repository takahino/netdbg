//! クラッシュダンプを ICorDebug で開く（段階 12）。生きたプロセスにつなぐ道とは別で、`ICLRDebugging::OpenVirtualProcess`
//! に、ダンプを読む口（`ICorDebugDataTarget`）と、ダンプのランタイムに合う DAC（mscordacwks.dll / mscordaccore.dll）と
//! mscordbi.dll を渡す口（`ICLRDebuggingLibraryProvider`）を渡す。返ってきた `ICorDebugProcess` は、止まったままの
//! プロセスと同じに読める（スレッド、スタック、ローカル変数、ヒープ）。動かすこと（続行、ステップ、関数の評価）はできない。
//!
//! ランタイム（clr.dll / coreclr.dll）はエンジンのプロセスに読み込まない。読み込むのは DAC と mscordbi だけ（生きた
//! プロセスのデバッグでも、mscordbi は読み込んでいる）。
//!
//! DAC はダンプと同じビット数でしか動かないので、x86 のダンプ（WOW64 のものも）は x86 のエンジン、x64 のダンプは x64 の
//! エンジンで開く。

#![allow(non_snake_case)]

use super::minidump::{Minidump, ARCHITECTURE_AMD64, ARCHITECTURE_X86};
use crate::com::{ComPtr, IUnknown, IUnknownVtbl, Interface, E_FAIL, E_NOINTERFACE, GUID, HRESULT, S_OK};
use crate::cordebug::*;
use crate::metahost::*;
use crate::win32::*;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

// metahost.idl では cpp_quote の中にあり、生成されない。値は同じファイルから写した。
const CLSID_CLR_DEBUGGING: GUID = GUID { data1: 0xBACC578D, data2: 0xFBDD, data3: 0x48a4, data4: [0x96, 0x9F, 0x02, 0xD9, 0x32, 0xB7, 0x46, 0x34] };
/// 読めない番地（HRESULT_FROM_WIN32(ERROR_PARTIAL_COPY)）。DAC は、これを「その番地はダンプに無い」と受け取る。
const PARTIAL_COPY: HRESULT = HRESULT(0x8007_012Bu32 as i32);
const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;

/// 開いたダンプ。`process` を使う間、読む口と DAC を渡す口を生かしておく。
pub struct OpenedDump {
    pub process: ComPtr<ICorDebugProcess>,
    /// ダンプを書いたときの例外のスレッドと、その例外のコード。
    pub exception: Option<(u32, u32)>,
    pub process_id: Option<u32>,
    /// ダンプに書かれたスレッドの順（作られた順。最初のものがプロセスの最初のスレッド）。
    pub thread_order: Vec<u32>,
    pub framework: bool,
    pub wow64: bool,
    _target: ComPtr<ICorDebugDataTarget>,
    _provider: ComPtr<ICLRDebuggingLibraryProvider>,
}

/// ダンプを開く。
pub fn open(path: &Path) -> Result<OpenedDump, String> {
    let dump = Minidump::open(path)?;
    let (runtime, framework) = match (dump.module_named("clr.dll"), dump.module_named("coreclr.dll")) {
        (Some(clr), _) => (clr, true),
        (None, Some(coreclr)) => (coreclr, false),
        // 単一ファイルで発行した .NET のアプリは、ランタイムを exe の中に持つ（DAC の探し方が別で、まだ対応していない）。
        _ => return Err(tr!("This dump has no .NET runtime (clr.dll / coreclr.dll). Open a dump of a .NET app (dumps of apps published as a single file are not supported yet).")),
    };
    let (runtime_base, runtime_path) = (runtime.base, runtime.path.clone());
    let x86 = match dump.architecture {
        ARCHITECTURE_X86 => true,
        ARCHITECTURE_AMD64 => image_machine(&dump, runtime_base) == Some(IMAGE_FILE_MACHINE_I386),
        other => return Err(tr!("The processor of this dump ({processor}) is not supported. x86 and x64 dumps can be opened.", processor = other)),
    };
    let wow64 = x86 && dump.architecture == ARCHITECTURE_AMD64;
    // DAC はダンプと同じビット数のプロセスでしか動かない。拡張は、ダンプのビット数でエンジンを選ぶ。
    let engine_is_x86 = std::mem::size_of::<usize>() == 4;
    if x86 != engine_is_x86 {
        let (dump_bits, engine) = if x86 { ("32bit", "x86") } else { ("64bit", "x64") };
        return Err(tr!("This dump is of a {bits} process. Open it with the {engine} engine.", bits = dump_bits, engine = engine));
    }
    let exception = dump.exception.as_ref().map(|e| (e.thread, e.code));
    let process_id = dump.process_id;
    let thread_order = dump.threads.iter().map(|t| t.id).collect();
    let directories = library_directories(path, &runtime_path, framework);
    crate::log!("dump: {} (runtime {runtime_path}, wow64 {wow64}, {} threads, {} modules)", path.display(), dump.threads.len(), dump.modules.len());

    let target = DataTarget::create(dump, if x86 { CORDB_PLATFORM_WINDOWS_X86 } else { CORDB_PLATFORM_WINDOWS_AMD64 });
    let provider = LibraryProvider::create(directories);
    let debugging = create_clr_debugging(framework)?;
    // 扱える版の上限。ランタイムの版がこれを超えると、ICLRDebugging は断る（どの版も受ける）。
    let mut max_version = CLR_DEBUGGING_VERSION { wStructVersion: 0, wMajor: 0xFFFF, wMinor: 0xFFFF, wBuild: 0xFFFF, wRevision: 0xFFFF };
    let mut version = CLR_DEBUGGING_VERSION { wStructVersion: 0, wMajor: 0, wMinor: 0, wBuild: 0, wRevision: 0 };
    let mut flags = 0;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe { debugging.OpenVirtualProcess(runtime_base, target.as_raw(), provider.as_raw(), &mut max_version, &ICorDebugProcess::IID, &mut raw, &mut version, &mut flags) };
    let process = unsafe { ComPtr::<ICorDebugProcess>::from_raw(raw) };
    let Some(process) = process.filter(|_| hr.is_ok()) else {
        return Err(open_failure(hr, &provider, &runtime_path, framework));
    };
    crate::log!("dump opened: runtime {}.{}.{}.{} (flags {flags:#x})", version.wMajor, version.wMinor, version.wBuild, version.wRevision);
    Ok(OpenedDump { process, exception, process_id, thread_order, framework, wow64, _target: target, _provider: provider })
}

/// モジュールの像の PE ヘッダーの Machine（ダンプのメモリーから）。
fn image_machine(dump: &Minidump, base: u64) -> Option<u16> {
    let mut lfanew = [0u8; 4];
    (dump.read(base + 0x3C, &mut lfanew) == 4).then_some(())?;
    let mut machine = [0u8; 2];
    (dump.read(base + u64::from(u32::from_le_bytes(lfanew)) + 4, &mut machine) == 2).then(|| u16::from_le_bytes(machine))
}

/// 開けなかった理由と、直し方。
fn open_failure(hr: HRESULT, provider: &ComPtr<ICLRDebuggingLibraryProvider>, runtime: &str, framework: bool) -> String {
    let missing = LibraryProvider::missing(provider);
    if !missing.is_empty() {
        let directory = Path::new(runtime).parent().map(|d| d.display().to_string()).unwrap_or_default();
        let kind = if framework { ".NET Framework" } else { ".NET" };
        return tr!(
            "The {files} matching the {kind} runtime of this dump is not found ({code}). Put the files of the same name from {folder} on the machine that wrote the dump into the dump's folder, then open it again (files of the same version from this machine also work).",
            files = missing.join(tr_str!(", ")),
            kind = kind,
            code = hr,
            folder = directory
        );
    }
    tr!("Cannot open the dump (ICLRDebugging::OpenVirtualProcess {code}). The dump is broken or lacks memory (use a full dump).", code = hr)
}

/// DAC と mscordbi を探すフォルダー: ダンプのフォルダー、ダンプを書いた機械のランタイムのフォルダー（同じ機械なら
/// そのまま合う）、.NET ならインストールされた共有ランタイム。
fn library_directories(dump: &Path, runtime: &str, framework: bool) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = dump.parent().into_iter().map(Path::to_path_buf).collect();
    directories.extend(Path::new(runtime).parent().map(Path::to_path_buf));
    if !framework {
        directories.extend(super::libraries::candidate_directories());
    }
    directories
}

/// `ICLRDebugging` を作る。.NET Framework は Windows の mscoree.dll、.NET はエンジンの隣の dbgshim.dll から。
fn create_clr_debugging(framework: bool) -> Result<ComPtr<ICLRDebugging>, String> {
    let library = if framework {
        "mscoree.dll".to_string()
    } else {
        std::env::current_exe().map_err(|e| tr!("Cannot tell where this executable is: {error}", error = e))?.with_file_name("dbgshim.dll").display().to_string()
    };
    unsafe {
        let module = LoadLibraryW(wide(&library).as_ptr());
        if module.is_null() {
            return Err(tr!("Cannot load {library} (Win32 error {code}).", library = library, code = GetLastError()));
        }
        let address = GetProcAddress(module, c"CLRCreateInstance".as_ptr().cast());
        if address.is_null() {
            return Err(tr!("{library} has no CLRCreateInstance.", library = library));
        }
        // 宣言は metahost.h の CLRCreateInstance と一致させてある。
        type ClrCreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
        let create = std::mem::transmute::<*mut c_void, ClrCreateInstance>(address);
        let mut raw = std::ptr::null_mut();
        create(&CLSID_CLR_DEBUGGING, &ICLRDebugging::IID, &mut raw).ok().map_err(|hr| tr!("CLRCreateInstance (ICLRDebugging) of {library} failed ({code})", library = library, code = hr))?;
        ComPtr::from_raw(raw).ok_or_else(|| tr!("CLRCreateInstance returned null"))
    }
}

// ---- 参照カウントを持つ COM のオブジェクト（下の 2 つに共通）

/// 先頭に vtable、続いて参照の数と中身（`inner`）。`I` は、QueryInterface で答えるインターフェイス。
#[repr(C)]
struct ComObject<V: 'static, T> {
    vtable: &'static V,
    references: AtomicU32,
    inner: T,
}

impl<V: 'static, T> ComObject<V, T> {
    fn create<I: Interface>(vtable: &'static V, inner: T) -> ComPtr<I> {
        let object = Box::new(ComObject { vtable, references: AtomicU32::new(1), inner });
        unsafe { ComPtr::from_raw(Box::into_raw(object).cast()) }.expect("a Box is never null")
    }

    /// COM のポインター（`this`）の中身。
    unsafe fn inner<'a>(this: *mut c_void) -> &'a T {
        &(*(this as *const Self)).inner
    }
}

unsafe extern "system" fn query_interface<I: Interface, V: 'static, T>(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    let iid = *riid;
    if iid == IUnknown::IID || iid == I::IID {
        add_ref::<V, T>(this);
        *out = this;
        S_OK
    } else {
        *out = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref<V: 'static, T>(this: *mut c_void) -> u32 {
    (*(this as *const ComObject<V, T>)).references.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn release<V: 'static, T>(this: *mut c_void) -> u32 {
    let left = (*(this as *const ComObject<V, T>)).references.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        drop(Box::from_raw(this as *mut ComObject<V, T>));
    }
    left
}

// ---- ダンプを読む口（ICorDebugDataTarget）

struct DataTarget {
    dump: Minidump,
    platform: CorDebugPlatform,
}

type DataTargetObject = ComObject<ICorDebugDataTargetVtbl, DataTarget>;

impl DataTarget {
    fn create(dump: Minidump, platform: CorDebugPlatform) -> ComPtr<ICorDebugDataTarget> {
        DataTargetObject::create(&DATA_TARGET_VTBL, DataTarget { dump, platform })
    }
}

unsafe extern "system" fn GetPlatform(this: *mut c_void, platform: *mut CorDebugPlatform) -> HRESULT {
    *platform = DataTargetObject::inner(this).platform;
    S_OK
}

unsafe extern "system" fn ReadVirtual(this: *mut c_void, address: CORDB_ADDRESS, buffer: *mut u8, requested: u32, read: *mut u32) -> HRESULT {
    let done = DataTargetObject::inner(this).dump.read(address, std::slice::from_raw_parts_mut(buffer, requested as usize));
    if !read.is_null() {
        *read = done as u32;
    }
    if done == 0 && requested > 0 {
        PARTIAL_COPY
    } else {
        S_OK
    }
}

unsafe extern "system" fn GetThreadContext(this: *mut c_void, thread_id: u32, _flags: u32, size: u32, context: *mut u8) -> HRESULT {
    let target = DataTargetObject::inner(this);
    let Some(thread) = target.dump.threads.iter().find(|t| t.id == thread_id) else { return E_FAIL };
    // x86 のプロセスなら x86 の CONTEXT（WOW64 のダンプでは、64bit のレジスターではなく WOW64 の作業領域のもの）。
    let registers = if target.platform == CORDB_PLATFORM_WINDOWS_X86 { target.dump.x86_context(thread) } else { target.dump.context(thread) };
    let Some(registers) = registers else { return E_FAIL };
    let count = registers.len().min(size as usize);
    std::ptr::copy_nonoverlapping(registers.as_ptr(), context, count);
    S_OK
}

static DATA_TARGET_VTBL: ICorDebugDataTargetVtbl = ICorDebugDataTargetVtbl {
    base: IUnknownVtbl {
        QueryInterface: query_interface::<ICorDebugDataTarget, ICorDebugDataTargetVtbl, DataTarget>,
        AddRef: add_ref::<ICorDebugDataTargetVtbl, DataTarget>,
        Release: release::<ICorDebugDataTargetVtbl, DataTarget>,
    },
    GetPlatform,
    ReadVirtual,
    GetThreadContext,
};

// ---- DAC と mscordbi を渡す口（ICLRDebuggingLibraryProvider）

struct LibraryProvider {
    directories: Vec<PathBuf>,
    /// 求められたのに見つからなかったファイル（開けなかったときに、理由として伝える）。
    missing: Mutex<Vec<String>>,
}

type LibraryProviderObject = ComObject<ICLRDebuggingLibraryProviderVtbl, LibraryProvider>;

impl LibraryProvider {
    fn create(directories: Vec<PathBuf>) -> ComPtr<ICLRDebuggingLibraryProvider> {
        LibraryProviderObject::create(&PROVIDER_VTBL, LibraryProvider { directories, missing: Mutex::new(Vec::new()) })
    }

    fn missing(provider: &ComPtr<ICLRDebuggingLibraryProvider>) -> Vec<String> {
        let this = unsafe { LibraryProviderObject::inner(provider.as_raw()) };
        this.missing.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// `file_name`（mscordacwks.dll など）で、版（TimeDateStamp と SizeOfImage）が合うもの。シンボル サーバーの名前
    /// （`mscordacwks_x86_x86_4.8.9200.00.dll` など）で置かれたものも見る。
    fn find(&self, file_name: &str, wanted: (u32, u32)) -> Option<PathBuf> {
        let stem = Path::new(file_name).file_stem()?.to_string_lossy().to_lowercase();
        let matches = |path: &Path| path.is_file() && super::libraries::identity(path) == Some(wanted);
        for directory in &self.directories {
            let exact = directory.join(file_name);
            if matches(&exact) {
                return Some(exact);
            }
            let renamed = std::fs::read_dir(directory).into_iter().flatten().filter_map(|e| e.ok()).map(|e| e.path()).find(|path| {
                let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                name.starts_with(&format!("{stem}_")) && name.ends_with(".dll") && matches(path)
            });
            if renamed.is_some() {
                return renamed;
            }
        }
        None
    }
}

unsafe extern "system" fn ProvideLibrary(this: *mut c_void, file_name: *mut u16, timestamp: u32, size_of_image: u32, module: *mut *mut c_void) -> HRESULT {
    let provider = LibraryProviderObject::inner(this);
    let name = from_wide_ptr(file_name);
    let Some(path) = provider.find(&name, (timestamp, size_of_image)) else {
        crate::log!("dump: {name} ({timestamp:#x}, {size_of_image:#x}) not found");
        if let Ok(mut missing) = provider.missing.lock() {
            missing.push(name);
        }
        return E_FAIL;
    };
    let handle = LoadLibraryW(wide(&path.to_string_lossy()).as_ptr());
    crate::log!("dump: {name} -> {} ({})", path.display(), if handle.is_null() { "load failed" } else { "loaded" });
    if handle.is_null() {
        return E_FAIL;
    }
    *module = handle;
    S_OK
}

static PROVIDER_VTBL: ICLRDebuggingLibraryProviderVtbl = ICLRDebuggingLibraryProviderVtbl {
    base: IUnknownVtbl {
        QueryInterface: query_interface::<ICLRDebuggingLibraryProvider, ICLRDebuggingLibraryProviderVtbl, LibraryProvider>,
        AddRef: add_ref::<ICLRDebuggingLibraryProviderVtbl, LibraryProvider>,
        Release: release::<ICLRDebuggingLibraryProviderVtbl, LibraryProvider>,
    },
    ProvideLibrary,
};