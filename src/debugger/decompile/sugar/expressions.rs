//! 式の形: `?.`、`??`、三項演算子、デリゲートの使い回し、初期化子、配列の要素、文字列の連結、複合代入。

use super::*;

/// `t = F; s = t; if (t == null) { u = Y; F = u; s = u; } … s …` → `… F ?? (F = Y) …`（デリゲートを使い回すための、
/// コンパイラの形）。s はここでしか使わない。
pub(super) fn coalesce_assign(context: &Context, block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 3 < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(t @ Expr::Temp(_), field) = &block[i].kind else { return None };
            let StmtKind::Assign(s @ Expr::Temp(_), copied) = &block[i + 1].kind else { return None };
            let StmtKind::If { condition, then, otherwise } = &block[i + 2].kind else { return None };
            if copied != t || is_null(condition) != Some(t) || !otherwise.is_empty() || !matches!(field, Expr::Field { target: None, .. }) {
                return None;
            }
            let [Stmt { kind: StmtKind::Assign(u @ Expr::Temp(_), created), .. }, Stmt { kind: StmtKind::Assign(stored, from_u), .. }, Stmt { kind: StmtKind::Assign(s2, from_u2), .. }] = then.as_slice() else { return None };
            if stored != field || from_u != u || s2 != s || from_u2 != u {
                return None;
            }
            if occurrences(block, s) != 3 || occurrences(&block[i + 3..i + 4], s) != 1 || occurrences(block, t) != 3 {
                return None;
            }
            // コンパイラが作った使い回し用のフィールド（`<>O`、`<>9__`）なら、作る式だけを書く（使い回しはコンパイラの都合）。
            let value = if is_cache_field(context, field) { created.clone() } else { Expr::Coalesce(field.clone().boxed(), Expr::Assign(field.clone().boxed(), created.clone().boxed()).boxed()) };
            let mut next = block[i + 3].clone();
            for e in next.expressions_mut() {
                substitute(e, s, &value);
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            for absorbed in &block[i..=i + 3] {
                stmt.absorb(absorbed);
            }
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 3, [stmt]);
        }
        i += 1;
    }
}

/// コンパイラがデリゲートを使い回す形（`t = <>9__0_0; …写し…; if (t == null) { u = new D(…); <>9__0_0 = u; …写し… }`）
/// から、使い回しを外して作る式だけにする。評価スタックの下に別の値（`list.Where(…)` の `list`）があると、分かれ道の
/// 両側で写す文が入るので、`coalesce_assign` の形にならない。写す文のうち、`t` を写すものは作る式に、ほかは一度だけに。
pub(super) fn delegate_caches(context: &Context, block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i < block.len() {
        if let Some((end, replacement)) = delegate_cache_at(context, block, i) {
            let mut replacement = replacement;
            if let Some(first) = replacement.first_mut() {
                for absorbed in &block[i..=end] {
                    first.absorb(absorbed);
                }
            }
            block.splice(i..=end, replacement);
        }
        i += 1;
    }
}

/// `block[i]` から始まる使い回しの形の、終わりの添字（`if` の文）と、置き換える文。
pub(super) fn delegate_cache_at(context: &Context, block: &[Stmt], i: usize) -> Option<(usize, Vec<Stmt>)> {
    let StmtKind::Assign(t @ Expr::Temp(_), field) = &block[i].kind else { return None };
    if !is_cache_field(context, field) {
        return None;
    }
    // 一時変数への代入（写す文と、評価スタックで先に積んだ値を受ける文）が続き、そのあとに `if (t == null)`。
    let mut j = i + 1;
    while j < block.len() && matches!(&block[j].kind, StmtKind::Assign(Expr::Temp(_), v) if v == t || !mentions(v, t)) {
        j += 1;
    }
    let StmtKind::If { condition, then, otherwise } = &block.get(j)?.kind else { return None };
    if is_null(condition) != Some(t) || !otherwise.is_empty() {
        return None;
    }
    let [Stmt { kind: StmtKind::Assign(u @ Expr::Temp(_), created @ Expr::NewDelegate { .. }), .. }, Stmt { kind: StmtKind::Assign(stored, from_u), .. }, copies @ ..] = then.as_slice() else { return None };
    if stored != field || from_u != u {
        return None;
    }
    // 中の写しは、どれも外にも同じ先への写しがあり、値が同じもの（`t` の所は `u`）。外で写した一時変数を通しても同じ
    // （分かれ道の両側へ、別の一時変数で渡した形: 外 `s53 = s4b; s6a = s4b;`、中 `s6a = s53;`）。
    let outside = &block[i + 1..j];
    let copied: Vec<(&Expr, &Expr)> = outside.iter().filter_map(|s| if let StmtKind::Assign(a, v) = &s.kind { v.is_simple().then_some((a, v)) } else { None }).collect();
    fn copy_of<'s>(copied: &[(&'s Expr, &'s Expr)], e: &Expr) -> Option<&'s Expr> {
        copied.iter().find(|(a, _)| *a == e).map(|(_, v)| *v)
    }
    fn resolve<'s>(copied: &[(&'s Expr, &'s Expr)], e: &'s Expr) -> &'s Expr {
        let mut current = e;
        // 写しの写しは、たかだか写しの数だけたどる。
        for _ in 0..copied.len() {
            match copy_of(copied, current) {
                Some(next) if matches!(current, Expr::Temp(_)) => current = next,
                _ => break,
            }
        }
        current
    }
    for inner in copies {
        let StmtKind::Assign(a, x) = &inner.kind else { return None };
        let y = copy_of(&copied, a)?;
        if !(resolve(&copied, x) == resolve(&copied, y) || (x == u && y == t)) {
            return None;
        }
    }
    // t と u は、ここでしか使わない。
    if occurrences(&block[j + 1..], t) != 0 || occurrences(&block[j + 1..], u) != 0 {
        return None;
    }
    let replacement = outside
        .iter()
        .map(|s| {
            let StmtKind::Assign(target, value) = &s.kind else { unreachable!() };
            let value = if value == t { created.clone() } else { value.clone() };
            Stmt::new(StmtKind::Assign(target.clone(), value), s.il)
        })
        .collect();
    Some((j, replacement))
}

/// `t = new T[n]; t[0] = a; t[1] = b; …`（要素を順に、間に別の文を挟まず）→ `t = new T[] { a, b, … }`。
pub(super) fn array_elements(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(array @ Expr::Temp(_), Expr::NewArray { element, size }) = &block[i].kind else { return None };
            let Expr::Int(count) = **size else { return None };
            let count = usize::try_from(count).ok().filter(|&c| c > 0 && i + c < block.len())?;
            let mut values = Vec::with_capacity(count);
            for k in 0..count {
                let StmtKind::Assign(Expr::Element { array: a, index }, value) = &block[i + 1 + k].kind else { return None };
                if **a != *array || **index != Expr::Int(k as i64) || mentions(value, array) {
                    return None;
                }
                values.push(value.clone());
            }
            let mut stmt = Stmt::new(StmtKind::Assign(array.clone(), Expr::ArrayInit { element: *element, values }), block[i].il);
            for absorbed in &block[i..=i + count] {
                stmt.absorb(absorbed);
            }
            Some((stmt, i + count))
        })();
        if let Some((stmt, end)) = rewritten {
            block.splice(i..=end, [stmt]);
        }
        i += 1;
    }
}

/// `t = new C(…); t.Add(a); t.Add(b); … t …` → `… new C(…) { a, b } …`、`t = new C(…); t.X = 1; … t …` →
/// `… new C(…) { X = 1 } …`（t は一時変数で、ここでしか使わない）。系列点は、まとめた先の文に残す。
pub(super) fn initializers(context: &Context, block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 2 < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(temp @ Expr::Temp(_), created @ Expr::New { method: constructor, .. }) = &block[i].kind else { return None };
            // `Add` を呼んでいても、IEnumerable でない型には、コレクション初期化子を書けない（CS1922）。
            let collection_type = context.names.method_owner(*constructor).is_some_and(|owner| context.names.is_collection_type(owner));
            let (mut collection, mut members) = (Vec::new(), Vec::new());
            let mut j = i + 1;
            while j < block.len() {
                match &block[j].kind {
                    StmtKind::Expr(call @ Expr::Call { target: Some(target), args, .. }) if **target == *temp && members.is_empty() && collection_type && call_name(context, call).is_some_and(|(n, _)| n == "Add") => {
                        if args.iter().any(|a| mentions(a, temp)) {
                            break;
                        }
                        collection.push(args.clone());
                    }
                    StmtKind::Assign(member @ (Expr::Field { target: Some(target), .. } | Expr::Property { target: Some(target), .. }), value) if **target == *temp && collection.is_empty() => {
                        if mentions(value, temp) {
                            break;
                        }
                        let name = match member {
                            Expr::Field { token, .. } => context.names.field_parts(*token, &Default::default()).1,
                            Expr::Property { method, args, .. } if args.is_empty() => super::super::names::ident(context.names.method_ref(*method, &Default::default())?.name.get(4..)?),
                            _ => break,
                        };
                        members.push((name, value.clone()));
                    }
                    _ => break,
                }
                j += 1;
            }
            if (collection.is_empty() && members.is_empty()) || j >= block.len() {
                return None;
            }
            let used = j - i - 1;
            if occurrences(block, temp) != used + 2 || occurrences(&block[j..=j], temp) != 1 {
                return None;
            }
            let value = Expr::Initializer { new: created.clone().boxed(), collection, members };
            let mut next = block[j].clone();
            for e in next.expressions_mut() {
                substitute(e, temp, &value);
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            for absorbed in &block[i..=j] {
                stmt.absorb(absorbed);
            }
            Some((stmt, j))
        })();
        if let Some((stmt, j)) = rewritten {
            block.splice(i..=j, [stmt]);
        }
        i += 1;
    }
}

/// `if (c) { } else { S }` → `if (!c) { S }`。
pub(super) fn empty_then(block: &mut [Stmt]) {
    for stmt in block.iter_mut() {
        if let StmtKind::If { condition, then, otherwise } = &mut stmt.kind {
            if then.is_empty() && !otherwise.is_empty() {
                *condition = negate(condition.clone());
                std::mem::swap(then, otherwise);
            }
        }
    }
}

/// `t = X; if (t != null) { t.M(…); }` → `X?.M(…);`（t はここでしか使わない）。
pub(super) fn null_conditional(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 1 < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(temp @ Expr::Temp(_), value) = &block[i].kind else { return None };
            let StmtKind::If { condition, then, otherwise } = &block[i + 1].kind else { return None };
            if not_null(condition) != Some(temp) || !otherwise.is_empty() || then.len() != 1 || occurrences(block, temp) != 3 {
                return None;
            }
            let StmtKind::Expr(Expr::Call { method, target: Some(target), args, virtual_call }) = &then[0].kind else { return None };
            if **target != *temp {
                return None;
            }
            let call = Expr::Call { method: *method, target: Some(Expr::NullConditional(value.clone().boxed()).boxed()), args: args.clone(), virtual_call: *virtual_call };
            let mut stmt = Stmt::new(StmtKind::Expr(call), block[i].il);
            stmt.absorb(&block[i]);
            stmt.absorb(&block[i + 1]);
            stmt.absorb(&then[0]);
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 1, [stmt]);
        }
        i += 1;
    }
}

/// `t = X; if (X == null) { t = Y; } … t …`（t はここでしか使わない、X は変数か定数）→ `… X ?? Y …`。
pub(super) fn coalesce(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 2 < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(temp @ Expr::Temp(_), value) = &block[i].kind else { return None };
            let StmtKind::If { condition, then, otherwise } = &block[i + 1].kind else { return None };
            let tested = is_null(condition)?;
            if (tested != value && tested != temp) || !value.is_simple() || !otherwise.is_empty() {
                return None;
            }
            let [Stmt { kind: StmtKind::Assign(again, fallback), .. }] = then.as_slice() else { return None };
            if again != temp || occurrences(block, temp) != 3 || occurrences(&block[i + 2..i + 3], temp) != 1 {
                return None;
            }
            let mut next = block[i + 2].clone();
            for e in next.expressions_mut() {
                substitute(e, temp, &Expr::Coalesce(value.clone().boxed(), fallback.clone().boxed()));
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            stmt.absorb(&block[i]);
            stmt.absorb(&block[i + 1]);
            stmt.absorb(&next);
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 2, [stmt]);
        }
        i += 1;
    }
}

/// `if (c) { t = A; } else { t = B; } … t …`（t はここでしか使わない）→ `… c ? A : B …`。
pub(super) fn conditional(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 1 < block.len() {
        let rewritten = (|| {
            let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return None };
            let ([Stmt { kind: StmtKind::Assign(t1 @ Expr::Temp(_), a), .. }], [Stmt { kind: StmtKind::Assign(t2, b), .. }]) = (then.as_slice(), otherwise.as_slice()) else { return None };
            if t1 != t2 || occurrences(block, t1) != 3 || occurrences(&block[i + 1..i + 2], t1) != 1 {
                return None;
            }
            let mut next = block[i + 1].clone();
            // 否定の条件（`!c ? a : b`）は、両側を入れ替えて `c ? b : a` にする。
            let ternary = match condition {
                Expr::Not(inner) => Expr::Conditional(inner.clone(), b.clone().boxed(), a.clone().boxed()),
                other => Expr::Conditional(other.clone().boxed(), a.clone().boxed(), b.clone().boxed()),
            };
            for e in next.expressions_mut() {
                substitute(e, t1, &ternary);
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            stmt.absorb(&block[i]);
            stmt.absorb(&next);
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 1, [stmt]);
        }
        i += 1;
    }
}

/// `t = X; … (t == null ? 既定 : t.M())` → `X?.M()`。既定が null でなければ `X?.M() ?? 既定`
/// （`Value?.GetDeepHashCode() ?? 0`、`_value?.GetType()`）。t はこの代入と、その比較と、受け手だけ。
pub(super) fn null_conditional_values(block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 1 < block.len() {
        let rewritten = (|| {
            let StmtKind::Assign(temp @ Expr::Temp(_), value) = &block[i].kind else { return None };
            if mentions(value, temp) || occurrences(block, temp) != 3 {
                return None;
            }
            let temp = temp.clone();
            let value = value.clone();
            let mut next = block[i + 1].clone();
            if !rewrite_null_conditional(&mut next, &temp, &value) {
                return None;
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            stmt.absorb(&block[i]);
            stmt.absorb(&block[i + 1]);
            Some(stmt)
        })();
        if let Some(stmt) = rewritten {
            block.splice(i..=i + 1, [stmt]);
        }
        i += 1;
    }
}

/// 式の中の `temp == null ? fallback : temp.メンバー` を、`value?.メンバー` に置き換える。1 か所でも置き換えたら true。
fn rewrite_null_conditional(stmt: &mut Stmt, temp: &Expr, value: &Expr) -> bool {
    let mut found = false;
    stmt.for_each_expression_mut(&mut |expr| found |= rewrite_null_conditional_expr(expr, temp, value));
    found
}

fn rewrite_null_conditional_expr(expr: &mut Expr, temp: &Expr, value: &Expr) -> bool {
    if let Some(rewritten) = null_conditional_of(expr, temp, value) {
        *expr = rewritten;
        return true;
    }
    let mut found = false;
    expr.for_each_child_mut(&mut |child| found |= rewrite_null_conditional_expr(child, temp, value));
    found
}

/// `temp` が null なら fallback、そうでなければ `temp` のメンバー、という式なら、`value` の `?.`。
fn null_conditional_of(expr: &Expr, temp: &Expr, value: &Expr) -> Option<Expr> {
    let Expr::Conditional(condition, a, b) = expr else { return None };
    let (access, fallback) = match condition.as_ref() {
        Expr::Compare(CmpOp::Eq, left, right) if null_and(left, right, temp) => (b.as_ref(), a.as_ref()),
        Expr::Compare(CmpOp::Ne, left, right) if null_and(left, right, temp) => (a.as_ref(), b.as_ref()),
        _ => return None,
    };
    let lifted = null_conditional_access(access, temp, value)?;
    if *fallback == Expr::Null { Some(lifted) } else { Some(Expr::Coalesce(Box::new(lifted), Box::new(fallback.clone()))) }
}

/// `x == null ? 既定 : x.M()`（x はローカル変数や引数）を `x?.M()` にする。既定が null でなければ `x?.M() ?? 既定`。
/// 一時変数へ入れた形は `null_conditional_values` が先に、受け手そのものへ戻している。
pub(super) fn lift_simple_null_conditionals(body: &mut Vec<Stmt>) {
    visit_expressions_mut(body, &mut |expr| {
        if let Some(rewritten) = simple_null_conditional(expr) {
            *expr = rewritten;
        }
    });
}

fn simple_null_conditional(expr: &Expr) -> Option<Expr> {
    let Expr::Conditional(condition, _, _) = expr else { return None };
    let tested = match condition.as_ref() {
        Expr::Compare(CmpOp::Eq | CmpOp::Ne, left, right) if **right == Expr::Null && left.is_simple() => left.as_ref(),
        Expr::Compare(CmpOp::Eq | CmpOp::Ne, left, right) if **left == Expr::Null && right.is_simple() => right.as_ref(),
        _ => return None,
    };
    null_conditional_of(expr, tested, tested)
}

fn null_and(left: &Expr, right: &Expr, temp: &Expr) -> bool {
    (left == temp && *right == Expr::Null) || (right == temp && *left == Expr::Null)
}

/// 分岐のまま残った `&&`、`||`、`?:`、`??` を式にする。
/// `if (a) { if (!b) goto F; t = c; goto E; } F: t = false; E:` は `t = a && b && c`。
/// `if (c) { t = a; } else { t = b; }` は `t = c ? a : b`（bool なら `&&` / `||`、null なら `??`）。
pub(super) fn short_circuit(body: &mut Vec<Stmt>) {
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if !fold_if_else_assign(None, block, i) && !fold_goto_assign(block, i) {
                i += 1;
            }
        }
    });
    visit_expressions_mut(body, &mut |expr| {
        let simplified = simplify_logic(expr.clone());
        if simplified != *expr {
            *expr = simplified;
        }
    });
}

/// `if (x == null) return 既定; return x.M();` と、`if (x != null) { t = x.M(); } else { t = 既定; return t; } return t;`
/// を `return x?.M() ?? 既定` にする。
pub(super) fn return_null_conditionals(body: &mut Vec<Stmt>) {
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if !fold_return_null(block, i) && !fold_assigned_return_null(block, i) && !fold_assigned_then_return(block, i) && !fold_null_if_else_assign(block, i) {
                i += 1;
            }
        }
    });
    fold_null_skips(body, &[]);
}

/// `if (x == null) { t = 既定; goto L; } t = x.M();` で、L が `t = x.M()` の流れ落ちる先なら `t = x?.M() ?? 既定`。
fn fold_null_skips(block: &mut Vec<Stmt>, following: &[Label]) {
    let mut i = 0;
    while i < block.len() {
        if !fold_null_skip(block, i, following) {
            i += 1;
        }
    }
    for index in 0..block.len() {
        let after = labels_after(block, index, following);
        for nested in block[index].blocks_mut() {
            fold_null_skips(nested, &after);
        }
    }
}

fn fold_null_skip(block: &mut Vec<Stmt>, i: usize, following: &[Label]) -> bool {
    if i + 1 >= block.len() {
        return false;
    }
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    if !otherwise.is_empty() || then.len() != 2 {
        return false;
    }
    let Some(tested) = is_null(condition).cloned() else { return false };
    let StmtKind::Assign(temp, fallback) = &then[0].kind else { return false };
    let StmtKind::Goto(label) = &then[1].kind else { return false };
    let StmtKind::Assign(again, access) = &block[i + 1].kind else { return false };
    if again != temp || !labels_after(block, i + 1, following).contains(label) {
        return false;
    }
    let (temp, fallback, access) = (temp.clone(), fallback.clone(), access.clone());
    let Some(value) = null_conditional_value(&tested, &access, &fallback) else { return false };
    let mut stmt = Stmt::new(StmtKind::Assign(temp, value), block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(&block[i + 1]);
    block.splice(i..=i + 1, [stmt]);
    true
}

/// `if (x == null) t = 既定; else t = x.M();` を `t = x?.M() ?? 既定` にする。次の文へはしまわない。
fn fold_null_if_else_assign(block: &mut Vec<Stmt>, i: usize) -> bool {
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    let ([Stmt { kind: StmtKind::Assign(left, when_null_side), .. }], [Stmt { kind: StmtKind::Assign(right, other), .. }]) = (then.as_slice(), otherwise.as_slice()) else { return false };
    if left != right {
        return false;
    }
    let (access, fallback) = if let Some(tested) = is_null(condition) {
        let Some(value) = null_conditional_value(tested, other, when_null_side) else { return false };
        (left.clone(), value)
    } else if let Some(tested) = not_null(condition) {
        let Some(value) = null_conditional_value(tested, when_null_side, other) else { return false };
        (left.clone(), value)
    } else {
        return false;
    };
    let mut stmt = Stmt::new(StmtKind::Assign(access, fallback), block[i].il);
    stmt.absorb(&block[i]);
    block[i] = stmt;
    true
}

/// `T.M(x)` が拡張メソッドのとき、`x != null ? T.M(x, …) : 既定` を `x?.M(…) ?? 既定` にする。
/// `if (x != null) T.M(x);` も `x?.M();` にする。拡張メソッドでない静的呼び出しはそのまま。
pub(super) fn extension_null_conditionals(context: &Context, body: &mut Vec<Stmt>) {
    for_each_block(body, &mut |block| {
        for stmt in block.iter_mut() {
            let StmtKind::If { condition, then, otherwise } = &stmt.kind else { continue };
            if !otherwise.is_empty() || then.len() != 1 {
                continue;
            }
            let Some(tested) = null_tested(condition) else { continue };
            let StmtKind::Expr(call) = &then[0].kind else { continue };
            let Some(lifted) = extension_call(context, call, &tested) else { continue };
            let inner = then[0].clone();
            let mut rewritten = Stmt::new(StmtKind::Expr(lifted), stmt.il);
            rewritten.absorb(stmt);
            rewritten.absorb(&inner);
            *stmt = rewritten;
        }
    });
    visit_expressions_mut(body, &mut |expr| {
        let Expr::Conditional(condition, when_true, when_false) = &expr.clone() else { return };
        let (tested, call, fallback) = if let Some(tested) = not_null(&condition) {
            (tested.clone(), when_true.clone(), when_false.clone())
        } else if let Some(tested) = is_null(&condition) {
            (tested.clone(), when_false.clone(), when_true.clone())
        } else {
            return;
        };
        let Some(lifted) = extension_call(context, call.as_ref(), &tested) else { return };
        *expr = if fallback.as_ref() == &Expr::Null { lifted } else { Expr::Coalesce(lifted.boxed(), fallback) };
    });
}

/// 最初の引数が `tested` である拡張メソッドの静的呼び出しを、`tested?.` の呼び出しにする。
fn extension_call(context: &Context, call: &Expr, tested: &Expr) -> Option<Expr> {
    let Expr::Call { method, target: None, args, virtual_call } = call else { return None };
    if args.first() != Some(tested) || args.iter().skip(1).any(|arg| mentions(arg, tested)) || !context.names.is_extension_method(*method) {
        return None;
    }
    Some(Expr::Call { method: *method, target: Some(Expr::NullConditional(tested.clone().boxed()).boxed()), args: args[1..].to_vec(), virtual_call: *virtual_call })
}

/// `if (x != null) x.M(…);` を `x?.M(…);` にする。本体がその呼び出しだけのとき。
pub(super) fn statement_null_conditional(block: &mut [Stmt]) {
    for stmt in block.iter_mut() {
        let StmtKind::If { condition, then, otherwise } = &stmt.kind else { continue };
        if !otherwise.is_empty() || then.len() != 1 {
            continue;
        }
        let Some(tested) = null_tested(condition) else { continue };
        let StmtKind::Expr(access) = &then[0].kind else { continue };
        let Some(lifted) = null_conditional_access(access, &tested, &tested) else { continue };
        let inner = then[0].clone();
        let mut rewritten = Stmt::new(StmtKind::Expr(lifted), stmt.il);
        rewritten.absorb(stmt);
        rewritten.absorb(&inner);
        *stmt = rewritten;
    }
}

/// `x != null` か `!(x == null)` の x。
fn null_tested(condition: &Expr) -> Option<Expr> {
    if let Expr::Not(inner) = condition {
        return is_null(inner).cloned();
    }
    not_null(condition).cloned()
}

fn fold_if_else_assign(context: Option<&Context>, block: &mut Vec<Stmt>, i: usize) -> bool {
    let (condition, left, a, right, b, slot) = {
        let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
        let Some((left, a)) = branch_value(then) else { return false };
        let Some((right, b)) = branch_value(otherwise) else { return false };
        let slot = materialized_slot(then).or_else(|| materialized_slot(otherwise));
        (condition.clone(), left, a, right, b, slot)
    };
    if left != right || slot.as_ref().is_some_and(|slot| local_reads(block, slot) != 1) {
        return false;
    }
    let (target, value) = (left, logic_value(condition, peel_nullable(context, a), peel_nullable(context, b)));
    let mut stmt = Stmt::new(StmtKind::Assign(target, value), block[i].il);
    stmt.absorb(&block[i]);
    block[i] = stmt;
    true
}

/// `new Nullable<T>(x.member)` は、null 側が null のときに `x?.member` と同じ値。
fn peel_nullable(context: Option<&Context>, expr: Expr) -> Expr {
    let Some(context) = context else { return expr };
    let Expr::New { method, args } = &expr else { return expr };
    if args.len() == 1 && context.names.method_owner(*method).is_some_and(|owner| context.names.is_nullable_type(owner)) {
        return args[0].clone();
    }
    expr
}

/// 一時変数をしまったあとの `if (x == null) { t = default; dest = t; } else { dest = x.member; }` を `dest = x?.member` にする。
pub(super) fn fold_member_assigns(context: &Context, body: &mut Vec<Stmt>) {
    for_each_block(body, &mut |block| {
        let mut i = 0;
        while i < block.len() {
            if !fold_if_else_assign(Some(context), block, i) {
                i += 1;
            }
        }
    });
    visit_expressions_mut(body, &mut |expr| {
        if let Some(value) = nullable_conditional(context, expr) {
            *expr = value;
        }
    });
}

/// `x == null ? null : new Nullable<T>(x.member)` を `x?.member` にする。
fn nullable_conditional(context: &Context, expr: &Expr) -> Option<Expr> {
    let Expr::Conditional(condition, when_true, when_false) = expr else { return None };
    let (tested, access, fallback) = if let Some(tested) = not_null(condition) {
        (tested, when_true.as_ref(), when_false.as_ref())
    } else {
        let tested = is_null(condition)?;
        (tested, when_false.as_ref(), when_true.as_ref())
    };
    let peeled = peel_nullable(Some(context), access.clone());
    if peeled == *access {
        return None;
    }
    null_conditional_value(tested, &peeled, fallback)
}

/// `if (c) { [if (skip) goto F;]… t = a; goto E; } [F:] t = b; E:`。
fn fold_goto_assign(block: &mut Vec<Stmt>, i: usize) -> bool {
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    if !otherwise.is_empty() {
        return false;
    }
    let Some((guards, target, when_true, end)) = split_guarded_assign(then) else { return false };
    let false_label = guards.first().map(|(_, label)| *label);
    if guards.iter().any(|(_, label)| Some(*label) != false_label) {
        return false;
    }
    let Some(end_at) = (i + 1..block.len()).find(|&k| matches!(block[k].kind, StmtKind::Label(label) if label == end)) else { return false };
    let mut region = &block[i + 1..end_at];
    if let Some(label) = false_label {
        let Some(Stmt { kind: StmtKind::Label(found), .. }) = region.first() else { return false };
        if *found != label || goto_count(block, label) != guards.len() {
            return false;
        }
        region = &region[1..];
    }
    let [Stmt { kind: StmtKind::Assign(assigned, when_false), .. }] = region else { return false };
    if assigned != &target {
        return false;
    }
    let mut condition = condition.clone();
    for (guard, _) in &guards {
        condition = Expr::AndAlso(condition.boxed(), Expr::Not(guard.clone().boxed()).boxed());
    }
    let value = logic_value(condition, when_true, when_false.clone());
    let mut stmt = Stmt::new(StmtKind::Assign(target, value), block[i].il);
    stmt.absorb(&block[i]);
    for absorbed in &block[i + 1..end_at] {
        stmt.absorb(absorbed);
    }
    block.splice(i..end_at, [stmt]);
    true
}

/// then は `if (guard) goto F` の並び、`t = 値`、`goto E`。
fn split_guarded_assign(then: &[Stmt]) -> Option<(Vec<(Expr, Label)>, Expr, Expr, Label)> {
    let mut guards = Vec::new();
    let mut rest = then;
    while let [Stmt { kind: StmtKind::If { condition, then, otherwise }, .. }, tail @ ..] = rest {
        let [Stmt { kind: StmtKind::Goto(label), .. }] = then.as_slice() else { break };
        if !otherwise.is_empty() {
            break;
        }
        guards.push((condition.clone(), *label));
        rest = tail;
    }
    let [Stmt { kind: StmtKind::Assign(target, value), .. }, Stmt { kind: StmtKind::Goto(end), .. }] = rest else { return None };
    Some((guards, target.clone(), value.clone(), *end))
}

fn goto_count(block: &[Stmt], label: Label) -> usize {
    let mut count = 0;
    visit(block, &mut |stmt| {
        if matches!(stmt.kind, StmtKind::Goto(found) if found == label) {
            count += 1;
        }
    });
    count
}

/// `if (x == null) return 既定; return x.M();`
fn fold_return_null(block: &mut Vec<Stmt>, i: usize) -> bool {
    if i + 1 >= block.len() {
        return false;
    }
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    if !otherwise.is_empty() {
        return false;
    }
    let [Stmt { kind: StmtKind::Return(Some(first)), .. }] = then.as_slice() else { return false };
    let StmtKind::Return(Some(second)) = &block[i + 1].kind else { return false };
    let (tested, access, fallback) = if let Some(tested) = is_null(condition) {
        (tested.clone(), second.clone(), first.clone())
    } else if let Some(tested) = null_tested(condition) {
        (tested, first.clone(), second.clone())
    } else {
        return false;
    };
    let Some(value) = null_conditional_value(&tested, &access, &fallback) else { return false };
    let mut stmt = Stmt::new(StmtKind::Return(Some(value)), block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(&block[i + 1]);
    block.splice(i..=i + 1, [stmt]);
    true
}

/// `if (x != null) { t = x.M(); return t; } t = 既定; return t;`
fn fold_assigned_then_return(block: &mut Vec<Stmt>, i: usize) -> bool {
    if i + 2 >= block.len() {
        return false;
    }
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    if !otherwise.is_empty() || then.len() != 2 {
        return false;
    }
    let StmtKind::Assign(temp, access) = &then[0].kind else { return false };
    let StmtKind::Return(Some(returned_then)) = &then[1].kind else { return false };
    if returned_then != temp {
        return false;
    }
    let StmtKind::Assign(after, fallback) = &block[i + 1].kind else { return false };
    let StmtKind::Return(Some(returned)) = &block[i + 2].kind else { return false };
    if after != temp || returned != temp {
        return false;
    }
    let Some(tested) = null_tested(condition) else { return false };
    let (access, fallback) = (access.clone(), fallback.clone());
    let Some(value) = null_conditional_value(&tested, &access, &fallback) else { return false };
    let mut stmt = Stmt::new(StmtKind::Return(Some(value)), block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(&block[i + 1]);
    stmt.absorb(&block[i + 2]);
    block.splice(i..=i + 2, [stmt]);
    true
}

/// `if (x != null) { t = x.M(); } else { t = 既定; return t; } return t;`
fn fold_assigned_return_null(block: &mut Vec<Stmt>, i: usize) -> bool {
    if i + 1 >= block.len() {
        return false;
    }
    let StmtKind::If { condition, then, otherwise } = &block[i].kind else { return false };
    let Some(tested) = null_tested(condition) else { return false };
    let [Stmt { kind: StmtKind::Assign(when_true, access), .. }] = then.as_slice() else { return false };
    let [Stmt { kind: StmtKind::Assign(when_false, fallback), .. }, Stmt { kind: StmtKind::Return(Some(returned_else)), .. }] = otherwise.as_slice() else { return false };
    let StmtKind::Return(Some(returned)) = &block[i + 1].kind else { return false };
    if when_true != when_false || returned_else != when_true || returned != when_true {
        return false;
    }
    let Some(value) = null_conditional_value(&tested, access, fallback) else { return false };
    let mut stmt = Stmt::new(StmtKind::Return(Some(value)), block[i].il);
    stmt.absorb(&block[i]);
    stmt.absorb(&block[i + 1]);
    block.splice(i..=i + 1, [stmt]);
    true
}

fn null_conditional_value(tested: &Expr, access: &Expr, fallback: &Expr) -> Option<Expr> {
    if access == tested && *fallback != Expr::Null {
        return Some(Expr::Coalesce(tested.clone().boxed(), fallback.clone().boxed()));
    }
    let lifted = null_conditional_access(access, tested, tested)?;
    Some(if *fallback == Expr::Null { lifted } else { Expr::Coalesce(lifted.boxed(), fallback.clone().boxed()) })
}

/// 両側が同じ変数への代入なら、その値。null 側が `T? t = default; 変数 = t` でも、値は null。
fn branch_value(stmts: &[Stmt]) -> Option<(Expr, Expr)> {
    match stmts {
        [Stmt { kind: StmtKind::Assign(target, value), .. }] => Some((target.clone(), value.clone())),
        [Stmt { kind: StmtKind::Declare { local, value }, .. }, Stmt { kind: StmtKind::Assign(target, Expr::Local(assigned)), .. }] if *local == *assigned && is_nullish(value) => Some((target.clone(), Expr::Null)),
        [Stmt { kind: StmtKind::Assign(Expr::Local(slot), value), .. }, Stmt { kind: StmtKind::Assign(target, Expr::Local(assigned)), .. }] if slot == assigned && is_nullish(value) => Some((target.clone(), Expr::Null)),
        _ => None,
    }
}

fn is_nullish(expr: &Expr) -> bool {
    matches!(expr, Expr::Default(_) | Expr::Null)
}

/// `t` を読む回数。代入先（`t = …`）は読みではない。
fn local_reads(body: &[Stmt], target: &Expr) -> usize {
    let mut count = 0;
    visit(body, &mut |stmt| {
        let StmtKind::Assign(dest, value) = &stmt.kind else {
            stmt.for_each_expression(&mut |expr| expr.walk(&mut |expr| count += (expr == target) as usize));
            return;
        };
        if dest != target {
            dest.walk(&mut |expr| count += (expr == target) as usize);
        }
        value.walk(&mut |expr| count += (expr == target) as usize);
    });
    count
}

/// `t = default; dest = t` の t。宣言の形は外から使えないので、調べない。
fn materialized_slot(stmts: &[Stmt]) -> Option<Expr> {
    match stmts {
        [Stmt { kind: StmtKind::Assign(slot @ Expr::Local(_), value), .. }, Stmt { kind: StmtKind::Assign(_, Expr::Local(assigned)), .. }] if matches!(slot, Expr::Local(local) if local == assigned) && is_nullish(value) => Some(slot.clone()),
        _ => None,
    }
}

fn logic_value(condition: Expr, when_true: Expr, when_false: Expr) -> Expr {
    if let Some(tested) = not_null(&condition).cloned() {
        if let Some(value) = null_conditional_value(&tested, &when_true, &when_false) {
            return value;
        }
    }
    if let Some(tested) = is_null(&condition).cloned() {
        if let Some(value) = null_conditional_value(&tested, &when_false, &when_true) {
            return value;
        }
    }
    simplify_logic(Expr::Conditional(condition.boxed(), when_true.boxed(), when_false.boxed()))
}

fn simplify_logic(expr: Expr) -> Expr {
    let expr = match expr {
        Expr::Conditional(c, a, b) => Expr::Conditional(simplify_logic(*c).boxed(), simplify_logic(*a).boxed(), simplify_logic(*b).boxed()),
        Expr::Not(inner) => Expr::Not(simplify_logic(*inner).boxed()),
        Expr::AndAlso(a, b) => Expr::AndAlso(simplify_logic(*a).boxed(), simplify_logic(*b).boxed()),
        Expr::OrElse(a, b) => Expr::OrElse(simplify_logic(*a).boxed(), simplify_logic(*b).boxed()),
        Expr::Coalesce(a, b) => Expr::Coalesce(simplify_logic(*a).boxed(), simplify_logic(*b).boxed()),
        other => other,
    };
    match expr {
        Expr::Not(inner) => match *inner {
            Expr::Not(value) => simplify_logic(*value),
            Expr::Bool(value) => Expr::Bool(!value),
            Expr::Compare(op @ (CmpOp::Eq | CmpOp::Ne), a, b) => Expr::Compare(op.negate(), a, b),
            Expr::AndAlso(a, b) => simplify_logic(Expr::OrElse(Expr::Not(a).boxed(), Expr::Not(b).boxed())),
            Expr::OrElse(a, b) => simplify_logic(Expr::AndAlso(Expr::Not(a).boxed(), Expr::Not(b).boxed())),
            other => Expr::Not(other.boxed()),
        },
        Expr::Conditional(c, a, b) => {
            if let Some(tested) = tested_of(&c) {
                if tested.is_simple() {
                    if let Some(value) = null_conditional_of(&Expr::Conditional(c.clone(), a.clone(), b.clone()), &tested, &tested) {
                        return simplify_logic(value);
                    }
                }
            }
            match (c, a, b) {
                (c, a, b) if *a == Expr::Bool(true) && *b == Expr::Bool(false) => simplify_logic(*c),
                (c, a, b) if *a == Expr::Bool(false) && *b == Expr::Bool(true) => simplify_logic(Expr::Not(c)),
                (c, a, b) if not_null(&c).is_some_and(|tested| *a == *tested) => Expr::Coalesce(a, b),
                (c, a, b) if is_null(&c).is_some_and(|tested| *b == *tested) => Expr::Coalesce(b, a),
                (c, a, b) if *b == Expr::Bool(false) && is_bool(&a) => simplify_logic(Expr::AndAlso(c, a)),
                (c, a, b) if *a == Expr::Bool(true) && is_bool(&b) => simplify_logic(Expr::OrElse(c, b)),
                (c, a, b) if *a == Expr::Bool(false) && is_bool(&b) => simplify_logic(Expr::AndAlso(Expr::Not(c).boxed(), b)),
                (c, a, b) if *b == Expr::Bool(true) && is_bool(&a) => simplify_logic(Expr::OrElse(Expr::Not(c).boxed(), a)),
                (c, a, b) => Expr::Conditional(c, a, b),
            }
        }
        other => other,
    }
}

fn tested_of(condition: &Expr) -> Option<Expr> {
    not_null(condition).or_else(|| is_null(condition)).cloned()
}

fn is_bool(expr: &Expr) -> bool {
    match expr {
        Expr::Bool(_) | Expr::Compare(..) | Expr::Not(_) | Expr::AndAlso(..) | Expr::OrElse(..) | Expr::Local(_) | Expr::Arg(_) | Expr::Temp(_) | Expr::Field { .. } | Expr::Property { .. } => true,
        Expr::Conditional(_, a, b) => is_bool(a) && is_bool(b),
        _ => false,
    }
}

/// 受け手が `temp` だけのメンバーアクセスを、`value?.` にする。
fn null_conditional_access(access: &Expr, temp: &Expr, value: &Expr) -> Option<Expr> {
    let receiver = Expr::NullConditional(Box::new(value.clone()));
    match access {
        Expr::Call { method, target: Some(target), args, virtual_call } if args.iter().all(|arg| !mentions(arg, temp)) => {
            let target = null_conditional_receiver(target, temp, value)?;
            Some(Expr::Call { method: *method, target: Some(Box::new(target)), args: args.clone(), virtual_call: *virtual_call })
        }
        Expr::Property { method, target: Some(target), args } if args.iter().all(|arg| !mentions(arg, temp)) => {
            let target = null_conditional_receiver(target, temp, value)?;
            Some(Expr::Property { method: *method, target: Some(Box::new(target)), args: args.clone() })
        }
        Expr::Field { token, target: Some(target) } => {
            let target = null_conditional_receiver(target, temp, value)?;
            Some(Expr::Field { token: *token, target: Some(Box::new(target)) })
        }
        Expr::Length(target) if target.as_ref() == temp => Some(Expr::Length(Box::new(receiver))),
        Expr::Element { array, index } if array.as_ref() == temp && !mentions(index, temp) => Some(Expr::Element { array: Box::new(receiver), index: index.clone() }),
        _ => None,
    }
}

/// 受け手が `temp` なら `value?.`。受け手が既に `temp` のメンバーの `?.` なら、その連鎖を `value?.` に伸ばす。
fn null_conditional_receiver(target: &Expr, temp: &Expr, value: &Expr) -> Option<Expr> {
    if target == temp {
        return Some(Expr::NullConditional(Box::new(value.clone())));
    }
    let Expr::NullConditional(inner) = target else { return None };
    Some(Expr::NullConditional(Box::new(null_conditional_access(inner, temp, value)?)))
}

/// `x = x + v` → `x += v`、`x = x + 1` → `x++`。
pub(super) fn compound(context: &Context, block: &mut [Stmt]) {
    for stmt in block.iter_mut() {
        let StmtKind::Assign(target, Expr::Binary(op, left, right)) = &stmt.kind else { continue };
        if !same_target(context, left, target) || !stable_target(target) {
            continue;
        }
        let one = matches!(**right, Expr::Int(1) | Expr::Long(1));
        stmt.kind = match op {
            BinOp::Add | BinOp::Sub if one => StmtKind::Increment { target: target.clone(), decrement: *op == BinOp::Sub },
            _ => StmtKind::Compound { target: target.clone(), op: *op, value: (**right).clone() },
        };
    }
}

/// `string.Concat(a, b, c)`（引数がすべて string のもの）→ `a + b + c`。
pub(super) fn concat(context: &Context, body: &mut Vec<Stmt>) {
    fn rewrite(context: &Context, expr: &mut Expr) {
        for child in expr.children_mut() {
            rewrite(context, child);
        }
        let Expr::Call { method, target: None, args, .. } = expr else { return };
        let Some(m) = context.names.method_ref(*method, &Default::default()) else { return };
        if m.name != "Concat" || m.declaring != "string" {
            return;
        }
        // 5 つ以上は、string の配列で渡している（`string.Concat(new string[] { … })`）。
        let parts: Vec<Expr> = match (args.as_slice(), m.sig.params.as_slice()) {
            ([Expr::ArrayInit { values, .. }], [Ty::SzArray(element)]) if **element == Ty::String && values.len() >= 2 => values.clone(),
            (_, params) if (2..=4).contains(&args.len()) && params.iter().all(|p| *p == Ty::String) => std::mem::take(args),
            _ => return,
        };
        let mut parts = parts.into_iter();
        let first = parts.next().unwrap_or(Expr::Str(String::new()));
        *expr = parts.fold(first, |left, right| Expr::Binary(BinOp::Add, left.boxed(), right.boxed()));
    }
    for_each_block(body, &mut |block| {
        for stmt in block.iter_mut() {
            for e in stmt.expressions_mut() {
                rewrite(context, e);
            }
        }
    });
}

/// 補間の形にする値: 箱詰めは書かなくても起きるので外す。
fn hole(value: &Expr, alignment: Option<i64>, format: Option<String>) -> Part {
    let value = match value {
        Expr::Box { value, .. } => (**value).clone(),
        other => other.clone(),
    };
    Part::Hole { value, alignment, format }
}

/// 続けて並んだ文字列の部分を 1 つにする。
fn push_text(parts: &mut Vec<Part>, text: &str) {
    match parts.last_mut() {
        Some(Part::Text(last)) => last.push_str(text),
        _ => parts.push(Part::Text(text.to_string())),
    }
}

/// 補間の書式に書ける文字か（`"`、`\`、括弧、改行は、補間の書式の中では意味が変わる）。
fn plain_format(format: &str) -> bool {
    !format.is_empty() && !format.contains(['"', '\\', '{', '}', '\n', '\r'])
}

/// `h = new DefaultInterpolatedStringHandler(n, m); h.AppendLiteral("a"); h.AppendFormatted(x); … h.ToStringAndClear() …` →
/// `… $"a{x}" …`（.NET 6 以降のコンパイラの形。h はここでしか使わない）。系列点は、まとめた先の文に残す。
pub(super) fn interpolated_handlers(context: &Context, block: &mut Vec<Stmt>) {
    let mut i = 0;
    while i + 1 < block.len() {
        let rewritten = (|| {
            let (handler, created) = match &block[i].kind {
                StmtKind::Assign(h @ (Expr::Local(_) | Expr::Temp(_)), created) => (h.clone(), created),
                StmtKind::Declare { local, value } => (Expr::Local(*local), value),
                _ => return None,
            };
            let Expr::New { method, .. } = created else { return None };
            if !context.names.method_ref(*method, &Default::default()).is_some_and(|m| m.declaring.ends_with("DefaultInterpolatedStringHandler")) {
                return None;
            }
            let on_handler = |target: &Expr| match target {
                Expr::AddressOf(inner) => **inner == handler,
                other => *other == handler,
            };
            let mut parts = Vec::new();
            let mut j = i + 1;
            while j < block.len() {
                let StmtKind::Expr(call @ Expr::Call { target: Some(target), args, .. }) = &block[j].kind else { break };
                if !on_handler(target) || args.iter().any(|a| mentions(a, &handler)) {
                    break;
                }
                match (call_name(context, call)?.0.as_str(), args.as_slice()) {
                    ("AppendLiteral", [Expr::Str(text)]) => push_text(&mut parts, text),
                    ("AppendFormatted", [value]) => parts.push(hole(value, None, None)),
                    ("AppendFormatted", [value, Expr::Int(alignment)]) => parts.push(hole(value, Some(*alignment), None)),
                    ("AppendFormatted", [value, Expr::Str(format)]) if plain_format(format) => parts.push(hole(value, None, Some(format.clone()))),
                    ("AppendFormatted", [value, Expr::Null]) => parts.push(hole(value, None, None)),
                    ("AppendFormatted", [value, Expr::Int(alignment), Expr::Str(format)]) if plain_format(format) => parts.push(hole(value, Some(*alignment), Some(format.clone()))),
                    _ => break,
                }
                j += 1;
            }
            if j >= block.len() || parts.is_empty() {
                return None;
            }
            // 作る文、足す文、文字列にする所のほかで、h を使っていない。
            if occurrences(block, &handler) != j - i + 1 {
                return None;
            }
            let value = Expr::Interpolated(parts);
            let mut next = block[j].clone();
            let mut replaced = 0;
            for e in next.expressions_mut() {
                e.walk_mut(&mut |e| {
                    let is_finish = matches!(e, Expr::Call { target: Some(target), args, .. } if args.is_empty() && on_handler(target)) && call_name(context, e).is_some_and(|(name, _)| name == "ToStringAndClear");
                    if is_finish {
                        *e = value.clone();
                        replaced += 1;
                    }
                });
            }
            if replaced != 1 {
                return None;
            }
            let mut stmt = Stmt::new(next.kind.clone(), block[i].il);
            for absorbed in &block[i..=j] {
                stmt.absorb(absorbed);
            }
            Some((stmt, j))
        })();
        if let Some((stmt, j)) = rewritten {
            block.splice(i..=j, [stmt]);
        }
        i += 1;
    }
}

/// `string.Format` の書式の部品: 文字列そのものか、`{番号,幅:書式}`。
#[derive(Debug, PartialEq)]
pub(super) enum FormatItem {
    Text(String),
    Hole { index: usize, alignment: Option<i64>, format: Option<String> },
}

/// `string.Format` の書式を読む（`{{` と `}}` は括弧そのもの）。読めないもの（閉じない括弧、対の無い `}`、補間に書けない
/// 書式）は `None`。元のコードなら実行のときに FormatException になるので、補間の形にはしない。
pub(super) fn parse_format(format: &str) -> Option<Vec<FormatItem>> {
    let mut items = Vec::new();
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' if chars.peek() == Some(&c) => {
                chars.next();
                text.push(c);
            }
            '{' => {
                let mut item = String::new();
                let closed = chars.by_ref().any(|c| {
                    if c == '}' {
                        return true;
                    }
                    item.push(c);
                    false
                });
                if !closed {
                    return None;
                }
                let (head, format) = match item.split_once(':') {
                    Some((head, format)) => (head, Some(format.to_string())),
                    None => (item.as_str(), None),
                };
                let (index, alignment) = match head.split_once(',') {
                    Some((index, alignment)) => (index, Some(alignment.trim().parse::<i64>().ok()?)),
                    None => (head, None),
                };
                if format.as_deref().is_some_and(|f| !plain_format(f)) {
                    return None;
                }
                if !text.is_empty() {
                    items.push(FormatItem::Text(std::mem::take(&mut text)));
                }
                items.push(FormatItem::Hole { index: index.trim().parse().ok()?, alignment, format });
            }
            '}' => return None,
            other => text.push(other),
        }
    }
    if !text.is_empty() {
        items.push(FormatItem::Text(text));
    }
    Some(items)
}
/// `string.Format("{0} has {1:F2}", a, b)`（.NET Framework のコンパイラの形）→ `$"{a} has {b:F2}"`。値の順に穴を書く書式
/// （値を読む順が変わらない）か、値に副作用が無いときだけ。
pub(super) fn format_interpolation(context: &Context, expr: &Expr) -> Option<Expr> {
    let Expr::Call { target: None, args, .. } = expr else { return None };
    let (name, declaring) = call_name(context, expr)?;
    if name != "Format" || !(declaring == "string" || declaring.ends_with(".String")) {
        return None;
    }
    let [Expr::Str(format), rest @ ..] = args.as_slice() else { return None };
    let values: Vec<&Expr> = match rest {
        [Expr::ArrayInit { values, .. }] => values.iter().collect(),
        // 引数が 1 つなら、`object[]` の変数を渡す（params の配列そのもの）呼び出しと見分けられない。型の分かる形だけ。
        [single @ (Expr::Box { .. } | Expr::Str(_))] => vec![single],
        many @ [_, _] | many @ [_, _, _] => many.iter().collect(),
        _ => return None,
    };
    let mut parts = Vec::new();
    let mut used = Vec::new();
    for item in parse_format(format)? {
        match item {
            FormatItem::Text(text) => push_text(&mut parts, &text),
            FormatItem::Hole { index, alignment, format } => {
                used.push(index);
                parts.push(hole(values.get(index)?, alignment, format));
            }
        }
    }
    // どの値も一度だけ使う。順が入れ替わるなら、どの値にも副作用が無い。
    let mut sorted = used.clone();
    sorted.sort_unstable();
    if sorted != (0..values.len()).collect::<Vec<_>>() {
        return None;
    }
    if used.windows(2).any(|w| w[0] > w[1]) && values.iter().any(|v| v.has_side_effects()) {
        return None;
    }
    Some(Expr::Interpolated(parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hole(index: usize, alignment: Option<i64>, format: Option<&str>) -> FormatItem {
        FormatItem::Hole { index, alignment, format: format.map(str::to_string) }
    }

    #[test]
    fn format_strings_are_split_into_text_and_holes() {
        assert_eq!(
            parse_format("{0} has {1,5:F2} {{x}}").unwrap(),
            vec![hole(0, None, None), FormatItem::Text(" has ".into()), hole(1, Some(5), Some("F2")), FormatItem::Text(" {x}".into())]
        );
        assert_eq!(parse_format("plain").unwrap(), vec![FormatItem::Text("plain".into())]);
    }

    #[test]
    fn format_strings_that_would_throw_are_not_read() {
        assert_eq!(parse_format("{0"), None, "閉じない括弧");
        assert_eq!(parse_format("a } b"), None, "対の無い閉じ括弧");
        assert_eq!(parse_format("{x}"), None, "番号でない");
        assert_eq!(parse_format("{0:a\"b}"), None, "補間に書けない書式");
    }
}
