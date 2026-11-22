//! 障害票用の環境（段階 12）: 対象が動いている環境のうち、障害の再現に要るもの。エンジンの側で読む（対象を止めて
//! 評価しない）: ビット数、ランタイムの版、利用者、マシン、OS、カルチャ、タイムゾーン、設定のファイルの接続文字列
//! （パスワードは隠す）。ここは、設定のファイルを解く部分と、Windows から読む部分。

use crate::win32::*;
use serde_json::{json, Value};
use std::ffi::c_void;
use std::path::Path;

/// 接続文字列の、隠す値のキー（大文字と小文字は見ない）。
const SECRET_KEYS: [&str; 4] = ["password", "pwd", "user password", "secret"];

/// 設定のファイルの接続文字列。
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionString {
    pub name: String,
    /// パスワードを隠したもの。
    pub value: String,
    pub provider: Option<String>,
    /// 読んだファイルの名前。
    pub file: String,
}

/// 接続文字列のパスワードの値を `***` にする（`Password=x;` の形。引用符で囲んだ値も）。
pub fn mask_secrets(connection_string: &str) -> String {
    let mut out = Vec::new();
    for part in split_parts(connection_string) {
        match part.split_once('=') {
            Some((key, _)) if SECRET_KEYS.contains(&key.trim().to_lowercase().as_str()) => out.push(format!("{}=***", key.trim_end())),
            _ => out.push(part.to_string()),
        }
    }
    out.join(";")
}

/// `;` で分ける（`"…"` と `'…'` の中の `;` は分けない）。
fn split_parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quote = None;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, ';') => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// .NET Framework の設定のファイル（`app.exe.config`）の `<connectionStrings>` の `<add>`。
pub fn from_app_config(xml: &str, file: &str) -> Vec<ConnectionString> {
    let Some(section) = between(xml, "<connectionStrings", "</connectionStrings>") else { return Vec::new() };
    let mut found = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.find("<add") {
        rest = &rest[at + 4..];
        let element = &rest[..rest.find('>').unwrap_or(rest.len())];
        if let (Some(name), Some(value)) = (attribute(element, "name"), attribute(element, "connectionString")) {
            found.push(ConnectionString { name, value: mask_secrets(&value), provider: attribute(element, "providerName"), file: file.to_string() });
        }
    }
    found
}

/// .NET の `appsettings.json` の `ConnectionStrings`。注釈（`//`）があれば、行の注釈だけ外して読む。
pub fn from_appsettings(json_text: &str, file: &str) -> Vec<ConnectionString> {
    let parsed: Option<Value> = serde_json::from_str(json_text).ok().or_else(|| {
        let without_comments: String = json_text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        serde_json::from_str(&without_comments).ok()
    });
    let Some(Value::Object(strings)) = parsed.and_then(|p| p.get("ConnectionStrings").cloned()) else { return Vec::new() };
    strings
        .into_iter()
        .filter_map(|(name, value)| Some(ConnectionString { name, value: mask_secrets(value.as_str()?), provider: None, file: file.to_string() }))
        .collect()
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)?;
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// XML の要素の中の属性の値（実体参照を戻す）。
fn attribute(element: &str, name: &str) -> Option<String> {
    let mut rest = element;
    loop {
        let at = rest.find(name)?;
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        rest = &rest[at + name.len()..];
        let trimmed = rest.trim_start();
        if before_ok && trimmed.starts_with('=') {
            let value = trimmed[1..].trim_start();
            let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let end = value[1..].find(quote)?;
            return Some(value[1..1 + end].replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"));
        }
    }
}

/// プログラムのフォルダーの設定のファイルから、接続文字列（`program.config`、`appsettings.json`）。
pub fn connection_strings(program: &Path) -> Vec<ConnectionString> {
    let mut found = Vec::new();
    let config = format!("{}.config", program.display());
    if let Ok(text) = std::fs::read_to_string(&config) {
        found.extend(from_app_config(&text, &file_name(&config)));
    }
    if let Some(folder) = program.parent() {
        let settings = folder.join("appsettings.json");
        if let Ok(text) = std::fs::read_to_string(&settings) {
            found.extend(from_appsettings(&text, "appsettings.json"));
        }
    }
    found
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// ランタイムの版を、読み込んだ System.Private.CoreLib / mscorlib のパスから（`.NET 10.0.5`、`.NET Framework 4.8.1`）。
pub fn runtime_description(corelib: &str) -> String {
    let path = Path::new(corelib);
    let folder = path.parent().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if corelib.to_lowercase().ends_with("mscorlib.dll") {
        return match framework_release() {
            Some(version) => format!(".NET Framework {version} ({folder})"),
            None => format!(".NET Framework ({folder})"),
        };
    }
    if folder.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!(".NET {folder}")
    } else {
        format!(".NET ({corelib})")
    }
}

/// .NET Framework 4.x の版（レジストリの Release）。
fn framework_release() -> Option<&'static str> {
    let release = registry_dword(r"SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full", "Release")?;
    // https://learn.microsoft.com/dotnet/framework/migration-guide/how-to-determine-which-versions-are-installed
    Some(match release {
        533_320.. => "4.8.1",
        528_040.. => "4.8",
        461_808.. => "4.7.2",
        461_308.. => "4.7.1",
        460_798.. => "4.7",
        394_802.. => "4.6.2",
        _ => "4.6.1 or earlier",
    })
}

fn registry_dword(key: &str, value: &str) -> Option<u32> {
    let (key, value) = (wide(key), wide(value));
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    (unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut c_void, &mut size) } == 0).then_some(data)
}

fn registry_string(key: &str, value: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(value));
    let mut buffer = vec![0u16; 512];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr() as *mut c_void, &mut size) };
    (status == 0).then(|| from_wide(&buffer[..(size as usize / 2)])).filter(|s| !s.is_empty())
}

/// Windows から読む環境（利用者、マシン、OS、カルチャ、タイムゾーン）。
pub fn machine_facts() -> Value {
    let env = |name: &str| std::env::var(name).ok();
    let user = match (env("USERDOMAIN"), env("USERNAME")) {
        (Some(domain), Some(name)) => Some(format!("{domain}\\{name}")),
        (None, name) => name,
        (_, None) => None,
    };
    let os_key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let os = [registry_string(os_key, "ProductName"), registry_string(os_key, "DisplayVersion"), registry_string(os_key, "CurrentBuild").map(|b| format!("build {b}"))].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let mut locale = vec![0u16; 85];
    let culture = (unsafe { GetUserDefaultLocaleName(locale.as_mut_ptr(), locale.len() as i32) } > 0).then(|| from_wide(&locale));
    let mut ui = vec![0u16; 85];
    let ui_language = (unsafe { LCIDToLocaleName(u32::from(GetUserDefaultUILanguage()), ui.as_mut_ptr(), ui.len() as i32, 0) } > 0).then(|| from_wide(&ui));
    json!({
        "user": user,
        "machine": env("COMPUTERNAME"),
        "os": os,
        "culture": culture,
        "uiLanguage": ui_language,
        "timeZone": registry_string(r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation", "TimeZoneKeyName"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_masked_and_other_parts_kept() {
        assert_eq!(mask_secrets("Data Source=db;User Id=app;Password=s3cr;et"), "Data Source=db;User Id=app;Password=***;et");
        assert_eq!(mask_secrets("Server=x;PWD=\"a;b\";Database=y"), "Server=x;PWD=***;Database=y");
        assert_eq!(mask_secrets("Server=x;Integrated Security=true"), "Server=x;Integrated Security=true");
    }

    #[test]
    fn connection_strings_are_read_from_app_config_and_appsettings() {
        let xml = r#"<configuration><connectionStrings>
            <add name="Main" connectionString="Data Source=db;Password=p" providerName="System.Data.SqlClient" />
            <add connectionString='Server=a&amp;b' name='Other'/>
        </connectionStrings></configuration>"#;
        let found = from_app_config(xml, "app.exe.config");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Main");
        assert_eq!(found[0].value, "Data Source=db;Password=***");
        assert_eq!(found[0].provider.as_deref(), Some("System.Data.SqlClient"));
        assert_eq!(found[1].value, "Server=a&b");
        let json = "{\n  // comment\n  \"ConnectionStrings\": { \"Db\": \"Host=h;Pwd=x\" }\n}";
        let found = from_appsettings(json, "appsettings.json");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "Host=h;Pwd=***");
    }

    #[test]
    fn the_runtime_is_named_from_the_corelib_folder() {
        assert_eq!(runtime_description(r"C:\Program Files\dotnet\shared\Microsoft.NETCore.App\10.0.5\System.Private.CoreLib.dll"), ".NET 10.0.5");
        assert!(runtime_description(r"C:\Windows\Microsoft.NET\Framework\v4.0.30319\mscorlib.dll").starts_with(".NET Framework"));
    }
}
