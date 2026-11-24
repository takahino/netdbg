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

/// `local.Dispose();`、`local?.Dispose();`、`if (local != null) local.Dispose();` だけの finally か。
/// `?.` は using を戻す前に付くので、ここで受け手をほどかないと using も foreach も残らない。
/// 型がこのアセンブリから `IDisposable` だと分からなくても using にする。別アセンブリの型は実装が読めない。
/// 列挙子は例外にする。`MoveNext` だけの while と、`GetEnumerator` の結果は、null かもしれないので while のまま残す。
/// 型が `IDisposable` だと分かるときは、今までどおり using にする。
pub(super) fn disposes(context: &Context, finally: &[Stmt], local: &Expr, protected: &[Stmt], value: &Expr) -> bool {
    let Expr::Local(index) = local else { return false };
    if !dispose_shape(context, finally, local) {
        return false;
    }
    let known = context.local_types.get(*index as usize).is_some_and(|ty| context.names.is_disposable(ty));
    known || (!enumerator_loop(context, protected, local) && !is_get_enumerator(context, value))
}

/// try の本体が、その変数の `MoveNext` だけの while か。
fn enumerator_loop(context: &Context, protected: &[Stmt], local: &Expr) -> bool {
    let [Stmt { kind: StmtKind::While { condition, .. }, .. }] = protected else { return false };
    matches!(condition, Expr::Call { target: Some(t), .. } if refers_to(t, local)) && call_name(context, condition).is_some_and(|(name, _)| name == "MoveNext")
}

/// 代入の右辺が `GetEnumerator` か（キャストは通り抜ける）。
fn is_get_enumerator(context: &Context, value: &Expr) -> bool {
    match value {
        Expr::Cast { value, .. } => is_get_enumerator(context, value),
        Expr::Call { .. } => call_name(context, value).is_some_and(|(name, _)| name == "GetEnumerator"),
        _ => false,
    }
}

/// foreach の finally。`Dispose` が必ず走るときと、非ジェネリックな
/// `t = e as IDisposable; t?.Dispose();` のときだけ foreach にする。
/// `e?.Dispose()` は列挙子が null かもしれないので、while のまま残す。
fn foreach_disposes(context: &Context, finally: &[Stmt], local: &Expr) -> bool {
    if unconditional_dispose(context, finally, local) {
        return true;
    }
    let [Stmt { kind: StmtKind::Assign(slot @ Expr::Local(_), Expr::As { value, .. }), .. }, tail @ ..] = finally else { return false };
    refers_to(value, local) && dispose_shape(context, tail, slot)
}

/// `local.Dispose();` だけ。`local?.` や null 判定は含まない。キャストとアドレスは通り抜ける。
fn unconditional_dispose(context: &Context, finally: &[Stmt], local: &Expr) -> bool {
    let [Stmt { kind: StmtKind::Expr(call), .. }] = finally else { return false };
    let Expr::Call { target: Some(receiver), args, .. } = call else { return false };
    args.is_empty() && bare_receiver(receiver, local) && call_name(context, call).is_some_and(|(name, _)| name == "Dispose")
}

fn bare_receiver(target: &Expr, local: &Expr) -> bool {
    match target {
        Expr::AddressOf(inner) | Expr::Deref(inner) | Expr::Cast { value: inner, .. } | Expr::Box { value: inner, .. } => bare_receiver(inner, local),
        other => other == local,
    }
}

fn dispose_shape(context: &Context, finally: &[Stmt], local: &Expr) -> bool {
    let call = match finally {
        [Stmt { kind: StmtKind::Expr(call), .. }] => call,
        [Stmt { kind: StmtKind::If { condition, then, otherwise }, .. }] if otherwise.is_empty() && not_null(condition).is_some_and(|x| refers_to(x, local)) => match then.as_slice() {
            [Stmt { kind: StmtKind::Expr(call), .. }] => call,
            _ => return false,
        },
        _ => return false,
    };
    matches!(call, Expr::Call { target: Some(t), args, .. } if args.is_empty() && dispose_receiver(t, local)) && call_name(context, call).is_some_and(|(name, _)| name == "Dispose")
}

/// `Dispose` の受け手が、そのローカルか `local?.` か（キャストは通り抜ける）。
pub(super) fn dispose_receiver(target: &Expr, local: &Expr) -> bool {
    match target {
        Expr::NullConditional(inner) | Expr::AddressOf(inner) | Expr::Deref(inner) | Expr::Cast { value: inner, .. } | Expr::Box { value: inner, .. } => dispose_receiver(inner, local),
        other => other == local,
    }
}

/// `i = 0; while (i < a.Length) { v = a[i]; 本体; i++; }` → `foreach (T v in a) { 本体 }`。
/// 配列の foreach は GetEnumerator ではなく、この形になる。i も v も、ループの外では使わない。
pub(super) fn array_foreach(body: &mut Vec<Stmt>, declared: &mut HashSet<u16>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            if let Some(stmt) = array_foreach_at(&snapshot, block, i, declared) {
                block.splice(i..=i + 1, [stmt]);
            }
            i += 1;
        }
    });
}

fn array_foreach_at(method: &[Stmt], block: &[Stmt], i: usize, declared: &mut HashSet<u16>) -> Option<Stmt> {
    let index = match &block[i].kind {
        StmtKind::Assign(index @ Expr::Local(_), Expr::Int(0)) => index.clone(),
        StmtKind::Declare { local, value } if value == &Expr::Int(0) => Expr::Local(*local),
        _ => return None,
    };
    let StmtKind::While { condition, body } = &block[i + 1].kind else { return None };
    let array = length_bound(condition, &index)?;
    if !array.is_simple() {
        return None;
    }
    let first = body.first()?;
    let StmtKind::Assign(item @ Expr::Local(local), value) = &first.kind else { return None };
    let element = match value {
        Expr::Cast { value, .. } => value.as_ref(),
        other => other,
    };
    let Expr::Element { array: element_array, index: element_index } = element else { return None };
    if element_array.as_ref() != array || element_index.as_ref() != &index || body.len() < 2 {
        return None;
    }
    let last = body.last()?;
    if !steps_index(&last.kind, &index) || has_continue(body) {
        return None;
    }
    let rest = &body[1..body.len() - 1];
    if occurrences(rest, &index) != 0 || assignments(rest, item) != 0 {
        return None;
    }
    if occurrences(method, &index) != occurrences(&block[i..=i + 1], &index) || occurrences(method, item) != occurrences(&block[i..=i + 1], item) {
        return None;
    }
    let mut stmt = Stmt::new(StmtKind::Foreach { local: *local, collection: array.clone(), body: rest.to_vec() }, block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(&block[i + 1]);
    stmt.absorb(first);
    stmt.absorb(last);
    declared.insert(*local);
    Some(stmt)
}

fn steps_index(kind: &StmtKind, index: &Expr) -> bool {
    match kind {
        StmtKind::Increment { target, decrement: false } if target == index => true,
        StmtKind::Assign(target, Expr::Binary(BinOp::Add, left, right)) if target == index && ((left.as_ref() == index && **right == Expr::Int(1)) || (right.as_ref() == index && **left == Expr::Int(1))) => true,
        _ => false,
    }
}

/// `i < a.Length` か `i < (int)a.Length` の、配列。
fn length_bound<'a>(condition: &'a Expr, index: &Expr) -> Option<&'a Expr> {
    let Expr::Compare(CmpOp::Lt, left, right) = condition else { return None };
    if left.as_ref() != index {
        return None;
    }
    let mut bound = right.as_ref();
    while let Expr::Convert { value, .. } = bound {
        bound = value;
    }
    match bound {
        Expr::Length(array) => Some(array),
        _ => None,
    }
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
    if !catches.is_empty() || !foreach_disposes(context, finally, enumerator) {
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

/// `r = X; try { 本体 } finally { r.Dispose(); }` または `r?.Dispose()` → `using (T r = X) { 本体 }`。
/// r は、ほかで使わず、本体で代入しないこと。型が別アセンブリでも戻す。
/// `GetEnumerator` の結果と、その `MoveNext` だけの while は戻さない。
pub(super) fn using_blocks(context: &Context, body: &mut Vec<Stmt>, declared: &mut HashSet<u16>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            let rewritten = (|| {
                let StmtKind::Assign(resource @ Expr::Local(local), value) = &block[i].kind else { return None };
                let StmtKind::Try { body: protected, catches, finally: Some(finally), fault: None } = &block[i + 1].kind else { return None };
                if !catches.is_empty() || !disposes(context, finally, resource, protected, value) || assignments(protected, resource) != 0 {
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
