//! ラムダのメソッドを、式の中に書く（`x => x * 3`、文が 2 つ以上なら複数の行の `x => { … }`）。表示用のクラスを戻した
//! ラムダ（`closures.rs`）もここで書く。入れたラムダの系列点は、式 1 つなら外の文の行に、複数の行なら本体の行に打つ
//! （本体の前に目印 `MARK` を埋め、メソッドを書き終えてから `resolve_lambdas` で置き直す）。

use super::*;
use crate::debugger::decompile::ast::{visit, visit_expressions};
/// 書いている文の中に入れたラムダ 1 つ: メソッドの行、系列点、ローカル変数の名前。
pub(super) struct PendingLambda {
    pub(super) row: u32,
    /// 式 1 つのラムダの系列点の IL オフセット（文の行に打つ）。
    pub(super) offsets: Vec<u32>,
    /// 複数の行で書いたラムダの本体の系列点（行は、目印の行からの数。列は、その行の字下げからの数）と、目印のメソッドの
    /// 行（入れ子のラムダは、外のラムダの目印から数える）。
    pub(super) body: Option<(u32, Vec<Point>)>,
    pub(super) locals: Vec<(u32, String)>,
}

impl Writer<'_> {
    /// コンパイラが作ったラムダのメソッド（`<Run>b__2_0`）を、式 1 つのラムダ（`x => x * 3 + 1`）として書く。入れられる
    /// のは、何も捕まえないもの（`<>c` のシングルトンのメソッド）と、`this` だけを使うもの（同じ型のインスタンスのメソッド）
    /// で、本体が式 1 つのもの。入れたら、そのメソッドの系列点は、この文の行に打つ。
    pub(super) fn lambda_text(&self, method: u32, target: Option<&Expr>) -> Option<String> {
        let names = self.names;
        let md = names.md;
        let row = names.method_definition(method)?;
        let name = md.string(md.cell(METHOD_DEF, row, 3));
        // 表示用のクラスを戻したラムダ（`closures.rs`）は、必ず入れる（外のメソッドは、表示用のクラスを消してある）。
        let closure = self.closures.contains(&row) && target.is_none();
        if (!self.inline_lambdas && !closure) || self.written.contains(&row) || !(name.starts_with('<') && name.contains(">b__")) {
            return None;
        }
        let declaring = md.declaring_type(row)?;
        let singleton = matches!(target, Some(Expr::Field { target: None, .. })) && md.string(md.cell(TYPE_DEF, declaring, 1)) == "<>c";
        let own_instance = target == Some(&Expr::Arg(0)) && Some(declaring) == self.type_row;
        if !singleton && !own_instance && !closure {
            return None;
        }
        let body = body::read(self.image, md.cell(METHOD_DEF, row, 0)).ok()?;
        let prepared = prepare(names, self.image, row, &body).ok()?;
        // シングルトンの `this`（`<>c` のインスタンス）は書けない。
        let mut uses_singleton = false;
        visit_expressions(&prepared.statements, &mut |e| uses_singleton |= singleton && *e == Expr::Arg(0));
        if uses_singleton {
            return None;
        }
        let arguments = ilview::argument_names(names, row, &prepared.signature);
        let has_this = prepared.signature.has_this && !prepared.signature.explicit_this;
        let parameters = &arguments[has_this as usize..];
        // 外の変数と同じ名前の引数は、C# では書けない。
        if parameters.iter().any(|p| self.shadowed(p)) {
            return None;
        }
        let generics = Generics { types: names.type_generics(declaring), methods: names.method_generics(row) };
        let head = match parameters {
            [single] => single.clone(),
            _ => format!("({})", parameters.join(", ")),
        };
        match self.expression_lambda(&prepared, &arguments, &generics, row, closure) {
            Some(text) => Some(format!("{head} => {text}")),
            None => Some(format!("{head} =>{}", self.block_lambda(&prepared, &arguments, &generics, row, closure)?)),
        }
    }

    /// 本体が式 1 つのラムダ（`x => x * 3 + 1`）。ローカル変数と一時変数を使うものは、文の並びとして書く。
    fn expression_lambda(&self, prepared: &Prepared, arguments: &[String], generics: &Generics, row: u32, closure: bool) -> Option<String> {
        let [stmt] = prepared.statements.as_slice() else { return None };
        let expr = match &stmt.kind {
            StmtKind::Return(Some(e)) | StmtKind::Expr(e) => e.clone(),
            StmtKind::Assign(a, b) => Expr::Assign(a.clone().boxed(), b.clone().boxed()),
            _ => return None,
        };
        let mut unsupported = false;
        expr.walk(&mut |e| unsupported |= matches!(e, Expr::Temp(_) | Expr::Local(_)));
        if unsupported {
            return None;
        }
        let mut inner = Writer::new(&self.scope(), generics, arguments, &[], &[], self.current_type.clone());
        inner.type_row = self.type_row;
        inner.inline_lambdas = self.inline_lambdas;
        inner.closure_body = closure;
        let text = inner.expr(&expr, 0);
        let mut pending = self.pending_lambdas.borrow_mut();
        pending.push(PendingLambda { row, offsets: stmt.il.iter().chain(&stmt.merged).copied().collect(), body: None, locals: Vec::new() });
        pending.extend(inner.pending_lambdas.take());
        Some(text)
    }

    /// 文が 2 つ以上のラムダ。本体は複数の行で書く（文ごとに止まれるように、ラムダの系列点は本体の行に打つ）:
    ///
    /// ```text
    /// button.Click += new EventHandler((s, e) =>
    /// {
    ///     string text = …;
    /// });
    /// ```
    ///
    /// 返す文字列は、改行から始まり、本体の開き括弧の前に目印（`MARK`）を持つ。goto のあるものは入れない（メソッドの
    /// まま残す）。表示用のクラスを戻したもの（`closure`）も同じ書き方。
    fn block_lambda(&self, prepared: &Prepared, arguments: &[String], generics: &Generics, row: u32, closure: bool) -> Option<String> {
        let names = self.names;
        let mut jumps = false;
        visit(&prepared.statements, &mut |s| jumps |= matches!(s.kind, StmtKind::Goto(_) | StmtKind::Label(_)));
        if jumps || prepared.statements.is_empty() {
            return None;
        }
        // ラムダのローカル変数は、外のローカル変数や引数と同じ名前にできない（CS0136）。
        let avoid: Vec<String> = arguments.iter().chain(self.locals).chain(self.arguments).cloned().collect();
        let local_names = local_names(names, &prepared.locals, generics, &avoid);
        let mut inner = Writer::new(&self.scope(), generics, arguments, &local_names, &prepared.locals, self.current_type.clone());
        inner.type_row = self.type_row;
        inner.inline_lambdas = self.inline_lambdas;
        inner.closure_body = closure;
        let mut text = Text::new();
        write_declarations(names, generics, &prepared.statements, &prepared.locals, &local_names, &prepared.applied, &prepared.temps, 1, &mut text);
        inner.block(&prepared.statements, 1, &mut text);
        // 中に入れたラムダの行は、この本体の中の行に置き直してから、この本体の目印から数える。
        inner.resolve_lambdas(&text);
        let mut points: Vec<Point> = inner.points.values().copied().collect();
        // 外した末尾の `return;` は、閉じ括弧の行。
        let close = text.lines + 1;
        points.extend(prepared.applied.end.iter().map(|&il_offset| Point { il_offset, line: close, start_column: 1, end_column: 2 }));
        let locals = local_names.iter().enumerate().map(|(i, name)| (i as u32, name.clone())).collect();
        let mut pending = self.pending_lambdas.borrow_mut();
        pending.push(PendingLambda { row, offsets: Vec::new(), body: Some((row, points)), locals });
        for nested in std::mem::take(&mut inner.lambdas).into_values() {
            pending.push(PendingLambda { row: nested.row, offsets: Vec::new(), body: Some((row, nested.points)), locals: nested.locals });
        }
        pending.extend(inner.pending_lambdas.take());
        let body = text.out.replace("\r\n", "\n");
        Some(format!("\n{MARK}{row}{MARK}{{\n{}\n}}", body.trim_end_matches('\n')))
    }
    /// 複数の行で書いたラムダの系列点を、目印の行（`Text::marks`）を基に、書いた行と列に置き直す。
    pub(super) fn resolve_lambdas(&mut self, text: &Text) {
        for (row, anchor) in std::mem::take(&mut self.anchors) {
            let (Some(lambda), Some(&(line, indent))) = (self.lambdas.get_mut(&row), text.marks.get(&anchor)) else { continue };
            for point in &mut lambda.points {
                point.line += line;
                point.start_column += indent;
                point.end_column += indent;
            }
        }
    }
}
