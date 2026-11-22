//! 表示の言語（D-084）。元の文は英語で書き、ほかの言語は、言語ごとの訳の表（`i18n/<言語>.json`。英語の文 → 訳）で
//! 引く。表が無い言語は英語にする。言語を足すときは、`i18n/<言語>.json` を作って `CATALOGS` に 1 行足すだけで、
//! 呼ぶ所は変えない。訳の表はエンジンと専用の画面（`ui`）で 1 つを使う。
//!
//! 言語を決める順:
//! 1. 環境変数 `NETDBG_LANG`（`ja`、`en` など。試験と、利用者が明示したいとき）。
//! 2. DAP の `initialize` の `locale`（VS Code は自分の表示言語を送る。拡張の言語と揃う）。
//! 3. OS の表示言語（`GetUserDefaultUILanguage`）。
//!
//! 文の書き方: `tr!("Cannot read {path}.", path = path.display())`。差し込む所は名前で書く（訳で順を入れ替えられる
//! ように）。`{{` と `}}` は波かっこそのもの。訳の表の漏れと、差し込む名前の食い違いは、単体試験がソースを読んで
//! 見つける（`tests::every_text_has_a_translation`）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// 訳の表（言語の札、英語の文 → 訳の JSON）。英語は元の文なので表を持たない。札は `<html lang>` にもそのまま使う。
/// 中国語は文字の種類で分ける（`catalog_tag`）。
const CATALOGS: &[(&str, &str)] = &[
    ("ja", include_str!("../i18n/ja.json")),
    ("zh-Hans", include_str!("../i18n/zh-Hans.json")),
    ("zh-Hant", include_str!("../i18n/zh-Hant.json")),
    ("ko", include_str!("../i18n/ko.json")),
];

/// 元の言語。
pub const SOURCE: &str = "en";

/// 環境変数の名前。
pub const VARIABLE: &str = "NETDBG_LANG";

/// いまの言語（0 は英語、n は `CATALOGS[n - 1]`。`usize::MAX` はまだ決めていない）。
static CURRENT: AtomicUsize = AtomicUsize::new(usize::MAX);

#[cfg(test)]
thread_local! {
    /// 試験のスレッドだけの言語（試験は並んで動くので、全体の言語を変えずに別の言語を確かめる）。
    static OVERRIDE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// 試験: このスレッドだけ `tag` の言語にして `work` を動かす。
#[cfg(test)]
pub fn with_lang<T>(tag: &str, work: impl FnOnce() -> T) -> T {
    OVERRIDE.with(|o| o.set(Some(index_of(tag))));
    let result = work();
    OVERRIDE.with(|o| o.set(None));
    result
}

fn current() -> usize {
    #[cfg(test)]
    if let Some(index) = OVERRIDE.with(std::cell::Cell::get) {
        return index;
    }
    match CURRENT.load(Ordering::Relaxed) {
        usize::MAX => {
            let index = index_of(&detect());
            CURRENT.store(index, Ordering::Relaxed);
            index
        }
        index => index,
    }
}

/// 言語の札（`ja-JP`、`en-US`、`zh-cn` など）に合う表の番号。合う表が無ければ英語（0）。
fn index_of(tag: &str) -> usize {
    let wanted = catalog_tag(tag);
    CATALOGS.iter().position(|(t, _)| t.eq_ignore_ascii_case(&wanted)).map_or(0, |i| i + 1)
}

/// 言語の札を、表の札の形にする（小文字）。ふつうは主言語（`ko-KR` → `ko`）。中国語は文字の種類で分け、書いて
/// いなければ地域で決める（台湾、香港、マカオは繁体字、ほかは簡体字）。
fn catalog_tag(tag: &str) -> String {
    let parts: Vec<String> = tag.trim().split(['-', '_']).map(str::to_ascii_lowercase).collect();
    let primary = parts.first().cloned().unwrap_or_default();
    if primary != "zh" {
        return primary;
    }
    let rest = &parts[1..];
    let traditional = !rest.iter().any(|p| p == "hans") && rest.iter().any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo"));
    if traditional { "zh-hant" } else { "zh-hans" }.into()
}

/// いまの言語で文書（成績書の Excel など）に使うフォント。漢字は言語ごとに字形が違うので、その言語のフォントにする。
pub fn font() -> &'static str {
    match tag() {
        "ja" => "Yu Gothic UI",
        "zh-Hans" => "Microsoft YaHei UI",
        "zh-Hant" => "Microsoft JhengHei UI",
        "ko" => "Malgun Gothic",
        _ => "Segoe UI",
    }
}

/// いまの言語の札（`ja`、`en`。HTML の lang、DAP の locale に使う）。
pub fn tag() -> &'static str {
    match current() {
        0 => SOURCE,
        index => CATALOGS[index - 1].0,
    }
}

/// 言語を決める（表が無い言語は英語）。
pub fn set(tag: &str) {
    CURRENT.store(index_of(tag), Ordering::Relaxed);
}

/// DAP の `initialize` の `locale`。環境変数で決めてあれば、そちらを使う。
pub fn from_client(locale: Option<&str>) {
    if std::env::var_os(VARIABLE).is_some() {
        return;
    }
    if let Some(locale) = locale.filter(|l| !l.trim().is_empty()) {
        set(locale);
    }
}

fn detect() -> String {
    if let Ok(value) = std::env::var(VARIABLE) {
        return value;
    }
    // 単体試験は、既存の試験の文（日本語）で確かめる。英語などは、試験ごとに `with_lang` で切り替える。
    if cfg!(test) {
        return "ja".into();
    }
    os()
}

/// OS の表示言語の札（`ja-JP` など）。
fn os() -> String {
    let mut name = [0u16; 85];
    let length = unsafe { crate::win32::LCIDToLocaleName(u32::from(crate::win32::GetUserDefaultUILanguage()), name.as_mut_ptr(), name.len() as i32, 0) };
    if length <= 1 {
        return SOURCE.into();
    }
    String::from_utf16_lossy(&name[..length as usize - 1])
}

/// 表を読んだもの（言語ごとに 1 回）。
fn catalog(index: usize) -> &'static HashMap<String, String> {
    static LOADED: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();
    let loaded = LOADED.get_or_init(|| CATALOGS.iter().map(|(tag, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("i18n/{tag}.json: {e}"))).collect());
    &loaded[index - 1]
}

/// 英語の文の、いまの言語の訳（表に無ければ英語のまま）。
pub fn text(source: &'static str) -> &'static str {
    match current() {
        0 => source,
        index => catalog(index).get(source).map_or(source, String::as_str),
    }
}

/// 差し込む名前の形（英数字と _）。`{ … }` のような、名前でない波かっこは、そのままの文字として扱う。
fn is_name(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `{名前}` に値を差し込む。`{{` と `}}` は波かっこそのもの。知らない名前は、そのまま残す。
pub fn fill(template: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        if after.starts_with("{{") || after.starts_with("}}") {
            out.push_str(&after[..1]);
            rest = &after[2..];
            continue;
        }
        if let Some(name) = after.strip_prefix('{').and_then(|a| a.find('}').map(|end| &a[..end])) {
            if let Some((_, value)) = values.iter().find(|(n, _)| *n == name) {
                out.push_str(value);
                rest = &after[name.len() + 2..];
                continue;
            }
        }
        out.push_str(&after[..1]);
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

/// 文が、訳した文の形（`template` の `{名前}` の所は何でもよい）に合うか。作った文の種類を見分けるのに使う
/// （「見つからない」の誤りなら別の手を試す、など）。言語によらず、いまの言語の文の形で見分ける。
pub fn matches(text: &str, template: &str) -> bool {
    let mut pieces = Vec::new();
    let mut rest = template;
    let mut from = 0;
    while let Some(start) = rest[from..].find('{').map(|s| s + from) {
        let Some(end) = rest[start..].find('}') else { break };
        if !is_name(&rest[start + 1..start + end]) {
            from = start + 1;
            continue;
        }
        pieces.push(&rest[..start]);
        rest = &rest[start + end + 1..];
        from = 0;
    }
    pieces.push(rest);
    let (first, last) = (pieces[0], pieces[pieces.len() - 1]);
    if pieces.len() == 1 {
        return text == first;
    }
    if !text.starts_with(first) || !text.ends_with(last) || text.len() < first.len() + last.len() {
        return false;
    }
    let mut middle = &text[first.len()..text.len() - last.len()];
    for piece in &pieces[1..pieces.len() - 1] {
        match middle.find(piece) {
            Some(at) => middle = &middle[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// 言語に合わせた文（`String`）。元の文は英語。差し込む値は名前で渡す: `tr!("{n} items", n = count)`。
#[macro_export]
macro_rules! tr {
    ($source:literal $(,)?) => {
        $crate::i18n::fill($crate::i18n::text($source), &[])
    };
    ($source:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::text($source), &[$((stringify!($name), format!("{}", $value))),+])
    };
}

/// 言語に合わせた、差し込みの無い文（`&'static str`）。
#[macro_export]
macro_rules! tr_str {
    ($source:literal $(,)?) => {
        $crate::i18n::text($source)
    };
}

/// 文の中の `{名前}`（`{{` `}}` は除く）。訳の表の確かめに使う。
#[cfg(test)]
pub fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let unescaped = text.replace("{{", "").replace("}}", "");
    let mut rest = unescaped.as_str();
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else { break };
        let name = &rest[start + 1..start + end];
        if is_name(name) {
            out.insert(name.to_string());
        }
        rest = &rest[start + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_without_a_catalog_are_english() {
        assert_eq!((index_of("ja"), index_of("ja-JP"), index_of("JA_jp"), index_of("en-US"), index_of("fr-FR"), index_of("")), (1, 1, 1, 0, 0, 0));
    }

    #[test]
    fn chinese_is_chosen_by_script_then_region() {
        let tag_of = |t: &str| match index_of(t) {
            0 => SOURCE,
            index => CATALOGS[index - 1].0,
        };
        for simplified in ["zh-CN", "zh-cn", "zh-SG", "zh", "zh-Hans", "zh-Hans-HK"] {
            assert_eq!(tag_of(simplified), "zh-Hans", "{simplified}");
        }
        for traditional in ["zh-TW", "zh-tw", "zh-HK", "zh-MO", "zh-Hant", "zh-Hant-TW", "zh_TW"] {
            assert_eq!(tag_of(traditional), "zh-Hant", "{traditional}");
        }
        assert_eq!((tag_of("ko-KR"), tag_of("ko")), ("ko", "ko"));
    }

    #[test]
    fn values_are_filled_by_name() {
        let values = [("path", "C:\\a".to_string()), ("n", "3".to_string())];
        assert_eq!(fill("{n} files in {path} {{x}} {unknown}", &values), "3 files in C:\\a {x} {unknown}");
        assert_eq!(placeholders("{n} files in {path} {{x}}").into_iter().collect::<Vec<_>>(), ["n", "path"]);
    }

    #[test]
    fn messages_are_matched_against_the_template_form() {
        assert!(matches("Method 'Foo' is not found.", "Method '{name}' is not found."));
        assert!(matches("メソッド 'Foo' が見つかりません。", "メソッド '{name}' が見つかりません。"));
        assert!(!matches("Method 'Foo' failed.", "Method '{name}' is not found."));
        assert!(matches("a 1 b 2 c", "a {x} b {y} c") && !matches("a 1 c 2 b", "a {x} b {y} c"));
    }

    #[test]
    fn the_language_is_chosen_per_thread_in_tests() {
        let count = 3;
        assert_eq!(tag(), "ja");
        assert_eq!(with_lang("en", || tr!("{n} items", n = count)), "3 items");
        assert_eq!(with_lang("fr", tag), "en", "表の無い言語は英語");
    }

    /// ソース（エンジンと画面）の `tr!` / `tr_str!` / 画面の `tr(` `trf(` の英語の文を集める。
    fn sources() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        let mut pending = vec![root.join("src"), root.join("ui").join("src")];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs") {
                    files.push(path);
                }
            }
        }
        let mut found = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for marker in ["tr!(", "tr_str!(", "tr(", "trf("] {
                let mut rest = text.as_str();
                while let Some(at) = rest.find(marker) {
                    // 名前の途中（attr(" など）は除く。
                    let boundary = rest[..at].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
                    let after = &rest[at + marker.len()..];
                    // 改行して書いた呼び出し（tr!(\n "…"）も拾う。文字列でなければ（tr(x) など）飛ばす。
                    let literal = after.trim_start();
                    if !boundary || !literal.starts_with('"') {
                        rest = after;
                        continue;
                    }
                    let literal = &literal[1..];
                    let mut value = String::new();
                    let mut chars = literal.chars();
                    while let Some(c) = chars.next() {
                        match c {
                            '"' => break,
                            '\\' => match chars.next() {
                                Some('n') => value.push('\n'),
                                Some('t') => value.push('\t'),
                                Some(other) => value.push(other),
                                None => break,
                            },
                            c => value.push(c),
                        }
                    }
                    found.push((value, file.display().to_string()));
                    rest = &rest[at + marker.len()..];
                }
            }
        }
        found
    }

    #[test]
    fn every_text_has_a_translation() {
        let used = sources();
        for (tag, json) in CATALOGS {
            let catalog: HashMap<String, String> = serde_json::from_str(json).unwrap();
            for (source, file) in &used {
                let translated = catalog.get(source).unwrap_or_else(|| panic!("i18n/{tag}.json に訳がありません: {source:?}（{file}）"));
                assert_eq!(placeholders(source), placeholders(translated), "i18n/{tag}.json の差し込む名前が違います: {source:?}");
            }
            let unused: Vec<&String> = catalog.keys().filter(|k| !used.iter().any(|(u, _)| u == *k)).collect();
            assert!(unused.is_empty(), "i18n/{tag}.json に、使っていない文があります: {unused:?}");
        }
    }
}
