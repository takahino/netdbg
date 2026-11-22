//! 差し替わった exe の差分（B2、D-080）。ICorDebug を知らない部分。
//!
//! 前後 2 つのアセンブリの、メソッドの IL を比べる。トークン（メタデータの番号）は名前に、命令の位置は何番目の命令かに
//! 書き直してから比べる（`decompile::body_lines`）。だから、ほかのメソッドが足されて番号や位置がずれても「変わった」に
//! ならず、逆コンパイラの出来にも PDB にも左右されない。
//!
//! 単位は、カバレッジ（B1、`coverage.rs`）と同じ「利用者の書いたメソッド」: async の本体、ラムダ、ローカル関数は、
//! それを書いたメソッドに含めて比べる。コンパイラが付ける名前の番号（`<Save>b__3_0` の `3_0`）は比べない。
//! カバレッジの記録と重ねて、変わったのに通っていないメソッド（再試験の範囲）を出す。

use crate::debugger::coverage::{user_names, Coverage};
use crate::debugger::decompile::{self, names::Names};
use crate::debugger::metadata::{Metadata, METHOD_DEF, TYPE_DEF};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// メソッドの本体 1 つ（利用者のメソッドそのもの、またはコンパイラが作った async の本体やラムダ）。
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub token: u32,
    /// この本体の引数の並び（カバレッジの `method@signature` と同じ）。
    pub signature: String,
    pub lines: Vec<String>,
}

/// 利用者のメソッド 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub class: String,
    pub name: String,
    pub signature: String,
    /// メソッドそのもののトークン（コンパイラが作った本体しか無いときは、その最初のもの）。
    pub token: u32,
    /// メソッドそのものの本体（無ければ `None`）。
    pub own: Option<Body>,
    /// コンパイラが作った本体（並びは行の順。前後で並びが変わっても同じに見る）。
    pub generated: Vec<Body>,
}

/// アセンブリ 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    /// ファイル名から拡張子を除いたもの（カバレッジの package と同じ）。
    pub name: String,
    pub path: String,
    pub methods: Vec<Method>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Changed,
    Added,
    Removed,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Changed => "changed",
            Kind::Added => "added",
            Kind::Removed => "removed",
        }
    }
}

/// 変わったメソッド 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub kind: Kind,
    pub assembly: String,
    pub class: String,
    pub name: String,
    pub signature: String,
    /// 新しい方のトークン（消えたものは古い方）。
    pub token: u32,
    /// 新しい方の、変わった本体（カバレッジと重ねるのに使う）。消えたものは空。
    pub changed_bodies: Vec<(u32, String)>,
    /// 変わった本体を、試験で通ったか（カバレッジと重ねたとき。記録に無いアセンブリなら `None`）。
    pub executed: Option<bool>,
}

impl Change {
    /// 再試験の範囲（変わったか足されたのに、通っていない）。
    pub fn needs_retest(&self) -> bool {
        self.kind != Kind::Removed && self.executed == Some(false)
    }
}

/// パスのアセンブリを読む。フォルダーなら、中のマネージドのアセンブリ（.NET の部品は除く）。exe が .NET の
/// ネイティブのホスト（apphost）なら、同じ名前の dll。
pub fn read(path: &Path) -> Result<Vec<Assembly>, String> {
    let files = if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| tr!("Cannot read {path}: {error}", path = path.display(), error = e))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll") || e.eq_ignore_ascii_case("exe")))
            .filter(|p| !decompile::is_platform_assembly(&p.file_name().unwrap_or_default().to_string_lossy()))
            .collect();
        files.sort();
        files
    } else if path.is_file() {
        vec![managed_file(path)]
    } else {
        return Err(tr!("{name} is missing.", name = path.display()));
    };
    let mut assemblies = Vec::new();
    for file in files {
        let Ok(image) = std::fs::read(&file) else { continue };
        // ネイティブの exe や、メタデータの無い dll は飛ばす。
        let Ok(md) = Metadata::from_pe(&image) else { continue };
        let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        assemblies.push(Assembly { name, path: file.to_string_lossy().into_owned(), methods: methods(&md, &image) });
    }
    if assemblies.is_empty() {
        return Err(tr!("{path} has no .NET assemblies.", path = path.display()));
    }
    Ok(assemblies)
}

/// .NET の apphost（ネイティブの exe）なら、本体の dll。
fn managed_file(path: &Path) -> PathBuf {
    let managed = std::fs::read(path).ok().and_then(|data| crate::debugger::target::inspect(&data)).is_some_and(|e| e.managed);
    let dll = path.with_extension("dll");
    if !managed && dll.is_file() { dll } else { path.to_path_buf() }
}

/// アセンブリのメソッド（利用者のメソッドにまとめたもの）。
pub fn methods(md: &Metadata, image: &[u8]) -> Vec<Method> {
    let names = Names::new(md);
    let mut real: Vec<Method> = Vec::new();
    let mut generated: Vec<(String, String, Body)> = Vec::new();
    for row in 1..=md.rows(METHOD_DEF) {
        let Some(lines) = decompile::body_lines(&names, image, row) else { continue };
        let mut types = Vec::new();
        let mut ty = md.declaring_type(row);
        while let Some(type_row) = ty {
            types.push((md.string(md.cell(TYPE_DEF, type_row, 1)), md.type_name(type_row)));
            ty = md.enclosing_type(type_row);
        }
        let raw = md.method_simple_name(row).unwrap_or_default();
        let is_generated = raw.starts_with('<') || types.iter().any(|(raw, _)| raw.starts_with('<'));
        let (class, name) = user_names(raw, &types);
        let body = Body { token: 0x0600_0000 | row, signature: decompile::parameter_list(&names, row), lines: lines.iter().map(|l| without_generated_numbers(l)).collect() };
        if is_generated {
            generated.push((class, name, body));
        } else {
            real.push(Method { class, name, signature: body.signature.clone(), token: body.token, own: Some(body), generated: Vec::new() });
        }
    }
    // コンパイラが作った本体を、それを書いたメソッドに付ける。同じ名前の多重定義があれば、どれのものか分からないので、
    // すべてに付ける（変わったときに、多めに出す方に倒す）。
    for (class, name, body) in generated {
        let mut attached = false;
        for method in real.iter_mut().filter(|m| m.class == class && m.name == name) {
            method.generated.push(body.clone());
            attached = true;
        }
        if !attached {
            real.push(Method { class, name, signature: String::new(), token: body.token, own: None, generated: vec![body] });
        }
    }
    for method in &mut real {
        method.generated.sort_by(|a, b| a.lines.cmp(&b.lines));
    }
    // それでも同じ鍵になるもの（引数の型が同じ名前に書ける多重定義など）は、宣言の順の番号で分ける。
    let mut seen: HashMap<(String, String, String), usize> = HashMap::new();
    for method in &mut real {
        let count = seen.entry((method.class.clone(), method.name.clone(), method.signature.clone())).or_insert(0);
        *count += 1;
        if *count > 1 {
            method.signature = format!("{} #{count}", method.signature);
        }
    }
    real
}

/// コンパイラが作った名前（`<Save>b__3_0`、`<>c__DisplayClass3_0`、`<Load>d__5`。逆コンパイラの書き方では `<` と `>` が
/// `_` になる）の番号を外す。番号は、同じ型のほかのメソッドやラムダを足すとずれる。
fn without_generated_numbers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.contains("__") || word.starts_with('<') {
            out.extend(word.chars().filter(|c| !c.is_ascii_digit()));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    let mut in_string = false;
    for c in line.chars() {
        // 文字列の定数の中は、そのまま比べる。
        if c == '"' {
            flush(&mut word, &mut out);
            in_string = !in_string;
            out.push(c);
            continue;
        }
        if !in_string && (c.is_alphanumeric() || matches!(c, '_' | '<' | '>' | '|' | '$' | '`')) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// 前後のアセンブリ（ファイル名で組にする）のメソッドを比べる。
pub fn compare(old: &[Assembly], new: &[Assembly]) -> Vec<Change> {
    let key = |name: &str| name.to_lowercase();
    let old_by_name: HashMap<String, &Assembly> = old.iter().map(|a| (key(&a.name), a)).collect();
    let new_names: HashSet<String> = new.iter().map(|a| key(&a.name)).collect();
    let mut changes = Vec::new();
    for assembly in new {
        let before = old_by_name.get(&key(&assembly.name)).map_or(&[][..], |a| &a.methods[..]);
        compare_methods(&assembly.name, before, &assembly.methods, &mut changes);
    }
    for assembly in old.iter().filter(|a| !new_names.contains(&key(&a.name))) {
        compare_methods(&assembly.name, &assembly.methods, &[], &mut changes);
    }
    changes.sort_by(|a, b| (&a.assembly, &a.class, &a.name, &a.signature, a.kind).cmp(&(&b.assembly, &b.class, &b.name, &b.signature, b.kind)));
    changes
}

fn compare_methods(assembly: &str, old: &[Method], new: &[Method], out: &mut Vec<Change>) {
    let key = |m: &Method| (m.class.clone(), m.name.clone(), m.signature.clone());
    let old_by_key: BTreeMap<_, &Method> = old.iter().map(|m| (key(m), m)).collect();
    let new_keys: HashSet<_> = new.iter().map(key).collect();
    let change = |kind, method: &Method, changed_bodies| Change {
        kind,
        assembly: assembly.to_string(),
        class: method.class.clone(),
        name: method.name.clone(),
        signature: method.signature.clone(),
        token: method.token,
        changed_bodies,
        executed: None,
    };
    for method in new {
        match old_by_key.get(&key(method)) {
            None => out.push(change(Kind::Added, method, all_bodies(method))),
            Some(before) => {
                let changed = changed_bodies(before, method);
                if !changed.is_empty() || before.generated.len() != method.generated.len() {
                    out.push(change(Kind::Changed, method, changed));
                }
            }
        }
    }
    for method in old.iter().filter(|m| !new_keys.contains(&key(m))) {
        out.push(change(Kind::Removed, method, Vec::new()));
    }
}

fn all_bodies(method: &Method) -> Vec<(u32, String)> {
    method.own.iter().chain(&method.generated).map(|b| (b.token, b.signature.clone())).collect()
}

/// 新しい方の本体のうち、古い方に同じものが無いもの。
fn changed_bodies(old: &Method, new: &Method) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if old.own.as_ref().map(|b| &b.lines) != new.own.as_ref().map(|b| &b.lines) {
        out.extend(new.own.iter().map(|b| (b.token, b.signature.clone())));
    }
    let mut remaining: Vec<&Vec<String>> = old.generated.iter().map(|b| &b.lines).collect();
    for body in &new.generated {
        match remaining.iter().position(|lines| **lines == body.lines) {
            Some(at) => {
                remaining.remove(at);
            }
            None => out.push((body.token, body.signature.clone())),
        }
    }
    out
}

/// 通ったかの記録。カバレッジの記録（いまのセッション）か、書き出した Cobertura XML。
pub enum Executed<'a> {
    /// いまのセッション（トークンで引く。新しい方の exe がデバッグしているものと同じとき）。
    Session(&'a Coverage),
    /// Cobertura XML の、（package、class、method、signature）ごとの、行を 1 つでも通ったか。
    Cobertura(HashMap<(String, String, String, String), bool>),
}

impl Executed<'_> {
    /// 本体（トークンと引数の並び）を通ったか。記録にそのアセンブリが無ければ `None`。
    fn body(&self, change: &Change, token: u32, signature: &str) -> Option<bool> {
        match self {
            Executed::Session(coverage) => {
                let module = coverage.modules.iter().find(|m| m.assembly.eq_ignore_ascii_case(&change.assembly))?;
                Some(module.methods.iter().find(|m| m.token == token).is_some_and(|m| m.hit.iter().any(|&h| h)))
            }
            Executed::Cobertura(methods) => {
                if !methods.keys().any(|(package, ..)| package.eq_ignore_ascii_case(&change.assembly)) {
                    return None;
                }
                let key = (change.assembly.to_lowercase(), change.class.clone(), change.name.clone(), signature.to_string());
                Some(methods.get(&key).copied().unwrap_or(false))
            }
        }
    }
}

/// 変わったメソッドに、通ったかを付ける（変わった本体がすべて通っていれば通った）。
pub fn overlay(changes: &mut [Change], executed: &Executed) {
    for change in changes.iter_mut().filter(|c| c.kind != Kind::Removed) {
        let results: Vec<Option<bool>> = change.changed_bodies.iter().map(|(token, signature)| executed.body(change, *token, signature)).collect();
        change.executed = if results.iter().any(Option::is_none) || results.is_empty() { None } else { Some(results.iter().all(|r| *r == Some(true))) };
    }
}

/// Cobertura XML から、メソッドごとの「行を 1 つでも通ったか」。B1 が書いたもの（`coverage::Coverage::to_cobertura`）を読む。
pub fn read_cobertura(xml: &str) -> HashMap<(String, String, String, String), bool> {
    let mut out = HashMap::new();
    let (mut package, mut class) = (String::new(), String::new());
    let mut method: Option<(String, String, String, String)> = None;
    for tag in tags(xml) {
        match tag.name.as_str() {
            "package" => package = tag.attribute("name").to_lowercase(),
            "class" => class = tag.attribute("name"),
            "method" => {
                let key = (package.clone(), class.clone(), tag.attribute("name"), tag.attribute("signature"));
                out.entry(key.clone()).or_insert(false);
                method = Some(key);
            }
            "/method" => method = None,
            "line" => {
                if let Some(key) = &method {
                    if tag.attribute("hits").parse::<u64>().unwrap_or(0) > 0 {
                        out.insert(key.clone(), true);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// XML のタグ（名前と属性）。閉じタグの名前は `/name`。
struct Tag {
    name: String,
    attributes: Vec<(String, String)>,
}

impl Tag {
    fn attribute(&self, name: &str) -> String {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_default()
    }
}

fn tags(xml: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else { break };
        let inner = rest[start + 1..start + end].trim_end_matches('/').trim();
        rest = &rest[start + end + 1..];
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        let (name, attributes) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        out.push(Tag { name: name.to_string(), attributes: parse_attributes(attributes) });
    }
    out
}

fn parse_attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quoted) = after.strip_prefix('"') else { break };
        let Some(close) = quoted.find('"') else { break };
        out.push((name, unescape(&quoted[..close])));
        rest = &quoted[close + 1..];
    }
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// 差分の要約（数）。
pub fn summary(changes: &[Change]) -> Value {
    let count = |kind| changes.iter().filter(|c| c.kind == kind).count();
    json!({
        "changed": count(Kind::Changed),
        "added": count(Kind::Added),
        "removed": count(Kind::Removed),
        "retest": changes.iter().filter(|c| c.needs_retest()).count(),
    })
}

/// 差分 1 つの JSON。
pub fn change_json(change: &Change) -> Value {
    let mut body = json!({
        "kind": change.kind.as_str(),
        "assembly": change.assembly,
        "class": change.class,
        "name": change.name,
        "signature": change.signature,
    });
    if let Some(executed) = change.executed {
        body["executed"] = json!(executed);
        body["retest"] = json!(change.needs_retest());
    }
    body
}

/// Markdown の報告（`--diff`）。
pub fn to_markdown(old: &str, new: &str, changes: &[Change]) -> String {
    let mut out = tr!("# Diff\n\n- Before: {old}\n- After: {new}\n\n", old = old, new = new);
    let count = |kind| changes.iter().filter(|c| c.kind == kind).count();
    out.push_str(&tr!("Changed {changed}, added {added}, removed {removed}", changed = count(Kind::Changed), added = count(Kind::Added), removed = count(Kind::Removed)));
    if changes.iter().any(|c| c.executed.is_some()) {
        out.push_str(&tr!(", retest scope {n}", n = changes.iter().filter(|c| c.needs_retest()).count()));
    }
    out.push_str(&tr!("\n\n| Kind | Method | Tests |\n|---|---|---|\n"));
    for change in changes {
        let kind = match change.kind {
            Kind::Changed => tr_str!("Changed"),
            Kind::Added => tr_str!("Added"),
            Kind::Removed => tr_str!("Removed"),
        };
        let tested = match change.executed {
            Some(true) => tr_str!("Run"),
            Some(false) => tr_str!("**Retest**"),
            None => "",
        };
        out.push_str(&format!("| {kind} | `{}.{}{}` ({}) | {tested} |\n", change.class, change.name, change.signature, change.assembly));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(class: &str, name: &str, own: &[&str], generated: &[&[&str]]) -> Method {
        let body = |token: u32, lines: &[&str]| Body { token, signature: "()".into(), lines: lines.iter().map(|l| l.to_string()).collect() };
        Method {
            class: class.into(),
            name: name.into(),
            signature: "()".into(),
            token: 1,
            own: Some(body(1, own)),
            generated: generated.iter().enumerate().map(|(i, lines)| body(10 + i as u32, lines)).collect(),
        }
    }

    fn assembly(methods: Vec<Method>) -> Assembly {
        Assembly { name: "App".into(), path: String::new(), methods }
    }

    #[test]
    fn only_methods_whose_bodies_differ_are_changed() {
        let old = assembly(vec![method("P", "A", &["ldc.i4.1", "ret"], &[]), method("P", "B", &["ldstr \"x\"", "ret"], &[]), method("P", "Gone", &["ret"], &[])]);
        let new = assembly(vec![method("P", "A", &["ldc.i4.1", "ret"], &[]), method("P", "B", &["ldstr \"y\"", "ret"], &[]), method("P", "New", &["ret"], &[])]);
        let changes = compare(&[old], &[new]);
        assert_eq!(changes.iter().map(|c| (c.name.as_str(), c.kind)).collect::<Vec<_>>(), [("B", Kind::Changed), ("Gone", Kind::Removed), ("New", Kind::Added)]);
    }

    #[test]
    fn a_changed_lambda_changes_its_method_and_order_does_not_matter() {
        let old = assembly(vec![method("P", "A", &["ret"], &[&["ldc.i4.1"], &["ldc.i4.2"]])]);
        let reordered = assembly(vec![method("P", "A", &["ret"], &[&["ldc.i4.2"], &["ldc.i4.1"]])]);
        assert!(compare(std::slice::from_ref(&old), &[reordered]).is_empty());
        let edited = assembly(vec![method("P", "A", &["ret"], &[&["ldc.i4.1"], &["ldc.i4.3"]])]);
        let changes = compare(&[old], &[edited]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].changed_bodies, [(11, "()".to_string())], "変わったのはラムダだけ");
    }

    #[test]
    fn numbers_in_compiler_generated_names_are_ignored_but_strings_are_kept() {
        assert_eq!(without_generated_numbers("ldftn int Program.__c._Sum_b__3_0(int)"), without_generated_numbers("ldftn int Program.__c._Sum_b__4_0(int)"));
        assert_eq!(without_generated_numbers("newobj Program.__c__DisplayClass2_0..ctor()"), "newobj Program.__c__DisplayClass_..ctor()");
        assert_ne!(without_generated_numbers("ldstr \"a__1\""), without_generated_numbers("ldstr \"a__2\""));
        assert_eq!(without_generated_numbers("ldc.i4.s 10"), "ldc.i4.s 10");
    }

    #[test]
    fn the_retest_range_is_what_changed_and_did_not_run() {
        let old = assembly(vec![method("P", "A", &["ldc.i4.1"], &[]), method("P", "B", &["ldc.i4.1"], &[])]);
        let new = assembly(vec![method("P", "A", &["ldc.i4.2"], &[]), method("P", "B", &["ldc.i4.2"], &[]), method("P", "C", &["ret"], &[])]);
        let mut changes = compare(&[old], &[new]);
        let xml = r#"<coverage><packages><package name="App"><classes><class name="P" filename="P.cs"><methods>
            <method name="A" signature="()"><lines><line number="3" hits="1" branch="false" /></lines></method>
            <method name="B" signature="()"><lines><line number="5" hits="0" branch="false" /></lines></method>
            </methods></class></classes></package></packages></coverage>"#;
        overlay(&mut changes, &Executed::Cobertura(read_cobertura(xml)));
        let retest: Vec<&str> = changes.iter().filter(|c| c.needs_retest()).map(|c| c.name.as_str()).collect();
        assert_eq!(retest, ["B", "C"], "A は通った。足された C は記録に無いので通っていない");
        assert_eq!(summary(&changes)["retest"], 2);
    }

    #[test]
    fn cobertura_attributes_are_unescaped() {
        let read = read_cobertura(r#"<package name="App"><class name="P&lt;T&gt;"><methods><method name="M" signature="(List&lt;int&gt;)"><lines><line number="1" hits="2"/></lines></method></methods></class></package>"#);
        assert_eq!(read.get(&("app".into(), "P<T>".into(), "M".into(), "(List<int>)".into())), Some(&true));
    }
}
