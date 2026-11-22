//! 一時変数とローカル変数: 写しを消す、同じ値の一時変数を置き換える、一度だけ使う一時変数を入れる、最初の代入で宣言する。

use super::*;

/// `t2 = t1;`（どちらも一度だけ代入される一時変数）を消し、`t2` を `t1` に置き換える。
pub(super) fn copy_temps(body: &mut Vec<Stmt>) {
    let mut candidates = Vec::new();
    visit(body, &mut |s| {
        if let StmtKind::Assign(target @ Expr::Temp(_), source @ Expr::Temp(_)) = &s.kind {
            candidates.push((target.clone(), source.clone()));
        }
    });
    for (target, source) in candidates {
        if assignments(body, &target) == 1 && assignments(body, &source) == 1 {
            remove_assignment(body, &target);
            substitute_in(body, &target, &source);
        }
    }
}

/// 何度も代入されるが、どの代入も同じ値（変数か定数。一度だけ代入される一時変数を通しても）の一時変数を、その値に
/// 置き換える（分岐の両側でスタックの下の値を渡した形。`s = account; … if (…) { s = t; }`、t も account）。
pub(super) fn uniform_temps(body: &mut Vec<Stmt>) {
    // 置き換えると、別の一時変数の値がそろうことがある（`s19 = s10 | s13`、`s10 = sd`、`sd = result | result`）ので、
    // 変わらなくなるまで繰り返す。
    for _ in 0..8 {
        if !replace_uniform_temps(body) {
            break;
        }
    }
    remove_dead_simple_temps(body);
}

/// `uniform_temps` の 1 回分。置き換えたら `true`。
pub(super) fn replace_uniform_temps(body: &mut Vec<Stmt>) -> bool {
    let mut changed = false;
    let mut values: HashMap<u32, Vec<Expr>> = HashMap::new();
    visit(body, &mut |s| {
        if let StmtKind::Assign(Expr::Temp(t), value) = &s.kind {
            values.entry(*t).or_default().push(value.clone());
        }
    });
    // 一度だけ代入される一時変数の値（たどる先）。
    let single: HashMap<u32, Expr> = values.iter().filter(|(_, v)| v.len() == 1).map(|(t, v)| (*t, v[0].clone())).collect();
    let resolve = |e: &Expr| -> Expr {
        let mut current = e.clone();
        for _ in 0..8 {
            match &current {
                Expr::Temp(t) if single.get(t).is_some_and(Expr::is_simple) => current = single[t].clone(),
                _ => break,
            }
        }
        current
    };
    for (temp, assigned) in &values {
        if assigned.len() < 2 {
            continue;
        }
        let first = resolve(&assigned[0]);
        // 置き換える値は、二度書いてもよいもの（変数か定数）。一時変数をたどり切れなければ、置き換えない。ただし、一度だけ
        // 代入される一時変数（`dup` した配列など）なら置き換える: どの代入よりも前に値が入り、そのあと変わらないので、
        // 使う所で読んでも同じ値になる（分かれ道で配列を持ち回す形。`if (c) { s0 = t0; s3 = ""; } else { s0 = t0; s3 = "!"; }`）。
        let settled = |e: &Expr| matches!(e, Expr::Temp(t) if values.get(t).is_some_and(|v| v.len() == 1));
        if !first.is_simple() || (matches!(first, Expr::Temp(_)) && !settled(&first)) {
            continue;
        }
        if assigned.iter().all(|v| resolve(v) == first) {
            let variable = Expr::Temp(*temp);
            while remove_assignment(body, &variable) {}
            substitute_in(body, &variable, &first);
            changed = true;
        }
    }
    changed
}

/// 読まれなくなった一時変数への、副作用の無い代入を消す。
pub(super) fn remove_dead_simple_temps(body: &mut Vec<Stmt>) {
    let mut reads = HashSet::new();
    visit(body, &mut |s| {
        for (k, e) in s.expressions().into_iter().enumerate() {
            if k == 0 && matches!(&s.kind, StmtKind::Assign(Expr::Temp(_), _)) {
                continue;
            }
            super::super::structure::collect_temps(e, &mut reads);
        }
    });
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if matches!(&block[i].kind, StmtKind::Assign(Expr::Temp(t), value) if !reads.contains(t) && value.is_simple()) {
                let removed = block.remove(i);
                if let Some(next) = block.get_mut(i) {
                    next.absorb(&removed);
                }
                continue;
            }
            i += 1;
        }
    });
}

/// 一度だけ代入され、すぐ次の文で一度だけ読まれる一時変数を、その場所に入れる。次の文の中で、一時変数より前に評価される
/// 副作用のある式があれば入れない（評価の順番が変わるため）。
pub(super) fn inline_temps(body: &mut Vec<Stmt>) {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    visit_expressions(body, &mut |e| {
        if let Expr::Temp(t) = e {
            *counts.entry(*t).or_insert(0) += 1;
        }
    });
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            let candidate = match &block[i].kind {
                StmtKind::Assign(temp @ Expr::Temp(t), value) if counts.get(t) == Some(&2) => Some((temp.clone(), value.clone())),
                _ => None,
            };
            if let Some((temp, value)) = candidate {
                if occurrences(&block[i + 1..i + 2], &temp) == 1 && (nothing_before(&block[i + 1], &temp) || plain_array(&value, &block[i + 1])) {
                    let removed = block.remove(i);
                    for e in block[i].expressions_mut() {
                        substitute(e, &temp, &value);
                    }
                    // 次の文を写さずに（入れ子のブロックごと写すと重い）、系列点だけを前の文の位置に合わせる。
                    let next = &mut block[i];
                    let mut points = Stmt::new(StmtKind::Break, removed.il);
                    points.absorb(&removed);
                    points.absorb(next);
                    next.il = points.il;
                    next.merged = points.merged;
                    continue;
                }
            }
            i += 1;
        }
    });
}

/// 文の中で `target` より前に評価される式に、副作用が無いか（入れ子のブロックを持つ文は、条件の式だけを見る）。
pub(super) fn nothing_before(stmt: &Stmt, target: &Expr) -> bool {
    let mut seen = false;
    let mut clean = true;
    for e in stmt.expressions() {
        side_effects_before(e, target, &mut seen, &mut clean);
    }
    seen && clean
}

pub(super) fn side_effects_before(expr: &Expr, target: &Expr, seen: &mut bool, clean: &mut bool) {
    if *seen {
        return;
    }
    if expr == target {
        *seen = true;
        return;
    }
    let children = expr.children();
    let contains = |e: &Expr| {
        let mut found = false;
        e.walk(&mut |x| found |= x == target);
        found
    };
    if !contains(expr) {
        if expr.has_side_effects() {
            *clean = false;
        }
        return;
    }
    for child in children {
        side_effects_before(child, target, seen, clean);
        if *seen {
            return;
        }
    }
}

/// 最初の代入で宣言する（`int num; num = 0;` → `int num = 0;`）。変数への参照がすべてそのブロックの中にあり、最初の
/// 参照がそのブロックの直下の代入（値に自分を含まない）のときだけ。goto のあるメソッドでは行わない（宣言を飛び越すため）。
pub(super) fn declare_at_first_assignment(body: &mut [Stmt], locals: usize, declared: &mut HashSet<u16>) {
    let totals = local_counts(body, locals);
    declare_in(body, &totals, declared);
}

pub(super) fn declare_in(block: &mut [Stmt], totals: &[usize], declared: &mut HashSet<u16>) {
    // ブロックの中で出てくる数と、最初に出てくる文（ブロックを一度たどって、すべての変数の分を数える）。
    let counts = local_counts(block, totals.len());
    let mut first: Vec<Option<usize>> = vec![None; totals.len()];
    for (index, stmt) in block.iter().enumerate() {
        for_each_local(std::slice::from_ref(stmt), &mut |local| {
            if let Some(slot @ None) = first.get_mut(local as usize) {
                *slot = Some(index);
            }
        });
    }
    for (index, &total) in totals.iter().enumerate() {
        let local = index as u16;
        let (Some(first), true) = (first[index], total != 0 && counts[index] == total && !declared.contains(&local)) else { continue };
        if let StmtKind::Assign(Expr::Local(l), value) = &block[first].kind {
            if *l == local && !mentions(value, &Expr::Local(local)) {
                block[first].kind = StmtKind::Declare { local, value: value.clone() };
                declared.insert(local);
            }
        }
    }
    for stmt in block.iter_mut() {
        // for の初期化の代入は、変数がその for の中だけで使われるなら、宣言つきにする。
        if let StmtKind::For { init: Some(init), .. } = &stmt.kind {
            if let StmtKind::Assign(variable @ Expr::Local(local), value) = &init.kind {
                let (local, value, variable) = (*local, value.clone(), variable.clone());
                if !declared.contains(&local) && totals.get(local as usize) == Some(&occurrences(std::slice::from_ref(stmt), &variable)) && !mentions(&value, &variable) {
                    if let StmtKind::For { init: Some(init), .. } = &mut stmt.kind {
                        init.kind = StmtKind::Declare { local, value };
                        declared.insert(local);
                    }
                }
            }
        }
        for inner in stmt.blocks_mut() {
            declare_in(inner, totals, declared);
        }
    }
}

/// 先に読んだ相手（`t = d; … t.form = x;`、`t = d.colorBox.Items; … t.AddRange(a);`）を、使う所に戻す（`d.form = x;`）。IL は、
/// フィールドへの代入やメソッドの呼び出しの相手を、値より先に積む。値を作る文（`new Form()` と、そのプロパティの代入）の
/// 間、相手は一時変数に残る。
///
/// 戻すのは、一時変数を一度だけ代入して一度だけ使い、その間に相手の値が変わらないと分かるときだけ:
/// - 相手がローカル変数か引数なら、その間（使う文まで）に、それへ代入せず、アドレスも取らない。
/// - それ以外の式なら、その間の文が、その間に作った一時変数と、その配列の要素に書くだけ（相手の式を後で読んでも同じ）。
///
/// どちらも、使う文の中で、相手より前に副作用のある式が無いこと。
pub(super) fn receiver_temps(body: &mut Vec<Stmt>) {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    visit_expressions(body, &mut |e| {
        if let Expr::Temp(t) = e {
            *counts.entry(*t).or_insert(0) += 1;
        }
    });
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i + 1 < block.len() {
            let rewritten = (|| {
                let StmtKind::Assign(temp @ Expr::Temp(t), value) = &block[i].kind else { return None };
                if counts.get(t) != Some(&2) || mentions(value, temp) {
                    return None;
                }
                let j = (i + 1..block.len()).find(|&j| occurrences(&block[j..=j], temp) > 0)?;
                if occurrences(&block[j..=j], temp) != 1 || !nothing_before(&block[j], temp) || j == i + 1 {
                    // すぐ次の文なら、inline_temps が戻す。
                    return None;
                }
                let between = &block[i + 1..j];
                // この文より前に、新しい配列を入れた一時変数（`t9 = new object[3];`）。その要素を入れる文は、間にあってよい。
                let fresh_arrays: Vec<&Expr> = block[..i].iter().filter_map(|s| match &s.kind {
                    StmtKind::Assign(temp @ Expr::Temp(_), Expr::NewArray { .. }) => Some(temp),
                    _ => None,
                }).collect();
                let movable = match value {
                    Expr::Local(_) | Expr::Arg(_) => between.iter().chain(std::iter::once(&block[j])).all(|s| !changes(s, value)),
                    _ => inert(between, &fresh_arrays) && !mentions_any_assigned(value, between),
                };
                if !movable {
                    return None;
                }
                Some((temp.clone(), value.clone(), j))
            })();
            if let Some((temp, value, j)) = rewritten {
                let removed = block.remove(i);
                let use_at = j - 1;
                for e in block[use_at].expressions_mut() {
                    substitute(e, &temp, &value);
                }
                // 消した文の系列点は、次の文にまとめる。
                block[i].absorb(&removed);
                continue;
            }
            i += 1;
        }
    });
}

/// 文が、変数 `variable` に代入するか、そのアドレスを取るか（入れ子のブロックも見る）。
fn changes(stmt: &Stmt, variable: &Expr) -> bool {
    let mut found = false;
    visit(std::slice::from_ref(stmt), &mut |s| {
        found |= match &s.kind {
            StmtKind::Assign(target, _) | StmtKind::Compound { target, .. } | StmtKind::Increment { target, .. } => target == variable,
            _ => false,
        };
        if let (Expr::Local(local), Some(declared)) = (variable, declared_local(s)) {
            found |= declared == *local;
        }
        s.for_each_expression(&mut |e| {
            e.walk(&mut |e| found |= matches!(e, Expr::AddressOf(inner) if **inner == *variable) || matches!(e, Expr::Assign(target, _) if **target == *variable));
        });
    });
    found
}

/// 文の並びが、その中で作った一時変数と、新しい配列（`fresh` か、その中で作ったもの）の要素に、副作用の無い値を書くだけか。
fn inert<'a>(stmts: &'a [Stmt], fresh_arrays: &[&'a Expr]) -> bool {
    let mut fresh: Vec<&Expr> = fresh_arrays.to_vec();
    for s in stmts {
        match &s.kind {
            StmtKind::Assign(temp @ Expr::Temp(_), value) if !value.has_side_effects() || matches!(value, Expr::NewArray { size, .. } if !size.has_side_effects()) => fresh.push(temp),
            StmtKind::Assign(Expr::Element { array, index }, value) if fresh.contains(&&**array) && !index.has_side_effects() && !value.has_side_effects() => {}
            _ => return false,
        }
    }
    true
}

/// 式が、文の並びで代入される一時変数を読むか。
fn mentions_any_assigned(value: &Expr, stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| matches!(&s.kind, StmtKind::Assign(temp @ Expr::Temp(_), _) if mentions(value, temp)))
}

/// 定数と、文 `next` が書き換えないローカル変数や引数だけを並べた配列（`new object[] { "Red", "Green" }`）。作るのに副作用が
/// 無く、中身も変わらないので、`next` の中で前に副作用のある式（`colorBox.Items` を読む）があっても、そこへ入れてよい。
fn plain_array(value: &Expr, next: &Stmt) -> bool {
    let Expr::ArrayInit { values, .. } = value else { return false };
    values.iter().all(|v| match v {
        Expr::Int(_) | Expr::Long(_) | Expr::Float(_) | Expr::Double(_) | Expr::Str(_) | Expr::Null | Expr::Bool(_) | Expr::Char(_) | Expr::Literal(_) => true,
        Expr::Local(_) | Expr::Arg(_) => !changes(next, v),
        Expr::Box { value, .. } => matches!(**value, Expr::Int(_) | Expr::Long(_) | Expr::Double(_) | Expr::Bool(_) | Expr::Char(_)),
        _ => false,
    })
}
