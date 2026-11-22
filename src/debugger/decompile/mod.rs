//! PDB の無いモジュールの逆コンパイル（D-060）。
//!
//! 入力はメタデータと PE の像（ファイルの並び）だけ。COM にもセッションにも触れない。出力は、型ごとのソースの
//! ファイルの中身と、メソッドごとの「IL オフセット ⇔ 行」の対応とローカル変数の名前。エンジンはそれをファイルに書き、
//! 合成した PDB（`Pdb::synthetic`）にして、PDB のあるモジュールと同じ道でブレークポイントやステップに使う。
//!
//! 流れ: 型とメンバーの宣言（`decl.rs`）→ 本体の式の木と平らな形（`build`）→ if / while への組み直し（`structure.rs`）→
//! C# の印字（`csharp.rs`）。戻せないメソッドの本体は IL の行（`ilview.rs`）。書く順の取りまとめは `writer.rs`。

mod ast;
pub mod body;
mod build;
mod closures;
mod csharp;
mod data;
pub mod decl;
pub mod flow;
mod ilview;
pub mod lookup;
pub mod names;
pub mod sig;
mod structure;
mod sugar;
mod writer;

use crate::debugger::metadata::{Metadata, FIELD, METHOD_DEF, NESTED_CLASS, TYPE_DEF};
use names::Names;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 逆コンパイラの版。出力の形を変えたら上げる（キャッシュのフォルダーの名前に入る）。
pub const VERSION: u32 = 6;

/// ソースのファイル 1 つ。
#[derive(Debug, Clone)]
pub struct Document {
    /// 出力のフォルダーからの相対パス（区切りは `\`）。
    pub path: String,
    pub text: String,
}

/// 系列点 1 つ。行と列は 1 始まり。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub il_offset: u32,
    pub line: u32,
    pub start_column: u32,
    pub end_column: u32,
}

/// メソッド 1 つ分の、ソースとの対応。
#[derive(Debug, Clone)]
pub struct Method {
    /// MethodDef の行番号。
    pub row: u32,
    /// `Decompiled::documents` の添字。
    pub document: usize,
    /// IL オフセットの昇順。
    pub points: Vec<Point>,
    /// ローカル変数の「番号、名前」。
    pub locals: Vec<(u32, String)>,
}

#[derive(Debug, Default)]
pub struct Decompiled {
    pub documents: Vec<Document>,
    pub methods: Vec<Method>,
}

/// 書いている途中のファイル。行の番号を数えながら足す。
pub(crate) struct Text {
    out: String,
    /// これまでに書いた行の数。
    lines: u32,
    /// 複数の行で書いたラムダの本体の目印（`MARK`）があった行と字下げ（ラムダのメソッドの行ごと）。ラムダの系列点は、
    /// 書き終えてから、この行を基に置き直す（`csharp.rs` の `Writer::resolve_lambdas`）。
    marks: HashMap<u32, (u32, u32)>,
}

/// 複数の行で書くラムダの本体の目印: `MARK` + メソッドの行 + `MARK`。`Text::line` が取り除く。
pub(crate) const MARK: char = '\u{1}';

impl Text {
    fn new() -> Text {
        Text { out: String::new(), lines: 0, marks: HashMap::new() }
    }

    /// 字下げ `depth` で 1 行足す（中に改行があれば、どの行も同じ字下げで）。最初の行の「番号、始まりの列、終わりの列」を
    /// 返す。
    fn line(&mut self, depth: usize, content: &str) -> (u32, u32, u32) {
        let indent = depth * 4;
        let mut first = None;
        for segment in content.split('\n') {
            let segment = self.take_marks(segment, indent as u32);
            for _ in 0..indent {
                self.out.push(' ');
            }
            self.out.push_str(&segment);
            self.out.push_str("\r\n");
            self.lines += 1;
            first.get_or_insert((self.lines, indent as u32 + 1, (indent + segment.chars().count()) as u32 + 1));
        }
        first.unwrap_or((self.lines, indent as u32 + 1, indent as u32 + 1))
    }

    /// 行の中の目印を取り除き、次に書く行の番号と字下げを覚える。
    fn take_marks(&mut self, segment: &str, indent: u32) -> String {
        if !segment.contains(MARK) {
            return segment.to_string();
        }
        let mut out = String::new();
        let mut parts = segment.split(MARK);
        out.push_str(parts.next().unwrap_or(""));
        while let (Some(row), Some(rest)) = (parts.next(), parts.next()) {
            if let Ok(row) = row.parse::<u32>() {
                self.marks.insert(row, (self.lines + 1, indent));
            }
            out.push_str(rest);
        }
        out
    }

    fn blank(&mut self) {
        self.out.push_str("\r\n");
        self.lines += 1;
    }
}

/// モジュールを逆コンパイルする。`image` は PE の像（ファイルの並び）。
pub fn module(md: &Metadata, image: &[u8]) -> Decompiled {
    let names = Names::new(md);
    let mut nested: HashMap<u32, Vec<u32>> = HashMap::new();
    for row in 1..=md.rows(NESTED_CLASS) {
        nested.entry(md.cell(NESTED_CLASS, row, 1)).or_default().push(md.cell(NESTED_CLASS, row, 0));
    }
    for list in nested.values_mut() {
        list.sort_unstable();
    }
    let mut writer = writer::ModuleWriter { names: &names, image, nested, methods: Vec::new(), written: HashSet::new(), inlined: HashSet::new(), moved: HashMap::new(), initializer_points: HashMap::new() };
    let mut out = Decompiled::default();
    let mut paths = HashSet::new();
    for row in 1..=md.rows(TYPE_DEF) {
        if md.enclosing_type(row).is_some() || (row == 1 && is_empty(md, row)) {
            continue;
        }
        names.take_namespaces();
        names.enter_type(row);
        let namespace = md.string(md.cell(TYPE_DEF, row, 2)).to_string();
        let mut body = Text::new();
        let depth = if namespace.is_empty() {
            0
        } else {
            body.line(0, &format!("namespace {namespace}"));
            body.line(0, "{");
            1
        };
        writer.write_type(row, depth, &mut body);
        if !namespace.is_empty() {
            body.line(0, "}");
        }

        // 頭（説明と using）は、本体を書き終えてから分かる。行の番号は、頭の分だけずらす。
        let mut head = Text::new();
        head.line(0, tr_str!("// Decompiled source (made by netdbg from a module without a PDB)."));
        head.line(0, tr_str!("// Editing it does not change the running program."));
        head.blank();
        let usings: Vec<String> = names.take_namespaces().into_iter().filter(|ns| !is_same_or_enclosing(ns, &namespace)).collect();
        for using in &usings {
            head.line(0, &format!("using {using};"));
        }
        if !usings.is_empty() {
            head.blank();
        }
        let shift = head.lines;
        let document = out.documents.len();
        for mut method in std::mem::take(&mut writer.methods) {
            method.document = document;
            for point in &mut method.points {
                point.line += shift;
            }
            out.methods.push(method);
        }
        out.documents.push(Document { path: unique_path(&mut paths, &namespace, &file_name(md, row)), text: head.out + &body.out });
    }
    out
}

/// 逆コンパイルしたソースを、フォルダー `root` の下に書く。書いたファイルのパスを、`documents` と同じ順で返す。
pub fn write(decompiled: &Decompiled, root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(decompiled.documents.len());
    for document in &decompiled.documents {
        let path = root.join(&document.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &document.text)?;
        written.push(path);
    }
    Ok(written)
}

/// `--decompile`: アセンブリのファイルを逆コンパイルして、フォルダーに書く。結果の要約を返す。
pub fn to_folder(assembly: &Path, output: &Path) -> Result<String, String> {
    let image = std::fs::read(assembly).map_err(|e| tr!("Cannot read {path}: {error}", path = assembly.display(), error = e))?;
    let md = Metadata::from_pe(&image)?;
    let started = std::time::Instant::now();
    let decompiled = module(&md, &image);
    let elapsed = started.elapsed();
    write(&decompiled, output).map_err(|e| tr!("Cannot write {path}: {error}", path = output.display(), error = e))?;
    let points: usize = decompiled.methods.iter().map(|m| m.points.len()).sum();
    Ok(tr!("{files} files, {methods} methods, {points} sequence points, decompiled in {ms} ms", files = decompiled.documents.len(), methods = decompiled.methods.len(), points = points, ms = format!("{:.1}", elapsed.as_secs_f64() * 1000.0)))
}

/// .NET と .NET Framework の部品の名前（自己完結のアプリでは、アプリのフォルダーに並ぶ）。逆コンパイル、カバレッジ、
/// 差分の対象にしない。
pub fn is_platform_assembly(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["system.", "microsoft.", "mscorlib", "netstandard", "windowsbase", "presentationcore", "presentationframework", "uiautomation", "accessibility", "direct", "reachframework"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// メソッドの IL の流れ（カバレッジの分かれ道の解析が使う。D-079）。
pub struct MethodFlow {
    pub code: Vec<crate::debugger::il::Instruction>,
    /// 命令ごとの、実行する前の評価スタックの深さ（たどり着かない命令は `None`）。
    pub depths: Vec<Option<u32>>,
    /// 例外のハンドラーとフィルターの頭（どの命令からも流れてこないが、入ってくる道がある所）。
    pub handlers: Vec<u32>,
}

/// MethodDef の行 `row` の本体を読み、命令とスタックの深さにする。本体の無いメソッド（abstract、extern）は `None`。
pub fn method_flow(names: &Names, image: &[u8], row: u32) -> Option<MethodFlow> {
    let rva = names.md.cell(METHOD_DEF, row, 0);
    if rva == 0 {
        return None;
    }
    let body = body::read(image, rva).ok()?;
    let signature = names.method_def_sig(row)?;
    let code: Vec<_> = crate::debugger::il::instructions(body.code).collect();
    let effect = |instruction: &crate::debugger::il::Instruction| ilview::stack_effect(names, &signature, instruction);
    let depths = flow::stack_depths(&code, &body.clauses, &effect);
    let handlers = body
        .clauses
        .iter()
        .flat_map(|clause| std::iter::once(clause.handler.start).chain((clause.kind == body::ClauseKind::Filter).then_some(clause.filter_start)))
        .collect();
    Some(MethodFlow { code, depths, handlers })
}

/// メソッドの本体の IL の行（差し替わった exe の差分が比べる。D-080）。逆コンパイラの IL の書き方（`ilview.rs`）で、
/// トークンは名前に書き直してある。命令の位置（`IL_0012`）は、何番目の命令か（`#3`）にする（前の命令の長さが
/// 変わっても、同じ本体は同じ行になる）。本体の無いメソッド（abstract、extern）は `None`。
pub fn body_lines(names: &Names, image: &[u8], row: u32) -> Option<Vec<String>> {
    let rva = names.md.cell(METHOD_DEF, row, 0);
    if rva == 0 {
        return None;
    }
    let body = body::read(image, rva).ok()?;
    let owner = names.md.declaring_type(row).unwrap_or(0);
    let generics = names::Generics { types: names.type_generics(owner), methods: names.method_generics(row) };
    let mut text = Text::new();
    ilview::write_body(names, row, &body, &generics, 0, &mut text);
    let offsets: Vec<u32> = crate::debugger::il::instructions(body.code).map(|i| i.offset).collect();
    Some(text.out.lines().map(|line| positions_to_indexes(line.trim_start(), &offsets)).collect())
}

/// 行の中の `IL_xxxx` を、命令の番号 `#n` にする。行の頭の `// IL_xxxx: ` は外す（順番は行の順で分かる）。
fn positions_to_indexes(line: &str, offsets: &[u32]) -> String {
    let line = match line.strip_prefix("// IL_").and_then(|rest| rest.split_once(": ")) {
        Some((_, instruction)) => instruction,
        None => line,
    };
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("IL_") {
        out.push_str(&rest[..at]);
        let digits: String = rest[at + 3..].chars().take_while(char::is_ascii_hexdigit).collect();
        match u32::from_str_radix(&digits, 16).ok().filter(|_| !digits.is_empty()) {
            Some(offset) => {
                let index = offsets.binary_search(&offset).map_or_else(|_| format!("?{offset:x}"), |i| i.to_string());
                out.push('#');
                out.push_str(&index);
                rest = &rest[at + 3 + digits.len()..];
            }
            None => {
                out.push_str("IL_");
                rest = &rest[at + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// メソッドの引数の型の並び（`(int, string)`）。多重定義を見分ける印に使う（カバレッジの `signature`、差分の鍵）。
/// 型引数だけが違う多重定義（`M<T>(string)` と `M(string)`）のために型引数を前に付け（`<T>(string)`）、戻り値だけが違う
/// 変換演算子（`op_Explicit`）のために、その戻り値の型を後ろに付ける（`(JToken) : int`）。
pub fn parameter_list(names: &Names, row: u32) -> String {
    let Some(signature) = names.method_def_sig(row) else { return "()".into() };
    let owner = names.md.declaring_type(row).unwrap_or(0);
    let generics = names::Generics { types: names.type_generics(owner), methods: names.method_generics(row) };
    let types: Vec<String> = signature.params.iter().map(|ty| names.ty(ty, &generics)).collect();
    let type_parameters = if generics.methods.is_empty() { String::new() } else { format!("<{}>", generics.methods.join(", ")) };
    let conversion = matches!(names.md.method_simple_name(row), Some("op_Explicit" | "op_Implicit"));
    let returns = if conversion { format!(" : {}", names.ty(&signature.ret, &generics)) } else { String::new() };
    format!("{type_parameters}({}){returns}", types.join(", "))
}

/// `<Module>` 型に、メンバーが無いか。
fn is_empty(md: &Metadata, row: u32) -> bool {
    md.child_rows(TYPE_DEF, row, 4, FIELD).next().is_none() && md.child_rows(TYPE_DEF, row, 5, METHOD_DEF).next().is_none()
}

/// `used` が、`namespace` そのものか、それを含む名前空間（書かなくても見える）か。
fn is_same_or_enclosing(used: &str, namespace: &str) -> bool {
    namespace == used || namespace.strip_prefix(used).is_some_and(|rest| rest.starts_with('.'))
}

/// 型のファイル名（ファイル名に使えない文字は `_` にする）。
fn file_name(md: &Metadata, row: u32) -> String {
    let raw = md.string(md.cell(TYPE_DEF, row, 1));
    let name: String = raw.chars().map(|c| if matches!(c, '<' | '>' | '|' | ':' | '*' | '?' | '"' | '/' | '\\') { '_' } else { c }).collect();
    if name.is_empty() { "_".into() } else { name }
}

/// 名前空間のフォルダーの下の、重ならないパス。
fn unique_path(paths: &mut HashSet<String>, namespace: &str, name: &str) -> String {
    let folder: String = namespace.split('.').filter(|part| !part.is_empty()).map(|part| format!("{part}\\")).collect();
    let mut candidate = format!("{folder}{name}.cs");
    let mut counter = 2;
    while !paths.insert(candidate.to_lowercase()) {
        candidate = format!("{folder}{name}_{counter}.cs");
        counter += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_positions_become_indexes() {
        let offsets = [0, 1, 6, 8];
        assert_eq!(positions_to_indexes("// IL_0006: brtrue.s IL_0008", &offsets), "brtrue.s #3");
        assert_eq!(positions_to_indexes("// IL_0000: switch (IL_0001, IL_0008)", &offsets), "switch (#1, #3)");
        assert_eq!(positions_to_indexes("// try {", &offsets), "// try {");
        assert_eq!(positions_to_indexes("// IL_0001: ldstr \"IL_x\"", &offsets), "ldstr \"IL_x\"");
    }

    #[test]
    fn enclosing_namespaces_need_no_using() {
        assert!(is_same_or_enclosing("App", "App.Models"));
        assert!(is_same_or_enclosing("App.Models", "App.Models"));
        assert!(!is_same_or_enclosing("App.Model", "App.Models"));
        assert!(!is_same_or_enclosing("System", "App"));
    }

    #[test]
    fn paths_do_not_collide() {
        let mut paths = HashSet::new();
        assert_eq!(unique_path(&mut paths, "A.B", "List`1"), "A\\B\\List`1.cs");
        assert_eq!(unique_path(&mut paths, "A.B", "list`1"), "A\\B\\list`1_2.cs");
        assert_eq!(unique_path(&mut paths, "", "Program"), "Program.cs");
    }

    #[test]
    fn lines_know_their_numbers_and_columns() {
        let mut text = Text::new();
        assert_eq!(text.line(0, "a"), (1, 1, 2));
        text.blank();
        assert_eq!(text.line(2, "// IL_0000: nop"), (3, 9, 24));
    }
}
