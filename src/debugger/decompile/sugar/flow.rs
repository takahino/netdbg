//! 流れの形: 共有の return、届かない文、流れ落ちる先への goto、switch の節と値、使われないラベル、Debug ビルドの return。

use super::*;

/// 飛ばれていないラベルを消す。
pub(super) fn remove_unused_labels(body: &mut Vec<Stmt>) {
    let mut targets = HashSet::new();
    visit(body, &mut |s| match &s.kind {
        StmtKind::Goto(l) => {
            targets.insert(*l);
        }
        StmtKind::Switch { targets: list, .. } => targets.extend(list.iter().copied()),
        _ => {}
    });
    for_each_block(body, &mut |block| block.retain(|s| !matches!(s.kind, StmtKind::Label(l) if !targets.contains(&l))));
}

/// `goto L; … L: return x;` の goto を `return x;` にする（x は定数か変数）。ただし、x が finally の中で書き換えられる
/// 変数なら、finally のあとで読んだ値と変わるので置き換えない。
pub(super) fn shared_returns(body: &mut [Stmt]) {
    let mut returns: HashMap<Label, Stmt> = HashMap::new();
    let mut in_finally: Vec<Expr> = Vec::new();
    visit(body, &mut |s| {
        if let StmtKind::Try { finally: Some(finally), .. } = &s.kind {
            visit(finally, &mut |inner| {
                if let StmtKind::Assign(target, _) = &inner.kind {
                    in_finally.push(target.clone());
                }
            });
        }
    });
    collect_returns(body, &mut returns);
    returns.retain(|_, stmt| match &stmt.kind {
        StmtKind::Return(Some(value)) => value.is_simple() && !in_finally.contains(value),
        _ => true,
    });
    if !returns.is_empty() {
        replace_returns(body, &returns);
    }
}

/// goto を return にする。catch の無い try（foreach、using、lock の形）と finally のある try の中は置き換えない。
/// finally が無い catch 付きの try では、中の return は catch を通らないので、外の return へ出る goto と同じ。
/// 置き換えた return は、飛び先の return の系列点も持つ（飛び先の文が届かなくなって消えても、その IL の位置で止まれる）。
pub(super) fn replace_returns(block: &mut [Stmt], returns: &HashMap<Label, Stmt>) {
    for stmt in block.iter_mut() {
        if let StmtKind::Goto(label) = stmt.kind {
            if let Some(target) = returns.get(&label) {
                stmt.kind = target.kind.clone();
                stmt.absorb(target);
            }
        }
        // catch の無い try（using、foreach、lock の形）と finally のある try は、中の goto を return にしない。
        if let StmtKind::Try { catches, finally, fault, .. } = &stmt.kind {
            if catches.is_empty() || finally.is_some() || fault.is_some() {
                continue;
            }
        }
        for inner in stmt.blocks_mut() {
            replace_returns(inner, returns);
        }
    }
}

pub(super) fn collect_returns(body: &[Stmt], returns: &mut HashMap<Label, Stmt>) {
    for (i, stmt) in body.iter().enumerate() {
        if let StmtKind::Label(label) = stmt.kind {
            if let Some(next) = body[i + 1..].iter().find(|s| !matches!(s.kind, StmtKind::Label(_))) {
                if matches!(next.kind, StmtKind::Return(_)) {
                    returns.insert(label, next.clone());
                }
            }
        }
        for inner in stmt.blocks() {
            collect_returns(inner, returns);
        }
    }
}

/// 流れが進まない文（return、throw、goto、break、continue）のあと、次のラベルまでの文は届かないので消す。
pub(super) fn remove_dead_code(body: &mut Vec<Stmt>) {
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if block[i].ends_flow() {
                let end = (i + 1..block.len()).find(|&j| matches!(block[j].kind, StmtKind::Label(_))).unwrap_or(block.len());
                block.drain(i + 1..end);
            }
            i += 1;
        }
    });
}

/// 流れ落ちる先への goto を消す。ブロックの終わりから流れ落ちる先は、入れ子をまたいで求める（`if (…) { try { …; goto L; } }
/// L:` の goto は、try と if を抜けて L へ流れ落ちるのと同じ）。ループの本体の終わりは条件へ戻るので、流れ落ちる先は無い。
pub(super) fn fallthrough_gotos(block: &mut Vec<Stmt>, following: &[Label]) {
    for i in 0..block.len() {
        let after = labels_after(block, i, following);
        // 流れ落ちて文の外へ出るブロック（if の両側、try の本体と catch、using、lock）は、文のあとの先へ着く。finally と
        // fault の終わりからは流れ落ちない（例外のときは、投げ直しへ戻る）。
        let (falls_out, handlers) = match &block[i].kind {
            StmtKind::Try { catches, .. } => (true, 1 + catches.len()),
            StmtKind::If { .. } | StmtKind::Using { .. } | StmtKind::Lock { .. } => (true, usize::MAX),
            _ => (false, 0),
        };
        for (k, inner) in block[i].blocks_mut().into_iter().enumerate() {
            let target: &[Label] = if falls_out && k < handlers { &after } else { &[] };
            fallthrough_gotos(inner, target);
        }
    }
    let mut i = 0;
    while i < block.len() {
        if matches!(block[i].kind, StmtKind::Goto(l) if labels_after(block, i, following).contains(&l)) {
            let removed = block.remove(i);
            if let Some(next) = block.get_mut(i) {
                next.absorb(&removed);
            }
            continue;
        }
        i += 1;
    }
}
/// 文 `i` から流れ落ちたときに着くラベル（すぐあとに並ぶラベル。ブロックの終わりなら、外の流れ落ちる先）。
pub(super) fn labels_after(block: &[Stmt], i: usize, following: &[Label]) -> Vec<Label> {
    let mut labels = Vec::new();
    for stmt in &block[i + 1..] {
        match stmt.kind {
            StmtKind::Label(l) => labels.push(l),
            _ => return labels,
        }
    }
    labels.extend_from_slice(following);
    labels
}
/// 同じ値への比較が並んで各 case へ飛ぶ形を、switch にする。
/// `if (v != 1) { if (v == 2) goto L2; goto D; } 中身1; L2: 中身2; D: 既定` と、
/// `if (v == 1) goto L1; if (v == 2) goto L2; goto D; L1: …`。
/// case は 2 つ以上。各節は次の節へ流れ落ちない。
pub(super) fn comparison_switches(body: &mut Vec<Stmt>) {
    let snapshot = body.clone();
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if let Some((stmt, tail, end)) = split_switch_at(block, i) {
                let mut replacement = vec![stmt];
                replacement.extend(tail);
                block.splice(i..end, replacement);
            } else if let Some((stmt, end)) = comparison_switch_at(block, i, &snapshot).or_else(|| decision_switch_at(block, i, &snapshot)).or_else(|| join_switch_at(block, i)) {
                block.splice(i..end, [stmt]);
            }
            i += 1;
        }
    });
}

fn comparison_switch_at(block: &[Stmt], i: usize, method: &[Stmt]) -> Option<(Stmt, usize)> {
    if let Some(found) = nested_comparison_switch(block, i) {
        return Some(found);
    }
    if let Some(found) = flat_comparison_switch(block, i) {
        return Some(found);
    }
    inequality_ladder(block, i, method)
}

/// `if (v != a) { if (v != b) { goto D; } b の中身 } a の中身` を switch にする。
/// 等しいときの中身は、その if のうしろにあって、そこで流れが終わる。一番奥が `goto D` なら D が既定。
fn inequality_ladder(block: &[Stmt], i: usize, method: &[Stmt]) -> Option<(Stmt, usize)> {
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return None };
    if !otherwise.is_empty() {
        return None;
    }
    let (value, first, ty) = inequality(condition)?;
    let (mut arms, default_label, inline_default, arms_ty) = ladder_arms(then, value)?;
    if arms.is_empty() || arms.iter().any(|(case, _)| case == &first) {
        return None;
    }
    let mut labels = HashSet::new();
    if let Some(label) = default_label {
        labels.insert(label);
    }
    let (body, mut end) = take_arm(block, i + 1, &labels, default_label, false)?;
    if !body.last().is_some_and(|stmt| stmt.ends_flow()) {
        return None;
    }
    if let Some(label) = default_label {
        if !label_owned_by(&block[i..end], method, label) {
            return None;
        }
    }
    arms.push((first, body));
    let mut sections = Vec::new();
    for (case, mut body) in arms {
        if let Some(label) = default_label {
            exit_to_break(&mut body, label);
        }
        if !body.last().is_some_and(|stmt| stmt.ends_flow()) {
            body.push(Stmt::new(StmtKind::Break, None));
        }
        sections.push(Section { cases: vec![case], default: false, body });
    }
    if let Some(label) = default_label {
        let StmtKind::Label(found) = block.get(end)?.kind else { return None };
        if found != label {
            return None;
        }
        let (body, next) = take_arm(block, end + 1, &HashSet::new(), None, true)?;
        sections.push(Section { cases: Vec::new(), default: true, body: finish_body(body) });
        end = next;
    } else if let Some(body) = inline_default {
        sections.push(Section { cases: Vec::new(), default: true, body: finish_body(body) });
    }
    if !sections.iter().any(|section| section.default) {
        return None;
    }
    let mut stmt = Stmt::new(StmtKind::SwitchBlock { value: value.clone(), sections, ty: ty.or(arms_ty) }, block[i].il);
    for absorbed in &block[i..end] {
        stmt.absorb(absorbed);
    }
    Some((stmt, end))
}

/// `if (v != c) { 奥 } c の中身` を外側からほどく。奥が `goto D` だけなら、それが既定。
fn ladder_arms(then: &[Stmt], value: &Expr) -> Option<(Vec<(Case, Vec<Stmt>)>, Option<Label>, Option<Vec<Stmt>>, Option<Ty>)> {
    if let [Stmt { kind: StmtKind::Goto(label), .. }] = then {
        return Some((Vec::new(), Some(*label), None, None));
    }
    let StmtKind::If { condition, then: inner, otherwise } = &then.first()?.kind else {
        return then.last()?.ends_flow().then_some((Vec::new(), None, Some(then.to_vec()), None));
    };
    if !otherwise.is_empty() {
        return None;
    }
    let (compared, case, ty) = inequality(condition)?;
    if compared != value {
        return None;
    }
    let (mut arms, default_label, inline_default, inner_ty) = ladder_arms(inner, value)?;
    let body = then[1..].to_vec();
    if body.is_empty() || !body.last()?.ends_flow() || arms.iter().any(|(existing, _)| existing == &case) {
        return None;
    }
    arms.push((case, body));
    Some((arms, default_label, inline_default, ty.or(inner_ty)))
}

/// `if (v != c0) { if (v == c) goto L; … goto D; }` のあとが c0 の中身。
/// 既定が `goto D` ではなく `return` で終わるときも、同じ switch にする。
fn nested_comparison_switch(block: &[Stmt], i: usize) -> Option<(Stmt, usize)> {
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return None };
    if !otherwise.is_empty() {
        return None;
    }
    let (value, first, ty) = inequality(condition)?;
    if let Some((arms, default_label, arms_ty)) = equality_gotos(then, value) {
        if arms.iter().any(|(case, _)| case == &first) {
            return None;
        }
        let mut cases = vec![(first, None)];
        cases.extend(arms.into_iter().map(|(case, label)| (case, Some(label))));
        return finish_switch(block, i + 1, value.clone(), ty.or(arms_ty), cases, Some(default_label), None, i);
    }
    let (arms, default_body, arms_ty) = inline_default_arms(then, value)?;
    if arms.iter().any(|(case, _)| case == &first) {
        return None;
    }
    let mut cases = vec![(first, None)];
    cases.extend(arms.into_iter().map(|(case, label)| (case, Some(label))));
    finish_switch(block, i + 1, value.clone(), ty.or(arms_ty), cases, None, Some(default_body), i)
}

/// `if (v == c) goto L` が 2 つ以上並び、最後が default への goto。
fn flat_comparison_switch(block: &[Stmt], i: usize) -> Option<(Stmt, usize)> {
    let (value, _, _, _) = equality_goto(block.get(i)?)?;
    let (arms, default_label, ty) = equality_gotos(&block[i..], value)?;
    if arms.len() < 2 {
        return None;
    }
    let start = i + arms.len() + 1;
    let cases = arms.into_iter().map(|(case, label)| (case, Some(label))).collect();
    finish_switch(block, start, value.clone(), ty, cases, Some(default_label), None, i)
}

/// `if (!(v > 中間)) { if (v == a) goto La; goto D; } if (v == b) goto Lb; goto D;` を switch にする。
/// 最後が `if (v != c) { … goto D; }` のときは、c は直後のラベルへ落ちる。
fn split_switch_at(block: &[Stmt], i: usize) -> Option<(Stmt, Vec<Stmt>, usize)> {
    let (value, arms, default_label, consumed) = gather_dispatch(&block[i..])?;
    if arms.len() < 2 {
        return None;
    }
    let cases = arms;
    let (mut stmt, end) = finish_switch(block, i + consumed, value, None, cases, Some(default_label), None, i)?;
    let mut tail = Vec::new();
    if let StmtKind::SwitchBlock { sections, .. } = &mut stmt.kind {
        // default が switch の外（文字列の照合の本体より後ろ）なら、そこへの goto は break にしない。
        let default_outside = sections.iter().any(|section| section.default && matches!(section.body.first().map(|stmt| &stmt.kind), Some(StmtKind::Goto(label)) if *label == default_label));
        if !default_outside {
            for section in sections.iter_mut() {
                exit_to_break(&mut section.body, default_label);
            }
        }
        // どの case も既定のラベルへ飛ぶときは、その先は switch のあと（ループの増分など）で、default の中身ではない。
        let shared_tail = sections.iter().filter(|section| !section.default).all(|section| section.body.last().is_some_and(|stmt| matches!(stmt.kind, StmtKind::Break)) && section.body.iter().all(|stmt| !matches!(stmt.kind, StmtKind::Return(_) | StmtKind::Throw(_))));
        if shared_tail {
            if let Some(default_section) = sections.iter_mut().find(|section| section.default) {
                tail = std::mem::take(&mut default_section.body);
                tail.retain(|stmt| !matches!(stmt.kind, StmtKind::Break));
                default_section.body = vec![Stmt::new(StmtKind::Break, None)];
            }
        }
    }
    Some((stmt, tail, end))
}

fn gather_dispatch(stmts: &[Stmt]) -> Option<(Expr, Vec<(Case, Option<Label>)>, Label, usize)> {
    let value = dispatch_value(stmts.first()?)?;
    let (arms, default_label, consumed) = gather_on(stmts, &value)?;
    (arms.len() >= 2).then_some((value, arms, default_label, consumed))
}

fn dispatch_value(stmt: &Stmt) -> Option<Expr> {
    if let Some((value, _, _, _)) = equality_goto(stmt) {
        return Some(value.clone());
    }
    let StmtKind::If { condition, .. } = &stmt.kind else { return None };
    split_subject(condition).cloned()
}

fn gather_on(stmts: &[Stmt], value: &Expr) -> Option<(Vec<(Case, Option<Label>)>, Label, usize)> {
    let mut arms = Vec::new();
    let mut j = 0;
    while j < stmts.len() {
        if let Some((compared, case, label, _)) = stmts.get(j).and_then(equality_goto) {
            if compared != value || arms.iter().any(|(existing, _)| existing == &case) {
                return None;
            }
            arms.push((case, Some(label)));
            j += 1;
            continue;
        }
        if let Some(StmtKind::Goto(label)) = stmts.get(j).map(|stmt| &stmt.kind) {
            if arms.is_empty() {
                return None;
            }
            return Some((arms, *label, j + 1));
        }
        let StmtKind::If { condition, then, otherwise } = &stmts[j].kind else { return None };
        if !otherwise.is_empty() {
            return None;
        }
        if is_split(condition, value) {
            let (low, default_low, _) = gather_on(then, value)?;
            let (high, default_high, consumed) = gather_on(&stmts[j + 1..], value)?;
            if default_low != default_high {
                return None;
            }
            for arm in low.into_iter().chain(high) {
                if arms.iter().any(|(existing, _)| existing == &arm.0) {
                    return None;
                }
                arms.push(arm);
            }
            return Some((arms, default_low, j + 1 + consumed));
        }
        let (value_ne, case) = inequality_case(condition)?;
        if value_ne != value {
            return None;
        }
        let (inner, default_label, _) = gather_on(then, value)?;
        for arm in &inner {
            if arms.iter().any(|(existing, _)| existing == &arm.0) || arm.0 == case {
                return None;
            }
        }
        // `if (v != c) { … goto D; } Label:` は c がそのラベルへ落ちる。ラベルが無いときは、if のうしろが c の中身。
        let fall = match stmts.get(j + 1).map(|stmt| &stmt.kind) {
            Some(StmtKind::Label(fall)) => Some(*fall),
            Some(_) if stmts.get(j + 1).is_some_and(|stmt| same_dispatch(stmt, value)) => return None,
            _ => None,
        };
        arms.extend(inner);
        arms.push((case, fall));
        return Some((arms, default_label, j + 1));
    }
    None
}

fn split_subject(condition: &Expr) -> Option<&Expr> {
    match condition {
        Expr::Not(inner) => split_subject(inner),
        Expr::Compare(op, left, right) if matches!(op, CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le) => {
            if case_value(right).is_some() { Some(left) } else { case_value(left).is_some().then_some(right.as_ref()) }
        }
        _ => None,
    }
}

fn is_split(condition: &Expr, value: &Expr) -> bool {
    split_subject(condition) == Some(value)
}

/// 同じ値の次の比較。`!=` のうしろが case の中身ではなく、まだ振り分けなら、ここは閉じない。
fn same_dispatch(stmt: &Stmt, value: &Expr) -> bool {
    if equality_goto(stmt).is_some_and(|(compared, _, _, _)| compared == value) {
        return true;
    }
    let StmtKind::If { condition, .. } = &stmt.kind else { return false };
    is_split(condition, value) || inequality_case(condition).is_some_and(|(compared, _)| compared == value)
}

fn inequality_case(condition: &Expr) -> Option<(&Expr, Case)> {
    let (value, op, case, _) = compared(condition)?;
    (op == CmpOp::Ne).then_some((value, case))
}

/// `if (v != n)` または `if (v == n)` の否定側が空でない形は、呼び出し側が空の else を確かめる。ここは `!=`。
fn inequality(condition: &Expr) -> Option<(&Expr, Case, Option<Ty>)> {
    let (value, op, case, ty) = compared(condition)?;
    (op == CmpOp::Ne).then_some((value, case, ty))
}

fn equality_goto(stmt: &Stmt) -> Option<(&Expr, Case, Label, Option<Ty>)> {
    let StmtKind::If { condition, then, otherwise } = &stmt.kind else { return None };
    if !otherwise.is_empty() {
        return None;
    }
    let [Stmt { kind: StmtKind::Goto(label), .. }] = then.as_slice() else { return None };
    let (value, op, case, ty) = compared(condition)?;
    (op == CmpOp::Eq).then_some((value, case, *label, ty))
}

/// 先頭から `if (v == c) goto L` を読み、そのあとの `goto D` まで。1 つも無ければ None。
fn equality_gotos(stmts: &[Stmt], value: &Expr) -> Option<(Vec<(Case, Label)>, Label, Option<Ty>)> {
    let (arms, j, ty) = read_arms(stmts, value)?;
    let StmtKind::Goto(default_label) = stmts.get(j)?.kind else { return None };
    Some((arms, default_label, ty))
}

/// `if (v == c) goto L` のあとが、ラベルではなく return や throw で終わる既定。
fn inline_default_arms(stmts: &[Stmt], value: &Expr) -> Option<(Vec<(Case, Label)>, Vec<Stmt>, Option<Ty>)> {
    let (arms, j, ty) = read_arms(stmts, value)?;
    if j >= stmts.len() {
        return None;
    }
    let default_body = stmts[j..].to_vec();
    default_body.last()?.ends_flow().then_some((arms, default_body, ty))
}

fn read_arms(stmts: &[Stmt], value: &Expr) -> Option<(Vec<(Case, Label)>, usize, Option<Ty>)> {
    let mut arms = Vec::new();
    let mut ty = None;
    let mut j = 0;
    while j < stmts.len() {
        if let Some((compared, case, label, case_ty)) = stmts.get(j).and_then(equality_goto) {
            if compared != value {
                break;
            }
            if let Some(case_ty) = case_ty {
                ty = Some(case_ty);
            }
            if arms.iter().any(|(existing, _)| existing == &case) {
                return None;
            }
            arms.push((case, label));
            j += 1;
            continue;
        }
        // `(uint)(v - low) <= (uint)span` の goto は、low から low+span までの case が同じ節。
        if let Some((compared, low, high, label)) = stmts.get(j).and_then(span_goto) {
            if compared != value {
                break;
            }
            for case in low..=high {
                let case = Case::Int(case);
                if arms.iter().any(|(existing, _)| existing == &case) {
                    return None;
                }
                arms.push((case, label));
            }
            j += 1;
            continue;
        }
        break;
    }
    if arms.is_empty() {
        return None;
    }
    Some((arms, j, ty))
}

/// `if ((uint)(v - low) <= (uint)span) goto L`。中に入ったときだけ L へ飛ぶ。
fn span_goto(stmt: &Stmt) -> Option<(&Expr, i64, i64, Label)> {
    let StmtKind::If { condition, then, otherwise } = &stmt.kind else { return None };
    if !otherwise.is_empty() {
        return None;
    }
    let [Stmt { kind: StmtKind::Goto(label), .. }] = then.as_slice() else { return None };
    let Expr::Compare(CmpOp::Le, _, _) = condition else { return None };
    let (low, high, base) = unsigned_range(condition)?;
    Some((base, low, high, *label))
}

fn compared(condition: &Expr) -> Option<(&Expr, CmpOp, Case, Option<Ty>)> {
    let Expr::Compare(op, left, right) = condition else { return None };
    if !matches!(op, CmpOp::Eq | CmpOp::Ne) {
        return None;
    }
    if let Some((case, ty)) = case_value(right) {
        return Some((left, *op, case, ty));
    }
    if let Some((case, ty)) = case_value(left) {
        return Some((right, *op, case, ty));
    }
    None
}

/// 同じ整数を比べて、どの道も return か throw で終わる if を switch にする。
/// `v == 2` と `3 <= v && v <= 10` のように範囲で書かれたものも、幅が狭いときは case を並べる。
fn decision_switch_at(block: &[Stmt], i: usize, method: &[Stmt]) -> Option<(Stmt, usize)> {
    let value = decision_value(&block[i])?;
    let mut end = i;
    while end < block.len() && is_decision_prefix(&block[end], &value) {
        end += 1;
    }
    // 範囲に入ったときの return が、if の直後のラベルに書いてある形。そのラベルへはこの if からしか飛ばない。
    while end + 1 < block.len() {
        let StmtKind::Label(label) = block[end].kind else { break };
        if !matches!(block[end + 1].kind, StmtKind::Return(_) | StmtKind::Throw(_)) || !label_owned_by(&block[i..end], method, label) {
            break;
        }
        end += 2;
    }
    if end == i {
        return None;
    }
    let region = &block[i..end];
    let constants = decision_constants(region);
    let (min, max) = (constants.iter().copied().min()?, constants.iter().copied().max()?);
    if min == i64::MIN || max == i64::MAX || max - min > 48 {
        return None;
    }
    let low = decide(region, method, &value, min - 1)?;
    let high = decide(region, method, &value, max + 1)?;
    if low.kind != high.kind {
        return None;
    }
    let mut arms = Vec::new();
    for n in min..=max {
        let outcome = decide(region, method, &value, n)?;
        if outcome.kind != low.kind {
            arms.push((n, outcome));
        }
    }
    if arms.len() < 2 {
        return None;
    }
    let mut sections = Vec::new();
    let mut taken = vec![false; arms.len()];
    for index in 0..arms.len() {
        if taken[index] {
            continue;
        }
        let mut cases = Vec::new();
        for other in index..arms.len() {
            if !taken[other] && arms[other].1.kind == arms[index].1.kind {
                taken[other] = true;
                cases.push(Case::Int(arms[other].0));
            }
        }
        sections.push(Section { cases, default: false, body: vec![arms[index].1.clone()] });
    }
    sections.push(Section { cases: Vec::new(), default: true, body: vec![low] });
    let mut stmt = Stmt::new(StmtKind::SwitchBlock { value, sections, ty: decision_type(region) }, block[i].il);
    for absorbed in region {
        stmt.absorb(absorbed);
    }
    Some((stmt, end))
}

/// `if (v == a) { … } else { if (v == b) { …; goto L; } … throw; }` のあとが `L:`。
/// 各節は L の手前で終わり、届かない節は default。
fn join_switch_at(block: &[Stmt], i: usize) -> Option<(Stmt, usize)> {
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return None };
    if otherwise.is_empty() {
        return None;
    }
    let (value, first) = equality_case(condition)?;
    let StmtKind::Label(label) = block.get(i + 1)?.kind else { return None };
    let (mut cases, mut default_body) = join_cases(otherwise, &value, label)?;
    let first_body = strip_join(then, label)?;
    if first_body.iter().any(|stmt| stmt.ends_flow()) {
        return None;
    }
    cases.insert(0, (first, first_body));
    if default_body.is_empty() {
        default_body.push(Stmt::new(StmtKind::Break, None));
    }
    if cases.len() < 2 || cases.iter().any(|(case, _)| cases.iter().filter(|(other, _)| other == case).count() != 1) {
        return None;
    }
    let mut sections = Vec::new();
    for (case, mut body) in cases {
        if !body.last().is_some_and(|stmt| stmt.ends_flow()) {
            body.push(Stmt::new(StmtKind::Break, None));
        }
        sections.push(Section { cases: vec![Case::Int(case)], default: false, body });
    }
    sections.push(Section { cases: Vec::new(), default: true, body: default_body });
    let mut stmt = Stmt::new(StmtKind::SwitchBlock { value: value.clone(), sections, ty: None }, block[i].il);
    stmt.absorb(&block[i]);
    Some((stmt, i + 1))
}

fn join_cases(stmts: &[Stmt], value: &Expr, label: Label) -> Option<(Vec<(i64, Vec<Stmt>)>, Vec<Stmt>)> {
    let mut cases = Vec::new();
    let mut index = 0;
    while let Some(StmtKind::If { condition, then, otherwise }) = stmts.get(index).map(|stmt| &stmt.kind) {
        if !otherwise.is_empty() {
            return None;
        }
        let (compared, case) = equality_case(condition)?;
        if compared != value {
            return None;
        }
        cases.push((case, strip_join(then, label)?));
        index += 1;
    }
    let default_body = match stmts.get(index).map(|stmt| &stmt.kind) {
        Some(StmtKind::Throw(_)) => vec![stmts[index].clone()],
        Some(StmtKind::Goto(target)) if *target == label => Vec::new(),
        _ => return None,
    };
    if index + 1 != stmts.len() || cases.is_empty() {
        return None;
    }
    Some((cases, default_body))
}

/// L へ飛ぶ goto を外した節。ほかの飛び方や、L 以外で終わる流れは戻さない。
fn strip_join(body: &[Stmt], label: Label) -> Option<Vec<Stmt>> {
    let mut body = body.to_vec();
    if let Some(StmtKind::Goto(target)) = body.last().map(|stmt| &stmt.kind) {
        if *target != label {
            return None;
        }
        body.pop();
    }
    if body.iter().any(|stmt| matches!(stmt.kind, StmtKind::Goto(_))) {
        return None;
    }
    Some(body)
}

fn equality_case(condition: &Expr) -> Option<(&Expr, i64)> {
    let (value, op, case, _) = compared(condition)?;
    let Case::Int(case) = case else { return None };
    (op == CmpOp::Eq).then_some((value, case))
}

fn decision_value(stmt: &Stmt) -> Option<Expr> {
    let StmtKind::If { condition, .. } = &stmt.kind else { return None };
    integer_subject(condition).cloned()
}

/// 比較の左辺。`v == n` も、`(uint)(v - low) <= span` も、その否定も、同じ `v`。
fn integer_subject(condition: &Expr) -> Option<&Expr> {
    match condition {
        Expr::Not(inner) => integer_subject(inner),
        Expr::Compare(_, _, _) => unsigned_range(condition).map(|(_, _, base)| base).or_else(|| compared_integer(condition).map(|(value, _)| value)),
        _ => None,
    }
}

/// このラベルへの goto が、`region` の中にしか無い。
fn label_owned_by(region: &[Stmt], method: &[Stmt], label: Label) -> bool {
    let inside = goto_targets(region, label);
    inside > 0 && inside == goto_targets(method, label)
}

fn goto_targets(stmts: &[Stmt], label: Label) -> usize {
    let mut count = 0;
    visit(stmts, &mut |stmt| {
        if matches!(stmt.kind, StmtKind::Goto(found) if found == label) {
            count += 1;
        }
    });
    count
}

fn is_decision_prefix(stmt: &Stmt, value: &Expr) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) => true,
        StmtKind::If { condition, then, otherwise } => condition_is_integer(condition, value) && then.iter().chain(otherwise.iter()).all(|inner| is_decision_nested(inner, value)),
        _ => false,
    }
}

fn is_decision_nested(stmt: &Stmt, value: &Expr) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Goto(_) | StmtKind::Label(_) => true,
        StmtKind::If { condition, then, otherwise } => condition_is_integer(condition, value) && then.iter().chain(otherwise.iter()).all(|inner| is_decision_nested(inner, value)),
        _ => false,
    }
}

fn condition_is_integer(condition: &Expr, value: &Expr) -> bool {
    match condition {
        Expr::Not(inner) => condition_is_integer(inner, value),
        Expr::Compare(_, _, _) => compared_integer(condition).is_some_and(|(compared, _)| compared == value) || unsigned_span(condition, value).is_some(),
        _ => false,
    }
}

fn compared_integer(condition: &Expr) -> Option<(&Expr, i64)> {
    let Expr::Compare(_, left, right) = condition else { return None };
    if let Some(case) = integer_literal(right) {
        return Some((left, case));
    }
    integer_literal(left).map(|case| (right.as_ref(), case))
}

fn decision_constants(stmts: &[Stmt]) -> Vec<i64> {
    let mut constants = Vec::new();
    for stmt in stmts {
        if let StmtKind::If { condition, then, otherwise } = &stmt.kind {
            collect_constants(condition, &mut constants);
            constants.extend(decision_constants(then));
            constants.extend(decision_constants(otherwise));
        }
    }
    constants
}

fn collect_constants(condition: &Expr, constants: &mut Vec<i64>) {
    match condition {
        Expr::Not(inner) => collect_constants(inner, constants),
        Expr::Compare(_, _, _) => {
            if let Some((_, case)) = compared_integer(condition) {
                constants.push(case);
            }
            if let Some((low, high)) = unsigned_span_any(condition) {
                constants.push(low);
                constants.push(high);
            }
        }
        _ => {}
    }
}

fn decision_type(stmts: &[Stmt]) -> Option<Ty> {
    for stmt in stmts {
        if let StmtKind::If { condition, then, otherwise } = &stmt.kind {
            if let Some(ty) = convert_type(condition).or_else(|| decision_type(then)).or_else(|| decision_type(otherwise)) {
                return Some(ty);
            }
        }
    }
    None
}

fn convert_type(condition: &Expr) -> Option<Ty> {
    let Expr::Compare(_, left, right) = condition else { return None };
    match (left.as_ref(), right.as_ref()) {
        (_, Expr::Convert { to, value, .. }) | (Expr::Convert { to, value, .. }, _) if matches!(value.as_ref(), Expr::Int(_)) => Some(to.clone()),
        _ => None,
    }
}

enum Step {
    Done(Stmt),
    Fall,
    Fail,
}

fn decide(region: &[Stmt], method: &[Stmt], value: &Expr, n: i64) -> Option<Stmt> {
    match decide_list(region, method, value, n) {
        Step::Done(stmt) => Some(stmt),
        Step::Fall | Step::Fail => None,
    }
}

fn decide_list(stmts: &[Stmt], method: &[Stmt], value: &Expr, n: i64) -> Step {
    for stmt in stmts {
        match decide_one(stmt, method, value, n) {
            Step::Done(done) => return Step::Done(done),
            Step::Fall => {}
            Step::Fail => return Step::Fail,
        }
    }
    Step::Fall
}

fn decide_one(stmt: &Stmt, method: &[Stmt], value: &Expr, n: i64) -> Step {
    match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) => Step::Done(stmt.clone()),
        StmtKind::Label(_) => Step::Fall,
        StmtKind::Goto(label) => labeled_exit(method, *label).map(Step::Done).unwrap_or(Step::Fail),
        StmtKind::If { condition, then, otherwise } => {
            let Some(taken_true) = eval_condition(condition, value, n) else { return Step::Fail };
            let taken = if taken_true { then } else { otherwise };
            match decide_list(taken, method, value, n) {
                Step::Fall => Step::Fall,
                other => other,
            }
        }
        _ => Step::Fail,
    }
}

fn labeled_exit(method: &[Stmt], label: Label) -> Option<Stmt> {
    let stmt = statement_after_label(method, label)?;
    matches!(stmt.kind, StmtKind::Return(_) | StmtKind::Throw(_)).then(|| stmt.clone())
}

fn statement_after_label(stmts: &[Stmt], label: Label) -> Option<&Stmt> {
    for (index, stmt) in stmts.iter().enumerate() {
        if let StmtKind::Label(found) = stmt.kind {
            if found == label {
                return stmts.get(index + 1);
            }
        }
        for inner in stmt.blocks() {
            if let Some(found) = statement_after_label(inner, label) {
                return Some(found);
            }
        }
    }
    None
}

fn eval_condition(condition: &Expr, value: &Expr, n: i64) -> Option<bool> {
    match condition {
        Expr::Not(inner) => Some(!eval_condition(inner, value, n)?),
        Expr::Compare(op, left, right) => {
            if let Some((low, high)) = unsigned_span(condition, value) {
                let inside = n >= low && n <= high;
                return Some(match op {
                    CmpOp::Le => inside,
                    CmpOp::Gt => !inside,
                    _ => return None,
                });
            }
            let value_on_left = left.as_ref() == value;
            let literal = if value_on_left { right } else if right.as_ref() == value { left } else { return None };
            let case = integer_literal(literal)?;
            Some(cmp_int(*op, n, case, value_on_left))
        }
        _ => None,
    }
}

/// `(uint)(v - low) <= (uint)span` は `low` から `low + span` まで。並んだ case を、コンパイラがこう比べる。
fn unsigned_span(condition: &Expr, value: &Expr) -> Option<(i64, i64)> {
    let (low, high, base) = unsigned_range(condition)?;
    (base == value).then_some((low, high))
}

fn unsigned_span_any(condition: &Expr) -> Option<(i64, i64)> {
    unsigned_range(condition).map(|(low, high, _)| (low, high))
}

fn unsigned_range(condition: &Expr) -> Option<(i64, i64, &Expr)> {
    let Expr::Compare(op, left, right) = condition else { return None };
    if !matches!(op, CmpOp::Le | CmpOp::Gt) {
        return None;
    }
    let Expr::Convert { to: Ty::U4 | Ty::U8, value: shifted, .. } = left.as_ref() else { return None };
    let span = unsigned_literal(right)?;
    if !(0..=48).contains(&span) {
        return None;
    }
    let Expr::Binary(BinOp::Sub, base, low_expr) = shifted.as_ref() else { return None };
    let low = integer_literal(low_expr)?;
    let high = low.checked_add(span)?;
    Some((low, high, peel_cast(base)))
}

fn unsigned_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Convert { to: Ty::U4 | Ty::U8, value, .. } => integer_literal(value),
        other => integer_literal(other),
    }
}

fn peel_cast(expr: &Expr) -> &Expr {
    match expr {
        Expr::Convert { value, .. } => peel_cast(value),
        other => other,
    }
}

/// `2223801888u` や `10L` のように、符号なしへ直した定数。
fn integer_literal_text(text: &str) -> Option<i64> {
    let text = text.trim();
    let (digits, unsigned) = if let Some(rest) = text.strip_suffix("UL").or_else(|| text.strip_suffix("ul")) {
        (rest, true)
    } else if let Some(rest) = text.strip_suffix('u').or_else(|| text.strip_suffix('U')) {
        (rest, true)
    } else if let Some(rest) = text.strip_suffix('L').or_else(|| text.strip_suffix('l')) {
        (rest, false)
    } else {
        (text, false)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if unsigned { digits.parse::<u64>().ok().map(|n| n as i64) } else { digits.parse::<i64>().ok() }
}

fn integer_literal(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value) => Some(*value),
        Expr::Long(value) => Some(*value),
        Expr::Convert { value, .. } => match value.as_ref() {
            Expr::Int(value) | Expr::Long(value) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

fn cmp_int(op: CmpOp, n: i64, case: i64, value_on_left: bool) -> bool {
    let (left, right) = if value_on_left { (n, case) } else { (case, n) };
    match op {
        CmpOp::Eq => left == right,
        CmpOp::Ne => left != right,
        CmpOp::Lt => left < right,
        CmpOp::Le => left <= right,
        CmpOp::Gt => left > right,
        CmpOp::Ge => left >= right,
    }
}

fn case_value(expr: &Expr) -> Option<(Case, Option<Ty>)> {
    match expr {
        Expr::Int(case) => Some((Case::Int(*case), None)),
        Expr::Long(case) => Some((Case::Int(*case), None)),
        Expr::Literal(text) => integer_literal_text(text).map(|case| (Case::Int(case), None)),
        Expr::Char(case) => Some((Case::Char(*case), None)),
        Expr::Str(case) => Some((Case::Str(case.clone()), None)),
        Expr::Convert { to, value, .. } => {
            let n = integer_literal(value)?;
            // `(uint)負の定数` は、ビットを符号なしの値にして case にする（`2223801888u`）。
            let case = match to {
                Ty::U1 => (n as u8) as i64,
                Ty::U2 => (n as u16) as i64,
                Ty::U4 => (n as u32) as i64,
                Ty::U8 => n as u64 as i64,
                _ => n,
            };
            Some((Case::Int(case), Some(to.clone())))
        }
        _ => None,
    }
}

/// `start` 以降の節を集めて switch にする。`head` は置き換える範囲の先頭（節の前の if）。
fn finish_switch(block: &[Stmt], start: usize, value: Expr, ty: Option<Ty>, cases: Vec<(Case, Option<Label>)>, default_label: Option<Label>, inline_default: Option<Vec<Stmt>>, head: usize) -> Option<(Stmt, usize)> {
    let mut pending: Vec<(Case, Option<Label>)> = cases;
    let mut labels = HashSet::new();
    for (_, label) in &pending {
        if let Some(label) = label {
            labels.insert(*label);
        }
    }
    if let Some(label) = default_label {
        labels.insert(label);
    }
    if labels.is_empty() || pending.iter().any(|(case, _)| pending.iter().filter(|(other, _)| other == case).count() != 1) {
        return None;
    }
    // ラベルの無い case は 1 つまで。その中身は、振り分けの直後（最後の `!=` のうしろ、または最初の `!=` のうしろ）。
    let mut sections = Vec::new();
    let mut cursor = start;
    let inline = pending.iter().filter(|(_, label)| label.is_none()).count();
    if inline > 1 {
        return None;
    }
    if let Some(index) = pending.iter().position(|(_, label)| label.is_none()) {
        let (case, _) = pending.remove(index);
        // 中身の無い case が、次の case のラベルに落ちる。同じ節にまとめる。
        let falls_into = match block.get(cursor).map(|stmt| &stmt.kind) {
            Some(StmtKind::Label(label)) if labels.contains(label) => Some(*label),
            _ => None,
        };
        if let Some(label) = falls_into {
            pending.push((case, Some(label)));
        } else {
            let (body, next) = take_arm(block, cursor, &labels, default_label, false)?;
            sections.push(Section { cases: vec![case], default: false, body: finish_body(body) });
            cursor = next;
        }
    }
    while cursor < block.len() {
        let StmtKind::Label(label) = block[cursor].kind else { break };
        if !labels.contains(&label) {
            // case の本体が別ラベルに分かれて、default がその先にある（文字列ハッシュの照合）。ここまでを switch にする。
            if !sections.is_empty() {
                if let Some(default_label) = default_label {
                    if block[cursor..].iter().any(|stmt| matches!(stmt.kind, StmtKind::Label(found) if found == default_label)) {
                        sections.push(Section { cases: Vec::new(), default: true, body: vec![Stmt::new(StmtKind::Goto(default_label), None)] });
                        labels.clear();
                        pending.clear();
                        break;
                    }
                }
            }
            return None;
        }
        let (body, next) = take_arm(block, cursor + 1, &labels, default_label, Some(label) == default_label)?;
        let shared: Vec<Case> = pending.iter().filter(|(_, arm)| *arm == Some(label)).map(|(case, _)| case.clone()).collect();
        if !shared.is_empty() {
            pending.retain(|(_, arm)| *arm != Some(label));
            sections.push(Section { cases: shared, default: Some(label) == default_label, body: finish_body(body) });
        } else if Some(label) == default_label {
            sections.push(Section { cases: Vec::new(), default: true, body: finish_body(body) });
        } else {
            return None;
        }
        labels.remove(&label);
        cursor = next;
        if labels.is_empty() {
            break;
        }
    }
    if !labels.is_empty() || !pending.is_empty() {
        return None;
    }
    if let Some(body) = inline_default {
        sections.push(Section { cases: Vec::new(), default: true, body: finish_body(body) });
    }
    if !sections.iter().any(|section| section.default) {
        return None;
    }
    // どの case も既定のラベルへ落ちるなら、そこは switch のうしろ（break）。
    // return や throw で抜ける case があるときは、既定のラベルは既定の中身なので goto のまま残す。
    let default_outside = default_label.is_some_and(|label| sections.iter().any(|section| section.default && matches!(section.body.first().map(|stmt| &stmt.kind), Some(StmtKind::Goto(target)) if *target == label)));
    let some_case_leaves = sections.iter().any(|section| !section.default && leaves_switch(&section.body));
    if !default_outside {
        if let Some(label) = default_label {
            if some_case_leaves {
                if let Some(section) = sections.iter_mut().find(|section| section.default) {
                    section.body.insert(0, Stmt::new(StmtKind::Label(label), None));
                }
            } else {
                for section in &mut sections {
                    exit_to_break(&mut section.body, label);
                }
            }
        }
    }
    let mut stmt = Stmt::new(StmtKind::SwitchBlock { value, sections, ty }, block[head].il);
    for absorbed in &block[head..cursor] {
        stmt.absorb(absorbed);
    }
    Some((stmt, cursor))
}

/// 節の中身。次の case のラベルの手前まで。流れ落ちていたら switch にしない。
/// 節のラベルではないもの（各 case が共有している return の入り口）は、節の中に含める。
fn take_arm(block: &[Stmt], start: usize, labels: &HashSet<Label>, default_label: Option<Label>, allow_empty: bool) -> Option<(Vec<Stmt>, usize)> {
    if start >= block.len() {
        return allow_empty.then_some((Vec::new(), start));
    }
    let mut j = start;
    while j < block.len() {
        if let StmtKind::Label(label) = block[j].kind {
            if labels.contains(&label) {
                if j == start {
                    return ((Some(label) == default_label) && allow_empty).then_some((Vec::new(), j));
                }
                // 既定のラベルへ流れ落ちる節は、break で閉じる。ほかの case へ落ちるものは switch にしない。
                if !block[j - 1].ends_flow() && Some(label) != default_label {
                    return None;
                }
                break;
            }
        }
        j += 1;
        if block[j - 1].ends_flow() {
            break;
        }
    }
    // ブロックの終わりまで流れ落ちる節は、switch のうしろがメソッドの終わりなので break で閉じる。
    if j == start || (j < block.len() && !block[j - 1].ends_flow() && !matches!(block[j].kind, StmtKind::Label(label) if Some(label) == default_label)) {
        return None;
    }
    Some((block[start..j].to_vec(), j))
}

/// case の中に、switch の外へ出る return か throw がある。
fn leaves_switch(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match &stmt.kind {
        StmtKind::Return(_) | StmtKind::Throw(_) => true,
        _ => stmt.blocks().iter().any(|inner| leaves_switch(inner)),
    })
}

/// 節が次へ落ちるなら break を足す。既定への goto は、switch が完成してから break にする。
fn finish_body(mut body: Vec<Stmt>) -> Vec<Stmt> {
    if !body.last().is_some_and(|stmt| stmt.ends_flow()) {
        body.push(Stmt::new(StmtKind::Break, None));
    }
    body
}

/// `switch (v) { case 0: goto L0; … } goto D; L0: 中身 … E:` → case の中身のある switch。case の入り口のラベルへは、
/// switch と default の goto 以外から飛ばれていないこと。出口への goto は break にする（中のループの中は除く）。
pub(super) fn switch_sections(body: &mut Vec<Stmt>) {
    let counts = label_references(body);
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if let Some((stmt, end)) = sections_at(block, i, &counts) {
                block.splice(i..end, [stmt]);
            }
            i += 1;
        }
    });
}

pub(super) fn sections_at(block: &[Stmt], i: usize, counts: &HashMap<Label, usize>) -> Option<(Stmt, usize)> {
    let StmtKind::Switch { value, targets, ty } = &block[i].kind else { return None };
    let (default, start) = match block.get(i + 1).map(|s| &s.kind) {
        Some(StmtKind::Goto(d)) => (Some(*d), i + 2),
        _ => (None, i + 1),
    };
    let entries: HashSet<Label> = targets.iter().copied().chain(default).collect();
    // 入り口のラベルの位置（このブロックの直下）。
    let positions: Vec<(usize, Label)> = (start..block.len()).filter_map(|j| if let StmtKind::Label(l) = block[j].kind { entries.contains(&l).then_some((j, l)) } else { None }).collect();
    let last_entry = positions.last()?.0;
    // 出口: 最後の入り口より後ろで、それより前の節から飛ばれているラベル（か default の行き先）。
    let before = label_references(&block[start..last_entry]);
    let exit = (last_entry + 1..block.len()).find_map(|j| match block[j].kind {
        StmtKind::Label(l) if before.contains_key(&l) || Some(l) == default => Some((j, l)),
        _ => None,
    });
    let end = exit.map_or(block.len(), |(j, _)| j);
    let exit_label = exit.map(|(_, l)| l);
    // 入り口へは、switch と default からだけ。
    for &l in &entries {
        if Some(l) == exit_label {
            continue;
        }
        let expected = targets.iter().filter(|&&t| t == l).count() + (Some(l) == default) as usize;
        if counts.get(&l).copied().unwrap_or(0) != expected || !positions.iter().any(|&(_, p)| p == l) {
            return None;
        }
    }
    // default が無く（goto も無い）、switch のすぐあとが入り口のラベルでなければ、そこからが default の節。
    let mut regions: Vec<(Option<Label>, Vec<Stmt>)> = Vec::new();
    if default.is_none() && start < positions[0].0 {
        regions.push((None, block[start..positions[0].0].to_vec()));
    }
    for (k, &(position, label)) in positions.iter().enumerate() {
        let region_end = positions.get(k + 1).map_or(end, |&(p, _)| p);
        regions.push((Some(label), block[position + 1..region_end].to_vec()));
    }
    // 節の中のラベルへは、その節の中からだけ。
    for (_, region) in &regions {
        let inner = label_references(region);
        let mut closed = true;
        visit(region, &mut |s| {
            if let StmtKind::Label(l) = s.kind {
                closed &= counts.get(&l).copied().unwrap_or(0) == inner.get(&l).copied().unwrap_or(0);
            }
        });
        if !closed {
            return None;
        }
    }
    // case の値: C# のコンパイラは、いちばん小さい case の値を引いて 0 から数える（`switch (x - 1)`）。
    // 引いた定数は、列挙型へのキャストに包まれていることがある（`t.Type - (BsonType)1`）。
    let constant_of = |e: &Expr| match e {
        Expr::Int(c) => Some(*c),
        Expr::Convert { value, .. } => if let Expr::Int(c) = **value { Some(c) } else { None },
        _ => None,
    };
    let (value, offset) = match value {
        Expr::Binary(BinOp::Sub, inner, constant) if constant_of(constant).is_some() => ((**inner).clone(), constant_of(constant).unwrap_or(0)),
        Expr::Binary(BinOp::Add, inner, constant) if constant_of(constant).is_some() => ((**inner).clone(), -constant_of(constant).unwrap_or(0)),
        other => (other.clone(), 0),
    };
    let last = regions.len() - 1;
    let mut sections = Vec::new();
    for (k, (label, mut region)) in regions.into_iter().enumerate() {
        let is_default = label.is_none() || label == default;
        // default の節に向く case（表の隙間）は、default が受けるので書かない。
        let cases: Vec<Case> = if is_default { Vec::new() } else { targets.iter().enumerate().filter(|(_, &t)| Some(t) == label).map(|(index, _)| Case::Int(index as i64 + offset)).collect() };
        if let Some(exit) = exit_label {
            exit_to_break(&mut region, exit);
        }
        // 節の終わりで次の節へ流れてはいけない。最後の節は出口へ流れるので break を足す。
        if !region.last().is_some_and(Stmt::ends_flow) {
            if k != last {
                return None;
            }
            region.push(Stmt::new(StmtKind::Break, None));
        }
        sections.push(Section { cases, default: is_default, body: region });
    }
    let mut stmt = Stmt::new(StmtKind::SwitchBlock { value, sections, ty: ty.clone() }, block[i].il);
    stmt.absorb(&block[i]);
    Some((stmt, end))
}

/// Debug ビルドの `b = a; c = b; switch (c - 1)`: 受け渡しの変数がほかで使われていなければ、値を switch の式に入れる。
pub(super) fn switch_values(body: &mut Vec<Stmt>) {
    let mut counts: HashMap<u16, usize> = HashMap::new();
    visit_expressions(body, &mut |e| {
        if let Expr::Local(l) = e {
            *counts.entry(*l).or_insert(0) += 1;
        }
    });
    for_each_block(body, &mut |block| {
        let mut i = 1;
        while i < block.len() {
            let candidate = match (&block[i - 1].kind, &block[i].kind) {
                (StmtKind::Assign(variable @ Expr::Local(l), value), StmtKind::SwitchBlock { value: switched, .. }) if counts.get(l) == Some(&2) && {
                    let mut found = 0;
                    switched.walk(&mut |e| found += (e == variable) as usize);
                    found == 1
                } => Some((variable.clone(), value.clone())),
                _ => None,
            };
            if let Some((variable, value)) = candidate {
                let removed = block.remove(i - 1);
                for e in block[i - 1].expressions_mut() {
                    substitute(e, &variable, &value);
                }
                let mut merged = Stmt::new(block[i - 1].kind.clone(), removed.il);
                merged.absorb(&removed);
                merged.absorb(&block[i - 1]);
                block[i - 1] = merged;
                i = i.max(2) - 1;
                continue;
            }
            i += 1;
        }
    });
}

/// 節の中の、出口への goto を break にする（ループの中のものは、ループを抜けることになるので除く）。
pub(super) fn exit_to_break(region: &mut [Stmt], exit: Label) {
    for stmt in region.iter_mut() {
        match &mut stmt.kind {
            StmtKind::Goto(l) if *l == exit => stmt.kind = StmtKind::Break,
            StmtKind::While { .. } | StmtKind::DoWhile { .. } | StmtKind::Foreach { .. } | StmtKind::SwitchBlock { .. } => {}
            _ => {
                for inner in stmt.blocks_mut() {
                    exit_to_break(inner, exit);
                }
            }
        }
    }
}

/// Debug ビルドの `v = 式; return v;`: v を読むのがすべてこの形の return なら、`return 式;` にする。
pub(super) fn return_values(body: &mut Vec<Stmt>) {
    let mut reads: HashMap<u16, usize> = HashMap::new();
    visit(body, &mut |s| {
        for (k, e) in s.expressions().into_iter().enumerate() {
            // 代入先そのもの（`v = …` の v）は読みではない。
            if k == 0 && matches!(&s.kind, StmtKind::Assign(Expr::Local(_), _)) {
                continue;
            }
            e.walk(&mut |x| {
                if let Expr::Local(l) = x {
                    *reads.entry(*l).or_insert(0) += 1;
                }
            });
        }
    });
    let mut pairs: HashMap<u16, usize> = HashMap::new();
    count_return_pairs(body, &mut pairs);
    let inline: HashSet<u16> = pairs.into_iter().filter(|(l, n)| reads.get(l) == Some(n)).map(|(l, _)| l).collect();
    if inline.is_empty() {
        return;
    }
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            if let Some((_, value)) = return_pair(block, i).filter(|(l, _)| inline.contains(l)) {
                let removed = block.remove(i);
                let mut merged = Stmt::new(StmtKind::Return(Some(value)), removed.il);
                merged.absorb(&removed);
                merged.absorb(&block[i]);
                block[i] = merged;
            }
            i += 1;
        }
    });
}

/// `block[i]` と次の文が `v = 式; return v;` なら、v と式。
pub(super) fn return_pair(block: &[Stmt], i: usize) -> Option<(u16, Expr)> {
    let (StmtKind::Assign(Expr::Local(local), value), Some(StmtKind::Return(Some(Expr::Local(returned))))) = (&block[i].kind, block.get(i + 1).map(|s| &s.kind)) else { return None };
    (local == returned).then(|| (*local, value.clone()))
}

pub(super) fn count_return_pairs(block: &[Stmt], pairs: &mut HashMap<u16, usize>) {
    for i in 0..block.len() {
        if let Some((local, _)) = return_pair(block, i) {
            *pairs.entry(local).or_insert(0) += 1;
        }
        for inner in block[i].blocks() {
            count_return_pairs(inner, pairs);
        }
    }
}
