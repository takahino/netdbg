//! 段階 3: 書き方の復元。組み直した文の木（`structure.rs` のあと）を、C# の書き方に近づける。
//!
//! 各変換は独立した関数で、`apply` が順に当てる。どれも意味を変えない条件を確かめてから当てる。消した文の系列点は、
//! まとめた先の文に残す（`Stmt::absorb`）ので、ブレークポイントとステップの位置は失われない。

mod blocks;
mod expressions;
mod flow;
mod temps;

use super::ast::{visit, visit_expressions, visit_expressions_mut, BinOp, Case, CmpOp, Expr, Label, Part, Section, Stmt, StmtKind};
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
    /// ローカル変数の型。このアセンブリで `IDisposable` だと分かるかを `using` の判定に使う。
    pub local_types: &'a [Ty],
}

pub fn apply(context: &Context, body: &mut Vec<Stmt>) -> Applied {
    let mut applied = Applied::default();
    // 式の形（`?.`、`??`、三項、一時変数）を先に戻す。流れの形を変える変換（共有の return など）のあとでは、形が崩れる。
    copy_temps(body);
    uniform_temps(body);
    receiver_temps(body);
    for_each_block(body, &mut |block| empty_then(block));
    // `&&` `||` `?:` `??` は、分岐と goto のまま残る。`?.` の文と return も、ここで式に戻す。
    short_circuit(body);
    return_null_conditionals(body);
    for_each_block(body, &mut |block| statement_null_conditional(block));
    for_each_block(body, &mut |block| null_conditional(block));
    for_each_block(body, &mut |block| coalesce(block));
    for_each_block(body, &mut |block| delegate_caches(context, block));
    remove_dead_simple_temps(body);
    for_each_block(body, &mut |block| coalesce_assign(context, block));
    for_each_block(body, &mut |block| conditional(block));
    // `t = X; … t == null ? 既定 : t.M()` を `X?.M()`（値型なら `?? 既定`）にする。三項にしたあと、一時変数をしまう前。
    for_each_block(body, &mut |block| null_conditional_values(block));
    // 残った `x == null ? 既定 : x.M()`（x はローカル変数）を `x?.M()` にする。
    lift_simple_null_conditionals(body);
    inline_temps(body);
    for_each_block(body, &mut |block| initializers(context, block));
    // 初期化子にまとめた値（`s = new List<int> { … }`）を、一度だけ使う所へ入れる。
    inline_temps(body);
    shared_returns(body);
    remove_dead_code(body);
    fallthrough_gotos(body, &[]);
    // 少ない case の switch は、IL では比較の並びになる（`if (v != 1) { if (v == 2) goto L; goto D; }`）。
    comparison_switches(body);
    switch_sections(body);
    switch_values(body);
    remove_unused_labels(body);
    // ラベルが消えて、届かなくなった文（文字列の switch の、goto で飛んでいた case の本体の残り）。
    remove_dead_code(body);
    return_values(body);
    // return を共有していた分岐は、ここで `if (x != null) { t = x.P; } else { t = 既定; return t; } return t;` になる。
    return_null_conditionals(body);
    for_each_block(body, &mut |block| statement_null_conditional(block));
    // 配列の foreach は、GetEnumerator ではなく `for (i = 0; i < a.Length; i++)` になる。enumerator の形より先に戻す。
    array_foreach(body, &mut applied.declared);
    foreach_loops(context, body, &mut applied.declared);
    using_blocks(context, body, &mut applied.declared);
    lock_blocks(context, body, &mut applied.declared);
    for_each_block(body, &mut |block| array_elements(block));
    // 配列の初期化子にまとめた一時変数（`t0 = new string[] { … }`）を、一度だけ使う所（`string.Concat(t0)`）へ入れる。
    inline_temps(body);
    // 一時変数をしまったあとで、同じ整数への比較が並ぶ。ここでも switch にする。
    comparison_switches(body);
    remove_unused_labels(body);
    remove_dead_code(body);
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
    // 拡張メソッドは静的呼び出しのまま残る（`T.M(x)`）。`x != null ? T.M(x) : 既定` は `x?.M() ?? 既定`。
    extension_null_conditionals(context, body);
    // 一時変数をしまったあとに残る `if (x == null) { T? t = default; dest = t; } else { dest = x.P; }`。
    fold_member_assigns(context, body);
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
    fn a_null_test_on_a_temp_becomes_a_null_conditional() {
        let call = Expr::Call { method: 1, target: Some(Box::new(Expr::Temp(1))), args: vec![], virtual_call: false };
        let mut body = vec![
            s(StmtKind::Assign(Expr::Temp(1), Expr::Arg(1)), 1),
            s(StmtKind::Return(Some(Expr::Conditional(Box::new(Expr::Compare(CmpOp::Eq, Box::new(Expr::Temp(1)), Box::new(Expr::Null))), Box::new(Expr::Null), Box::new(call)))), 2),
        ];
        for_each_block(&mut body, &mut |block| null_conditional_values(block));
        assert_eq!(body.len(), 1);
        assert!(matches!(&body[0].kind, StmtKind::Return(Some(Expr::Call { target: Some(target), .. })) if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == Expr::Arg(1))));
    }

    #[test]
    fn a_null_test_with_a_default_becomes_a_coalesce() {
        let property = Expr::Property { method: 2, target: Some(Box::new(Expr::Temp(1))), args: vec![] };
        let mut body = vec![
            s(StmtKind::Assign(Expr::Temp(1), Expr::Arg(1)), 1),
            s(StmtKind::Assign(Expr::Local(0), Expr::Conditional(Box::new(Expr::Compare(CmpOp::Eq, Box::new(Expr::Temp(1)), Box::new(Expr::Null))), Box::new(Expr::Int(0)), Box::new(property))), 2),
        ];
        for_each_block(&mut body, &mut |block| null_conditional_values(block));
        assert!(matches!(&body[0].kind, StmtKind::Assign(Expr::Local(0), Expr::Coalesce(left, right)) if matches!(left.as_ref(), Expr::Property { target: Some(target), .. } if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == Expr::Arg(1))) && **right == Expr::Int(0)));
    }

    #[test]
    fn comparisons_of_one_value_become_a_switch() {
        let v = Expr::Local(0);
        let eq = |case: i64, label: u32| {
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(case))), then: vec![Stmt::new(StmtKind::Goto(label), None)], otherwise: vec![] }, None)
        };
        let mut body = vec![
            eq(1, 10),
            eq(2, 20),
            Stmt::new(StmtKind::Goto(30), None),
            Stmt::new(StmtKind::Label(10), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("c".into()))), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections.len(), 3);
        assert!(sections[2].default);
        assert_eq!(sections[0].cases, vec![Case::Int(1)]);
        assert_eq!(sections[1].cases, vec![Case::Int(2)]);
    }

    #[test]
    fn an_inequality_around_comparisons_becomes_a_switch() {
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(2))), then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(1))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        body.push(Stmt::new(StmtKind::Label(99), None));
        body.push(Stmt::new(StmtKind::Return(Some(Expr::Int(0))), None));
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(1)]);
        assert_eq!(sections[1].cases, vec![Case::Int(2)]);
        assert!(sections[2].default);
        assert!(matches!(body[1].kind, StmtKind::Label(99)), "うしろのラベルまで飲み込んだ: {body:#?}");
    }

    #[test]
    fn an_unsigned_span_with_a_statement_arm_becomes_a_switch() {
        // if (v != 1) { if ((uint)(v - 2) <= 1) goto 20; return; } assign; return; 20: assign;
        let v = Expr::Local(0);
        let shifted = Expr::Binary(BinOp::Sub, Box::new(Expr::Convert { to: Ty::I4, checked: false, value: Box::new(v.clone()) }), Box::new(Expr::Int(2)));
        let span = Expr::Compare(CmpOp::Le, Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(shifted) }), Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(Expr::Int(1)) }));
        let inner = vec![
            Stmt::new(StmtKind::If { condition: span, then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(None), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(1))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(1)), None),
            Stmt::new(StmtKind::Return(None), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(2)), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(1)]);
        assert_eq!(sections[1].cases, vec![Case::Int(2), Case::Int(3)]);
        assert!(sections[2].default);
        assert!(matches!(sections[1].body.last().map(|stmt| &stmt.kind), Some(StmtKind::Break)));
        assert_eq!(body.len(), 1);
    }

    fn uint_span(value: Expr, low: i64, span: i64) -> Expr {
        let shifted = Expr::Binary(BinOp::Sub, Box::new(Expr::Convert { to: Ty::I4, checked: false, value: Box::new(value) }), Box::new(Expr::Int(low)));
        Expr::Compare(CmpOp::Le, Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(shifted) }), Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(Expr::Int(span)) }))
    }

    #[test]
    fn an_unsigned_span_that_returns_becomes_a_switch() {
        // if (!((uint)(v - 5) <= 7)) { if ((uint)(v - 13) <= 2) goto 20; goto 30; } return true; 20: return false; 30: throw;
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: uint_span(v.clone(), 13, 2), then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(uint_span(v, 5, 7))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(true))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(false))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, (5..=12).map(Case::Int).collect::<Vec<_>>());
        assert_eq!(sections[1].cases, (13..=15).map(Case::Int).collect::<Vec<_>>());
        assert!(sections[2].default);
    }

    #[test]
    fn an_unsigned_span_that_falls_into_a_label_becomes_a_switch() {
        // if (!((uint)(v - 7) <= 5)) { if ((uint)(v - 16) <= 1) goto 20; goto 30; } 20: return true; 30: return false;
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: uint_span(v.clone(), 16, 1), then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(uint_span(v, 7, 5))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(true))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(false))), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        let mut cases = (7..=12).map(Case::Int).collect::<Vec<_>>();
        cases.extend((16..=17).map(Case::Int));
        assert_eq!(sections[0].cases, cases);
        assert!(sections[1].default);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn a_char_split_becomes_a_switch() {
        // if (!(c > 'm')) { if (c == 'i') goto 1; goto 9; } if (c == 's') goto 2; goto 9; 1: a = 1; goto 9; 2: a = 2; 9:
        let c = Expr::Local(0);
        let low = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c.clone()), Box::new(Expr::Char(b'i' as u16))), then: vec![Stmt::new(StmtKind::Goto(1), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(Expr::Compare(CmpOp::Gt, Box::new(c.clone()), Box::new(Expr::Char(b'm' as u16))))), then: low, otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c), Box::new(Expr::Char(b's' as u16))), then: vec![Stmt::new(StmtKind::Goto(2), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(1), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(1)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(2), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(2)), None),
            Stmt::new(StmtKind::Label(9), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Char(b'i' as u16)]);
        assert_eq!(sections[1].cases, vec![Case::Char(b's' as u16)]);
        assert!(sections[2].default);
        assert!(matches!(sections[0].body.last().map(|stmt| &stmt.kind), Some(StmtKind::Break)));
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn a_char_split_keeps_the_loop_step_after_the_switch() {
        // 既定ラベルの先は num++ とループ先頭への goto。どの case もそこへ飛ぶので、増分は switch のあと。
        let c = Expr::Local(0);
        let low = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c.clone()), Box::new(Expr::Char(b'i' as u16))), then: vec![Stmt::new(StmtKind::Goto(1), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::Label(8), None),
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(Expr::Compare(CmpOp::Gt, Box::new(c.clone()), Box::new(Expr::Char(b'm' as u16))))), then: low, otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c), Box::new(Expr::Char(b's' as u16))), then: vec![Stmt::new(StmtKind::Goto(2), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(1), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(1)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(2), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(2)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(9), None),
            Stmt::new(StmtKind::Assign(Expr::Local(2), Expr::Binary(BinOp::Add, Box::new(Expr::Local(2)), Box::new(Expr::Int(1)))), None),
            Stmt::new(StmtKind::Goto(8), None),
        ];
        comparison_switches(&mut body);
        assert!(matches!(body[0].kind, StmtKind::Label(8)));
        let StmtKind::SwitchBlock { sections, .. } = &body[1].kind else { panic!("switch にならない: {body:#?}") };
        let default_section = sections.iter().find(|section| section.default).expect("default");
        assert_eq!(default_section.body.len(), 1);
        assert!(matches!(default_section.body[0].kind, StmtKind::Break));
        assert!(sections.iter().all(|section| !section.body.iter().any(|stmt| matches!(stmt.kind, StmtKind::Assign(Expr::Local(2), _)))));
        assert!(matches!(body[2].kind, StmtKind::Assign(Expr::Local(2), _)));
        assert!(matches!(body[3].kind, StmtKind::Goto(8)));
    }

    #[test]
    fn a_char_split_with_an_inline_last_case_becomes_a_switch() {
        // if (!(c > 'm')) { if (c == 'i') goto 1; goto 9; } if (c != 's') { if (c == 'x') goto 2; goto 9; } a = 3; goto 9; …
        let c = Expr::Local(0);
        let low = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c.clone()), Box::new(Expr::Char(b'i' as u16))), then: vec![Stmt::new(StmtKind::Goto(1), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
        ];
        let high = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(c.clone()), Box::new(Expr::Char(b'x' as u16))), then: vec![Stmt::new(StmtKind::Goto(2), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(Expr::Compare(CmpOp::Gt, Box::new(c.clone()), Box::new(Expr::Char(b'm' as u16))))), then: low, otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(c), Box::new(Expr::Char(b's' as u16))), then: high, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(3)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(1), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(1)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(2), None),
            Stmt::new(StmtKind::Assign(Expr::Local(1), Expr::Int(2)), None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(9), None),
            Stmt::new(StmtKind::Return(Some(Expr::Int(0))), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert!(sections.iter().any(|section| section.cases == vec![Case::Char(b's' as u16)] && matches!(section.body.first().map(|stmt| &stmt.kind), Some(StmtKind::Assign(_, Expr::Int(3))))));
        assert!(sections.iter().any(|section| section.cases.contains(&Case::Char(b'i' as u16))));
        assert!(sections.iter().any(|section| section.cases.contains(&Case::Char(b'x' as u16))));
        assert!(sections.iter().any(|section| section.default && section.body.len() == 1 && matches!(section.body[0].kind, StmtKind::Break)));
        assert!(matches!(body[1].kind, StmtKind::Return(_)));
    }

    #[test]
    fn a_large_unsigned_split_becomes_a_switch() {
        // if (!(num > 3000000000u)) { if (num == 2223801888u) goto 1; goto 9; } if (num == 10) goto 2; goto 9;
        let num = Expr::Local(0);
        let wide = |text: &str| Expr::Literal(text.into());
        let low = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(num.clone()), Box::new(wide("2223801888u"))), then: vec![Stmt::new(StmtKind::Goto(1), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(Expr::Compare(CmpOp::Gt, Box::new(num.clone()), Box::new(wide("3000000000u"))))), then: low, otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(num), Box::new(Expr::Int(10))), then: vec![Stmt::new(StmtKind::Goto(2), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(9), None),
            Stmt::new(StmtKind::Label(1), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(2), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(9), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("c".into()))), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert!(sections.iter().any(|section| section.cases.contains(&Case::Int(2_223_801_888))));
        assert!(sections.iter().any(|section| section.cases.contains(&Case::Int(10))));
        assert!(sections.iter().any(|section| section.default));
    }

    #[test]
    fn a_hash_switch_keeps_the_matched_arm_outside() {
        // ハッシュで飛んだ先が `if (text == "a") goto 20; goto 9;` で、20 の本体は default より前にある。
        let num = Expr::Local(0);
        let text = Expr::Local(1);
        let arm = |hash: i64, label: u32| Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(num.clone()), Box::new(Expr::Int(hash))), then: vec![Stmt::new(StmtKind::Goto(label), None)], otherwise: vec![] }, None);
        let check = |label: u32, word: &str, work: u32| {
            vec![
                Stmt::new(StmtKind::Label(label), None),
                Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(text.clone()), Box::new(Expr::Str(word.into()))), then: vec![Stmt::new(StmtKind::Goto(work), None)], otherwise: vec![] }, None),
                Stmt::new(StmtKind::Goto(9), None),
            ]
        };
        let mut body = vec![arm(1, 11), arm(2, 12), Stmt::new(StmtKind::Goto(9), None)];
        body.extend(check(11, "a", 21));
        body.extend(check(12, "b", 22));
        body.push(Stmt::new(StmtKind::Label(21), None));
        body.push(Stmt::new(StmtKind::Assign(Expr::Local(3), Expr::Str("a".into())), None));
        body.push(Stmt::new(StmtKind::Goto(9), None));
        body.push(Stmt::new(StmtKind::Label(22), None));
        body.push(Stmt::new(StmtKind::Assign(Expr::Local(3), Expr::Str("b".into())), None));
        body.push(Stmt::new(StmtKind::Goto(9), None));
        body.push(Stmt::new(StmtKind::Label(9), None));
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert!(sections.iter().any(|section| section.cases.contains(&Case::Int(1)) && section.body.iter().any(|stmt| matches!(stmt.kind, StmtKind::Goto(21) | StmtKind::If { .. }))));
        assert!(body.iter().any(|stmt| matches!(&stmt.kind, StmtKind::Assign(_, Expr::Str(text)) if text == "a")));
        assert!(sections.iter().all(|section| !section.body.iter().any(|stmt| matches!(&stmt.kind, StmtKind::Assign(_, Expr::Str(text)) if text == "a"))));
    }

    #[test]
    fn an_unsigned_span_becomes_one_case_group() {
        // if (v != 1) { if ((uint)(v - 2) <= 1) goto 20; goto 30; } return a; 20: return b; 30: throw;
        let v = Expr::Local(0);
        let shifted = Expr::Binary(BinOp::Sub, Box::new(Expr::Convert { to: Ty::I4, checked: false, value: Box::new(v.clone()) }), Box::new(Expr::Int(2)));
        let span = Expr::Compare(CmpOp::Le, Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(shifted) }), Box::new(Expr::Convert { to: Ty::U4, checked: false, value: Box::new(Expr::Int(1)) }));
        let inner = vec![
            Stmt::new(StmtKind::If { condition: span, then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(1))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(1)]);
        assert_eq!(sections[1].cases, vec![Case::Int(2), Case::Int(3)]);
        assert!(sections[2].default);
    }

    #[test]
    fn an_empty_case_joins_the_following_case() {
        // if (v != 0) { if (v == 4) goto 20; if (v == 8) goto 10; goto 30; } 10: return a; 20: return b; 30: throw;
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(4))), then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(8))), then: vec![Stmt::new(StmtKind::Goto(10), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(0))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Label(10), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(8), Case::Int(0)]);
        assert_eq!(sections[1].cases, vec![Case::Int(4)]);
        assert!(sections[2].default);
    }

    #[test]
    fn a_goto_into_a_real_default_stays_a_goto() {
        // if (v != 0) { if (v == 4) goto 20; if (v == 8) goto 10; goto 30; } 10: goto 30; 20: return b; 30: throw;
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(4))), then: vec![Stmt::new(StmtKind::Goto(20), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(8))), then: vec![Stmt::new(StmtKind::Goto(10), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Goto(30), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(0))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Label(10), None),
            Stmt::new(StmtKind::Goto(30), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
            Stmt::new(StmtKind::Label(30), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert!(matches!(sections[0].body.first().map(|stmt| &stmt.kind), Some(StmtKind::Goto(30))));
        assert!(matches!(sections.last().unwrap().body.first().map(|stmt| &stmt.kind), Some(StmtKind::Label(30))));
    }

    #[test]
    fn nested_inequalities_become_a_switch() {
        // if (v != 1) { if (v != 2) { goto 9; } return "b"; } return "a"; 9: throw;
        let v = Expr::Local(0);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v.clone()), Box::new(Expr::Int(2))), then: vec![Stmt::new(StmtKind::Goto(9), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("b".into()))), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v), Box::new(Expr::Int(1))), then: inner, otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Str("a".into()))), None),
            Stmt::new(StmtKind::Label(9), None),
            Stmt::new(StmtKind::Throw(None), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(2)]);
        assert_eq!(sections[1].cases, vec![Case::Int(1)]);
        assert!(sections[2].default);
        assert!(matches!(sections[2].body.first().map(|stmt| &stmt.kind), Some(StmtKind::Throw(None))));
    }

    #[test]
    fn a_string_inequality_with_a_return_default_becomes_a_switch() {
        // if (v != "B") { if (v == "KB") goto 20; return false; } return true; 20: return true;
        let v = Expr::Local(0);
        let arm = |text: &str, label: u32| {
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Str(text.into()))), then: vec![Stmt::new(StmtKind::Goto(label), None)], otherwise: vec![] }, None)
        };
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Ne, Box::new(v.clone()), Box::new(Expr::Str("B".into()))), then: vec![arm("KB", 20), Stmt::new(StmtKind::Return(Some(Expr::Bool(false))), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(true))), None),
            Stmt::new(StmtKind::Label(20), None),
            Stmt::new(StmtKind::Return(Some(Expr::Bool(true))), None),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Str("B".into())]);
        assert_eq!(sections[1].cases, vec![Case::Str("KB".into())]);
        assert!(sections[2].default);
    }

    #[test]
    fn returned_equalities_become_a_switch() {
        let v = Expr::Local(0);
        let arm = |case: i64, text: &str| {
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(v.clone()), Box::new(Expr::Int(case))), then: vec![Stmt::new(StmtKind::Return(Some(Expr::Str(text.into()))), None)], otherwise: vec![] }, None)
        };
        let mut body = vec![arm(0, "_Singular"), arm(1, "_Paucal"), Stmt::new(StmtKind::Return(Some(Expr::Str(String::new()))), None)];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(0)]);
        assert_eq!(sections[1].cases, vec![Case::Int(1)]);
        assert!(sections[2].default);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn a_narrow_range_becomes_one_switch() {
        // if (n == 2) return dual; if (!(n < 3)) { if (n > 10) return key; return plural; } return key;
        let n = Expr::Local(0);
        let ret = |text: &str| Stmt::new(StmtKind::Return(Some(Expr::Str(text.into()))), None);
        let inner = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Gt, Box::new(n.clone()), Box::new(Expr::Int(10))), then: vec![ret("key")], otherwise: vec![] }, None),
            ret("plural"),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(n.clone()), Box::new(Expr::Int(2))), then: vec![ret("dual")], otherwise: vec![] }, None),
            Stmt::new(StmtKind::If { condition: Expr::Not(Box::new(Expr::Compare(CmpOp::Lt, Box::new(n), Box::new(Expr::Int(3))))), then: inner, otherwise: vec![] }, None),
            ret("key"),
        ];
        comparison_switches(&mut body);
        let StmtKind::SwitchBlock { sections, .. } = &body[0].kind else { panic!("switch にならない: {body:#?}") };
        assert_eq!(sections[0].cases, vec![Case::Int(2)]);
        assert_eq!(sections[1].cases, (3..=10).map(Case::Int).collect::<Vec<_>>());
        assert!(sections[2].default);
    }

    #[test]
    fn a_null_member_chain_becomes_one_conditional() {
        // if (s == null) { r = null; } else { t = s.Name; if (t == null) { r = null; goto 1; } r = t.Trim(); } 1: return r;
        let section = Expr::Local(0);
        let result = Expr::Temp(1);
        let name = Expr::Temp(2);
        let trim = Expr::Call { method: 2, target: Some(Box::new(name.clone())), args: vec![], virtual_call: false };
        let property = Expr::Property { method: 1, target: Some(Box::new(section.clone())), args: vec![] };
        let otherwise = vec![
            Stmt::new(StmtKind::Assign(name.clone(), property), None),
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(name.clone()), Box::new(Expr::Null)), then: vec![Stmt::new(StmtKind::Assign(result.clone(), Expr::Null), None), Stmt::new(StmtKind::Goto(1), None)], otherwise: vec![] }, None),
            Stmt::new(StmtKind::Assign(result.clone(), trim), None),
        ];
        let mut body = vec![
            Stmt::new(StmtKind::If { condition: Expr::Compare(CmpOp::Eq, Box::new(section), Box::new(Expr::Null)), then: vec![Stmt::new(StmtKind::Assign(result.clone(), Expr::Null), None)], otherwise }, None),
            Stmt::new(StmtKind::Label(1), None),
            Stmt::new(StmtKind::Return(Some(result)), None),
        ];
        return_null_conditionals(&mut body);
        inline_temps(&mut body);
        return_null_conditionals(&mut body);
        let text = format!("{body:#?}");
        assert!(text.matches("NullConditional").count() >= 2, "連鎖にならない: {body:#?}");
    }

    #[test]
    fn a_null_test_on_a_local_becomes_a_null_conditional() {
        let local = Expr::Local(1);
        let call = Expr::Call { method: 1, target: Some(Box::new(local.clone())), args: vec![], virtual_call: false };
        let mut body = vec![Stmt::new(StmtKind::Return(Some(Expr::Conditional(Box::new(Expr::Compare(CmpOp::Eq, Box::new(local.clone()), Box::new(Expr::Null))), Box::new(Expr::Null), Box::new(call)))), None)];
        lift_simple_null_conditionals(&mut body);
        assert!(matches!(&body[0].kind, StmtKind::Return(Some(Expr::Call { target: Some(target), .. })) if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == local)));
    }

    #[test]
    fn a_null_test_on_a_local_with_a_default_becomes_a_coalesce() {
        let local = Expr::Local(1);
        let property = Expr::Property { method: 2, target: Some(Box::new(local.clone())), args: vec![] };
        let mut body = vec![Stmt::new(StmtKind::Return(Some(Expr::Conditional(Box::new(Expr::Compare(CmpOp::Eq, Box::new(local), Box::new(Expr::Null))), Box::new(Expr::Str("Key".into())), Box::new(property)))), None)];
        lift_simple_null_conditionals(&mut body);
        assert!(matches!(&body[0].kind, StmtKind::Return(Some(Expr::Coalesce(left, right))) if matches!(left.as_ref(), Expr::Property { target: Some(target), .. } if matches!(target.as_ref(), Expr::NullConditional(_))) && **right == Expr::Str("Key".into())));
    }

    #[test]
    fn a_call_with_a_null_argument_stays_a_conditional() {
        let local = Expr::Local(1);
        let call = Expr::Call { method: 1, target: None, args: vec![local.clone()], virtual_call: false };
        let original = Expr::Conditional(Box::new(Expr::Compare(CmpOp::Eq, Box::new(local), Box::new(Expr::Null))), Box::new(Expr::Int(0)), Box::new(call));
        let mut body = vec![Stmt::new(StmtKind::Return(Some(original.clone())), None)];
        lift_simple_null_conditionals(&mut body);
        assert_eq!(body[0].kind, StmtKind::Return(Some(original)));
    }

    #[test]
    fn an_index_loop_over_an_array_becomes_foreach() {
        let index = Expr::Local(1);
        let array = Expr::Local(2);
        let item = Expr::Local(3);
        let mut body = vec![
            Stmt::new(StmtKind::Assign(index.clone(), Expr::Int(0)), None),
            Stmt::new(StmtKind::While {
                condition: Expr::Compare(CmpOp::Lt, Box::new(index.clone()), Box::new(Expr::Convert { to: Ty::I4, checked: false, value: Box::new(Expr::Length(Box::new(array.clone()))) })),
                body: vec![
                    Stmt::new(StmtKind::Assign(item.clone(), Expr::Element { array: Box::new(array.clone()), index: Box::new(index.clone()) }), None),
                    Stmt::new(StmtKind::Expr(Expr::Call { method: 1, target: None, args: vec![item.clone()], virtual_call: false }), None),
                    Stmt::new(StmtKind::Assign(index.clone(), Expr::Binary(BinOp::Add, Box::new(index), Box::new(Expr::Int(1)))), None),
                ],
            }, None),
        ];
        let mut declared = HashSet::new();
        array_foreach(&mut body, &mut declared);
        assert!(matches!(&body[0].kind, StmtKind::Foreach { local: 3, collection, body } if collection == &array && body.len() == 1), "{body:#?}");
        assert_eq!(body.len(), 1);
        assert!(declared.contains(&3));
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

    fn assign_of(body: &[Stmt]) -> &Expr {
        let StmtKind::Assign(_, value) = &body[0].kind else { panic!("代入ではない") };
        value
    }

    #[test]
    fn guarded_gotos_become_and_also() {
        // if (num == 0) { if (!allow) goto F; t = depth == 1; goto E; } F: t = false; E:
        let (num, allow, depth, t) = (Expr::Local(0), Expr::Local(1), Expr::Local(2), Expr::Local(3));
        let mut body = vec![
            both(
                Expr::Compare(CmpOp::Eq, num.clone().boxed(), Expr::Int(0).boxed()),
                vec![
                    both(Expr::Not(allow.clone().boxed()), vec![s(StmtKind::Goto(0x4b), 0x10)], vec![]),
                    s(StmtKind::Assign(t.clone(), Expr::Compare(CmpOp::Eq, depth.clone().boxed(), Expr::Int(1).boxed())), 0x20),
                    s(StmtKind::Goto(0x4c), 0x30),
                ],
                vec![],
            ),
            s(StmtKind::Label(0x4b), 0x40),
            s(StmtKind::Assign(t.clone(), Expr::Bool(false)), 0x48),
            s(StmtKind::Label(0x4c), 0x4c),
        ];
        short_circuit(&mut body);
        assert_eq!(
            assign_of(&body),
            &Expr::AndAlso(
                Expr::AndAlso(Expr::Compare(CmpOp::Eq, num.boxed(), Expr::Int(0).boxed()).boxed(), allow.boxed()).boxed(),
                Expr::Compare(CmpOp::Eq, depth.boxed(), Expr::Int(1).boxed()).boxed(),
            )
        );
    }

    #[test]
    fn guarded_gotos_to_true_become_or_else() {
        // if (injection == 0) { if (quote != null) goto T; t = space; goto E; } T: t = true; E:
        let (injection, quote, space, t) = (Expr::Local(0), Expr::Local(1), Expr::Local(2), Expr::Local(3));
        let mut body = vec![
            both(
                Expr::Compare(CmpOp::Eq, injection.clone().boxed(), Expr::Int(0).boxed()),
                vec![
                    both(Expr::Compare(CmpOp::Ne, quote.clone().boxed(), Expr::Null.boxed()), vec![s(StmtKind::Goto(0x10), 0x8)], vec![]),
                    s(StmtKind::Assign(t.clone(), space.clone()), 0x12),
                    s(StmtKind::Goto(0x20), 0x16),
                ],
                vec![],
            ),
            s(StmtKind::Label(0x10), 0x18),
            s(StmtKind::Assign(t, Expr::Bool(true)), 0x1a),
            s(StmtKind::Label(0x20), 0x20),
        ];
        short_circuit(&mut body);
        assert_eq!(
            assign_of(&body),
            &Expr::OrElse(
                Expr::OrElse(
                    Expr::Compare(CmpOp::Ne, injection.boxed(), Expr::Int(0).boxed()).boxed(),
                    Expr::Compare(CmpOp::Ne, quote.boxed(), Expr::Null.boxed()).boxed(),
                )
                .boxed(),
                space.boxed(),
            )
        );
    }

    #[test]
    fn an_if_else_assign_of_true_becomes_or_else() {
        let (flag, other, t) = (Expr::Local(0), Expr::Local(1), Expr::Local(2));
        let mut body = vec![both(
            flag.clone(),
            vec![s(StmtKind::Assign(t.clone(), Expr::Bool(true)), 0x2)],
            vec![s(StmtKind::Assign(t, other.clone()), 0x6)],
        )];
        short_circuit(&mut body);
        assert_eq!(assign_of(&body), &Expr::OrElse(flag.boxed(), other.boxed()));
    }

    #[test]
    fn a_goto_out_of_a_catching_try_becomes_a_return() {
        let value = Expr::Local(0);
        let mut body = vec![
            s(
                StmtKind::Try {
                    body: vec![s(StmtKind::Goto(1), 0x2)],
                    catches: vec![super::super::ast::Catch { ty: 1, variable: None, filter: None, body: vec![s(StmtKind::Throw(None), 0x6)] }],
                    finally: None,
                    fault: None,
                },
                0x1,
            ),
            s(StmtKind::Label(1), 0x10),
            s(StmtKind::Return(Some(value.clone())), 0x11),
        ];
        shared_returns(&mut body);
        let StmtKind::Try { body: tried, .. } = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(&tried[0].kind, StmtKind::Return(Some(returned)) if *returned == value));
    }

    #[test]
    fn a_null_arm_of_a_member_becomes_a_null_conditional() {
        let (value, dest) = (Expr::Local(0), Expr::Local(1));
        let property = Expr::Property { method: 2, target: Some(value.clone().boxed()), args: vec![] };
        let mut body = vec![both(
            Expr::Compare(CmpOp::Eq, value.clone().boxed(), Expr::Null.boxed()),
            vec![
                s(StmtKind::Assign(Expr::Local(3), Expr::Default(Ty::I4)), 0x2),
                s(StmtKind::Assign(dest.clone(), Expr::Local(3)), 0x4),
            ],
            vec![s(StmtKind::Assign(dest, property), 0x8)],
        )];
        short_circuit(&mut body);
        let StmtKind::Assign(_, Expr::Property { target: Some(target), .. }) = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == value));
    }

    #[test]
    fn a_null_arm_becomes_coalesce() {
        let (value, fallback, t) = (Expr::Local(0), Expr::Local(1), Expr::Local(2));
        let mut body = vec![both(
            Expr::Compare(CmpOp::Ne, value.clone().boxed(), Expr::Null.boxed()),
            vec![s(StmtKind::Assign(t.clone(), value.clone()), 0x2)],
            vec![s(StmtKind::Assign(t, fallback.clone()), 0x6)],
        )];
        short_circuit(&mut body);
        assert_eq!(assign_of(&body), &Expr::Coalesce(value.boxed(), fallback.boxed()));
    }

    #[test]
    fn a_null_return_becomes_a_null_conditional() {
        let arg = Expr::Local(0);
        let call = Expr::Call { method: 0x0A00_0001, target: Some(arg.clone().boxed()), args: vec![], virtual_call: false };
        let mut body = vec![
            both(Expr::Compare(CmpOp::Eq, arg.clone().boxed(), Expr::Null.boxed()), vec![s(StmtKind::Return(Some(Expr::Bool(false))), 0x2)], vec![]),
            s(StmtKind::Return(Some(call.clone())), 0x8),
        ];
        return_null_conditionals(&mut body);
        let StmtKind::Return(Some(Expr::Coalesce(left, right))) = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(left.as_ref(), Expr::Call { target: Some(target), .. } if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == arg)));
        assert_eq!(right.as_ref(), &Expr::Bool(false));
    }

    #[test]
    fn an_assign_and_return_becomes_a_null_conditional() {
        let local = Expr::Local(0);
        let temp = Expr::Local(1);
        let prop = Expr::Property { method: 0x0A00_0003, target: Some(local.clone().boxed()), args: vec![] };
        let mut body = vec![
            both(Expr::Compare(CmpOp::Ne, local.clone().boxed(), Expr::Null.boxed()), vec![s(StmtKind::Assign(temp.clone(), prop), 0x2), s(StmtKind::Return(Some(temp.clone())), 0x4)], vec![]),
            s(StmtKind::Assign(temp.clone(), Expr::Int(0)), 0x6),
            s(StmtKind::Return(Some(temp)), 0x8),
        ];
        return_null_conditionals(&mut body);
        let StmtKind::Return(Some(Expr::Coalesce(left, right))) = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(left.as_ref(), Expr::Property { target: Some(target), .. } if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == local)));
        assert_eq!(right.as_ref(), &Expr::Int(0));
    }

    #[test]
    fn a_null_conditional_dispose_still_names_the_local() {
        let local = Expr::Local(0);
        assert!(dispose_receiver(&local, &local));
        assert!(dispose_receiver(&Expr::NullConditional(local.clone().boxed()), &local));
        assert!(dispose_receiver(&Expr::Cast { ty: 0, value: Expr::NullConditional(local.clone().boxed()).boxed() }, &local));
        assert!(!dispose_receiver(&Expr::NullConditional(Expr::Local(1).boxed()), &local));
    }

    #[test]
    fn an_assigned_return_becomes_a_null_conditional() {
        // if (x != null) { t = x.Prop; } else { t = 0; return t; } return t;
        let (local, temp) = (Expr::Local(0), Expr::Local(1));
        let prop = Expr::Property { method: 0x0A00_0003, target: Some(local.clone().boxed()), args: vec![] };
        let mut body = vec![
            both(
                Expr::Compare(CmpOp::Ne, local.clone().boxed(), Expr::Null.boxed()),
                vec![s(StmtKind::Assign(temp.clone(), prop), 0x4)],
                vec![s(StmtKind::Assign(temp.clone(), Expr::Int(0)), 0x8), s(StmtKind::Return(Some(temp.clone())), 0xa)],
            ),
            s(StmtKind::Return(Some(temp.clone())), 0xc),
        ];
        return_null_conditionals(&mut body);
        let StmtKind::Return(Some(Expr::Coalesce(left, right))) = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(left.as_ref(), Expr::Property { target: Some(target), .. } if matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == local)));
        assert_eq!(right.as_ref(), &Expr::Int(0));
    }

    #[test]
    fn a_statement_null_check_becomes_a_null_conditional_call() {
        let local = Expr::Local(0);
        let call = Expr::Call { method: 0x0A00_0002, target: Some(local.clone().boxed()), args: vec![], virtual_call: true };
        let mut body = vec![both(Expr::Compare(CmpOp::Ne, local.clone().boxed(), Expr::Null.boxed()), vec![s(StmtKind::Expr(call), 0x4)], vec![])];
        statement_null_conditional(&mut body);
        let StmtKind::Expr(Expr::Call { target: Some(target), .. }) = &body[0].kind else { panic!("{:?}", body) };
        assert!(matches!(target.as_ref(), Expr::NullConditional(inner) if **inner == local));
    }
}