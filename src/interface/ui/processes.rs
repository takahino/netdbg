//! 画面だけが使う、アタッチするプロセスの一覧（`GET /sys/processes`）。
//!
//! 同じログオンのセッションの、開けるプロセスだけを出す（サービスや、ほかの利用者のものは開けない）。ビット数と、
//! .NET のランタイム（`clr.dll` / `coreclr.dll`）を読み込んでいるかを添える。ビット数の違うプロセスのモジュールは
//! 32bit のエンジンからは読めないので、そのときの `managed` は null（分からない）。

use crate::debugger::target;
use crate::win32::{self, OwnedHandle, PROCESSENTRY32W};
use serde_json::{json, Value};

pub fn list() -> Value {
    let mine = std::process::id();
    let session = session_of(mine);
    let mut processes = Vec::new();
    for (process_id, name) in all() {
        if process_id == mine || process_id == 0 || session_of(process_id) != session {
            continue;
        }
        let Ok(bits) = target::bits_of_process(process_id) else { continue };
        let managed = (bits == target::Bits::of_this_process() || usize::BITS == 64).then(|| {
            win32::process_modules(process_id).iter().any(|m| m.eq_ignore_ascii_case("clr.dll") || m.eq_ignore_ascii_case("coreclr.dll"))
        });
        processes.push(json!({ "pid": process_id, "name": name, "path": target::process_image_path(process_id), "bits": bits.name(), "managed": managed }));
    }
    processes.sort_by_key(|p| (p["managed"] != json!(true), p["name"].as_str().unwrap_or("").to_lowercase()));
    json!({ "processes": processes, "engineBits": target::Bits::of_this_process().name() })
}

/// すべてのプロセス（番号と exe の名前）。
fn all() -> Vec<(u32, String)> {
    let mut list = Vec::new();
    unsafe {
        let Some(snapshot) = OwnedHandle::new(win32::CreateToolhelp32Snapshot(win32::TH32CS_SNAPPROCESS, 0)) else { return list };
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = win32::Process32FirstW(snapshot.raw(), &mut entry) != 0;
        while more {
            list.push((entry.th32ProcessID, win32::from_wide(&entry.szExeFile)));
            more = win32::Process32NextW(snapshot.raw(), &mut entry) != 0;
        }
    }
    list
}

fn session_of(process_id: u32) -> Option<u32> {
    let mut session = 0;
    (unsafe { win32::ProcessIdToSessionId(process_id, &mut session) } != 0).then_some(session)
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_session_lists_the_running_tests_parent() {
        let listed = super::list();
        let processes = listed["processes"].as_array().unwrap();
        assert!(!processes.is_empty());
        assert!(processes.iter().all(|p| p["pid"].as_u64() != Some(u64::from(std::process::id()))), "the engine itself is not listed");
        assert!(processes.iter().all(|p| p["bits"] == "x86" || p["bits"] == "x64"));
    }
}
