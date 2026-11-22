//! 画面の言語（D-084）。元の文は英語で、ほかの言語は訳の表（エンジンと同じ `i18n/<言語>.json`）で引く。言語は、
//! エンジンが頁の `<html lang>` に書いたもの（エンジンが OS の表示言語などから決める）。表の無い言語は英語。
//! 言語を足すときは、`i18n/<言語>.json` を作り、エンジンと同じく `CATALOGS` に 1 行足す。

use std::collections::HashMap;
use std::sync::OnceLock;

/// 訳の表（言語の札、英語の文 → 訳の JSON）。エンジンの `src/i18n.rs` と同じ並び。
const CATALOGS: &[(&str, &str)] = &[
    ("ja", include_str!("../../i18n/ja.json")),
    ("zh-Hans", include_str!("../../i18n/zh-Hans.json")),
    ("zh-Hant", include_str!("../../i18n/zh-Hant.json")),
    ("ko", include_str!("../../i18n/ko.json")),
];

/// 頁の言語の札に合う表の札。エンジンは表の札そのもの（`zh-Hant` など）を書くので、まず札で、無ければ主言語で引く。
fn catalog_tag(page: &str) -> Option<&'static str> {
    let primary = page.split(['-', '_']).next().unwrap_or("");
    CATALOGS.iter().find(|(t, _)| t.eq_ignore_ascii_case(page)).or_else(|| CATALOGS.iter().find(|(t, _)| t.eq_ignore_ascii_case(primary))).map(|(t, _)| *t)
}

/// いまの言語の札と表（英語なら `None`）。
fn catalog() -> Option<&'static (&'static str, HashMap<String, String>)> {
    static LOADED: OnceLock<Option<(&'static str, HashMap<String, String>)>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let tag = catalog_tag(&page_lang())?;
            let json = CATALOGS.iter().find(|(t, _)| *t == tag)?.1;
            Some((tag, serde_json::from_str(json).expect("i18n catalog")))
        })
        .as_ref()
}

/// 頁の言語（`<html lang>`）。試験（wasm でない）では日本語。
fn page_lang() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()).map(|e| e.get_attribute("lang").unwrap_or_default()).unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "ja".into()
    }
}

/// いまの言語の札（`ja`、`en`。DAP の locale に使う）。
pub fn tag() -> &'static str {
    catalog().map_or("en", |(tag, _)| *tag)
}

/// 英語の文の、いまの言語の訳（表に無ければ英語のまま）。
pub fn tr(source: &'static str) -> &'static str {
    catalog().and_then(|(_, c)| c.get(source)).map_or(source, String::as_str)
}

/// 訳した文の `{名前}` に値を差し込む（`{{` と `}}` は波かっこそのもの）。
pub fn trf(source: &'static str, values: &[(&str, String)]) -> String {
    let template = tr(source);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_translated_and_filled() {
        assert_eq!(tr("Open"), "開く");
        assert_eq!(tr("Not in the catalog"), "Not in the catalog");
        assert_eq!(trf("{n} of {m}", &[("n", "1".into()), ("m", "2".into())]), "1 of 2");
    }

    #[test]
    fn the_page_language_picks_the_catalog() {
        assert_eq!(catalog_tag("zh-Hant"), Some("zh-Hant"));
        assert_eq!(catalog_tag("zh-hans"), Some("zh-Hans"));
        assert_eq!((catalog_tag("ko"), catalog_tag("ja-JP"), catalog_tag("en"), catalog_tag("fr-FR")), (Some("ko"), Some("ja"), None, None));
    }
}
