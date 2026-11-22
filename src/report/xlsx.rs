//! 成績書の Excel（.xlsx）。中身はテンプレート（D-083）で書く: 現場の .xlsx のセルに印（`{{番号}}`、繰り返す行、
//! 画像のセル）を書いておくと、`fill` が値で埋める。既定のテンプレート（`default_template`）が今の形（D-082）:
//! - シート「成績書」: 1 項目が 1 行。番号は、その項目のシートへのハイパーリンク。
//! - シート `{{番号}}`（項目ごとに複製）: 表題、成績書のその行へ戻るハイパーリンク、1 操作 1 行（操作した箇所を塗った
//!   画面とあとの画面）、始め・写す・終わりの画面。
//! - 1 つのセルに収まらない文字（Excel の上限は 32,767 文字）は切り捨てず、改行の所で分けて、すぐ下の「続き」の行に書く。
//!
//! 依存を増やさないよう、ZIP（`zip`）と XML（`xml`）と SpreadsheetML の部品は自前で書く。

mod fill;

use super::{data, escape, zip, Report};
use std::fmt::Write as _;

/// 1 つのセルに書く文字の上限（UTF-16 の数。Excel の上限 32,767 より少し小さく、続きの印の分を残す）。
const CELL_LIMIT: usize = 32_000;
/// 分けたセルの終わりに付ける印のために空けておく分（UTF-16 の数。どの言語の印もこれに収める）。
const CONTINUED_RESERVE: usize = 64;

/// 分けたセルの終わりに付ける印（いまの言語）。
fn continued() -> String {
    tr!("\n(continued on the next row)")
}
/// シートの名前の長さの上限（Excel。UTF-16 の数）。
const SHEET_NAME_LIMIT: usize = 31;

/// 貼る画像の、縮めたあとの大きさ（画素）の上限。
const THUMBNAIL_WIDTH: u32 = 360;
const THUMBNAIL_HEIGHT: u32 = 240;
/// 画素から、Excel の長さ（EMU）。
const EMU_PER_PIXEL: u64 = 9525;
/// 画像の列の幅（文字の幅はおよそ 7 画素）。
const PICTURE_COLUMN_WIDTH: u32 = THUMBNAIL_WIDTH / 7 + 2;

/// 成績書の .xlsx。`template` が無ければ既定のテンプレート。`generated` は書いた時刻。
pub fn write(report: &Report, generated: &str, template: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let root = data::build(report, generated);
    let default;
    let template = match template {
        Some(template) => template,
        None => {
            default = default_template();
            &default
        }
    };
    fill::fill(template, &root).map_err(|e| tr!("Excel template: {error}", error = e))
}

/// テンプレートを作るときの 1 枚。セルは（文字、書式）。
struct TemplateSheet {
    name: &'static str,
    widths: Vec<u32>,
    /// 見出しの行（0 始まり。ここまでを、スクロールしても残す）。
    header: usize,
    rows: Vec<Vec<(&'static str, u8)>>,
}

/// 既定のテンプレート（今の形）。現場は、これを書き出して直して使える。
pub fn default_template() -> Vec<u8> {
    // 書式: 1 見出し、2 中身（折り返し、上寄せ）、3 リンク、4 表題。
    let list = TemplateSheet {
        name: tr_str!("Test Report"),
        widths: vec![10, 28, 7, 19, 19, 12, 40, 14, 19, 60, 50, 60, 40, 40, 10, 30],
        header: 0,
        rows: vec![
            [
                tr_str!("No."),
                tr_str!("Item"),
                tr_str!("Result"),
                tr_str!("Started at"),
                tr_str!("Ended at"),
                tr_str!("Duration (s)"),
                "exe",
                tr_str!("exe version"),
                tr_str!("exe modified"),
                tr_str!("Actions"),
                tr_str!("Changed controls"),
                "SQL",
                tr_str!("Messages"),
                tr_str!("Exceptions"),
                tr_str!("Pictures"),
                tr_str!("Remarks"),
            ]
            .map(|h| (h, 1))
            .to_vec(),
            vec![
                ("{{#items}}{{itemLink}}", 3),
                ("{{title}}", 2),
                ("{{result}}", 2),
                ("{{started}}", 2),
                ("{{ended}}", 2),
                ("{{seconds}}", 2),
                ("{{exe}}", 2),
                ("{{exeVersion}}", 2),
                ("{{exeModified}}", 2),
                ("{{operationText}}", 2),
                ("{{changeText}}", 2),
                ("{{sqlText}}", 2),
                ("{{messageText}}", 2),
                ("{{exceptionText}}", 2),
                ("{{pictureCount}}", 2),
                ("{{note}}{{/items}}", 2),
            ],
        ],
    };
    let item = TemplateSheet {
        name: "{{id}}",
        widths: vec![5, 10, 6, 60, PICTURE_COLUMN_WIDTH, PICTURE_COLUMN_WIDTH],
        header: 2,
        rows: vec![
            vec![(tr_str!("{{id}} {{title}} ({{result}}{{#duration}} · {{duration}}{{/duration}})"), 4)],
            vec![("{{listLink}}", 3)],
            ["#", tr_str!("Time"), tr_str!("Who"), tr_str!("Action"), tr_str!("Operated spot"), tr_str!("After the action")].map(|h| (h, 1)).to_vec(),
            vec![("{{#operations}}{{index}}", 2), ("{{time}}", 2), ("{{actor}}", 2), ("{{text}}", 2), ("{{spotPicture}}", 2), ("{{afterPicture}}{{/operations}}", 2)],
            vec![("{{^operations}}", 2), ("", 2), ("", 2), (tr_str!("(no actions were recorded){{/operations}}"), 2)],
            vec![(tr_str!("{{#pictures}}Picture"), 2), ("", 2), ("", 2), ("{{caption}}", 2), ("{{picture}}{{/pictures}}", 2)],
        ],
    };    let sheets = [list, item];
    let mut zip = zip::Writer::default();
    let mut types = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>");
    let mut workbook = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>");
    let mut rels = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for (index, sheet) in sheets.iter().enumerate() {
        let number = index + 1;
        let _ = write!(types, "<Override PartName=\"/xl/worksheets/sheet{number}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>");
        let _ = write!(workbook, "<sheet name=\"{}\" sheetId=\"{number}\" r:id=\"rId{number}\"/>", escape(sheet.name));
        let _ = write!(rels, "<Relationship Id=\"rId{number}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{number}.xml\"/>");
    }
    types.push_str("<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>");
    workbook.push_str("</sheets></workbook>");
    let _ = write!(rels, "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>", sheets.len() + 1);
    zip.add("[Content_Types].xml", types.as_bytes());
    zip.add("_rels/.rels", ROOT_RELS.as_bytes());
    zip.add("xl/workbook.xml", workbook.as_bytes());
    zip.add("xl/_rels/workbook.xml.rels", rels.as_bytes());
    // フォントは表示の言語のもの (漢字は言語ごとに字形が違う)。STYLES は日本語のフォントで書いてある。
    let styles = STYLES.replace("Yu Gothic UI", crate::i18n::font());
    zip.add("xl/styles.xml", styles.as_bytes());
    for (index, sheet) in sheets.iter().enumerate() {
        zip.add(&format!("xl/worksheets/sheet{}.xml", index + 1), template_sheet_xml(sheet, index == 0).as_bytes());
    }
    zip.finish()
}

fn template_sheet_xml(sheet: &TemplateSheet, selected: bool) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">");
    let frozen = sheet.header + 1;
    let _ = write!(
        xml,
        "<sheetViews><sheetView{} workbookViewId=\"0\"><pane ySplit=\"{frozen}\" topLeftCell=\"A{}\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews><cols>",
        if selected { " tabSelected=\"1\"" } else { "" },
        frozen + 1
    );
    for (index, width) in sheet.widths.iter().enumerate() {
        let _ = write!(xml, "<col min=\"{0}\" max=\"{0}\" width=\"{width}\" customWidth=\"1\"/>", index + 1);
    }
    xml.push_str("</cols><sheetData>");
    for (r, row) in sheet.rows.iter().enumerate() {
        let _ = write!(xml, "<row r=\"{}\">", r + 1);
        for (c, (text, style)) in row.iter().enumerate() {
            let reference = format!("{}{}", column(c), r + 1);
            if text.is_empty() {
                let _ = write!(xml, "<c r=\"{reference}\" s=\"{style}\"/>");
            } else {
                let _ = write!(xml, "<c r=\"{reference}\" s=\"{style}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", escape(text));
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

/// 文字を、`limit`（UTF-16 の数。続きの印の分を除く）に収まる塊に分ける。なるべく改行の所で分け、1 行が長すぎる
/// ときだけ行の途中で分ける。
fn split_text(text: &str, limit: usize) -> Vec<String> {
    let limit = limit - CONTINUED_RESERVE;
    let size = |s: &str| s.encode_utf16().count();
    if size(text) <= limit {
        return vec![text.to_string()];
    }
    // 塊の終わりの改行 1 つは、分け目になるので落とす（つなぎ直すときに戻す）。
    let close = |piece: String| piece.strip_suffix('\n').map(str::to_string).unwrap_or(piece);
    let mut pieces = Vec::new();
    let (mut current, mut current_size) = (String::new(), 0);
    for line in text.split_inclusive('\n') {
        let line_size = size(line);
        if current_size + line_size <= limit {
            current.push_str(line);
            current_size += line_size;
            continue;
        }
        if !current.is_empty() {
            pieces.push(close(std::mem::take(&mut current)));
        }
        // 1 行が長すぎる: 文字の境目で分ける。
        let mut rest = line;
        let mut rest_size = line_size;
        while rest_size > limit {
            let mut units = 0;
            let at = rest.char_indices().find(|(_, c)| {
                units += c.len_utf16();
                units > limit
            });
            let at = at.map_or(rest.len(), |(at, _)| at);
            pieces.push(rest[..at].to_string());
            rest = &rest[at..];
            rest_size = size(rest);
        }
        current.push_str(rest);
        current_size = rest_size;
    }
    if !current.is_empty() {
        pieces.push(close(current));
    }
    pieces
}

/// 画像の縮めたあとの大きさ（画素）。縦横の比を保って上限に収める（大きくはしない）。
fn thumbnail_size(png: &[u8]) -> (u32, u32) {
    let (width, height) = png_size(png).unwrap_or((THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT));
    let scale = (THUMBNAIL_WIDTH as f64 / width as f64).min(THUMBNAIL_HEIGHT as f64 / height as f64).min(1.0);
    (((width as f64 * scale) as u32).max(1), ((height as f64 * scale) as u32).max(1))
}

/// PNG の幅と高さ（IHDR）。
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[12..16] != b"IHDR" {
        return None;
    }
    let read = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    Some((read(16), read(20))).filter(|(w, h)| *w > 0 && *h > 0)
}

/// 列の名前（0 → A、25 → Z、26 → AA）。
fn column(index: usize) -> String {
    let mut name = String::new();
    let mut n = index + 1;
    while n > 0 {
        let rest = (n - 1) % 26;
        name.insert(0, (b'A' + rest as u8) as char);
        n = (n - 1) / 26;
    }
    name
}

/// セルの番地（`AB12`）を、列（0 始まり）と行（1 始まり）にする。
fn cell_reference(reference: &str) -> Option<(usize, u32)> {
    let letters: String = reference.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let row = reference[letters.len()..].parse().ok()?;
    let column = letters.to_ascii_uppercase().bytes().try_fold(0usize, |n, b| Some(n * 26 + (b - b'A') as usize + 1))?;
    (column > 0).then(|| (column - 1, row))
}

/// シートの名前に使える形にする（使えない文字 `[]:*?/\` は `_`、前後の `'` は除く、31 文字まで）。
fn clean_sheet_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if "[]:*?/\\".contains(c) || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('\'').to_string();
    shorten(if cleaned.is_empty() { tr_str!("Item") } else { &cleaned }, SHEET_NAME_LIMIT)
}

/// `limit`（UTF-16 の数）に収まるところまで。
fn shorten(text: &str, limit: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= limit
        })
        .collect()
}

/// 重ならない名前にする（大文字と小文字は区別しない）。重なれば `(2)` を付ける（同じ番号を試し直したとき）。
fn unique_sheet_name(base: &str, taken: &mut Vec<String>) -> String {
    let mut name = base.to_string();
    let mut number = 2;
    while taken.contains(&name.to_lowercase()) {
        let suffix = format!("({number})");
        name = shorten(base, SHEET_NAME_LIMIT - suffix.len()) + &suffix;
        number += 1;
    }
    taken.push(name.to_lowercase());
    name
}

/// ほかのシートのセルを指す、ハイパーリンクの行き先（`'1-1'!A1`）。
fn location(sheet: &str, cell: &str) -> String {
    format!("'{}'!{cell}", sheet.replace('\'', "''"))
}

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
/// 書式: 0 既定、1 見出し（太字、色付き）、2 中身（折り返し、上寄せ）、3 ハイパーリンク（青の下線、上寄せ）、4 表題（太字）。
const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"4\"><font><sz val=\"10\"/><name val=\"Yu Gothic UI\"/></font><font><b/><sz val=\"10\"/><name val=\"Yu Gothic UI\"/></font><font><u/><sz val=\"10\"/><color rgb=\"FF0563C1\"/><name val=\"Yu Gothic UI\"/></font><font><b/><sz val=\"12\"/><name val=\"Yu Gothic UI\"/></font></fonts><fills count=\"3\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill><fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFE8E8E8\"/><bgColor indexed=\"64\"/></patternFill></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"5\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\"/><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyAlignment=\"1\"><alignment wrapText=\"1\" vertical=\"top\"/></xf><xf numFmtId=\"0\" fontId=\"2\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyAlignment=\"1\"><alignment vertical=\"top\"/></xf><xf numFmtId=\"0\" fontId=\"3\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/></cellXfs></styleSheet>";

#[cfg(test)]
pub(crate) mod tests {
    use super::super::html::tests::sample;
    use super::*;

    /// .xlsx の中身（名前と文字。画像はバイト列を文字にしたもの）。
    pub(crate) fn entries(xlsx: &[u8]) -> Vec<(String, String)> {
        zip::read(xlsx).unwrap().into_iter().map(|(name, data)| (name, String::from_utf8_lossy(&data).into_owned())).collect()
    }

    pub(crate) fn entry<'a>(entries: &'a [(String, String)], name: &str) -> &'a str {
        &entries.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name}")).1
    }

    /// シートの名前と、その中身（ブックの並び）。
    pub(crate) fn sheets(entries: &[(String, String)]) -> Vec<(String, String)> {
        let workbook = entry(entries, "xl/workbook.xml");
        let rels = entry(entries, "xl/_rels/workbook.xml.rels");
        workbook
            .split("<sheet ")
            .skip(1)
            .map(|s| {
                let attribute = |text: &str, name: &str| text.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).unwrap_or_default().to_string();
                let (name, id) = (attribute(s, "name"), attribute(s, "r:id"));
                let relationship = rels.split("<Relationship ").find(|r| r.contains(&format!("Id=\"{id}\""))).unwrap();
                let target = attribute(relationship, "Target");
                (name, entry(entries, &format!("xl/{}", target.trim_start_matches("/xl/"))).to_string())
            })
            .collect()
    }

    fn written(report: &Report) -> Vec<(String, String)> {
        entries(&write(report, "2026-11-21 09:43:00", None).unwrap())
    }

    #[test]
    fn the_default_template_writes_the_item_list_and_a_sheet_per_item_linked_both_ways() {
        let entries = written(&sample());
        let sheets = sheets(&entries);
        assert_eq!(sheets.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["成績書", "1-1"], "項目のシートの名前は番号");
        let (_, items) = &sheets[0];
        assert_eq!(items.matches("<row ").count(), 2, "見出しと項目 1 つ");
        assert!(items.contains("<c r=\"A2\" s=\"3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1-1</t></is></c>"), "番号はリンクの書式");
        assert!(items.contains("<hyperlink ref=\"A2\" location=\"&#39;1-1&#39;!A1\"/>"), "番号から項目のシートへ");
        assert!(items.contains("挨拶 &lt;Hello&gt; &amp; &quot;名前&quot;"));
        assert!(items.contains("<c r=\"F2\" s=\"2\"><v>17.26</v></c>"), "所要時間は数");
        assert!(items.contains("[AI] ui_invoke greetButton\n09:41:55 [手] nameBox に「Taro」を入れた"));
        assert!(!items.contains("{{"), "印は残らない");
        // 項目のシート: 表題、戻るリンク、見出し、操作 2 つ、画面 1 つ（「操作が無い」の行は消える）。
        let (_, operations) = &sheets[1];
        assert_eq!(operations.matches("<row ").count(), 6, "{operations}");
        assert!(operations.contains("<c r=\"A1\" s=\"4\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1-1 挨拶 &lt;Hello&gt; &amp; &quot;名前&quot;（OK・17.3 秒）</t>"));
        assert!(operations.contains("<hyperlink ref=\"A2\" location=\"&#39;成績書&#39;!A2\"/>"), "成績書の項目の行へ戻る");
        assert!(operations.contains("<pane ySplit=\"3\" topLeftCell=\"A4\""), "見出しまでを残す");
        assert!(operations.contains("<c r=\"A4\" s=\"2\"><v>1</v></c>") && operations.contains("<t xml:space=\"preserve\">ui_invoke greetButton</t>"));
        assert!(!operations.contains("記録された操作はありません") && !operations.contains("{{"));
        let drawing = entries.iter().find(|(n, d)| n.starts_with("xl/drawings/") && !n.contains("_rels") && d.contains("<xdr:wsDr")).map(|(_, d)| d.as_str()).unwrap();
        assert_eq!(drawing.matches("<xdr:oneCellAnchor>").count(), 3, "塗った画面、あとの画面、終わりの画面");
        assert!(drawing.contains("<xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>3</xdr:row>") && drawing.contains("<xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>3</xdr:row>"));
        assert!(drawing.contains("<xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row>"), "画面の行");
        assert!(entry(&entries, "[Content_Types].xml").contains("image/png"));
    }

    #[test]
    fn each_item_gets_its_own_sheet_even_with_the_same_number() {
        let mut report = sample();
        let mut again = report.items[0].clone();
        again.operations.clear();
        report.items.push(again);
        let mut odd = report.items[0].clone();
        odd.id = "成績書".into();
        report.items.push(odd);
        let mut long = report.items[0].clone();
        long.id = "A/B:C*".to_string() + &"x".repeat(40);
        report.items.push(long);
        let sheets = sheets(&written(&report));
        let names: Vec<&str> = sheets.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names[..4], ["成績書", "1-1", "1-1(2)", "成績書(2)"], "試し直した番号も、一覧の名前とも重ならない");
        assert_eq!(names[4], format!("A_B_C_{}", "x".repeat(25)), "使えない文字と 31 文字");
        assert!(sheets[2].1.contains("（記録された操作はありません）"));
        assert!(sheets[0].1.contains("<hyperlink ref=\"A3\" location=\"&#39;1-1(2)&#39;!A1\"/>"));
    }

    #[test]
    fn text_too_long_for_a_cell_continues_on_the_next_rows() {
        let mut report = sample();
        report.items[0].sql = vec![vec!["SELECT 1 FROM T;".repeat(100); 30].join("\n")];
        report.items.push(report.items[0].clone());
        let sheets = sheets(&written(&report));
        let items = &sheets[0].1;
        assert_eq!(items.matches("<row ").count(), 5, "見出しと、項目ごとに 2 行");
        assert!(items.contains("<c r=\"A3\" s=\"3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1-1（続き 2/2）</t>"), "続きの行の番号");
        assert!(items.contains("（下の行に続く）</t>"));
        assert!(items.contains("<hyperlink ref=\"A4\" location=\"&#39;1-1(2)&#39;!A1\"/>"), "次の項目は続きの行の下");
        assert!(sheets[2].1.contains("location=\"&#39;成績書&#39;!A4\""), "戻る先も、ずれた行");
    }

    #[test]
    fn a_single_long_line_is_cut_between_characters() {
        let long = "あ".repeat(70_000);
        let pieces = split_text(&long, CELL_LIMIT);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|p| p.encode_utf16().count() <= CELL_LIMIT));
        assert_eq!(pieces.concat(), long);
    }

    #[test]
    fn columns_and_cells_are_named_like_excel() {
        assert_eq!((column(0), column(25), column(26), column(701)), ("A".into(), "Z".into(), "AA".into(), "ZZ".into()));
        assert_eq!((cell_reference("A1"), cell_reference("AB12"), cell_reference("12")), (Some((0, 1)), Some((27, 12)), None));
    }
}
