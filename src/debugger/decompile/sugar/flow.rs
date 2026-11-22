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

/// goto を return にする。try の中から外へ出る goto（IL の leave）は置き換えない: try の形（foreach、using、lock）を
/// 戻すのに邪魔になるうえ、戻り値を読む時が finally の前に変わる。
/// 置き換えた return は、飛び先の return の系列点も持つ（飛び先の文が届かなくなって消えても、その IL の位置で止まれる）。
pub(super) fn replace_returns(block: &mut [Stmt], returns: &HashMap<Label, Stmt>) {
    for stmt in block.iter_mut() {
        if let StmtKind::Goto(label) = stmt.kind {
            if let Some(target) = returns.get(&label) {
                stmt.kind = target.kind.clone();
                stmt.absorb(target);
            }
        }
        if matches!(stmt.kind, StmtKind::Try { .. }) {
            continue;
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
        let cases: Vec<i64> = if is_default { Vec::new() } else { targets.iter().enumerate().filter(|(_, &t)| Some(t) == label).map(|(index, _)| index as i64 + offset).collect() };
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
