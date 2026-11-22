//! IL（ECMA-335 III 巻）の命令を読む。
//!
//! 命令の表（名前、オペランドの種類、評価スタックの出入り、流れ）を一つ持ち、命令を一つずつ読む `decode` を置く。
//! 使い道は二つ:
//! - ステップする文の中の呼び出し（call / callvirt）を見つけて、その戻り値を拾う（`session/stepping.rs`）。
//! - PDB の無いモジュールの逆コンパイル（`decompile`）。

/// オペランドの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    None,
    /// 1 バイトの符号つき整数（`ldc.i4.s`）。
    Int8,
    /// 1 バイトの、引数かローカル変数の番号（`ldarg.s` など）。
    Var8,
    /// 2 バイトの、引数かローカル変数の番号（`ldarg` など）。
    Var16,
    Int32,
    Int64,
    Float32,
    Float64,
    /// 1 バイトの相対の分岐先（次の命令から）。
    ShortBranch,
    /// 4 バイトの相対の分岐先（次の命令から）。
    Branch,
    /// `switch`: 分岐先の数と、その数だけの 4 バイトの相対の分岐先。
    Switch,
    Method,
    Field,
    Type,
    /// `ldstr` の文字列（#US のトークン）。
    String,
    /// `calli` のシグネチャ（StandAloneSig のトークン）。
    Signature,
    /// `ldtoken`: 型、メソッド、フィールドのどれか。
    Token,
    /// `unaligned.` と `no.` の 1 バイト。
    Byte,
}

/// 評価スタックから取り出す数、積む数。`Call` はシグネチャで決まる（呼び出し、`newobj`、`ret`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Fixed(u8),
    Call,
}

/// 命令のあとの流れ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// 次の命令へ進む。
    Next,
    /// いつも分岐する（`br`）。
    Branch,
    /// 条件によって分岐するか、次へ進む。
    Conditional,
    /// `switch`: どれかの分岐先か、次へ進む。
    Switch,
    /// `leave`: try を出て分岐する。評価スタックは空になる。
    Leave,
    /// メソッドから戻る（`ret`、`jmp`）。
    Return,
    /// 例外を投げる（`throw`、`rethrow`）。
    Throw,
    /// finally / fault / filter の終わり（`endfinally`、`endfilter`）。
    EndHandler,
    /// 次の命令に付く前置（`constrained.` など）。
    Prefix,
}

/// 命令の種類 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpCode {
    /// 0xFE で始まる 2 バイトのものは 0xFExx。
    pub code: u16,
    pub name: &'static str,
    pub operand: Operand,
    pub pop: Stack,
    pub push: Stack,
    pub flow: Flow,
}

const fn op(code: u16, name: &'static str, operand: Operand, pop: u8, push: u8, flow: Flow) -> OpCode {
    OpCode { code, name, operand, pop: Stack::Fixed(pop), push: Stack::Fixed(push), flow }
}

const fn call(code: u16, name: &'static str, operand: Operand, flow: Flow) -> OpCode {
    OpCode { code, name, operand, pop: Stack::Call, push: Stack::Call, flow }
}

use Flow::*;
use Operand as O;

/// 1 バイトのオペコード（0x00〜0xE0）。添字がオペコード。使われていない番号は `None`。
static ONE_BYTE: [Option<OpCode>; 0xE1] = {
    let mut table = [None; 0xE1];
    macro_rules! set {
        ($($e:expr),* $(,)?) => { $( { let o: OpCode = $e; table[o.code as usize] = Some(o); } )* };
    }
    set!(
        op(0x00, "nop", O::None, 0, 0, Next),
        op(0x01, "break", O::None, 0, 0, Next),
        op(0x02, "ldarg.0", O::None, 0, 1, Next),
        op(0x03, "ldarg.1", O::None, 0, 1, Next),
        op(0x04, "ldarg.2", O::None, 0, 1, Next),
        op(0x05, "ldarg.3", O::None, 0, 1, Next),
        op(0x06, "ldloc.0", O::None, 0, 1, Next),
        op(0x07, "ldloc.1", O::None, 0, 1, Next),
        op(0x08, "ldloc.2", O::None, 0, 1, Next),
        op(0x09, "ldloc.3", O::None, 0, 1, Next),
        op(0x0A, "stloc.0", O::None, 1, 0, Next),
        op(0x0B, "stloc.1", O::None, 1, 0, Next),
        op(0x0C, "stloc.2", O::None, 1, 0, Next),
        op(0x0D, "stloc.3", O::None, 1, 0, Next),
        op(0x0E, "ldarg.s", O::Var8, 0, 1, Next),
        op(0x0F, "ldarga.s", O::Var8, 0, 1, Next),
        op(0x10, "starg.s", O::Var8, 1, 0, Next),
        op(0x11, "ldloc.s", O::Var8, 0, 1, Next),
        op(0x12, "ldloca.s", O::Var8, 0, 1, Next),
        op(0x13, "stloc.s", O::Var8, 1, 0, Next),
        op(0x14, "ldnull", O::None, 0, 1, Next),
        op(0x15, "ldc.i4.m1", O::None, 0, 1, Next),
        op(0x16, "ldc.i4.0", O::None, 0, 1, Next),
        op(0x17, "ldc.i4.1", O::None, 0, 1, Next),
        op(0x18, "ldc.i4.2", O::None, 0, 1, Next),
        op(0x19, "ldc.i4.3", O::None, 0, 1, Next),
        op(0x1A, "ldc.i4.4", O::None, 0, 1, Next),
        op(0x1B, "ldc.i4.5", O::None, 0, 1, Next),
        op(0x1C, "ldc.i4.6", O::None, 0, 1, Next),
        op(0x1D, "ldc.i4.7", O::None, 0, 1, Next),
        op(0x1E, "ldc.i4.8", O::None, 0, 1, Next),
        op(0x1F, "ldc.i4.s", O::Int8, 0, 1, Next),
        op(0x20, "ldc.i4", O::Int32, 0, 1, Next),
        op(0x21, "ldc.i8", O::Int64, 0, 1, Next),
        op(0x22, "ldc.r4", O::Float32, 0, 1, Next),
        op(0x23, "ldc.r8", O::Float64, 0, 1, Next),
        op(0x25, "dup", O::None, 1, 2, Next),
        op(0x26, "pop", O::None, 1, 0, Next),
        op(0x27, "jmp", O::Method, 0, 0, Return),
        call(0x28, "call", O::Method, Next),
        call(0x29, "calli", O::Signature, Next),
        call(0x2A, "ret", O::None, Return),
        op(0x2B, "br.s", O::ShortBranch, 0, 0, Branch),
        op(0x2C, "brfalse.s", O::ShortBranch, 1, 0, Conditional),
        op(0x2D, "brtrue.s", O::ShortBranch, 1, 0, Conditional),
        op(0x2E, "beq.s", O::ShortBranch, 2, 0, Conditional),
        op(0x2F, "bge.s", O::ShortBranch, 2, 0, Conditional),
        op(0x30, "bgt.s", O::ShortBranch, 2, 0, Conditional),
        op(0x31, "ble.s", O::ShortBranch, 2, 0, Conditional),
        op(0x32, "blt.s", O::ShortBranch, 2, 0, Conditional),
        op(0x33, "bne.un.s", O::ShortBranch, 2, 0, Conditional),
        op(0x34, "bge.un.s", O::ShortBranch, 2, 0, Conditional),
        op(0x35, "bgt.un.s", O::ShortBranch, 2, 0, Conditional),
        op(0x36, "ble.un.s", O::ShortBranch, 2, 0, Conditional),
        op(0x37, "blt.un.s", O::ShortBranch, 2, 0, Conditional),
        op(0x38, "br", O::Branch, 0, 0, Branch),
        op(0x39, "brfalse", O::Branch, 1, 0, Conditional),
        op(0x3A, "brtrue", O::Branch, 1, 0, Conditional),
        op(0x3B, "beq", O::Branch, 2, 0, Conditional),
        op(0x3C, "bge", O::Branch, 2, 0, Conditional),
        op(0x3D, "bgt", O::Branch, 2, 0, Conditional),
        op(0x3E, "ble", O::Branch, 2, 0, Conditional),
        op(0x3F, "blt", O::Branch, 2, 0, Conditional),
        op(0x40, "bne.un", O::Branch, 2, 0, Conditional),
        op(0x41, "bge.un", O::Branch, 2, 0, Conditional),
        op(0x42, "bgt.un", O::Branch, 2, 0, Conditional),
        op(0x43, "ble.un", O::Branch, 2, 0, Conditional),
        op(0x44, "blt.un", O::Branch, 2, 0, Conditional),
        op(0x45, "switch", O::Switch, 1, 0, Switch),
        op(0x46, "ldind.i1", O::None, 1, 1, Next),
        op(0x47, "ldind.u1", O::None, 1, 1, Next),
        op(0x48, "ldind.i2", O::None, 1, 1, Next),
        op(0x49, "ldind.u2", O::None, 1, 1, Next),
        op(0x4A, "ldind.i4", O::None, 1, 1, Next),
        op(0x4B, "ldind.u4", O::None, 1, 1, Next),
        op(0x4C, "ldind.i8", O::None, 1, 1, Next),
        op(0x4D, "ldind.i", O::None, 1, 1, Next),
        op(0x4E, "ldind.r4", O::None, 1, 1, Next),
        op(0x4F, "ldind.r8", O::None, 1, 1, Next),
        op(0x50, "ldind.ref", O::None, 1, 1, Next),
        op(0x51, "stind.ref", O::None, 2, 0, Next),
        op(0x52, "stind.i1", O::None, 2, 0, Next),
        op(0x53, "stind.i2", O::None, 2, 0, Next),
        op(0x54, "stind.i4", O::None, 2, 0, Next),
        op(0x55, "stind.i8", O::None, 2, 0, Next),
        op(0x56, "stind.r4", O::None, 2, 0, Next),
        op(0x57, "stind.r8", O::None, 2, 0, Next),
        op(0x58, "add", O::None, 2, 1, Next),
        op(0x59, "sub", O::None, 2, 1, Next),
        op(0x5A, "mul", O::None, 2, 1, Next),
        op(0x5B, "div", O::None, 2, 1, Next),
        op(0x5C, "div.un", O::None, 2, 1, Next),
        op(0x5D, "rem", O::None, 2, 1, Next),
        op(0x5E, "rem.un", O::None, 2, 1, Next),
        op(0x5F, "and", O::None, 2, 1, Next),
        op(0x60, "or", O::None, 2, 1, Next),
        op(0x61, "xor", O::None, 2, 1, Next),
        op(0x62, "shl", O::None, 2, 1, Next),
        op(0x63, "shr", O::None, 2, 1, Next),
        op(0x64, "shr.un", O::None, 2, 1, Next),
        op(0x65, "neg", O::None, 1, 1, Next),
        op(0x66, "not", O::None, 1, 1, Next),
        op(0x67, "conv.i1", O::None, 1, 1, Next),
        op(0x68, "conv.i2", O::None, 1, 1, Next),
        op(0x69, "conv.i4", O::None, 1, 1, Next),
        op(0x6A, "conv.i8", O::None, 1, 1, Next),
        op(0x6B, "conv.r4", O::None, 1, 1, Next),
        op(0x6C, "conv.r8", O::None, 1, 1, Next),
        op(0x6D, "conv.u4", O::None, 1, 1, Next),
        op(0x6E, "conv.u8", O::None, 1, 1, Next),
        call(0x6F, "callvirt", O::Method, Next),
        op(0x70, "cpobj", O::Type, 2, 0, Next),
        op(0x71, "ldobj", O::Type, 1, 1, Next),
        op(0x72, "ldstr", O::String, 0, 1, Next),
        call(0x73, "newobj", O::Method, Next),
        op(0x74, "castclass", O::Type, 1, 1, Next),
        op(0x75, "isinst", O::Type, 1, 1, Next),
        op(0x76, "conv.r.un", O::None, 1, 1, Next),
        op(0x79, "unbox", O::Type, 1, 1, Next),
        op(0x7A, "throw", O::None, 1, 0, Throw),
        op(0x7B, "ldfld", O::Field, 1, 1, Next),
        op(0x7C, "ldflda", O::Field, 1, 1, Next),
        op(0x7D, "stfld", O::Field, 2, 0, Next),
        op(0x7E, "ldsfld", O::Field, 0, 1, Next),
        op(0x7F, "ldsflda", O::Field, 0, 1, Next),
        op(0x80, "stsfld", O::Field, 1, 0, Next),
        op(0x81, "stobj", O::Type, 2, 0, Next),
        op(0x82, "conv.ovf.i1.un", O::None, 1, 1, Next),
        op(0x83, "conv.ovf.i2.un", O::None, 1, 1, Next),
        op(0x84, "conv.ovf.i4.un", O::None, 1, 1, Next),
        op(0x85, "conv.ovf.i8.un", O::None, 1, 1, Next),
        op(0x86, "conv.ovf.u1.un", O::None, 1, 1, Next),
        op(0x87, "conv.ovf.u2.un", O::None, 1, 1, Next),
        op(0x88, "conv.ovf.u4.un", O::None, 1, 1, Next),
        op(0x89, "conv.ovf.u8.un", O::None, 1, 1, Next),
        op(0x8A, "conv.ovf.i.un", O::None, 1, 1, Next),
        op(0x8B, "conv.ovf.u.un", O::None, 1, 1, Next),
        op(0x8C, "box", O::Type, 1, 1, Next),
        op(0x8D, "newarr", O::Type, 1, 1, Next),
        op(0x8E, "ldlen", O::None, 1, 1, Next),
        op(0x8F, "ldelema", O::Type, 2, 1, Next),
        op(0x90, "ldelem.i1", O::None, 2, 1, Next),
        op(0x91, "ldelem.u1", O::None, 2, 1, Next),
        op(0x92, "ldelem.i2", O::None, 2, 1, Next),
        op(0x93, "ldelem.u2", O::None, 2, 1, Next),
        op(0x94, "ldelem.i4", O::None, 2, 1, Next),
        op(0x95, "ldelem.u4", O::None, 2, 1, Next),
        op(0x96, "ldelem.i8", O::None, 2, 1, Next),
        op(0x97, "ldelem.i", O::None, 2, 1, Next),
        op(0x98, "ldelem.r4", O::None, 2, 1, Next),
        op(0x99, "ldelem.r8", O::None, 2, 1, Next),
        op(0x9A, "ldelem.ref", O::None, 2, 1, Next),
        op(0x9B, "stelem.i", O::None, 3, 0, Next),
        op(0x9C, "stelem.i1", O::None, 3, 0, Next),
        op(0x9D, "stelem.i2", O::None, 3, 0, Next),
        op(0x9E, "stelem.i4", O::None, 3, 0, Next),
        op(0x9F, "stelem.i8", O::None, 3, 0, Next),
        op(0xA0, "stelem.r4", O::None, 3, 0, Next),
        op(0xA1, "stelem.r8", O::None, 3, 0, Next),
        op(0xA2, "stelem.ref", O::None, 3, 0, Next),
        op(0xA3, "ldelem", O::Type, 2, 1, Next),
        op(0xA4, "stelem", O::Type, 3, 0, Next),
        op(0xA5, "unbox.any", O::Type, 1, 1, Next),
        op(0xB3, "conv.ovf.i1", O::None, 1, 1, Next),
        op(0xB4, "conv.ovf.u1", O::None, 1, 1, Next),
        op(0xB5, "conv.ovf.i2", O::None, 1, 1, Next),
        op(0xB6, "conv.ovf.u2", O::None, 1, 1, Next),
        op(0xB7, "conv.ovf.i4", O::None, 1, 1, Next),
        op(0xB8, "conv.ovf.u4", O::None, 1, 1, Next),
        op(0xB9, "conv.ovf.i8", O::None, 1, 1, Next),
        op(0xBA, "conv.ovf.u8", O::None, 1, 1, Next),
        op(0xC2, "refanyval", O::Type, 1, 1, Next),
        op(0xC3, "ckfinite", O::None, 1, 1, Next),
        op(0xC6, "mkrefany", O::Type, 1, 1, Next),
        op(0xD0, "ldtoken", O::Token, 0, 1, Next),
        op(0xD1, "conv.u2", O::None, 1, 1, Next),
        op(0xD2, "conv.u1", O::None, 1, 1, Next),
        op(0xD3, "conv.i", O::None, 1, 1, Next),
        op(0xD4, "conv.ovf.i", O::None, 1, 1, Next),
        op(0xD5, "conv.ovf.u", O::None, 1, 1, Next),
        op(0xD6, "add.ovf", O::None, 2, 1, Next),
        op(0xD7, "add.ovf.un", O::None, 2, 1, Next),
        op(0xD8, "mul.ovf", O::None, 2, 1, Next),
        op(0xD9, "mul.ovf.un", O::None, 2, 1, Next),
        op(0xDA, "sub.ovf", O::None, 2, 1, Next),
        op(0xDB, "sub.ovf.un", O::None, 2, 1, Next),
        op(0xDC, "endfinally", O::None, 0, 0, EndHandler),
        op(0xDD, "leave", O::Branch, 0, 0, Leave),
        op(0xDE, "leave.s", O::ShortBranch, 0, 0, Leave),
        op(0xDF, "stind.i", O::None, 2, 0, Next),
        op(0xE0, "conv.u", O::None, 1, 1, Next),
    );
    table
};

/// 2 バイトのオペコード（0xFE00〜0xFE1E）。添字は 2 バイト目。
static TWO_BYTE: [Option<OpCode>; 0x1F] = {
    let mut table = [None; 0x1F];
    macro_rules! set {
        ($($e:expr),* $(,)?) => { $( { let o: OpCode = $e; table[(o.code & 0xFF) as usize] = Some(o); } )* };
    }
    set!(
        op(0xFE00, "arglist", O::None, 0, 1, Next),
        op(0xFE01, "ceq", O::None, 2, 1, Next),
        op(0xFE02, "cgt", O::None, 2, 1, Next),
        op(0xFE03, "cgt.un", O::None, 2, 1, Next),
        op(0xFE04, "clt", O::None, 2, 1, Next),
        op(0xFE05, "clt.un", O::None, 2, 1, Next),
        op(0xFE06, "ldftn", O::Method, 0, 1, Next),
        op(0xFE07, "ldvirtftn", O::Method, 1, 1, Next),
        op(0xFE09, "ldarg", O::Var16, 0, 1, Next),
        op(0xFE0A, "ldarga", O::Var16, 0, 1, Next),
        op(0xFE0B, "starg", O::Var16, 1, 0, Next),
        op(0xFE0C, "ldloc", O::Var16, 0, 1, Next),
        op(0xFE0D, "ldloca", O::Var16, 0, 1, Next),
        op(0xFE0E, "stloc", O::Var16, 1, 0, Next),
        op(0xFE0F, "localloc", O::None, 1, 1, Next),
        op(0xFE11, "endfilter", O::None, 1, 0, EndHandler),
        op(0xFE12, "unaligned.", O::Byte, 0, 0, Prefix),
        op(0xFE13, "volatile.", O::None, 0, 0, Prefix),
        op(0xFE14, "tail.", O::None, 0, 0, Prefix),
        op(0xFE15, "initobj", O::Type, 1, 0, Next),
        op(0xFE16, "constrained.", O::Type, 0, 0, Prefix),
        op(0xFE17, "cpblk", O::None, 3, 0, Next),
        op(0xFE18, "initblk", O::None, 3, 0, Next),
        op(0xFE19, "no.", O::Byte, 0, 0, Prefix),
        op(0xFE1A, "rethrow", O::None, 0, 0, Throw),
        op(0xFE1C, "sizeof", O::Type, 0, 1, Next),
        op(0xFE1D, "refanytype", O::None, 1, 1, Next),
        op(0xFE1E, "readonly.", O::None, 0, 0, Prefix),
    );
    table
};

/// オペランドの値。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    Int(i64),
    Float(f64),
    /// 引数かローカル変数の番号。
    Var(u16),
    /// 分岐先の IL オフセット（命令列の頭から）。
    Target(u32),
    Targets(Vec<u32>),
    /// メタデータのトークン（メソッド、フィールド、型、文字列、シグネチャ）。
    Token(u32),
}

/// 読んだ命令 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub offset: u32,
    /// 命令の長さ（オペコードとオペランド）。
    pub size: u32,
    pub op: &'static OpCode,
    pub value: Value,
}

impl Instruction {
    /// 次の命令の位置。
    pub fn end(&self) -> u32 {
        self.offset + self.size
    }

    /// 分岐先（`switch` は全部）。分岐しない命令は空。
    pub fn targets(&self) -> &[u32] {
        match &self.value {
            Value::Target(target) => std::slice::from_ref(target),
            Value::Targets(targets) => targets,
            _ => &[],
        }
    }
}

/// オペコードの表を引く。知らない番号は `None`。
pub fn opcode(code: u16) -> Option<&'static OpCode> {
    if code >> 8 == 0xFE {
        TWO_BYTE.get((code & 0xFF) as usize)?.as_ref()
    } else {
        ONE_BYTE.get(code as usize)?.as_ref()
    }
}

fn bytes<const N: usize>(il: &[u8], at: usize) -> Option<[u8; N]> {
    il.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// `offset` にある命令を読む。命令列の外、知らないオペコード、途中で切れた命令は `None`。
pub fn decode(il: &[u8], offset: u32) -> Option<Instruction> {
    let at = offset as usize;
    let first = *il.get(at)?;
    let (op, operand_at) = if first == 0xFE { (opcode(0xFE00 | *il.get(at + 1)? as u16)?, at + 2) } else { (opcode(first as u16)?, at + 1) };
    let (value, operand_size) = match op.operand {
        O::None => (Value::None, 0),
        O::Int8 => (Value::Int(i8::from_le_bytes(bytes(il, operand_at)?) as i64), 1),
        O::Var8 | O::Byte => (Value::Var(*il.get(operand_at)? as u16), 1),
        O::Var16 => (Value::Var(u16::from_le_bytes(bytes(il, operand_at)?)), 2),
        O::Int32 => (Value::Int(i32::from_le_bytes(bytes(il, operand_at)?) as i64), 4),
        O::Int64 => (Value::Int(i64::from_le_bytes(bytes(il, operand_at)?)), 8),
        O::Float32 => (Value::Float(f32::from_le_bytes(bytes(il, operand_at)?) as f64), 4),
        O::Float64 => (Value::Float(f64::from_le_bytes(bytes(il, operand_at)?)), 8),
        O::ShortBranch => {
            let delta = i8::from_le_bytes(bytes(il, operand_at)?) as i64;
            (Value::Target(branch_target(operand_at + 1, delta)?), 1)
        }
        O::Branch => {
            let delta = i32::from_le_bytes(bytes(il, operand_at)?) as i64;
            (Value::Target(branch_target(operand_at + 4, delta)?), 4)
        }
        O::Switch => {
            let count = u32::from_le_bytes(bytes(il, operand_at)?) as usize;
            // 壊れた数で巨大な領域を取らないよう、命令列に収まる数に限る。
            let end = operand_at.checked_add(4)?.checked_add(count.checked_mul(4)?)?;
            if end > il.len() {
                return None;
            }
            let targets = (0..count).map(|i| branch_target(end, i32::from_le_bytes(bytes(il, operand_at + 4 + i * 4)?) as i64)).collect::<Option<Vec<_>>>()?;
            (Value::Targets(targets), 4 + count * 4)
        }
        O::Method | O::Field | O::Type | O::String | O::Signature | O::Token => (Value::Token(u32::from_le_bytes(bytes(il, operand_at)?)), 4),
    };
    let size = (operand_at - at + operand_size) as u32;
    Some(Instruction { offset, size, op, value })
}

/// 分岐先: 次の命令の位置 `next` からの相対。
fn branch_target(next: usize, delta: i64) -> Option<u32> {
    u32::try_from(next as i64 + delta).ok()
}

/// 命令列を頭から順に読む。読めない所で終わる。
pub fn instructions(il: &[u8]) -> impl Iterator<Item = Instruction> + '_ {
    let mut offset = 0u32;
    std::iter::from_fn(move || {
        let instruction = decode(il, offset)?;
        offset = instruction.end();
        Some(instruction)
    })
}

/// `start..end` にある call / callvirt 命令の「IL オフセット、呼び出し先のメソッドのトークン」。
pub fn find_calls(il: &[u8], start: u32, end: u32) -> Vec<(u32, u32)> {
    let mut calls = Vec::new();
    let mut offset = start;
    while offset < end.min(il.len() as u32) {
        let Some(instruction) = decode(il, offset) else { break };
        if let ("call" | "callvirt", Value::Token(token)) = (instruction.op.name, &instruction.value) {
            calls.push((offset, *token));
        }
        offset = instruction.end();
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_are_found_with_their_offsets_and_tokens() {
        // ldarg.0; call 0x06000002; stloc.0; ldloc.0; callvirt 0x0A000011; pop; ret
        let il = [0x02, 0x28, 0x02, 0x00, 0x00, 0x06, 0x0A, 0x06, 0x6F, 0x11, 0x00, 0x00, 0x0A, 0x26, 0x2A];
        assert_eq!(find_calls(&il, 0, il.len() as u32), [(1, 0x0600_0002), (8, 0x0A00_0011)]);
    }

    #[test]
    fn only_the_given_range_is_searched() {
        let il = [0x02, 0x28, 0x02, 0x00, 0x00, 0x06, 0x0A, 0x06, 0x6F, 0x11, 0x00, 0x00, 0x0A, 0x26, 0x2A];
        assert_eq!(find_calls(&il, 6, 14), [(8, 0x0A00_0011)]);
        assert_eq!(find_calls(&il, 0, 6), [(1, 0x0600_0002)]);
    }

    #[test]
    fn an_operand_that_looks_like_a_call_is_not_one() {
        // ldc.i4 0x0000286F（オペランドの中に 0x6F と 0x28 がある）; ldstr 0x70000028; ret
        let il = [0x20, 0x6F, 0x28, 0x00, 0x00, 0x72, 0x28, 0x00, 0x00, 0x70, 0x2A];
        assert!(find_calls(&il, 0, il.len() as u32).is_empty());
    }

    #[test]
    fn a_switch_is_skipped_as_one_instruction() {
        // switch (2 個の分岐先); call 0x06000001
        let il = [0x45, 0x02, 0x00, 0x00, 0x00, 0x28, 0, 0, 0, 0x6F, 0, 0, 0, 0x28, 0x01, 0x00, 0x00, 0x06];
        assert_eq!(find_calls(&il, 0, il.len() as u32), [(13, 0x0600_0001)]);
    }

    #[test]
    fn two_byte_opcodes_and_short_forms_have_their_own_sizes() {
        // constrained. token; ldloc.s 5; ldc.i8 ...; call
        let mut il = vec![0xFE, 0x16, 0x28, 0x00, 0x00, 0x01, 0x11, 0x28, 0x21];
        il.extend([0x28; 8]);
        il.extend([0x28, 0x07, 0x00, 0x00, 0x06]);
        assert_eq!(find_calls(&il, 0, il.len() as u32), [(17, 0x0600_0007)]);
    }

    #[test]
    fn truncated_code_ends_the_search_without_a_panic() {
        assert!(find_calls(&[0x28, 0x01, 0x00], 0, 3).is_empty());
        assert!(find_calls(&[0x45, 0x02], 0, 2).is_empty());
        assert!(find_calls(&[0xFE], 0, 1).is_empty());
        assert!(find_calls(&[], 0, 10).is_empty());
    }

    #[test]
    fn branches_are_resolved_to_offsets_from_the_start() {
        // br.s +2; nop; nop; brtrue -7 (IL_0004 から 5 バイトの命令 → 次は 9、9 - 7 = 2)
        let il = [0x2B, 0x02, 0x00, 0x00, 0x3A, 0xF9, 0xFF, 0xFF, 0xFF];
        let decoded: Vec<_> = instructions(&il).collect();
        assert_eq!(decoded[0].value, Value::Target(4));
        assert_eq!(decoded[3].offset, 4);
        assert_eq!(decoded[3].value, Value::Target(2));
        assert_eq!(decoded[3].op.flow, Flow::Conditional);
    }

    #[test]
    fn a_switch_has_all_of_its_targets() {
        // switch (2): +0, +4; ret
        let il = [0x45, 0x02, 0, 0, 0, 0, 0, 0, 0, 0x04, 0, 0, 0, 0x2A];
        let switch = decode(&il, 0).unwrap();
        assert_eq!(switch.size, 13);
        assert_eq!(switch.targets(), [13, 17]);
    }

    #[test]
    fn operands_are_read_with_their_signs_and_widths() {
        assert_eq!(decode(&[0x1F, 0xFF], 0).unwrap().value, Value::Int(-1));
        assert_eq!(decode(&[0x20, 0x00, 0x00, 0x00, 0x80], 0).unwrap().value, Value::Int(i32::MIN as i64));
        assert_eq!(decode(&[0xFE, 0x0C, 0x34, 0x12], 0).unwrap().value, Value::Var(0x1234));
        assert_eq!(decode(&[0x22, 0x00, 0x00, 0xC0, 0x3F], 0).unwrap().value, Value::Float(1.5));
        assert_eq!(decode(&[0x72, 0x01, 0x00, 0x00, 0x70], 0).unwrap().value, Value::Token(0x7000_0001));
    }

    #[test]
    fn every_opcode_in_the_tables_has_its_own_code() {
        for code in (0..0xE1u16).chain(0xFE00..0xFE1F) {
            if let Some(op) = opcode(code) {
                assert_eq!(op.code, code, "{}", op.name);
            }
        }
        assert!(opcode(0x24).is_none());
        assert!(opcode(0xFE08).is_none());
    }
}
