//! IL から、文の木の「平らな形」（ラベルと goto）を作る。
//!
//! 命令を順にたどり、評価スタックを式の木で模擬する。ブロックの境（分岐先、分岐のあと）でスタックに値が残るときは、
//! 受け取る側のブロックの一時変数（`Temp`）に代入して渡す。try / catch / finally は IL の節の入れ子のとおりに組む。
//! C# で書けない命令に出会ったら `Err` を返し、呼び出し側はそのメソッドを IL の形で書く。

use super::ast::{BinOp, Catch, CmpOp, Expr, Stmt, StmtKind};
use super::body::{Clause, ClauseKind};
use super::names::{MethodRef, Names};
use super::sig::{MethodSig, Ty};
use crate::debugger::il::{Instruction, Value};
use std::collections::BTreeSet;
use typing::{in_caller, is_boolean};

mod typing;

/// 一時変数の番号: 受け取るブロックの IL オフセットと、スタックの位置から作る（`dup` のものは別の範囲）。
fn block_temp(offset: u32, slot: usize) -> u32 {
    offset << 4 | slot.min(15) as u32
}

/// メソッド 1 つ分の材料。
pub struct Method<'a> {
    pub names: &'a Names<'a>,
    pub signature: &'a MethodSig,
    pub locals: &'a [Ty],
    pub code: &'a [Instruction],
    pub clauses: &'a [Clause],
    /// 命令ごとの、実行前のスタックの深さ（`flow::stack_depths`）。
    pub depths: &'a [Option<u32>],
    /// PE の像（配列の初期化子のデータを読む）。
    pub image: &'a [u8],
    /// インスタンスメソッドの `this` の型（宣言している型）。静的メソッドは `None`。
    pub this_type: Option<Ty>,
    /// インスタンスのコンストラクターか（`this` で呼ぶコンストラクターは、初期化子 `: base(…)` になる）。
    pub constructor: bool,
}

/// 作った本体と、一時変数の型（宣言に使う）。
pub struct Built {
    pub body: Vec<Stmt>,
    pub temps: Vec<(u32, Option<Ty>)>,
}

/// try とその節のまとまり（同じ try の範囲の節をまとめたもの）。
struct Protected {
    try_range: std::ops::Range<u32>,
    clauses: Vec<Clause>,
    /// まとまり全体の範囲（try の頭から、最後の節の終わりまで）。
    span: std::ops::Range<u32>,
}

pub fn build(method: &Method) -> Result<Built, String> {
    let mut groups: Vec<Protected> = Vec::new();
    for clause in method.clauses {
        let entry_start = if clause.kind == ClauseKind::Filter { clause.filter_start } else { clause.handler.start };
        match groups.iter_mut().find(|g| g.try_range == clause.try_range) {
            Some(group) => {
                group.span.end = group.span.end.max(clause.handler.end);
                group.span.start = group.span.start.min(entry_start);
                group.clauses.push(clause.clone());
            }
            None => groups.push(Protected { try_range: clause.try_range.clone(), clauses: vec![clause.clone()], span: clause.try_range.start..clause.handler.end.max(clause.try_range.end) }),
        }
    }
    let leaders = leaders(method);
    let mut builder = Builder { method, groups, leaders, temps: Vec::new(), next_dup: 0 };
    let end = method.code.last().map_or(0, Instruction::end);
    let mut body = builder.range(0..end, Vec::new())?;
    builder.coerce_temps(&mut body);
    let temps = builder.temps;
    Ok(Built { body, temps })
}

/// ブロックの頭: 分岐先、分岐・戻り・投げのあと、try と節の頭と終わり。
fn leaders(method: &Method) -> BTreeSet<u32> {
    let mut set = BTreeSet::new();
    set.insert(0);
    for instruction in method.code {
        set.extend(instruction.targets().iter().copied());
        use crate::debugger::il::Flow::*;
        if matches!(instruction.op.flow, Branch | Conditional | Switch | Leave | Return | Throw | EndHandler) {
            set.insert(instruction.end());
        }
    }
    for clause in method.clauses {
        set.extend([clause.try_range.start, clause.try_range.end, clause.handler.start, clause.handler.end]);
        if clause.kind == ClauseKind::Filter {
            set.insert(clause.filter_start);
        }
    }
    set
}

struct Builder<'a> {
    method: &'a Method<'a>,
    groups: Vec<Protected>,
    leaders: BTreeSet<u32>,
    temps: Vec<(u32, Option<Ty>)>,
    next_dup: u32,
}

/// 文を足す先と、いまの IL の文の頭（系列点）。
struct Out {
    stmts: Vec<Stmt>,
    group: Option<u32>,
}

impl Out {
    fn push(&mut self, kind: StmtKind) {
        self.stmts.push(Stmt::new(kind, self.group));
    }
}

impl<'a> Builder<'a> {
    fn names(&self) -> &'a Names<'a> {
        self.method.names
    }

    fn index_of(&self, offset: u32) -> Option<usize> {
        self.method.code.binary_search_by_key(&offset, |i| i.offset).ok()
    }

    /// 範囲 `range` の文。`entry` は、範囲の頭で積まれている値（catch の例外）。
    fn range(&mut self, range: std::ops::Range<u32>, entry: Vec<Expr>) -> Result<Vec<Stmt>, String> {
        let mut out = Out { stmts: Vec::new(), group: None };
        let mut stack = entry;
        let mut offset = range.start;
        let mut falls_through = true;
        while offset < range.end {
            // この範囲の直下の try のまとまり（入れ子の外側のもの）。
            if let Some(index) = self.child_group(&range, offset) {
                self.spill_for(offset, &mut stack, &mut out);
                let span = self.groups[index].span.clone();
                let stmt = self.protected(index)?;
                out.stmts.push(Stmt::new(StmtKind::Label(offset), None));
                out.stmts.push(stmt);
                offset = span.end;
                falls_through = true;
                continue;
            }
            let index = self.index_of(offset).ok_or_else(|| tr!("There is no instruction at {label}", label = format!("IL_{offset:04x}")))?;
            let instruction = &self.method.code[index];
            if self.leaders.contains(&offset) {
                // 範囲の頭では、渡された値（catch の例外）をそのまま使う。
                if offset != range.start {
                    if falls_through {
                        self.spill_for(offset, &mut stack, &mut out);
                    }
                    stack = self.entry_stack(offset, index);
                }
                out.stmts.push(Stmt::new(StmtKind::Label(offset), None));
            }
            if self.method.depths[index] == Some(0) {
                out.group = Some(offset);
            }
            falls_through = self.instruction(instruction, &mut stack, &mut out)?;
            offset = instruction.end();
        }
        Ok(out.stmts)
    }

    /// ブロックの頭で、前のブロックから受け取る値（一時変数）。たどり着かない命令は空。
    fn entry_stack(&mut self, offset: u32, index: usize) -> Vec<Expr> {
        let depth = self.method.depths[index].unwrap_or(0) as usize;
        (0..depth).map(|slot| Expr::Temp(block_temp(offset, slot))).collect()
    }

    /// ブロック `target` へ進む前に、残っている値をその一時変数に入れる。
    fn spill_for(&mut self, target: u32, stack: &mut Vec<Expr>, out: &mut Out) {
        for (slot, value) in std::mem::take(stack).into_iter().enumerate() {
            let temp = block_temp(target, slot);
            if value == Expr::Temp(temp) {
                continue;
            }
            self.note_temp(temp, &value);
            out.push(StmtKind::Assign(Expr::Temp(temp), value));
        }
    }

    /// 分岐する前に、分岐先へ渡す値を入れる。値は一度だけ評価するよう、単純でなければ先に一時変数にする。
    fn spill_for_branch(&mut self, targets: &[u32], stack: &mut [Expr], out: &mut Out) {
        if let [target] = targets {
            // 飛び先が一つなら、値は一度しか使わないので、そのまま渡す。
            let mut values = stack.to_vec();
            self.spill_for(*target, &mut values, out);
            return;
        }
        for value in stack.iter_mut() {
            if !value.is_simple() {
                let temp = self.dup_temp(value, out);
                *value = Expr::Temp(temp);
            }
        }
        for &target in targets {
            for (slot, value) in stack.iter().enumerate() {
                let temp = block_temp(target, slot);
                if *value != Expr::Temp(temp) {
                    self.note_temp(temp, value);
                    out.push(StmtKind::Assign(Expr::Temp(temp), value.clone()));
                }
            }
        }
    }

    /// 値を新しい一時変数に入れる（`dup`、分岐の前）。
    fn dup_temp(&mut self, value: &Expr, out: &mut Out) -> u32 {
        // ブロックの一時変数（offset << 4）と重ならないよう、上の範囲を使う。
        let temp = 0xF000_0000 | self.next_dup;
        self.next_dup += 1;
        self.note_temp(temp, value);
        out.push(StmtKind::Assign(Expr::Temp(temp), value.clone()));
        temp
    }

    fn note_temp(&mut self, temp: u32, value: &Expr) {
        if !self.temps.iter().any(|(t, _)| *t == temp) {
            let ty = self.type_of(value);
            self.temps.push((temp, ty));
        }
    }

    /// 範囲の中で `offset` から始まる、直下の try のまとまり。
    fn child_group(&self, range: &std::ops::Range<u32>, offset: u32) -> Option<usize> {
        // まとまりの範囲は、いまの範囲と同じでもよい（`try { try … catch … } finally` の内側は、外側の try の本体と同じ範囲）。
        // いま組んでいる try の本体そのもの（try の範囲が同じもの）は除く。
        let inside = |span: &std::ops::Range<u32>| span.start >= range.start && span.end <= range.end;
        let candidates = (0..self.groups.len()).filter(|&i| self.groups[i].span.start == offset && inside(&self.groups[i].span) && self.groups[i].try_range != *range);
        // 同じ位置から始まるものがいくつもあれば、いちばん外側（広いもの）。
        candidates.max_by_key(|&i| self.groups[i].span.end)
    }

    /// try のまとまり 1 つを、`Try` の文にする。
    fn protected(&mut self, index: usize) -> Result<Stmt, String> {
        let try_range = self.groups[index].try_range.clone();
        let clauses = self.groups[index].clauses.clone();
        let body = self.range(try_range.clone(), Vec::new())?;
        let (mut catches, mut finally, mut fault) = (Vec::new(), None, None);
        for clause in &clauses {
            match clause.kind {
                ClauseKind::Catch => {
                    let handler = self.range(clause.handler.clone(), vec![Expr::Exception])?;
                    let (variable, handler) = catch_variable(handler);
                    catches.push(Catch { ty: clause.class_token, variable, filter: None, body: handler });
                }
                ClauseKind::Filter => return Err(tr!("catch when (a filter) cannot be turned back into C# yet")),
                ClauseKind::Finally => finally = Some(self.range(clause.handler.clone(), Vec::new())?),
                ClauseKind::Fault => fault = Some(self.range(clause.handler.clone(), Vec::new())?),
            }
        }
        Ok(Stmt::new(StmtKind::Try { body, catches, finally, fault }, None))
    }

    /// 命令 1 つ。次の命令へ流れるなら `true`。
    fn instruction(&mut self, instruction: &Instruction, stack: &mut Vec<Expr>, out: &mut Out) -> Result<bool, String> {
        let name = instruction.op.name;
        let pop = |stack: &mut Vec<Expr>| stack.pop().ok_or_else(|| tr!("The stack is empty at {label} ({name})", label = format!("IL_{:04x}", instruction.offset), name = name));
        let token = match instruction.value {
            Value::Token(token) => token,
            _ => 0,
        };
        let var = match instruction.value {
            Value::Var(index) => index,
            _ => 0,
        };
        // ldarg.0 などの番号つきの形。
        let numbered = |prefix: &str| name.strip_prefix(prefix).and_then(|n| n.parse::<u16>().ok());
        match name {
            "nop" | "break" | "volatile." | "readonly." | "tail." | "unaligned." | "no." | "constrained." => {}
            _ if numbered("ldarg.").is_some() => stack.push(Expr::Arg(numbered("ldarg.").unwrap_or(0))),
            _ if numbered("ldloc.").is_some() => stack.push(Expr::Local(numbered("ldloc.").unwrap_or(0))),
            _ if numbered("stloc.").is_some() => {
                let value = pop(stack)?;
                self.assign(Expr::Local(numbered("stloc.").unwrap_or(0)), value, stack, out);
            }
            "ldarg.s" | "ldarg" => stack.push(Expr::Arg(var)),
            "ldloc.s" | "ldloc" => stack.push(Expr::Local(var)),
            "ldarga.s" | "ldarga" => stack.push(Expr::AddressOf(Expr::Arg(var).boxed())),
            "ldloca.s" | "ldloca" => stack.push(Expr::AddressOf(Expr::Local(var).boxed())),
            "starg.s" | "starg" => {
                let value = pop(stack)?;
                self.assign(Expr::Arg(var), value, stack, out);
            }
            "stloc.s" | "stloc" => {
                let value = pop(stack)?;
                self.assign(Expr::Local(var), value, stack, out);
            }
            "ldnull" => stack.push(Expr::Null),
            "ldc.i4.m1" => stack.push(Expr::Int(-1)),
            _ if numbered("ldc.i4.").is_some() => stack.push(Expr::Int(numbered("ldc.i4.").unwrap_or(0) as i64)),
            "ldc.i4.s" | "ldc.i4" => stack.push(Expr::Int(int(&instruction.value))),
            "ldc.i8" => stack.push(Expr::Long(int(&instruction.value))),
            "ldc.r4" => stack.push(Expr::Float(float(&instruction.value))),
            "ldc.r8" => stack.push(Expr::Double(float(&instruction.value))),
            "ldstr" => stack.push(Expr::Str(self.names().md.user_string(token).unwrap_or_default())),
            "dup" => {
                let value = pop(stack)?;
                if value.is_simple() {
                    stack.push(value.clone());
                    stack.push(value);
                } else {
                    let temp = self.dup_temp(&value, out);
                    stack.push(Expr::Temp(temp));
                    stack.push(Expr::Temp(temp));
                }
            }
            "pop" => {
                let value = pop(stack)?;
                if value.is_statement() {
                    self.statement(StmtKind::Expr(value), stack, out);
                } else if has_call(&value) {
                    self.statement(StmtKind::Assign(Expr::Literal("_".into()), value), stack, out);
                }
            }
            "add" | "sub" | "mul" | "div" | "rem" | "and" | "or" | "xor" | "shl" | "shr" => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                // 列挙型と整数の定数のビット演算（`color & 4`）は、定数を列挙型にする。足し引き（範囲の判定の`x - 13`）は、
                // 列挙型を整数にして計算する（C# では、列挙型どうしの引き算は整数になり、比べる相手と型が合わない）。
                let op = binop(name);
                let (a, b) = if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) && (self.is_value_type(&a) || self.is_value_type(&b)) {
                    self.matched(a, b)
                } else if matches!(op, BinOp::Add | BinOp::Sub) && self.is_value_type(&a) {
                    (Expr::Convert { to: Ty::I4, checked: false, value: a.boxed() }, b)
                } else {
                    (a, b)
                };
                stack.push(Expr::Binary(op, a.boxed(), b.boxed()));
            }
            "div.un" | "rem.un" | "shr.un" => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                let op = binop(name.trim_end_matches(".un"));
                stack.push(Expr::Binary(op, self.unsigned(a).boxed(), if op == BinOp::Shr { b.boxed() } else { self.unsigned(b).boxed() }));
            }
            "add.ovf" | "sub.ovf" | "mul.ovf" | "add.ovf.un" | "sub.ovf.un" | "mul.ovf.un" => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                stack.push(Expr::Checked(binop(name.split('.').next().unwrap_or("add")), a.boxed(), b.boxed()));
            }
            "neg" => {
                let value = pop(stack)?;
                stack.push(Expr::Neg(value.boxed()));
            }
            "not" => {
                let value = pop(stack)?;
                stack.push(Expr::BitNot(value.boxed()));
            }
            "ceq" | "cgt" | "clt" | "cgt.un" | "clt.un" => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                stack.push(self.compare(name, a, b));
            }
            _ if name.starts_with("conv.") => {
                let value = pop(stack)?;
                let converted = convert(name, value).ok_or_else(|| tr!("{name} cannot be turned back into C#", name = name))?;
                stack.push(fold_constant(converted));
            }
            "br" | "br.s" => {
                let target = instruction.targets()[0];
                self.spill_for_branch(&[target], stack, out);
                stack.clear();
                out.push(StmtKind::Goto(target));
                return Ok(false);
            }
            "leave" | "leave.s" => {
                stack.clear();
                out.push(StmtKind::Goto(instruction.targets()[0]));
                return Ok(false);
            }
            "brtrue" | "brtrue.s" | "brfalse" | "brfalse.s" => {
                let value = pop(stack)?;
                let condition = if name.starts_with("brtrue") { self.truthy(value) } else { self.negate(self.truthy(value)) };
                self.branch(instruction, condition, stack, out);
            }
            _ if is_compare_branch(name) => {
                let (b, a) = (pop(stack)?, pop(stack)?);
                let condition = self.branch_condition(name, a, b);
                self.branch(instruction, condition, stack, out);
            }
            "switch" => {
                let value = pop(stack)?;
                let targets = instruction.targets().to_vec();
                let mut all = targets.clone();
                all.push(instruction.end());
                self.spill_for_branch(&all, stack, out);
                stack.clear();
                let ty = self.type_of(&value);
                out.push(StmtKind::Switch { value, targets, ty });
            }
            "ret" => {
                let value = if self.method.signature.ret.is_void() { None } else { Some(pop(stack)?) };
                let value = value.map(|v| self.coerce(v, &self.method.signature.ret.clone()));
                out.push(StmtKind::Return(value));
                return Ok(false);
            }
            "throw" => {
                let value = pop(stack)?;
                out.push(StmtKind::Throw(Some(value)));
                return Ok(false);
            }
            "rethrow" => {
                out.push(StmtKind::Throw(None));
                return Ok(false);
            }
            "endfinally" => return Ok(false),
            "call" | "callvirt" => self.call(token, name == "callvirt", stack, out)?,
            "newobj" => self.new_object(token, stack)?,
            "ldfld" => {
                let target = pop(stack)?;
                stack.push(Expr::Field { target: Some(target.boxed()), token });
            }
            "ldflda" => {
                let target = pop(stack)?;
                stack.push(Expr::AddressOf(Expr::Field { target: Some(target.boxed()), token }.boxed()));
            }
            "stfld" => {
                let (value, target) = (pop(stack)?, pop(stack)?);
                self.assign(Expr::Field { target: Some(target.boxed()), token }, value, stack, out);
            }
            "ldsfld" => stack.push(Expr::Field { target: None, token }),
            "ldsflda" => stack.push(Expr::AddressOf(Expr::Field { target: None, token }.boxed())),
            "stsfld" => {
                let value = pop(stack)?;
                self.assign(Expr::Field { target: None, token }, value, stack, out);
            }
            _ if name.starts_with("ldelem") && name != "ldelema" => {
                let (index, array) = (self.array_index(pop(stack)?), pop(stack)?);
                stack.push(Expr::Element { array: array.boxed(), index: index.boxed() });
            }
            "ldelema" => {
                let (index, array) = (self.array_index(pop(stack)?), pop(stack)?);
                stack.push(Expr::AddressOf(Expr::Element { array: array.boxed(), index: index.boxed() }.boxed()));
            }
            _ if name.starts_with("stelem") => {
                let (value, index, array) = (pop(stack)?, self.array_index(pop(stack)?), pop(stack)?);
                self.assign(Expr::Element { array: array.boxed(), index: index.boxed() }, value, stack, out);
            }
            "ldlen" => {
                let array = pop(stack)?;
                stack.push(Expr::Length(array.boxed()));
            }
            "newarr" => {
                let size = pop(stack)?;
                stack.push(Expr::NewArray { element: token, size: size.boxed() });
            }
            "castclass" | "unbox.any" => {
                let value = pop(stack)?;
                stack.push(Expr::Cast { ty: token, value: value.boxed() });
            }
            "isinst" => {
                let value = pop(stack)?;
                stack.push(Expr::As { ty: token, value: value.boxed() });
            }
            "box" => {
                let value = pop(stack)?;
                stack.push(Expr::Box { ty: token, value: value.boxed() });
            }
            _ if name.starts_with("ldind.") || name == "ldobj" => {
                let address = pop(stack)?;
                stack.push(deref(address));
            }
            _ if name.starts_with("stind.") || name == "stobj" => {
                let (value, address) = (pop(stack)?, pop(stack)?);
                self.assign(deref(address), value, stack, out);
            }
            "initobj" => {
                let address = pop(stack)?;
                let ty = Ty::Named { token, value_type: true };
                self.statement(StmtKind::Assign(deref(address), Expr::Default(ty)), stack, out);
            }
            "ldtoken" => stack.push(Expr::Token(token)),
            "sizeof" => stack.push(Expr::SizeOf(token)),
            "ldftn" => stack.push(Expr::FunctionPointer(token)),
            "ldvirtftn" => {
                pop(stack)?;
                stack.push(Expr::FunctionPointer(token));
            }
            other => return Err(tr!("{name} cannot be turned back into C# yet", name = other)),
        }
        // 前置の命令（constrained. など）は、次の命令とまとめて扱う。
        Ok(true)
    }

    /// 条件つきの分岐: 分岐先と次の命令へ渡す値を入れてから `if (条件) goto 先;`。渡し終えたので、スタックは空にする
    /// （次の命令のブロックは、渡した一時変数から始まる）。
    fn branch(&mut self, instruction: &Instruction, condition: Expr, stack: &mut Vec<Expr>, out: &mut Out) {
        let target = instruction.targets()[0];
        self.spill_for_branch(&[target, instruction.end()], stack, out);
        stack.clear();
        out.push(StmtKind::If { condition, then: vec![Stmt::new(StmtKind::Goto(target), None)], otherwise: Vec::new() });
    }

    /// 代入。代入先の型に合わせて、定数を直す（`1` を `true` に、など）。
    fn assign(&mut self, target: Expr, value: Expr, stack: &mut [Expr], out: &mut Out) {
        let value = match self.type_of(&target) {
            Some(ty) => self.coerce(value, &ty),
            None => value,
        };
        self.statement(StmtKind::Assign(target, value), stack, out);
    }

    /// 文を出す。スタックに残っている式は、文より前に評価されるはずのもの（`f(x) + (x = 1)` の左側など）なので、先に
    /// 一時変数に入れて順番を保つ。定数と、ローカル変数のアドレス（IL でも同じ場所を指したまま）は、そのままでよい。
    fn statement(&mut self, kind: StmtKind, stack: &mut [Expr], out: &mut Out) {
        for value in stack.iter_mut() {
            // クラスの `this` は書き換えられないので、そのまま残す（一時変数に入れると、読み取り専用のフィールドへの
            // 代入がコンストラクターの外の形になる）。
            let class_this = *value == Expr::Arg(0) && matches!(self.method.this_type, Some(Ty::Named { value_type: false, .. } | Ty::Generic { value_type: false, .. }));
            let stable = class_this
                || matches!(value, Expr::Int(_) | Expr::Long(_) | Expr::Float(_) | Expr::Double(_) | Expr::Str(_) | Expr::Null | Expr::Bool(_) | Expr::Char(_) | Expr::Literal(_) | Expr::Temp(_))
                || matches!(value, Expr::AddressOf(inner) if matches!(**inner, Expr::Local(_) | Expr::Arg(_)));
            if !stable {
                let temp = self.dup_temp(value, out);
                *value = Expr::Temp(temp);
            }
        }
        out.push(kind);
    }

    /// 呼び出し（`call` / `callvirt`）。プロパティのアクセサーと演算子は、C# の書き方にする。
    fn call(&mut self, token: u32, virtual_call: bool, stack: &mut Vec<Expr>, out: &mut Out) -> Result<(), String> {
        let names = self.names();
        let method = names.method_ref(token, &Default::default()).ok_or_else(|| tr!("Cannot read the callee {token}", token = format!("0x{token:08X}")))?;
        let mut args = Vec::with_capacity(method.sig.params.len());
        for _ in 0..method.sig.params.len() {
            args.push(stack.pop().ok_or_else(|| tr!("The call has too few arguments"))?);
        }
        args.reverse();
        let args: Vec<Expr> = args.into_iter().zip(method.sig.params.clone()).map(|(a, ty)| match in_caller(names, token, ty) {
            Some(ty) => self.coerce(a, &ty),
            None => a,
        }).collect();
        let mut target = if method.sig.has_this && !method.sig.explicit_this { Some(stack.pop().ok_or_else(|| tr!("The call has no this"))?) } else { None };
        // 配列に対するインターフェイスのメソッド（`IEnumerable<T>.GetEnumerator` など）は、配列が明示的に実装しているので、
        // 受け手をインターフェイスにキャストして呼ぶ（そのままでは、配列の非ジェネリックのメソッドに結び付く）。
        // 型が自分で実装しているインターフェイスのメソッドを、型の値で呼ぶ所（明示的な実装を呼ぶため、元のコードでキャスト
        // していた所）も同じ。
        if let (Some(receiver), Some(owner)) = (&target, names.method_owner(token)) {
            let cast = match self.type_of(receiver) {
                Some(Ty::SzArray(_)) => !matches!(method.declaring.as_str(), "Array" | "object"),
                Some(Ty::Named { token: own, .. } | Ty::Generic { token: own, .. }) if own >> 24 == 0x02 && own != owner => names.implements_directly(own & 0x00FF_FFFF, owner),
                _ => false,
            };
            if cast {
                target = Some(Expr::Cast { ty: owner, value: receiver.clone().boxed() });
            }
        }
        // イベントの add / remove: `x.Changed += h;`（`add_Changed(h)` は C# で直接呼べない）。
        if args.len() == 1 && names.is_event_accessor(token) {
            let op = if method.name.starts_with("add_") { BinOp::Add } else { BinOp::Sub };
            let event = Expr::Property { target: target.map(Box::new), method: token, args: Vec::new() };
            let value = args.into_iter().next().unwrap_or(Expr::Null);
            self.statement(StmtKind::Compound { target: event, op, value }, stack, out);
            return Ok(());
        }
        // 配列の初期化子: `new int[4]; RuntimeHelpers.InitializeArray(a, <データのフィールド>)` → `new int[] { … }`。
        if method.name == "InitializeArray" && method.declaring == "RuntimeHelpers" {
            // 戻せなければ、正しくない C# を書かずに IL に落とす（データのフィールドは C# で書けない）。
            return if self.array_initializer(&args, out) { Ok(()) } else { Err(tr!("Cannot read the data of the array initializer")) };
        }
        // 値型のコンストラクターの呼び出し（`call instance void S::.ctor` の this がアドレス）は、`x = new S(…)`。
        if method.name == ".ctor" {
            if let Some(target) = target {
                // コンストラクターの中で `this` に呼ぶもの（基底か、同じ型のほかのコンストラクター）は、呼び出しの文に
                // しておき、本体を書くときに初期化子 `: base(…)` へ移す（`csharp::take_initializer`）。
                if matches!(target, Expr::Arg(0)) && self.method.constructor {
                    self.statement(StmtKind::Expr(Expr::Call { method: token, target: Some(Box::new(target)), args, virtual_call: false }), stack, out);
                    return Ok(());
                }
                self.statement(StmtKind::Assign(deref(target), Expr::New { method: token, args }), stack, out);
                return Ok(());
            }
        }
        let expr = special_call(&method, token, target.clone(), args.clone()).unwrap_or(Expr::Call { method: token, target: target.map(Box::new), args, virtual_call });
        if method.sig.ret.is_void() {
            match expr {
                Expr::Assign(target, value) => self.statement(StmtKind::Assign(*target, *value), stack, out),
                other => self.statement(StmtKind::Expr(other), stack, out),
            }
        } else {
            stack.push(expr);
        }
        Ok(())
    }

    /// 配列の添字。列挙型の値で引く所（IL では整数）は、C# では `(int)` が要る。
    fn array_index(&self, index: Expr) -> Expr {
        if self.is_value_type(&index) {
            Expr::Convert { to: Ty::I4, checked: false, value: index.boxed() }
        } else {
            index
        }
    }

    /// `InitializeArray(a, field)` の `a` が、直前に作った `new T[n]` の一時変数なら、データを読んで初期化子にする。
    fn array_initializer(&self, args: &[Expr], out: &mut Out) -> bool {
        let [Expr::Temp(temp), Expr::Token(field)] = args else { return false };
        let Some(stmt) = out.stmts.iter_mut().rev().find(|s| matches!(&s.kind, StmtKind::Assign(Expr::Temp(t), _) if t == temp)) else { return false };
        let StmtKind::Assign(_, Expr::NewArray { element, size }) = &stmt.kind else { return false };
        let Expr::Int(count) = **size else { return false };
        let Some(values) = super::data::array_values(self.names(), self.method.image, *element, *field, count as usize) else { return false };
        stmt.kind = StmtKind::Assign(Expr::Temp(*temp), Expr::ArrayInit { element: *element, values });
        true
    }

    /// `newobj`。デリゲートの作成は `new D(target.Method)`。
    fn new_object(&mut self, token: u32, stack: &mut Vec<Expr>) -> Result<(), String> {
        let sig = self.names().method_sig(token).ok_or_else(|| tr!("Cannot read the constructor {token}", token = format!("0x{token:08X}")))?;
        let mut args = Vec::with_capacity(sig.params.len());
        for _ in 0..sig.params.len() {
            args.push(stack.pop().ok_or_else(|| tr!("The constructor has too few arguments"))?);
        }
        args.reverse();
        if let [target, Expr::FunctionPointer(method)] = args.as_slice() {
            let target = if *target == Expr::Null { None } else { Some(target.clone().boxed()) };
            stack.push(Expr::NewDelegate { ty: token, target, method: *method });
            return Ok(());
        }
        let args = args.into_iter().zip(sig.params).map(|(a, ty)| match in_caller(self.names(), token, ty) {
            Some(ty) => self.coerce(a, &ty),
            None => a,
        }).collect();
        stack.push(Expr::New { method: token, args });
        Ok(())
    }

    /// `ceq`、`cgt`、`clt`（`.un` を含む）。`cgt.un` と null は `!= null`（C# のコンパイラの書き方）。
    fn compare(&self, name: &str, a: Expr, b: Expr) -> Expr {
        let op = match name {
            "ceq" => CmpOp::Eq,
            "cgt" | "cgt.un" => CmpOp::Gt,
            _ => CmpOp::Lt,
        };
        if name == "cgt.un" && (b == Expr::Null || a == Expr::Null) {
            return Expr::Compare(CmpOp::Ne, a.boxed(), b.boxed());
        }
        // bool と 0 / 1 の比較（`b == false`）は、否定にする。
        if op == CmpOp::Eq && b == Expr::Int(0) && (is_boolean(&a) || self.type_of(&a) == Some(Ty::Bool)) {
            return self.negate(a);
        }
        let (a, b) = self.matched(a, b);
        if name.ends_with(".un") && self.is_integer(&a) {
            return Expr::Compare(op, self.unsigned(a).boxed(), self.unsigned(b).boxed());
        }
        Expr::Compare(op, a.boxed(), b.boxed())
    }

    /// 比べる分岐（`beq`、`blt.un` など）の条件。`.un` の大小（NaN で真になる）は、逆の比較の否定にする。
    fn branch_condition(&self, name: &str, a: Expr, b: Expr) -> Expr {
        let base = name.trim_end_matches(".s");
        let (a, b) = self.matched(a, b);
        let (op, unordered) = match base {
            "beq" => (CmpOp::Eq, false),
            "bne.un" => (CmpOp::Ne, false),
            "bge" => (CmpOp::Ge, false),
            "bgt" => (CmpOp::Gt, false),
            "ble" => (CmpOp::Le, false),
            "blt" => (CmpOp::Lt, false),
            "bge.un" => (CmpOp::Ge, true),
            "bgt.un" => (CmpOp::Gt, true),
            "ble.un" => (CmpOp::Le, true),
            _ => (CmpOp::Lt, true),
        };
        if !unordered {
            return Expr::Compare(op, a.boxed(), b.boxed());
        }
        if self.is_integer(&a) {
            return Expr::Compare(op, self.unsigned(a).boxed(), self.unsigned(b).boxed());
        }
        // 浮動小数点数: `a >= b`（順序なしで真）は `!(a < b)`。
        Expr::Not(Expr::Compare(op.negate(), a.boxed(), b.boxed()).boxed())
    }

}

/// 条件の否定。比べる式は逆の比較に（整数と参照だけ。浮動小数点数の大小は `!` のまま）。
pub fn negate(condition: Expr) -> Expr {
    match condition {
        Expr::Not(inner) => *inner,
        Expr::Compare(op @ (CmpOp::Eq | CmpOp::Ne), a, b) => Expr::Compare(op.negate(), a, b),
        Expr::Bool(value) => Expr::Bool(!value),
        other => Expr::Not(other.boxed()),
    }
}

fn has_call(value: &Expr) -> bool {
    match value {
        Expr::Call { .. } | Expr::New { .. } | Expr::Property { .. } | Expr::Assign(..) => true,
        Expr::Binary(_, a, b) | Expr::Checked(_, a, b) | Expr::Compare(_, a, b) => has_call(a) || has_call(b),
        Expr::Field { target: Some(t), .. } => has_call(t),
        Expr::Cast { value, .. } | Expr::As { value, .. } | Expr::Box { value, .. } | Expr::Convert { value, .. } => has_call(value),
        _ => false,
    }
}

fn int(value: &Value) -> i64 {
    match value {
        Value::Int(n) => *n,
        _ => 0,
    }
}

fn float(value: &Value) -> f64 {
    match value {
        Value::Float(f) => *f,
        _ => 0.0,
    }
}

fn binop(name: &str) -> BinOp {
    match name {
        "add" => BinOp::Add,
        "sub" => BinOp::Sub,
        "mul" => BinOp::Mul,
        "div" => BinOp::Div,
        "rem" => BinOp::Rem,
        "and" => BinOp::And,
        "or" => BinOp::Or,
        "xor" => BinOp::Xor,
        "shl" => BinOp::Shl,
        _ => BinOp::Shr,
    }
}

fn is_compare_branch(name: &str) -> bool {
    ["beq", "bne.un", "bge", "bgt", "ble", "blt", "bge.un", "bgt.un", "ble.un", "blt.un"].contains(&name.trim_end_matches(".s"))
}

/// `conv.*` の変換先の型。
fn convert(name: &str, value: Expr) -> Option<Expr> {
    let checked = name.contains(".ovf");
    let target = name.trim_start_matches("conv.").trim_start_matches("ovf.").trim_end_matches(".un");
    let to = match target {
        "i1" => Ty::I1,
        "u1" => Ty::U1,
        "i2" => Ty::I2,
        "u2" => Ty::U2,
        "i4" => Ty::I4,
        "u4" => Ty::U4,
        "i8" => Ty::I8,
        "u8" => Ty::U8,
        "r4" => Ty::R4,
        "r8" | "r" => Ty::R8,
        "i" => Ty::IntPtr,
        "u" => Ty::UIntPtr,
        _ => return None,
    };
    Some(Expr::Convert { to, checked, value: value.boxed() })
}

/// 定数の変換は、変換先の型の定数にする（`(long)1` → `1L`）。
fn fold_constant(converted: Expr) -> Expr {
    let Expr::Convert { to, checked: false, value } = &converted else { return converted };
    match (to, &**value) {
        (Ty::I8, Expr::Int(n)) => Expr::Long(*n),
        (Ty::I4, Expr::Int(n)) => Expr::Int(*n),
        (Ty::R8, Expr::Int(n)) => Expr::Double(*n as f64),
        (Ty::R4, Expr::Int(n)) => Expr::Float(*n as f64),
        _ => converted,
    }
}

/// アドレスの指す先。`&x` の先は `x`。
fn deref(address: Expr) -> Expr {
    match address {
        Expr::AddressOf(inner) => *inner,
        other => Expr::Deref(other.boxed()),
    }
}

/// C# の書き方のある呼び出し: プロパティ、インデクサー、演算子、`typeof`。
fn special_call(method: &MethodRef, token: u32, target: Option<Expr>, args: Vec<Expr>) -> Option<Expr> {
    let name = method.name.as_str();
    if let Some(_property) = name.strip_prefix("get_") {
        // ゲッター: 引数があればインデクサー。
        return Some(Expr::Property { target: target.map(Box::new), method: token, args });
    }
    if name.starts_with("set_") && !args.is_empty() {
        let mut args = args;
        let value = args.pop()?;
        return Some(Expr::Assign(Expr::Property { target: target.map(Box::new), method: token, args }.boxed(), value.boxed()));
    }
    if name == "GetTypeFromHandle" && method.declaring == "Type" {
        if let [Expr::Token(ty)] = args.as_slice() {
            return Some(Expr::TypeOf(*ty));
        }
    }
    if target.is_none() {
        let binary = match name {
            "op_Addition" => Some(BinOp::Add),
            "op_Subtraction" => Some(BinOp::Sub),
            "op_Multiply" => Some(BinOp::Mul),
            "op_Division" => Some(BinOp::Div),
            "op_Modulus" => Some(BinOp::Rem),
            "op_BitwiseAnd" => Some(BinOp::And),
            "op_BitwiseOr" => Some(BinOp::Or),
            "op_ExclusiveOr" => Some(BinOp::Xor),
            _ => None,
        };
        let compare = match name {
            "op_Equality" => Some(CmpOp::Eq),
            "op_Inequality" => Some(CmpOp::Ne),
            "op_LessThan" => Some(CmpOp::Lt),
            "op_LessThanOrEqual" => Some(CmpOp::Le),
            "op_GreaterThan" => Some(CmpOp::Gt),
            "op_GreaterThanOrEqual" => Some(CmpOp::Ge),
            _ => None,
        };
        if let [value] = args.as_slice() {
            let value = value.clone().boxed();
            match name {
                // 変換演算子は、キャストの形にする（`Type.op_Explicit(x)` は C# で直接呼べない）。
                "op_Implicit" | "op_Explicit" => return Some(Expr::Convert { to: method.sig.ret.clone(), checked: false, value }),
            // `++` と `--`（decimal など）: 呼んだ結果は、1 を足した値。
            "op_Increment" => return Some(Expr::Binary(BinOp::Add, value, Expr::Int(1).boxed())),
            "op_Decrement" => return Some(Expr::Binary(BinOp::Sub, value, Expr::Int(1).boxed())),
                "op_UnaryNegation" => return Some(Expr::Neg(value)),
                "op_LogicalNot" => return Some(Expr::Not(value)),
                "op_OnesComplement" => return Some(Expr::BitNot(value)),
                _ => {}
            }
        }
        if let [a, b] = args.as_slice() {
            if let Some(op) = binary {
                return Some(Expr::Binary(op, a.clone().boxed(), b.clone().boxed()));
            }
            if let Some(op) = compare {
                return Some(Expr::Compare(op, a.clone().boxed(), b.clone().boxed()));
            }
        }
    }
    None
}

/// catch のハンドラーの最初の文が `V_n = <例外>` なら、例外の変数の名前を付ける（使わなければ `pop` で捨てている）。
fn catch_variable(mut body: Vec<Stmt>) -> (Option<String>, Vec<Stmt>) {
    let first = body.iter().position(|s| !matches!(s.kind, StmtKind::Label(_)));
    if let Some(index) = first {
        if let StmtKind::Assign(_, Expr::Exception) = &body[index].kind {
            return (Some("ex".into()), body);
        }
        // 例外を使わない catch（最初の `pop`）。
        if matches!(&body[index].kind, StmtKind::Expr(Expr::Exception)) {
            body.remove(index);
        }
    }
    (None, body)
}

/// 一時変数の名前（`build` が付けた番号から）。
pub fn temp_name(temp: u32) -> String {
    if temp >= 0xF000_0000 { format!("t{}", temp & 0x0FFF_FFFF) } else { format!("s{:x}_{}", temp >> 4, temp & 15) }
}

