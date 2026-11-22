//! 書き出す先のフォルダー（保存先）。成績書とカバレッジの XML で同じ決まりにする。

use serde_json::Value;
use std::path::PathBuf;

/// 引数の保存先（`what` は、エラーに出す名前）。絶対パスだけを受ける（エンジンの作業フォルダーからの相対は、思った
/// 所にならない）。空なら `None`。
pub fn argument(value: Option<&Value>, what: &str) -> Result<Option<PathBuf>, String> {
    let Some(folder) = value.and_then(Value::as_str).map(str::trim).filter(|f| !f.is_empty()) else { return Ok(None) };
    let path = PathBuf::from(folder);
    if !path.is_absolute() {
        return Err(tr!("{what} must be a folder written from the drive (for example D:\\tests\\results): {folder}", what = what, folder = folder));
    }
    Ok(Some(path))
}

/// 既定の保存先: `%LOCALAPPDATA%\netdbg\<sub>`（アプリのフォルダー。Program Files のこともある。は汚さない）。
pub fn app_data(sub: &str) -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("netdbg").join(sub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_absolute_folders_are_accepted() {
        assert_eq!(argument(Some(&json!("D:\\out")), "保存先"), Ok(Some(PathBuf::from("D:\\out"))));
        assert_eq!(argument(Some(&json!("  ")), "保存先"), Ok(None));
        assert_eq!(argument(None, "保存先"), Ok(None));
        assert!(argument(Some(&json!("out")), "保存先").unwrap_err().starts_with("保存先は、ドライブから"));
    }
}
