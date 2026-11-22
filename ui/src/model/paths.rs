//! Windows のパスの小さな道具（ブラウザの中には std::path の Windows の扱いが無い）。

/// 最後の部分（ファイルの名前）。
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// フォルダーとファイルの名前をつなぐ。
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        return name.to_string();
    }
    if folder.ends_with(['\\', '/']) {
        format!("{folder}{name}")
    } else {
        format!("{folder}\\{name}")
    }
}

/// フォルダー（最後の部分を除いたもの）。
pub fn parent(path: &str) -> &str {
    match path.trim_end_matches(['\\', '/']).rfind(['\\', '/']) {
        Some(at) => &path[..at],
        None => "",
    }
}

/// 引数の文字列を、空白で区切る（`"` で囲んだ部分は 1 つ）。
pub fn split_arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_folders_and_joins() {
        assert_eq!(file_name("C:\\app\\Program.cs"), "Program.cs");
        assert_eq!(parent("C:\\app\\Program.cs"), "C:\\app");
        assert_eq!(join("C:\\", "app"), "C:\\app");
        assert_eq!(join("C:\\app", "a.exe"), "C:\\app\\a.exe");
    }

    #[test]
    fn arguments_are_split_with_quotes() {
        assert_eq!(split_arguments(r#"a "b c" "" d"#), vec!["a", "b c", "", "d"]);
        assert!(split_arguments("  ").is_empty());
    }
}
