//! 対象のプロセスの使用量（段階 11）: Private Bytes、Working Set、CPU 時間、GDI / USER のオブジェクト、ハンドル、32bit の
//! アドレス空間。拡張の補助（PowerShell の processSampler.ps1）が読んでいたものを、エンジンが Win32 で直接読む。グラフは
//! 拡張が描く（ここは今の値を返すだけ）。

use crate::win32::*;
use serde_json::{json, Value};
use std::ffi::c_void;

const GB: u64 = 1 << 30;
/// GDI / USER のオブジェクトの、プロセスごとの上限の既定値（レジストリで変えていなければ）。
const DEFAULT_QUOTA: u32 = 10_000;

/// 1 回の読み取り。
#[derive(Debug)]
pub struct Usage {
    pub private_bytes: u64,
    pub working_set: u64,
    /// 起動からの CPU 時間（カーネル + ユーザー、ミリ秒）。使用率は、呼ぶ側が前の値との差から出す。
    pub cpu_ms: u64,
    pub gdi_objects: u32,
    pub user_objects: u32,
    pub handles: u32,
    /// 32bit のプロセス（64bit の Windows の上）のアドレス空間: 上限、予約済みと確保済みの合計、いちばん大きな空き。
    /// 大きな確保ができるかは、空きの合計ではなく、いちばん大きな空きで決まる。
    pub address_space: Option<AddressSpace>,
}

#[derive(Debug)]
pub struct AddressSpace {
    pub limit: u64,
    pub used: u64,
    pub largest_free: u64,
}

impl Usage {
    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "privateBytes": self.private_bytes,
            "workingSet": self.working_set,
            "cpuMs": self.cpu_ms,
            "gdiObjects": self.gdi_objects,
            "userObjects": self.user_objects,
            "handles": self.handles,
            "gdiQuota": quota("GDIProcessHandleQuota"),
            "userQuota": quota("USERProcessHandleQuota"),
            "wow64": self.address_space.is_some(),
        });
        if let Some(space) = &self.address_space {
            out["addressLimit"] = json!(space.limit);
            out["addressUsed"] = json!(space.used);
            out["largestFree"] = json!(space.largest_free);
        }
        out
    }
}

/// プロセスの今の使用量。`process` は、情報とメモリの区画を読める権限のハンドル（デバッガーが持つもの）。
///
/// # Safety
/// `process` は開いているプロセスのハンドル。
pub unsafe fn sample(process: HANDLE) -> Usage {
    let mut memory = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    K32GetProcessMemoryInfo(process, &mut memory, memory.cb);
    let (mut creation, mut exit, mut kernel, mut user) = (0u64, 0u64, 0u64, 0u64);
    GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
    let mut handles = 0u32;
    GetProcessHandleCount(process, &mut handles);
    let mut wow64 = 0i32;
    IsWow64Process(process, &mut wow64);
    Usage {
        private_bytes: memory.PrivateUsage as u64,
        working_set: memory.WorkingSetSize as u64,
        // FILETIME の単位は 100 ナノ秒。
        cpu_ms: (kernel + user) / 10_000,
        gdi_objects: GetGuiResources(process, GR_GDIOBJECTS),
        user_objects: GetGuiResources(process, GR_USEROBJECTS),
        handles,
        address_space: (wow64 != 0).then(|| address_space(process)),
    }
}

/// 32bit のプロセスのアドレス空間をたどる。上限は 2 GB、exe が大きなアドレスに対応していれば 4 GB。
unsafe fn address_space(process: HANDLE) -> AddressSpace {
    let limit = if large_address_aware(process) { 4 * GB } else { 2 * GB };
    let (mut used, mut largest_free, mut address) = (0u64, 0u64, 0u64);
    while address < limit {
        let mut info: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        if VirtualQueryEx(process, address as usize, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
            break;
        }
        let start = info.BaseAddress as u64;
        let end = (start + info.RegionSize as u64).min(limit);
        if end <= address {
            break;
        }
        if info.State == MEM_FREE {
            largest_free = largest_free.max(end - start);
        } else {
            used += end - start;
        }
        address = end;
    }
    AddressSpace { limit, used, largest_free }
}

/// プロセスの exe の PE ヘッダーに、IMAGE_FILE_LARGE_ADDRESS_AWARE があるか。読めなければ `false`。
unsafe fn large_address_aware(process: HANDLE) -> bool {
    const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;
    let mut name = vec![0u16; 1024];
    let mut size = name.len() as u32;
    if QueryFullProcessImageNameW(process, 0, name.as_mut_ptr(), &mut size) == 0 {
        return false;
    }
    let Ok(image) = std::fs::read(from_wide(&name[..size as usize])) else { return false };
    // e_lfanew（0x3C）の先に "PE\0\0"、続く IMAGE_FILE_HEADER の Characteristics は 18 バイト目。
    let read_u32 = |at: usize| image.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let Some(pe) = read_u32(0x3C).map(|offset| offset as usize) else { return false };
    image.get(pe + 4 + 18..pe + 4 + 20).is_some_and(|b| u16::from_le_bytes([b[0], b[1]]) & IMAGE_FILE_LARGE_ADDRESS_AWARE != 0)
}

/// GDI / USER のオブジェクトの、プロセスごとの上限（レジストリ。無ければ既定の 10,000）。
fn quota(name: &str) -> u32 {
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows");
    let value = wide(name);
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut c_void, &mut size) };
    if status == 0 && data > 0 {
        data
    } else {
        DEFAULT_QUOTA
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_reads_its_own_usage() {
        let usage = unsafe { sample(GetCurrentProcess()) };
        assert!(usage.private_bytes > 0 && usage.working_set > 0, "{usage:?}");
        assert!(usage.handles > 0, "{usage:?}");
        // 32bit のエンジン（64bit の Windows の上）なら、アドレス空間も読める。
        if let Some(space) = &usage.address_space {
            assert!(space.used > 0 && space.largest_free > 0 && space.used + space.largest_free <= space.limit, "{space:?}");
        }
        let shown = usage.to_json();
        assert!(shown["gdiQuota"].as_u64().unwrap() > 0);
    }

    #[test]
    fn the_memory_layouts_match_the_windows_headers() {
        // MEMORY_BASIC_INFORMATION: RegionSize は、2 つのアドレスと AllocationProtect（と 64bit の PartitionId）の後ろの、
        // 境界に合う位置。大きさは 32bit で 28 バイト、64bit で 48 バイト。
        let word = std::mem::size_of::<usize>();
        let region_size = std::mem::offset_of!(MEMORY_BASIC_INFORMATION, RegionSize);
        assert_eq!(region_size, (2 * word + 4).next_multiple_of(word));
        assert_eq!(std::mem::size_of::<MEMORY_BASIC_INFORMATION>(), (region_size + word + 12).next_multiple_of(word));
        assert_eq!(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>(), 8 + 9 * std::mem::size_of::<usize>());
    }
}
