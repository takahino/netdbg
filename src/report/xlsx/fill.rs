//! Excel のテンプレートを埋める（D-083）。現場の .xlsx（表紙、罫線、ロゴ、列の並び）はそのまま残し、印の所だけを書く:
//! - セル: 文字に `{{…}}` があれば、値で埋める（書式は残す）。印 1 つだけのセルは、画像なら貼り、数なら数のセル、
//!   リンクならハイパーリンクにする。
//! - 行: 同じ行の別のセルに `{{#一覧}}` と `{{/一覧}}` がある行は、要素ごとに行を増やす（0 件なら消す）。下の行、
//!   結合したセル、ハイパーリンク、絵（ロゴ）はずらす。式の中の参照はずらさない。
//! - シート: 名前に `{{` を含むシートは、項目ごとに複製する（名前は項目の値で埋める）。
//! - 収まらない文字は、すぐ下に行を足して続きを書く。

use super::{cell_reference, clean_sheet_name, column, location, split_text, thumbnail_size, unique_sheet_name, CELL_LIMIT, EMU_PER_PIXEL};
use crate::report::template::{self, Plain, Scope, Target, Value};
use crate::report::xml::{self, Element, Node};
use crate::report::{escape, zip, Picture};

/// .xlsx の部品（名前と中身。読んだ順を保つ）。
struct Parts(Vec<(String, Vec<u8>)>);

impl Parts {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, data)| data.as_slice())
    }

    fn text(&self, name: &str) -> Result<String, String> {
        let data = self.get(name).ok_or_else(|| tr!("{name} is missing.", name = name))?;
        String::from_utf8(data.to_vec()).map_err(|_| tr!("Cannot read {name} as UTF-8.", name = name))
    }

    fn xml(&self, name: &str) -> Result<Vec<Node>, String> {
        xml::parse(&self.text(name)?).map_err(|e| format!("{name}: {e}"))
    }

    fn set(&mut self, name: &str, data: Vec<u8>) {
        match self.0.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = data,
            None => self.0.push((name.to_string(), data)),
        }
    }

    fn remove(&mut self, name: &str) {
        self.0.retain(|(n, _)| n != name);
    }
}

/// 名前空間の前置きを除いた名前。
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// `like` と同じ前置きを付けた名前（前置きを使う書き手のファイルにも、同じ形で足す）。
fn named(like: &Element, local_name: &str) -> String {
    match like.name.split_once(':') {
        Some((prefix, _)) => format!("{prefix}:{local_name}"),
        None => local_name.to_string(),
    }
}

fn child<'e>(element: &'e Element, name: &str) -> Option<&'e Element> {
    element.elements().find(|e| local(&e.name) == name)
}

fn child_mut<'e>(element: &'e mut Element, name: &str) -> Option<&'e mut Element> {
    element.elements_mut().find(|e| local(&e.name) == name)
}

/// 前置き付きの属性（`r:id` など）。
fn prefixed_attribute(element: &Element, name: &str) -> Option<(String, String)> {
    element.attributes.iter().find(|(key, _)| key.contains(':') && local(key) == name && !key.starts_with("xmlns")).map(|(key, value)| (key.clone(), xml::unescape(value)))
}

fn dir_of(part: &str) -> &str {
    part.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// 部品の関係のファイル（`xl/worksheets/_rels/sheet1.xml.rels`）。
fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// 関係の Target を、部品の名前にする（相対なら、元の部品のフォルダーから）。
fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            segment => parts.push(segment),
        }
    }
    parts.join("/")
}

/// 関係のファイルの中の関係（Id、Type、Target）。
fn relationships(rels: &[Node]) -> Vec<(String, String, String)> {
    rels.iter()
        .filter_map(|n| if let Node::Element(e) = n { Some(e) } else { None })
        .flat_map(|root| root.elements().filter(|e| local(&e.name) == "Relationship").map(|e| (e.attribute("Id").unwrap_or_default(), e.attribute("Type").unwrap_or_default(), e.attribute("Target").unwrap_or_default())).collect::<Vec<_>>())
        .collect()
}

fn empty_rels() -> Vec<Node> {
    xml::parse("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>").expect("fixed XML")
}

fn add_relationship(rels: &mut [Node], id: &str, kind: &str, target: &str) -> Result<(), String> {
    let root = xml::root(rels)?;
    let mut relationship = Element::new(&named(root, "Relationship"));
    relationship.set("Id", id).set("Type", &format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}")).set("Target", target);
    root.children.push(Node::Element(relationship));
    Ok(())
}

/// 埋めたシート 1 枚。
struct Rendered<'a> {
    nodes: Vec<Node>,
    /// 貼る画像（行は 1 始まり、列は 0 始まり）。
    pictures: Vec<(u32, usize, &'a Picture)>,
    /// 一覧の「項目」の行（項目のシートから戻る先）。
    list_rows: Vec<u32>,
    /// もとの行 → 新しい行（繰り返した行は、最初の行。消した行は `None`）と、行の増減。
    rows: Vec<(u32, Option<u32>, i64)>,
}

impl Rendered<'_> {
    fn shift(&self, old: u32) -> Option<u32> {
        shift(&self.rows, old)
    }
}

/// もとの行の、新しい行（`rows` は、もとの行、新しい最初の行、行の増減）。間の空の行は、上の増減の分だけずれる。
fn shift(rows: &[(u32, Option<u32>, i64)], old: u32) -> Option<u32> {
    if let Some((_, new, _)) = rows.iter().find(|(r, ..)| *r == old) {
        return *new;
    }
    let delta: i64 = rows.iter().filter(|(r, ..)| *r < old).map(|(.., d)| d).sum();
    Some((old as i64 + delta).max(1) as u32)
}

/// 項目のシートへのリンクの行き先（`Item`）と、一覧へ戻る行き先（`List`）。
struct Links<'n> {
    item_sheets: Option<&'n [String]>,
    list: Option<(String, Vec<u32>)>,
}

impl Links<'_> {
    fn location(&self, target: Target) -> Option<String> {
        match target {
            Target::Item(index) => self.item_sheets?.get(index).map(|name| location(name, "A1")),
            Target::List(index) => self.list.as_ref().and_then(|(sheet, rows)| rows.get(index).map(|row| location(sheet, &format!("A{row}")))),
        }
    }
}

/// テンプレートの .xlsx を、`root` の値で埋める。
pub fn fill(template: &[u8], root: &Scope) -> Result<Vec<u8>, String> {
    let mut parts = Parts(zip::read(template)?);
    let workbook_path = relationships(&parts.xml("_rels/.rels")?)
        .into_iter()
        .find(|(_, kind, _)| kind.ends_with("/officeDocument"))
        .map(|(_, _, target)| resolve("", &target))
        .ok_or_else(|| tr!("Not an Excel workbook (.xlsx): _rels/.rels has no workbook."))?;
    let workbook_dir = dir_of(&workbook_path).to_string();
    let workbook_rels_path = rels_path(&workbook_path);
    let mut workbook = parts.xml(&workbook_path)?;
    let mut workbook_rels = parts.xml(&workbook_rels_path)?;
    let book_relationships = relationships(&workbook_rels);
    let strings: Vec<String> = match book_relationships.iter().find(|(_, kind, _)| kind.ends_with("/sharedStrings")) {
        Some((_, _, target)) => {
            let mut nodes = parts.xml(&resolve(&workbook_dir, target))?;
            xml::root(&mut nodes)?.elements().filter(|e| local(&e.name) == "si").map(Element::text).collect()
        }
        None => Vec::new(),
    };

    // シートの並び（名前、関係の Id、部品、項目ごとに複製するか）。
    struct Entry {
        name: String,
        id: String,
        path: String,
        element: Element,
    }
    let entries: Vec<Entry> = {
        let book = xml::root(&mut workbook)?;
        let sheets = child(book, "sheets").ok_or_else(|| tr!("The workbook has no sheets."))?;
        sheets
            .elements()
            .map(|sheet| {
                let name = sheet.attribute("name").unwrap_or_default();
                let id = prefixed_attribute(sheet, "id").map(|(_, v)| v).unwrap_or_default();
                let target = book_relationships.iter().find(|(rid, ..)| *rid == id).map(|(.., t)| t.clone()).ok_or_else(|| tr!("The part of the sheet \"{name}\" is not found.", name = name))?;
                Ok(Entry { name, id, path: resolve(&workbook_dir, &target), element: sheet.clone() })
            })
            .collect::<Result<_, String>>()?
    };
    let is_template = |entry: &Entry| entry.name.contains("{{");
    let items: Vec<&Scope> = match root.get("items") {
        Some(Value::List(items)) => items.iter().collect(),
        _ => Vec::new(),
    };

    // 項目のシートの名前（ほかのシートの名前と重ならないように）。
    let mut taken: Vec<String> = entries.iter().filter(|e| !is_template(e)).map(|e| e.name.to_lowercase()).chain(["history".to_string()]).collect();
    let mut item_names: Vec<Vec<String>> = Vec::new();
    for entry in entries.iter().filter(|e| is_template(e)) {
        let mut names = Vec::new();
        for item in &items {
            let name = template::fill_text(&entry.name, &mut vec![root, *item]).map_err(|e| tr!("Sheet name \"{name}\": {error}", name = entry.name, error = e))?;
            names.push(unique_sheet_name(&clean_sheet_name(&name), &mut taken));
        }
        item_names.push(names);
    }
    let mut links = Links { item_sheets: item_names.first().map(Vec::as_slice), list: None };

    // ほかのシートを先に埋め、一覧の項目の行を覚える（項目のシートの「一覧へのリンク」の行き先）。
    let mut rendered: Vec<Option<Rendered>> = Vec::new();
    for entry in &entries {
        if is_template(entry) {
            rendered.push(None);
            continue;
        }
        let sheet = render_sheet(&parts.text(&entry.path)?, &[root], &strings, &links).map_err(|e| tr!("Sheet \"{name}\": {error}", name = entry.name, error = e))?;
        if links.list.is_none() && !sheet.list_rows.is_empty() {
            links.list = Some((entry.name.clone(), sheet.list_rows.clone()));
        }
        rendered.push(Some(sheet));
    }

    // 書き出すシート（ブックの並び）。
    struct Output<'a> {
        name: String,
        path: String,
        source: String,
        duplicate: bool,
        sheet: Rendered<'a>,
    }
    let mut outputs: Vec<Output> = Vec::new();
    let mut templates_seen = 0;
    let mut next_sheet = 0;
    // 複製元の番号（もとの並び）→ 書き出す番号。definedNames の localSheetId に使う。
    let mut index_map: Vec<Vec<usize>> = Vec::new();
    for (entry, sheet) in entries.iter().zip(rendered) {
        match sheet {
            Some(sheet) => {
                index_map.push(vec![outputs.len()]);
                outputs.push(Output { name: entry.name.clone(), path: entry.path.clone(), source: entry.path.clone(), duplicate: false, sheet });
            }
            None => {
                let names = &item_names[templates_seen];
                templates_seen += 1;
                let text = parts.text(&entry.path)?;
                let mut indexes = Vec::new();
                for (item, name) in items.iter().zip(names) {
                    let sheet = render_sheet(&text, &[root, *item], &strings, &links).map_err(|e| tr!("Sheet \"{name}\" (template \"{template}\"): {error}", name = name, template = entry.name, error = e))?;
                    next_sheet += 1;
                    indexes.push(outputs.len());
                    outputs.push(Output { name: name.clone(), path: format!("xl/worksheets/dx_sheet{next_sheet}.xml"), source: entry.path.clone(), duplicate: true, sheet });
                }
                index_map.push(indexes);
            }
        }
    }
    if outputs.is_empty() {
        return Err(tr!("There are no sheets to write (there are no items, and the template has only the item sheet)."));
    }

    // 部品を書く。
    let mut types = parts.xml("[Content_Types].xml")?;
    let mut new_types: Vec<(String, &str)> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut counter = 0usize;
    let mut next_id = || {
        counter += 1;
        counter
    };
    for (position, output) in outputs.iter_mut().enumerate() {
        // 先頭のシートだけを選んでおく（複製したシートが全部選ばれていると、Excel がまとめて編集する）。
        select_tab(&mut output.sheet.nodes, position == 0)?;
        let source_rels_path = rels_path(&output.source);
        let mut rels = match parts.get(&source_rels_path) {
            Some(_) => Some(parts.xml(&source_rels_path)?),
            None => None,
        };
        // 複製したシートの持ち物（絵、印刷の設定）は、シートごとに写す。
        if output.duplicate {
            if let Some(rels) = rels.as_mut() {
                let root_rels = xml::root(rels)?;
                for relationship in root_rels.elements_mut() {
                    let kind = relationship.attribute("Type").unwrap_or_default();
                    let target = resolve(dir_of(&output.source), &relationship.attribute("Target").unwrap_or_default());
                    let kind_name = kind.rsplit('/').next().unwrap_or_default().to_string();
                    match kind_name.as_str() {
                        "drawing" | "printerSettings" => {
                            let n = next_id();
                            let extension = target.rsplit_once('.').map_or("xml", |(_, e)| e);
                            let folder = dir_of(&target);
                            let copy = format!("{folder}/dx_{kind_name}{n}.{extension}");
                            let data = parts.get(&target).ok_or_else(|| tr!("{name} is missing.", name = target))?.to_vec();
                            parts.set(&copy, data);
                            if let Some(own_rels) = parts.get(&rels_path(&target)).map(<[u8]>::to_vec) {
                                parts.set(&rels_path(&copy), own_rels);
                            }
                            if kind_name == "drawing" {
                                new_types.push((copy.clone(), "application/vnd.openxmlformats-officedocument.drawing+xml"));
                            }
                            relationship.set("Target", &format!("/{copy}"));
                        }
                        "comments" | "vmlDrawing" | "table" | "pivotTable" | "threadedComment" | "ctrlProp" | "control" => {
                            return Err(tr!("The item sheet template has something that cannot be copied ({kind}: comments, tables, controls and so on). Remove it.", kind = kind_name));
                        }
                        _ => {}
                    }
                }
            }
        }
        // 絵: もとからある絵（ロゴ）は行のずれに合わせて動かし、埋めた画像を足す。
        let drawing_relationship = rels.as_ref().and_then(|r| relationships(r).into_iter().find(|(_, kind, _)| kind.ends_with("/drawing")));
        let drawing_path = drawing_relationship.as_ref().map(|(_, _, target)| resolve(dir_of(&output.source), target));
        if drawing_path.is_some() || !output.sheet.pictures.is_empty() {
            let (path, mut drawing, mut drawing_rels) = match &drawing_path {
                Some(path) => (path.clone(), parts.xml(path)?, match parts.get(&rels_path(path)) {
                    Some(_) => parts.xml(&rels_path(path))?,
                    None => empty_rels(),
                }),
                None => {
                    let path = format!("xl/drawings/dx_drawing_new{}.xml", next_id());
                    let nodes = xml::parse(EMPTY_DRAWING).expect("fixed XML");
                    let rels = rels.get_or_insert_with(empty_rels);
                    let id = format!("rIdDxDrawing{}", next_id());
                    add_relationship(rels, &id, "drawing", &format!("/{path}"))?;
                    // シートの <drawing r:id> を足す（関係の名前空間の前置きは、シートが使っているもの。無ければ足す）。
                    let sheet_root = xml::root(&mut output.sheet.nodes)?;
                    let prefix = match sheet_root.attributes.iter().find(|(k, v)| k.starts_with("xmlns:") && xml::unescape(v) == RELATIONSHIPS) {
                        Some((key, _)) => key["xmlns:".len()..].to_string(),
                        None => {
                            sheet_root.set("xmlns:dxr", RELATIONSHIPS);
                            "dxr".into()
                        }
                    };
                    let mut element = Element::new(&named(sheet_root, "drawing"));
                    element.set(&format!("{prefix}:id"), &id);
                    sheet_root.insert_before(element, &AFTER_DRAWING);
                    new_types.push((path.clone(), "application/vnd.openxmlformats-officedocument.drawing+xml"));
                    (path, nodes, empty_rels())
                }
            };
            place_pictures(&mut drawing, &mut drawing_rels, &output.sheet, &mut parts, &mut next_id)?;
            parts.set(&path, xml::write(&drawing).into_bytes());
            parts.set(&rels_path(&path), xml::write(&drawing_rels).into_bytes());
        }
        parts.set(&output.path, xml::write(&output.sheet.nodes).into_bytes());
        if let Some(rels) = &rels {
            parts.set(&rels_path(&output.path), xml::write(rels).into_bytes());
        }
        if output.duplicate {
            new_types.push((output.path.clone(), "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"));
        }
    }

    // 複製元のシートと、その持ち物を消す。計算の順の記録（calcChain）も消す（行がずれると合わなくなる。Excel が作り直す）。
    for entry in entries.iter().filter(|e| is_template(e)) {
        if let Some(rels) = parts.get(&rels_path(&entry.path)).map(|_| parts.xml(&rels_path(&entry.path))).transpose()? {
            for (_, kind, target) in relationships(&rels) {
                if kind.ends_with("/drawing") || kind.ends_with("/printerSettings") {
                    let path = resolve(dir_of(&entry.path), &target);
                    removed.push(rels_path(&path));
                    removed.push(path);
                }
            }
        }
        removed.push(rels_path(&entry.path));
        removed.push(entry.path.clone());
    }
    let calc_chain: Vec<(String, String)> = book_relationships.iter().filter(|(_, kind, _)| kind.ends_with("/calcChain")).map(|(id, _, target)| (id.clone(), resolve(&workbook_dir, target))).collect();
    removed.extend(calc_chain.iter().map(|(_, path)| path.clone()));
    for path in &removed {
        parts.remove(path);
    }

    // ブック: シートの並び、関係、名前の定義、選んでいるシート。
    {
        let book = xml::root(&mut workbook)?;
        let max_id = entries.iter().filter_map(|e| e.element.attribute("sheetId")?.parse::<u32>().ok()).max().unwrap_or(0);
        let mut new_id = max_id;
        let rels_root = xml::root(&mut workbook_rels)?;
        rels_root.children.retain(|node| match node {
            Node::Element(e) => {
                let id = e.attribute("Id").unwrap_or_default();
                !entries.iter().any(|entry| is_template(entry) && entry.id == id) && !calc_chain.iter().any(|(rid, _)| *rid == id)
            }
            _ => true,
        });
        let mut sheet_elements = Vec::new();
        for (entry, indexes) in entries.iter().zip(&index_map) {
            if !is_template(entry) {
                sheet_elements.push(Node::Element(entry.element.clone()));
                continue;
            }
            for output in indexes.iter().map(|&i| &outputs[i]) {
                new_id += 1;
                let rid = format!("rIdDxSheet{new_id}");
                let mut element = entry.element.clone();
                element.set("name", &output.name).set("sheetId", &new_id.to_string());
                if let Some((key, _)) = prefixed_attribute(&element, "id") {
                    element.set(&key, &rid);
                }
                sheet_elements.push(Node::Element(element));
                let mut relationship = Element::new(&named(rels_root, "Relationship"));
                relationship.set("Id", &rid).set("Type", "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet").set("Target", &format!("/{}", output.path));
                rels_root.children.push(Node::Element(relationship));
            }
        }
        child_mut(book, "sheets").expect("checked above").children = sheet_elements;
        if let Some(views) = child_mut(book, "bookViews") {
            for view in views.elements_mut() {
                view.set("activeTab", "0");
                view.remove("firstSheet");
            }
        }
        if let Some(defined) = child_mut(book, "definedNames") {
            let mut names = Vec::new();
            for node in std::mem::take(&mut defined.children) {
                let Node::Element(name) = node else {
                    names.push(node);
                    continue;
                };
                let Some(old) = name.attribute("localSheetId").and_then(|i| i.parse::<usize>().ok()) else {
                    names.push(Node::Element(name));
                    continue;
                };
                let Some(new_indexes) = index_map.get(old) else { continue };
                let template_name = &entries[old].name;
                for &index in new_indexes {
                    let mut copy = name.clone();
                    copy.set("localSheetId", &index.to_string());
                    if is_template(&entries[old]) {
                        // 印刷タイトルなどの式の中の、複製元のシートの名前を、複製したシートの名前にする。
                        let formula = copy.text().replace(&format!("'{}'!", template_name.replace('\'', "''")), &format!("'{}'!", outputs[index].name.replace('\'', "''")));
                        copy.children = vec![Node::Text(escape(&formula))];
                    }
                    names.push(Node::Element(copy));
                }
            }
            defined.children = names;
        }
    }
    parts.set(&workbook_path, xml::write(&workbook).into_bytes());
    parts.set(&workbook_rels_path, xml::write(&workbook_rels).into_bytes());

    // 中身の種類: 消した部品を除き、足した部品と png を書く。
    {
        let root_types = xml::root(&mut types)?;
        root_types.children.retain(|node| match node {
            Node::Element(e) if local(&e.name) == "Override" => !removed.iter().any(|path| e.attribute("PartName").as_deref() == Some(&format!("/{path}"))),
            _ => true,
        });
        let has_default = |root: &Element, extension: &str| root.elements().any(|e| local(&e.name) == "Default" && e.attribute("Extension").is_some_and(|x| x.eq_ignore_ascii_case(extension)));
        if !has_default(root_types, "png") {
            let mut png = Element::new(&named(root_types, "Default"));
            png.set("Extension", "png").set("ContentType", "image/png");
            root_types.children.insert(0, Node::Element(png));
        }
        if !has_default(root_types, "bin") {
            let mut bin = Element::new(&named(root_types, "Default"));
            bin.set("Extension", "bin").set("ContentType", "application/vnd.openxmlformats-officedocument.spreadsheetml.printerSettings");
            root_types.children.insert(0, Node::Element(bin));
        }
        for (path, kind) in &new_types {
            let mut entry = Element::new(&named(root_types, "Override"));
            entry.set("PartName", &format!("/{path}")).set("ContentType", kind);
            root_types.children.push(Node::Element(entry));
        }
    }
    parts.set("[Content_Types].xml", xml::write(&types).into_bytes());

    let mut writer = zip::Writer::default();
    for (name, data) in &parts.0 {
        writer.add(name, data);
    }
    Ok(writer.finish())
}

const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// `drawing` より後ろに来る部品（シートの部品の並びの決まり）。
const AFTER_DRAWING: [&str; 9] = ["legacyDrawing", "legacyDrawingHF", "drawingHF", "picture", "oleObjects", "controls", "webPublishItems", "tableParts", "extLst"];
/// `hyperlinks` より後ろに来る部品。
const AFTER_HYPERLINKS: [&str; 19] = ["printOptions", "pageMargins", "pageSetup", "headerFooter", "rowBreaks", "colBreaks", "customProperties", "cellWatches", "ignoredErrors", "smartTags", "drawing", "legacyDrawing", "legacyDrawingHF", "drawingHF", "picture", "oleObjects", "controls", "webPublishItems", "tableParts"];
const EMPTY_DRAWING: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"></xdr:wsDr>";

/// 先頭のシートだけを選んでおく。
fn select_tab(nodes: &mut [Node], selected: bool) -> Result<(), String> {
    let root = xml::root(nodes)?;
    if let Some(views) = child_mut(root, "sheetViews") {
        for (index, view) in views.elements_mut().enumerate() {
            view.remove("tabSelected");
            if selected && index == 0 {
                view.set("tabSelected", "1");
            }
        }
    }
    Ok(())
}

/// 絵に、もとからある絵のずれを入れ、埋めた画像を足す。
fn place_pictures(drawing: &mut [Node], drawing_rels: &mut [Node], sheet: &Rendered, parts: &mut Parts, next_id: &mut impl FnMut() -> usize) -> Result<(), String> {
    let root = xml::root(drawing)?;
    // もとからある絵（ロゴなど）を、行のずれに合わせて動かす（行は 0 始まり）。
    for anchor in root.elements_mut().filter(|e| matches!(local(&e.name), "twoCellAnchor" | "oneCellAnchor")) {
        for corner in anchor.elements_mut().filter(|e| matches!(local(&e.name), "from" | "to")) {
            if let Some(row) = corner.elements_mut().find(|e| local(&e.name) == "row") {
                if let Some(new) = row.text().trim().parse::<u32>().ok().and_then(|old| sheet.shift(old + 1)) {
                    row.children = vec![Node::Text((new - 1).to_string())];
                }
            }
        }
    }
    for (key, value) in [("xmlns:xdr", "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"), ("xmlns:a", "http://schemas.openxmlformats.org/drawingml/2006/main"), ("xmlns:r", RELATIONSHIPS)] {
        if root.attribute(key).is_none() {
            root.set(key, value);
        }
    }
    let mut max_id = max_shape_id(root);
    for (row, column, picture) in &sheet.pictures {
        let n = next_id();
        let media = format!("xl/media/dx_image{n}.png");
        parts.set(&media, picture.png.clone());
        let rid = format!("rIdDxImage{n}");
        add_relationship(drawing_rels, &rid, "image", &format!("/{media}"))?;
        max_id += 1;
        let (width, height) = thumbnail_size(&picture.png);
        let (cx, cy) = (width as u64 * EMU_PER_PIXEL, height as u64 * EMU_PER_PIXEL);
        let anchor = format!(
            "<xdr:oneCellAnchor><xdr:from><xdr:col>{column}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx=\"{cx}\" cy=\"{cy}\"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{max_id}\" name=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>",
            row - 1,
            escape(&picture.caption)
        );
        root.children.extend(xml::parse(&anchor)?);
    }
    Ok(())
}

/// 絵の中の図形の番号のいちばん大きいもの。
fn max_shape_id(element: &Element) -> u32 {
    let own = if local(&element.name) == "cNvPr" { element.attribute("id").and_then(|i| i.parse().ok()).unwrap_or(0) } else { 0 };
    element.elements().map(max_shape_id).fold(own, u32::max)
}

/// セルの文字（共有の文字、セルの中の文字）。式のセルと、数のセルは `None`（埋めない）。
fn cell_text(cell: &Element, strings: &[String]) -> Option<String> {
    if child(cell, "f").is_some() {
        return None;
    }
    match cell.attribute("t").as_deref() {
        Some("s") => strings.get(child(cell, "v")?.text().trim().parse::<usize>().ok()?).cloned(),
        Some("inlineStr") => Some(child(cell, "is")?.text()),
        _ => None,
    }
}

/// `{{…}}` の印（始まり、終わり、種類 `#` `^` `/` か空白、名前）。
fn tags(text: &str) -> Vec<(usize, usize, char, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..].find("{{").map(|s| s + at) {
        let Some(end) = text[start + 2..].find("}}").map(|e| e + start + 2) else { break };
        let tag = text[start + 2..end].trim();
        let (kind, name) = match tag.chars().next() {
            Some(kind @ ('#' | '^' | '/')) => (kind, tag[1..].trim().to_string()),
            _ => (' ', tag.to_string()),
        };
        out.push((start, end + 2, kind, name));
        at = end + 2;
    }
    out
}

/// 行の繰り返し: 同じ行の別のセルで開いて閉じる `{{#名前}}` / `{{^名前}}`。（名前、反対か、開くセル、印の範囲、閉じるセル、印の範囲）。
type RowSection = (String, bool, usize, (usize, usize), usize, (usize, usize));

/// 同じ名前か（別名でもよい。{{#項目}}…{{/items}}）。
fn same(a: &str, b: &str) -> bool {
    crate::report::data::canonical(a) == crate::report::data::canonical(b)
}

fn row_section(texts: &[Option<String>], row: u32) -> Result<Option<RowSection>, String> {
    for (i, text) in texts.iter().enumerate() {
        let Some(text) = text else { continue };
        // このセルの中で閉じていない、開く印。
        let mut open: Vec<(String, bool, (usize, usize))> = Vec::new();
        for (start, end, kind, name) in tags(text) {
            match kind {
                '#' | '^' => open.push((name, kind == '^', (start, end))),
                '/' if open.last().is_some_and(|(n, ..)| same(n, &name)) => {
                    open.pop();
                }
                _ => {}
            }
        }
        let Some((name, inverted, span)) = open.into_iter().next() else { continue };
        for (j, later) in texts.iter().enumerate().skip(i + 1) {
            let Some(later) = later else { continue };
            let mut depth = 0i32;
            for (start, end, kind, tag_name) in tags(later) {
                match kind {
                    '#' | '^' if same(&tag_name, &name) => depth += 1,
                    '/' if same(&tag_name, &name) && depth > 0 => depth -= 1,
                    '/' if same(&tag_name, &name) => return Ok(Some((name, inverted, i, span, j, (start, end)))),
                    _ => {}
                }
            }
        }
        return Err(tr!("The {{{{/{name}}}}} closing {{{{{kind}{name}}}}} on row {row} is not in a cell to its right on the same row (a repeated row opens and closes within one row).", row = row, kind = if inverted { '^' } else { '#' }, name = name));
    }
    Ok(None)
}

/// 埋めたセル。
enum Filled<'a> {
    /// 触らないセル（印が無い）。
    Keep,
    Text(String),
    Number(f64),
    Image(&'a Picture),
    Link(String, Option<String>),
    Empty,
}

impl Filled<'_> {
    fn display(&self) -> Option<String> {
        match self {
            Filled::Text(text) | Filled::Link(text, _) => Some(text.clone()),
            Filled::Number(value) => Some(template::number_text(*value)),
            _ => None,
        }
    }
}

fn fill_cell<'a>(text: &str, scopes: &mut Vec<&Scope<'a>>, links: &Links) -> Result<Filled<'a>, String> {
    let nodes = template::parse(text)?;
    if let [template::Node::Var { name, modifier: None }] = nodes.as_slice() {
        return Ok(match template::lookup(scopes, name)? {
            Value::Image(picture) => Filled::Image(picture),
            Value::Number(value) => Filled::Number(*value),
            Value::Link { text, target } => Filled::Link(text.clone(), links.location(*target)),
            Value::Bool(_) | Value::List(_) => Filled::Empty,
            value => {
                let text = value.text();
                if text.is_empty() { Filled::Empty } else { Filled::Text(text) }
            }
        });
    }
    let mut plain = Plain::default();
    template::render(&nodes, scopes, &mut plain)?;
    Ok(if plain.0.is_empty() { Filled::Empty } else { Filled::Text(plain.0) })
}

/// シート 1 枚を埋める。
fn render_sheet<'a>(text: &str, scopes: &[&Scope<'a>], strings: &[String], links: &Links) -> Result<Rendered<'a>, String> {
    let mut nodes = xml::parse(text)?;
    let root = xml::root(&mut nodes)?;
    let mut pictures = Vec::new();
    let mut list_rows = Vec::new();
    let mut row_map: Vec<(u32, Option<u32>, i64)> = Vec::new();
    let mut new_links: Vec<(String, String)> = Vec::new();
    // 繰り返した行（もとの行 → 繰り返しごとの最初の行）。結合したセルを複製するのに使う。
    let mut repeated: Vec<(u32, Vec<u32>)> = Vec::new();
    let sheet_data = child_mut(root, "sheetData").ok_or_else(|| tr!("There is no sheetData."))?;
    let rows: Vec<Node> = std::mem::take(&mut sheet_data.children);
    let mut out_rows: Vec<Node> = Vec::new();
    let mut delta: i64 = 0;
    for node in rows {
        let Node::Element(row) = node else { continue };
        let old: u32 = row.attribute("r").and_then(|r| r.parse().ok()).ok_or_else(|| tr!("A row has no row number (r)."))?;
        let cells: Vec<Element> = row.elements().filter(|e| local(&e.name) == "c").cloned().collect();
        let mut texts: Vec<Option<String>> = cells.iter().map(|c| cell_text(c, strings)).collect();
        let section = row_section(&texts, old)?;
        let is_section = section.is_some();
        let has_marks = texts.iter().flatten().any(|t| t.contains("{{"));
        let base = (old as i64 + delta) as u32;
        if !has_marks {
            out_rows.push(Node::Element(renumber(&row, &cells, base)));
            row_map.push((old, Some(base), 0));
            continue;
        }
        // 繰り返しの印を外し、繰り返す分の値を決める。
        let mut repetitions: Vec<Option<&Scope<'a>>> = vec![None];
        let mut section_name = None;
        if let Some((name, inverted, open_cell, open_span, close_cell, close_span)) = section {
            if let Some(text) = texts[close_cell].as_mut() {
                text.replace_range(close_span.0..close_span.1, "");
            }
            if let Some(text) = texts[open_cell].as_mut() {
                text.replace_range(open_span.0..open_span.1, "");
            }
            let value = template::lookup(scopes, &name).map_err(|e| tr!("Row {row}: {error}", row = old, error = e))?;
            repetitions = match (value, inverted) {
                (Value::List(items), false) => items.iter().map(Some).collect(),
                (value, inverted) if value.truthy() != inverted => vec![None],
                _ => Vec::new(),
            };
            section_name = Some((name, inverted));
        }
        let mut produced = 0u32;
        let mut starts = Vec::new();
        for (index, repetition) in repetitions.iter().enumerate() {
            let mut scopes: Vec<&Scope<'a>> = scopes.to_vec();
            scopes.extend(repetition);
            let filled: Vec<Filled<'a>> = cells
                .iter()
                .zip(&texts)
                .map(|(cell, text)| match text {
                    Some(text) if text.contains("{{") || is_section => fill_cell(text, &mut scopes, links).map_err(|e| format!("{}: {e}", cell.attribute("r").unwrap_or_default())),
                    _ => Ok(Filled::Keep),
                })
                .collect::<Result<_, _>>()?;
            let start = base + produced;
            starts.push(start);
            // 一覧の「項目」の行（項目のシートから戻る先）。
            if let Some((name, false)) = &section_name {
                if crate::report::data::canonical(name) == "items" && scopes.len() == 2 && scopes[0].get("items").is_some() && index == list_rows.len() {
                    list_rows.push(start);
                }
            }
            for split in split_cells(&filled) {
                let number = base + produced;
                let mut new_row = row.clone();
                new_row.set("r", &number.to_string());
                new_row.children = Vec::new();
                let mut tallest = 0u32;
                for (c, (cell, value)) in cells.iter().zip(&split).enumerate() {
                    let column_index = cell.attribute("r").and_then(|r| cell_reference(&r)).map_or(c, |(col, _)| col);
                    let reference = format!("{}{number}", column(column_index));
                    let element = match value {
                        Filled::Keep => {
                            let mut kept = cell.clone();
                            kept.set("r", &reference);
                            kept
                        }
                        value => {
                            if let Filled::Image(picture) = value {
                                pictures.push((number, column_index, *picture));
                                tallest = tallest.max(thumbnail_size(&picture.png).1);
                            }
                            if let Filled::Link(_, Some(target)) = value {
                                new_links.push((reference.clone(), target.clone()));
                            }
                            filled_cell(cell, &reference, value)
                        }
                    };
                    new_row.children.push(Node::Element(element));
                }
                // 画像のある行は、画像の高さにする（ポイント。1 画素は 0.75 ポイント）。
                if tallest > 0 {
                    let height = tallest as f64 * 0.75 + 4.0;
                    if new_row.attribute("ht").and_then(|h| h.parse::<f64>().ok()).is_none_or(|h| h < height) {
                        new_row.set("ht", &height.to_string()).set("customHeight", "1");
                    }
                }
                out_rows.push(Node::Element(new_row));
                produced += 1;
            }
        }
        if section_name.is_some() {
            repeated.push((old, starts.clone()));
        }
        row_map.push((old, starts.first().copied(), produced as i64 - 1));
        delta += produced as i64 - 1;
    }
    child_mut(root, "sheetData").expect("checked above").children = out_rows;
    // 範囲（dimension）は合わなくなるので消す（Excel が数え直す）。
    root.children.retain(|n| !matches!(n, Node::Element(e) if local(&e.name) == "dimension"));
    // 結合したセル: 繰り返した行の中の結合は行ごとに複製し、ほかはずらす。
    if let Some(merges) = child_mut(root, "mergeCells") {
        let mut kept = Vec::new();
        for node in std::mem::take(&mut merges.children) {
            let Node::Element(merge) = node else { continue };
            let Some((first, last)) = merge.attribute("ref").and_then(|r| range(&r)) else {
                kept.push(Node::Element(merge));
                continue;
            };
            match repeated.iter().find(|(old, _)| *old == first.1 && *old == last.1) {
                Some((_, starts)) => {
                    for start in starts {
                        let mut copy = merge.clone();
                        copy.set("ref", &format!("{}{start}:{}{start}", column(first.0), column(last.0)));
                        kept.push(Node::Element(copy));
                    }
                }
                None => {
                    if let (Some(top), Some(bottom)) = (shift(&row_map, first.1), shift(&row_map, last.1)) {
                        let mut moved = merge;
                        moved.set("ref", &format!("{}{top}:{}{bottom}", column(first.0), column(last.0)));
                        kept.push(Node::Element(moved));
                    }
                }
            }
        }
        merges.set("count", &kept.len().to_string());
        merges.children = kept;
        if merges.children.is_empty() {
            root.children.retain(|n| !matches!(n, Node::Element(e) if local(&e.name) == "mergeCells"));
        }
    }
    // ハイパーリンク: もとからあるものはずらし、埋めたリンクを足す（ブックの中なので location で書く）。
    if child(root, "hyperlinks").is_none() && !new_links.is_empty() {
        let element = Element::new(&named(root, "hyperlinks"));
        root.insert_before(element, &AFTER_HYPERLINKS);
    }
    if let Some(hyperlinks) = child_mut(root, "hyperlinks") {
        for link in hyperlinks.elements_mut() {
            if let Some((cell, row)) = link.attribute("ref").and_then(|r| cell_reference(&r)) {
                if let Some(new) = shift(&row_map, row) {
                    link.set("ref", &format!("{}{new}", column(cell)));
                }
            }
        }
        for (reference, target) in &new_links {
            let mut link = Element::new(&named(hyperlinks, "hyperlink"));
            link.set("ref", reference).set("location", target);
            hyperlinks.children.push(Node::Element(link));
        }
    }
    Ok(Rendered { nodes, pictures, list_rows, rows: row_map })
}

/// `A1:C2` を、両端（列、行）にする。1 つのセルなら、両端は同じ。
fn range(reference: &str) -> Option<((usize, u32), (usize, u32))> {
    let (first, last) = reference.split_once(':').unwrap_or((reference, reference));
    Some((cell_reference(first)?, cell_reference(last)?))
}

/// 印の無い行の番号を付け直す。
fn renumber(row: &Element, cells: &[Element], number: u32) -> Element {
    let mut new_row = row.clone();
    new_row.set("r", &number.to_string());
    let mut index = 0;
    for node in &mut new_row.children {
        if let Node::Element(cell) = node {
            if local(&cell.name) == "c" {
                if let Some((column_index, _)) = cells.get(index).and_then(|c| c.attribute("r")).and_then(|r| cell_reference(&r)) {
                    cell.set("r", &format!("{}{number}", column(column_index)));
                }
                index += 1;
            }
        }
    }
    new_row
}

/// 収まらない文字があれば、続きの行に分ける（分けたセルは続きの印、ほかは空。続きの行の最初の値のセルには、
/// その値と「（続き 2/3）」を書く）。
fn split_cells<'a>(filled: &[Filled<'a>]) -> Vec<Vec<Filled<'a>>> {
    let pieces: Vec<Option<Vec<String>>> = filled.iter().map(|f| match f {
        Filled::Text(text) => Some(split_text(text, CELL_LIMIT)).filter(|p| p.len() > 1),
        _ => None,
    }).collect();
    let count = pieces.iter().flatten().map(Vec::len).max().unwrap_or(1);
    let copy = |f: &Filled<'a>| match f {
        Filled::Keep => Filled::Keep,
        Filled::Text(t) => Filled::Text(t.clone()),
        Filled::Number(n) => Filled::Number(*n),
        Filled::Image(p) => Filled::Image(p),
        Filled::Link(t, l) => Filled::Link(t.clone(), l.clone()),
        Filled::Empty => Filled::Empty,
    };
    if count == 1 {
        return vec![filled.iter().map(copy).collect()];
    }
    let marker = filled.iter().zip(&pieces).position(|(f, p)| p.is_none() && f.display().is_some());
    (0..count)
        .map(|k| {
            filled
                .iter()
                .zip(&pieces)
                .enumerate()
                .map(|(c, (f, p))| match p {
                    Some(p) => p.get(k).map_or(Filled::Empty, |piece| Filled::Text(if k + 1 < p.len() { format!("{piece}{}", super::continued()) } else { piece.clone() })),
                    None if k == 0 => copy(f),
                    None if Some(c) == marker => Filled::Text(tr!("{value} (continued {part}/{count})", value = f.display().unwrap_or_default(), part = k + 1, count = count)),
                    None => match f {
                        Filled::Keep => Filled::Keep,
                        _ => Filled::Empty,
                    },
                })
                .collect()
        })
        .collect()
}

/// 埋めた値のセル（書式 `s` は残す）。
fn filled_cell(original: &Element, reference: &str, value: &Filled) -> Element {
    let mut cell = Element::new(&original.name);
    cell.set("r", reference);
    if let Some(style) = original.attribute("s") {
        cell.set("s", &style);
    }
    let text_cell = |cell: &mut Element, text: &str| {
        cell.set("t", "inlineStr");
        let mut is = Element::new(&named(original, "is"));
        let mut t = Element::new(&named(original, "t"));
        t.set("xml:space", "preserve");
        t.children.push(Node::Text(escape(text)));
        is.children.push(Node::Element(t));
        cell.children.push(Node::Element(is));
    };
    match value {
        Filled::Text(text) | Filled::Link(text, _) => text_cell(&mut cell, text),
        Filled::Number(number) => {
            let mut v = Element::new(&named(original, "v"));
            v.children.push(Node::Text(number.to_string()));
            cell.children.push(Node::Element(v));
        }
        Filled::Keep | Filled::Image(_) | Filled::Empty => {}
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entries, entry, sheets};
    use super::*;
    use crate::report::html::tests::sample;

    /// 現場風のテンプレート: 共有の文字、表紙、結合、印刷タイトル、ロゴの絵、もとからあるリンク、計算の順の記録。
    fn site_template() -> Vec<u8> {
        let strings = ["試験成績書", "{{プログラム名}} 版 {{版}}", "番号", "結果", "{{#項目}}{{項目へのリンク}}", "{{結果}}{{/項目}}", "合計", "{{番号}}：{{項目名}}", "{{#操作}}{{内容}}", "{{箇所画像}}{{/操作}}", "以上", "{{一覧へのリンク}}"];
        let shared: String = strings.iter().map(|s| format!("<si><t>{}</t></si>", escape(s))).collect();
        let mut zip = zip::Writer::default();
        zip.add("[Content_Types].xml", b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/worksheets/sheet2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/drawings/drawing1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/><Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/><Override PartName=\"/xl/calcChain.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml\"/></Types>");
        zip.add("_rels/.rels", super::super::ROOT_RELS.as_bytes());
        zip.add("xl/workbook.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><bookViews><workbookView activeTab=\"1\"/></bookViews><sheets><sheet name=\"表紙\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"{{番号}}\" sheetId=\"2\" r:id=\"rId2\"/></sheets><definedNames><definedName name=\"_xlnm.Print_Titles\" localSheetId=\"1\">'{{番号}}'!$1:$1</definedName></definedNames></workbook>".as_bytes());
        zip.add("xl/_rels/workbook.xml.rels", b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet2.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" Target=\"sharedStrings.xml\"/><Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain\" Target=\"calcChain.xml\"/></Relationships>");
        zip.add("xl/sharedStrings.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">{shared}</sst>").as_bytes());
        zip.add("xl/calcChain.xml", b"<calcChain xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><c r=\"B6\" i=\"1\"/></calcChain>");
        // 表紙: 1 行目は結合した表題、3 行目が見出し、4 行目が項目の繰り返し、6 行目が合計（式）、7 行目にもとからのリンク。
        zip.add("xl/worksheets/sheet1.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><dimension ref=\"A1:C7\"/><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetData><row r=\"1\"><c r=\"A1\" s=\"5\" t=\"s\"><v>0</v></c></row><row r=\"2\"><c r=\"A2\" t=\"s\"><v>1</v></c></row><row r=\"3\"><c r=\"A3\" s=\"1\" t=\"s\"><v>2</v></c><c r=\"B3\" s=\"1\" t=\"s\"><v>3</v></c></row><row r=\"4\"><c r=\"A4\" s=\"7\" t=\"s\"><v>4</v></c><c r=\"B4\" s=\"7\" t=\"s\"><v>5</v></c></row><row r=\"6\"><c r=\"A6\" t=\"s\"><v>6</v></c><c r=\"B6\"><f>COUNTA(A4:A4)</f><v>1</v></c></row><row r=\"7\"><c r=\"A7\" t=\"s\"><v>10</v></c></row></sheetData><mergeCells count=\"2\"><mergeCell ref=\"A1:C1\"/><mergeCell ref=\"B4:C4\"/></mergeCells><hyperlinks><hyperlink ref=\"A7\" location=\"'表紙'!A1\"/></hyperlinks><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/><drawing r:id=\"rId1\"/></worksheet>".as_bytes());
        zip.add("xl/worksheets/_rels/sheet1.xml.rels", b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing\" Target=\"../drawings/drawing1.xml\"/></Relationships>");
        // ロゴ: 6 行目（0 始まりで 5）から 7 行目に置いた絵。
        zip.add("xl/drawings/drawing1.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><xdr:twoCellAnchor><xdr:from><xdr:col>3</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>6</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"7\" name=\"ロゴ\"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr/></xdr:sp><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>".as_bytes());
        // 項目のシート: 表題、戻るリンク、操作の繰り返し（画像は B 列）、以上。
        zip.add("xl/worksheets/sheet2.xml", b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetViews><sheetView tabSelected=\"1\" workbookViewId=\"0\"/></sheetViews><sheetData><row r=\"1\"><c r=\"A1\" s=\"4\" t=\"s\"><v>7</v></c></row><row r=\"2\"><c r=\"A2\" t=\"s\"><v>11</v></c></row><row r=\"3\"><c r=\"A3\" s=\"2\" t=\"s\"><v>8</v></c><c r=\"B3\" t=\"s\"><v>9</v></c></row><row r=\"4\"><c r=\"A4\" t=\"s\"><v>10</v></c></row></sheetData><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>");
        zip.finish()
    }

    fn two_items() -> crate::report::Report {
        let mut report = sample();
        let mut second = report.items[0].clone();
        second.id = "1-2".into();
        second.result = Some("NG".into());
        report.items.push(second);
        report
    }

    #[test]
    fn a_site_template_keeps_its_layout_and_repeats_the_marked_rows() {
        let xlsx = crate::report::xlsx::write(&two_items(), "", Some(&site_template())).unwrap();
        let entries = entries(&xlsx);
        let sheets = sheets(&entries);
        assert_eq!(sheets.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["表紙", "1-1", "1-2"], "項目のシートは、テンプレートの位置に項目ごと");
        let cover = &sheets[0].1;
        assert!(cover.contains("<c r=\"A1\" s=\"5\" t=\"s\"><v>0</v></c>"), "印の無いセルは触らない");
        assert!(cover.contains("<c r=\"A2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">SmokeGui48.exe 版 1.2.3.4</t></is></c>"));
        assert!(cover.contains("<c r=\"A4\" s=\"7\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1-1</t>") && cover.contains("<c r=\"B5\" s=\"7\" t=\"inlineStr\"><is><t xml:space=\"preserve\">NG</t>"), "項目ごとに行を増やす（書式は残す）");
        assert!(cover.contains("<row r=\"7\"><c r=\"A7\" t=\"s\"><v>6</v></c><c r=\"B7\"><f>COUNTA(A4:A4)</f>"), "下の行は 1 行ずれる（式の中はずらさない）");
        assert!(cover.contains("<mergeCell ref=\"A1:C1\"/><mergeCell ref=\"B4:C4\"/><mergeCell ref=\"B5:C5\"/>") && cover.contains("<mergeCells count=\"3\">"), "繰り返した行の結合は行ごと");
        assert!(cover.contains("<hyperlink ref=\"A8\" location=\"'表紙'!A1\"/><hyperlink ref=\"A4\" location=\"&#39;1-1&#39;!A1\"/><hyperlink ref=\"A5\" location=\"&#39;1-2&#39;!A1\"/>"), "もとのリンクはずらし、番号は項目のシートへ");
        assert!(!cover.contains("<dimension"));
        let logo = entry(&entries, "xl/drawings/drawing1.xml");
        assert!(logo.contains("<xdr:row>6</xdr:row>") && logo.contains("<xdr:row>7</xdr:row>"), "ロゴも 1 行下へ: {logo}");
        let item = &sheets[1].1;
        assert!(item.contains("1-1：挨拶 &lt;Hello&gt;"));
        assert!(item.contains("<hyperlink ref=\"A2\" location=\"&#39;表紙&#39;!A4\"/>") && sheets[2].1.contains("location=\"&#39;表紙&#39;!A5\""), "一覧のその項目の行へ戻る");
        assert!(item.contains("<c r=\"A3\" s=\"2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">ui_invoke greetButton</t>") && item.contains("<c r=\"A5\" t=\"s\"><v>10</v></c>"), "操作 2 つで、以上は 5 行目");
        assert!(!item.contains("tabSelected") && sheets[0].1.contains("tabSelected=\"1\""), "先頭のシートだけを選ぶ");
        let workbook = entry(&entries, "xl/workbook.xml");
        assert!(workbook.contains("<definedName name=\"_xlnm.Print_Titles\" localSheetId=\"1\">&#39;1-1&#39;!$1:$1</definedName><definedName name=\"_xlnm.Print_Titles\" localSheetId=\"2\">&#39;1-2&#39;!$1:$1</definedName>"), "印刷タイトルも複製: {workbook}");
        assert!(workbook.contains("activeTab=\"0\""));
        assert!(!entries.iter().any(|(n, _)| n == "xl/calcChain.xml" || n == "xl/worksheets/sheet2.xml"), "計算の順の記録と、複製元は消す");
        let types = entry(&entries, "[Content_Types].xml");
        assert!(!types.contains("calcChain") && !types.contains("/xl/worksheets/sheet2.xml") && types.contains("/xl/worksheets/dx_sheet1.xml"));
        // 項目のシートの画像（新しい絵）。
        assert!(entries.iter().any(|(n, d)| n.starts_with("xl/drawings/dx_drawing_new") && !n.contains("_rels") && d.contains("<xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row>")));
        assert!(item.contains("<drawing dxr:id=") || item.contains("<drawing r:id="));
    }

    #[test]
    fn unclosed_row_sections_are_explained() {
        let texts = vec![Some("{{#操作}}{{内容}}".to_string()), Some("{{時刻}}".to_string())];
        assert!(row_section(&texts, 3).unwrap_err().contains("3 行目の {{#操作}} を閉じる {{/操作}} が、同じ行の右のセルにありません"));
        let in_one_cell = vec![Some("{{#変化}}{{内容}}\n{{/変化}}".to_string())];
        assert_eq!(row_section(&in_one_cell, 1).unwrap(), None, "1 つのセルの中で閉じるものは、セルの中で繰り返す");
    }

    #[test]
    fn relative_and_absolute_targets_resolve_to_part_names() {
        assert_eq!(resolve("xl/worksheets", "../drawings/drawing1.xml"), "xl/drawings/drawing1.xml");
        assert_eq!(resolve("xl", "/xl/worksheets/dx_sheet1.xml"), "xl/worksheets/dx_sheet1.xml");
        assert_eq!(rels_path("xl/worksheets/sheet1.xml"), "xl/worksheets/_rels/sheet1.xml.rels");
    }
}
