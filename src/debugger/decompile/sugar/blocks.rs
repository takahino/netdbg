//! 文の形: foreach、using、lock、for。

use super::*;

/// `int i = 0; while (i < n) { 本体; i++; }` → `for (int i = 0; i < n; i++) { 本体 }`。変数は条件で使い、ループの外では
/// 使わない（宣言つきの初期化のとき）。本体に continue があれば、増分を飛ばす意味になるので行わない。
pub(super) fn for_loops(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 1 < block.len() {
        let rewritten = (|| {
            let variable = match &block[i].kind {
                StmtKind::Declare { local, .. } => Expr::Local(*local),
                StmtKind::Assign(variable @ Expr::Local(_), _) => variable.clone(),
                _ => return None,
            };
            let StmtKind::While { condition, body } = &block[i + 1].kind else { return None };
            let step = body.last()?;
            let steps = matches!(&step.kind, StmtKind::Increment { target, .. } | StmtKind::Compound { target, .. } | StmtKind::Assign(target, _) if *target == variable);
            if !steps || !mentions(condition, &variable) || has_continue(&body[..body.len() - 1]) {
                return None;
            }
            if matches!(block[i].kind, StmtKind::Declare { .. }) && occurrences(&block[i + 2..], &variable) != 0 {
                return None;
            }
            let mut stmt = Stmt::new(
                StmtKind::For { init: Some(Box::new(block[i].clone())), condition: condition.clone(), step: Some(Box::new(step.clone())), body: body[..body.len() - 1].to_vec() },
                block[i].il,
            );
            stmt.absorb(&block[i]);
            stmt.absorb(&block[i + 1]);
            stmt.absorb(step);
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 1, [stmt]);
        }
        i += 1;
    }
}

/// このループの continue があるか（内側のループの中のものは数えない）。
pub(super) fn has_continue(body: &[Stmt]) -> bool {
    body.iter().any(|s| match &s.kind {
        StmtKind::Continue => true,
        StmtKind::While { .. } | StmtKind::DoWhile { .. } | StmtKind::Foreach { .. } | StmtKind::For { .. } => false,
        _ => s.blocks().iter().any(|inner| has_continue(inner)),
    })
}

/// `local.Dispose();` か `if (local != null) local.Dispose();` だけの finally か。
pub(super) fn disposes(context: &Context, finally: &[Stmt], local: &Expr) -> bool {
    let call = match finally {
        [Stmt { kind: StmtKind::Expr(call), .. }] => call,
        [Stmt { kind: StmtKind::If { condition, then, otherwise }, .. }] if otherwise.is_empty() && not_null(condition).is_some_and(|x| refers_to(x, local)) => match then.as_slice() {
            [Stmt { kind: StmtKind::Expr(call), .. }] => call,
            _ => return false,
        },
        _ => return false,
    };
    matches!(call, Expr::Call { target: Some(t), args, .. } if refers_to(t, local) && args.is_empty()) && call_name(context, call).is_some_and(|(name, _)| name == "Dispose")
}

/// `e = X.GetEnumerator(); try { while (e.MoveNext()) { v = e.Current; 本体 } } finally { e.Dispose(); }` →
/// `foreach (T v in X) { 本体 }`。e は、ほかで使わないこと。v は、ループの外で使わず、本体で代入しないこと。
pub(super) fn foreach_loops(context: &Context, body: &mut Vec<Stmt>, declared: &mut HashSet<u16>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            if let Some(stmt) = foreach_at(context, &snapshot, block, i, declared) {
                block.splice(i..=i + 1, [stmt]);
            }
            i += 1;
        }
    });
}

pub(super) fn foreach_at(context: &Context, method: &[Stmt], block: &[Stmt], i: usize, declared: &mut HashSet<u16>) -> Option<Stmt> {
    let StmtKind::Assign(enumerator @ Expr::Local(_), call @ Expr::Call { target: Some(collection), args, .. }) = &block[i].kind else { return None };
    if !args.is_empty() || call_name(context, call)?.0 != "GetEnumerator" {
        return None;
    }
    let StmtKind::Try { body: protected, catches, finally: Some(finally), fault: None } = &block[i + 1].kind else { return None };
    if !catches.is_empty() || !disposes(context, finally, enumerator) {
        return None;
    }
    let [loop_stmt @ Stmt { kind: StmtKind::While { condition, body: loop_body }, .. }] = protected.as_slice() else { return None };
    if !matches!(condition, Expr::Call { target: Some(t), .. } if refers_to(t, enumerator)) || call_name(context, condition)?.0 != "MoveNext" {
        return None;
    }
    let first = loop_body.first()?;
    let StmtKind::Assign(variable @ Expr::Local(local), current) = &first.kind else { return None };
    let current_call = match current {
        Expr::Cast { value, .. } => &**value,
        other => other,
    };
    if !matches!(current_call, Expr::Property { target: Some(t), .. } if refers_to(t, enumerator)) || call_name(context, current_call)?.0 != "get_Current" {
        return None;
    }
    let rest = &loop_body[1..];
    // 列挙子は、この形の中だけ（GetEnumerator、MoveNext、Current、Dispose）。ループの変数は本体の中だけで、代入しない。
    if occurrences(method, enumerator) != occurrences(&block[i..=i + 1], enumerator) || occurrences(rest, enumerator) != 0 {
        return None;
    }
    if occurrences(method, variable) != occurrences(&block[i..=i + 1], variable) || assignments(rest, variable) != 0 {
        return None;
    }
    let mut stmt = Stmt::new(StmtKind::Foreach { local: *local, collection: (**collection).clone(), body: rest.to_vec() }, block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(loop_stmt);
    stmt.absorb(first);
    for s in finally {
        stmt.absorb(s);
    }
    declared.insert(*local);
    if let Expr::Local(e) = enumerator {
        declared.insert(*e);
    }
    Some(stmt)
}

/// `r = X; try { 本体 } finally { if (r != null) r.Dispose(); }` → `using (T r = X) { 本体 }`。r は、ほかで使わず、
/// 本体で代入しないこと。
pub(super) fn using_blocks(context: &Context, body: &mut Vec<Stmt>, declared: &mut HashSet<u16>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            let rewritten = (|| {
                let StmtKind::Assign(resource @ Expr::Local(local), value) = &block[i].kind else { return None };
                let StmtKind::Try { body: protected, catches, finally: Some(finally), fault: None } = &block[i + 1].kind else { return None };
                if !catches.is_empty() || !disposes(context, finally, resource) || assignments(protected, resource) != 0 {
                    return None;
                }
                if occurrences(&snapshot, resource) != occurrences(&block[i..=i + 1], resource) {
                    return None;
                }
                let mut stmt = Stmt::new(StmtKind::Using { local: Some(*local), resource: value.clone(), body: protected.clone() }, block[i].il);
                stmt.absorb(&block[i]);
                for s in finally {
                    stmt.absorb(s);
                }
                declared.insert(*local);
                Some(stmt)
            })();
            if let Some(stmt) = rewritten {
                block.splice(i..=i + 1, [stmt]);
            }
            i += 1;
        }
    });
}

/// `o = X; f = false; try { Monitor.Enter(o, ref f); 本体 } finally { if (f) Monitor.Exit(o); }` → `lock (X) { 本体 }`。
pub(super) fn lock_blocks(context: &Context, body: &mut Vec<Stmt>, declared: &mut HashSet<u16>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 2 < block.len() {
            let rewritten = (|| {
                let StmtKind::Assign(object @ Expr::Local(o), target) = &block[i].kind else { return None };
                let StmtKind::Assign(taken @ Expr::Local(f), Expr::Bool(false)) = &block[i + 1].kind else { return None };
                let StmtKind::Try { body: protected, catches, finally: Some(finally), fault: None } = &block[i + 2].kind else { return None };
                let enter = protected.first()?;
                let StmtKind::Expr(enter_call @ Expr::Call { args, .. }) = &enter.kind else { return None };
                if !catches.is_empty() || call_name(context, enter_call)? != ("Enter".into(), "Monitor".into()) || args.len() != 2 || args[0] != *object || !refers_to(&args[1], taken) {
                    return None;
                }
                let [Stmt { kind: StmtKind::If { condition, then, otherwise }, .. }] = finally.as_slice() else { return None };
                let [Stmt { kind: StmtKind::Expr(exit_call @ Expr::Call { args: exit_args, .. }), .. }] = then.as_slice() else { return None };
                if condition != taken || !otherwise.is_empty() || call_name(context, exit_call)? != ("Exit".into(), "Monitor".into()) || exit_args.first() != Some(object) {
                    return None;
                }
                let inside = &block[i..=i + 2];
                if occurrences(&snapshot, object) != occurrences(inside, object) || occurrences(&snapshot, taken) != occurrences(inside, taken) || occurrences(&protected[1..], object) != 0 {
                    return None;
                }
                let mut stmt = Stmt::new(StmtKind::Lock { target: target.clone(), body: protected[1..].to_vec() }, block[i].il);
                for s in inside.iter().chain(std::iter::once(enter)).chain(finally.iter()) {
                    stmt.absorb(s);
                }
                declared.insert(*o);
                declared.insert(*f);
                Some(stmt)
            })();
            if let Some(stmt) = rewritten {
                block.splice(i..=i + 2, [stmt]);
            }
            i += 1;
        }
    });
}
