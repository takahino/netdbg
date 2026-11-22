//! 文と式の木を、C# として書く。演算子の優先順位に合わせて括弧を付け、系列点を打つ行（文の行、`if` / `while` の行、
//! `do` の終わりの `while` の行）を記録する。

use super::ast::{CmpOp, Expr, Part, Stmt, StmtKind};
use super::body::{self, MethodBody};
use super::build::{self, temp_name};
use super::flow;
use super::ilview;
use super::structure;
use super::sugar;
use super::writer::{BodyMap, InlinedLambda};
use crate::debugger::il::{self, Instruction};
use crate::debugger::metadata::{METHOD_DEF, TYPE_DEF};
use super::decl::{char_literal, float_literal};
use super::names::{ident, string_literal, Generics, Names};
use super::sig::Ty;
use super::{Point, Text, MARK};
use std::collections::{BTreeMap, HashSet};

mod lambdas;
use lambdas::PendingLambda;

/// 演算子の優先順位（大きいほど強い）。
mod precedence {
    pub const ASSIGN: u8 = 1;
    pub const CONDITIONAL: u8 = 2;
    pub const COALESCE: u8 = 3;
    pub const BIT_OR: u8 = 6;
    pub const BIT_XOR: u8 = 7;
    pub const BIT_AND: u8 = 8;
    pub const EQUALITY: u8 = 9;
    pub const RELATIONAL: u8 = 10;
    pub const SHIFT: u8 = 11;
    pub const ADDITIVE: u8 = 12;
    pub const MULTIPLICATIVE: u8 = 13;
    pub const UNARY: u8 = 14;
    pub const PRIMARY: u8 = 15;
}
use precedence as p;

/// モジュールの中で、メソッドの本体を書くのに使うもの。
pub struct Scope<'a> {
    pub names: &'a Names<'a>,
    /// PE の像（ラムダのメソッドの本体を読む）。
    pub image: &'a [u8],
    /// もう書いたメソッドの行（ラムダとして式の中に入れられない）。
    pub written: &'a HashSet<u32>,
    /// コンストラクターの行 → フィールドの宣言に移した、本体の頭の文の数。
    pub moved: &'a std::collections::HashMap<u32, usize>,
}

/// メソッド 1 つを書く文脈。
pub struct Writer<'a> {
    pub names: &'a Names<'a>,
    image: &'a [u8],
    written: &'a HashSet<u32>,
    moved: &'a std::collections::HashMap<u32, usize>,
    /// 書いている文の中に入れたラムダ。文の行が決まったら `lambdas` に移す。
    pending_lambdas: std::cell::RefCell<Vec<PendingLambda>>,
    /// 式の中に入れたラムダのメソッド（メソッドの行ごと）。
    pub lambdas: BTreeMap<u32, InlinedLambda>,
    /// まだ行を置き直していないラムダ（メソッドの行 → 目印のメソッドの行）。
    anchors: std::collections::HashMap<u32, u32>,
    /// ラムダを式の中に入れるか。デリゲートの使い回しの形が残ったメソッドでは入れない（`<>c` の宣言が要るので）。
    inline_lambdas: bool,
    pub generics: &'a Generics,
    /// 引数の名前（`ldarg` の番号の順。インスタンスメソッドは 0 が `this`）。
    pub arguments: &'a [String],
    pub locals: &'a [String],
    /// ローカル変数の型（宣言つきの代入、`foreach` の変数を書くとき）。
    pub local_types: &'a [Ty],
    /// メソッドを宣言している型の名前（同じ型の静的メンバーは、型の名前を省く）。
    pub current_type: String,
    /// 系列点（IL オフセット → 点）。同じ IL オフセットの文が続くときは、最初の文の行。
    pub points: BTreeMap<u32, Point>,
    /// いま書いているメソッドの out の引数（`ldarg` の番号）。そのまま別の呼び出しに渡すときは `out` と書く。
    pub out_arguments: Vec<u32>,
    /// いま書いているメソッドを宣言している型（TypeDef の行）。初期化子の `this` と `base` を分ける。
    pub type_row: Option<u32>,
    /// いま書いているメソッドの引数の型（`ldarg` の番号の順。`this` は `None`）。
    pub argument_types: Vec<Option<Ty>>,
    /// いま書いている catch の入れ子の深さ（例外の変数の名前を分ける）。
    catch_depth: usize,
    /// 表示用のクラスを戻したラムダのメソッドの行（`closures.rs`。必ず式の中に入れる）。
    pub closures: HashSet<u32>,
    /// 表示用のクラスのラムダの本体を書いている（`this` は表示用のクラスで、`this.<>4__this` が外の `this`）。
    closure_body: bool,
}

impl<'a> Writer<'a> {
    pub fn new(scope: &Scope<'a>, generics: &'a Generics, arguments: &'a [String], locals: &'a [String], local_types: &'a [Ty], current_type: String) -> Writer<'a> {
        Writer {
            names: scope.names,
            image: scope.image,
            written: scope.written,
            moved: scope.moved,
            pending_lambdas: Default::default(),
            lambdas: BTreeMap::new(),
            anchors: Default::default(),
            inline_lambdas: true,
            generics,
            arguments,
            locals,
            local_types,
            current_type,
            points: BTreeMap::new(),
            out_arguments: Vec::new(),
            type_row: None,
            argument_types: Vec::new(),
            catch_depth: 0,
            closures: HashSet::new(),
            closure_body: false,
        }
    }

    fn scope(&self) -> Scope<'a> {
        Scope { names: self.names, image: self.image, written: self.written, moved: self.moved }
    }

    /// 名前が、ローカル変数か引数と重なるか（`this.` や型の名前を省けない）。
    fn shadowed(&self, name: &str) -> bool {
        self.locals.iter().chain(self.arguments).any(|n| n == name)
    }

    /// メンバーの持ち主の書き方: `this` と同じ型の静的メンバーは省く（名前が重ならなければ）。省くなら空。
    fn owner_prefix(&self, target: Option<&Expr>, declaring: &str, name: &str) -> String {
        match target {
            Some(Expr::Arg(0)) if self.arguments.first().map(String::as_str) == Some("this") && !self.shadowed(name) => String::new(),
            Some(target) => format!("{}.", self.target(target)),
            None if declaring == self.current_type && !self.shadowed(name) => String::new(),
            None if declaring.is_empty() => String::new(),
            None => format!("{declaring}."),
        }
    }

    /// コンストラクターの初期化子の行（`: base(x, y)`、同じ型なら `: this(…)`）。引数の無い基底の呼び出しは書かない。
    fn initializer_text(&self, stmt: &Stmt) -> Option<String> {
        let StmtKind::Expr(Expr::Call { method, args, .. }) = &stmt.kind else { return None };
        let m = self.names.method_ref(*method, self.generics)?;
        let own = self.names.owner_type_row(*method).is_some_and(|t| Some(t) == self.type_row);
        if !own && args.is_empty() {
            return None;
        }
        // 同じ型のほかのコンストラクター（多重定義）を呼ぶ所は、引数を相手の型にキャストして、呼び先を決める
        // （`JArray(params object[] content) : this((object)content)`）。
        let args: Vec<Expr> = args
            .iter()
            .zip(&m.sig.params)
            .map(|(a, param)| match a {
                Expr::Arg(i) if own && self.argument_types.get(*i as usize).is_some_and(|t| t.as_ref() != Some(param)) => Expr::Convert { to: param.clone(), checked: false, value: a.clone().boxed() },
                _ => a.clone(),
            })
            .collect();
        let keyword = if own { "this" } else { "base" };
        Some(format!(": {keyword}({})", self.arguments_text(&args, &m.sig.params, *method)))
    }

    /// 呼び先に、同じ名前で引数の数も同じメソッドがほかにあるか（このモジュールの中で分かるものだけ）。`null` の引数は、
    /// 型をキャストで書かないと、呼び先が決まらない。
    fn overloaded(&self, method: u32, count: usize) -> bool {
        let Some(row) = self.names.method_definition(method) else { return false };
        let md = self.names.md;
        let Some(owner) = md.declaring_type(row) else { return false };
        let name = md.string(md.cell(METHOD_DEF, row, 3));
        md.method_rows_named(owner, name).into_iter().filter(|&r| self.names.method_def_sig(r).is_some_and(|s| s.params.len() == count)).count() > 1
    }

    fn local_declaration(&self, local: u16) -> String {
        let name = self.locals.get(local as usize).cloned().unwrap_or_else(|| format!("V_{local}"));
        let ty = self.local_types.get(local as usize).map_or_else(|| "var".to_string(), |t| declared_type(self.names, t, self.generics));
        format!("{ty} {name}")
    }

    fn point(&mut self, stmt: &Stmt, line: (u32, u32, u32)) {
        for &il in stmt.il.iter().chain(&stmt.merged) {
            self.points.entry(il).or_insert(Point { il_offset: il, line: line.0, start_column: line.1, end_column: line.2 });
        }
        // この行に入れたラムダの系列点も、この行に打つ（同じ式を二度書いたときは、最初の分）。
        for PendingLambda { row, offsets, body, locals } in self.pending_lambdas.take() {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.lambdas.entry(row) {
                let points = match body {
                    Some((anchor, points)) => {
                        self.anchors.insert(row, anchor);
                        points
                    }
                    None => offsets.into_iter().map(|il| Point { il_offset: il, line: line.0, start_column: line.1, end_column: line.2 }).collect(),
                };
                entry.insert(InlinedLambda { row, points, locals });
            }
        }
    }

    pub fn block(&mut self, body: &[Stmt], depth: usize, text: &mut Text) {
        for (i, stmt) in body.iter().enumerate() {
            let last = i + 1 == body.len();
            self.stmt(stmt, depth, last, text);
        }
    }

    fn braced(&mut self, body: &[Stmt], depth: usize, text: &mut Text) {
        text.line(depth, "{");
        self.block(body, depth + 1, text);
        text.line(depth, "}");
    }

    fn stmt(&mut self, stmt: &Stmt, depth: usize, last: bool, text: &mut Text) {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                let line = text.line(depth, &format!("{};", self.expr(e, 0)));
                self.point(stmt, line);
            }
            StmtKind::Assign(target, value) => {
                let line = text.line(depth, &format!("{} = {};", self.expr(target, p::ASSIGN + 1), self.expr(value, p::ASSIGN)));
                self.point(stmt, line);
            }
            StmtKind::Return(value) => {
                let line = text.line(depth, &match value {
                    Some(value) => format!("return {};", self.expr(value, 0)),
                    None => "return;".into(),
                });
                self.point(stmt, line);
            }
            StmtKind::Throw(value) => {
                let line = text.line(depth, &match value {
                    Some(value) => format!("throw {};", self.expr(value, 0)),
                    None => "throw;".into(),
                });
                self.point(stmt, line);
            }
            StmtKind::If { condition, then, otherwise } => {
                let line = text.line(depth, &format!("if ({})", self.expr(condition, 0)));
                self.point(stmt, line);
                self.braced(then, depth, text);
                self.else_chain(otherwise, depth, text);
            }
            StmtKind::While { condition, body } => {
                let line = text.line(depth, &format!("while ({})", self.expr(condition, 0)));
                self.point(stmt, line);
                self.braced(body, depth, text);
            }
            StmtKind::DoWhile { body, condition } => {
                text.line(depth, "do");
                self.braced(body, depth, text);
                let line = text.line(depth, &format!("while ({});", self.expr(condition, 0)));
                self.point(stmt, line);
            }
            StmtKind::Break => {
                let line = text.line(depth, "break;");
                self.point(stmt, line);
            }
            StmtKind::Continue => {
                let line = text.line(depth, "continue;");
                self.point(stmt, line);
            }
            StmtKind::Goto(label) => {
                let line = text.line(depth, &format!("goto IL_{label:04x};"));
                self.point(stmt, line);
            }
            // ラベルは文の前にしか書けない。ブロックの最後なら空の文を付ける。
            StmtKind::Label(label) => {
                text.line(depth, &if last { format!("IL_{label:04x}: ;") } else { format!("IL_{label:04x}:") });
            }
            StmtKind::Switch { value, targets, ty } => {
                let line = text.line(depth, &format!("switch ({})", self.expr(value, 0)));
                self.point(stmt, line);
                text.line(depth, "{");
                let cast = self.enum_cast(ty);
                for (case, label) in targets.iter().enumerate() {
                    text.line(depth + 1, &format!("case {cast}{case}: goto IL_{label:04x};"));
                }
                text.line(depth, "}");
            }
            StmtKind::Compound { target, op, value } => {
                let line = text.line(depth, &format!("{} {}= {};", self.expr(target, p::ASSIGN + 1), op.symbol(), self.expr(value, p::ASSIGN)));
                self.point(stmt, line);
            }
            StmtKind::Increment { target, decrement } => {
                let line = text.line(depth, &format!("{}{};", self.expr(target, p::UNARY), if *decrement { "--" } else { "++" }));
                self.point(stmt, line);
            }
            StmtKind::Declare { local, value } => {
                let line = text.line(depth, &format!("{} = {};", self.local_declaration(*local), self.expr(value, p::ASSIGN)));
                self.point(stmt, line);
            }
            StmtKind::Foreach { local, collection, body } => {
                let line = text.line(depth, &format!("foreach ({} in {})", self.local_declaration(*local), self.expr(collection, 0)));
                self.point(stmt, line);
                self.braced(body, depth, text);
            }
            StmtKind::Using { local, resource, body } => {
                let head = match local {
                    Some(local) => format!("using ({} = {})", self.local_declaration(*local), self.expr(resource, p::ASSIGN)),
                    None => format!("using ({})", self.expr(resource, 0)),
                };
                let line = text.line(depth, &head);
                self.point(stmt, line);
                self.braced(body, depth, text);
            }
            StmtKind::SwitchBlock { value, sections, ty } => {
                let cast = self.enum_cast(ty);
                let line = text.line(depth, &format!("switch ({})", self.expr(value, 0)));
                self.point(stmt, line);
                text.line(depth, "{");
                for section in sections {
                    for case in &section.cases {
                        text.line(depth + 1, &format!("case {cast}{case}:"));
                    }
                    if section.default {
                        text.line(depth + 1, "default:");
                    }
                    self.block(&section.body, depth + 2, text);
                }
                text.line(depth, "}");
            }
            StmtKind::For { init, condition, step, body } => {
                let init = init.as_deref().and_then(|s| self.inline_text(s)).unwrap_or_default();
                let step = step.as_deref().and_then(|s| self.inline_text(s)).unwrap_or_default();
                let line = text.line(depth, &format!("for ({init}; {}; {step})", self.expr(condition, 0)));
                self.point(stmt, line);
                self.braced(body, depth, text);
            }
            StmtKind::Lock { target, body } => {
                let line = text.line(depth, &format!("lock ({})", self.expr(target, 0)));
                self.point(stmt, line);
                self.braced(body, depth, text);
            }
            StmtKind::Try { body, catches, finally, fault } => {
                text.line(depth, "try");
                self.braced(body, depth, text);
                for catch in catches {
                    // `catch (object)`（例外でない値も受ける）は、C# では `Exception` で受ける（C# のアセンブリでは、
                    // 例外でない値は RuntimeWrappedException に包まれて届く）。
                    let ty = match self.names.type_token(catch.ty, &[], self.generics) {
                        ty if ty == "object" => "Exception".to_string(),
                        ty => ty,
                    };
                    self.catch_depth += 1;
                    let header = match &catch.variable {
                        Some(_) => format!("catch ({ty} {})", self.exception_name()),
                        None => format!("catch ({ty})"),
                    };
                    text.line(depth, &header);
                    self.braced(&catch.body, depth, text);
                    self.catch_depth -= 1;
                }
                if let Some(fault) = fault {
                    // C# に fault は無い。例外のときだけ動いて投げ直す catch と同じ。
                    text.line(depth, "catch");
                    text.line(depth, "{");
                    self.block(fault, depth + 1, text);
                    text.line(depth + 1, "throw;");
                    text.line(depth, "}");
                }
                if let Some(finally) = finally {
                    text.line(depth, "finally");
                    self.braced(finally, depth, text);
                }
            }
        }
    }

    /// 列挙型の `switch` の case に付けるキャスト（`(Color)`）。列挙型でなければ空。
    fn enum_cast(&self, ty: &Option<Ty>) -> String {
        match ty {
            Some(t @ (Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. })) => format!("({})", self.names.ty(t, self.generics)),
            _ => String::new(),
        }
    }

    /// `for` の初期化と増分に書く、`;` の無い文（`int i = 0`、`i++`）。
    fn inline_text(&self, stmt: &Stmt) -> Option<String> {
        Some(match &stmt.kind {
            StmtKind::Declare { local, value } => format!("{} = {}", self.local_declaration(*local), self.expr(value, p::ASSIGN)),
            StmtKind::Assign(target, value) => format!("{} = {}", self.expr(target, p::ASSIGN + 1), self.expr(value, p::ASSIGN)),
            StmtKind::Increment { target, decrement } => format!("{}{}", self.expr(target, p::UNARY), if *decrement { "--" } else { "++" }),
            StmtKind::Compound { target, op, value } => format!("{} {}= {}", self.expr(target, p::ASSIGN + 1), op.symbol(), self.expr(value, p::ASSIGN)),
            StmtKind::Expr(e) => self.expr(e, 0),
            _ => return None,
        })
    }

    /// `else` の部分。中身が if 一つだけなら `else if` にする。
    fn else_chain(&mut self, otherwise: &[Stmt], depth: usize, text: &mut Text) {
        match otherwise {
            [] => {}
            [only @ Stmt { kind: StmtKind::If { condition, then, otherwise }, .. }] => {
                let line = text.line(depth, &format!("else if ({})", self.expr(condition, 0)));
                self.point(only, line);
                self.braced(then, depth, text);
                self.else_chain(otherwise, depth, text);
            }
            _ => {
                text.line(depth, "else");
                self.braced(otherwise, depth, text);
            }
        }
    }

    fn exception_name(&self) -> String {
        if self.catch_depth <= 1 { "ex".into() } else { format!("ex{}", self.catch_depth) }
    }

    /// 式を、優先順位 `min` 以上の形で書く（弱ければ括弧を付ける）。
    fn expr(&self, expr: &Expr, min: u8) -> String {
        let (text, level) = self.expr_with_level(expr);
        if level < min { format!("({text})") } else { text }
    }

    fn expr_with_level(&self, expr: &Expr) -> (String, u8) {
        let g = self.generics;
        let n = self.names;
        let primary = |s: String| (s, p::PRIMARY);
        match expr {
            Expr::Local(index) => primary(self.locals.get(*index as usize).cloned().unwrap_or_else(|| format!("V_{index}"))),
            Expr::Arg(index) => primary(self.arguments.get(*index as usize).cloned().unwrap_or_else(|| format!("A_{index}"))),
            Expr::Temp(temp) => primary(temp_name(*temp)),
            Expr::Exception => primary(self.exception_name()),
            Expr::Int(value) => (value.to_string(), if *value < 0 { p::UNARY } else { p::PRIMARY }),
            Expr::Long(value) => (format!("{value}L"), if *value < 0 { p::UNARY } else { p::PRIMARY }),
            Expr::Float(value) => (float_literal(*value, true), p::UNARY),
            Expr::Double(value) => (float_literal(*value, false), p::UNARY),
            Expr::Str(text) => primary(string_literal(text)),
            Expr::Null => primary("null".into()),
            Expr::Bool(value) => primary(value.to_string()),
            Expr::Char(unit) => primary(char_literal(*unit)),
            Expr::Default(ty) => primary(format!("default({})", n.ty(ty, g))),
            Expr::Literal(text) => primary(text.clone()),
            Expr::Binary(op, a, b) => {
                let level = binary_level(op.symbol());
                (format!("{} {} {}", self.expr(a, level), op.symbol(), self.expr(b, level + 1)), level)
            }
            Expr::Checked(op, a, b) => primary(format!("checked({} {} {})", self.expr(a, binary_level(op.symbol())), op.symbol(), self.expr(b, binary_level(op.symbol()) + 1))),
            Expr::Neg(value) => (format!("-{}", self.expr(value, p::UNARY + 1)), p::UNARY),
            Expr::BitNot(value) => (format!("~{}", self.expr(value, p::UNARY)), p::UNARY),
            Expr::Not(value) => (format!("!{}", self.expr(value, p::UNARY)), p::UNARY),
            // `x as T` と null の比較は、型の判定（値型でも書ける）。
            Expr::Compare(op @ (CmpOp::Ne | CmpOp::Eq), a, b) if matches!((&**a, &**b), (Expr::As { .. }, Expr::Null)) => {
                let Expr::As { ty, value } = &**a else { unreachable!() };
                let test = format!("{} is {}", self.expr(value, p::RELATIONAL), n.type_token(*ty, &[], g));
                if *op == CmpOp::Ne { (test, p::RELATIONAL) } else { (format!("!({test})"), p::UNARY) }
            }
            Expr::Compare(op, a, b) => {
                let level = if matches!(op.symbol(), "==" | "!=") { p::EQUALITY } else { p::RELATIONAL };
                (format!("{} {} {}", self.expr(a, level), op.symbol(), self.expr(b, level + 1)), level)
            }
            // 負の定数を符号なしの型にする所は、その値のリテラル（`(ulong)-1` は C# ではエラー）。
            Expr::Convert { to: to @ (Ty::U1 | Ty::U2 | Ty::U4 | Ty::U8), checked: false, value } if matches!(**value, Expr::Int(n) if n < 0) || matches!(**value, Expr::Long(n) if n < 0) => {
                let n = match **value {
                    Expr::Int(n) => n,
                    Expr::Long(n) => n,
                    _ => 0,
                };
                let literal = match to {
                    Ty::U1 => format!("(byte){}", n as u8),
                    Ty::U2 => format!("(ushort){}", n as u16),
                    Ty::U4 => format!("{}u", n as u32),
                    _ => format!("{}UL", n as u64),
                };
                (literal, p::UNARY)
            }
            Expr::Convert { to, checked, value } => {
                let cast = format!("({}){}", n.ty(to, g), self.expr(value, p::UNARY));
                if *checked { primary(format!("checked({cast})")) } else { (cast, p::UNARY) }
            }
            // 表示用のクラスのラムダの中: `this.<>4__this` は外の `this`。
            Expr::Field { target: Some(target), token } if self.closure_body && **target == Expr::Arg(0) && n.md.field_declaration(*token & 0x00FF_FFFF).is_some_and(|(_, f)| f.name == "<>4__this") => primary("this".into()),
            Expr::Field { target, token } => {
                let (owner, name) = n.field_parts(*token, g);
                // decimal の定数は、リテラルで書く（`decimal.Zero` → `0m`）。
                if target.is_none() && owner == "decimal" {
                    match name.as_str() {
                        "Zero" => return primary("0m".into()),
                        "One" => return primary("1m".into()),
                        "MinusOne" => return ("-1m".into(), p::UNARY),
                        _ => {}
                    }
                }
                primary(format!("{}{name}", self.owner_prefix(target.as_deref(), &owner, &name)))
            }
            Expr::Property { target, method, args } => {
                let Some(m) = n.method_ref(*method, g) else { return primary(format!("[0x{method:08X}]")) };
                // アクセサーの名前の頭（get_、set_、イベントの add_、remove_）を外した名前。
                let name = ["get_", "set_", "add_", "remove_"].iter().find_map(|prefix| m.name.strip_prefix(prefix)).unwrap_or(&m.name);
                if !args.is_empty() {
                    let owner = match target {
                        Some(target) => self.target(target),
                        None => m.declaring.clone(),
                    };
                    let args: Vec<String> = args.iter().map(|a| self.expr(a, 0)).collect();
                    primary(format!("{owner}[{}]", args.join(", ")))
                } else {
                    let name = ident(name);
                    primary(format!("{}{name}", self.owner_prefix(target.as_deref(), &m.declaring, &name)))
                }
            }
            Expr::AddressOf(inner) => (format!("ref {}", self.expr(inner, p::UNARY)), p::UNARY),
            // ref の引数やローカル変数の指す先は、名前そのものと同じ書き方。
            // アドレスの指す先（一時変数に入れたアドレスを、その場所に戻した形）は、変数そのもの。
            Expr::Deref(inner) => match &**inner {
                Expr::AddressOf(target) => self.expr_with_level(target),
                other => self.expr_with_level(other),
            },
            Expr::Call { method, target, args, .. } => {
                let Some(m) = n.method_ref(*method, g) else { return primary(format!("[0x{method:08X}]")) };
                let mut name = ident(&m.name);
                if !m.method_arguments.is_empty() {
                    name = format!("{name}<{}>", m.method_arguments.join(", "));
                }
                // 明示的なインターフェイスの実装を直接呼ぶ（IL ではできる）: C# では、インターフェイスへのキャストを通して呼ぶ。
                if let Some(row) = n.method_definition(*method) {
                    if let Some(interface) = n.explicit_interface(row) {
                        let simple = ident(m.name.rsplit('.').next().unwrap_or(&m.name));
                        let receiver = target.as_deref().map_or_else(|| "this".to_string(), |t| self.target(t));
                        return primary(format!("(({interface}){receiver}).{simple}({})", self.arguments_text(args, &m.sig.params, *method)));
                    }
                }
                // デリゲートの呼び出し `f.Invoke(x)` は `f(x)`（`f?.Invoke(x)` はそのまま）。
                if m.name == "Invoke" && n.is_delegate(*method) {
                    if let Some(target) = target.as_deref().filter(|t| !matches!(t, Expr::NullConditional(_))) {
                        return primary(format!("{}({})", self.target(target), self.arguments_text(args, &m.sig.params, *method)));
                    }
                }
                let owner = self.owner_prefix(target.as_deref(), &m.declaring, &ident(&m.name));
                primary(format!("{owner}{name}({})", self.arguments_text(args, &m.sig.params, *method)))
            }
            Expr::New { method, args } => {
                let Some(m) = n.method_ref(*method, g) else { return primary(format!("[0x{method:08X}]")) };
                // `new decimal(5)` は `5m`。
                if m.declaring == "decimal" {
                    if let [Expr::Int(value) | Expr::Long(value)] = args.as_slice() {
                        return (format!("{value}m"), if *value < 0 { p::UNARY } else { p::PRIMARY });
                    }
                }
                primary(format!("new {}({})", m.declaring, self.arguments_text(args, &m.sig.params, *method)))
            }
            Expr::NewDelegate { ty, target, method } => {
                let owner = n.method_ref(*ty, g).map(|m| m.declaring).unwrap_or_else(|| "Delegate".into());
                if let Some(lambda) = self.lambda_text(*method, target.as_deref()) {
                    return primary(format!("new {owner}({lambda})"));
                }
                let Some(m) = n.method_ref(*method, g) else { return primary(format!("[0x{method:08X}]")) };
                let mut name = ident(&m.name);
                // ジェネリックなメソッドは、型引数を書く（メソッドのグループからは推論できない）。
                if !m.method_arguments.is_empty() {
                    name = format!("{name}<{}>", m.method_arguments.join(", "));
                }
                let source = format!("{}{name}", self.owner_prefix(target.as_deref(), &m.declaring, &ident(&m.name)));
                primary(format!("new {owner}({source})"))
            }
            Expr::NewArray { element, size } => {
                let element = n.type_token(*element, &[], g);
                // ジャグ配列（int[][]）は、大きさを最初の [] に入れる。
                let (base, rest) = match element.find('[') {
                    Some(at) => (&element[..at], &element[at..]),
                    None => (element.as_str(), ""),
                };
                primary(format!("new {base}[{}]{rest}", self.expr(size, 0)))
            }
            Expr::ArrayInit { element, values } => {
                let values: Vec<String> = values.iter().map(|v| self.expr(v, 0)).collect();
                primary(format!("new {}[] {{ {} }}", n.type_token(*element, &[], g), values.join(", ")))
            }
            Expr::Element { array, index } => primary(format!("{}[{}]", self.target(array), self.expr(index, 0))),
            Expr::Length(array) => primary(format!("{}.Length", self.target(array))),
            // `(T)(x as T)`（値型のパターン）は `(T)x`。
            Expr::Cast { ty, value } => {
                let inner = match &**value {
                    Expr::As { ty: inner_ty, value: inner } if inner_ty == ty => inner,
                    other => other,
                };
                (format!("({}){}", n.type_token(*ty, &[], g), self.expr(inner, p::UNARY)), p::UNARY)
            }
            Expr::As { ty, value } => (format!("{} as {}", self.expr(value, p::RELATIONAL), n.type_token(*ty, &[], g)), p::RELATIONAL),
            // 値型の箱詰めは、C# では書かなくても起きる。
            Expr::Box { value, .. } => self.expr_with_level(value),
            Expr::TypeOf(ty) => primary(format!("typeof({})", n.open_type(*ty, g))),
            Expr::Token(ty) => primary(format!("typeof({}).TypeHandle", n.type_token(*ty, &[], g))),
            Expr::SizeOf(ty) => primary(format!("sizeof({})", n.type_token(*ty, &[], g))),
            Expr::FunctionPointer(method) => match n.method_ref(*method, g) {
                Some(m) => (format!("&{}.{}", m.declaring, ident(&m.name)), p::UNARY),
                None => primary(format!("[0x{method:08X}]")),
            },
            Expr::Assign(target, value) => (format!("{} = {}", self.expr(target, p::ASSIGN + 1), self.expr(value, p::ASSIGN)), p::ASSIGN),
            Expr::Conditional(condition, a, b) => (format!("{} ? {} : {}", self.expr(condition, p::COALESCE), self.expr(a, p::CONDITIONAL), self.expr(b, p::CONDITIONAL)), p::CONDITIONAL),
            Expr::Coalesce(a, b) => (format!("{} ?? {}", self.expr(a, p::COALESCE + 1), self.expr(b, p::COALESCE)), p::COALESCE),
            Expr::Initializer { new, collection, members } => {
                let items: Vec<String> = if !collection.is_empty() {
                    collection
                        .iter()
                        .map(|args| match args.as_slice() {
                            [single] => self.expr(single, p::ASSIGN),
                            many => format!("{{ {} }}", many.iter().map(|a| self.expr(a, p::ASSIGN)).collect::<Vec<_>>().join(", ")),
                        })
                        .collect()
                } else {
                    members.iter().map(|(member, value)| format!("{member} = {}", self.expr(value, p::ASSIGN))).collect()
                };
                // 引数の無い `new C()` は、初期化子があれば `()` を省く。
                let created = match &**new {
                    Expr::New { method, args } if args.is_empty() => n.method_ref(*method, g).map_or_else(|| self.expr(new, p::PRIMARY), |m| format!("new {}", m.declaring)),
                    other => self.expr(other, p::PRIMARY),
                };
                primary(format!("{created} {{ {} }}", items.join(", ")))
            }
            Expr::NullConditional(inner) => primary(format!("{}?", self.expr(inner, p::PRIMARY))),
            Expr::Interpolated(parts) => primary(self.interpolated_text(parts)),

        }
    }

    /// 文字列の補間（`$"{name} has {count,5:F2}"`）。文字列の部分は `{` `}` を二重にし、穴の値に `:` があれば括弧に入れる
    /// （`:` は書式の始まりなので、`?:` や `global::` はそのままでは書けない）。
    fn interpolated_text(&self, parts: &[Part]) -> String {
        let mut text = String::from("$\"");
        for part in parts {
            match part {
                Part::Text(literal) => {
                    let quoted = string_literal(literal);
                    text.push_str(&quoted[1..quoted.len() - 1].replace('{', "{{").replace('}', "}}"));
                }
                Part::Hole { value, alignment, format } => {
                    let inner = self.expr(value, p::COALESCE);
                    let inner = if inner.contains(':') { format!("({inner})") } else { inner };
                    let alignment = alignment.map(|a| format!(",{a}")).unwrap_or_default();
                    let format = format.as_ref().map(|f| format!(":{f}")).unwrap_or_default();
                    text.push_str(&format!("{{{inner}{alignment}{format}}}"));
                }
            }
        }
        text.push('"');
        text
    }
    /// メンバーを呼ぶ相手（`x.Method()` の `x`）。アドレスは、その指す先の名前。
    fn target(&self, target: &Expr) -> String {
        match target {
            Expr::AddressOf(inner) | Expr::Deref(inner) => self.expr(inner, p::PRIMARY),
            Expr::Box { value, .. } => self.expr(value, p::PRIMARY),
            other => self.expr(other, p::PRIMARY),
        }
    }

    /// 引数の並び。ByRef の引数は `ref x`（MethodDef で Out の印があれば `out x`）。
    fn arguments_text(&self, args: &[Expr], params: &[Ty], method: u32) -> String {
        // out の印を読むのは、ref の引数があるときだけ（引数の表を引くので）。
        let outs = if params.iter().any(|t| matches!(t.unmodified(), Ty::ByRef(_))) { self.names.out_parameters(method) } else { Vec::new() };
        let overloaded = args.contains(&Expr::Null) && self.overloaded(method, args.len());
        args.iter()
            .enumerate()
            .map(|(i, a)| {
                let by_ref = params.get(i).is_some_and(|t| matches!(t.unmodified(), Ty::ByRef(_)));
                match a {
                    Expr::AddressOf(inner) if by_ref => {
                        let modifier = if outs.contains(&i) { "out" } else { "ref" };
                        format!("{modifier} {}", self.expr(inner, p::UNARY))
                    }
                    // ref の引数をそのまま渡す（ref の先を渡し直す）。自分の out の引数を渡すなら、相手も out。
                    other if by_ref && !matches!(other, Expr::AddressOf(_)) => {
                        let forwarded_out = matches!(other, Expr::Arg(a) if self.out_arguments.contains(&(*a as u32)));
                        let modifier = if outs.contains(&i) || forwarded_out { "out" } else { "ref" };
                        format!("{modifier} {}", self.expr(other, p::UNARY))
                    }
                    // 引数に渡すラムダは、デリゲートの型を書かない（`Where(n => n > 1)`）。受ける型が `Delegate` などなら、
                    // 型が決まらないので書く。
                    Expr::NewDelegate { target, method, .. } if params.get(i).is_some_and(|t| !matches!(self.names.ty(t, self.generics).as_str(), "Delegate" | "MulticastDelegate" | "object")) => {
                        match self.lambda_text(*method, target.as_deref()) {
                            Some(lambda) => lambda,
                            None => self.expr(a, 0),
                        }
                    }
                    Expr::Null if overloaded => match params.get(i) {
                        Some(ty) => format!("({})null", self.names.ty(ty, self.generics)),
                        None => "null".to_string(),
                    },
                    other => self.expr(other, 0),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// 二項演算子の優先順位。
fn binary_level(symbol: &str) -> u8 {
    match symbol {
        "*" | "/" | "%" => p::MULTIPLICATIVE,
        "+" | "-" => p::ADDITIVE,
        "<<" | ">>" => p::SHIFT,
        "&" => p::BIT_AND,
        "^" => p::BIT_XOR,
        _ => p::BIT_OR,
    }
}

/// フィールドの初期化子に移せる値（引数、`this`、ローカル変数、一時変数、ラムダを使わない）。
pub(super) fn is_initializer_value(value: &Expr) -> bool {
    let mut ok = true;
    value.walk(&mut |e| ok &= !matches!(e, Expr::Arg(_) | Expr::Local(_) | Expr::Temp(_) | Expr::NewDelegate { .. } | Expr::Exception));
    ok
}

/// メソッド `row` の文脈で、式を C# で書く（フィールドの初期化子の値）。
pub(super) fn expression_text(scope: &Scope, row: u32, generics: &Generics, value: &Expr) -> String {
    let names = scope.names;
    let arguments = names.method_def_sig(row).map(|s| ilview::argument_names(names, row, &s)).unwrap_or_default();
    let current_type = names.md.declaring_type(row).map(|t| names.type_def(t, &[], 0)).unwrap_or_default();
    let mut writer = Writer::new(scope, generics, &arguments, &[], &[], current_type);
    writer.type_row = names.md.declaring_type(row);
    writer.expr(value, p::ASSIGN)
}
/// メソッドのローカル変数の名前。捕まえた変数（戻した表示用のクラスのフィールド）は、もとの名前のまま。ほかの変数は、
/// それらと、式の中に入れるラムダの引数の名前を避けて、型から付ける。
fn method_local_names(names: &Names, locals: &[Ty], generics: &Generics, arguments: &[String], lifted: &super::closures::Lifted) -> Vec<String> {
    let avoid: Vec<String> = arguments.iter().cloned().chain(lifted.captured.iter().map(|(_, n)| n.clone())).chain(lifted.parameters.iter().cloned()).collect();
    let mut list = local_names(names, locals, generics, &avoid);
    for (index, name) in &lifted.captured {
        if let Some(slot) = list.get_mut(*index as usize) {
            *slot = name.clone();
        }
    }
    list
}
/// ローカル変数の名前を、型から付ける（`num`、`text`、`list`）。引数の名前とも重ならないようにする。
pub fn local_names(names: &Names, locals: &[Ty], generics: &Generics, arguments: &[String]) -> Vec<String> {
    let mut used: HashSet<String> = arguments.iter().cloned().collect();
    locals
        .iter()
        .map(|ty| {
            let base = base_name(names, ty, generics);
            let mut name = base.clone();
            let mut counter = 2;
            while used.contains(&name) || super::names::is_keyword(&name) {
                name = format!("{base}{counter}");
                counter += 1;
            }
            used.insert(name.clone());
            name
        })
        .collect()
}

fn base_name(names: &Names, ty: &Ty, generics: &Generics) -> String {
    match ty.unmodified() {
        Ty::Bool => "flag".into(),
        Ty::Char => "c".into(),
        Ty::I1 | Ty::U1 | Ty::I2 | Ty::U2 | Ty::I4 | Ty::U4 | Ty::I8 | Ty::U8 => "num".into(),
        Ty::R4 | Ty::R8 => "value".into(),
        Ty::String => "text".into(),
        Ty::Object => "obj".into(),
        Ty::SzArray(_) | Ty::Array { .. } => "array".into(),
        Ty::Pointer(_) | Ty::IntPtr | Ty::UIntPtr => "ptr".into(),
        Ty::ByRef(inner) => base_name(names, inner, generics),
        Ty::Var(_) | Ty::MVar(_) => "item".into(),
        other => {
            // 型の名前の頭を小文字にする（`List<string>` → `list`、`IEnumerator<T>` → `enumerator`）。
            let full = names.ty(other, generics);
            let simple = full.split('<').next().unwrap_or(&full).rsplit('.').next().unwrap_or(&full).trim_end_matches('?');
            let simple = if simple.len() > 1 && simple.starts_with('I') && simple[1..].starts_with(|c: char| c.is_ascii_uppercase()) { &simple[1..] } else { simple };
            let mut chars = simple.chars().filter(|c| c.is_alphanumeric() || *c == '_');
            match chars.next() {
                Some(first) => first.to_lowercase().chain(chars).collect(),
                None => "local".into(),
            }
        }
    }
}

/// メソッドの本体を C# で書く。戻せなければ `Err` を返し、何も書かない（呼び出し側が IL の形で書く）。
pub fn write_method_body(scope: &Scope, row: u32, body: &MethodBody, generics: &Generics, depth: usize, text: &mut Text) -> Result<BodyMap, String> {
    let names = scope.names;
    let Prepared { signature, locals, mut statements, temps, applied, initializer, real_locals, lifted, .. } = prepare(names, scope.image, row, body)?;
    // フィールドの宣言に移した初期化子（`writer.rs`）は、本体に書かない。
    if let Some(&moved) = scope.moved.get(&row) {
        statements.drain(..moved.min(statements.len()));
    }

    let arguments = ilview::argument_names(names, row, &signature);
    let local_names = method_local_names(names, &locals, generics, &arguments, &lifted);
    let current_type = names.md.declaring_type(row).map(|t| names.type_def(t, &[], 0)).unwrap_or_default();
    let mut writer = Writer::new(scope, generics, &arguments, &local_names, &locals, current_type);
    writer.closures = lifted.lambdas.clone();
    let has_this = signature.has_this && !signature.explicit_this;
    writer.out_arguments = names.out_parameters((METHOD_DEF as u32) << 24 | row).into_iter().map(|i| i as u32 + has_this as u32).collect();
    writer.type_row = names.md.declaring_type(row);
    writer.argument_types = has_this.then_some(None).into_iter().chain(signature.params.iter().map(|t| Some(t.clone()))).collect();
    let context = sugar::Context { names, returns_void: true, locals: 0 };
    let mut caches = false;
    super::ast::visit_expressions(&statements, &mut |e| caches |= sugar::is_cache_field(&context, e));
    writer.inline_lambdas = !caches;
    // 本体の括弧は、初期化子の行（頭の次の行）のあとに書く。
    if let Some(stmt) = &initializer {
        if let Some(line) = writer.initializer_text(stmt) {
            let line = text.line(depth, &line);
            writer.point(stmt, line);
        }
    }
    text.line(depth - 1, "{");
    write_declarations(names, generics, &statements, &locals, &local_names, &applied, &temps, depth, text);
    writer.block(&statements, depth, text);
    writer.resolve_lambdas(text);
    let points = writer.points.into_values().collect();
    let lambdas = std::mem::take(&mut writer.lambdas).into_values().collect();
    // 合成した PDB には IL のローカル変数だけ。表示用のクラスを入れたものは `CS$<>8__locals0`（式の評価が、そのフィールドを
    // 捕まえた変数として見せる）。
    let pdb_locals = local_names
        .into_iter()
        .take(real_locals)
        .enumerate()
        .map(|(i, name)| match lifted.slots.iter().position(|&s| s as usize == i) {
            Some(n) => (i as u32, format!("CS$<>8__locals{n}")),
            None => (i as u32, name),
        })
        .collect();
    Ok(BodyMap { points, locals: pdb_locals, end: applied.end, lambdas })
}

/// メソッドの本体を、C# の文の木にしたもの（書く前）。
pub(super) struct Prepared {
    pub signature: super::sig::MethodSig,
    /// ローカル変数の型。戻した表示用のクラスの、捕まえた変数の分が後ろに付く（`real_locals` から）。
    pub locals: Vec<Ty>,
    pub statements: Vec<Stmt>,
    /// 一時変数と、その型（分かれば）。
    pub temps: Vec<(u32, Option<Ty>)>,
    pub applied: sugar::Applied,
    /// コンストラクターの初期化子にする呼び出し。
    pub initializer: Option<Stmt>,
    /// 初期化子の呼び出しより前にあった文の数（フィールドの初期化子。`writer.rs` がフィールドの宣言に移す）。
    pub before_initializer: usize,
    /// IL のローカル変数の数。
    pub real_locals: usize,
    /// 戻した表示用のクラス（`closures.rs`）。
    pub lifted: super::closures::Lifted,
}

/// IL から文の木を組み立て、制御構造と書き方を戻す。戻せなければ `Err`。
pub(super) fn prepare(names: &Names, image: &[u8], row: u32, body: &MethodBody) -> Result<Prepared, String> {
    let signature = names.method_def_sig(row).ok_or_else(|| tr!("Cannot read the signature"))?;
    let mut locals = names.local_types(body.local_signature);
    let real_locals = locals.len();
    let code: Vec<Instruction> = il::instructions(body.code).collect();
    if code.last().map_or(0, Instruction::end) as usize != body.code.len() {
        return Err(tr!("Cannot read the IL to the end"));
    }
    let effect = |instruction: &Instruction| ilview::stack_effect(names, &signature, instruction);
    let depths = flow::stack_depths(&code, &body.clauses, &effect);
    let this_type = this_type(names, row, &signature);
    let constructor = this_type.is_some() && names.md.string(names.md.cell(METHOD_DEF, row, 3)) == ".ctor";
    let value_type = matches!(this_type, Some(Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. }));
    let built = build::build(&build::Method { names, signature: &signature, locals: &locals, code: &code, clauses: &body.clauses, depths: &depths, image, this_type, constructor })?;
    let mut statements = built.body;
    structure::structure(&mut statements)?;
    // 捕まえた変数を、表示用のクラスのフィールドから変数に戻す（書き方の変換の前に。最初の代入での宣言にまとめるため）。
    let arguments = ilview::argument_names(names, row, &signature);
    let lifted = super::closures::lift(names, image, &arguments, &mut locals, &mut statements);
    let applied = sugar::apply(&sugar::Context { names, returns_void: signature.ret.is_void(), locals: locals.len() }, &mut statements);
    let (initializer, before_initializer) = if constructor { take_initializer(&mut statements, value_type)?.map_or((None, 0), |(stmt, index)| (Some(stmt), index)) } else { (None, 0) };
    Ok(Prepared { signature, locals, statements, temps: built.temps, applied, initializer, before_initializer, real_locals, lifted })
}

/// 本体の頭の、ローカル変数と一時変数の宣言。
#[allow(clippy::too_many_arguments)]
fn write_declarations(names: &Names, generics: &Generics, statements: &[Stmt], locals: &[Ty], local_names: &[String], applied: &sugar::Applied, temps: &[(u32, Option<Ty>)], depth: usize, text: &mut Text) {
    // 文の中で宣言する変数（foreach、using、最初の代入）と、使われなくなった変数は、先頭では宣言しない。
    let mut used = HashSet::new();
    super::ast::visit_expressions(statements, &mut |e| {
        if let Expr::Local(l) = e {
            used.insert(*l);
        }
    });
    // 先頭で宣言する変数は、C# のフロー解析で「代入されていない」とされることがある（構造体のフィールドだけに代入する、
    // goto で代入を飛び越す）。IL のローカル変数は 0 で初期化されるので、`= default` は IL と同じ意味。
    let has_gotos = {
        let mut found = false;
        super::ast::visit(statements, &mut |s| found |= matches!(s.kind, StmtKind::Goto(_) | StmtKind::Label(_)));
        found
    };
    for (_, (ty, name)) in locals.iter().zip(local_names).enumerate().filter(|(i, _)| !applied.declared.contains(&(*i as u16)) && used.contains(&(*i as u16))) {
        let value_type = matches!(ty.unmodified(), Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. } | Ty::Var(_) | Ty::MVar(_));
        let initializer = if (has_gotos || value_type) && !matches!(ty.unmodified(), Ty::ByRef(_)) { " = default" } else { "" };
        text.line(depth, &format!("{} {name}{initializer};", declared_type(names, ty, generics)));
    }
    // 使われている一時変数だけを宣言する（消した代入の分は要らない）。
    let mut used = HashSet::new();
    collect_statement_temps(statements, &mut used);
    for (temp, ty) in temps.iter().filter(|(t, _)| used.contains(t)) {
        let ty = ty.as_ref().map_or_else(|| "object".to_string(), |t| declared_type(names, t, generics));
        text.line(depth, &format!("{ty} {};", temp_name(*temp)));
    }
}

/// コンストラクターの本体から、`this` に呼ぶコンストラクター（初期化子にするもの）を取り出す。C# の初期化子に書けるのは、
/// 本体の一番外の文で、引数がローカル変数を使わないもの。書けなければ C# に戻さない（IL の形で見せる）。
/// 取り出した呼び出しと、その前にあった文の数。
fn take_initializer(statements: &mut Vec<Stmt>, value_type: bool) -> Result<Option<(Stmt, usize)>, String> {
    // 呼び出しの前に分かれ道があると、`this` が一時変数を通るので、`this = new C(…)` の形で届く（一時変数は sugar で
    // `this` に戻る）。クラスでは `this` に代入できないので、これも初期化子。構造体では、本体の頭にあるときだけ
    // （途中の `this = new S(…)` は、ふつうの代入）。
    for (index, stmt) in statements.iter_mut().enumerate() {
        if value_type && index > 0 {
            break;
        }
        if let StmtKind::Assign(Expr::Arg(0) | Expr::Deref(_), Expr::New { method, args }) = &stmt.kind {
            if matches!(&stmt.kind, StmtKind::Assign(Expr::Deref(inner), _) if **inner != Expr::Arg(0)) {
                continue;
            }
            stmt.kind = StmtKind::Expr(Expr::Call { method: *method, target: Some(Expr::Arg(0).boxed()), args: args.clone(), virtual_call: false });
        }
    }
    let is_call = |s: &Stmt| matches!(&s.kind, StmtKind::Expr(Expr::Call { target: Some(t), .. }) if matches!(**t, Expr::Arg(0)));
    let Some(index) = statements.iter().position(is_call) else {
        let mut nested = false;
        super::ast::visit(statements, &mut |s| nested |= is_call(s));
        return if nested { Err(tr!("The call to the base constructor is inside a statement")) } else { Ok(None) };
    };
    let mut uses_locals = false;
    for e in statements[index].expressions() {
        e.walk(&mut |e| uses_locals |= matches!(e, Expr::Local(_) | Expr::Temp(_)));
    }
    if uses_locals {
        return Err(tr!("The base constructor's arguments use local variables"));
    }
    Ok(Some((statements.remove(index), index)))
}

/// インスタンスメソッドの `this` の型（宣言している型。ジェネリックなら型引数をそのまま並べたもの）。
fn this_type(names: &Names, row: u32, signature: &super::sig::MethodSig) -> Option<Ty> {
    if !signature.has_this || signature.explicit_this {
        return None;
    }
    let owner = names.md.declaring_type(row)?;
    let token = 0x0200_0000 | owner;
    let value_type = matches!(names.type_kind(owner), super::decl::TypeKind::Struct | super::decl::TypeKind::Enum);
    let count = names.md.type_parameter_count(owner) as u32;
    Some(if count == 0 { Ty::Named { token, value_type } } else { Ty::Generic { token, value_type, arguments: (0..count).map(Ty::Var).collect() } })
}

/// 宣言に書く型。`pinned` と ByRef（ref のローカル変数）は、中の型で書く。
fn declared_type(names: &Names, ty: &Ty, generics: &Generics) -> String {
    match ty.unmodified() {
        Ty::ByRef(inner) => format!("ref {}", names.ty(inner, generics)),
        other => names.ty(other, generics),
    }
}
/// 文の中に出てくる一時変数（代入先も含む）。
fn collect_statement_temps(body: &[Stmt], used: &mut HashSet<u32>) {
    super::ast::visit_expressions(body, &mut |e| {
        if let Expr::Temp(t) = e {
            used.insert(*t);
        }
    });
}
