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
