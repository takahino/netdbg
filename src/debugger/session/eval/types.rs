//! 式の「コンパイル時の型」のうち、評価した値から分かる範囲と、型どうしの暗黙の変換の規則。
//!
//! 条件演算子（`c ? x : y`）や `??` の結果の型、`as` や定数パターンが使える組み合わせかどうかは、C# では
//! コンパイル時の型で決まる。値型の値は、実行時の型がそのままコンパイル時の型なので（`int` の値は `int`）、
//! そこから規則を当てはめる。参照型は、実行時の型しか分からないので、検査をゆるくする（docs/DECISIONS.md D-027）。
//!
//! 規則は、総当たり試験（D-026）のコンパイラの結果に合わせてある。

use super::super::operators::{EnumType, IntKind};

/// 型の種類。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Kind {
    Bool,
    Int(IntKind),
    Float { single: bool },
    Decimal,
    Enum(EnumType),
    /// そのほかの構造体。中身は名前。
    Struct(String),
    Str,
    /// `object`（に入った値型）。どの値からも変換できる。
    Object,
    /// そのほかの参照型。実行時の型しか分からない。
    Class(String),
    /// `null` のリテラル。型を持たない。
    Null,
}

/// 式の型。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Typed {
    pub kind: Kind,
    /// null 許容の値型（`int?`）か。
    pub nullable: bool,
    /// 整数の定数なら、その値。定数は、値が収まる整数の型へ暗黙に変換できる（`byte b = 1`）。
    pub constant: Option<i128>,
}

impl Typed {
    pub fn of(kind: Kind) -> Typed {
        Typed { kind, nullable: false, constant: None }
    }

    /// 値型か（null 許容も含む）。
    pub fn is_value_type(&self) -> bool {
        matches!(self.kind, Kind::Bool | Kind::Int(_) | Kind::Float { .. } | Kind::Decimal | Kind::Enum(_) | Kind::Struct(_))
    }

    /// null になれない値型か。
    pub fn never_null(&self) -> bool {
        self.is_value_type() && !self.nullable
    }

    /// 数（整数、浮動小数点、decimal）か列挙型か。この間には、明示の変換がある。
    fn is_numeric_or_enum(&self) -> bool {
        matches!(self.kind, Kind::Int(_) | Kind::Float { .. } | Kind::Decimal | Kind::Enum(_))
    }

    /// null 許容を外した型。
    pub fn underlying(&self) -> Typed {
        Typed { nullable: false, ..self.clone() }
    }

    /// 型のキーワード（`int`、`double` など）から。分からない名前は構造体とする。
    pub fn from_keyword(name: &str) -> Typed {
        Typed::of(match name {
            "bool" => Kind::Bool,
            "sbyte" => Kind::Int(IntKind::I8),
            "byte" => Kind::Int(IntKind::U8),
            "short" => Kind::Int(IntKind::I16),
            "ushort" => Kind::Int(IntKind::U16),
            "char" => Kind::Int(IntKind::Char),
            "int" => Kind::Int(IntKind::I32),
            "uint" => Kind::Int(IntKind::U32),
            "long" => Kind::Int(IntKind::I64),
            "ulong" => Kind::Int(IntKind::U64),
            "nint" | "System.IntPtr" => Kind::Int(IntKind::IPtr),
            "nuint" | "System.UIntPtr" => Kind::Int(IntKind::UPtr),
            "float" => Kind::Float { single: true },
            "double" => Kind::Float { single: false },
            "decimal" => Kind::Decimal,
            "string" => Kind::Str,
            "object" => Kind::Object,
            other => Kind::Struct(other.to_string()),
        })
    }
}

/// 型から型への暗黙の変換があるか（定数の変換は含めない）。
pub(super) fn converts(from: &Typed, to: &Typed) -> bool {
    // null 許容の値は、null 許容でない値型へは暗黙に変換できない（object へは、ボックス化で変換できる）。
    if from.nullable && !to.nullable && to.kind != Kind::Object {
        return false;
    }
    match (&from.kind, &to.kind) {
        // `null` は、参照型と null 許容の値型へ。`null` どうしは、型が決まらない。
        (Kind::Null, Kind::Null) => false,
        (Kind::Null, Kind::Str | Kind::Object | Kind::Class(_)) => true,
        (Kind::Null, _) => to.nullable,
        (_, Kind::Null) => false,
        // どの値も object へ（値型はボックス化）。
        (_, Kind::Object) => true,
        (Kind::Int(a), Kind::Int(b)) => a.converts_to(*b),
        (Kind::Int(_), Kind::Float { .. } | Kind::Decimal) => true,
        (Kind::Float { single: true }, Kind::Float { .. }) | (Kind::Float { single: false }, Kind::Float { single: false }) => true,
        (a, b) => a == b,
    }
}

/// 定数の変換も含めて、暗黙に変換できるか（`int` の定数は、値が収まれば `byte` や `uint` へ。0 は列挙型へ）。
pub(super) fn converts_with_constant(from: &Typed, to: &Typed) -> bool {
    if converts(from, to) {
        return true;
    }
    match (from.constant, &to.kind) {
        // long の定数は、負でなければ ulong へだけ。
        (Some(value), Kind::Int(IntKind::U64)) if from.kind == Kind::Int(IntKind::I64) => value >= 0,
        (Some(_), _) if from.kind != Kind::Int(IntKind::I32) => false,
        // int の定数からの変換があるのは、sbyte、byte、short、ushort、uint、ulong（と nuint）へだけ。char へは無い。
        (Some(value), Kind::Int(kind)) => !matches!(kind, IntKind::Char) && kind.wrap(value) == value,
        (Some(0), Kind::Enum(_)) => true,
        _ => false,
    }
}

/// 条件演算子の、両側の型を一つにする。できなければ `None`（C# でもエラー）。
///
/// 片方からもう片方へだけ暗黙の変換があれば、変換先の型。型どうしの変換が無いときだけ、定数の変換を見る
/// （`flag ? b : 1` は int。`flag ? u : 1` は uint）。
pub(super) fn unify(a: &Typed, b: &Typed) -> Option<Typed> {
    let plain = |typed: &Typed| Typed { constant: None, ..typed.clone() };
    match (converts(a, b), converts(b, a)) {
        (true, _) => Some(plain(b)),
        (false, true) => Some(plain(a)),
        (false, false) if converts_with_constant(a, b) => Some(plain(b)),
        (false, false) if converts_with_constant(b, a) => Some(plain(a)),
        _ => None,
    }
}

/// `a ?? b` の結果の型。できなければ `None`。`a` は null になれる型であること。
pub(super) fn coalesce(a: &Typed, b: &Typed) -> Option<Typed> {
    let plain = |typed: &Typed| Typed { constant: None, ..typed.clone() };
    let inner = a.underlying();
    if a.nullable && converts_with_constant(b, &inner) {
        return Some(plain(&inner));
    }
    if converts_with_constant(b, a) {
        return Some(plain(a));
    }
    converts(&inner, b).then(|| plain(b))
}

/// 値型どうしの明示の変換があるか（同じ型か、数と列挙型の間）。`(int?)perhapsNot` のような、null 許容の値の
/// キャストが書けるかの判断に使う。
pub(super) fn explicitly_converts(from: &Typed, to: &Typed) -> bool {
    from.kind == to.kind || (from.is_numeric_or_enum() && to.is_numeric_or_enum())
}

/// `値 as 型` が書けるか。`target_is_value` は、変換先が値型（null 許容を外したもの）かどうか。
/// 変換先は、参照型か null 許容の値型でなければならない。値型の値は、ボックス化できる先（`fits`: object、実装している
/// インターフェイス）か、明示の変換がある null 許容の型へだけ。
pub(super) fn as_is_allowed(source: &Typed, target: &Typed, target_is_value: bool, fits: bool) -> Result<(), String> {
    if target_is_value && !target.nullable {
        return Err(tr!("The target of as must be a reference type or a nullable value type (int?, for example)."));
    }
    // string は封じられた型で、値型の値にはならない（`s as int?` は、C# でもエラー）。
    if source.kind == Kind::Str && target_is_value {
        return Err(tr!("A string value cannot be converted to a value type with as."));
    }
    if !source.is_value_type() {
        return Ok(());
    }
    let allowed = if target_is_value { source.underlying().kind == target.kind || (source.is_numeric_or_enum() && target.is_numeric_or_enum()) } else { fits };
    if allowed { Ok(()) } else { Err(tr!("A value of this type cannot be converted to that type with as.")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(kind: IntKind) -> Typed {
        Typed::of(Kind::Int(kind))
    }

    fn constant(value: i128) -> Typed {
        Typed { constant: Some(value), ..int(IntKind::I32) }
    }

    fn nullable(typed: Typed) -> Typed {
        Typed { nullable: true, ..typed }
    }

    fn tone() -> Typed {
        Typed::of(Kind::Enum(EnumType { name: "Tone".into(), module: 1, row: 2 }))
    }

    #[test]
    fn the_conditional_operator_takes_the_wider_type() {
        let double = Typed::of(Kind::Float { single: false });
        let float = Typed::of(Kind::Float { single: true });
        assert_eq!(unify(&int(IntKind::U8), &int(IntKind::I32)), Some(int(IntKind::I32)));
        assert_eq!(unify(&int(IntKind::I64), &float), Some(float.clone()));
        assert_eq!(unify(&float, &double), Some(double));
        assert_eq!(unify(&int(IntKind::IPtr), &int(IntKind::I64)), Some(int(IntKind::I64)));
        // 互いに変換できない組は、エラー。
        assert_eq!(unify(&int(IntKind::I8), &int(IntKind::U8)), None);
        assert_eq!(unify(&int(IntKind::U64), &int(IntKind::I64)), None);
        assert_eq!(unify(&int(IntKind::Char), &int(IntKind::U8)), None);
        assert_eq!(unify(&Typed::of(Kind::Str), &int(IntKind::I32)), None);
    }

    #[test]
    fn constants_convert_only_when_types_do_not() {
        // byte と定数 1: byte から int への変換があるので int。
        assert_eq!(unify(&int(IntKind::U8), &constant(1)), Some(int(IntKind::I32)));
        // uint と定数 1: 型どうしの変換は無い。定数 1 は uint に収まるので uint。-1 は収まらない。
        assert_eq!(unify(&int(IntKind::U32), &constant(1)), Some(int(IntKind::U32)));
        assert_eq!(unify(&int(IntKind::U32), &constant(-1)), None);
        assert_eq!(unify(&tone(), &constant(0)), Some(tone()));
        assert_eq!(unify(&tone(), &constant(1)), None);
        // uint の定数（1u）は、int へは変換できない。相手の int の定数が uint になる。
        assert_eq!(unify(&Typed { constant: Some(1), ..int(IntKind::U32) }, &constant(0)), Some(int(IntKind::U32)));
        assert_eq!(unify(&Typed { constant: Some(1), ..int(IntKind::I64) }, &int(IntKind::U64)), Some(int(IntKind::U64)), "1L : ulong");
        assert_eq!(unify(&Typed::of(Kind::Object), &nullable(int(IntKind::I32))), Some(Typed::of(Kind::Object)));
    }

    #[test]
    fn null_and_nullable_types_unify_to_the_nullable_type() {
        let maybe = nullable(int(IntKind::I32));
        assert_eq!(unify(&maybe, &Typed::of(Kind::Null)), Some(maybe.clone()));
        assert_eq!(unify(&maybe, &constant(1)), Some(maybe.clone()));
        assert_eq!(unify(&maybe, &int(IntKind::U8)), Some(maybe.clone()));
        assert_eq!(unify(&maybe, &int(IntKind::I64)), None, "int? と long は、どちらへも変換できない");
        assert_eq!(unify(&maybe, &nullable(Typed::of(Kind::Float { single: false }))), Some(nullable(Typed::of(Kind::Float { single: false }))));
        assert_eq!(unify(&int(IntKind::I32), &Typed::of(Kind::Null)), None, "int と null は、型が決まらない");
        assert_eq!(unify(&Typed::of(Kind::Null), &Typed::of(Kind::Null)), None);
        assert_eq!(unify(&Typed::of(Kind::Str), &Typed::of(Kind::Null)), Some(Typed::of(Kind::Str)));
        assert_eq!(unify(&Typed::of(Kind::Str), &Typed::of(Kind::Object)), Some(Typed::of(Kind::Object)));
        assert_eq!(unify(&Typed::of(Kind::Object), &int(IntKind::I32)), Some(Typed::of(Kind::Object)));
    }

    #[test]
    fn coalesce_prefers_the_type_inside_the_nullable() {
        let maybe = nullable(int(IntKind::I32));
        let double = Typed::of(Kind::Float { single: false });
        assert_eq!(coalesce(&maybe, &int(IntKind::U8)), Some(int(IntKind::I32)));
        assert_eq!(coalesce(&maybe, &constant(0)), Some(int(IntKind::I32)));
        assert_eq!(coalesce(&maybe, &maybe), Some(maybe.clone()));
        assert_eq!(coalesce(&maybe, &Typed::of(Kind::Null)), Some(maybe.clone()));
        assert_eq!(coalesce(&maybe, &int(IntKind::I64)), Some(int(IntKind::I64)));
        assert_eq!(coalesce(&maybe, &nullable(double.clone())), Some(nullable(double.clone())));
        assert_eq!(coalesce(&nullable(double.clone()), &int(IntKind::I32)), Some(double));
        assert_eq!(coalesce(&maybe, &Typed::of(Kind::Str)), None);
        assert_eq!(coalesce(&maybe, &Typed::of(Kind::Object)), Some(Typed::of(Kind::Object)));
        assert_eq!(coalesce(&Typed::of(Kind::Str), &Typed::of(Kind::Object)), Some(Typed::of(Kind::Object)));
        assert_eq!(coalesce(&Typed::of(Kind::Object), &Typed::of(Kind::Str)), Some(Typed::of(Kind::Object)));
        assert_eq!(coalesce(&Typed::of(Kind::Str), &constant(1)), None);
    }

    #[test]
    fn as_needs_a_reference_or_nullable_target_and_a_possible_conversion() {
        let sbyte = int(IntKind::I8);
        assert!(as_is_allowed(&sbyte, &int(IntKind::I8), true, true).is_err(), "sb as sbyte");
        assert!(as_is_allowed(&sbyte, &nullable(int(IntKind::I32)), true, false).is_ok(), "sb as int?（結果は null）");
        assert!(as_is_allowed(&sbyte, &Typed::of(Kind::Object), false, true).is_ok());
        assert!(as_is_allowed(&sbyte, &Typed::of(Kind::Str), false, false).is_err(), "sb as string");
        assert!(as_is_allowed(&Typed::of(Kind::Bool), &nullable(Typed::of(Kind::Float { single: false })), true, false).is_err(), "true as double?");
        assert!(as_is_allowed(&Typed::of(Kind::Object), &Typed::of(Kind::Str), false, false).is_ok(), "参照型からは、実行時に決まる");
    }
}
