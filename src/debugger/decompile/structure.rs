//! 平らな形（ラベルと goto）を、if、while、do-while、break、continue に組み直す。
//!
//! 置き換えは、外から飛び込む goto の無い範囲だけで行う（意味を変えない）。C# のコンパイラが出す決まった形を順に
//! 当て、当てられなくなるまで繰り返す。残った goto が C# の規則（外側のブロックのラベルへだけ飛べる）を満たさなければ、
//! 呼び出し側はそのメソッドを IL の形で書く。

use super::ast::{visit, Expr, Label, Stmt, StmtKind};
use super::build::negate;
use std::collections::{HashMap, HashSet};

/// 本体を組み直す。C# の規則を満たせなければ `Err`。
pub fn structure(body: &mut Vec<Stmt>) -> Result<(), String> {
    // 条件の一時変数を先に戻しておくと、while の条件がそのまま式になる。
    inline_condition_temps(body);
    let mut counts = count_targets(body);
    // 当てられる形がある限り繰り返す（一つ当てると、外側に次の形ができることがある）。
    for _ in 0..64 {
        if !pass(body, &mut counts) {
            break;
        }
    }
    remove_unused_labels(body, &counts);
    inline_condition_temps(body);
    remove_dead_temps(body);
    check_gotos(body, &mut Vec::new())
}

/// goto と switch の飛び先ごとの数（入れ子の中も含む）。
fn count_targets(body: &[Stmt]) -> HashMap<Label, usize> {
    let mut counts = HashMap::new();
    visit(body, &mut |s| match &s.kind {
        StmtKind::Goto(label) => *counts.entry(*label).or_insert(0) += 1,
        StmtKind::Switch { targets, .. } => {
            for label in targets {
                *counts.entry(*label).or_insert(0) += 1;
            }
        }
        _ => {}
    });
    counts
}

/// 一回り。何か置き換えたら `true`。
fn pass(body: &mut Vec<Stmt>, counts: &mut HashMap<Label, usize>) -> bool {
    let mut changed = false;
    for stmt in body.iter_mut() {
        for inner in stmt.blocks_mut() {
            changed |= pass(inner, counts);
        }
    }
    let mut i = 0;
    while i < body.len() {
        if remove_goto_next(body, i, counts) || while_loop(body, i, counts) || do_while(body, i, counts) || if_then(body, i, counts) || if_else(body, i, counts) {
            changed = true;
            continue;
        }
        i += 1;
    }
    changed
}

/// 範囲の中で定義されたラベルが、範囲の外から飛ばれていないか。
fn closed(range: &[Stmt], counts: &HashMap<Label, usize>) -> bool {
    let inner = count_targets(range);
    let mut ok = true;
    visit(range, &mut |s| {
        if let StmtKind::Label(label) = s.kind {
            if counts.get(&label).copied().unwrap_or(0) != inner.get(&label).copied().unwrap_or(0) {
                ok = false;
            }
        }
    });
    ok
}

/// `i` の次の、ラベルでない文の位置。
fn label_position(body: &[Stmt], from: usize, label: Label) -> Option<usize> {
    (from..body.len()).find(|&j| body[j].kind == StmtKind::Label(label))
}

fn decrement(counts: &mut HashMap<Label, usize>, label: Label) {
    if let Some(count) = counts.get_mut(&label) {
        *count = count.saturating_sub(1);
    }
}

/// `goto L;` のすぐあと（ラベルだけを挟んで）に `L:` があれば、goto を消す。
fn remove_goto_next(body: &mut Vec<Stmt>, i: usize, counts: &mut HashMap<Label, usize>) -> bool {
    let StmtKind::Goto(label) = body[i].kind else { return false };
    let mut j = i + 1;
    while j < body.len() {
        match body[j].kind {
            StmtKind::Label(l) if l == label => {
                decrement(counts, label);
                body.remove(i);
                return true;
            }
            StmtKind::Label(_) => j += 1,
            _ => return false,
        }
    }
    false
}

/// `if (c) goto L; S…; L:` → `if (!c) { S… }`。
fn if_then(body: &mut Vec<Stmt>, i: usize, counts: &mut HashMap<Label, usize>) -> bool {
    let StmtKind::If { condition, then, otherwise } = &body[i].kind else { return false };
    let ([Stmt { kind: StmtKind::Goto(label), .. }], true) = (then.as_slice(), otherwise.is_empty()) else { return false };
    let label = *label;
    let Some(j) = label_position(body, i + 1, label) else { return false };
    if j == i + 1 || !closed(&body[i + 1..j], counts) {
        return false;
    }
    let condition = negate(condition.clone());
    let inner: Vec<Stmt> = body.drain(i + 1..j).collect();
    decrement(counts, label);
    body[i].kind = StmtKind::If { condition, then: inner, otherwise: Vec::new() };
    true
}

/// `if (c) { S1…; goto L; } S2…; L:` → `if (c) { S1… } else { S2… }`。
fn if_else(body: &mut Vec<Stmt>, i: usize, counts: &mut HashMap<Label, usize>) -> bool {
    let StmtKind::If { then, otherwise, .. } = &body[i].kind else { return false };
    let Some(Stmt { kind: StmtKind::Goto(label), .. }) = then.last() else { return false };
    if !otherwise.is_empty() {
        return false;
    }
    let label = *label;
    let Some(j) = label_position(body, i + 1, label) else { return false };
    if j == i + 1 || !closed(&body[i + 1..j], counts) {
        return false;
    }
    let inner: Vec<Stmt> = body.drain(i + 1..j).collect();
    decrement(counts, label);
    if let StmtKind::If { then, otherwise, .. } = &mut body[i].kind {
        then.pop();
        *otherwise = inner;
    }
    true
}

/// C# のコンパイラの while / for の形: `goto C; B: 本体…; C: 条件の文…; if (c) goto B;` →
/// `while (true) { 条件の文…; if (!c) break; 本体… }`（条件の文が無ければ `while (c) { 本体… }`）。
fn while_loop(body: &mut Vec<Stmt>, i: usize, counts: &mut HashMap<Label, usize>) -> bool {
    let StmtKind::Goto(condition_label) = body[i].kind else { return false };
    let Some(StmtKind::Label(body_label)) = body.get(i + 1).map(|s| s.kind.clone()) else { return false };
    let Some(c) = label_position(body, i + 2, condition_label) else { return false };
    // 条件の分岐（本体の頭へ戻る if）を、C のあとで探す。
    let Some(back) = (c + 1..body.len()).find(|&k| is_branch_to(&body[k], body_label)) else { return false };
    // 戻る分岐の数は 1 つだけ（ほかから B へ飛ぶものがあれば、continue ではない別の形）。
    if counts.get(&body_label).copied().unwrap_or(0) != 1 || counts.get(&condition_label).copied().unwrap_or(0) != 1 {
        return false;
    }
    let loop_body = &body[i + 2..c];
    let condition_stmts = &body[c + 1..back];
    if !closed(loop_body, counts) || !closed(condition_stmts, counts) || condition_stmts.iter().any(Stmt::ends_flow) {
        return false;
    }
    let StmtKind::If { condition, .. } = body[back].kind.clone() else { return false };
    let il = body[back].il;
    let exit = body.get(back + 1).and_then(|s| if let StmtKind::Label(l) = s.kind { Some(l) } else { None });
    let mut loop_body: Vec<Stmt> = body[i + 2..c].to_vec();
    let condition_stmts: Vec<Stmt> = body[c + 1..back].to_vec();
    decrement(counts, body_label);
    decrement(counts, condition_label);
    let looped = if condition_stmts.is_empty() {
        let mut stmt = Stmt::new(StmtKind::While { condition, body: Vec::new() }, il);
        to_break_continue(&mut loop_body, exit, Some(condition_label), counts);
        if let StmtKind::While { body, .. } = &mut stmt.kind {
            *body = loop_body;
        }
        stmt
    } else {
        let mut inner = condition_stmts;
        inner.push(Stmt::new(StmtKind::If { condition: negate(condition), then: vec![Stmt::new(StmtKind::Break, None)], otherwise: Vec::new() }, il));
        inner.extend(loop_body);
        to_break_continue(&mut inner, exit, Some(condition_label), counts);
        Stmt::new(StmtKind::While { condition: Expr::Bool(true), body: inner }, None)
    };
    body.splice(i..=back, [looped]);
    true
}

/// `B: 本体…; if (c) goto B;` → `do { 本体… } while (c);`。
fn do_while(body: &mut Vec<Stmt>, i: usize, counts: &mut HashMap<Label, usize>) -> bool {
    let StmtKind::Label(label) = body[i].kind else { return false };
    if counts.get(&label).copied().unwrap_or(0) != 1 {
        return false;
    }
    let Some(back) = (i + 1..body.len()).find(|&k| is_branch_to(&body[k], label)) else { return false };
    if !closed(&body[i + 1..back], counts) {
        return false;
    }
    let StmtKind::If { condition, .. } = body[back].kind.clone() else { return false };
    let il = body[back].il;
    let exit = body.get(back + 1).and_then(|s| if let StmtKind::Label(l) = s.kind { Some(l) } else { None });
    let mut inner: Vec<Stmt> = body[i + 1..back].to_vec();
    decrement(counts, label);
    to_break_continue(&mut inner, exit, None, counts);
    body.splice(i..=back, [Stmt::new(StmtKind::DoWhile { body: inner, condition }, il)]);
    true
}

/// `if (c) goto label;` か。
fn is_branch_to(stmt: &Stmt, label: Label) -> bool {
    matches!(&stmt.kind, StmtKind::If { then, otherwise, .. } if otherwise.is_empty() && matches!(then.as_slice(), [Stmt { kind: StmtKind::Goto(l), .. }] if *l == label))
}

/// ループの本体の、出口への goto を break に、条件への goto を continue にする（内側のループの中は除く）。
fn to_break_continue(body: &mut [Stmt], exit: Option<Label>, next: Option<Label>, counts: &mut HashMap<Label, usize>) {
    for stmt in body.iter_mut() {
        match stmt.kind {
            StmtKind::Goto(label) if Some(label) == exit => {
                decrement(counts, label);
                stmt.kind = StmtKind::Break;
            }
            StmtKind::Goto(label) if Some(label) == next => {
                decrement(counts, label);
                stmt.kind = StmtKind::Continue;
            }
            StmtKind::While { .. } | StmtKind::DoWhile { .. } => {}
            // try の中からの break / continue は C# で書けるが、finally の中からは書けない。
            StmtKind::Try { ref mut body, ref mut catches, .. } => {
                to_break_continue(body, exit, next, counts);
                for catch in catches {
                    to_break_continue(&mut catch.body, exit, next, counts);
                }
            }
            StmtKind::If { ref mut then, ref mut otherwise, .. } => {
                to_break_continue(then, exit, next, counts);
                to_break_continue(otherwise, exit, next, counts);
            }
            _ => {}
        }
    }
}

/// 飛ばれていないラベルを消す。
fn remove_unused_labels(body: &mut Vec<Stmt>, counts: &HashMap<Label, usize>) {
    body.retain(|s| !matches!(s.kind, StmtKind::Label(l) if counts.get(&l).copied().unwrap_or(0) == 0));
    for stmt in body.iter_mut() {
        for inner in stmt.blocks_mut() {
            remove_unused_labels(inner, counts);
        }
    }
}

/// Debug ビルドの形 `V = 条件; if (V) …`（V はここでしか読まない）を、`if (条件) …` にする。
fn inline_condition_temps(body: &mut Vec<Stmt>) {
    let mut reads: HashMap<u16, usize> = HashMap::new();
    count_local_reads(body, &mut reads);
    inline_in(body, &reads);
}

fn inline_in(body: &mut Vec<Stmt>, reads: &HashMap<u16, usize>) {
    for stmt in body.iter_mut() {
        for inner in stmt.blocks_mut() {
            inline_in(inner, reads);
        }
    }
    let mut removed = Vec::new();
    for i in 0..body.len().saturating_sub(1) {
        let StmtKind::Assign(Expr::Local(local), value) = &body[i].kind else { continue };
        if reads.get(local).copied().unwrap_or(0) != 1 {
            continue;
        }
        let (local, value) = (*local, value.clone());
        let replaced = match &mut body[i + 1].kind {
            StmtKind::If { condition, .. } | StmtKind::While { condition, .. } => replace_local(condition, local, &value),
            // Debug ビルドの `V = 式; return V;`。
            StmtKind::Return(Some(returned)) if *returned == Expr::Local(local) => {
                *returned = value.clone();
                true
            }
            _ => false,
        };
        if replaced {
            // 系列点は、条件の評価が始まる代入の側の位置にし、if の側の点もまとめて残す。
            let assignment = body[i].clone();
            let mut merged = Stmt::new(body[i + 1].kind.clone(), assignment.il);
            merged.absorb(&assignment);
            merged.absorb(&body[i + 1]);
            body[i + 1] = merged;
            removed.push(i);
        }
    }
    for i in removed.into_iter().rev() {
        body.remove(i);
    }
}
/// 条件の中の `Local(local)` を `value` に置き換える（1 つだけ）。
fn replace_local(condition: &mut Expr, local: u16, value: &Expr) -> bool {
    match condition {
        Expr::Local(l) if *l == local => {
            *condition = value.clone();
            true
        }
        Expr::Not(inner) => {
            if replace_local(inner, local, value) {
                *condition = negate((**inner).clone());
                true
            } else {
                false
            }
        }
        Expr::Compare(_, a, b) => replace_local(a, local, value) || replace_local(b, local, value),
        _ => false,
    }
}

fn count_local_reads(body: &[Stmt], reads: &mut HashMap<u16, usize>) {
    visit(body, &mut |s| {
        for (k, e) in s.expressions().into_iter().enumerate() {
            // 代入先そのもの（`v = …` の v）は読みではない（フィールドの持ち主などは読み）。
            if k == 0 && matches!(&s.kind, StmtKind::Assign(Expr::Local(_), _)) {
                continue;
            }
            count_expr_reads(e, reads);
        }
    });
}
fn count_expr_reads(expr: &Expr, reads: &mut HashMap<u16, usize>) {
    if let Expr::Local(l) = expr {
        *reads.entry(*l).or_insert(0) += 1;
        return;
    }
    expr.for_each_child(&mut |child| count_expr_reads(child, reads));
}

/// 残った goto が、外側（か同じ）のブロックのラベルへ飛んでいるか。`scopes` は外側から順の、見えるラベル。
fn check_gotos(body: &[Stmt], scopes: &mut Vec<HashSet<Label>>) -> Result<(), String> {
    let here: HashSet<Label> = body.iter().filter_map(|s| if let StmtKind::Label(l) = s.kind { Some(l) } else { None }).collect();
    scopes.push(here);
    let visible = |label: &Label, scopes: &Vec<HashSet<Label>>| scopes.iter().any(|s| s.contains(label));
    for stmt in body {
        match &stmt.kind {
            StmtKind::Goto(label) if !visible(label, scopes) => {
                scopes.pop();
                return Err(tr!("The goto to {label} cannot be written under C# rules", label = format!("IL_{label:04x}")));
            }
            StmtKind::Switch { targets, .. } if targets.iter().any(|l| !visible(l, scopes)) => {
                scopes.pop();
                return Err(tr!("The switch targets cannot be written under C# rules"));
            }
            _ => {}
        }
        for inner in stmt.blocks() {
            if let Err(e) = check_gotos(inner, scopes) {
                scopes.pop();
                return Err(e);
            }
        }
    }
    scopes.pop();
    Ok(())
}

/// 一度も読まれない一時変数への代入（値に副作用が無いもの）を消す。
fn remove_dead_temps(body: &mut Vec<Stmt>) {
    let mut reads = HashSet::new();
    visit(body, &mut |s| {
        for (k, e) in s.expressions().into_iter().enumerate() {
            // 代入先の一時変数そのものは読みではない。
            if k == 0 && matches!(&s.kind, StmtKind::Assign(Expr::Temp(_), _)) {
                continue;
            }
            collect_temps(e, &mut reads);
        }
    });
    remove_dead_in(body, &reads);
}
fn remove_dead_in(body: &mut Vec<Stmt>, reads: &HashSet<u32>) {
    body.retain(|s| !matches!(&s.kind, StmtKind::Assign(Expr::Temp(t), value) if !reads.contains(t) && value.is_simple()));
    for stmt in body.iter_mut() {
        for inner in stmt.blocks_mut() {
            remove_dead_in(inner, reads);
        }
    }
}

/// 式の中の一時変数の番号を集める。
pub(super) fn collect_temps(expr: &Expr, found: &mut HashSet<u32>) {
    if let Expr::Temp(t) = expr {
        found.insert(*t);
    }
    expr.for_each_child(&mut |child| collect_temps(child, found));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(kind: StmtKind) -> Stmt {
        Stmt::new(kind, None)
    }

    fn call(name: &str) -> Stmt {
        s(StmtKind::Assign(Expr::Literal(name.into()), Expr::Int(1)))
    }

    fn branch(condition: Expr, label: Label) -> Stmt {
        s(StmtKind::If { condition, then: vec![s(StmtKind::Goto(label))], otherwise: Vec::new() })
    }

    #[test]
    fn a_forward_branch_over_statements_becomes_if() {
        // if (x) goto 10; a; 10: b
        let mut body = vec![branch(Expr::Local(0), 10), call("a"), s(StmtKind::Label(10)), call("b")];
        structure(&mut body).unwrap();
        assert_eq!(body.len(), 2);
        let StmtKind::If { condition, then, otherwise } = &body[0].kind else { panic!("{body:?}") };
        assert_eq!(*condition, Expr::Not(Box::new(Expr::Local(0))));
        assert_eq!(then.len(), 1);
        assert!(otherwise.is_empty());
    }

    #[test]
    fn the_compilers_while_shape_becomes_while() {
        // goto 20; 10: a; 20: if (x) goto 10; b
        let mut body = vec![s(StmtKind::Goto(20)), s(StmtKind::Label(10)), call("a"), s(StmtKind::Label(20)), branch(Expr::Local(0), 10), call("b")];
        structure(&mut body).unwrap();
        let StmtKind::While { condition, body: inner } = &body[0].kind else { panic!("{body:?}") };
        assert_eq!(*condition, Expr::Local(0));
        assert_eq!(inner.len(), 1);
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn a_backward_branch_becomes_do_while_and_its_exit_becomes_break() {
        // 10: if (y) goto 30; a; if (x) goto 10; 30: b
        let mut body = vec![s(StmtKind::Label(10)), branch(Expr::Local(1), 30), call("a"), branch(Expr::Local(0), 10), s(StmtKind::Label(30)), call("b")];
        structure(&mut body).unwrap();
        let StmtKind::DoWhile { body: inner, .. } = &body[0].kind else { panic!("{body:?}") };
        let StmtKind::If { then, .. } = &inner[0].kind else { panic!("{inner:?}") };
        assert_eq!(then[0].kind, StmtKind::Break);
    }

    #[test]
    fn a_jump_into_a_block_from_outside_stays_a_goto() {
        // if (x) goto 20; a; 10: b; 20: c; goto 10;（10 へは外から飛ぶので、if にまとめない）
        let mut body = vec![branch(Expr::Local(0), 20), call("a"), s(StmtKind::Label(10)), call("b"), s(StmtKind::Label(20)), call("c"), s(StmtKind::Goto(10))];
        structure(&mut body).unwrap();
        assert!(matches!(body[0].kind, StmtKind::If { ref then, .. } if matches!(then[0].kind, StmtKind::Goto(20))));
    }

    #[test]
    fn a_goto_into_a_nested_block_is_rejected() {
        // { 10: a } の外から goto 10
        let inner = vec![s(StmtKind::Label(10)), call("a")];
        let mut body = vec![s(StmtKind::Goto(10)), s(StmtKind::Try { body: inner, catches: Vec::new(), finally: Some(Vec::new()), fault: None })];
        assert!(structure(&mut body).is_err());
    }

    #[test]
    fn debug_condition_temps_are_inlined() {
        // V0 = x < 3; if (V0) goto 10; a; 10:
        let condition = Expr::Compare(super::super::ast::CmpOp::Lt, Box::new(Expr::Local(1)), Box::new(Expr::Int(3)));
        let mut body = vec![s(StmtKind::Assign(Expr::Local(0), condition.clone())), branch(Expr::Local(0), 10), call("a"), s(StmtKind::Label(10))];
        structure(&mut body).unwrap();
        let StmtKind::If { condition: inlined, .. } = &body[0].kind else { panic!("{body:?}") };
        assert_eq!(*inlined, Expr::Not(Box::new(condition)));
    }
}