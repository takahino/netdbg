//! 式の演算子の計算。
//!
//! 演算子の両辺は、対象から読んだ値をエンジンの側の値（`Operand`）にしてから、ここで計算する。対象のコードは
//! 実行しない（利用者が定義した演算子は呼ばない。数、bool、列挙型、文字列、参照の比較だけ）。結果は、呼び出し側が
//! 対象の中に値として作り直す。
//!
//! 型の規則は C# と同じにする（確かめるのは総当たり試験。docs/DECISIONS.md D-026）:
//! - `int` より小さい型（`sbyte`、`byte`、`short`、`ushort`、`char`）は、先に `int` に広げる。
//! - 二項数値昇格: どちらかが `double` なら `double`、`float` なら `float`、`ulong` なら `ulong`（相手が符号つきなら
//!   エラー）、`long` なら `long`、`uint` と符号つきの組は `long`、`uint` と符号なしの組は `uint`、ほかは `int`。
//!   `nint` / `nuint` は、対象のポインターの幅の整数。
//! - 列挙型: 同じ列挙型どうしの比較とビット演算、列挙型と数の足し算・引き算（結果は列挙型）、列挙型どうしの
//!   引き算（結果は、もとになる整数の型）。
//! - null 許容の値型（`int?`）: どちらかが null なら、計算の結果は null、比較は偽（`==` と `!=` は null どうしかどうか）。
//! - あふれは折り返す（C# の既定）。0 での割り算と、最小値 ÷ -1 は、実行時と同じく失敗にする。

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    /// `>>>`: 符号を広げない右シフト。
    ShiftRightUnsigned,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

/// 整数の型。計算の結果をこの幅と符号に収める。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntKind {
    I8,
    U8,
    I16,
    U16,
    Char,
    I32,
    U32,
    I64,
    U64,
    /// `nint`。幅は、対象（＝エンジン）のポインターの幅。
    IPtr,
    /// `nuint`
    UPtr,
}

impl IntKind {
    fn name(self) -> &'static str {
        match self {
            IntKind::I8 => "sbyte",
            IntKind::U8 => "byte",
            IntKind::I16 => "short",
            IntKind::U16 => "ushort",
            IntKind::Char => "char",
            IntKind::I32 => "int",
            IntKind::U32 => "uint",
            IntKind::I64 => "long",
            IntKind::U64 => "ulong",
            IntKind::IPtr => "nint",
            IntKind::UPtr => "nuint",
        }
    }

    /// CorElementType から。整数の型でなければ `None`。
    pub fn from_element(element: u8) -> Option<IntKind> {
        Some(match element {
            0x03 => IntKind::Char,
            0x04 => IntKind::I8,
            0x05 => IntKind::U8,
            0x06 => IntKind::I16,
            0x07 => IntKind::U16,
            0x08 => IntKind::I32,
            0x09 => IntKind::U32,
            0x0A => IntKind::I64,
            0x0B => IntKind::U64,
            0x18 => IntKind::IPtr,
            0x19 => IntKind::UPtr,
            _ => return None,
        })
    }

    /// CorElementType。
    pub fn element(self) -> u8 {
        match self {
            IntKind::Char => 0x03,
            IntKind::I8 => 0x04,
            IntKind::U8 => 0x05,
            IntKind::I16 => 0x06,
            IntKind::U16 => 0x07,
            IntKind::I32 => 0x08,
            IntKind::U32 => 0x09,
            IntKind::I64 => 0x0A,
            IntKind::U64 => 0x0B,
            IntKind::IPtr => 0x18,
            IntKind::UPtr => 0x19,
        }
    }

    /// 値をこの型に収める（上のビットを落とし、符号つきなら符号を広げる）。値は、ビット列を i128 で持つ。
    pub fn wrap(self, value: i128) -> i128 {
        match self {
            IntKind::I8 => value as i8 as i128,
            IntKind::U8 => value as u8 as i128,
            IntKind::I16 => value as i16 as i128,
            IntKind::U16 | IntKind::Char => value as u16 as i128,
            IntKind::I32 => value as i32 as i128,
            IntKind::U32 => value as u32 as i128,
            IntKind::I64 => value as i64 as i128,
            IntKind::U64 => value as u64 as i128,
            IntKind::IPtr => value as isize as i128,
            IntKind::UPtr => value as usize as i128,
        }
    }

    /// 値を、この型のバイト列にする。
    pub fn bytes(self, value: i128) -> Vec<u8> {
        (value as u64).to_le_bytes()[..self.bits() as usize / 8].to_vec()
    }

    fn bits(self) -> u32 {
        match self {
            IntKind::I8 | IntKind::U8 => 8,
            IntKind::I16 | IntKind::U16 | IntKind::Char => 16,
            IntKind::I32 | IntKind::U32 => 32,
            IntKind::I64 | IntKind::U64 => 64,
            IntKind::IPtr | IntKind::UPtr => usize::BITS,
        }
    }

    fn is_unsigned(self) -> bool {
        matches!(self, IntKind::U8 | IntKind::U16 | IntKind::Char | IntKind::U32 | IntKind::U64 | IntKind::UPtr)
    }

    /// いちばん小さい値（符号つきの型で、-1 で割るとあふれる値）。
    fn min(self) -> i128 {
        if self.is_unsigned() { 0 } else { -(1i128 << (self.bits() - 1)) }
    }

    /// 単項数値昇格: `int` より小さい型は `int` にする。
    fn promoted(self) -> IntKind {
        match self {
            IntKind::I8 | IntKind::U8 | IntKind::I16 | IntKind::U16 | IntKind::Char => IntKind::I32,
            other => other,
        }
    }

    /// 二項数値昇格: 二つの整数を計算するときの型。C# がエラーにする組（`ulong` と符号つき、など）は `None`。
    fn promote(self, other: IntKind) -> Option<IntKind> {
        use IntKind::*;
        let either = |kind| self == kind || other == kind;
        let partner = |kind| if self == kind { other } else { self };
        if either(U64) {
            return partner(U64).is_unsigned().then_some(U64);
        }
        if either(I64) {
            return (partner(I64) != UPtr).then_some(I64);
        }
        if either(UPtr) {
            return partner(UPtr).is_unsigned().then_some(UPtr);
        }
        if either(U32) {
            // uint と、符号つきの型（nint も）の組は、どちらの値も表せる long。
            return Some(if partner(U32).is_unsigned() { U32 } else { I64 });
        }
        Some(if either(IPtr) { IPtr } else { I32 })
    }

    /// 暗黙に変換できるか（値を失わない変換）。
    pub fn converts_to(self, to: IntKind) -> bool {
        use IntKind::*;
        self == to
            || match self {
                I8 => matches!(to, I16 | I32 | I64 | IPtr),
                U8 => matches!(to, I16 | U16 | I32 | U32 | I64 | U64 | IPtr | UPtr),
                I16 => matches!(to, I32 | I64 | IPtr),
                U16 | Char => matches!(to, U16 | I32 | U32 | I64 | U64 | IPtr | UPtr),
                I32 => matches!(to, I64 | IPtr),
                U32 => matches!(to, I64 | U64 | UPtr),
                IPtr => to == I64,
                UPtr => to == U64,
                I64 | U64 => false,
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Not,
    /// 単項の `+`。値は変わらないが、`int` より小さい型は `int` になる。
    Plus,
    Negate,
    /// `~`: ビットの反転。
    Complement,
}

/// 列挙型。名前はエラーの文に、モジュールと行は、結果の値を対象の中に作るのに使う。
#[derive(Debug, Clone, PartialEq)]
pub struct EnumType {
    pub name: String,
    pub module: usize,
    pub row: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Bool(bool),
    Int { value: i128, kind: IntKind },
    /// 浮動小数点。`single` は `float`（結果も `float` の精度に丸める）。
    Float { value: f64, single: bool },
    /// 列挙型の値。`underlying` は、もとになる整数の型。
    Enum { value: i128, underlying: IntKind, ty: EnumType },
    Str(String),
    Null,
    /// 文字列以外のオブジェクト。中身は対象の中の場所で、同じオブジェクトかどうかの比較にだけ使う。
    /// `object` に入った数（ボックス）もこれになる。C# でも、キャストしないと計算できない。
    Reference(u64),
    /// 演算子に使えない値（構造体など）。中身は、エラーに出す型の名前。
    Other(String),
}

impl Operand {
    fn kind(&self) -> String {
        match self {
            Operand::Bool(_) => "bool".into(),
            Operand::Int { kind, .. } => kind.name().into(),
            Operand::Float { single: true, .. } => "float".into(),
            Operand::Float { single: false, .. } => "double".into(),
            Operand::Enum { ty, .. } => ty.name.clone(),
            Operand::Str(_) => "string".into(),
            Operand::Null => "null".into(),
            Operand::Reference(_) => tr!("object"),
            Operand::Other(type_name) => type_name.clone(),
        }
    }

    /// 値型の値か（null 許容の計算の対象になる）。
    fn is_value(&self) -> bool {
        matches!(self, Operand::Bool(_) | Operand::Int { .. } | Operand::Float { .. } | Operand::Enum { .. })
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Operand::Int { value, .. } => Some(*value as f64),
            Operand::Float { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// `float` としての値。整数からは、`double` を経由せずに直接丸める（二重に丸めると、最後の桁がずれることがある）。
    fn as_f32(&self) -> Option<f32> {
        match self {
            Operand::Int { value, .. } => Some(*value as f32),
            Operand::Float { value, .. } => Some(*value as f32),
            _ => None,
        }
    }
}

/// 二つの整数の型を計算するときの型（二項数値昇格）。C# がエラーにする組は `None`。
pub fn promote_pair(a: IntKind, b: IntKind) -> Option<IntKind> {
    a.promote(b)
}

/// 整数の型の C# のキーワード。
pub fn int_keyword(kind: IntKind) -> &'static str {
    kind.name()
}

/// エラーに出す、値の種類の名前。
pub fn kind_name(operand: &Operand) -> String {
    operand.kind()
}

/// 「この演算子は、この型には使えない」の文。型の誤り（C# ではコンパイルのエラー）で、値にはよらない。
fn not_applicable(operator: &str, operands: &str) -> String {
    tr!("Operator {operator} cannot be used on {operands}.", operator = operator, operands = operands)
}

/// 失敗の文が、型の誤り（`not_applicable` の文）か。値のせいの失敗（0 での割り算、あふれ）と見分けるのに使う。
pub fn is_type_error(message: &str) -> bool {
    // 文はいまの言語で作るので、いまの言語の文の形（{operator} の前と、{operator} と {operands} の間）で見分ける
    // （bool? の文は {operands} の後ろが違うので、後ろでは見分けない）。
    let template = tr_str!("Operator {operator} cannot be used on {operands}.");
    let prefix = template.split("{operator}").next().unwrap_or(template);
    let middle = template.split("{operator}").nth(1).and_then(|rest| rest.split("{operands}").next()).unwrap_or(template);
    message.starts_with(prefix) && message.contains(middle)
}

pub fn symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "||",
        BinaryOp::And => "&&",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::LessOrEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterOrEqual => ">=",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::BitAnd => "&",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::ShiftRightUnsigned => ">>>",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Remainder => "%",
    }
}

/// 整数の結果を、型の幅に収める（あふれたら折り返す。C# の既定と同じ）。
fn int(value: i128, kind: IntKind) -> Operand {
    Operand::Int { value: kind.wrap(value), kind }
}

/// 二つの数を計算するときの型。
enum Common {
    Int(IntKind),
    Single,
    Double,
}

/// 二項数値昇格。数でない値、C# がエラーにする組は `None`。
fn common(left: &Operand, right: &Operand) -> Option<Common> {
    Some(match (left, right) {
        (Operand::Int { kind: a, .. }, Operand::Int { kind: b, .. }) => Common::Int(a.promote(*b)?),
        (Operand::Int { .. } | Operand::Float { .. }, Operand::Int { .. } | Operand::Float { .. }) => {
            if matches!(left, Operand::Float { single: false, .. }) || matches!(right, Operand::Float { single: false, .. }) { Common::Double } else { Common::Single }
        }
        _ => return None,
    })
}

/// 二つの数の大小。NaN が入っていれば `None`。数でない値、C# がエラーにする組は `Err`。
fn compare(left: &Operand, right: &Operand) -> Result<Option<std::cmp::Ordering>, ()> {
    Ok(match common(left, right).ok_or(())? {
        // 整数どうしは、浮動小数点に直さずに比べる（大きな long で誤差が出ないように）。
        Common::Int(_) => match (left, right) {
            (Operand::Int { value: a, .. }, Operand::Int { value: b, .. }) => a.partial_cmp(b),
            _ => return Err(()),
        },
        Common::Single => left.as_f32().zip(right.as_f32()).and_then(|(a, b)| a.partial_cmp(&b)),
        Common::Double => left.as_f64().zip(right.as_f64()).and_then(|(a, b)| a.partial_cmp(&b)),
    })
}

/// 同じ値か。比べられない組み合わせは `None`。
fn equal(left: &Operand, right: &Operand) -> Option<bool> {
    Some(match (left, right) {
        (Operand::Bool(a), Operand::Bool(b)) => a == b,
        (Operand::Str(a), Operand::Str(b)) => a == b,
        (Operand::Reference(a), Operand::Reference(b)) => a == b,
        (Operand::Null, Operand::Null) => true,
        (Operand::Null, Operand::Str(_) | Operand::Reference(_)) | (Operand::Str(_) | Operand::Reference(_), Operand::Null) => false,
        // 文字列とほかのオブジェクトは、同じものではない。
        (Operand::Str(_), Operand::Reference(_)) | (Operand::Reference(_), Operand::Str(_)) => false,
        _ => compare(left, right).ok()?.is_some_and(|o| o.is_eq()),
    })
}

/// 列挙型が入った演算。C# が認めない組は `None`。
fn enum_binary(op: BinaryOp, left: &Operand, right: &Operand) -> Option<Operand> {
    let made = |value: i128, underlying: IntKind, ty: &EnumType| Operand::Enum { value: underlying.wrap(value), underlying, ty: ty.clone() };
    match (left, right) {
        (Operand::Enum { value: a, underlying, ty }, Operand::Enum { value: b, ty: other, .. }) if ty == other => Some(match op {
            BinaryOp::Equal => Operand::Bool(a == b),
            BinaryOp::NotEqual => Operand::Bool(a != b),
            BinaryOp::Less => Operand::Bool(a < b),
            BinaryOp::LessOrEqual => Operand::Bool(a <= b),
            BinaryOp::Greater => Operand::Bool(a > b),
            BinaryOp::GreaterOrEqual => Operand::Bool(a >= b),
            BinaryOp::BitOr => made(a | b, *underlying, ty),
            BinaryOp::BitXor => made(a ^ b, *underlying, ty),
            BinaryOp::BitAnd => made(a & b, *underlying, ty),
            // 列挙型どうしの差は、もとになる整数の型の数。
            BinaryOp::Subtract => int(a - b, *underlying),
            _ => return None,
        }),
        (Operand::Enum { value: a, underlying, ty }, Operand::Int { value: b, kind }) if kind.converts_to(*underlying) => match op {
            BinaryOp::Add => Some(made(a + b, *underlying, ty)),
            BinaryOp::Subtract => Some(made(a - b, *underlying, ty)),
            _ => None,
        },
        (Operand::Int { value: a, kind }, Operand::Enum { value: b, underlying, ty }) if kind.converts_to(*underlying) => match op {
            BinaryOp::Add => Some(made(a + b, *underlying, ty)),
            // 仕様には無いが、Roslyn は `数 - 列挙型` も列挙型の結果で受け付ける（昔のコンパイラとの互換のため）。
            BinaryOp::Subtract => Some(made(a - b, *underlying, ty)),
            _ => None,
        },
        _ => None,
    }
}

pub fn binary(op: BinaryOp, left: &Operand, right: &Operand) -> Result<Operand, String> {
    let mismatch = || not_applicable(symbol(op), &tr!("{left} and {right}", left = left.kind(), right = right.kind()));
    let has_enum = matches!(left, Operand::Enum { .. }) || matches!(right, Operand::Enum { .. });
    let has_string = matches!(left, Operand::Str(_)) || matches!(right, Operand::Str(_));
    if has_enum && !(has_string && op == BinaryOp::Add) {
        return enum_binary(op, left, right).ok_or_else(mismatch);
    }
    Ok(match op {
        BinaryOp::And | BinaryOp::Or => match (left, right) {
            (Operand::Bool(a), Operand::Bool(b)) => Operand::Bool(if op == BinaryOp::And { *a && *b } else { *a || *b }),
            _ => return Err(mismatch()),
        },
        BinaryOp::Equal => Operand::Bool(equal(left, right).ok_or_else(mismatch)?),
        BinaryOp::NotEqual => Operand::Bool(!equal(left, right).ok_or_else(mismatch)?),
        // NaN との比較は、どれも偽。
        BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual => Operand::Bool(compare(left, right).map_err(|()| mismatch())?.is_some_and(|o| match op {
            BinaryOp::Less => o.is_lt(),
            BinaryOp::LessOrEqual => o.is_le(),
            BinaryOp::Greater => o.is_gt(),
            _ => o.is_ge(),
        })),
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder => match (left, right) {
            // 文字列の連結。相手が文字列でなければ、表示用の文字列をつなぐ。
            (Operand::Str(a), Operand::Str(b)) if op == BinaryOp::Add => Operand::Str(format!("{a}{b}")),
            (Operand::Str(a), other) if op == BinaryOp::Add => Operand::Str(format!("{a}{}", text(other).ok_or_else(mismatch)?)),
            (other, Operand::Str(b)) if op == BinaryOp::Add => Operand::Str(format!("{}{b}", text(other).ok_or_else(mismatch)?)),
            _ => match common(left, right).ok_or_else(mismatch)? {
                Common::Int(kind) => {
                    let (Operand::Int { value: a, .. }, Operand::Int { value: b, .. }) = (left, right) else { return Err(mismatch()) };
                    if matches!(op, BinaryOp::Divide | BinaryOp::Remainder) {
                        if *b == 0 {
                            return Err(tr!("Division by zero."));
                        }
                        // 実行時には OverflowException になる（結果が、その型に収まらない）。
                        if !kind.is_unsigned() && *a == kind.min() && *b == -1 {
                            return Err(tr!("Dividing the smallest {type} by -1 overflows.", type = kind.name()));
                        }
                    }
                    int(
                        match op {
                            BinaryOp::Add => a + b,
                            BinaryOp::Subtract => a - b,
                            BinaryOp::Multiply => a.wrapping_mul(*b),
                            BinaryOp::Divide => a / b,
                            _ => a % b,
                        },
                        kind,
                    )
                }
                // float どうしの計算は、float のまま行う。
                Common::Single => {
                    let (a, b) = left.as_f32().zip(right.as_f32()).ok_or_else(mismatch)?;
                    let result = match op {
                        BinaryOp::Add => a + b,
                        BinaryOp::Subtract => a - b,
                        BinaryOp::Multiply => a * b,
                        BinaryOp::Divide => a / b,
                        _ => a % b,
                    };
                    Operand::Float { value: result as f64, single: true }
                }
                Common::Double => {
                    let (a, b) = left.as_f64().zip(right.as_f64()).ok_or_else(mismatch)?;
                    let result = match op {
                        BinaryOp::Add => a + b,
                        BinaryOp::Subtract => a - b,
                        BinaryOp::Multiply => a * b,
                        BinaryOp::Divide => a / b,
                        _ => a % b,
                    };
                    Operand::Float { value: result, single: false }
                }
            },
        },
        BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::BitAnd => match (left, right) {
            // bool では、両辺を必ず評価する論理演算。
            (Operand::Bool(a), Operand::Bool(b)) => Operand::Bool(match op {
                BinaryOp::BitOr => a | b,
                BinaryOp::BitXor => a ^ b,
                _ => a & b,
            }),
            (Operand::Int { value: a, kind: kind_a }, Operand::Int { value: b, kind: kind_b }) => int(
                match op {
                    BinaryOp::BitOr => a | b,
                    BinaryOp::BitXor => a ^ b,
                    _ => a & b,
                },
                kind_a.promote(*kind_b).ok_or_else(mismatch)?,
            ),
            _ => return Err(mismatch()),
        },
        BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::ShiftRightUnsigned => match (left, right) {
            // シフトの結果の型は、左辺の型。ずらす数は int で、型の幅で割った余りを使う（C# の規則）。
            (Operand::Int { value, kind }, Operand::Int { value: count, kind: count_kind }) if count_kind.converts_to(IntKind::I32) => {
                let kind = kind.promoted();
                let count = (*count as u32) & (kind.bits() - 1);
                // 符号なしとして見たビット列（>>> で使う）。
                let unsigned = (*value as u128) & (u128::MAX >> (128 - kind.bits()));
                int(
                    match op {
                        BinaryOp::ShiftLeft => value << count,
                        BinaryOp::ShiftRight => value >> count,
                        _ => (unsigned >> count) as i128,
                    },
                    kind,
                )
            }
            _ => return Err(mismatch()),
        },
    })
}

/// 整数の足し算・引き算・掛け算の結果が、結果の型に収まらないか（`checked` の中では、失敗にする）。
pub fn overflows(op: BinaryOp, left: &Operand, right: &Operand) -> bool {
    let (Operand::Int { value: a, kind: kind_a }, Operand::Int { value: b, kind: kind_b }) = (left, right) else { return false };
    let Some(kind) = kind_a.promote(*kind_b) else { return false };
    let exact = match op {
        BinaryOp::Add => a.checked_add(*b),
        BinaryOp::Subtract => a.checked_sub(*b),
        BinaryOp::Multiply => a.checked_mul(*b),
        _ => return false,
    };
    exact.is_none_or(|exact| kind.wrap(exact) != exact)
}

/// 計算の結果と、その型が null 許容（`int?` など）かどうか。
#[derive(Debug, PartialEq)]
pub struct Lifted {
    pub value: Operand,
    pub nullable: bool,
}

/// 二項演算。null 許容の値型（`int?`）が入っていれば、C# の「持ち上げた演算子」の規則で計算する。
/// `*_nullable` は、その辺のコンパイル時の型が null 許容の値型であること（値が null かどうかとは別）。
pub fn binary_lifted(op: BinaryOp, left: &Operand, left_nullable: bool, right: &Operand, right_nullable: bool) -> Result<Lifted, String> {
    let plain = |value| Lifted { value, nullable: false };
    // 文字列の連結では、null は空の文字列。
    if op == BinaryOp::Add && (matches!(left, Operand::Str(_)) || matches!(right, Operand::Str(_))) {
        return binary(op, left, right).map(plain);
    }
    // null のリテラルは、相手が値型なら、その型の null 許容の null として扱われる。
    let left_null = *left == Operand::Null && (left_nullable || right_nullable || right.is_value());
    let right_null = *right == Operand::Null && (right_nullable || left_nullable || left.is_value());
    if !(left_nullable || right_nullable || left_null || right_null) {
        return binary(op, left, right).map(plain);
    }
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return Err(tr!("Operator {operator} cannot be used on bool? (use & or |, or compare with == true).", operator = symbol(op)));
    }
    if !(left_null || right_null) {
        let comparison = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual | BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual);
        return Ok(Lifted { value: binary(op, left, right)?, nullable: !comparison });
    }
    // 結果は null でも、その演算子が相手の型に使えることは要る（`yes - null` は、C# でもエラー）。
    // null でない側の値を両辺に置いて、計算できるかで確かめる。シフトは、左右で要る型が違うので、int の 0 を相手にする。
    let zero = Operand::Int { value: 0, kind: IntKind::I32 };
    let shift = matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::ShiftRightUnsigned);
    let usable = match (left_null, right_null) {
        (false, true) if matches!(left, Operand::Bool(_) | Operand::Int { .. } | Operand::Float { .. }) => binary(op, left, if shift { &zero } else { left }).is_ok() || is_overflow_or_zero(op, left),
        (true, false) if matches!(right, Operand::Bool(_) | Operand::Int { .. } | Operand::Float { .. }) => binary(op, if shift { &zero } else { right }, right).is_ok() || is_overflow_or_zero(op, right),
        // 列挙型に使えるのは、比較、ビット演算、数との足し算・引き算だけ。
        (false, true) | (true, false) if matches!(left, Operand::Enum { .. }) || matches!(right, Operand::Enum { .. }) => !matches!(op, BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder) && !shift,
        _ => true,
    };
    if !usable {
        return Err(not_applicable(symbol(op), &tr!("{left} and {right}", left = left.kind(), right = right.kind())));
    }
    Ok(match op {
        BinaryOp::Equal => plain(Operand::Bool(left_null && right_null)),
        BinaryOp::NotEqual => plain(Operand::Bool(!(left_null && right_null))),
        BinaryOp::Less | BinaryOp::LessOrEqual | BinaryOp::Greater | BinaryOp::GreaterOrEqual => plain(Operand::Bool(false)),
        // bool? の & と | は、null を「分からない」とする 3 値の論理。片方だけで決まるなら、その値。
        BinaryOp::BitAnd if *left == Operand::Bool(false) || *right == Operand::Bool(false) => Lifted { value: Operand::Bool(false), nullable: true },
        BinaryOp::BitOr if *left == Operand::Bool(true) || *right == Operand::Bool(true) => Lifted { value: Operand::Bool(true), nullable: true },
        _ => Lifted { value: Operand::Null, nullable: true },
    })
}

/// 型としては使える演算子が、値のせいで失敗するだけか（0 での割り算、最小値 ÷ -1）。
fn is_overflow_or_zero(op: BinaryOp, operand: &Operand) -> bool {
    matches!(op, BinaryOp::Divide | BinaryOp::Remainder) && matches!(operand, Operand::Int { .. })
}

/// 単項演算。null 許容の値型の null は、null のまま。
pub fn unary_lifted(op: UnaryOp, operand: &Operand, nullable: bool) -> Result<Lifted, String> {
    if nullable && *operand == Operand::Null {
        return Ok(Lifted { value: Operand::Null, nullable: true });
    }
    Ok(Lifted { value: unary(op, operand)?, nullable })
}

/// 文字列の連結で、文字列でない側をどう書くか。
fn text(operand: &Operand) -> Option<String> {
    Some(match operand {
        Operand::Bool(flag) => if *flag { "True" } else { "False" }.into(),
        Operand::Int { value, kind: IntKind::Char } => String::from_utf16_lossy(&[*value as u16]),
        Operand::Int { value, .. } => value.to_string(),
        Operand::Float { value, .. } => value.to_string(),
        Operand::Str(text) => text.clone(),
        Operand::Null => String::new(),
        Operand::Enum { .. } | Operand::Reference(_) | Operand::Other(_) => return None,
    })
}

/// 数を、プリミティブ型（CorElementType）の値のバイト列に変換する（明示のキャスト）。数でない値、数でない型へは `None`。
///
/// 整数から整数へは、上のビットを落とす（折り返す）。浮動小数点から整数へは、0 の方向に切り捨て、範囲の外は
/// 端の値にする（.NET 9 以降の動作。NaN は 0）。`int` より小さい型へは、いったん `int` にしてから上のビットを落とす。
pub fn convert(operand: &Operand, element: u8) -> Option<Vec<u8>> {
    let whole = match operand {
        Operand::Int { value, .. } | Operand::Enum { value, .. } => Some(*value),
        Operand::Float { .. } => None,
        _ => return None,
    };
    let floating = operand.as_f64();
    Some(match element {
        0x0C => whole.map_or_else(|| floating.unwrap_or_default() as f32, |value| value as f32).to_le_bytes().to_vec(),
        0x0D => whole.map_or_else(|| floating.unwrap_or_default(), |value| value as f64).to_le_bytes().to_vec(),
        _ => {
            let kind = IntKind::from_element(element)?;
            let value = whole.unwrap_or_else(|| {
                let value = floating.unwrap_or_default();
                match kind {
                    IntKind::U32 => value as u32 as i128,
                    IntKind::I64 => value as i64 as i128,
                    IntKind::U64 => value as u64 as i128,
                    IntKind::IPtr => value as isize as i128,
                    IntKind::UPtr => value as usize as i128,
                    _ => value as i32 as i128,
                }
            });
            kind.bytes(kind.wrap(value))
        }
    })
}

pub fn unary(op: UnaryOp, operand: &Operand) -> Result<Operand, String> {
    let symbol = match op {
        UnaryOp::Not => "!",
        UnaryOp::Plus => "+",
        UnaryOp::Negate => "-",
        UnaryOp::Complement => "~",
    };
    let mismatch = || not_applicable(symbol, &operand.kind());
    match (op, operand) {
        (UnaryOp::Not, Operand::Bool(flag)) => Ok(Operand::Bool(!flag)),
        (UnaryOp::Plus, Operand::Int { value, kind }) => Ok(int(*value, kind.promoted())),
        (UnaryOp::Plus | UnaryOp::Negate, Operand::Float { value, single }) => Ok(Operand::Float { value: if op == UnaryOp::Negate { -value } else { *value }, single: *single }),
        // uint の符号反転は long になる（C# の規則）。ulong と nuint には使えない。
        (UnaryOp::Negate, Operand::Int { value, kind: IntKind::U32 }) => Ok(int(-value, IntKind::I64)),
        (UnaryOp::Negate, Operand::Int { kind: IntKind::U64 | IntKind::UPtr, .. }) => Err(mismatch()),
        (UnaryOp::Negate, Operand::Int { value, kind }) => Ok(int(-value, kind.promoted())),
        (UnaryOp::Complement, Operand::Int { value, kind }) => Ok(int(!value, kind.promoted())),
        (UnaryOp::Complement, Operand::Enum { value, underlying, ty }) => Ok(Operand::Enum { value: underlying.wrap(!value), underlying: *underlying, ty: ty.clone() }),
        _ => Err(mismatch()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(value: i128) -> Operand {
        Operand::Int { value, kind: IntKind::I32 }
    }

    fn u(value: i128) -> Operand {
        Operand::Int { value, kind: IntKind::U32 }
    }

    fn l(value: i128) -> Operand {
        Operand::Int { value, kind: IntKind::I64 }
    }

    fn ul(value: i128) -> Operand {
        Operand::Int { value, kind: IntKind::U64 }
    }

    fn d(value: f64) -> Operand {
        Operand::Float { value, single: false }
    }

    fn f(value: f64) -> Operand {
        Operand::Float { value, single: true }
    }

    fn s(text: &str) -> Operand {
        Operand::Str(text.into())
    }

    #[test]
    fn integers_stay_int_and_wrap_like_csharp() {
        assert_eq!(binary(BinaryOp::Add, &i(30), &i(1)), Ok(i(31)));
        assert_eq!(binary(BinaryOp::Divide, &i(7), &i(2)), Ok(i(3)));
        assert_eq!(binary(BinaryOp::Remainder, &i(-7), &i(2)), Ok(i(-1)));
        assert_eq!(binary(BinaryOp::Add, &i(i32::MAX as i128), &i(1)), Ok(i(i32::MIN as i128)));
    }

    #[test]
    fn a_long_operand_makes_the_result_long() {
        assert_eq!(binary(BinaryOp::Add, &l(i32::MAX as i128), &i(1)), Ok(l(i32::MAX as i128 + 1)));
        assert_eq!(binary(BinaryOp::Multiply, &i(2), &l(3)), Ok(l(6)));
    }

    #[test]
    fn unsigned_integers_follow_the_promotion_rules() {
        // uint どうしは uint のまま。あふれたら折り返す。
        assert_eq!(binary(BinaryOp::Add, &u(4_000_000_000), &u(1)), Ok(u(4_000_000_001)));
        assert_eq!(binary(BinaryOp::Multiply, &u(4_000_000_000), &u(2)), Ok(u(3_705_032_704)));
        // uint と int の組は long。
        assert_eq!(binary(BinaryOp::Add, &u(4_000_000_000), &i(-1)), Ok(l(3_999_999_999)));
        // ulong は ulong のまま。上半分の値も正しく扱う。
        assert_eq!(binary(BinaryOp::Add, &ul(u64::MAX as i128), &ul(1)), Ok(ul(0)));
        assert_eq!(binary(BinaryOp::Divide, &ul(u64::MAX as i128), &ul(2)), Ok(ul((u64::MAX / 2) as i128)));
        assert_eq!(binary(BinaryOp::Greater, &ul(u64::MAX as i128), &ul(1)), Ok(Operand::Bool(true)));
    }

    #[test]
    fn a_double_operand_makes_the_result_double() {
        assert_eq!(binary(BinaryOp::Add, &i(1), &d(0.5)), Ok(d(1.5)));
        assert_eq!(binary(BinaryOp::Divide, &d(1.0), &i(4)), Ok(d(0.25)));
        assert_eq!(binary(BinaryOp::Divide, &d(1.0), &i(0)), Ok(d(f64::INFINITY)), "浮動小数点の 0 除算は無限大");
        assert_eq!(binary(BinaryOp::Add, &f(0.5), &d(1.0)), Ok(d(1.5)));
    }

    #[test]
    fn float_stays_float_and_keeps_float_precision() {
        assert_eq!(binary(BinaryOp::Add, &f(0.5), &i(1)), Ok(f(1.5)));
        assert_eq!(binary(BinaryOp::Multiply, &f(0.5), &f(0.5)), Ok(f(0.25)));
        // 0.1f + 0.2f は、float の精度で丸めた値になる（double で足した値とは違う）。
        let Ok(Operand::Float { value, single: true }) = binary(BinaryOp::Add, &f(0.1f32 as f64), &f(0.2f32 as f64)) else { panic!("float になっていない") };
        assert_eq!(value, (0.1f32 + 0.2f32) as f64);
    }

    #[test]
    fn dividing_an_integer_by_zero_is_an_error() {
        assert!(binary(BinaryOp::Divide, &i(1), &i(0)).unwrap_err().contains("0 で割る"));
        assert!(binary(BinaryOp::Remainder, &l(1), &i(0)).is_err());
    }

    #[test]
    fn bitwise_operators_work_on_integers_and_bools() {
        assert_eq!(binary(BinaryOp::BitAnd, &i(7), &i(3)), Ok(i(3)));
        assert_eq!(binary(BinaryOp::BitOr, &i(7), &i(8)), Ok(i(15)));
        assert_eq!(binary(BinaryOp::BitXor, &i(7), &i(3)), Ok(i(4)));
        assert_eq!(binary(BinaryOp::BitAnd, &i(-1), &l(0xFF)), Ok(l(0xFF)));
        assert_eq!(binary(BinaryOp::BitXor, &Operand::Bool(true), &Operand::Bool(true)), Ok(Operand::Bool(false)));
        assert_eq!(unary(UnaryOp::Complement, &i(7)), Ok(i(-8)));
        assert_eq!(unary(UnaryOp::Complement, &u(0)), Ok(u(u32::MAX as i128)));
        assert!(binary(BinaryOp::BitAnd, &d(1.0), &i(1)).is_err());
        assert!(unary(UnaryOp::Complement, &Operand::Bool(true)).is_err());
    }

    #[test]
    fn shifts_keep_the_type_of_the_left_operand_and_mask_the_count() {
        assert_eq!(binary(BinaryOp::ShiftLeft, &i(7), &i(2)), Ok(i(28)));
        assert_eq!(binary(BinaryOp::ShiftRight, &i(7), &i(1)), Ok(i(3)));
        assert_eq!(binary(BinaryOp::ShiftRight, &i(-8), &i(1)), Ok(i(-4)), "符号を広げる");
        assert_eq!(binary(BinaryOp::ShiftRightUnsigned, &i(-8), &i(1)), Ok(i(0x7FFF_FFFC)), "符号を広げない");
        assert_eq!(binary(BinaryOp::ShiftLeft, &i(1), &i(33)), Ok(i(2)), "int では、ずらす数の下 5 ビットだけを使う");
        assert_eq!(binary(BinaryOp::ShiftLeft, &l(1), &i(33)), Ok(l(1 << 33)));
        assert_eq!(binary(BinaryOp::ShiftLeft, &i(1), &i(31)), Ok(i(i32::MIN as i128)));
        assert_eq!(binary(BinaryOp::ShiftRight, &u(0x8000_0000), &i(4)), Ok(u(0x0800_0000)));
    }

    #[test]
    fn comparisons_give_bool() {
        assert_eq!(binary(BinaryOp::Less, &i(1), &i(2)), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::GreaterOrEqual, &i(2), &d(2.0)), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Greater, &l(i64::MAX as i128), &l(i64::MAX as i128 - 1)), Ok(Operand::Bool(true)), "大きな long も正しく比べる");
        assert_eq!(binary(BinaryOp::Less, &d(f64::NAN), &i(1)), Ok(Operand::Bool(false)));
        assert!(binary(BinaryOp::Less, &s("a"), &s("b")).is_err());
    }

    #[test]
    fn equality_compares_by_content_for_values_and_strings() {
        assert_eq!(binary(BinaryOp::Equal, &i(3), &l(3)), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Equal, &i(3), &d(3.0)), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Equal, &s("A-1"), &s("A-1")), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::NotEqual, &s("A-1"), &s("B-2")), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Equal, &Operand::Bool(true), &Operand::Bool(false)), Ok(Operand::Bool(false)));
    }

    #[test]
    fn equality_compares_objects_by_identity_and_knows_null() {
        assert_eq!(binary(BinaryOp::Equal, &Operand::Reference(0x1000), &Operand::Reference(0x1000)), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Equal, &Operand::Reference(0x1000), &Operand::Reference(0x2000)), Ok(Operand::Bool(false)));
        assert_eq!(binary(BinaryOp::Equal, &Operand::Null, &Operand::Null), Ok(Operand::Bool(true)));
        assert_eq!(binary(BinaryOp::Equal, &s("x"), &Operand::Null), Ok(Operand::Bool(false)));
        assert_eq!(binary(BinaryOp::NotEqual, &Operand::Reference(0x1000), &Operand::Null), Ok(Operand::Bool(true)));
    }

    #[test]
    fn values_of_different_kinds_cannot_be_compared() {
        let error = binary(BinaryOp::Equal, &i(1), &s("1")).unwrap_err();
        assert!(error.contains("==") && error.contains("int") && error.contains("string"), "{error}");
        assert!(binary(BinaryOp::Equal, &Operand::Bool(true), &i(1)).is_err());
        assert!(binary(BinaryOp::Equal, &Operand::Other("Point".into()), &Operand::Other("Point".into())).unwrap_err().contains("Point"));
    }

    #[test]
    fn plus_joins_strings() {
        assert_eq!(binary(BinaryOp::Add, &s("a"), &s("b")), Ok(s("ab")));
        assert_eq!(binary(BinaryOp::Add, &s("n="), &i(3)), Ok(s("n=3")));
        assert_eq!(binary(BinaryOp::Add, &Operand::Bool(true), &s("!")), Ok(s("True!")));
        assert_eq!(binary(BinaryOp::Add, &s("x"), &Operand::Null), Ok(s("x")));
        assert!(binary(BinaryOp::Subtract, &s("a"), &s("b")).is_err());
    }

    #[test]
    fn logical_operators_need_bool_on_both_sides() {
        assert_eq!(binary(BinaryOp::And, &Operand::Bool(true), &Operand::Bool(false)), Ok(Operand::Bool(false)));
        assert_eq!(binary(BinaryOp::Or, &Operand::Bool(true), &Operand::Bool(false)), Ok(Operand::Bool(true)));
        assert!(binary(BinaryOp::And, &i(1), &Operand::Bool(true)).is_err());
    }

    #[test]
    fn a_cast_truncates_and_wraps_like_unchecked_csharp() {
        assert_eq!(convert(&i(300), 0x05), Some(vec![44]), "(byte)300");
        assert_eq!(convert(&i(-1), 0x09), Some(u32::MAX.to_le_bytes().to_vec()), "(uint)-1");
        assert_eq!(convert(&d(2.9), 0x08), Some(2i32.to_le_bytes().to_vec()), "(int)2.9 は切り捨て");
        assert_eq!(convert(&d(-2.9), 0x08), Some((-2i32).to_le_bytes().to_vec()));
        assert_eq!(convert(&i(7), 0x0A), Some(7i64.to_le_bytes().to_vec()), "(long)7");
        assert_eq!(convert(&i(7), 0x0D), Some(7f64.to_le_bytes().to_vec()), "(double)7");
        assert_eq!(convert(&i(120), 0x03), Some(120u16.to_le_bytes().to_vec()), "(char)120");
        assert_eq!(convert(&s("7"), 0x08), None);
        assert_eq!(convert(&i(1), 0x02), None, "数から bool へは変換できない");
    }

    #[test]
    fn unary_operators_check_their_operand() {
        assert_eq!(unary(UnaryOp::Not, &Operand::Bool(true)), Ok(Operand::Bool(false)));
        assert_eq!(unary(UnaryOp::Negate, &i(5)), Ok(i(-5)));
        assert_eq!(unary(UnaryOp::Negate, &d(1.5)), Ok(d(-1.5)));
        assert_eq!(unary(UnaryOp::Negate, &u(5)), Ok(l(-5)), "uint の符号反転は long");
        assert!(unary(UnaryOp::Negate, &ul(5)).is_err());
        assert!(unary(UnaryOp::Not, &i(1)).unwrap_err().contains("int"));
        assert!(unary(UnaryOp::Negate, &s("a")).is_err());
    }

    fn kind(value: i128, kind: IntKind) -> Operand {
        Operand::Int { value, kind }
    }

    fn tone(value: i128) -> Operand {
        Operand::Enum { value, underlying: IntKind::I32, ty: EnumType { name: "Tone".into(), module: 1, row: 2 } }
    }

    #[test]
    fn small_unsigned_types_stay_unsigned_next_to_uint() {
        // byte と uint は uint（byte は uint に暗黙に変換できる）。sbyte と uint は long。
        assert_eq!(binary(BinaryOp::Add, &kind(200, IntKind::U8), &u(4_000_000_000)), Ok(u(4_000_000_200)));
        assert_eq!(binary(BinaryOp::Add, &kind('x' as i128, IntKind::Char), &u(1)), Ok(u(121)));
        assert_eq!(binary(BinaryOp::Add, &kind(-5, IntKind::I8), &u(4_000_000_000)), Ok(l(3_999_999_995)));
        assert_eq!(binary(BinaryOp::Add, &kind(200, IntKind::U8), &kind(200, IntKind::U8)), Ok(i(400)), "byte どうしは int");
        assert_eq!(binary(BinaryOp::Add, &kind(200, IntKind::U8), &ul(1)), Ok(ul(201)));
    }

    #[test]
    fn ulong_and_a_signed_integer_cannot_be_mixed() {
        assert!(binary(BinaryOp::Add, &ul(1), &i(1)).is_err());
        assert!(binary(BinaryOp::Equal, &l(1), &ul(1)).is_err());
        assert!(binary(BinaryOp::Less, &kind(1, IntKind::I8), &ul(1)).is_err());
        assert!(binary(BinaryOp::BitAnd, &ul(1), &kind(1, IntKind::IPtr)).is_err());
    }

    #[test]
    fn native_integers_have_their_own_type() {
        let n = |value| kind(value, IntKind::IPtr);
        assert_eq!(binary(BinaryOp::Add, &n(5), &i(1)), Ok(n(6)));
        assert_eq!(binary(BinaryOp::Add, &n(5), &l(1)), Ok(l(6)));
        assert_eq!(binary(BinaryOp::Add, &n(5), &u(1)), Ok(l(6)));
        assert_eq!(unary(UnaryOp::Negate, &n(5)), Ok(n(-5)));
    }

    #[test]
    fn unary_plus_promotes_small_types() {
        assert_eq!(unary(UnaryOp::Plus, &kind(-5, IntKind::I8)), Ok(i(-5)));
        assert_eq!(unary(UnaryOp::Plus, &u(7)), Ok(u(7)));
        assert_eq!(unary(UnaryOp::Plus, &d(1.5)), Ok(d(1.5)));
        assert!(unary(UnaryOp::Plus, &Operand::Bool(true)).is_err());
        assert!(unary(UnaryOp::Negate, &ul(1)).is_err());
    }

    #[test]
    fn the_smallest_value_divided_by_minus_one_overflows() {
        assert!(binary(BinaryOp::Divide, &i(i32::MIN as i128), &i(-1)).unwrap_err().contains("あふれ"));
        assert!(binary(BinaryOp::Remainder, &l(i64::MIN as i128), &i(-1)).is_err());
        assert_eq!(binary(BinaryOp::Divide, &i(i32::MIN as i128), &l(-1)), Ok(l(-(i32::MIN as i128))), "long なら収まる");
    }

    #[test]
    fn type_errors_are_told_apart_from_failures_caused_by_values() {
        // 型に使えない演算子は、型の誤り。0 での割り算は、値のせいの失敗。
        assert!(binary(BinaryOp::Add, &Operand::Bool(true), &Operand::Bool(false)).is_err_and(|message| is_type_error(&message)));
        assert!(unary(UnaryOp::Negate, &Operand::Bool(true)).is_err_and(|message| is_type_error(&message)));
        assert!(binary_lifted(BinaryOp::And, &i(1), true, &i(1), true).is_err_and(|message| is_type_error(&message)));
        assert!(binary(BinaryOp::Divide, &i(1), &i(0)).is_err_and(|message| !is_type_error(&message)));
    }

    #[test]
    fn enums_keep_their_type() {
        assert_eq!(binary(BinaryOp::BitOr, &tone(1), &tone(2)), Ok(tone(3)));
        assert_eq!(binary(BinaryOp::Add, &tone(1), &i(1)), Ok(tone(2)));
        assert_eq!(binary(BinaryOp::Add, &i(1), &tone(1)), Ok(tone(2)));
        assert_eq!(binary(BinaryOp::Subtract, &tone(2), &tone(1)), Ok(i(1)), "列挙型どうしの差は数");
        assert_eq!(binary(BinaryOp::Less, &tone(1), &tone(2)), Ok(Operand::Bool(true)));
        assert_eq!(unary(UnaryOp::Complement, &tone(1)), Ok(tone(-2)));
        assert!(binary(BinaryOp::Equal, &tone(1), &i(1)).is_err(), "数とは比べられない");
        assert!(binary(BinaryOp::Add, &tone(1), &tone(1)).is_err());
        assert!(binary(BinaryOp::Multiply, &tone(1), &i(2)).is_err());
        assert_eq!(binary(BinaryOp::Subtract, &i(2), &tone(1)), Ok(tone(1)), "Roslyn は 数 - 列挙型 も受け付ける");
        assert!(binary(BinaryOp::Add, &tone(1), &l(1)).is_err(), "long は int の列挙型に暗黙に変換できない");
    }

    #[test]
    fn float_arithmetic_is_done_in_float() {
        // ulong を double 経由で float にすると、二重に丸められて最後の桁がずれる。
        let Ok(Operand::Float { value, single: true }) = binary(BinaryOp::Multiply, &ul(10_000_000_000_000_000_000), &f(1.5)) else { panic!("float になっていない") };
        assert_eq!(value, (10_000_000_000_000_000_000u64 as f32 * 1.5f32) as f64);
        assert_eq!(binary(BinaryOp::Equal, &i(16_777_217), &f(16_777_216.0)), Ok(Operand::Bool(true)), "int を float にしてから比べる");
    }

    #[test]
    fn the_shift_count_must_be_an_int() {
        assert!(binary(BinaryOp::ShiftLeft, &i(1), &l(1)).is_err());
        assert!(binary(BinaryOp::ShiftLeft, &i(1), &u(1)).is_err());
        assert_eq!(binary(BinaryOp::ShiftLeft, &kind(200, IntKind::U8), &kind(1, IntKind::U8)), Ok(i(400)), "byte は int に広げてからずらす");
    }

    #[test]
    fn nullable_operands_follow_the_lifted_rules() {
        let lifted = |op, left: &Operand, left_nullable, right: &Operand, right_nullable| binary_lifted(op, left, left_nullable, right, right_nullable).map(|r| (r.value, r.nullable));
        assert_eq!(lifted(BinaryOp::Add, &i(5), true, &i(1), false), Ok((i(6), true)), "int? + int は int?");
        assert_eq!(lifted(BinaryOp::Add, &Operand::Null, true, &i(1), false), Ok((Operand::Null, true)));
        assert_eq!(lifted(BinaryOp::Add, &i(1), false, &Operand::Null, false), Ok((Operand::Null, true)), "null のリテラルも同じ");
        assert_eq!(lifted(BinaryOp::Less, &Operand::Null, true, &i(1), false), Ok((Operand::Bool(false), false)));
        assert_eq!(lifted(BinaryOp::Equal, &Operand::Null, true, &Operand::Null, false), Ok((Operand::Bool(true), false)));
        assert_eq!(lifted(BinaryOp::Equal, &i(5), false, &Operand::Null, false), Ok((Operand::Bool(false), false)));
        assert_eq!(lifted(BinaryOp::NotEqual, &i(5), true, &Operand::Null, false), Ok((Operand::Bool(true), false)));
        assert_eq!(lifted(BinaryOp::Less, &i(5), true, &i(6), false), Ok((Operand::Bool(true), false)), "比較の結果は bool");
        // bool? の 3 値の論理
        assert_eq!(lifted(BinaryOp::BitAnd, &Operand::Bool(false), false, &Operand::Null, true), Ok((Operand::Bool(false), true)));
        assert_eq!(lifted(BinaryOp::BitAnd, &Operand::Bool(true), false, &Operand::Null, true), Ok((Operand::Null, true)));
        assert_eq!(lifted(BinaryOp::BitOr, &Operand::Null, true, &Operand::Bool(true), false), Ok((Operand::Bool(true), true)));
        assert!(lifted(BinaryOp::And, &Operand::Bool(true), true, &Operand::Bool(true), false).is_err());
        // 参照型の null は、これまでどおり。
        assert_eq!(lifted(BinaryOp::Equal, &s("x"), false, &Operand::Null, false), Ok((Operand::Bool(false), false)));
        assert_eq!(lifted(BinaryOp::Add, &s("x"), false, &Operand::Null, true), Ok((s("x"), false)));
        // 結果が null でも、型に使えない演算子はエラー。
        assert!(lifted(BinaryOp::Subtract, &Operand::Bool(true), false, &Operand::Null, false).is_err());
        assert!(lifted(BinaryOp::BitAnd, &d(1.0), false, &Operand::Null, false).is_err());
        assert_eq!(lifted(BinaryOp::Divide, &i(0), false, &Operand::Null, false), Ok((Operand::Null, true)), "0 / null は null");
        assert_eq!(lifted(BinaryOp::ShiftLeft, &l(1), false, &Operand::Null, false), Ok((Operand::Null, true)));
        assert_eq!(unary_lifted(UnaryOp::Negate, &i(5), true), Ok(Lifted { value: i(-5), nullable: true }));
        assert_eq!(unary_lifted(UnaryOp::Negate, &Operand::Null, true), Ok(Lifted { value: Operand::Null, nullable: true }));
    }

    #[test]
    fn overflow_is_detected_for_checked_arithmetic() {
        assert!(overflows(BinaryOp::Add, &i(i32::MAX as i128), &i(1)));
        assert!(overflows(BinaryOp::Multiply, &ul(u64::MAX as i128), &ul(2)));
        assert!(overflows(BinaryOp::Subtract, &u(0), &u(1)));
        assert!(!overflows(BinaryOp::Add, &i(1), &i(2)));
        assert!(!overflows(BinaryOp::Add, &i(i32::MAX as i128), &l(1)), "long なら収まる");
        assert!(!overflows(BinaryOp::Divide, &i(1), &i(2)));
    }

    #[test]
    fn floats_cast_to_integers_saturate() {
        assert_eq!(convert(&f(f32::MAX as f64), 0x08), Some(i32::MAX.to_le_bytes().to_vec()));
        assert_eq!(convert(&d(-1e30), 0x0A), Some(i64::MIN.to_le_bytes().to_vec()));
        assert_eq!(convert(&d(f64::NAN), 0x08), Some(0i32.to_le_bytes().to_vec()));
        assert_eq!(convert(&d(-1.0), 0x09), Some(0u32.to_le_bytes().to_vec()));
        assert_eq!(convert(&tone(2), 0x08), Some(2i32.to_le_bytes().to_vec()), "列挙型から数へ");
    }
}
