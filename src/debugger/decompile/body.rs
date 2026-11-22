//! メソッドの本体（ECMA-335 II.25.4）を、PE の像から読む: ヘッダー、IL、例外の節。
//!
//! ICorDebug の `GetILCode` は IL だけで、例外の節もローカル変数のシグネチャも返さない。逆コンパイルには両方要るので、
//! MethodDef の RVA からファイルの像を読む。

use crate::debugger::metadata::Reader;
use std::ops::Range;

/// 例外の節の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseKind {
    /// `catch (型)`。型は `class_token`。
    Catch,
    /// `catch when (…)`。条件の IL は `filter_start` から、ハンドラーの頭まで。
    Filter,
    Finally,
    Fault,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub kind: ClauseKind,
    pub try_range: Range<u32>,
    pub handler: Range<u32>,
    /// `Catch` の型のトークン（ほかは 0）。
    pub class_token: u32,
    /// `Filter` の条件の頭（ほかは 0）。
    pub filter_start: u32,
}

#[derive(Debug, Clone)]
pub struct MethodBody<'a> {
    pub code: &'a [u8],
    /// ローカル変数のシグネチャ（StandAloneSig のトークン。無ければ 0）。
    pub local_signature: u32,
    pub clauses: Vec<Clause>,
}

const TINY: u8 = 0x2;
const FAT: u8 = 0x3;
const MORE_SECTIONS: u16 = 0x8;
const EH_TABLE: u8 = 0x1;
const FAT_SECTION: u8 = 0x40;
const MORE_SECTIONS_AFTER: u8 = 0x80;

/// RVA `rva` にあるメソッドの本体を、PE の像（ファイルの並び）から読む。
pub fn read(image: &[u8], rva: u32) -> Result<MethodBody<'_>, String> {
    let start = crate::debugger::target::file_offset(image, rva).ok_or_else(|| tr!("The method body's position is outside the file"))?;
    parse(image.get(start..).ok_or_else(|| tr!("The method body's position is outside the file"))?)
}

/// 頭から本体が並ぶバイト列（対象のメモリーから読んだものなど）を、メソッドの本体として読む。
pub fn parse(rest: &[u8]) -> Result<MethodBody<'_>, String> {
    let first = *rest.first().ok_or_else(|| tr!("The method body is empty"))?;
    match first & 0x3 {
        TINY => {
            let size = (first >> 2) as usize;
            let code = rest.get(1..1 + size).ok_or_else(|| tr!("The method's IL is cut short"))?;
            Ok(MethodBody { code, local_signature: 0, clauses: Vec::new() })
        }
        FAT => {
            let mut r = Reader::new(rest);
            let flags = r.u16()? as u16;
            let header_size = ((flags >> 12) & 0xF) as usize * 4;
            r.u16()?; // MaxStack
            let code_size = r.u32()? as usize;
            let local_signature = r.u32()?;
            let code_end = header_size.checked_add(code_size).ok_or_else(|| tr!("The size of the method's IL is broken"))?;
            let code = rest.get(header_size..code_end).ok_or_else(|| tr!("The method's IL is cut short"))?;
            let clauses = if flags & MORE_SECTIONS != 0 { sections(rest, code_end)? } else { Vec::new() };
            Ok(MethodBody { code, local_signature, clauses })
        }
        _ => Err(tr!("Cannot read the header of the method body")),
    }
}

/// IL のあとの節（4 バイト境界から）。例外の節の表だけを読む。
fn sections(rest: &[u8], code_end: usize) -> Result<Vec<Clause>, String> {
    let mut clauses = Vec::new();
    let mut at = (code_end + 3) & !3;
    loop {
        let mut r = Reader::new(rest.get(at..).ok_or_else(|| tr!("The exception clauses are cut short"))?);
        let kind = r.u8()?;
        let fat = kind & FAT_SECTION != 0;
        let size = if fat { r.u8()? as usize | (r.u16()? as usize) << 8 } else { r.u8()? as usize };
        if !fat {
            r.u16()?;
        }
        if kind & EH_TABLE != 0 {
            let (clause_size, header) = if fat { (24, 4) } else { (12, 4) };
            let count = size.saturating_sub(header) / clause_size;
            for _ in 0..count {
                clauses.push(clause(&mut r, fat)?);
            }
        }
        if kind & MORE_SECTIONS_AFTER == 0 || size == 0 {
            break;
        }
        at = (at + size + 3) & !3;
    }
    Ok(clauses)
}

fn clause(r: &mut Reader, fat: bool) -> Result<Clause, String> {
    let (flags, try_offset, try_length, handler_offset, handler_length) =
        if fat { (r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?) } else { (r.u16()?, r.u16()?, r.u8()? as u32, r.u16()?, r.u8()? as u32) };
    let extra = r.u32()?;
    let kind = match flags {
        0 => ClauseKind::Catch,
        1 => ClauseKind::Filter,
        2 => ClauseKind::Finally,
        4 => ClauseKind::Fault,
        other => return Err(tr!("Unknown exception clause kind {kind}", kind = other)),
    };
    Ok(Clause {
        kind,
        try_range: try_offset..try_offset.saturating_add(try_length),
        handler: handler_offset..handler_offset.saturating_add(handler_length),
        class_token: if kind == ClauseKind::Catch { extra } else { 0 },
        filter_start: if kind == ClauseKind::Filter { extra } else { 0 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_exception_sections_are_read() {
        // 小さい節: kind 0x01、size 16、予約 2 バイト、節 1 つ（catch、try 0..4、handler 4..6、型 0x01000005）
        let mut data = vec![0x01, 16, 0, 0];
        data.extend([0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x00, 0x02, 0x05, 0x00, 0x00, 0x01]);
        let clauses = sections(&data, 0).unwrap();
        assert_eq!(clauses, [Clause { kind: ClauseKind::Catch, try_range: 0..4, handler: 4..6, class_token: 0x0100_0005, filter_start: 0 }]);
    }

    #[test]
    fn fat_exception_sections_are_read() {
        // 大きい節: kind 0x41、size 28（3 バイト）、節 1 つ（finally、try 2..10、handler 10..14）
        let mut data = vec![0x41, 28, 0, 0];
        for value in [2u32, 2, 8, 10, 4, 0] {
            data.extend(value.to_le_bytes());
        }
        let clauses = sections(&data, 0).unwrap();
        assert_eq!(clauses[0].kind, ClauseKind::Finally);
        assert_eq!(clauses[0].try_range, 2..10);
        assert_eq!(clauses[0].handler, 10..14);
    }
}
