//! 成績書の HTML。1 ファイルで完結させる: 画像は data URI、CSS は style 要素。script、link、外部の URL は書かない
//! （ブラウザでそのファイルだけを開けば成績書になる。メールで送っても、ネットにつながらない所でも同じに見える）。
//!
//! 中身はテンプレート（D-083）で書く。既定のテンプレート（言語ごとの `default_template.<言語>.html`。無い言語は英語。D-084）が今の形で、現場はそれを直した
//! テンプレートを渡せる。既定の形（D-082）: 開くと一覧だけを見せ、番号を押すとその項目だけの頁（CSS の `:target`）。
//! 項目は見出しの 1 行と数の 1 行、操作は表で、1 行に「操作した箇所を塗った画面」と「操作のあとの画面」を縮めて並べ、
//! 押すと窓いっぱいに広げる（隠したチェックボックスと CSS）。空の欄は書かない。

use super::template::{self, Sink, Target, Value};
use super::{data, escape, Report};
use crate::base64;

/// 言語ごとの既定のテンプレート（言語を足すときは、ここに 1 行足す。無い言語は英語）。
const DEFAULT_TEMPLATES: &[(&str, &str)] = &[
    ("en", include_str!("default_template.en.html")),
    ("ja", include_str!("default_template.ja.html")),
    ("zh-Hans", include_str!("default_template.zh-Hans.html")),
    ("zh-Hant", include_str!("default_template.zh-Hant.html")),
    ("ko", include_str!("default_template.ko.html")),
];

/// いまの言語の既定のテンプレート。
pub fn default_template() -> &'static str {
    let tag = crate::i18n::tag();
    DEFAULT_TEMPLATES.iter().find(|(t, _)| *t == tag).unwrap_or(&DEFAULT_TEMPLATES[0]).1
}

/// 成績書の HTML。`template` が無ければ既定のテンプレート。`generated` は書いた時刻。
pub fn write(report: &Report, generated: &str, template: Option<&str>) -> Result<String, String> {
    let template = match template {
        Some(template) => template,
        None => default_template(),
    };
    check(template)?;
    let nodes = template::parse(template).map_err(|e| tr!("HTML template: {error}", error = e))?;
    let root = data::build(report, generated);
    let mut sink = Html::default();
    template::render(&nodes, &mut vec![&root], &mut sink).map_err(|e| tr!("HTML template: {error}", error = e))?;
    Ok(sink.0)
}

/// 値を HTML にして書く（文字は逃がす）。
#[derive(Default)]
struct Html(String);

impl Sink for Html {
    fn text(&mut self, text: &str) {
        self.0.push_str(text);
    }

    fn value(&mut self, value: &Value, modifier: Option<&str>) -> Result<(), String> {
        match (value, modifier) {
            (Value::Image(picture), Some("uri")) => self.0.push_str(&format!("data:image/png;base64,{}", base64::encode(&picture.png))),
            // 押すと広げる（label の中のチェックボックスが切り替わり、CSS が画像を窓いっぱいにする）。
            (Value::Image(picture), None) => self.0.push_str(&format!(
                "<label class=\"zoom\"><input type=\"checkbox\"><img src=\"data:image/png;base64,{}\" alt=\"{}\" title=\"{}\"></label>",
                base64::encode(&picture.png),
                escape(&picture.caption),
                escape(&tr!("{caption} (click to enlarge)", caption = picture.caption))
            )),
            (Value::Link { text, target }, None) => {
                let href = match target {
                    Target::Item(index) => format!("#item-{index}"),
                    Target::List(_) => "#top".into(),
                };
                self.0.push_str(&format!("<a href=\"{href}\">{}</a>", escape(text)));
            }
            (value, None) => self.0.push_str(&escape(&value.text())),
            (_, Some(other)) => return Err(tr!("{{{{…:{modifier}}}}} cannot be used (only :uri on a picture can).", modifier = other)),
        }
        Ok(())
    }
}

/// テンプレートが、1 ファイルで完結する HTML になるか。埋める値は逃がすので、確かめるのはテンプレートだけでよい。
pub fn check(template: &str) -> Result<(), String> {
    let lower = template.to_lowercase();
    let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    let mut found: Vec<&str> = ["<script", "<link", "<iframe", "<object", "<embed", "<base", "@import", "javascript:"].into_iter().filter(|p| lower.contains(p)).collect();
    // 外の資源を読む書き方（src、href、url() の http や //）。
    for (pattern, label) in [("src=\"http", "src=\"http…"), ("src='http", "src='http…"), ("src=http", "src=http…"), ("src=\"//", "src=\"//…"), ("url(http", "url(http…"), ("url(\"http", "url(\"http…"), ("url('http", "url('http…"), ("url(//", "url(//…"), ("href=\"http", "href=\"http…"), ("href='http", "href='http…"), ("href=\"//", "href=\"//…")] {
        if compact.contains(pattern) {
            found.push(label);
        }
    }
    // on… の属性（onclick= など。script と同じ）。
    if lower.match_indices(" on").any(|(at, _)| lower[at + 3..].chars().take_while(|c| c.is_ascii_alphabetic()).count() > 0 && lower[at + 3..].trim_start_matches(|c: char| c.is_ascii_alphabetic()).trim_start().starts_with('=')) {
        found.push(tr_str!("on… attributes"));
    }
    if found.is_empty() {
        return Ok(());
    }
    Err(tr!("The HTML template has things that do not stay within one file: {found}. Scripts and outside references (CSS, pictures and links over http) cannot be used.", found = found.join(tr_str!(", "))))
}

/// テンプレートに渡す値（試験と、テンプレートの説明の確かめに）。
#[cfg(test)]
fn root<'a>(report: &'a Report) -> template::Scope<'a> {
    data::build(report, "2026-11-21 09:43:00")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{tree, Actor, Exe, Item, Operation, Picture};
    use super::*;

    pub(crate) fn sample() -> Report {
        let picture = |caption: &str, tail: u8| Some(Picture { caption: caption.into(), png: vec![0x89, b'P', b'N', b'G', tail] });
        let item = Item {
            id: "1-1".into(),
            title: "挨拶 <Hello> & \"名前\"".into(),
            result: Some("OK".into()),
            started: "2026-11-21 09:41:44".into(),
            ended: Some("2026-11-21 09:42:01".into()),
            duration_ms: Some(17_260),
            exe: Some(Exe { path: "C:\\app\\SmokeGui48.exe".into(), version: Some("1.2.3.4".into()), modified: Some("2026-11-20 18:00:00".into()) }),
            operations: vec![
                Operation { marked: picture("操作 1 の箇所", 2), picture: picture("Smoke GUI（操作 1 のあと）", 1), ..Operation::new("09:41:50".into(), Actor::Ai, "ui_invoke greetButton".into()) },
                Operation::new("09:41:55".into(), Actor::Hand, "nameBox に「Taro」を入れた".into()),
            ],
            pictures: vec![Picture { caption: "Smoke GUI（終わり）".into(), png: vec![0x89, b'P', b'N', b'G'] }],
            changes: vec![tree::Change { control: "resultLabel".into(), what: "text".into(), before: String::new(), after: "Hello, Taro".into() }],
            sql: vec!["SELECT * FROM T WHERE id = 1".into()],
            ..Item::default()
        };
        Report { items: vec![item], folder: None }
    }

    fn html(report: &Report) -> String {
        write(report, "2026-11-21 09:43:00", None).unwrap()
    }

    #[test]
    fn the_report_is_one_self_contained_file() {
        let html = html(&sample());
        assert!(html.contains("<img src=\"data:image/png;base64,iVBORw==\""), "画像は data URI");
        for forbidden in ["<script", "<link", "src=\"http", "href=\"http", "url(http", "@import", "{{"] {
            assert!(!html.contains(forbidden), "{forbidden}");
        }
        assert!(html.contains("<style>"));
    }

    #[test]
    fn the_item_carries_its_parts_escaped() {
        let html = html(&sample());
        assert!(html.contains("挨拶 &lt;Hello&gt; &amp; &quot;名前&quot;"));
        assert!(html.contains("<td class=\"who who-ai\">AI</td><td>ui_invoke greetButton</td><td class=\"shot\"><label class=\"zoom\"><input type=\"checkbox\"><img src=\"data:image/png;base64,iVBORwI=\""), "塗った画面、あとの画面の順");
        assert!(html.contains("<td class=\"who who-hand\">手</td><td>nameBox に「Taro」を入れた</td><td class=\"shot\"></td><td class=\"shot\"></td>"));
        assert!(html.contains("<h3>変わったコントロール</h3><ul><li>resultLabel の文字: 「」→「Hello, Taro」</li></ul>"), "一覧は 1 つの ul");
        assert!(html.contains("<div class=\"pictures\"><figure><label class=\"zoom\">"), "始め・写す・終わりの画面は横に並べる");
        assert!(html.contains("1.2.3.4") && html.contains("17.3 秒") && html.contains("OK 1 / NG 0 / 全 1"));
        assert!(html.contains("SELECT * FROM T WHERE id = 1"));
        assert!(html.contains("画面 <b>3</b>"), "画面の数は、操作の分も数える");
    }

    #[test]
    fn the_list_opens_first_and_each_item_is_its_own_page() {
        let mut report = sample();
        report.items.push(report.items[0].clone());
        let html = html(&report);
        assert!(html.contains("section.item { display: none;") && html.contains("section.item:target, #all:target ~ section.item { display: block; }"), "開くと一覧だけ、番号でその項目だけ");
        assert!(html.contains("<td><a href=\"#item-1\">1-1</a></td>"), "一覧の番号から項目へ");
        assert!(html.contains("id=\"item-0\">\n<nav class=\"pager\"><a href=\"#top\">← 一覧に戻る</a><a href=\"#item-1\">次の項目 ›</a></nav>"));
        assert!(html.contains("id=\"item-1\">\n<nav class=\"pager\"><a href=\"#top\">← 一覧に戻る</a><a href=\"#item-0\">‹ 前の項目</a></nav>"));
        assert!(html.contains("<a href=\"#all\">すべての項目を並べて見る</a>") && html.contains("<div id=\"all\"></div>"));
    }

    #[test]
    fn empty_parts_are_left_out() {
        let html = html(&sample());
        assert!(!html.contains("<h3>メッセージ</h3>") && !html.contains("<h3>例外</h3>"), "無いものの欄は書かない");
        assert!(html.contains("<h3>SQL</h3>"));
        let mut empty = sample();
        empty.items[0].operations.clear();
        assert!(super::write(&empty, "", None).unwrap().contains("（記録された操作はありません）"));
    }

    #[test]
    fn a_site_template_is_filled_with_the_same_names() {
        let site = "<h1>{{プログラム名}} 試験結果</h1>{{#項目}}<p>{{番号}}={{結果}} {{所要秒}}秒 <img src=\"{{#操作}}{{#箇所画像}}{{箇所画像:uri}}{{/箇所画像}}{{/操作}}\"></p>{{/項目}}";
        let html = write(&sample(), "", Some(site)).unwrap();
        assert_eq!(html, "<h1>SmokeGui48.exe 試験結果</h1><p>1-1=OK 17.26秒 <img src=\"data:image/png;base64,iVBORwI=\"></p>");
        assert!(write(&sample(), "", Some("{{番号違い}}")).unwrap_err().contains("{{番号違い}} が分かりません"));
    }

    #[test]
    fn templates_that_reach_outside_are_refused() {
        for bad in ["<script>x</script>", "<link rel=stylesheet href=a.css>", "<img src=\"https://x/a.png\">", "<div style=\"background: url( http://x )\">", "<a href=\"http://x\">", "<button onclick=\"x()\">", "<img src = \"//cdn/a.png\">"] {
            assert!(check(bad).is_err(), "{bad}");
        }
        for (_, template) in DEFAULT_TEMPLATES {
            assert!(check(template).is_ok(), "既定のテンプレートは通る");
        }
        assert!(check("<p>online = 1, ok</p>").is_ok(), "on で始まる語だけでは断らない");
    }

    #[test]
    fn every_name_the_default_template_uses_is_described() {
        // 既定のテンプレートの名前は、すべて値にある（説明の一覧と値がずれていない）。
        let report = sample();
        let root = root(&report);
        let item = match root.get("項目") {
            Some(Value::List(items)) => items[0].clone(),
            _ => panic!(),
        };
        let operation = match item.get("操作") {
            Some(Value::List(items)) => items[0].clone(),
            _ => panic!(),
        };
        let described: String = data::names().iter().flat_map(|(_, names)| names.iter().map(|(name, _)| *name)).collect::<Vec<_>>().join(" / ");
        for name in root.names().chain(item.names()).chain(operation.names()) {
            assert!(described.split(" / ").any(|d| d == name), "{name} の説明がありません");
            assert!(data::ALIASES.iter().any(|(_, english)| *english == name) || name == "exe", "{name} に日本語の別名がありません");
        }
    }

    #[test]
    fn the_report_follows_the_language_and_japanese_names_are_aliases() {
        let html = crate::i18n::with_lang("en", || write(&sample(), "", None).unwrap());
        assert!(html.contains("<h1>Test Report</h1>") && html.contains("<th>Operated spot</th>"));
        assert!(html.contains("<span class=\"result ok\">OK</span>"));
        let japanese = crate::i18n::with_lang("en", || write(&sample(), "", Some("{{#項目}}{{番号}}:{{id}}{{/項目}}")).unwrap());
        assert_eq!(japanese, "1-1:1-1", "日本語の名前と英語の名前のどちらでも");
        assert!(crate::i18n::with_lang("fr", || write(&sample(), "", None).unwrap()).contains("<h1>Test Report</h1>"), "表の無い言語は英語");
    }
}
