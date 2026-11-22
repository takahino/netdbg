//! 型とメンバーを、ソースのファイルに書く。メソッドの本体は C# で書き（`csharp.rs`）、戻せなければ IL で書く（`ilview.rs`）。

use super::body;
use super::decl::{Event, Property, TypeKind};
use super::names::{Generics, Names};
use super::ast::{Expr, StmtKind};
use super::{csharp, ilview, Method, Text};
use crate::debugger::metadata::{FIELD, METHOD_DEF, TYPE_DEF};
use std::collections::{HashMap, HashSet};

/// メソッドの本体を書いた結果。
#[derive(Default)]
pub struct BodyMap {
    /// 系列点（IL オフセットの昇順）。
    pub points: Vec<super::Point>,
    /// ローカル変数の「番号、名前」。
    pub locals: Vec<(u32, String)>,
    /// 閉じ括弧の行に打つ系列点の IL オフセット（外した末尾の `return;`）。
    pub end: Vec<u32>,
    /// 式の中にラムダとして書いたメソッド。
    pub lambdas: Vec<InlinedLambda>,
}

/// 式の中にラムダとして書いたメソッド 1 つ: 行、系列点（ラムダを書いた文の行）、ローカル変数の名前。
pub struct InlinedLambda {
    pub row: u32,
    pub points: Vec<super::Point>,
    pub locals: Vec<(u32, String)>,
}

/// モジュール 1 つ分を書く文脈。書いたメソッドの系列点を `methods` に集める。
pub struct ModuleWriter<'a> {
    pub names: &'a Names<'a>,
    /// PE の像（ファイルの並び）。
    pub image: &'a [u8],
    /// 型の行 → 入れ子の型の行。
    pub nested: HashMap<u32, Vec<u32>>,
    pub methods: Vec<Method>,
    /// 書いたメソッドの行。
    pub written: HashSet<u32>,
    /// ラムダとして式の中に書いたメソッドの行（宣言は書かない）。
    pub inlined: HashSet<u32>,
    /// コンストラクターの行 → フィールドの宣言に移した、本体の頭の文の数。
    pub moved: HashMap<u32, usize>,
    /// コンストラクターの行 → フィールドの宣言の行に打つ系列点（移した初期化子の分）。
    pub initializer_points: HashMap<u32, Vec<super::Point>>,
}

/// フィールドの初期化子 1 つ: フィールドの行、値、コンストラクターごとの系列点の IL オフセット。
struct Initializer {
    field: u32,
    value: super::ast::Expr,
    offsets: Vec<(u32, Vec<u32>)>,
}

impl ModuleWriter<'_> {
    /// 型 1 つ（入れ子の型を含む）を書く。
    pub fn write_type(&mut self, row: u32, depth: usize, text: &mut Text) {
        let names = self.names;
        let kind = names.type_kind(row);
        if kind == TypeKind::Delegate {
            text.line(depth, &names.delegate_declaration(row));
            return;
        }
        text.line(depth, &names.type_header(row));
        text.line(depth, "{");
        let generics = Generics { types: names.type_generics(row), methods: Vec::new() };
        let properties = names.properties(row);
        let events = names.events(row);
        // アクセサーは、プロパティとイベントの宣言の中に書く。
        let accessors: HashSet<u32> = properties.iter().flat_map(|p| p.getter.into_iter().chain(p.setter)).chain(events.iter().flat_map(|e| e.add.into_iter().chain(e.remove))).collect();
        // 自動プロパティのバッキング フィールドと、フィールドのようなイベントのデリゲートは、宣言に含まれるので書かない。
        let hidden: HashSet<u32> = properties.iter().filter_map(|p| names.backing_field(row, p)).chain(events.iter().filter_map(|e| e.backing_field)).collect();
        let mut first = true;
        let separate = |text: &mut Text, first: &mut bool| {
            if !*first {
                text.blank();
            }
            *first = false;
        };
        // コンストラクターの頭の、フィールドの初期化子（`count = 3;`）は、フィールドの宣言に戻す（`int count = 3;`）。
        // 自動プロパティの裏のフィールドの初期化子は、プロパティの宣言に書く（`{ get; } = new List<object>();`）。宣言を
        // 書かないほかのフィールド（イベントの裏）の初期化子は、動かさない。
        let event_fields: HashSet<u32> = events.iter().filter_map(|e| e.backing_field).collect();
        let initializers = if kind == TypeKind::Class { self.field_initializers(row, &event_fields) } else { Vec::new() };
        first &= !self.write_fields(row, kind, &generics, &hidden, &initializers, depth + 1, text);
        for event in &events {
            separate(text, &mut first);
            self.write_event(event, kind, &generics, depth + 1, text);
        }
        for property in &properties {
            separate(text, &mut first);
            match names.backing_field(row, property) {
                Some(backing) => {
                    let init = initializers.iter().find(|i| i.field == backing);
                    let value = init.map(|init| self.initializer_text(init, &generics));
                    let line = self.write_auto_property(property, kind, &generics, value.as_deref(), depth + 1, text);
                    if let Some(init) = init {
                        self.initializer_line(init, line);
                    }
                }
                None => self.write_property(property, kind, &generics, depth + 1, text),
            }
        }
        let methods: Vec<u32> = names.md.child_rows(TYPE_DEF, row, 5, METHOD_DEF).filter(|m| !accessors.contains(m)).collect();
        for method in methods {
            // 前のメソッドの中にラムダとして書いたもの。
            if self.inlined.contains(&method) {
                continue;
            }            // 中身をすべてフィールドの初期化子に移した静的コンストラクター（系列点は、フィールドの行）。
            if self.moved.get(&method) == Some(&usize::MAX) {
                let points = self.initializer_points.remove(&method).unwrap_or_default();
                self.written.insert(method);
                self.methods.push(Method { row: method, document: 0, points, locals: Vec::new() });
                continue;
            }
            separate(text, &mut first);
            let generics = Generics { types: generics.types.clone(), methods: names.method_generics(method) };
            let header = names.method_header(method, kind, &generics);
            self.write_member(method, &header, &generics, depth + 1, text);
        }
        // 配列の初期化子のデータの器（`__StaticArrayInitTypeSize=16`）は、初期化子を戻したので書かない。
        // ラムダをすべて式の中に書いた `<>c` と表示用のクラス（ラムダの入れ物）も書かない。
        let inner: Vec<u32> = self
            .nested
            .get(&row)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&n| !names.md.string(names.md.cell(TYPE_DEF, n, 1)).starts_with("__StaticArrayInitTypeSize=") && !self.only_lambdas(n))
            .collect();
        for inner in inner {
            separate(text, &mut first);
            self.write_type(inner, depth + 1, text);
        }
        text.line(depth, "}");
    }

    /// 型のコンストラクターの頭にある、フィールドの初期化子。基底のコンストラクターを呼ぶ（`this(…)` を呼ばない）すべての
    /// インスタンスのコンストラクターで、基底の呼び出しより前に同じ並びの「自分のフィールド = 値」があるときだけ戻す
    /// （値は、引数やローカル変数を使わないもの）。静的コンストラクターは、頭から並ぶ「自分の静的フィールド = 値」。
    /// 戻したら、コンストラクターから消す文の数を `moved` に入れる（中身がすべてなら `usize::MAX`）。
    fn field_initializers(&mut self, type_row: u32, unmovable: &HashSet<u32>) -> Vec<Initializer> {
        let names = self.names;
        let md = names.md;
        let mut instance: Option<Vec<(u32, Expr)>> = None;
        let mut per_ctor: Vec<(u32, usize, Vec<Vec<u32>>)> = Vec::new();
        let mut consistent = true;
        let mut statics = Vec::new();
        for ctor in md.child_rows(TYPE_DEF, type_row, 5, METHOD_DEF) {
            let name = md.string(md.cell(METHOD_DEF, ctor, 3));
            if name != ".ctor" && name != ".cctor" {
                continue;
            }
            let Ok(body) = body::read(self.image, md.cell(METHOD_DEF, ctor, 0)) else {
                consistent &= name != ".ctor";
                continue;
            };
            let Ok(prepared) = csharp::prepare(names, self.image, ctor, &body) else {
                consistent &= name != ".ctor";
                continue;
            };
            if name == ".cctor" {
                statics.extend(self.static_initializers(type_row, unmovable, ctor, &prepared));
                continue;
            }
            // `this(…)` を呼ぶコンストラクターには、初期化子が無い。
            let calls_this = prepared.initializer.as_ref().is_some_and(|i| matches!(&i.kind, StmtKind::Expr(Expr::Call { method, .. }) if names.owner_type_row(*method) == Some(type_row)));
            if calls_this {
                continue;
            }
            // 移せない文があれば、その手前まで（残りは、これまでどおり本体に書く）。
            let head = &prepared.statements[..prepared.before_initializer.min(prepared.statements.len())];
            let found: Vec<_> = head.iter().map_while(|s| initializer_of(names, type_row, unmovable, s, false)).collect();
            let list: Vec<(u32, Expr)> = found.iter().map(|(f, v, _)| (*f, v.clone())).collect();
            match &instance {
                None => instance = Some(list),
                Some(existing) if *existing == list => {}
                Some(_) => consistent = false,
            }
            per_ctor.push((ctor, found.len(), found.into_iter().map(|(_, _, o)| o).collect()));
        }
        let mut out = statics;
        if let (true, Some(list)) = (consistent, instance) {
            if !list.is_empty() {
                for (index, (field, value)) in list.into_iter().enumerate() {
                    let offsets = per_ctor.iter().map(|(ctor, _, offsets)| (*ctor, offsets.get(index).cloned().unwrap_or_default())).collect();
                    out.push(Initializer { field, value, offsets });
                }
                for (ctor, count, _) in per_ctor {
                    self.moved.insert(ctor, count);
                }
            }
        }
        out
    }

    /// 静的コンストラクターの頭から並ぶ「自分の静的フィールド = 値」。中身がすべてなら、コンストラクターは書かない
    /// （`moved` に `usize::MAX`）。そのとき外した末尾の `return;` の系列点は、最後のフィールドの行に打つ。
    fn static_initializers(&mut self, type_row: u32, unmovable: &HashSet<u32>, ctor: u32, prepared: &csharp::Prepared) -> Vec<Initializer> {
        let found: Vec<_> = prepared.statements.iter().map_while(|s| initializer_of(self.names, type_row, unmovable, s, true)).collect();
        if found.is_empty() {
            return Vec::new();
        }
        let all = found.len() == prepared.statements.len();
        self.moved.insert(ctor, if all { usize::MAX } else { found.len() });
        let last = found.len() - 1;
        found
            .into_iter()
            .enumerate()
            .map(|(index, (field, value, mut offsets))| {
                if all && index == last {
                    offsets.extend(&prepared.applied.end);
                }
                Initializer { field, value, offsets: vec![(ctor, offsets)] }
            })
            .collect()
    }

    /// 型が `<>c` か表示用のクラス（`<>c__DisplayClass0_0`）で、コンストラクター以外のメソッドが、すべて式の中に書いた
    /// ラムダか。
    fn only_lambdas(&self, row: u32) -> bool {
        let md = self.names.md;
        let name = md.string(md.cell(TYPE_DEF, row, 1));
        if name != "<>c" && !name.starts_with("<>c__DisplayClass") {
            return false;
        }
        let methods: Vec<u32> = md.child_rows(TYPE_DEF, row, 5, METHOD_DEF).filter(|&m| !md.string(md.cell(METHOD_DEF, m, 3)).ends_with("ctor")).collect();
        !methods.is_empty() && methods.iter().all(|m| self.inlined.contains(m))
    }

    /// フィールドの宣言を書く（初期化子があれば ` = 値;`）。データの器と、宣言に含まれる裏のフィールド（`hidden`）は書かない。
    /// 1 つでも書いたか。
    #[allow(clippy::too_many_arguments)]
    fn write_fields(&mut self, row: u32, kind: TypeKind, generics: &Generics, hidden: &HashSet<u32>, initializers: &[Initializer], depth: usize, text: &mut Text) -> bool {
        let names = self.names;
        let mut wrote = false;
        for field in names.md.child_rows(TYPE_DEF, row, 4, FIELD).filter(|&f| !names.has_data(f) && !hidden.contains(&f)) {
            let Some(declaration) = names.field_declaration(field, kind, generics) else { continue };
            wrote = true;
            match initializers.iter().find(|i| i.field == field) {
                Some(init) => {
                    let value = self.initializer_text(init, generics);
                    let line = text.line(depth, &format!("{} = {value};", declaration.trim_end_matches(';')));
                    self.initializer_line(init, line);
                }
                None => {
                    text.line(depth, &declaration);
                }
            }
        }
        wrote
    }

    /// 初期化子の値を、C# で書く（最初のコンストラクターの文脈で）。
    fn initializer_text(&self, init: &Initializer, generics: &Generics) -> String {
        let ctor = init.offsets.first().map_or(0, |(ctor, _)| *ctor);
        let scope = csharp::Scope { names: self.names, image: self.image, written: &self.written, moved: &self.moved };
        csharp::expression_text(&scope, ctor, generics, &init.value)
    }

    /// 移した初期化子の系列点は、宣言の行に打つ（Roslyn の PDB と同じ）。
    fn initializer_line(&mut self, init: &Initializer, line: (u32, u32, u32)) {
        for (ctor, offsets) in &init.offsets {
            let points = self.initializer_points.entry(*ctor).or_default();
            points.extend(offsets.iter().map(|&il_offset| super::Point { il_offset, line: line.0, start_column: line.1, end_column: line.2 }));
        }
    }

    /// 自動プロパティ: `public int X { get; private set; }`（初期化子があれば ` = 値;`）。アクセサーの系列点は、その行の
    /// `get;` / `set;` に打つ。書いた行を返す。
    fn write_auto_property(&mut self, property: &Property, kind: TypeKind, generics: &Generics, initializer: Option<&str>, depth: usize, text: &mut Text) -> (u32, u32, u32) {
        let mut parts = Vec::new();
        for (keyword, accessor) in [("get;", property.getter), ("set;", property.setter)] {
            let Some(accessor) = accessor else { continue };
            parts.push((self.names.accessor_visibility(property, accessor).map_or_else(|| keyword.to_string(), |v| format!("{v} {keyword}")), keyword, accessor));
        }
        let head = format!("{} {{ {} }}", self.names.property_header(property, kind, generics), parts.iter().map(|(p, _, _)| p.as_str()).collect::<Vec<_>>().join(" "));
        let written = match initializer {
            Some(value) => text.line(depth, &format!("{head} = {value};")),
            None => text.line(depth, &head),
        };
        let (line, start, _) = written;
        for (part, keyword, accessor) in parts {
            let at = head.find(&format!(" {part}")).map_or(0, |i| i + 1 + part.len() - keyword.len());
            let column = start + head[..at].chars().count() as u32;
            self.methods.push(Method { row: accessor, document: 0, points: vec![super::Point { il_offset: 0, line, start_column: column, end_column: column + keyword.len() as u32 }], locals: Vec::new() });
        }
        written
    }

    /// イベント 1 つ。フィールドのようなイベントは 1 行（`public event EventHandler Changed;`。アクセサーの系列点もその行）、
    /// そうでなければ `add` / `remove` の本体を書く。
    fn write_event(&mut self, event: &Event, kind: TypeKind, generics: &Generics, depth: usize, text: &mut Text) {
        let header = self.names.event_header(event, kind, generics);
        if event.backing_field.is_some() {
            let (line, start_column, end_column) = text.line(depth, &format!("{header};"));
            for accessor in event.add.into_iter().chain(event.remove) {
                self.methods.push(Method { row: accessor, document: 0, points: vec![super::Point { il_offset: 0, line, start_column, end_column }], locals: Vec::new() });
            }
            return;
        }
        text.line(depth, &header);
        text.line(depth, "{");
        for (keyword, accessor) in [("add", event.add), ("remove", event.remove)] {
            if let Some(accessor) = accessor {
                self.write_member(accessor, keyword, generics, depth + 1, text);
            }
        }
        text.line(depth, "}");
    }

    /// プロパティ 1 つ: 頭と、`get` / `set` の本体。
    fn write_property(&mut self, property: &Property, kind: TypeKind, generics: &Generics, depth: usize, text: &mut Text) {
        text.line(depth, &self.names.property_header(property, kind, generics));
        text.line(depth, "{");
        for (keyword, accessor) in [("get", property.getter), ("set", property.setter)] {
            let Some(accessor) = accessor else { continue };
            let header = match self.names.accessor_visibility(property, accessor) {
                Some(visibility) => format!("{visibility} {keyword}"),
                None => keyword.to_string(),
            };
            self.write_member(accessor, &header, generics, depth + 1, text);
        }
        text.line(depth, "}");
    }

    /// メソッドかアクセサー 1 つ: 頭と本体。本体は C# で書き、戻せなければ IL で書く。本体が無ければ `;`。
    fn write_member(&mut self, row: u32, header: &str, generics: &Generics, depth: usize, text: &mut Text) {
        let mut method = Method { row, document: 0, points: Vec::new(), locals: Vec::new() };
        let rva = self.names.md.cell(METHOD_DEF, row, 0);
        if rva == 0 {
            text.line(depth, &format!("{header};"));
            self.methods.push(method);
            return;
        }
        text.line(depth, header);
        let mut end = Vec::new();
        match body::read(self.image, rva) {
            Ok(body) => {
                // C# の本体は、開き括弧を自分で書く（コンストラクターの初期化子が、頭と括弧の間に入る）。
                let scope = csharp::Scope { names: self.names, image: self.image, written: &self.written, moved: &self.moved };
                let map = match csharp::write_method_body(&scope, row, &body, generics, depth + 1, text) {
                    Ok(map) => map,
                    Err(reason) => {
                        text.line(depth, "{");
                        text.line(depth + 1, &tr!("// It cannot be turned back into C#, so it is shown as IL ({reason}).", reason = reason));
                        ilview::write_body(self.names, row, &body, generics, depth + 1, text)
                    }
                };
                method.points = map.points;
                method.locals = map.locals;
                end = map.end;
                for InlinedLambda { row: lambda, points, locals } in map.lambdas {
                    self.inlined.insert(lambda);
                    self.written.insert(lambda);
                    self.methods.push(Method { row: lambda, document: 0, points, locals });
                }
            }
            Err(e) => {
                text.line(depth, "{");
                text.line(depth + 1, &tr!("// Cannot read the body: {error}", error = e));
            }
        }
        let (line, start_column, end_column) = text.line(depth, "}");
        // 外した末尾の `return;` の系列点は、閉じ括弧の行に打つ（Visual Studio と同じ所で止まれる）。
        for il_offset in end {
            method.points.push(super::Point { il_offset, line, start_column, end_column });
        }
        method.points.extend(self.initializer_points.remove(&row).unwrap_or_default());
        method.points.sort_by_key(|p| p.il_offset);
        self.written.insert(row);
        self.methods.push(method);
    }
}

/// 文が、フィールドの初期化子に移せる代入（自分の型の、静的なら静的・インスタンスなら `this` の、`unmovable` でない
/// フィールドへの、引数やローカル変数を使わない値の代入）なら、フィールドの行、値、系列点の IL オフセット。
fn initializer_of(names: &Names, type_row: u32, unmovable: &HashSet<u32>, stmt: &super::ast::Stmt, statik: bool) -> Option<(u32, Expr, Vec<u32>)> {
    let StmtKind::Assign(Expr::Field { target, token }, value) = &stmt.kind else { return None };
    let row = *token & 0x00FF_FFFF;
    let own = (*token >> 24) as usize == FIELD && names.md.field_declaration(row).is_some_and(|(owner, _)| owner == type_row);
    let receiver = match target.as_deref() {
        None => statik,
        Some(t) => !statik && *t == Expr::Arg(0),
    };
    (own && receiver && !unmovable.contains(&row) && csharp::is_initializer_value(value)).then(|| (row, value.clone(), stmt.il.iter().chain(&stmt.merged).copied().collect()))
}
