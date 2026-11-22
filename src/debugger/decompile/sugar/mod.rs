//! 段階 3: 書き方の復元。組み直した文の木（`structure.rs` のあと）を、C# の書き方に近づける。
//!
//! 各変換は独立した関数で、`apply` が順に当てる。どれも意味を変えない条件を確かめてから当てる。消した文の系列点は、
//! まとめた先の文に残す（`Stmt::absorb`）ので、ブレークポイントとステップの位置は失われない。

mod blocks;
mod expressions;
mod flow;
mod temps;

use super::ast::{visit, visit_expressions, visit_expressions_mut, BinOp, CmpOp, Expr, Label, Part, Section, Stmt, StmtKind};
use super::build::negate;
use super::names::Names;
use super::sig::Ty;
use blocks::*;
use expressions::*;
use flow::*;
use std::collections::{HashMap, HashSet};
use temps::*;

/// 変換の結果: 文の中で宣言するローカル変数（先頭で宣言しない）と、閉じ括弧の行に打つ系列点（外した末尾の `return;`）。
#[derive(Default)]
pub struct Applied {
    pub declared: HashSet<u16>,
    pub end: Vec<u32>,
}

pub struct Context<'a> {
    pub names: &'a Names<'a>,
    pub returns_void: bool,
    /// ローカル変数の数。
    pub locals: usize,
}

pub fn apply(context: &Context, body: &mut Vec<Stmt>) -> Applied {
    let mut applied = Applied::default();
    // 式の形（`?.`、`??`、三項、一時変数）を先に戻す。流れの形を変える変換（共有の return など）のあとでは、形が崩れる。
    copy_temps(body);
    uniform_temps(body);
    receiver_temps(body);
    for_each_block(body, &mut |block| empty_then(block));
    for_each_block(body, &mut |block| null_conditional(block));
    for_each_block(body, &mut |block| coalesce(block));
    for_each_block(body, &mut |block| delegate_caches(context, block));
    remove_dead_simple_temps(body);
    for_each_block(body, &mut |block| coalesce_assign(context, block));
    for_each_block(body, &mut |block| conditional(block));
    inline_temps(body);
    for_each_block(body, &mut |block| initializers(context, block));
    // 初期化子にまとめた値（`s = new List<int> { … }`）を、一度だけ使う所へ入れる。
    inline_temps(body);
    shared_returns(body);
    remove_dead_code(body);
    fallthrough_gotos(body, &[]);
    switch_sections(body);
    switch_values(body);
    remove_unused_labels(body);
    // ラベルが消えて、届かなくなった文（文字列の switch の、goto で飛んでいた case の本体の残り）。
    remove_dead_code(body);
    return_values(body);
    foreach_loops(context, body, &mut applied.declared);
    using_blocks(context, body, &mut applied.declared);
    lock_blocks(context, body, &mut applied.declared);
    for_each_block(body, &mut |block| array_elements(block));
    // 配列の初期化子にまとめた一時変数（`t0 = new string[] { … }`）を、一度だけ使う所（`string.Concat(t0)`）へ入れる。
    inline_temps(body);
    concat(context, body);
    // 文字列の補間（.NET の DefaultInterpolatedStringHandler と、.NET Framework の string.Format）。
    for_each_block(body, &mut |block| interpolated_handlers(context, block));
    visit_expressions_mut(body, &mut |e| {
        if let Some(interpolated) = format_interpolation(context, e) {
            *e = interpolated;
        }
    });
    for_each_block(body, &mut |block| compound(context, block));
    for_each_block(body, &mut |block| for_loops(block));
    if context.returns_void {
        if let Some(Stmt { kind: StmtKind::Return(None), .. }) = body.last() {
            let last = body.pop().unwrap_or_else(|| Stmt::new(StmtKind::Break, None));
            applied.end.extend(last.il.iter().chain(&last.merged));
        }
    }
    if !has_gotos(body) {
        declare_at_first_assignment(body, context.locals, &mut applied.declared);
    }
    applied
}

/// ブロック（文の並び）ごとに `f` を当てる（内側から）。
fn for_each_block(body: &mut Vec<Stmt>, f: &mut impl FnMut(&mut Vec<Stmt>)) {
    for stmt in body.iter_mut() {
        stmt.for_each_block_mut(&mut |inner| for_each_block(inner, f));
    }
    f(body);
}

/// 式の中の `Temp(t)` を `replacement` に置き換える。置き換えた数。
fn substitute(expr: &mut Expr, target: &Expr, replacement: &Expr) -> usize {
    if expr == target {
        *expr = replacement.clone();
        return 1;
    }
    let mut count = 0;
    expr.for_each_child_mut(&mut |child| count += substitute(child, target, replacement));
    count
}

fn substitute_in(body: &mut [Stmt], target: &Expr, replacement: &Expr) -> usize {
    let mut count = 0;
    for stmt in body.iter_mut() {
        stmt.for_each_expression_mut(&mut |e| count += substitute(e, target, replacement));
        stmt.for_each_block_mut(&mut |inner| count += substitute_in(inner, target, replacement));
    }
    count
}

/// 式 `target`（ローカル変数か一時変数）が出てくる数（読みと書きの両方）。
fn occurrences(body: &[Stmt], target: &Expr) -> usize {
    let mut count = 0;
    visit(body, &mut |s| {
        // foreach / using / 宣言の変数は、式ではなく文の欄に入っている。
        if let (Expr::Local(local), Some(l)) = (target, declared_local(s)) {
            count += (l == *local) as usize;
        }
        s.for_each_expression(&mut |e| e.walk(&mut |e| count += (e == target) as usize));
    });
    count
}

/// 文の欄にある変数（`foreach (int x …)`、`using (R r = …)`、宣言 `int x = …;`）。
fn declared_local(stmt: &Stmt) -> Option<u16> {
    match &stmt.kind {
        StmtKind::Foreach { local, .. } | StmtKind::Declare { local, .. } | StmtKind::Using { local: Some(local), .. } => Some(*local),
        _ => None,
    }
}

/// 文の中に出てくるローカル変数を、出てくるたびに `f` に渡す（`occurrences` を、すべての変数について一度に数える）。
fn for_each_local(body: &[Stmt], f: &mut impl FnMut(u16)) {
    visit(body, &mut |s| {
        if let Some(local) = declared_local(s) {
            f(local);
        }
        s.for_each_expression(&mut |e| {
            e.walk(&mut |e| {
                if let Expr::Local(local) = e {
                    f(*local);
                }
            })
        });
    });
}

/// ローカル変数ごとの、出てくる数。
fn local_counts(body: &[Stmt], locals: usize) -> Vec<usize> {
    let mut counts = vec![0; locals];
    for_each_local(body, &mut |local| {
        if let Some(count) = counts.get_mut(local as usize) {
            *count += 1;
        }
    });
    counts
}

/// 式の中に `target` が出てくるか。
fn mentions(expr: &Expr, target: &Expr) -> bool {
    let mut found = false;
    expr.walk(&mut |e| found |= e == target);
    found
}

/// 代入された数（`target = …`）。
fn assignments(body: &[Stmt], target: &Expr) -> usize {
    let mut count = 0;
    visit(body, &mut |s| count += matches!(&s.kind, StmtKind::Assign(t, _) if t == target) as usize);
    count
}

fn has_gotos(body: &[Stmt]) -> bool {
    let mut found = false;
    visit(body, &mut |s| found |= matches!(s.kind, StmtKind::Goto(_) | StmtKind::Label(_) | StmtKind::Switch { .. }));
    found
}

/// goto と switch が、ラベルごとに飛ぶ数（入れ子の中も含む）。
fn label_references(body: &[Stmt]) -> HashMap<Label, usize> {
    let mut counts = HashMap::new();
    visit(body, &mut |s| match &s.kind {
        StmtKind::Goto(l) => *counts.entry(*l).or_insert(0) += 1,
        StmtKind::Switch { targets, .. } => {
            for l in targets {
                *counts.entry(*l).or_insert(0) += 1;
            }
        }
        _ => {}
    });
    counts
}

/// `target = …;` の文を消す（系列点は次の文に移す）。
fn remove_assignment(body: &mut Vec<Stmt>, target: &Expr) -> bool {
    for i in 0..body.len() {
        if matches!(&body[i].kind, StmtKind::Assign(t, _) if t == target) {
            let removed = body.remove(i);
            if let Some(next) = body.get_mut(i) {
                next.absorb(&removed);
            }
            return true;
        }
        for inner in body[i].blocks_mut() {
            if remove_assignment(inner, target) {
                return true;
            }
        }
    }
    false
}

/// コンパイラが作った、デリゲートを使い回すためのフィールドか（`<>O` の型のフィールド、`<>9__` で始まる名前）。
pub(super) fn is_cache_field(context: &Context, field: &Expr) -> bool {
    let Expr::Field { target: None, token } = field else { return false };
    let md = context.names.md;
    let row = token & 0x00FF_FFFF;
    // ジェネリックな型の `<>c<T>` のフィールドは、MemberRef で指す。
    if token >> 24 == 0x0A {
        return md.string(md.cell(crate::debugger::metadata::MEMBER_REF, row, 1)).starts_with("<>9__");
    }
    if token >> 24 != 0x04 {
        return false;
    }
    let Some((owner, declared)) = md.field_declaration(row) else { return false };
    declared.name.starts_with("<>9__") || md.string(md.cell(crate::debugger::metadata::TYPE_DEF, owner, 1)) == "<>O"
}

/// 比べる式が `x != null`（`null != x`）なら x。
fn not_null(condition: &Expr) -> Option<&Expr> {
    match condition {
        Expr::Compare(CmpOp::Ne, a, b) if **b == Expr::Null => Some(a),
        Expr::Compare(CmpOp::Ne, a, b) if **a == Expr::Null => Some(b),
        _ => None,
    }
}

fn is_null(condition: &Expr) -> Option<&Expr> {
    match condition {
        Expr::Compare(CmpOp::Eq, a, b) if **b == Expr::Null => Some(a),
        Expr::Compare(CmpOp::Eq, a, b) if **a == Expr::Null => Some(b),
        _ => None,
    }
}

/// 呼び出しのメソッドの名前（と、宣言している型の名前）。
fn call_name(context: &Context, expr: &Expr) -> Option<(String, String)> {
    let method = match expr {
        Expr::Call { method, .. } | Expr::Property { method, .. } => *method,
        _ => return None,
    };
    let m = context.names.method_ref(method, &Default::default())?;
    Some((m.name, m.declaring))
}

/// 式が、ローカル変数 `local` そのもの（アドレス、キャスト、箱詰めを通しても）を指すか。
fn refers_to(expr: &Expr, local: &Expr) -> bool {
    match expr {
        Expr::AddressOf(inner) | Expr::Deref(inner) | Expr::Cast { value: inner, .. } | Expr::Box { value: inner, .. } => refers_to(inner, local),
        other => other == local,
    }
}

/// 二度評価しても同じ代入先（変数、静的フィールド、変数のフィールドとプロパティ、配列の要素）。
fn stable_target(expr: &Expr) -> bool {
    match expr {
        Expr::Local(_) | Expr::Arg(_) | Expr::Temp(_) => true,
        Expr::Field { target: None, .. } => true,
        Expr::Field { target: Some(t), .. } => stable_target(t) || matches!(**t, Expr::AddressOf(ref inner) if stable_target(inner)),
        Expr::Property { target, args, .. } => args.is_empty() && target.as_deref().is_none_or(stable_target),
        Expr::Element { array, index } => stable_target(array) && (index.is_simple()),
        _ => false,
    }
}

/// 同じ代入先か。プロパティは、get と set のアクセサーのトークンが違うので、受け手と名前（`get_` / `set_` を外したもの）で
/// 比べる。
fn same_target(context: &Context, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Property { target: ta, method: ma, args: aa }, Expr::Property { target: tb, method: mb, args: ab }) => {
            let name = |token: u32| context.names.method_ref(token, &Default::default()).map(|m| m.name.get(4..).unwrap_or_default().to_string());
            ta == tb && aa.is_empty() && ab.is_empty() && name(*ma).is_some() && name(*ma) == name(*mb)
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(kind: StmtKind, il: u32) -> Stmt {
        Stmt::new(kind, Some(il))
    }

    fn call(method: u32, args: Vec<Expr>) -> Expr {
        Expr::Call { method, target: None, args, virtual_call: false }
    }

    fn both(condition: Expr, then: Vec<Stmt>, otherwise: Vec<Stmt>) -> Stmt {
        Stmt::new(StmtKind::If { condition, then, otherwise }, None)
    }

    #[test]
    fn uniform_temps_follow_a_chain_of_copies_to_the_shared_value() {
        // 分かれ道で out の引数のアドレスを持ち回す形:
        // if (c) { t1 = result; } else { t1 = result; }  t10 = t1;  t13 = t1;  if (d) { t19 = t10; } else { t19 = t13; }  *t19 = v;
        let assign = |to: u32, value: Expr| Stmt::new(StmtKind::Assign(Expr::Temp(to), value), None);
        let mut body = vec![
            both(Expr::Local(0), vec![assign(1, Expr::Arg(2))], vec![assign(1, Expr::Arg(2))]),
            assign(10, Expr::Temp(1)),
            assign(13, Expr::Temp(1)),
            both(Expr::Local(1), vec![assign(19, Expr::Temp(10))], vec![assign(19, Expr::Temp(13))]),
            s(StmtKind::Assign(Expr::Deref(Box::new(Expr::Temp(19))), Expr::Str("v".into())), 0x20),
        ];
        uniform_temps(&mut body);
        let last = body.last().unwrap();
        assert_eq!(last.kind, StmtKind::Assign(Expr::Deref(Box::new(Expr::Arg(2))), Expr::Str("v".into())), "{body:?}");
    }

    #[test]
    fn an_array_carried_across_a_conditional_becomes_one_expression() {
        // `"a" + (c ? "!" : "") ` の IL: 配列を dup したまま分かれ道を通り、両方の枝が同じ配列と違う値を渡す形。
        // t0 = new string[2]; t0[0] = "a"; if (!c) { s0 = t0; s1 = t0; s3 = ""; } else { s0 = t0; s1 = t0; s3 = "!"; }
        // s1[1] = s3; x = Concat(s0);
        let assign = |to: Expr, value: Expr| Stmt::new(StmtKind::Assign(to, value), None);
        let t0 = || Expr::Temp(0xF000_0000);
        let element = |array: Expr, index: i64| Expr::Element { array: Box::new(array), index: Box::new(Expr::Int(index)) };
        let branch = |last: &str| vec![assign(Expr::Temp(0x370), t0()), assign(Expr::Temp(0x371), t0()), assign(Expr::Temp(0x373), Expr::Str(last.into()))];
        let mut body = vec![
            s(StmtKind::Assign(t0(), Expr::NewArray { element: 0x0100_0001, size: Box::new(Expr::Int(2)) }), 0x0),
            assign(element(t0(), 0), Expr::Str("a".into())),
            both(Expr::Not(Box::new(Expr::Local(1))), branch(""), branch("!")),
            assign(element(Expr::Temp(0x371), 1), Expr::Temp(0x373)),
            s(StmtKind::Assign(Expr::Local(0), call(0x0A00_0001, vec![Expr::Temp(0x370)])), 0x30),
        ];
        copy_temps(&mut body);
        uniform_temps(&mut body);
        for_each_block(&mut body, &mut |block| conditional(block));
        let ternary = Expr::Conditional(Box::new(Expr::Local(1)), Box::new(Expr::Str("!".into())), Box::new(Expr::Str("".into())));
        assert!(body.iter().any(|stmt| stmt.kind == StmtKind::Assign(element(t0(), 1), ternary.clone())), "{body:#?}");
        assert!(body.iter().all(|stmt| !matches!(&stmt.kind, StmtKind::If { .. })), "the branch is gone: {body:#?}");
        assert!(matches!(&body.last().unwrap().kind, StmtKind::Assign(Expr::Local(0), Expr::Call { args, .. }) if args == &vec![t0()]));
    }

    #[test]
    fn a_temp_used_once_in_the_next_statement_is_inlined_and_keeps_both_points() {
        // t1 = F(); return t1;  →  return F();
        let mut body = vec![s(StmtKind::Assign(Expr::Temp(1), call(0x0A00_0001, vec![])), 0x0), s(StmtKind::Return(Some(Expr::Temp(1))), 0x6)];
        inline_temps(&mut body);
        assert_eq!(body.len(), 1);
        assert_eq!(body[0].kind, StmtKind::Return(Some(call(0x0A00_0001, vec![]))));
        assert_eq!(body[0].il, Some(0x0));
        assert_eq!(body[0].merged, vec![0x6]);
    }

    #[test]
    fn a_temp_is_not_inlined_past_an_earlier_side_effect() {
        // t1 = F();  x = G(t2) + t1;  (G は t1 より先に評価されるので、入れると順番が変わる)
        let sum = Expr::Binary(BinOp::Add, Box::new(call(0x0A00_0002, vec![])), Box::new(Expr::Temp(1)));
        let mut body = vec![s(StmtKind::Assign(Expr::Temp(1), call(0x0A00_0001, vec![])), 0x0), s(StmtKind::Assign(Expr::Local(0), sum), 0x6)];
        inline_temps(&mut body);
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn a_local_is_declared_at_its_first_assignment() {
        // num = 1; F(num);  →  int num = 1; F(num);
        let mut body = vec![s(StmtKind::Assign(Expr::Local(0), Expr::Int(1)), 0x0), s(StmtKind::Expr(call(0x0A00_0001, vec![Expr::Local(0)])), 0x2)];
        let mut declared = HashSet::new();
        declare_at_first_assignment(&mut body, 1, &mut declared);
        assert_eq!(body[0].kind, StmtKind::Declare { local: 0, value: Expr::Int(1) });
        assert!(declared.contains(&0));
    }

    #[test]
    fn a_local_read_before_it_is_assigned_is_not_declared_there() {
        // F(num); num = 1;  (最初が読み: 宣言は先頭に残す)
        let mut body = vec![s(StmtKind::Expr(call(0x0A00_0001, vec![Expr::Local(0)])), 0x0), s(StmtKind::Assign(Expr::Local(0), Expr::Int(1)), 0x2)];
        let mut declared = HashSet::new();
        declare_at_first_assignment(&mut body, 1, &mut declared);
        assert!(declared.is_empty());
        assert!(matches!(body[1].kind, StmtKind::Assign(..)));
    }

    #[test]
    fn a_local_used_in_two_blocks_is_declared_in_neither() {
        // if (c) { num = 1; } F(num);
        let mut body = vec![both(Expr::Local(1), vec![s(StmtKind::Assign(Expr::Local(0), Expr::Int(1)), 0x2)], vec![]), s(StmtKind::Expr(call(0x0A00_0001, vec![Expr::Local(0)])), 0x8)];
        let mut declared = HashSet::new();
        declare_at_first_assignment(&mut body, 2, &mut declared);
        assert!(!declared.contains(&0));
    }

    #[test]
    fn local_counts_include_the_variables_of_foreach_and_declarations() {
        let body = vec![
            s(StmtKind::Declare { local: 1, value: Expr::Local(0) }, 0x0),
            s(StmtKind::Foreach { local: 2, collection: Expr::Local(1), body: vec![s(StmtKind::Expr(call(0x0A00_0001, vec![Expr::Local(2), Expr::Local(2)])), 0x4)] }, 0x2),
        ];
        assert_eq!(local_counts(&body, 3), vec![1, 2, 3]);
        assert_eq!(occurrences(&body, &Expr::Local(2)), 3);
    }
}