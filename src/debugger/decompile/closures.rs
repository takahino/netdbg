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

use super::ast::{visit, visit_expressions, Expr, Stmt, StmtKind};
use super::body;
use super::csharp::prepare;
use super::ilview;
use super::names::{ident, Names};
use super::sig::Ty;
use crate::debugger::metadata::{FIELD, METHOD_DEF, TYPE_DEF};
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

/// フィールドのトークンが、表示用のクラス `owner` のものなら、その名前。
fn field_name<'a>(names: &'a Names, token: u32, owner: u32) -> Option<&'a str> {
    if (token >> 24) as usize != FIELD {
        return None;
    }
    let (declaring, field) = names.md.field_declaration(token & 0x00FF_FFFF)?;
    (declaring == owner).then_some(field.name)
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

/// 表示用のクラスを写しただけの一時変数（`t = d;`。代入の相手を先に読む IL の形）。ほかに代入されないものだけ。
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
    aliases
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
    let expected = uses.allowed + uses.assigned_by_creation + aliases.len() * 2;
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
        let row = token & 0x00FF_FFFF;
        let index = locals.len() as u16;
        locals.push(names.field_def_type(row).unwrap_or(Ty::Object));
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
