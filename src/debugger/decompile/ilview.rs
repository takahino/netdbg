//! C# に戻せないメソッドの本体を、IL の 1 命令 1 行で書く（段階 1 の形。宣言は C# のまま）。
//!
//! 本体の行はコメント（`// IL_0004: call …`）にする。ファイルは C# として正しいまま（エディターが赤い波線で埋まらない）で、
//! 行にはブレークポイントを置ける。系列点は、評価スタックが空になる命令の行に打つ（`flow.rs`）。

use super::body::{self, ClauseKind, MethodBody};
use super::flow::{self, Effect};
use super::names::{string_literal, Generics, Names};
use super::sig;
use super::writer::BodyMap;
use super::{Point, Text};
use crate::debugger::il::{self, Instruction, Operand, Stack, Value};
use crate::debugger::metadata::MEMBER_REF;
use std::collections::HashMap;

/// メソッドの本体の IL。
pub(super) fn write_body(names: &Names, row: u32, body: &MethodBody, generics: &Generics, depth: usize, text: &mut Text) -> BodyMap {
    let Some(signature) = names.method_def_sig(row) else {
        text.line(depth, tr_str!("// Cannot read the signature"));
        return BodyMap::default();
    };
    let locals = write_locals(names, body, generics, depth, text);
    let local_names = (0..locals.len() as u32).map(|i| (i, format!("V_{i}"))).collect();
    let mut points = Vec::new();

    let code: Vec<Instruction> = il::instructions(body.code).collect();
    let effect = |instruction: &Instruction| stack_effect(names, &signature, instruction);
    let starts = flow::statement_starts(&code, &flow::stack_depths(&code, &body.clauses, &effect));
    let arguments = argument_names(names, row, &signature);
    let markers = clause_markers(names, body, generics);
    let mut nesting = 0usize;
    for (instruction, &start) in code.iter().zip(&starts) {
        for marker in markers.get(&instruction.offset).into_iter().flatten() {
            write_marker(marker, depth, &mut nesting, text);
        }
        let line = instruction_line(instruction, &operand_text(names, instruction, generics, &arguments, &locals));
        let (number, start_column, end_column) = text.line(depth + nesting, &line);
        if start {
            points.push(Point { il_offset: instruction.offset, line: number, start_column, end_column });
        }
    }
    // 最後の命令のあとで閉じる節（メソッドの終わりまで続くハンドラー）。
    let end = code.last().map_or(0, Instruction::end);
    for marker in markers.get(&end).into_iter().flatten().filter(|m| m.closes) {
        write_marker(marker, depth, &mut nesting, text);
    }
    if end as usize != body.code.len() {
        text.line(depth, tr_str!("// (the IL from here on cannot be read)"));
    }
    BodyMap { points, locals: local_names, ..Default::default() }
}

/// ローカル変数の型を読み、`// ローカル変数: V_0 (int), …` の行を書く。
fn write_locals(names: &Names, body: &MethodBody, generics: &Generics, depth: usize, text: &mut Text) -> Vec<sig::Ty> {
    let locals = names.local_types(body.local_signature);
    if !locals.is_empty() {
        let list: Vec<String> = locals.iter().enumerate().map(|(i, ty)| format!("V_{i} ({})", names.ty(ty, generics))).collect();
        text.line(depth, &tr!("// Locals: {list}", list = list.join(", ")));
    }
    locals
}

/// 例外の節の印を 1 行書き、入れ子の深さを進める。
fn write_marker(marker: &Marker, depth: usize, nesting: &mut usize, text: &mut Text) {
    if marker.closes {
        *nesting = nesting.saturating_sub(1);
    }
    text.line(depth + *nesting, &format!("// {}", marker.text));
    if marker.opens {
        *nesting += 1;
    }
}

/// 命令の行: `// IL_0004: call int Program.Compute(int)`。
fn instruction_line(instruction: &Instruction, operand: &str) -> String {
    if operand.is_empty() {
        format!("// IL_{:04x}: {}", instruction.offset, instruction.op.name)
    } else {
        format!("// IL_{:04x}: {} {operand}", instruction.offset, instruction.op.name)
    }
}
/// 引数の名前（`ldarg` の番号の順。インスタンスメソッドは 0 が `this`）。
pub(super) fn argument_names(names: &Names, row: u32, signature: &sig::MethodSig) -> Vec<String> {
    let mut list = Vec::new();
    if signature.has_this && !signature.explicit_this {
        list.push("this".to_string());
    }
    list.extend(names.parameters(row, signature).into_iter().map(|p| p.name));
    list
}

/// 例外の節の始まりと終わりの印（`try {`、`} catch (Exception) {`、`}`）。キーは IL オフセット。
struct Marker {
    text: String,
    opens: bool,
    closes: bool,
}

fn clause_markers(names: &Names, body: &MethodBody, generics: &Generics) -> HashMap<u32, Vec<Marker>> {
    let mut markers: HashMap<u32, Vec<Marker>> = HashMap::new();
    let mut add = |offset: u32, text: String, opens: bool, closes: bool| markers.entry(offset).or_default().push(Marker { text, opens, closes });
    let mut tries = Vec::new();
    for clause in &body.clauses {
        // 同じ try に catch がいくつも付くときは、try の印は一度だけ。
        if !tries.contains(&clause.try_range) {
            tries.push(clause.try_range.clone());
        }
    }
    // 外側の try（範囲が広い方）の印を先に開き、あとに閉じる。
    tries.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
    for range in &tries {
        add(range.start, "try {".into(), true, false);
    }
    // 節の入り口（フィルターなら条件の頭）。
    let entry = |clause: &body::Clause| if clause.kind == ClauseKind::Filter { clause.filter_start } else { clause.handler.start };
    for clause in &body.clauses {
        let handler_text = match clause.kind {
            ClauseKind::Catch => format!("catch ({}) {{", names.type_token(clause.class_token, &[], generics)),
            ClauseKind::Filter => tr!("catch when (the filter above is true) {"),
            ClauseKind::Finally => "finally {".into(),
            ClauseKind::Fault => "fault {".into(),
        };
        // try の終わりは、最初の節の入り口（`} catch … {` が閉じと開きを兼ねる）なので、別の印は要らない。
        if clause.kind == ClauseKind::Filter {
            add(clause.filter_start, "} filter {".into(), true, true);
        }
        add(clause.handler.start, format!("}} {handler_text}"), true, true);
        // 同じ try の次の節がすぐ続くなら、その `} catch … {` が閉じる。
        let followed = body.clauses.iter().any(|other| other.try_range == clause.try_range && entry(other) == clause.handler.end);
        if !followed {
            add(clause.handler.end, "}".into(), false, true);
        }
    }
    // 同じ位置では、閉じるだけの印、閉じて開く印、開くだけの印の順。
    for list in markers.values_mut() {
        list.sort_by_key(|m| match (m.closes, m.opens) {
            (true, false) => 0,
            (true, true) => 1,
            _ => 2,
        });
    }
    markers
}

/// 命令の評価スタックの出入り。呼び出しはシグネチャから数える。
pub(super) fn stack_effect(names: &Names, signature: &sig::MethodSig, instruction: &Instruction) -> Effect {
    if let (Stack::Fixed(pop), Stack::Fixed(push)) = (instruction.op.pop, instruction.op.push) {
        return (pop as u32, push as u32);
    }
    let returns = |s: &sig::MethodSig| !s.ret.is_void() as u32;
    match (instruction.op.name, &instruction.value) {
        ("ret", _) => (returns(signature), 0),
        ("newobj", Value::Token(token)) => names.method_sig(*token).map_or((0, 1), |s| (s.params.len() as u32, 1)),
        ("calli", Value::Token(token)) => names.signature(*token).map_or((1, 0), |s| (s.stack_arguments() + 1, returns(&s))),
        (_, Value::Token(token)) => names.method_sig(*token).map_or((0, 0), |s| (s.stack_arguments(), returns(&s))),
        _ => (0, 0),
    }
}

/// オペランドの書き方。
fn operand_text(names: &Names, instruction: &Instruction, generics: &Generics, arguments: &[String], locals: &[sig::Ty]) -> String {
    let local = |index: u16| if (index as usize) < locals.len() { format!("V_{index}") } else { format!("V_{index}?") };
    match (&instruction.value, instruction.op.operand) {
        (Value::None, _) => String::new(),
        (Value::Int(value), _) => value.to_string(),
        (Value::Float(value), Operand::Float32) => super::decl::float_literal(*value, true),
        (Value::Float(value), _) => super::decl::float_literal(*value, false),
        (Value::Var(index), Operand::Byte) => index.to_string(),
        (Value::Var(index), _) if instruction.op.name.contains("arg") => arguments.get(*index as usize).cloned().unwrap_or_else(|| format!("A_{index}")),
        (Value::Var(index), _) => local(*index),
        (Value::Target(target), _) => format!("IL_{target:04x}"),
        (Value::Targets(targets), _) => format!("({})", targets.iter().map(|t| format!("IL_{t:04x}")).collect::<Vec<_>>().join(", ")),
        (Value::Token(token), Operand::String) => names.md.user_string(*token).map_or_else(|| format!("[0x{token:08X}]"), |s| string_literal(&s)),
        (Value::Token(token), Operand::Field) => names.field_text(*token, generics),
        (Value::Token(token), Operand::Signature) => names.signature_text(*token, generics),
        (Value::Token(token), Operand::Method) => method_text(names, *token, generics),
        (Value::Token(token), Operand::Token) => match token >> 24 {
            0x04 => names.field_text(*token, generics),
            0x06 | 0x2B => method_text(names, *token, generics),
            0x0A if is_field_ref(names, *token) => names.field_text(*token, generics),
            0x0A => method_text(names, *token, generics),
            _ => names.type_token(*token, &[], generics),
        },
        (Value::Token(token), _) => names.type_token(*token, &[], generics),
    }
}

/// 呼び出し先のメソッドの書き方。読めなければトークン。
fn method_text(names: &Names, token: u32, generics: &Generics) -> String {
    names.method_ref(token, generics).map_or_else(|| format!("[0x{token:08X}]"), |m| names.call_text(&m))
}

/// MemberRef のトークンが、フィールドを指すか。
fn is_field_ref(names: &Names, token: u32) -> bool {
    names.md.blob(names.md.cell(MEMBER_REF, token & 0x00FF_FFFF, 2)).is_ok_and(sig::is_field)
}
