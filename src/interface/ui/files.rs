//! 画面だけが使うファイルの口: プログラムを開く画面のフォルダーの一覧（`GET /fs/list`）と、ソースの読み取り
//! （`GET /fs/read`）。ブラウザは手元のファイルのパスを渡せないので、エンジンが代わりに読む。
//!
//! 逆コンパイルしたソース（D-060 のキャッシュ）も、ふつうのパスとして同じ道で読む。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 読むソースの大きさの上限。
const MAX_SOURCE: u64 = 8 * 1024 * 1024;
/// 一覧に出すファイルの拡張子（起動できるもの、ダンプ、ソース。ソースは、起動の前に開いてブレークポイントを置くため）。
const LISTED: [&str; 6] = ["exe", "dll", "dmp", "cs", "vb", "fs"];

/// フォルダーの中身。`path` が空なら、ドライブの一覧。
pub fn list(path: &str) -> Result<Value, String> {
    if path.is_empty() {
        let drives: Vec<Value> = drives().into_iter().map(|d| json!({ "name": d, "kind": "dir" })).collect();
        return Ok(json!({ "path": "", "parent": null, "entries": drives }));
    }
    let folder = Path::new(path);
    let entries = std::fs::read_dir(folder).map_err(|e| tr!("Cannot read the folder: {path}: {error}", path = path, error = e))?;
    let (mut folders, mut files) = (Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            folders.push(json!({ "name": name, "kind": "dir" }));
        } else if let Some(extension) = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).filter(|e| LISTED.contains(&e.as_str())) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push(json!({ "name": name, "kind": extension, "size": size }));
        }
    }
    let by_name = |a: &Value, b: &Value| a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase());
    folders.sort_by(by_name);
    files.sort_by(by_name);
    folders.extend(files);
    // ドライブの直下（`C:\`）の親は、ドライブの一覧（空）。
    let parent = folder.parent().map(|p| p.display().to_string()).unwrap_or_default();
    Ok(json!({ "path": folder.display().to_string(), "parent": parent, "entries": folders }))
}

/// ドライブ（`C:\` など）。
fn drives() -> Vec<String> {
    let mask = unsafe { crate::win32::GetLogicalDrives() };
    (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| format!("{}:\\", (b'A' + i) as char)).collect()
}

/// ソースのファイルを読む。BOM（UTF-8、UTF-16）があれば従い、無ければ UTF-8 として読み、読めなければ ANSI の
/// コード ページ（日本語の Windows では Shift_JIS。古い .NET Framework のソースに多い）として読む。
pub fn read(path: &str) -> Result<Value, String> {
    let file = PathBuf::from(path);
    let size = std::fs::metadata(&file).map_err(|e| tr!("Cannot read the file: {path}: {error}", path = path, error = e))?.len();
    if size > MAX_SOURCE {
        return Err(tr!("The file is too large ({size} bytes; the limit is {limit}): {path}", size = size, limit = MAX_SOURCE, path = path));
    }
    let bytes = std::fs::read(&file).map_err(|e| tr!("Cannot read the file: {path}: {error}", path = path, error = e))?;
    let (text, encoding) = decode(&bytes);
    Ok(json!({ "path": path, "text": text, "encoding": encoding }))
}

/// 文字に戻す。どの符号化で読んだかも返す。
pub fn decode(bytes: &[u8]) -> (String, &'static str) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), "utf-8");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return (String::from_utf16_lossy(&units), "utf-16le");
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return (String::from_utf16_lossy(&units), "utf-16be");
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), "utf-8"),
        Err(_) => (ansi(bytes), "ansi"),
    }
}

/// ANSI のコード ページで読む。
fn ansi(bytes: &[u8]) -> String {
    use crate::win32::MultiByteToWideChar;
    const CP_ACP: u32 = 0;
    let length = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
    let needed = unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    let written = unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, wide.as_mut_ptr(), needed) };
    String::from_utf16_lossy(&wide[..written.max(0) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_order_marks_and_utf8_are_followed() {
        assert_eq!(decode(b"\xEF\xBB\xBFa"), ("a".to_string(), "utf-8"));
        assert_eq!(decode(&[0xFF, 0xFE, b'a', 0, 0xE5, 0x65]), ("a日".to_string(), "utf-16le"));
        assert_eq!(decode("日本".as_bytes()), ("日本".to_string(), "utf-8"));
        // UTF-8 として読めないもの（Shift_JIS の「日本」）は、ANSI のコード ページで読む。
        assert_eq!(decode(&[0x93, 0xFA, 0x96, 0x7B]).1, "ansi");
    }

    #[test]
    fn a_folder_lists_folders_first_and_only_programs() {
        let dir = std::env::temp_dir().join(format!("netdbg-files-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        for name in ["b.exe", "a.DLL", "notes.txt", "crash.dmp", "Form1.cs"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let listed = list(&dir.display().to_string()).unwrap();
        let names: Vec<&str> = listed["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Sub", "a.DLL", "b.exe", "crash.dmp", "Form1.cs"]);
        assert_eq!(listed["parent"].as_str(), dir.parent().map(|p| p.to_str().unwrap()));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(list("").unwrap()["entries"].as_array().unwrap().iter().any(|d| d["name"] == "C:\\"));
    }

    #[test]
    fn large_or_missing_files_are_refused_with_the_reason() {
        assert!(read("Z:\\no\\such\\file.cs").unwrap_err().contains("読めません"));
    }
}
