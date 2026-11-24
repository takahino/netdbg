//! 捕まえた変数を持つラムダ（表示用のクラス `<>c__DisplayClass0_0`）を、C# の書き方に戻す（TODO 268）。
//!
//! コンパイラは、ラムダが捕まえた変数を表示用のクラスのフィールドにし、ラムダをそのクラスのメソッドにする。外のメソッドは
//! `d = new <>c__DisplayClass0_0(); d.limit = limit; d.seen = 0; … new Action<int>(d.<Captured>b__0) …` の形になる。
//!
//! ここでは、外のメソッドの `d.seen` を変数 `seen` に、`d.limit`（引数を写したもの）を引数 `limit` に、`d.<>4__this` を
//! `this` に戻し、作る文と写す文を消す。ラムダは、印字のときに式の中へ入れる（`csharp.rs` の `lambda_text`。表示用の
//! クラスの `this.seen` は `seen` と書ける）。表示用のクラスの型は、メソッドをすべて入れたら書かない（`writer.rs`）。
//!
//! 一つでも入れられないラムダがあると、外のメソッドが消した変数を指すので、すべて入れられると分かったときだけ戻す:
//! ラムダの本体を C# にでき、goto が無く、表示用のクラスの `this` をフィールドを読む所だけで使い、入れ子の表示用のクラス
//! （親を指すフィールド）やデリゲートの使い回し（`<>9__0`）が無いこと。
//!
//! 捕まえた変数は、表示用のクラスを作ったブロックで宣言する（ループの中で作るものは、回るたびに別の変数。外で宣言すると
//! すべてのラムダが 1 つの変数を共有してしまう）。デバッグ中に値を読めるよう、表示用のクラスのローカル変数は、合成した
//! PDB で `CS$<>8__locals0` と名付ける（式の評価は、その名前のローカル変数のフィールドを、変数として見せる。`value.rs`）。

use super::ast::{visit, visit_expressions, CmpOp, Expr, Stmt, StmtKind};
use super::body;
use super::csharp::prepare;
use super::ilview;
use super::names::{ident, Names};
use super::sig::Ty;
use crate::debugger::metadata::{FIELD, MEMBER_REF, METHOD_DEF, TYPE_DEF};
use std::collections::{HashMap, HashSet};

/// 戻した結果。
#[derive(Default)]
pub struct Lifted {
    /// 捕まえた変数にした、新しいローカル変数（番号と名前）。IL のローカル変数ではないので、合成した PDB には入れない。
    pub captured: Vec<(u16, String)>,
    /// 表示用のクラスを入れていたローカル変数（合成した PDB で `CS$<>8__locals0` と名付ける）。
    pub slots: Vec<u16>,
    /// 式の中へ入れるラムダのメソッドの行。
    pub lambdas: HashSet<u32>,
    /// それらのラムダの引数の名前（外のローカル変数の名前に使わない）。
    pub parameters: Vec<String>,
}

/// 表示用のクラスのフィールドの名前（`<>4__this` は外の `this`）。
const OUTER_THIS: &str = "<>4__this";

/// フィールドのトークン（FieldDef か、ジェネリックな型の MemberRef）が、表示用のクラス `owner` のものなら、その名前。
fn field_name<'a>(names: &'a Names, token: u32, owner: u32) -> Option<&'a str> {
    let (declaring, name) = field_of(names, token)?;
    (declaring == owner).then_some(name)
}

/// フィールドのトークンを、宣言している型の行と名前にほどく。ジェネリックな表示用のクラス（`<>c__DisplayClass33_0<T>`）の
/// フィールドは MemberRef で指す。
fn field_of<'a>(names: &'a Names, token: u32) -> Option<(u32, &'a str)> {
    let row = token & 0x00FF_FFFF;
    match (token >> 24) as usize {
        FIELD => {
            let (declaring, field) = names.md.field_declaration(row)?;
            Some((declaring, field.name))
        }
        MEMBER_REF => {
            let declaring = names.owner_type_row(token)?;
            let name = names.md.string(names.md.cell(MEMBER_REF, row, 1));
            names.md.fields(declaring).iter().any(|field| field.name == name).then_some((declaring, name))
        }
        _ => None,
    }
}

/// 捕まえた変数の型。MemberRef は、同じ名前の FieldDef の型を読む。
fn field_type(names: &Names, token: u32) -> Ty {
    let Some((declaring, name)) = field_of(names, token) else { return Ty::Object };
    names.md.fields(declaring).into_iter().find(|field| field.name == name).and_then(|field| names.field_def_type(field.token & 0x00FF_FFFF)).unwrap_or(Ty::Object)
}

/// 表示用のクラスのラムダ 1 つを、式の中に入れられるか。入れられるなら、その引数の名前。
fn lambda_parameters(names: &Names, image: &[u8], method: u32, owner: u32) -> Option<Vec<String>> {
    let md = names.md;
    let row = names.method_definition(method)?;
    if md.declaring_type(row) != Some(owner) || !md.string(md.cell(METHOD_DEF, row, 3)).contains(">b__") {
        return None;
    }
    let body = body::read(image, md.cell(METHOD_DEF, row, 0)).ok()?;
    let prepared = prepare(names, image, row, &body).ok()?;
    let mut ok = true;
    visit(&prepared.statements, &mut |s| ok &= !matches!(s.kind, StmtKind::Goto(_) | StmtKind::Label(_)));
    // 表示用のクラスの `this` は、そのフィールドを読む所だけ（入れ子のラムダや、`this` そのものを渡す所は戻せない）。
    let mut this_uses = 0;
    let mut field_uses = 0;
    visit_expressions(&prepared.statements, &mut |e| match e {
        Expr::Arg(0) => this_uses += 1,
        Expr::Field { target: Some(target), token } if **target == Expr::Arg(0) => {
            field_uses += 1;
            ok &= field_name(names, *token, owner).is_some_and(|name| name == OUTER_THIS || !name.contains("<>"));
        }
        Expr::NewDelegate { target: Some(target), .. } if **target == Expr::Arg(0) => ok = false,
        _ => {}
    });
    if !ok || this_uses != field_uses {
        return None;
    }
    let arguments = ilview::argument_names(names, row, &prepared.signature);
    Some(arguments.into_iter().skip(1).collect())
}

/// 表示用のクラスを作る文なら、それを入れる所（ローカル変数か、Release の評価スタックの値の一時変数）とクラスの行。
fn creation(names: &Names, stmt: &Stmt) -> Option<(Expr, u32)> {
    let (local, method) = match &stmt.kind {
        StmtKind::Assign(holder @ (Expr::Local(_) | Expr::Temp(_)), Expr::New { method, args }) if args.is_empty() => (holder.clone(), *method),
        StmtKind::Declare { local, value: Expr::New { method, args } } if args.is_empty() => (Expr::Local(*local), *method),
        _ => return None,
    };
    let owner = names.owner_type_row(method)?;
    names.md.string(names.md.cell(TYPE_DEF, owner, 1)).starts_with("<>c__DisplayClass").then_some((local, owner))
}

/// 表示用のクラス 1 つ分の調べた結果。
struct Candidate {
    /// 表示用のクラスを入れる所（`Expr::Local` か `Expr::Temp`）。
    holder: Expr,
    /// それを写しただけの一時変数（`t1 = d; … t1.form = x;`。代入の相手を先に読む IL の形）。
    aliases: Vec<Expr>,
    /// 使っているフィールド（トークン → 名前）。
    fields: HashMap<u32, String>,
    lambdas: HashSet<u32>,
    parameters: Vec<String>,
}

/// 表示用のクラスに置いたデリゲートの使い回し（`t = d.<>9__0; … if (t == null) { u = new D(d.method); d.<>9__0 = u; }`）を、
/// 作る式だけにする。このフィールドが残っていると、表示用のクラスを変数に戻せない。
fn strip_cached_delegates(names: &Names, body: &mut Vec<Stmt>) {
    fn walk(names: &Names, block: &mut Vec<Stmt>) {
        let mut i = 0;
        while i < block.len() {
            if let Some((end, mut replacement)) = cached_delegate_at(names, block, i) {
                if let Some(first) = replacement.first_mut() {
                    for absorbed in &block[i..=end] {
                        first.absorb(absorbed);
                    }
                }
                block.splice(i..=end, replacement);
                continue;
            }
            block[i].for_each_block_mut(&mut |inner| walk(names, inner));
            i += 1;
        }
    }
    walk(names, body);
}

fn is_delegate_cache(names: &Names, token: u32) -> bool {
    field_of(names, token).is_some_and(|(_, name)| name.starts_with("<>9__"))
}

fn cached_delegate_at(names: &Names, block: &[Stmt], i: usize) -> Option<(usize, Vec<Stmt>)> {
    let StmtKind::Assign(slot @ (Expr::Temp(_) | Expr::Local(_)), Expr::Field { target: Some(holder), token }) = &block[i].kind else { return None };
    if !is_delegate_cache(names, *token) {
        return None;
    }
    let slot = slot.clone();
    let holder = (**holder).clone();
    let mut j = i + 1;
    while j < block.len() && matches!(&block[j].kind, StmtKind::Assign(target, value) if target != &slot && (value == &slot || !expr_mentions(value, &slot))) {
        j += 1;
    }
    let StmtKind::If { condition, then, otherwise } = &block.get(j)?.kind else { return None };
    if !is_null_test(condition, &slot) || !otherwise.is_empty() {
        return None;
    }
    let outside = &block[i + 1..j];
    let (created_slot, created) = then.iter().find_map(|stmt| match &stmt.kind {
        StmtKind::Assign(dest @ (Expr::Temp(_) | Expr::Local(_)), created @ Expr::NewDelegate { target: Some(delegate_target), .. }) if same_holder(then, delegate_target, &holder) => Some((dest.clone(), created.clone())),
        _ => None,
    })?;
    let stores_cache = then.iter().any(|stmt| matches!(&stmt.kind, StmtKind::Assign(Expr::Field { target: Some(field_target), token: stored }, source) if *stored == *token && same_holder(then, field_target, &holder) && resolves_to(then, source, &created_slot)));
    if !stores_cache {
        return None;
    }
    let tail = &block[j + 1..];
    for stmt in then {
        let StmtKind::Assign(dest, source) = &stmt.kind else { return None };
        let cache_store = matches!(dest, Expr::Field { token: stored, .. } if *stored == *token);
        if source == &created || resolves_to(then, source, &created_slot) {
            if dest == &created_slot || cache_store {
                continue;
            }
            // 外でも同じ先へ、使い回す値を写している（分かれ道の両側へ渡す一時変数）。
            if outside.iter().any(|outer| matches!(&outer.kind, StmtKind::Assign(target, value) if target == dest && (value == &slot || resolves_to(outside, value, &slot)))) {
                continue;
            }
            // if の中だけで使う写し（`func = t2` を、フィールドへ戻す前に挟む）。
            if !expr_in(outside, dest) && !expr_in(tail, dest) {
                continue;
            }
            return None;
        }
        if source == &holder || resolves_to(then, source, &holder) {
            continue;
        }
        // 使い回しと関係の無い、評価スタックの写し（`list.Where` の list）。外の写しと同じ値であること。
        let Some(outer) = outside.iter().find_map(|outer| match &outer.kind {
            StmtKind::Assign(target, value) if target == dest => Some(value),
            _ => None,
        }) else {
            return None;
        };
        if resolve_assign(outside, outer) != resolve_assign(outside, source) {
            return None;
        }
    }
    if expr_in(tail, &slot) || expr_in(tail, &created_slot) {
        return None;
    }
    let replacement = block[i + 1..j].iter().map(|stmt| {
        let StmtKind::Assign(target, value) = &stmt.kind else { unreachable!() };
        let value = if value == &slot { created.clone() } else { value.clone() };
        Stmt::new(StmtKind::Assign(target.clone(), value), stmt.il)
    }).collect();
    Some((j, replacement))
}

fn is_null_test(condition: &Expr, slot: &Expr) -> bool {
    match condition {
        Expr::Compare(CmpOp::Eq, left, right) => (left.as_ref() == slot && **right == Expr::Null) || (right.as_ref() == slot && **left == Expr::Null),
        _ => false,
    }
}

/// `expr` が `holder` そのものか、`holder` を写した一時変数か。
fn same_holder(stmts: &[Stmt], expr: &Expr, holder: &Expr) -> bool {
    expr == holder || resolves_to(stmts, expr, holder)
}

/// `dest = source` をたどって、`expr` が `created` に行き着くか。
fn resolves_to(stmts: &[Stmt], expr: &Expr, created: &Expr) -> bool {
    let mut current = expr.clone();
    for _ in 0..stmts.len() + 1 {
        if &current == created {
            return true;
        }
        let Some(source) = stmts.iter().find_map(|stmt| match &stmt.kind {
            StmtKind::Assign(dest, source) if dest == &current => Some(source.clone()),
            _ => None,
        }) else {
            return false;
        };
        current = source;
    }
    false
}

/// 代入をたどった先。代入が無ければ `expr` のまま。
fn resolve_assign(stmts: &[Stmt], expr: &Expr) -> Expr {
    let mut current = expr.clone();
    for _ in 0..stmts.len() + 1 {
        let Some(source) = stmts.iter().find_map(|stmt| match &stmt.kind {
            StmtKind::Assign(dest, source) if dest == &current => Some(source.clone()),
            _ => None,
        }) else {
            return current;
        };
        current = source;
    }
    current
}

fn expr_mentions(expr: &Expr, target: &Expr) -> bool {
    let mut found = false;
    expr.walk(&mut |inner| found |= inner == target);
    found
}

fn expr_in(body: &[Stmt], target: &Expr) -> bool {
    let mut found = false;
    visit(body, &mut |stmt| stmt.for_each_expression(&mut |expr| expr.walk(&mut |inner| found |= inner == target)));
    found
}

/// 表示用のクラスを写しただけの一時変数（`t = d;`。代入の相手を先に読む IL の形）。一度だけの写しに加え、両側の分岐で
/// 写してからフィールドの相手にだけ使う一時変数（`t = d; t.field = v`）も含める。
fn aliases_of(body: &[Stmt], holder: &Expr) -> Vec<Expr> {
    let mut assigned: HashMap<u32, usize> = HashMap::new();
    visit(body, &mut |s| {
        if let StmtKind::Assign(Expr::Temp(t), _) = &s.kind {
            *assigned.entry(*t).or_insert(0) += 1;
        }
    });
    let mut aliases = Vec::new();
    visit(body, &mut |s| {
        if let StmtKind::Assign(alias @ Expr::Temp(t), value) = &s.kind {
            if value == holder && assigned.get(t) == Some(&1) {
                aliases.push(alias.clone());
            }
        }
    });
    // 写しの写し（両側で別の一時変数から受け、フィールドの相手にだけ使う）。
    loop {
        let mut sources_of: HashMap<u32, Vec<Expr>> = HashMap::new();
        let mut bare: HashMap<u32, usize> = HashMap::new();
        let mut receivers: HashMap<u32, usize> = HashMap::new();
        let mut targets: HashMap<u32, usize> = HashMap::new();
        visit(body, &mut |s| {
            if let StmtKind::Assign(Expr::Temp(t), value) = &s.kind {
                sources_of.entry(*t).or_default().push(value.clone());
                *targets.entry(*t).or_insert(0) += 1;
            }
            s.for_each_expression(&mut |e| {
                e.walk(&mut |e| match e {
                    Expr::Temp(t) => *bare.entry(*t).or_insert(0) += 1,
                    Expr::Field { target: Some(target), .. } | Expr::NewDelegate { target: Some(target), .. } => {
                        if let Expr::Temp(t) = target.as_ref() {
                            *receivers.entry(*t).or_insert(0) += 1;
                        }
                    }
                    _ => {}
                });
            });
        });
        let mut extra = Vec::new();
        for (temp, sources) in sources_of {
            let mentions = bare.get(&temp).copied().unwrap_or(0);
            let allowed = receivers.get(&temp).copied().unwrap_or(0) + targets.get(&temp).copied().unwrap_or(0);
            let from_holder = sources.iter().all(|e| e == holder || aliases.iter().any(|a| a == e));
            if mentions > allowed || aliases.iter().any(|a| *a == Expr::Temp(temp)) || sources.is_empty() || !from_holder {
                continue;
            }
            extra.push(temp);
        }
        if extra.is_empty() {
            break;
        }
        for temp in extra {
            aliases.push(Expr::Temp(temp));
        }
    }
    aliases
}

/// 写しの代入（両側が表示用のクラスかその写し）に出てくる数。`aliases.len() * 2` は、写しが一度だけのときにこれと一致する。
fn copy_mentions(body: &[Stmt], holder: &Expr, aliases: &[Expr]) -> usize {
    let is_holder = |e: &Expr| e == holder || aliases.contains(e);
    let mut count = 0;
    visit(body, &mut |s| {
        if let StmtKind::Assign(target, value) = &s.kind {
            if is_holder(target) && is_holder(value) {
                count += 2;
            }
        }
    });
    count
}

/// 表示用のクラス（と写し）の使われ方。
#[derive(Default)]
struct Uses {
    /// 式の中に出てくる数。
    total: usize,
    /// そのうち、フィールドを読み書きする所と、デリゲートを作る所の数（戻せる使い方）。
    allowed: usize,
    /// 作る文の数と、そのうち代入（`Assign`）で作るものの数。
    creations: usize,
    assigned_by_creation: usize,
    /// 戻せない使い方（入れ子の表示用のクラス、使い回しのデリゲート、宣言の形の再代入）があったか。
    refused: bool,
    /// 使っているフィールド（トークン → 名前）と、デリゲートにしたメソッド。
    fields: HashMap<u32, String>,
    delegates: HashSet<u32>,
}

/// 本体の中で、`holder`（と写し `aliases`）がどう使われているかを数える。
fn count_uses(names: &Names, body: &[Stmt], holder: &Expr, aliases: &[Expr], owner: u32) -> Uses {
    let local = match holder {
        Expr::Local(l) => Some(*l),
        _ => None,
    };
    let is_holder = |e: &Expr| e == holder || aliases.contains(e);
    let mut uses = Uses::default();
    visit(body, &mut |s| {
        if creation(names, s).is_some_and(|(h, _)| h == *holder) {
            uses.creations += 1;
        } else if matches!(&s.kind, StmtKind::Declare { local: l, .. } if Some(*l) == local) {
            uses.refused = true;
        }
        // 作る文の代入先（Assign のとき）も、式の中で 1 回数える。
        uses.assigned_by_creation += matches!(&s.kind, StmtKind::Assign(h, _) if h == holder) as usize;
        s.for_each_expression(&mut |e| {
            e.walk(&mut |e| match e {
                e if is_holder(e) => uses.total += 1,
                Expr::Field { target: Some(t), token } if is_holder(t) => {
                    uses.allowed += 1;
                    match field_name(names, *token, owner) {
                        Some(name) if name == OUTER_THIS || !name.contains("<>") => {
                            uses.fields.insert(*token, name.to_string());
                        }
                        _ => uses.refused = true,
                    }
                }
                Expr::NewDelegate { target: Some(t), method, .. } if is_holder(t) => {
                    uses.allowed += 1;
                    uses.delegates.insert(*method);
                }
                _ => {}
            });
        });
    });
    uses
}

/// `holder` に入れた表示用のクラスを、戻せるか調べる。作るのは 1 か所で、フィールドの読み書き、デリゲートを作る所、写す文
/// （`t = d;`。写し先と写し元で 2 回）のほかに使われず、どのラムダも式の中に入れられること。
fn examine(names: &Names, image: &[u8], body: &[Stmt], holder: &Expr, owner: u32) -> Option<Candidate> {
    let aliases = aliases_of(body, holder);
    let uses = count_uses(names, body, holder, &aliases, owner);
    let expected = uses.allowed + uses.assigned_by_creation + copy_mentions(body, holder, &aliases);
    if uses.refused || uses.creations != 1 || uses.assigned_by_creation > 1 || uses.total != expected || uses.delegates.is_empty() {
        return None;
    }
    let mut lambdas = HashSet::new();
    let mut parameters = Vec::new();
    for method in uses.delegates {
        parameters.extend(lambda_parameters(names, image, method, owner)?);
        lambdas.insert(names.method_definition(method)?);
    }
    Some(Candidate { holder: holder.clone(), aliases, fields: uses.fields, lambdas, parameters })
}
/// 外のメソッドの本体の、表示用のクラスを戻す。`arguments` は引数の名前、`locals` はローカル変数の型（捕まえた変数の分を
/// 足す）。
pub fn lift(names: &Names, image: &[u8], arguments: &[String], locals: &mut Vec<Ty>, body: &mut Vec<Stmt>) -> Lifted {
    // 捕まえたラムダの使い回し（`d.<>9__0`）を先に外す。残っていると、`<>` のフィールドとして戻せないと判断する。
    strip_cached_delegates(names, body);
    let mut lifted = Lifted::default();
    let mut found = Vec::new();
    visit(body, &mut |s| {
        if let Some(pair) = creation(names, s) {
            found.push(pair);
        }
    });
    for (holder, owner) in found {
        let Some(candidate) = examine(names, image, body, &holder, owner) else { continue };
        apply(names, arguments, locals, body, &candidate, &mut lifted);
    }
    lifted
}

/// 調べた表示用のクラスを、変数と引数に戻す。
fn apply(names: &Names, arguments: &[String], locals: &mut Vec<Ty>, body: &mut Vec<Stmt>, candidate: &Candidate, lifted: &mut Lifted) {
    let target = candidate.holder.clone();
    // 作る文のすぐあとの、引数（と外の this）を写す文。そのフィールドは、引数そのものにする。
    let mut mapped: HashMap<u32, Expr> = HashMap::new();
    find_copies(body, candidate, arguments, &mut mapped, names);
    // 残りのフィールドは、新しいローカル変数（捕まえた変数）。
    let mut tokens: Vec<(&u32, &String)> = candidate.fields.iter().filter(|(t, _)| !mapped.contains_key(t)).collect();
    tokens.sort();
    for (&token, name) in tokens {
        let index = locals.len() as u16;
        locals.push(field_type(names, token));
        lifted.captured.push((index, ident(name)));
        mapped.insert(token, Expr::Local(index));
    }
    remove_setup(body, candidate, &mapped);
    let is_holder = |e: &Expr| *e == target || candidate.aliases.contains(e);
    super::ast::visit_expressions_mut(body, &mut |e| match e {
        Expr::Field { target: Some(t), token } if is_holder(t) => {
            if let Some(replacement) = mapped.get(token) {
                *e = replacement.clone();
            }
        }
        // デリゲートの相手（表示用のクラス）は外す。印字では、ラムダとして式の中に入れる（`Lifted::lambdas`）。
        Expr::NewDelegate { target: t @ Some(_), .. } if t.as_deref().is_some_and(is_holder) => *t = None,
        _ => {}
    });
    if let Expr::Local(local) = target {
        lifted.slots.push(local);
    }
    lifted.lambdas.extend(&candidate.lambdas);
    lifted.parameters.extend(candidate.parameters.iter().cloned());
}

/// 作る文のすぐあとに並ぶ「`d.x = x;`（同じ名前の引数）」と「`d.<>4__this = this;`」を見つける。
fn find_copies(body: &[Stmt], candidate: &Candidate, arguments: &[String], mapped: &mut HashMap<u32, Expr>, names: &Names) {
    let target = candidate.holder.clone();
    let mut search = |block: &[Stmt]| {
        let Some(at) = block.iter().position(|s| creation(names, s).is_some_and(|(h, _)| h == target)) else { return };
        for stmt in &block[at + 1..] {
            let StmtKind::Assign(Expr::Field { target: Some(t), token }, Expr::Arg(index)) = &stmt.kind else { break };
            if **t != target {
                break;
            }
            let name = candidate.fields.get(token).map(String::as_str).unwrap_or("");
            let same = if *index == 0 && arguments.first().map(String::as_str) == Some("this") { name == OUTER_THIS } else { arguments.get(*index as usize).is_some_and(|a| *a == ident(name)) };
            if !same {
                break;
            }
            mapped.insert(*token, Expr::Arg(*index));
        }
    };
    search(body);
    visit(body, &mut |s| s.for_each_block(&mut |block| search(block)));
}

/// 作る文と、引数を写す文を消す（系列点は、次の文にまとめる）。捕まえた変数への最初の代入は、そのまま残す（宣言は、
/// あとで最初の代入にまとめる）。
fn remove_setup(body: &mut Vec<Stmt>, candidate: &Candidate, mapped: &HashMap<u32, Expr>) {
    let target = candidate.holder.clone();
    let is_setup = |stmt: &Stmt| match &stmt.kind {
        StmtKind::Assign(holder, Expr::New { .. }) => *holder == target,
        StmtKind::Assign(alias, value) if *value == target && candidate.aliases.contains(alias) => true,
        StmtKind::Declare { local, value: Expr::New { .. } } => Expr::Local(*local) == target,
        StmtKind::Assign(Expr::Field { target: Some(t), token }, value @ Expr::Arg(_)) => **t == target && mapped.get(token) == Some(value),
        _ => false,
    };
    fn remove_in(block: &mut Vec<Stmt>, is_setup: &dyn Fn(&Stmt) -> bool) {
        let mut i = 0;
        let mut carried: Vec<Stmt> = Vec::new();
        while i < block.len() {
            if is_setup(&block[i]) {
                carried.push(block.remove(i));
                continue;
            }
            for removed in carried.drain(..) {
                block[i].absorb(&removed);
            }
            block[i].for_each_block_mut(&mut |inner| remove_in(inner, is_setup));
            i += 1;
        }
        // 消した文のあとに文が無ければ、系列点は前の文にまとめる。
        if let Some(last) = block.last_mut() {
            for removed in carried.drain(..) {
                last.absorb(&removed);
            }
        }
    }
    remove_in(body, &is_setup);
}
