//! PE の像に置かれたデータ（FieldRVA）を、値の並びにする。配列の初期化子（`new int[] { 1, 2, 3 }`）を戻すのに使う。

use super::ast::Expr;
use super::decl::{char_literal, float_literal};
use super::names::{Generics, Names};

/// 要素の型 `element`（型のトークン）の配列 `count` 個分の値を、フィールド `field` のデータから読む。
/// 組み込みの数値の型、bool、char だけ。読めなければ `None`。
pub fn array_values(names: &Names, image: &[u8], element: u32, field: u32, count: usize) -> Option<Vec<Expr>> {
    let field_row = field & 0x00FF_FFFF;
    if field >> 24 != 0x04 {
        return None;
    }
    let start = crate::debugger::target::file_offset(image, *names.lookups().field_rva.get(&field_row)?)?;
    // 列挙型の要素は、元の整数の型で読んで `(Color)1` と書く。
    let (element_name, cast) = match names.enum_of(element) {
        Some((underlying, name)) => (names.ty(&underlying, &Generics::default()), Some(name)),
        None => (names.type_token(element, &[], &Generics::default()), None),
    };
    let size = match element_name.as_str() {
        "bool" | "byte" | "sbyte" => 1,
        "short" | "ushort" | "char" => 2,
        "int" | "uint" | "float" => 4,
        "long" | "ulong" | "double" => 8,
        _ => return None,
    };
    let bytes = image.get(start..start.checked_add(size * count)?)?;
    let values = bytes
        .chunks_exact(size)
        .map(|chunk| {
            let mut buffer = [0u8; 8];
            buffer[..size].copy_from_slice(chunk);
            let raw = u64::from_le_bytes(buffer);
            let text = match element_name.as_str() {
                "bool" => (raw != 0).to_string(),
                "byte" => (raw as u8).to_string(),
                "sbyte" => (raw as u8 as i8).to_string(),
                "short" => (raw as u16 as i16).to_string(),
                "ushort" => (raw as u16).to_string(),
                "char" => char_literal(raw as u16),
                "int" => (raw as u32 as i32).to_string(),
                "uint" => format!("{}u", raw as u32),
                "float" => float_literal(f32::from_bits(raw as u32) as f64, true),
                "long" => format!("{}L", raw as i64),
                "ulong" => format!("{raw}UL"),
                _ => float_literal(f64::from_bits(raw), false),
            };
            Expr::Literal(match &cast {
                Some(name) => format!("({name}){text}"),
                None => text,
            })
        })
        .collect();
    Some(values)
}