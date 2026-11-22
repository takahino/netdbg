//! ECMA-335 のメタデータのリーダ（docs/DECISIONS.md D-017）。
//!
//! アセンブリのメタデータと Portable PDB は同じ器（BSJB、`#~`、`#Strings`、`#Blob`、`#GUID`）なので、
//! 表の読み方はここに一つだけ置き、`pdb.rs` もこれを使う。ランタイムの `IMetaDataImport` を使わないのは、
//! 手書きの COM バインディングを増やさず、対象のランタイムの版にも左右されないため。
//!
//! 表の定義は ECMA-335 II.22、PDB の表は
//! https://github.com/dotnet/runtime/blob/main/docs/design/specs/PortablePdb-Metadata.md 。

use std::collections::HashMap;
use std::ops::Range;

pub const TYPE_REF: usize = 0x01;
pub const TYPE_DEF: usize = 0x02;
pub const FIELD: usize = 0x04;
pub const METHOD_DEF: usize = 0x06;
pub const PARAM: usize = 0x08;
pub const INTERFACE_IMPL: usize = 0x09;
pub const MEMBER_REF: usize = 0x0A;
pub const STAND_ALONE_SIG: usize = 0x11;
pub const TYPE_SPEC: usize = 0x1B;
pub const METHOD_SPEC: usize = 0x2B;
pub const NESTED_CLASS: usize = 0x29;
pub const GENERIC_PARAM: usize = 0x2A;
pub const PROPERTY: usize = 0x17;
pub const EVENT: usize = 0x14;
pub const ENC_LOG: usize = 0x1E;
pub const ENC_MAP: usize = 0x1F;
pub const DOCUMENT: usize = 0x30;
pub const METHOD_DEBUG_INFORMATION: usize = 0x31;
pub const LOCAL_SCOPE: usize = 0x32;
pub const LOCAL_VARIABLE: usize = 0x33;
pub const CUSTOM_DEBUG_INFORMATION: usize = 0x37;

const TABLE_COUNT: usize = 64;
/// 複合添字の「使われていない枠」。行を持つことのない番号を当てる。
const NONE: usize = 63;

#[derive(Clone, Copy)]
enum Column {
    U16,
    U32,
    Str,
    Guid,
    Blob,
    /// ほかの表の行番号。
    Table(usize),
    /// 複合添字（下位のビットが表の種類、残りが行番号）。
    Coded(&'static [usize]),
}
use Column::*;

const TYPE_DEF_OR_REF: Column = Coded(&[0x02, 0x01, 0x1B]);
const HAS_CONSTANT: Column = Coded(&[0x04, 0x08, 0x17]);
const HAS_CUSTOM_ATTRIBUTE: Column =
    Coded(&[0x06, 0x04, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x00, 0x0E, 0x17, 0x14, 0x11, 0x1A, 0x1B, 0x20, 0x23, 0x26, 0x27, 0x28, 0x2A, 0x2C, 0x2B]);
const HAS_FIELD_MARSHAL: Column = Coded(&[0x04, 0x08]);
const HAS_DECL_SECURITY: Column = Coded(&[0x02, 0x06, 0x20]);
const MEMBER_REF_PARENT: Column = Coded(&[0x02, 0x01, 0x1A, 0x06, 0x1B]);
const HAS_SEMANTICS: Column = Coded(&[0x14, 0x17]);
const METHOD_DEF_OR_REF: Column = Coded(&[0x06, 0x0A]);
const MEMBER_FORWARDED: Column = Coded(&[0x04, 0x06]);
const IMPLEMENTATION: Column = Coded(&[0x26, 0x23, 0x27]);
const CUSTOM_ATTRIBUTE_TYPE: Column = Coded(&[NONE, NONE, 0x06, 0x0A, NONE]);
const RESOLUTION_SCOPE: Column = Coded(&[0x00, 0x1A, 0x23, 0x01]);
const TYPE_OR_METHOD_DEF: Column = Coded(&[0x02, 0x06]);
const HAS_CUSTOM_DEBUG_INFORMATION: Column = Coded(&[
    0x06, 0x04, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x00, 0x0E, 0x17, 0x14, 0x11, 0x1A, 0x1B, 0x20, 0x23, 0x26, 0x27, 0x28, 0x2A, 0x2C, 0x2B, 0x30, 0x32, 0x33, 0x34,
    0x35,
]);

/// 表ごとの列の並び。定義の無い番号は空（その表に行があれば読めないのでエラーにする）。
fn schema(table: usize) -> &'static [Column] {
    match table {
        0x00 => &[U16, Str, Guid, Guid, Guid],                               // Module
        0x01 => &[RESOLUTION_SCOPE, Str, Str],                               // TypeRef
        0x02 => &[U32, Str, Str, TYPE_DEF_OR_REF, Table(0x04), Table(0x06)], // TypeDef
        0x03 => &[Table(0x04)],                                              // FieldPtr
        0x04 => &[U16, Str, Blob],                                           // Field
        0x05 => &[Table(0x06)],                                              // MethodPtr
        0x06 => &[U32, U16, U16, Str, Blob, Table(0x08)],                    // MethodDef
        0x07 => &[Table(0x08)],                                              // ParamPtr
        0x08 => &[U16, U16, Str],                                            // Param
        0x09 => &[Table(0x02), TYPE_DEF_OR_REF],                             // InterfaceImpl
        0x0A => &[MEMBER_REF_PARENT, Str, Blob],                             // MemberRef
        0x0B => &[U16, HAS_CONSTANT, Blob],                                  // Constant
        0x0C => &[HAS_CUSTOM_ATTRIBUTE, CUSTOM_ATTRIBUTE_TYPE, Blob],        // CustomAttribute
        0x0D => &[HAS_FIELD_MARSHAL, Blob],                                  // FieldMarshal
        0x0E => &[U16, HAS_DECL_SECURITY, Blob],                             // DeclSecurity
        0x0F => &[U16, U32, Table(0x02)],                                    // ClassLayout
        0x10 => &[U32, Table(0x04)],                                         // FieldLayout
        0x11 => &[Blob],                                                     // StandAloneSig
        0x12 => &[Table(0x02), Table(0x14)],                                 // EventMap
        0x13 => &[Table(0x14)],                                              // EventPtr
        0x14 => &[U16, Str, TYPE_DEF_OR_REF],                                // Event
        0x15 => &[Table(0x02), Table(0x17)],                                 // PropertyMap
        0x16 => &[Table(0x17)],                                              // PropertyPtr
        0x17 => &[U16, Str, Blob],                                           // Property
        0x18 => &[U16, Table(0x06), HAS_SEMANTICS],                          // MethodSemantics
        0x19 => &[Table(0x02), METHOD_DEF_OR_REF, METHOD_DEF_OR_REF],        // MethodImpl
        0x1A => &[Str],                                                      // ModuleRef
        0x1B => &[Blob],                                                     // TypeSpec
        0x1C => &[U16, MEMBER_FORWARDED, Str, Table(0x1A)],                  // ImplMap
        0x1D => &[U32, Table(0x04)],                                         // FieldRVA
        0x1E => &[U32, U32],                                                 // EncLog
        0x1F => &[U32],                                                      // EncMap
        0x20 => &[U32, U16, U16, U16, U16, U32, Blob, Str, Str],             // Assembly
        0x21 => &[U32],                                                      // AssemblyProcessor
        0x22 => &[U32, U32, U32],                                            // AssemblyOS
        0x23 => &[U16, U16, U16, U16, U32, Blob, Str, Str, Blob],            // AssemblyRef
        0x24 => &[U32, Table(0x23)],                                         // AssemblyRefProcessor
        0x25 => &[U32, U32, U32, Table(0x23)],                               // AssemblyRefOS
        0x26 => &[U32, Str, Blob],                                           // File
        0x27 => &[U32, U32, Str, Str, IMPLEMENTATION],                       // ExportedType
        0x28 => &[U32, U32, Str, IMPLEMENTATION],                            // ManifestResource
        0x29 => &[Table(0x02), Table(0x02)],                                 // NestedClass
        0x2A => &[U16, U16, TYPE_OR_METHOD_DEF, Str],                        // GenericParam
        0x2B => &[METHOD_DEF_OR_REF, Blob],                                  // MethodSpec
        0x2C => &[Table(0x2A), TYPE_DEF_OR_REF],                             // GenericParamConstraint
        0x30 => &[Blob, Guid, Blob, Guid],                                   // Document
        0x31 => &[Table(0x30), Blob],                                        // MethodDebugInformation
        0x32 => &[Table(0x06), Table(0x35), Table(0x33), Table(0x34), U32, U32], // LocalScope
        0x33 => &[U16, U16, Str],                                            // LocalVariable
        0x34 => &[Str, Blob],                                                // LocalConstant
        0x35 => &[Table(0x35), Blob],                                        // ImportScope
        0x36 => &[Table(0x06), Table(0x06)],                                 // StateMachineMethod
        0x37 => &[HAS_CUSTOM_DEBUG_INFORMATION, Guid, Blob],                 // CustomDebugInformation
        _ => &[],
    }
}

/// 列の幅（バイト）。添字の幅は、指す先の行数とヒープの大きさで 2 か 4 に変わる。
/// Edit and Continue の差分（`#JTD` のある「最小の差分」）では、表の添字はいつも 4 バイト。
fn column_width(column: Column, rows: &[u32; TABLE_COUNT], heap_sizes: u8, minimal_delta: bool) -> usize {
    let wide = |is_wide: bool| if is_wide { 4 } else { 2 };
    match column {
        U16 => 2,
        U32 => 4,
        Str => wide(heap_sizes & 1 != 0),
        Guid => wide(heap_sizes & 2 != 0),
        Blob => wide(heap_sizes & 4 != 0),
        Table(_) | Coded(_) if minimal_delta => 4,
        Table(table) => wide(rows[table] > 0xFFFF),
        Coded(tables) => {
            let tag_bits = usize::BITS - (tables.len() - 1).leading_zeros();
            let largest = tables.iter().map(|&t| rows[t]).max().unwrap_or(0);
            wide(largest >= 1 << (16 - tag_bits))
        }
    }
}

fn truncated() -> &'static str {
    tr_str!("The metadata ends early")
}

/// バイト列を前から読む。範囲を越えたらエラー（パニックしない）。
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or_else(truncated)?;
        self.pos += 1;
        Ok(b)
    }
    pub fn u16(&mut self) -> Result<u32, String> {
        Ok(self.u8()? as u32 | (self.u8()? as u32) << 8)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(self.u16()? | self.u16()? << 16)
    }
    pub fn skip(&mut self, count: usize) -> Result<(), String> {
        self.pos = self.pos.checked_add(count).filter(|&p| p <= self.data.len()).ok_or_else(truncated)?;
        Ok(())
    }
    /// ECMA-335 II.23.2 の圧縮された符号なし整数。符号化の幅（バイト数）も返す。
    fn compressed(&mut self) -> Result<(u32, u32), String> {
        let b = self.u8()? as u32;
        if b & 0x80 == 0 {
            Ok((b, 1))
        } else if b & 0xC0 == 0x80 {
            Ok(((b & 0x3F) << 8 | self.u8()? as u32, 2))
        } else {
            let rest = (self.u8()? as u32) << 16 | (self.u8()? as u32) << 8 | self.u8()? as u32;
            Ok(((b & 0x1F) << 24 | rest, 4))
        }
    }
    pub fn unsigned(&mut self) -> Result<u32, String> {
        Ok(self.compressed()?.0)
    }
    /// 圧縮された符号つき整数。符号ビットが最下位に回されている。
    pub fn signed(&mut self) -> Result<i32, String> {
        let (raw, width) = self.compressed()?;
        let value = (raw >> 1) as i32;
        if raw & 1 == 0 {
            return Ok(value);
        }
        Ok(value - match width {
            1 => 0x40,
            2 => 0x2000,
            _ => 0x1000_0000,
        })
    }
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }
}

#[derive(Default)]
struct TableLayout {
    rows: u32,
    /// `data` の中での先頭。
    start: usize,
    row_size: usize,
    /// 列ごとの「行の中での位置、幅」。
    columns: Vec<(usize, usize)>,
}

pub struct Metadata {
    data: Vec<u8>,
    strings: Range<usize>,
    blobs: Range<usize>,
    guids: Range<usize>,
    /// `#Pdb` ストリーム（単体の PDB だけが持つ）。
    pdb: Range<usize>,
    /// `#US`（`ldstr` の文字列。UTF-16）。
    user_strings: Range<usize>,
    tables: Vec<TableLayout>,
    /// Edit and Continue で当てた差分（当てた順）。空なら、ファイルのメタデータそのまま。
    generations: Vec<Generation>,
    /// 差分を重ねたあとの、表ごとの行数。
    row_counts: [u32; TABLE_COUNT],
    /// 差分で変わった行と足された行: （表、行）→（世代の添字、差分の中の行）。新しい世代が勝つ。
    overrides: HashMap<(usize, u32), (usize, u32)>,
    /// 差分で足された子（EncLog の AddMethod など）: （親の表、親の行、子の表）→ 子の行。
    added_children: HashMap<(usize, u32, usize), Vec<u32>>,
    /// 差分で足された子の親: （子の表、子の行）→ 親の行。
    added_parents: HashMap<(usize, u32), u32>,
}

/// 当てた差分 1 つ。差分の #Strings と #Blob の添字は、前の世代までのヒープに続く通しの位置
/// （System.Reflection.Metadata の MetadataAggregator と同じ）。#GUID の添字も通し（`guid`）。
struct Generation {
    delta: Metadata,
    string_start: usize,
    blob_start: usize,
    /// この世代までの、表ごとの行数（この世代で足された行の範囲を知るため）。
    row_counts: [u32; TABLE_COUNT],
}

impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Metadata({} bytes)", self.data.len())
    }
}

impl Metadata {
    /// アセンブリ（PE ファイル）の中のメタデータを読む。
    pub fn from_pe(file: &[u8]) -> Result<Metadata, String> {
        let range = crate::debugger::target::metadata_range(file).ok_or_else(|| tr!("Not a managed assembly (there is no CLR header)"))?;
        Metadata::parse(file[range].to_vec())
    }

    /// `BSJB` で始まるメタデータを読む。
    pub fn parse(data: Vec<u8>) -> Result<Metadata, String> {
        let mut r = Reader::new(&data);
        if r.u32()? != 0x424A_5342 {
            return Err(tr!("Not metadata (there is no BSJB signature)"));
        }
        r.skip(8)?;
        let version_len = r.u32()? as usize;
        r.skip(version_len)?;
        r.skip(2)?;
        let stream_count = r.u16()?;
        let (mut tables, mut strings, mut blobs, mut guids, mut pdb, mut user_strings) = (0..0, 0..0, 0..0, 0..0, 0..0, 0..0);
        let mut minimal_delta = false;
        for _ in 0..stream_count {
            let (offset, size) = (r.u32()? as usize, r.u32()? as usize);
            let start = r.pos;
            while r.u8()? != 0 {}
            let name = &data[start..r.pos - 1];
            r.pos = (r.pos + 3) & !3;
            let range = offset..offset.saturating_add(size);
            if range.end > data.len() {
                return Err(tr!("A metadata stream is outside the file"));
            }
            match name {
                b"#~" => tables = range,
                b"#Strings" => strings = range,
                b"#Blob" => blobs = range,
                b"#GUID" => guids = range,
                b"#Pdb" => pdb = range,
                b"#US" => user_strings = range,
                // 最適化されていない表。Edit and Continue の差分がこの形（普通のビルドでは出ない）。
                b"#-" => tables = range,
                // 「最小の差分」の印（中身は空）。表の添字がいつも 4 バイトになる。
                b"#JTD" => minimal_delta = true,
                _ => {}
            }
        }

        let mut t = Reader { data: &data[..tables.end], pos: tables.start };
        t.skip(6)?;
        let heap_sizes = t.u8()?;
        t.skip(1)?;
        let valid = t.u32()? as u64 | (t.u32()? as u64) << 32;
        t.skip(8)?;
        let mut rows = [0u32; TABLE_COUNT];
        for (bit, row) in rows.iter_mut().enumerate() {
            if valid & (1 << bit) != 0 {
                *row = t.u32()?;
            }
        }
        // ヒープの大きさの 0x40: 行数のあとに 4 バイトの追加のデータがある。
        if heap_sizes & 0x40 != 0 {
            t.skip(4)?;
        }

        // 単体の PDB は型システムの表を持たない。それらを指す添字の幅は、#Pdb に写してある行数で決まる。
        let mut sizing = rows;
        if !pdb.is_empty() {
            let mut p = Reader { data: &data[..pdb.end], pos: pdb.start };
            p.skip(20 + 4)?;
            let referenced = p.u32()? as u64 | (p.u32()? as u64) << 32;
            for (bit, row) in sizing.iter_mut().enumerate() {
                if referenced & (1 << bit) != 0 {
                    *row = p.u32()?;
                }
            }
        }

        let mut layouts = Vec::with_capacity(TABLE_COUNT);
        let mut start = t.pos;
        for (table, &count) in rows.iter().enumerate() {
            let columns = schema(table);
            if count > 0 && columns.is_empty() {
                return Err(tr!("The metadata has an unknown table {table}", table = format!("0x{table:02X}")));
            }
            let mut layout = TableLayout { rows: count, start, row_size: 0, columns: Vec::with_capacity(columns.len()) };
            for &column in columns {
                let width = column_width(column, &sizing, heap_sizes, minimal_delta);
                layout.columns.push((layout.row_size, width));
                layout.row_size += width;
            }
            start = (count as usize).checked_mul(layout.row_size).and_then(|size| start.checked_add(size)).filter(|&end| end <= tables.end).ok_or_else(truncated)?;
            layouts.push(layout);
        }
        let mut row_counts = [0u32; TABLE_COUNT];
        for (count, layout) in row_counts.iter_mut().zip(&layouts) {
            *count = layout.rows;
        }
        Ok(Metadata {
            data,
            strings,
            blobs,
            guids,
            pdb,
            user_strings,
            tables: layouts,
            generations: Vec::new(),
            row_counts,
            overrides: HashMap::new(),
            added_children: HashMap::new(),
            added_parents: HashMap::new(),
        })
    }

    /// Edit and Continue の差分（`EmitDifference` が書いたメタデータか PDB の差分）を重ねる。差分の EncMap に載った
    /// トークン（変わった行と足された行）を返す。
    pub fn apply_delta(&mut self, bytes: Vec<u8>) -> Result<Vec<u32>, String> {
        let delta = Metadata::parse(bytes)?;
        if delta.rows(ENC_MAP) == 0 {
            return Err(tr!("Not an Edit and Continue delta (there is no EncMap)"));
        }
        let generation = self.generations.len();
        let (mut string_start, mut blob_start) = (self.string_heap_len(), self.blobs.len());
        if let Some(last) = self.generations.last() {
            string_start = last.string_start + last.delta.string_heap_len();
            blob_start = last.blob_start + last.delta.blobs.len();
        }
        // EncMap はトークンの昇順。表ごとに、差分の表の行は EncMap に載った順に並んでいる。
        let mut tokens = Vec::with_capacity(delta.rows(ENC_MAP) as usize);
        let mut delta_rows = [0u32; TABLE_COUNT];
        let mut overrides = Vec::new();
        let mut row_counts = self.row_counts;
        for row in 1..=delta.rows(ENC_MAP) {
            let token = delta.raw_cell(ENC_MAP, row, 0);
            let (table, rid) = ((token >> 24) as usize, token & 0x00FF_FFFF);
            if table >= TABLE_COUNT || rid == 0 {
                return Err(tr!("The delta's EncMap has an unreadable token {token}", token = format!("0x{token:08X}")));
            }
            delta_rows[table] += 1;
            if delta_rows[table] > delta.tables[table].rows {
                return Err(tr!("The row counts of the delta's EncMap and table {table} do not match", table = format!("0x{table:02X}")));
            }
            overrides.push(((table, rid), (generation, delta_rows[table])));
            row_counts[table] = row_counts[table].max(rid);
            tokens.push(token);
        }
        // ここから先は失敗しない（途中で失敗して、半分だけ重なった状態を残さない）。
        self.overrides.extend(overrides);
        self.row_counts = row_counts;
        // EncLog: 「親のトークン、AddMethod など」の次の項目が、足された子。
        let mut pending: Option<(usize, u32, usize)> = None;
        for row in 1..=delta.rows(ENC_LOG) {
            let (token, func) = (delta.raw_cell(ENC_LOG, row, 0), delta.raw_cell(ENC_LOG, row, 1));
            let (table, rid) = ((token >> 24) as usize, token & 0x00FF_FFFF);
            let child_table = match func {
                1 => Some(METHOD_DEF),
                2 => Some(FIELD),
                3 => Some(PARAM),
                4 => Some(PROPERTY),
                5 => Some(EVENT),
                _ => None,
            };
            if let Some(child_table) = child_table {
                pending = Some((table, rid, child_table));
                continue;
            }
            if let Some((parent_table, parent, child_table)) = pending.take().filter(|&(_, _, child)| child == table) {
                let children = self.added_children.entry((parent_table, parent, child_table)).or_default();
                if !children.contains(&rid) {
                    children.push(rid);
                }
                self.added_parents.insert((child_table, rid), parent);
            }
        }
        self.generations.push(Generation { delta, string_start, blob_start, row_counts });
        Ok(tokens)
    }

    pub fn rows(&self, table: usize) -> u32 {
        self.row_counts[table]
    }

    /// ファイル（世代 0）の行数。
    fn base_rows(&self, table: usize) -> u32 {
        self.tables[table].rows
    }

    /// 表の 1 マス。`row` は 1 始まり。範囲外は 0（「無し」を表す添字と同じ）。差分で変わった行は、最新の世代の値。
    pub fn cell(&self, table: usize, row: u32, column: usize) -> u32 {
        if !self.overrides.is_empty() {
            if let Some(&(generation, delta_row)) = self.overrides.get(&(table, row)) {
                return self.generations[generation].delta.raw_cell(table, delta_row, column);
            }
        }
        self.raw_cell(table, row, column)
    }

    /// この世代（ファイルか差分そのもの）の表の 1 マス。
    fn raw_cell(&self, table: usize, row: u32, column: usize) -> u32 {
        let layout = &self.tables[table];
        let Some(&(offset, width)) = layout.columns.get(column) else { return 0 };
        if row == 0 || row > layout.rows {
            return 0;
        }
        // 表がデータに収まることは parse で確かめてある。
        let at = layout.start + (row as usize - 1) * layout.row_size + offset;
        let bytes = &self.data[at..at + width];
        if width == 2 {
            u16::from_le_bytes([bytes[0], bytes[1]]) as u32
        } else {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    }

    /// `#Strings` の長さ。末尾の詰め物（4 バイトにそろえる 0）は数えない。差分の添字は、この長さに続けて数える
    /// （System.Reflection.Metadata の StringHeap と同じ。終わりの 0 は 1 つだけ残す）。
    fn string_heap_len(&self) -> usize {
        let heap = &self.data[self.strings.clone()];
        heap.iter().rposition(|&b| b != 0).map_or(heap.len().min(1), |last| (last + 2).min(heap.len()))
    }

    /// `#Strings` の文字列。添字が壊れていれば空。差分の添字は、世代をまたいだ通しの位置。
    pub fn string(&self, index: u32) -> &str {
        let at = index as usize;
        if at >= self.string_heap_len() {
            if let Some(g) = self.generations.iter().find(|g| (g.string_start..g.string_start + g.delta.string_heap_len()).contains(&at)) {
                return g.delta.local_string((at - g.string_start) as u32);
            }
        }
        self.local_string(index)
    }

    fn local_string(&self, index: u32) -> &str {
        let heap = &self.data[self.strings.clone()];
        let Some(rest) = heap.get(index as usize..) else { return "" };
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        std::str::from_utf8(&rest[..end]).unwrap_or("")
    }

    /// `ldstr` の文字列（#US。トークンは 0x70 と添字）。中身は「圧縮した長さ、UTF-16LE、終わりの 1 バイト（印）」。
    /// 読めなければ `None`。Edit and Continue の差分の文字列は読まない（逆コンパイルはファイルの分だけ）。
    pub fn user_string(&self, token: u32) -> Option<String> {
        let heap = &self.data[self.user_strings.clone()];
        let mut r = Reader { data: heap, pos: (token & 0x00FF_FFFF) as usize };
        let len = r.unsigned().ok()? as usize;
        let bytes = heap.get(r.pos..r.pos.checked_add(len)?)?;
        // 長さは終わりの印の 1 バイトを含む（空の文字列は長さ 0 か 1）。
        let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// `#Blob` の中身。差分の添字は、世代をまたいだ通しの位置。
    pub fn blob(&self, index: u32) -> Result<&[u8], String> {
        let at = index as usize;
        if at >= self.blobs.len() {
            if let Some(g) = self.generations.iter().find(|g| (g.blob_start..g.blob_start + g.delta.blobs.len()).contains(&at)) {
                return g.delta.local_blob((at - g.blob_start) as u32);
            }
        }
        self.local_blob(index)
    }

    fn local_blob(&self, index: u32) -> Result<&[u8], String> {
        let heap = &self.data[self.blobs.clone()];
        let mut r = Reader { data: heap, pos: index as usize };
        let len = r.unsigned()? as usize;
        r.pos.checked_add(len).and_then(|end| heap.get(r.pos..end)).ok_or_else(|| tr!("It points outside the metadata's #Blob"))
    }

    /// このモジュールが参照しているアセンブリ `name` の、完全な名前（`System.Core, Version=4.0.0.0, Culture=neutral,
    /// PublicKeyToken=b77a5c561934e089`）。参照していない、または公開鍵そのものを持つ参照（トークンを計算しないと書けない）なら `None`。
    pub fn assembly_reference(&self, name: &str) -> Option<String> {
        const ASSEMBLY_REF: usize = 0x23;
        (1..=self.rows(ASSEMBLY_REF)).find_map(|row| {
            if !self.string(self.cell(ASSEMBLY_REF, row, 6)).eq_ignore_ascii_case(name) {
                return None;
            }
            // 列: 版の 4 つ、フラグ、公開鍵かトークン、名前、カルチャ、ハッシュ。フラグの 1 は「公開鍵そのもの」。
            if self.cell(ASSEMBLY_REF, row, 4) & 1 != 0 {
                return None;
            }
            let version: Vec<String> = (0..4).map(|column| self.cell(ASSEMBLY_REF, row, column).to_string()).collect();
            let culture = match self.string(self.cell(ASSEMBLY_REF, row, 7)) {
                "" => "neutral",
                culture => culture,
            };
            let token = self.blob(self.cell(ASSEMBLY_REF, row, 5)).ok()?;
            let token = if token.is_empty() { "null".to_string() } else { token.iter().map(|b| format!("{b:02x}")).collect() };
            Some(format!("{}, Version={}, Culture={culture}, PublicKeyToken={token}", self.string(self.cell(ASSEMBLY_REF, row, 6)), version.join(".")))
        })
    }

    /// モジュールの版の ID（MVID）。ビルドのたびに変わり、アセンブリのそのビルドを一意に指す。
    /// アセンブリの名前と版（`Assembly` の表。無ければ `None`。アセンブリを読めなかった理由の、読み込み済みの版。D-091）。
    pub fn assembly_identity(&self) -> Option<(String, String)> {
        const ASSEMBLY: usize = 0x20;
        if self.rows(ASSEMBLY) == 0 {
            return None;
        }
        let version = (1..=4).map(|column| self.cell(ASSEMBLY, 1, column).to_string()).collect::<Vec<_>>().join(".");
        Some((self.string(self.cell(ASSEMBLY, 1, 7)).to_string(), version))
    }

    pub fn mvid(&self) -> Option<[u8; 16]> {
        // Module 表（0x00）の 1 行目、3 列目が #GUID の添字（1 始まり）。
        self.guid(self.cell(0x00, 1, 2))
    }

    /// `#GUID` の GUID。添字は 1 始まり。差分の #GUID は、前の世代の分の場所を空けたうえで自分の分を続けて持つ
    /// （添字は世代をまたいだ通し）。添字がファイルの #GUID に収まればファイルの値、そうでなければ、その添字まで
    /// 持っている最初の世代の値（System.Reflection.Metadata の MetadataAggregator と同じ）。
    pub fn guid(&self, index: u32) -> Option<[u8; 16]> {
        let count = |m: &Metadata| (m.guids.len() / 16) as u32;
        if index <= count(self) {
            return self.local_guid(index);
        }
        self.generations.iter().find(|g| index <= count(&g.delta)).and_then(|g| g.delta.local_guid(index))
    }

    fn local_guid(&self, index: u32) -> Option<[u8; 16]> {
        let at = index.checked_sub(1)? as usize * 16;
        self.data[self.guids.clone()].get(at..at + 16)?.try_into().ok()
    }

    /// PDB の ID（GUID 16 バイトと日時印 4 バイト）。アセンブリ側の記録（CodeView）と突き合わせる。
    pub fn pdb_id(&self) -> Option<[u8; 20]> {
        self.data[self.pdb.clone()].get(..20)?.try_into().ok()
    }

    /// `column` の値が `value` の行（昇順に並んでいる表）。差分で足された行は並びの外にあるので、後ろから足す。
    pub(crate) fn matching_rows(&self, table: usize, column: usize, value: u32) -> Vec<u32> {
        let mut rows = Vec::new();
        let mut row = self.lower_bound(table, column, value);
        while row <= self.base_rows(table) && self.cell(table, row, column) == value {
            rows.push(row);
            row += 1;
        }
        rows.extend((self.base_rows(table) + 1..=self.rows(table)).filter(|&row| self.cell(table, row, column) == value));
        rows
    }

    /// `column` の値が昇順に並んでいる表（ファイルの分）で、値が `value` 以上になる最初の行（無ければ 行数 + 1）。
    fn lower_bound(&self, table: usize, column: usize, value: u32) -> u32 {
        let (mut low, mut high) = (1, self.base_rows(table) + 1);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.cell(table, middle, column) < value {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }

    /// メソッドを宣言している型（TypeDef の行番号）。
    pub fn declaring_type(&self, method_row: u32) -> Option<u32> {
        if let Some(&owner) = self.added_parents.get(&(METHOD_DEF, method_row)) {
            return Some(owner);
        }
        if method_row == 0 || method_row > self.base_rows(METHOD_DEF) {
            return None;
        }
        // TypeDef.MethodList は昇順。メソッドを持たない型は次の型と同じ値なので、「値が method_row 以下の最後の行」が持ち主。
        let after = self.lower_bound(TYPE_DEF, 5, method_row + 1);
        (after > 1).then(|| after - 1)
    }

    /// フィールドを宣言している型（TypeDef の行番号）と、そのフィールド。
    pub fn field_declaration(&self, field_row: u32) -> Option<(u32, Field<'_>)> {
        let owner = match self.added_parents.get(&(FIELD, field_row)) {
            Some(&owner) => owner,
            None => {
                if field_row == 0 || field_row > self.base_rows(FIELD) {
                    return None;
                }
                // TypeDef.FieldList も昇順（`declaring_type` と同じ）。
                let after = self.lower_bound(TYPE_DEF, 4, field_row + 1);
                (after > 1).then(|| after - 1)?
            }
        };
        let field = self.fields(owner).into_iter().find(|field| field.token & 0x00FF_FFFF == field_row)?;
        Some((owner, field))
    }

    /// 型の `Namespace.Name`（型引数の数の印 `` `N `` も残した、メタデータのままの名前）。入れ子の型は名前だけ。
    pub fn raw_type_name(&self, type_row: u32) -> String {
        let name = self.string(self.cell(TYPE_DEF, type_row, 1));
        match self.string(self.cell(TYPE_DEF, type_row, 2)) {
            "" => name.to_string(),
            namespace => format!("{namespace}.{name}"),
        }
    }

    /// 入れ子の型の、外側の型。
    pub fn enclosing_type(&self, type_row: u32) -> Option<u32> {
        let row = *self.matching_rows(NESTED_CLASS, 0, type_row).first()?;
        Some(self.cell(NESTED_CLASS, row, 1))
    }

    /// 型またはメソッドの型引数の名前（宣言の順）。`owner` は TypeOrMethodDef の複合添字。
    pub(crate) fn generic_params(&self, owner: u32) -> Vec<&str> {
        self.matching_rows(GENERIC_PARAM, 2, owner).into_iter().map(|row| self.string(self.cell(GENERIC_PARAM, row, 3))).collect()
    }

    /// 型の名前。`Namespace.Outer.Inner<T>` の形（入れ子は `.` でつなぐ。netcoredbg と同じ）。
    pub fn type_name(&self, type_row: u32) -> String {
        self.type_name_within(type_row, &[], 0)
    }

    /// 型引数を実際の型の名前に置き換えた型の名前。`arguments` は外側の型の分から順に並べる
    /// （ICorDebugType::EnumerateTypeParameters の順）。足りなければ宣言の名前（`T`）で補う。
    pub fn type_name_with(&self, type_row: u32, arguments: &[String]) -> String {
        self.type_name_within(type_row, arguments, 0)
    }

    fn type_name_within(&self, type_row: u32, arguments: &[String], depth: u32) -> String {
        let raw = self.string(self.cell(TYPE_DEF, type_row, 1));
        // 名前の末尾の `N は、この型が自分で足した型引数の数。入れ子の型は外側の型引数も持っているので、後ろの N 個が自分の分。
        let (base, own_count) = match raw.rsplit_once('`') {
            Some((base, count)) => (base, count.parse::<usize>().unwrap_or(0)),
            None => (raw, 0),
        };
        let mut params = self.generic_params(type_row << 1);
        for (param, argument) in params.iter_mut().zip(arguments) {
            *param = argument.as_str();
        }
        let own = &params[params.len().saturating_sub(own_count)..];

        // 壊れたメタデータで入れ子が輪になっていても止まるように、深さを限る。
        let mut name = match self.enclosing_type(type_row).filter(|_| depth < 32) {
            Some(outer) => self.type_name_within(outer, arguments, depth + 1) + ".",
            None => match self.string(self.cell(TYPE_DEF, type_row, 2)) {
                "" => String::new(),
                namespace => format!("{namespace}."),
            },
        };
        name.push_str(base);
        push_generic_params(&mut name, own);
        name
    }

    /// `list_column` が「子の表の最初の行」を指す列のとき、`row` の子の行。次の行の値（最後の行なら子の表の終わり）までが
    /// 自分の分。Edit and Continue の差分で足された子（EncLog の AddMethod など）は、並びの外にあるので後ろに足す。
    pub fn child_rows(&self, table: usize, row: u32, list_column: usize, child_table: usize) -> std::iter::Chain<Range<u32>, std::vec::IntoIter<u32>> {
        let added = self.added_children.get(&(table, row, child_table)).cloned().unwrap_or_default();
        let range = if row == 0 || row > self.rows(table) {
            0..0
        } else if row <= self.base_rows(table) {
            // ファイルの行: ファイルの表どうしの並びで決まる（差分で行の中身が変わっても、範囲は変わらない）。
            let end = self.base_rows(child_table) + 1;
            let start = self.raw_cell(table, row, list_column);
            let next = if row < self.base_rows(table) { self.raw_cell(table, row + 1, list_column) } else { end };
            if start == 0 { 0..0 } else { start..next.clamp(start, end) }
        } else if !added.is_empty() {
            // 差分で足された親（新しい型など）の子は、EncLog でだけ分かる。
            0..0
        } else {
            // 差分で足された親（PDB の LocalScope など）: 同じ世代の次の行まで。無ければ、その世代で足された子の終わりまで。
            let generation = self.overrides.get(&(table, row)).map(|&(g, _)| g);
            let end = generation.map_or(self.rows(child_table), |g| self.generations[g].row_counts[child_table]) + 1;
            let start = self.cell(table, row, list_column);
            let next = match self.overrides.get(&(table, row + 1)) {
                Some(&(g, _)) if Some(g) == generation => self.cell(table, row + 1, list_column),
                _ => end,
            };
            if start == 0 { 0..0 } else { start..next.clamp(start, end) }
        };
        range.chain(added)
    }

    /// メソッドがインスタンスメソッドか（引数 0 が `this` になる）。
    pub fn method_has_this(&self, method_row: u32) -> bool {
        const STATIC: u32 = 0x0010;
        self.cell(METHOD_DEF, method_row, 2) & STATIC == 0
    }


    /// 型が宣言している、名前・静的かどうか・引数の数が合うメソッドのトークン（最初の一つ）。
    pub fn find_method(&self, type_row: u32, name: &str, is_static: bool, parameters: u32) -> Option<u32> {
        self.child_rows(TYPE_DEF, type_row, 5, METHOD_DEF)
            .find(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == name && self.has_shape(row, is_static, parameters))
            .map(|row| (METHOD_DEF as u32) << 24 | row)
    }

    /// メソッドが、静的かどうかと引数の数の指定に合うか（ジェネリックメソッドは、型引数を決めないと呼べないので合わない扱い）。
    fn has_shape(&self, method_row: u32, is_static: bool, parameters: u32) -> bool {
        const HAS_THIS: u8 = 0x20;
        const GENERIC: u8 = 0x10;
        // シグネチャ: 呼び出し規約、（ジェネリックなら型引数の数）、引数の数、…
        let Ok(signature) = self.blob(self.cell(METHOD_DEF, method_row, 4)) else { return false };
        let mut r = Reader::new(signature);
        let Ok(convention) = r.u8() else { return false };
        (convention & HAS_THIS == 0) == is_static && convention & GENERIC == 0 && r.unsigned() == Ok(parameters)
    }

    /// 型が宣言している、名前・静的かどうか・引数の数が合うメソッドすべて（オーバーロード）。
    pub fn find_methods_shaped(&self, type_row: u32, name: &str, is_static: bool, parameters: u32) -> Vec<u32> {
        self.child_rows(TYPE_DEF, type_row, 5, METHOD_DEF)
            .filter(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == name && self.has_shape(row, is_static, parameters))
            .map(|row| (METHOD_DEF as u32) << 24 | row)
            .collect()
    }

    /// メソッドの引数の型の大まかな種類。オーバーロードを選ぶのに使う。読めないシグネチャ（関数ポインターなど）は `None`。
    pub fn parameter_kinds(&self, method: u32) -> Option<Vec<ParameterKind>> {
        const GENERIC: u8 = 0x10;
        let signature = self.blob(self.cell(METHOD_DEF, method & 0x00FF_FFFF, 4)).ok()?;
        let mut r = Reader::new(signature);
        if r.u8().ok()? & GENERIC != 0 {
            r.unsigned().ok()?;
        }
        let count = r.unsigned().ok()?;
        read_type(&mut r)?; // 戻り値
        (0..count).map(|_| read_type(&mut r)).collect()
    }

    /// メソッドの引数の型を、値の型と比べられる形（名前と型引数）にしたもの。参照型の引数に、型の合わない
    /// オブジェクトを渡さないために使う（渡すと、対象のプロセスが落ちる）。分からない型は `None`。
    /// `class_arguments` は、メソッドを持つ型の型引数（`List<string>` の `string`）。
    /// `method_arguments` は、メソッドの型引数（`Echo<int>` の `int`）。
    pub fn parameter_types(&self, method: u32, class_arguments: &[String], method_arguments: &[String]) -> Option<Vec<Option<TypeShape>>> {
        const GENERIC: u8 = 0x10;
        let signature = self.blob(self.cell(METHOD_DEF, method & 0x00FF_FFFF, 4)).ok()?;
        let mut r = Reader::new(signature);
        if r.u8().ok()? & GENERIC != 0 {
            r.unsigned().ok()?;
        }
        let count = r.unsigned().ok()?;
        read_type(&mut r)?; // 戻り値
        (0..count).map(|_| self.read_type_shape(&mut r, class_arguments, method_arguments, 0)).collect()
    }

    /// 型が自分で持つ型引数の数（外側の型の分も含む）。
    pub fn type_parameter_count(&self, type_row: u32) -> usize {
        self.generic_params(type_row << 1).len()
    }

    /// メソッドの戻り値の型（名前と型引数）。`void`、分からない型、読めないシグネチャは `None`。
    pub fn return_type(&self, method: u32, class_arguments: &[String]) -> Option<TypeShape> {
        const GENERIC: u8 = 0x10;
        let signature = self.blob(self.cell(METHOD_DEF, method & 0x00FF_FFFF, 4)).ok()?;
        let mut r = Reader::new(signature);
        if r.u8().ok()? & GENERIC != 0 {
            r.unsigned().ok()?;
        }
        r.unsigned().ok()?;
        self.read_type_shape(&mut r, class_arguments, &[], 0)?
    }

    /// シグネチャの中の型を一つ読み進めて、名前と型引数にする。外側の `None` は読めないシグネチャ、内側の `None` は
    /// 名前にできない型（メソッドの型引数、ポインターなど）。
    fn read_type_shape(&self, r: &mut Reader, class_arguments: &[String], method_arguments: &[String], depth: u32) -> Option<Option<TypeShape>> {
        let plain = |name: &str| Some(Some(TypeShape { name: name.to_string(), arguments: Vec::new() }));
        if depth > 16 {
            return None;
        }
        let element = r.u8().ok()?;
        match element {
            0x02 => plain("bool"),
            0x03 => plain("char"),
            0x04 => plain("sbyte"),
            0x05 => plain("byte"),
            0x06 => plain("short"),
            0x07 => plain("ushort"),
            0x08 => plain("int"),
            0x09 => plain("uint"),
            0x0A => plain("long"),
            0x0B => plain("ulong"),
            0x0C => plain("float"),
            0x0D => plain("double"),
            0x0E => plain("string"),
            0x1C => plain("object"),
            0x11 | 0x12 => {
                let name = self.type_def_or_ref_name(r.unsigned().ok()?);
                Some(name.map(|name| TypeShape { name: without_arity(&name), arguments: Vec::new() }))
            }
            // 型の型引数は、実際の型に置き換える。
            0x13 => {
                let index = r.unsigned().ok()? as usize;
                Some(class_arguments.get(index).map(|name| TypeShape { name: name.clone(), arguments: Vec::new() }))
            }
            // ref / out は、中の型。
            0x10 => self.read_type_shape(r, class_arguments, method_arguments, depth + 1),
            // メソッドの型引数も、決まっていれば実際の型に置き換える。
            0x1E => {
                let index = r.unsigned().ok()? as usize;
                Some(method_arguments.get(index).map(|name| TypeShape { name: name.clone(), arguments: Vec::new() }))
            }
            0x15 => {
                r.u8().ok()?;
                let name = self.type_def_or_ref_name(r.unsigned().ok()?);
                let mut arguments = Vec::new();
                let mut known = true;
                for _ in 0..r.unsigned().ok()? {
                    match self.read_type_shape(r, class_arguments, method_arguments, depth + 1)? {
                        Some(shape) => arguments.push(shape.display()),
                        None => known = false,
                    }
                }
                Some(name.filter(|_| known).map(|name| TypeShape { name: without_arity(&name), arguments }))
            }
            0x1D => Some(self.read_type_shape(r, class_arguments, method_arguments, depth + 1)?.map(|shape| TypeShape { name: format!("{}[]", shape.display()), arguments: Vec::new() })),
            0x1F | 0x20 => {
                r.unsigned().ok()?;
                self.read_type_shape(r, class_arguments, method_arguments, depth + 1)
            }
            // そのほか（メソッドの型引数、ポインター、多次元の配列など）は、読み進めるだけ。
            _ => {
                r.pos -= 1;
                read_type(r)?;
                Some(None)
            }
        }
    }

    /// メソッドの戻り値の型の大まかな種類。`void` と、読めないシグネチャは `None`。
    pub fn return_kind(&self, method: u32) -> Option<ParameterKind> {
        const GENERIC: u8 = 0x10;
        let signature = self.blob(self.cell(METHOD_DEF, method & 0x00FF_FFFF, 4)).ok()?;
        let mut r = Reader::new(signature);
        if r.u8().ok()? & GENERIC != 0 {
            r.unsigned().ok()?;
        }
        r.unsigned().ok()?;
        read_type(&mut r)
    }

    /// フィールドの型の大まかな種類。シグネチャは「0x06、型」。
    pub fn field_kind(&self, field: u32) -> Option<ParameterKind> {
        const FIELD_SIGNATURE: u8 = 0x06;
        let mut r = Reader::new(self.blob(self.cell(FIELD, field & 0x00FF_FFFF, 2)).ok()?);
        if r.u8().ok()? != FIELD_SIGNATURE {
            return None;
        }
        read_type(&mut r)
    }

    /// フィールドの型の、名前と型引数。`class_arguments` は、フィールドを持つ型の型引数（表示の書き方）。
    pub fn field_type(&self, field: u32, class_arguments: &[String]) -> Option<TypeShape> {
        const FIELD_SIGNATURE: u8 = 0x06;
        let mut r = Reader::new(self.blob(self.cell(FIELD, field & 0x00FF_FFFF, 2)).ok()?);
        if r.u8().ok()? != FIELD_SIGNATURE {
            return None;
        }
        self.read_type_shape(&mut r, class_arguments, &[], 0)?
    }

    /// ローカル変数のシグネチャ（StandAloneSig の行）の、変数ごとの型の大まかな種類。シグネチャは「0x07、数、型の並び」。
    /// 型の前の `pinned`（0x45）と修飾子（0x1F / 0x20 とトークン）は読み飛ばす。
    pub fn local_kinds(&self, signature: u32) -> Option<Vec<ParameterKind>> {
        const STAND_ALONE_SIG: usize = 0x11;
        const LOCAL_SIGNATURE: u8 = 0x07;
        let mut r = Reader::new(self.blob(self.cell(STAND_ALONE_SIG, signature & 0x00FF_FFFF, 0)).ok()?);
        if r.u8().ok()? != LOCAL_SIGNATURE {
            return None;
        }
        let count = r.unsigned().ok()?;
        let mut kinds = Vec::with_capacity(count as usize);
        for _ in 0..count {
            loop {
                match r.data.get(r.pos) {
                    Some(0x45) => r.pos += 1,
                    Some(0x1F | 0x20) => {
                        r.pos += 1;
                        r.unsigned().ok()?;
                    }
                    _ => break,
                }
            }
            kinds.push(read_type(&mut r)?);
        }
        Some(kinds)
    }

    /// フィールドの型が値型（構造体、列挙型）か。シグネチャは「0x06、型」。
    pub fn field_is_value_type(&self, field: u32) -> bool {
        const FIELD_SIGNATURE: u8 = 0x06;
        const ELEMENT_TYPE_VALUETYPE: u8 = 0x11;
        matches!(self.blob(self.cell(FIELD, field & 0x00FF_FFFF, 2)), Ok([FIELD_SIGNATURE, ELEMENT_TYPE_VALUETYPE, ..]))
    }

    /// 定数（`const`）のフィールドの値。「型（CorElementType）、値のバイト列」。文字列は UTF-16、`null` は型が 0x12。
    pub fn constant(&self, field: u32) -> Option<(u8, &[u8])> {
        const CONSTANT: usize = 0x0B;
        // Constant は Parent（HasConstant: フィールドは 行 << 2）の順に並んでいる。
        let parent = (field & 0x00FF_FFFF) << 2;
        let row = *self.matching_rows(CONSTANT, 1, parent).first()?;
        Some(((self.cell(CONSTANT, row, 0) & 0xFF) as u8, self.blob(self.cell(CONSTANT, row, 2)).ok()?))
    }

    /// TypeDefOrRef の複合添字が指す型の、完全な名前（型引数は付けない）。ジェネリック型のインスタンス（TypeSpec）は、
    /// もとのジェネリック型の名前。
    fn type_def_or_ref_name(&self, coded: u32) -> Option<String> {
        const TYPE_REF: usize = 0x01;
        const TYPE_SPEC: usize = 0x1B;
        let row = coded >> 2;
        match coded & 3 {
            0 if row != 0 => Some(self.plain_type_name(row, 0)),
            1 => {
                let raw = self.string(self.cell(TYPE_REF, row, 1));
                let base = raw.rsplit_once('`').map_or(raw, |(base, _)| base);
                // 入れ子の型の TypeRef は、ResolutionScope が外側の TypeRef（下位 2 ビットが 3）。
                let scope = self.cell(TYPE_REF, row, 0);
                Some(match (scope & 3, self.string(self.cell(TYPE_REF, row, 2))) {
                    (3, _) => format!("{}.{base}", self.type_def_or_ref_name((scope >> 2) << 2 | 1)?),
                    (_, "") => base.to_string(),
                    (_, namespace) => format!("{namespace}.{base}"),
                })
            }
            2 => {
                // シグネチャ: GENERICINST（0x15）、CLASS か VALUETYPE、TypeDefOrRef の圧縮整数、…
                let signature = self.blob(self.cell(TYPE_SPEC, row, 0)).ok()?;
                let mut r = Reader::new(signature);
                if r.u8().ok()? != 0x15 {
                    return None;
                }
                r.u8().ok()?;
                self.type_def_or_ref_name(r.unsigned().ok()?)
            }
            _ => None,
        }
    }

    /// 基底の型の完全な名前（`System.Object`、`System.ValueType`、`System.Enum` など）。
    pub fn base_type_name(&self, type_row: u32) -> Option<String> {
        self.type_def_or_ref_name(self.cell(TYPE_DEF, type_row, 3))
    }

    /// 型が直接実装しているインターフェイスの、完全な名前。
    pub fn interface_names(&self, type_row: u32) -> Vec<String> {
        const INTERFACE_IMPL: usize = 0x09;
        let mut names = Vec::new();
        // InterfaceImpl は Class の順に並んでいる。
        for row in self.matching_rows(INTERFACE_IMPL, 0, type_row) {
            names.extend(self.type_def_or_ref_name(self.cell(INTERFACE_IMPL, row, 1)));
        }
        names
    }

    /// 基底の型（同じモジュールの中にあるときだけ）。
    pub fn base_type(&self, type_row: u32) -> Option<u32> {
        // Extends は TypeDefOrRef: 下位 2 ビットが 0 なら TypeDef。
        let extends = self.cell(TYPE_DEF, type_row, 3);
        (extends & 3 == 0 && extends >> 2 != 0).then_some(extends >> 2)
    }

    /// 型が宣言している、引数の無い静的プロパティの getter（MethodDef トークン）。
    pub fn static_property_getter(&self, type_row: u32, name: &str) -> Option<u32> {
        self.properties(type_row, true).into_iter().find(|(existing, _)| *existing == name).map(|(_, getter)| getter)
    }

    /// 型の名前から型引数を除いたもの（`Namespace.Outer.Inner`）。`find_type` に渡せる形。
    pub fn plain_name(&self, type_row: u32) -> String {
        self.plain_type_name(type_row, 0)
    }

    /// 完全な名前（`Namespace.Outer.Inner`。型引数は書かない）に合う型の行番号。
    pub fn find_type(&self, name: &str) -> Option<u32> {
        // 先に、いちばん後ろの部分だけを比べる（名前を組み立てるのは、そこが合ったものだけ）。
        // 型引数の無い型（`Holder`）を先に探す。同じ名前のジェネリック型（``Holder`1``）は、無かったときだけ。
        let last = name.rsplit('.').next().unwrap_or(name);
        let matches = |row: u32, exact: bool| {
            let raw = self.string(self.cell(TYPE_DEF, row, 1));
            let base = if exact { raw } else { raw.rsplit_once('`').map_or(raw, |(base, _)| base) };
            base == last && self.plain_type_name(row, 0) == name
        };
        (1..=self.rows(TYPE_DEF)).find(|&row| matches(row, true)).or_else(|| (1..=self.rows(TYPE_DEF)).find(|&row| matches(row, false)))
    }

    /// 完全な名前（型引数は書かない）と型引数の数に合う、ジェネリック型の行番号（`List` と 1 で ``List`1``）。
    pub fn find_generic_type(&self, name: &str, arity: usize) -> Option<u32> {
        let last = name.rsplit('.').next().unwrap_or(name);
        let raw_name = format!("{last}`{arity}");
        (1..=self.rows(TYPE_DEF)).find(|&row| self.string(self.cell(TYPE_DEF, row, 1)) == raw_name && self.plain_type_name(row, 0) == name)
    }

    /// C# のコンパイラが作った匿名型（`<>f__AnonymousType番号`）のうち、メンバーの名前がこの順に並ぶもの。
    /// 匿名型は、メンバーごとに型引数を持ち、その名前は `<名前>j__TPar`。
    pub fn anonymous_type(&self, members: &[&str]) -> Option<u32> {
        (1..=self.rows(TYPE_DEF)).find(|&row| {
            self.string(self.cell(TYPE_DEF, row, 1)).starts_with("<>f__AnonymousType") && {
                let parameters = self.generic_params(row << 1);
                parameters.len() == members.len() && parameters.iter().zip(members).all(|(parameter, member)| parameter.strip_prefix('<').and_then(|rest| rest.strip_suffix(">j__TPar")) == Some(member))
            }
        })
    }

    /// 型の名前空間（入れ子の型なら、いちばん外側の型の名前空間）。
    pub fn namespace_of(&self, type_row: u32) -> &str {
        let mut row = type_row;
        for _ in 0..32 {
            match self.enclosing_type(row) {
                Some(outer) => row = outer,
                None => break,
            }
        }
        self.string(self.cell(TYPE_DEF, row, 2))
    }

    /// 型が宣言している、読めるインスタンスプロパティ（名前と、getter の MethodDef トークン）。宣言の順。
    /// 引数を取るもの（インデクサー）は、何を渡すか決められないので含めない。
    pub fn instance_properties(&self, type_row: u32) -> Vec<(&str, u32)> {
        self.properties(type_row, false)
    }

    /// 型が宣言している、読める静的プロパティ（名前と、getter の MethodDef トークン）。宣言の順。
    pub fn static_properties(&self, type_row: u32) -> Vec<(&str, u32)> {
        self.properties(type_row, true)
    }

    fn properties(&self, type_row: u32, is_static: bool) -> Vec<(&str, u32)> {
        const PROPERTY_MAP: usize = 0x15;
        const PROPERTY: usize = 0x17;
        const METHOD_SEMANTICS: usize = 0x18;
        const GETTER: u32 = 0x0002;
        let Some(map) = (1..=self.rows(PROPERTY_MAP)).find(|&row| self.cell(PROPERTY_MAP, row, 0) == type_row) else { return Vec::new() };
        let mut properties = Vec::new();
        for property in self.child_rows(PROPERTY_MAP, map, 1, PROPERTY) {
            // MethodSemantics は Association（HasSemantics: プロパティは 行 << 1 | 1）の順に並んでいる。
            let association = property << 1 | 1;
            for row in self.matching_rows(METHOD_SEMANTICS, 2, association) {
                let method = self.cell(METHOD_SEMANTICS, row, 1);
                if self.cell(METHOD_SEMANTICS, row, 0) & GETTER != 0 && self.has_shape(method, is_static, 0) {
                    properties.push((self.string(self.cell(PROPERTY, property, 1)), (METHOD_DEF as u32) << 24 | method));
                }
            }
        }
        properties
    }

    /// プロパティのアクセサーか演算子か（`get_X` / `set_X` / `op_X`。コンパイラが特別な名前の印を付ける）。
    /// ステップインは、既定ではこれらの中に入らない。
    pub fn is_property_or_operator(&self, method_row: u32) -> bool {
        const SPECIAL_NAME: u32 = 0x0800;
        let name = self.string(self.cell(METHOD_DEF, method_row, 3));
        self.cell(METHOD_DEF, method_row, 2) & SPECIAL_NAME != 0 && ["get_", "set_", "op_"].iter().any(|prefix| name.starts_with(prefix))
    }

    /// メソッドの引数の名前。添字は宣言の順（`this` は含まない）。名前の記録が無い引数は空文字列。
    pub fn parameter_names(&self, method_row: u32) -> Vec<&str> {
        let mut names = Vec::new();
        for row in self.child_rows(METHOD_DEF, method_row, 5, PARAM) {
            // Sequence は 1 始まり。0 は戻り値の行で、引数ではない。
            let sequence = self.cell(PARAM, row, 1) as usize;
            if sequence == 0 {
                continue;
            }
            if names.len() < sequence {
                names.resize(sequence, "");
            }
            names[sequence - 1] = self.string(self.cell(PARAM, row, 2));
        }
        names
    }

    /// 型が宣言しているフィールド（基底の型の分は含まない）。
    pub fn fields(&self, type_row: u32) -> Vec<Field<'_>> {
        const STATIC: u32 = 0x0010;
        const LITERAL: u32 = 0x0040;
        self.child_rows(TYPE_DEF, type_row, 4, FIELD)
            .map(|row| {
                let flags = self.cell(FIELD, row, 0);
                Field { token: (FIELD as u32) << 24 | row, name: self.string(self.cell(FIELD, row, 1)), is_static: flags & STATIC != 0, is_literal: flags & LITERAL != 0 }
            })
            .collect()
    }

    /// カスタム属性の型の「名前空間、名前」。属性のコンストラクターは、ほかのアセンブリの型なら MemberRef、
    /// 同じアセンブリの型なら MethodDef で指されている。
    fn attribute_type(&self, attribute_row: u32) -> Option<(&str, &str)> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        const MEMBER_REF: usize = 0x0A;
        const TYPE_REF: usize = 0x01;
        let constructor = self.cell(CUSTOM_ATTRIBUTE, attribute_row, 1);
        match (constructor & 7, constructor >> 3) {
            (2, method) => {
                let ty = self.declaring_type(method)?;
                Some((self.string(self.cell(TYPE_DEF, ty, 2)), self.string(self.cell(TYPE_DEF, ty, 1))))
            }
            (3, member) => {
                let parent = self.cell(MEMBER_REF, member, 0);
                // MemberRefParent の 1 番が TypeRef。
                (parent & 7 == 1).then(|| (self.string(self.cell(TYPE_REF, parent >> 3, 2)), self.string(self.cell(TYPE_REF, parent >> 3, 1))))
            }
            _ => None,
        }
    }

    /// `parent`（HasCustomAttribute の複合添字）に、その名前の属性が付いているか。
    fn has_attribute(&self, parent: u32, namespace: &str, name: &str) -> bool {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some((namespace, name)) {
                return true;
            }
        }
        false
    }

    /// `parent` に付いた、その名前の属性の、最初の引数の文字列。
    fn attribute_string(&self, parent: u32, namespace: &str, name: &str) -> Option<String> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some((namespace, name)) {
                // 値: 0x0001、続いて文字列（長さの圧縮整数と UTF-8。0xFF は null）。
                let value = self.blob(self.cell(CUSTOM_ATTRIBUTE, row, 2)).ok()?;
                let mut r = Reader::new(value);
                if r.u16().ok()? != 1 || value.get(2) == Some(&0xFF) {
                    return None;
                }
                let length = r.unsigned().ok()? as usize;
                let start = r.pos;
                return value.get(start..start + length).map(|bytes| String::from_utf8_lossy(bytes).into_owned());
            }
        }
        None
    }

    /// C# 14 の拡張ブロックの静的メンバー（`extension(IEnumerable<int>) { public static int Answer => 42; }`）のうち、
    /// 実装のメソッドの名前が `name`（プロパティなら `get_Answer`）のもの。戻り値は、実装のメソッドを持つ型（拡張ブロックを
    /// 書いた静的クラス）の行と、そのブロックの受け手の型（表示の書き方）。
    ///
    /// コンパイラは、ブロックごとに、入れ子の型 `<G>$…`（宣言の形だけのメンバーを持つ）と、その中の `<M>$…`（受け手の型を
    /// 引数にした `<Extension>$` を持つ）を作る。宣言の形だけのメンバーには、どの `<M>$…` のものかを `ExtensionMarker` 属性で
    /// 書く。実装は、静的クラスの、同じ名前の静的メソッド（静的メンバーは、受け手の引数を持たない）。
    pub fn static_extension_members(&self, name: &str) -> Vec<(u32, String)> {
        let mut found = Vec::new();
        for nested_row in 1..=self.rows(NESTED_CLASS) {
            let (grouping, container) = (self.cell(NESTED_CLASS, nested_row, 0), self.cell(NESTED_CLASS, nested_row, 1));
            if !self.string(self.cell(TYPE_DEF, grouping, 1)).starts_with("<G>$") {
                continue;
            }
            for method in self.child_rows(TYPE_DEF, grouping, 5, METHOD_DEF) {
                let is_static = self.cell(METHOD_DEF, method, 2) & 0x0010 != 0;
                if !is_static || self.string(self.cell(METHOD_DEF, method, 3)) != name {
                    continue;
                }
                // HasCustomAttribute: MethodDef は 0。
                let Some(marker) = self.attribute_string(method << 5, "System.Runtime.CompilerServices", "ExtensionMarkerAttribute") else { continue };
                let marker_row = (1..=self.rows(NESTED_CLASS)).find_map(|row| {
                    let inner = self.cell(NESTED_CLASS, row, 0);
                    (self.cell(NESTED_CLASS, row, 1) == grouping && self.string(self.cell(TYPE_DEF, inner, 1)) == marker).then_some(inner)
                });
                let Some(marker_row) = marker_row else { continue };
                let receiver = self.child_rows(TYPE_DEF, marker_row, 5, METHOD_DEF).find(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == "<Extension>$").and_then(|row| self.parameter_types(row, &[], &[])).and_then(|types| types.into_iter().next().flatten());
                if let Some(receiver) = receiver {
                    found.push((container, receiver.display()));
                }
            }
        }
        found
    }

    /// 型が宣言している、その名前のメソッドすべての「呼び出しの形」。
    /// メソッドから見える型引数の名前: 型の分（入れ子の型は外側の分も）に続けてメソッドの分。
    /// ICorDebugILFrame2::EnumerateTypeParameters と同じ順。
    pub fn type_parameter_names(&self, method_row: u32) -> Vec<&str> {
        let mut names = self.declaring_type(method_row).map(|ty| self.generic_params(ty << 1)).unwrap_or_default();
        names.extend(self.generic_params(method_row << 1 | 1));
        names
    }

    /// メソッドの名前だけ（型の名前を付けない）。
    pub fn method_simple_name(&self, method_row: u32) -> Option<&str> {
        (method_row != 0 && method_row <= self.rows(METHOD_DEF)).then(|| self.string(self.cell(METHOD_DEF, method_row, 3)))
    }

    /// 型が宣言している、その名前のメソッドの MethodDef の行番号（宣言の順）。
    pub fn method_rows_named(&self, type_row: u32, name: &str) -> Vec<u32> {
        self.child_rows(TYPE_DEF, type_row, 5, METHOD_DEF).filter(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == name).collect()
    }

    pub fn methods_named(&self, type_row: u32, name: &str) -> Vec<MethodShape> {
        self.child_rows(TYPE_DEF, type_row, 5, METHOD_DEF).filter(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == name).filter_map(|row| self.method_shape(row)).collect()
    }

    /// モジュールの中の、その名前の拡張メソッドすべて。2 番目は、宣言している型の名前空間。
    /// C# 14 の extension ブロックのメンバー（拡張プロパティの `get_名前` など）は、拡張メソッドの印が無いので、
    /// 宣言している型が extension ブロックの入れ物を持つことで見分ける。
    pub fn extension_methods(&self, name: &str) -> Vec<(MethodShape, &str)> {
        (1..=self.rows(METHOD_DEF))
            .filter(|&row| self.string(self.cell(METHOD_DEF, row, 3)) == name)
            .filter_map(|row| self.method_shape(row))
            .filter(|shape| shape.is_extension || (shape.is_static && !shape.parameters.is_empty() && self.declaring_type(shape.token & 0x00FF_FFFF).is_some_and(|ty| self.has_extension_blocks(ty))))
            .filter_map(|shape| Some((self.namespace_of(self.declaring_type(shape.token & 0x00FF_FFFF)?), shape)).map(|(namespace, shape)| (shape, namespace)))
            .collect()
    }

    /// 型が、C# 14 の extension ブロックを持つか（コンパイラが、ブロックごとに `<G>$…` という入れ子の型を作る）。
    fn has_extension_blocks(&self, type_row: u32) -> bool {
        (1..=self.rows(NESTED_CLASS)).any(|row| self.cell(NESTED_CLASS, row, 1) == type_row && self.string(self.cell(TYPE_DEF, self.cell(NESTED_CLASS, row, 0), 1)).starts_with("<G>$"))
    }

    /// メソッドの「呼び出しの形」: 引数の名前、省略できるか、`params` か、型引数の数。読めないシグネチャは `None`。
    pub fn method_shape(&self, method_row: u32) -> Option<MethodShape> {
        const HAS_THIS: u8 = 0x20;
        const GENERIC: u8 = 0x10;
        const OPTIONAL: u32 = 0x0010;
        const HAS_DEFAULT: u32 = 0x1000;
        const CONSTANT: usize = 0x0B;
        let signature = self.blob(self.cell(METHOD_DEF, method_row, 4)).ok()?;
        let mut r = Reader::new(signature);
        let convention = r.u8().ok()?;
        let type_parameters = if convention & GENERIC != 0 { r.unsigned().ok()? } else { 0 };
        let count = r.unsigned().ok()? as usize;
        read_type(&mut r)?;
        let mut parameters: Vec<Parameter> = Vec::with_capacity(count);
        for _ in 0..count {
            let start = r.pos;
            let generic = read_generic_use(&mut r);
            // 型引数の使われ方を読めなかったときは、型として読み飛ばす。
            if generic.is_none() {
                r.pos = start;
                read_type(&mut r)?;
            }
            parameters.push(Parameter { name: String::new(), optional: false, default: None, is_params: false, generic: generic.unwrap_or(GenericUse::None) });
        }
        for row in self.child_rows(METHOD_DEF, method_row, 5, PARAM) {
            // Sequence は 1 始まり。0 は戻り値の行。
            let sequence = self.cell(PARAM, row, 1) as usize;
            let Some(parameter) = sequence.checked_sub(1).and_then(|index| parameters.get_mut(index)) else { continue };
            let flags = self.cell(PARAM, row, 0);
            parameter.name = self.string(self.cell(PARAM, row, 2)).to_string();
            parameter.optional = flags & OPTIONAL != 0;
            if flags & HAS_DEFAULT != 0 {
                // Constant の Parent（HasConstant）: Param は 行 << 2 | 1。
                let parent = row << 2 | 1;
                if let Some(&constant) = self.matching_rows(CONSTANT, 1, parent).first() {
                    parameter.default = self.blob(self.cell(CONSTANT, constant, 2)).ok().map(|bytes| ((self.cell(CONSTANT, constant, 0) & 0xFF) as u8, bytes.to_vec()));
                }
            }
            // HasCustomAttribute: Param は 行 << 5 | 4。
            parameter.is_params = self.has_attribute(row << 5 | 4, "System", "ParamArrayAttribute");
        }
        Some(MethodShape {
            token: (METHOD_DEF as u32) << 24 | method_row,
            is_static: convention & HAS_THIS == 0,
            type_parameters,
            parameters,
            // HasCustomAttribute: MethodDef は 行 << 5 | 0。
            is_extension: convention & HAS_THIS == 0 && self.has_attribute(method_row << 5, "System.Runtime.CompilerServices", "ExtensionAttribute"),
        })
    }

    /// `parent`（HasCustomAttribute の複合添字）に付いた `[DebuggerBrowsable(…)]` の値（0: Never、2: Collapsed、3: RootHidden）。
    fn debugger_browsable(&self, parent: u32) -> Option<u32> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some(("System.Diagnostics", "DebuggerBrowsableAttribute")) {
                // 値: 0x0001、続いて引数の列挙型の値（int32）。
                let mut r = Reader::new(self.blob(self.cell(CUSTOM_ATTRIBUTE, row, 2)).ok()?);
                return (r.u16().ok()? == 1).then(|| r.u32().ok()).flatten();
            }
        }
        None
    }

    /// 型が宣言しているフィールドとプロパティのうち、`[DebuggerBrowsable]` の付いたもの。「隠すもの、中身をその場に並べる
    /// もの」の名前（フィールドの名前は、表示の名前。自動実装プロパティの裏のフィールドは、プロパティの名前）。
    pub fn browsable_rules(&self, type_row: u32) -> (Vec<String>, Vec<String>) {
        const NEVER: u32 = 0;
        const ROOT_HIDDEN: u32 = 3;
        const PROPERTY_MAP: usize = 0x15;
        const PROPERTY: usize = 0x17;
        let (mut never, mut root_hidden) = (Vec::new(), Vec::new());
        let mut sort = |state: Option<u32>, name: String| match state {
            Some(NEVER) => never.push(name),
            Some(ROOT_HIDDEN) => root_hidden.push(name),
            _ => {}
        };
        // HasCustomAttribute: Field は 行 << 5 | 1、Property は 行 << 5 | 9。
        for row in self.child_rows(TYPE_DEF, type_row, 4, FIELD) {
            let name = self.string(self.cell(FIELD, row, 1));
            // 自動実装プロパティの裏のフィールドには、コンパイラが Never を付ける。裏のフィールドは、プロパティの名前で
            // プロパティの代わりに見せているので、この印は見ない（プロパティ自身の印は、下で見る）。
            if name.ends_with(">k__BackingField") {
                continue;
            }
            sort(self.debugger_browsable(row << 5 | 1), crate::debugger::value::member_name(name).to_string());
        }
        if let Some(map) = (1..=self.rows(PROPERTY_MAP)).find(|&row| self.cell(PROPERTY_MAP, row, 0) == type_row) {
            for row in self.child_rows(PROPERTY_MAP, map, 1, PROPERTY) {
                sort(self.debugger_browsable(row << 5 | 9), self.string(self.cell(PROPERTY, row, 1)).to_string());
            }
        }
        (never, root_hidden)
    }

    /// 型が宣言している、公開されていない（public でない）インスタンスフィールドの、表示の名前。
    pub fn non_public_fields(&self, type_row: u32) -> Vec<String> {
        const ACCESS: u32 = 0x0007;
        const PUBLIC: u32 = 0x0006;
        const STATIC: u32 = 0x0010;
        self.child_rows(TYPE_DEF, type_row, 4, FIELD)
            .filter(|&row| {
                let flags = self.cell(FIELD, row, 0);
                flags & STATIC == 0 && flags & ACCESS != PUBLIC
            })
            .map(|row| crate::debugger::value::member_name(self.string(self.cell(FIELD, row, 1))).to_string())
            .collect()
    }

    /// 型に付いた `[DebuggerTypeProxy(…)]` の、代わりの型の名前（アセンブリの名前を除き、入れ子の `+` は `.` にしたもの）。
    pub fn type_proxy(&self, type_row: u32) -> Option<String> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        let parent = type_row << 5 | 3;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some(("System.Diagnostics", "DebuggerTypeProxyAttribute")) {
                // 値: 0x0001、続いて型の名前の文字列（Type の引数も、文字列として入っている）。
                let value = self.blob(self.cell(CUSTOM_ATTRIBUTE, row, 2)).ok()?;
                let mut r = Reader::new(value);
                if r.u16().ok()? != 1 || value.get(2) == Some(&0xFF) {
                    return None;
                }
                let length = r.unsigned().ok()? as usize;
                let name = std::str::from_utf8(value.get(r.pos..r.pos + length)?).ok()?;
                return Some(name.split(',').next()?.trim().replace('+', "."));
            }
        }
        None
    }

    /// 型に付いた `[DebuggerDisplay("...")]` の書式（`Count = {Count}` など）。付いていなければ `None`。
    pub fn debugger_display(&self, type_row: u32) -> Option<String> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        // CustomAttribute は Parent の順に並んでいる。TypeDef の Parent は 行 << 5 | 3。
        let parent = type_row << 5 | 3;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some(("System.Diagnostics", "DebuggerDisplayAttribute")) {
                // 値: 0x0001、続いて最初の引数の文字列（長さの圧縮整数と UTF-8。0xFF は null）。
                let value = self.blob(self.cell(CUSTOM_ATTRIBUTE, row, 2)).ok()?;
                let mut r = Reader::new(value);
                if r.u16().ok()? != 1 || value.get(2) == Some(&0xFF) {
                    return None;
                }
                let length = r.unsigned().ok()? as usize;
                return std::str::from_utf8(value.get(r.pos..r.pos + length)?).ok().map(str::to_string);
            }
        }
        None
    }

    /// 型に付いた `[DebuggerDisplay("…", Name = "…")]` の `Name`（変数の欄の名前の書式。`[{Key}]` など）。
    pub fn debugger_display_name(&self, type_row: u32) -> Option<String> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        let parent = type_row << 5 | 3;
        for row in self.matching_rows(CUSTOM_ATTRIBUTE, 0, parent) {
            if self.attribute_type(row) == Some(("System.Diagnostics", "DebuggerDisplayAttribute")) {
                // 値: 0x0001、最初の引数の文字列、名前つきの引数の数（u16）、名前つきの引数（0x53 フィールド / 0x54 プロパティ、
                // 型 0x0E（文字列）、名前、値）の並び。
                let value = self.blob(self.cell(CUSTOM_ATTRIBUTE, row, 2)).ok()?;
                let mut r = Reader::new(value);
                if r.u16().ok()? != 1 {
                    return None;
                }
                let read_string = |r: &mut Reader| -> Option<Option<String>> {
                    if value.get(r.pos) == Some(&0xFF) {
                        r.pos += 1;
                        return Some(None);
                    }
                    let length = r.unsigned().ok()? as usize;
                    let text = std::str::from_utf8(value.get(r.pos..r.pos + length)?).ok()?.to_string();
                    r.pos += length;
                    Some(Some(text))
                };
                read_string(&mut r)?;
                for _ in 0..r.u16().ok()? {
                    let _kind = r.u8().ok()?;
                    if r.u8().ok()? != 0x0E {
                        return None;
                    }
                    let name = read_string(&mut r)??;
                    let text = read_string(&mut r)?;
                    if name == "Name" {
                        return text;
                    }
                }
                return None;
            }
        }
        None
    }

    /// 「利用者のコードではない」と印の付いたメソッド（MethodDef トークン）。ステップで中に入らず、通り過ぎる。
    /// `DebuggerNonUserCode` / `DebuggerStepThrough` / `DebuggerHidden` がメソッドか、その型に付いているもの。
    pub fn non_user_methods(&self) -> Vec<u32> {
        const CUSTOM_ATTRIBUTE: usize = 0x0C;
        const NAMES: [&str; 3] = ["DebuggerNonUserCodeAttribute", "DebuggerStepThroughAttribute", "DebuggerHiddenAttribute"];
        let mut methods = Vec::new();
        for row in 1..=self.rows(CUSTOM_ATTRIBUTE) {
            // HasCustomAttribute: 下位 5 ビットが種類（0 = MethodDef、3 = TypeDef）。
            let parent = self.cell(CUSTOM_ATTRIBUTE, row, 0);
            let (kind, target) = (parent & 31, parent >> 5);
            if kind != 0 && kind != 3 {
                continue;
            }
            if !self.attribute_type(row).is_some_and(|(namespace, name)| namespace == "System.Diagnostics" && NAMES.contains(&name)) {
                continue;
            }
            if kind == 0 {
                methods.push((METHOD_DEF as u32) << 24 | target);
            } else {
                methods.extend(self.child_rows(TYPE_DEF, target, 5, METHOD_DEF).map(|method| (METHOD_DEF as u32) << 24 | method));
            }
        }
        methods.sort_unstable();
        methods.dedup();
        methods
    }

    /// 呼び出し命令のトークンが指すメソッドの、短い名前（`Type.Method`。名前空間と型引数は付けない）。
    /// 戻り値の表示の見出しに使う。型が分からない呼び出し（ジェネリック型のメソッドなど）は、メソッド名だけ。
    pub fn call_name(&self, token: u32) -> String {
        const MEMBER_REF: usize = 0x0A;
        const METHOD_SPEC: usize = 0x2B;
        const TYPE_REF: usize = 0x01;
        let (mut table, mut row) = ((token >> 24) as usize, token & 0x00FF_FFFF);
        // ジェネリックメソッドの呼び出しは MethodSpec。もとのメソッド（MethodDefOrRef）に直す。
        if table == METHOD_SPEC {
            let method = self.cell(METHOD_SPEC, row, 0);
            table = if method & 1 == 0 { METHOD_DEF } else { MEMBER_REF };
            row = method >> 1;
        }
        let (owner, method) = match table {
            METHOD_DEF => (self.declaring_type(row).map(|ty| self.string(self.cell(TYPE_DEF, ty, 1))), self.string(self.cell(METHOD_DEF, row, 3))),
            MEMBER_REF => {
                let parent = self.cell(MEMBER_REF, row, 0);
                let owner = match parent & 7 {
                    0 => Some(self.string(self.cell(TYPE_DEF, parent >> 3, 1))),
                    1 => Some(self.string(self.cell(TYPE_REF, parent >> 3, 1))),
                    _ => None,
                };
                (owner, self.string(self.cell(MEMBER_REF, row, 1)))
            }
            _ => return "?".into(),
        };
        match owner.filter(|name| !name.is_empty()) {
            Some(owner) => format!("{owner}.{method}"),
            None => method.to_string(),
        }
    }

    /// 型の名前から型引数を除いたもの（`Namespace.Outer.Inner`）。名前で探すときの照合に使う。
    fn plain_type_name(&self, type_row: u32, depth: u32) -> String {
        let raw = self.string(self.cell(TYPE_DEF, type_row, 1));
        let base = raw.rsplit_once('`').map_or(raw, |(base, _)| base);
        match self.enclosing_type(type_row).filter(|_| depth < 32) {
            Some(outer) => format!("{}.{base}", self.plain_type_name(outer, depth + 1)),
            None => match self.string(self.cell(TYPE_DEF, type_row, 2)) {
                "" => base.to_string(),
                namespace => format!("{namespace}.{base}"),
            },
        }
    }

    /// 名前に合うメソッド（MethodDef トークン）。オーバーロードはすべて返す。
    ///
    /// `name` は `Namespace.Type.Method`、`Type.Method`、`Method` のどれでもよい（後ろから、`.` の区切りで一致すればよい）。
    /// 型引数と引数の並びは書かない。
    pub fn find_methods(&self, name: &str) -> Vec<u32> {
        let (wanted_type, wanted_method) = match name.rsplit_once('.') {
            Some((ty, method)) => (Some(ty), method),
            None => (None, name),
        };
        let mut found = Vec::new();
        for row in 1..=self.rows(METHOD_DEF) {
            // 先にメソッド名だけを比べる（型名を組み立てるのは、名前が合ったものだけ）。
            if self.string(self.cell(METHOD_DEF, row, 3)) != wanted_method {
                continue;
            }
            let type_matches = match (wanted_type, self.declaring_type(row)) {
                (None, _) => true,
                (Some(wanted), Some(ty)) => {
                    let actual = self.plain_type_name(ty, 0);
                    actual == wanted || actual.strip_suffix(wanted).is_some_and(|rest| rest.ends_with('.'))
                }
                (Some(_), None) => false,
            };
            if type_matches {
                found.push((METHOD_DEF as u32) << 24 | row);
            }
        }
        found
    }

    /// メソッドの名前。`Namespace.Type.Method<T>` の形（引数の括弧は付けない）。`token` は MethodDef（0x06xxxxxx）。
    pub fn method_name(&self, token: u32) -> Option<String> {
        self.method_name_with(token, &[])
    }

    /// 型引数を実際の型の名前にしたメソッドの名前（`Task<int>.InnerInvoke`、`Start<Program.<Main>d__0>`）。
    /// `arguments` は型の分（外側の型から）に続けてメソッドの分（ICorDebugILFrame2::EnumerateTypeParameters の順）。
    /// 足りなければ、宣言の名前（`T`）で補う。
    pub fn method_name_with(&self, token: u32, arguments: &[String]) -> Option<String> {
        if token >> 24 != METHOD_DEF as u32 {
            return None;
        }
        let row = token & 0x00FF_FFFF;
        let ty = self.declaring_type(row)?;
        // 型の型引数の数（入れ子の型は、外側の型の分も自分の GenericParam に持っている）。
        let type_count = self.generic_params(ty << 1).len().min(arguments.len());
        let mut name = self.type_name_with(ty, &arguments[..type_count]);
        name.push('.');
        name.push_str(self.string(self.cell(METHOD_DEF, row, 3)));
        let mut params = self.generic_params(row << 1 | 1);
        for (param, argument) in params.iter_mut().zip(&arguments[type_count..]) {
            *param = argument.as_str();
        }
        push_generic_params(&mut name, &params);
        Some(name)
    }
}

/// 引数の型の大まかな種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParameterKind {
    /// プリミティブ型。中身は CorElementType（`int` は 0x08）。
    Primitive(u8),
    String,
    /// `object`。何でも受け取る。
    Object,
    /// クラス、配列、インターフェイスなど、参照型。
    Reference,
    /// 構造体、列挙型。
    ValueType,
    /// 型引数（`T`）。何が来るかは、インスタンス化しだい。
    TypeParameter,
    /// ポインターなど。式からは渡せない。
    Unsupported,
    /// `ref` / `out`。`out` や `ref` を付けた引数だけを渡せる。
    ByRef,
}

/// メソッドの「呼び出しの形」。省略できる引数、名前つき引数、`params`、型引数の推論に使う。
#[derive(Debug, Clone, PartialEq)]
pub struct MethodShape {
    pub token: u32,
    pub is_static: bool,
    /// メソッドの型引数の数（`Echo<T>` は 1）。
    pub type_parameters: u32,
    pub parameters: Vec<Parameter>,
    /// 拡張メソッド（`this` の付いた最初の引数を持つ静的メソッド）か。
    pub is_extension: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    /// 省略できるか（既定値つき）。
    pub optional: bool,
    /// 既定値（CorElementType と、値のバイト列）。
    pub default: Option<(u8, Vec<u8>)>,
    /// `params` の付いた配列か。
    pub is_params: bool,
    /// 引数の型の中での、メソッドの型引数の使われ方。
    pub generic: GenericUse,
}

/// 引数の型の中で、メソッドの型引数がどう使われているか（型引数の推論に使う）。
#[derive(Debug, Clone, PartialEq)]
pub enum GenericUse {
    /// 使われていない（か、読み取れない形）。
    None,
    /// 型引数そのもの（`T value`）。中身は、型引数の番号。
    Direct(u32),
    /// 型引数の配列（`T[] values`）。
    ArrayOf(u32),
    /// ジェネリック型の型引数として（`IEnumerable<T> source`）。位置ごとの、型引数の番号。
    Inside(Vec<Option<u32>>),
}

/// シグネチャの中の型を一つ読み進めて、メソッドの型引数の使われ方を返す。読み取れない形は `None`（読む位置は不定）。
fn read_generic_use(r: &mut Reader) -> Option<GenericUse> {
    const MVAR: u8 = 0x1E;
    Some(match r.u8().ok()? {
        MVAR => GenericUse::Direct(r.unsigned().ok()?),
        0x1D => {
            let start = r.pos;
            if r.u8().ok()? == MVAR {
                GenericUse::ArrayOf(r.unsigned().ok()?)
            } else {
                r.pos = start;
                read_type(r)?;
                GenericUse::None
            }
        }
        0x15 => {
            r.u8().ok()?;
            r.unsigned().ok()?;
            let mut positions = Vec::new();
            for _ in 0..r.unsigned().ok()? {
                let start = r.pos;
                if r.u8().ok()? == MVAR {
                    positions.push(Some(r.unsigned().ok()?));
                } else {
                    r.pos = start;
                    read_type(r)?;
                    positions.push(None);
                }
            }
            if positions.iter().any(Option::is_some) { GenericUse::Inside(positions) } else { GenericUse::None }
        }
        _ => return None,
    })
}

/// 型の名前と、型引数（`System.Collections.Generic.IEnumerable` と `["string"]`）。名前は、表示と同じ書き方
/// （`int`、`string[]`）。
#[derive(Debug, Clone, PartialEq)]
pub struct TypeShape {
    pub name: String,
    pub arguments: Vec<String>,
}

impl TypeShape {
    pub fn display(&self) -> String {
        if self.arguments.is_empty() { self.name.clone() } else { format!("{}<{}>", self.name, self.arguments.join(", ")) }
    }
}

/// 型の名前から、型引数の数の印（`List`1` の `` `1 ``）を除く。
pub fn without_arity(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            while chars.next_if(|d| d.is_ascii_digit()).is_some() {}
        } else {
            result.push(c);
        }
    }
    result
}

/// シグネチャの中の型を一つ読み進めて、その種類を返す（ECMA-335 II.23.2.12）。
fn read_type(r: &mut Reader) -> Option<ParameterKind> {
    let element = r.u8().ok()?;
    Some(match element {
        0x02..=0x0D | 0x18 | 0x19 => ParameterKind::Primitive(element),
        0x0E => ParameterKind::String,
        0x1C => ParameterKind::Object,
        // void（戻り値だけ）、typedref
        0x01 | 0x16 => ParameterKind::Unsupported,
        // ポインター
        0x0F => {
            read_type(r)?;
            ParameterKind::Unsupported
        }
        // ref / out
        0x10 => {
            read_type(r)?;
            ParameterKind::ByRef
        }
        // 構造体、クラス: トークン
        0x11 | 0x12 => {
            r.unsigned().ok()?;
            if element == 0x11 { ParameterKind::ValueType } else { ParameterKind::Reference }
        }
        // 型引数、メソッドの型引数: 番号
        0x13 | 0x1E => {
            r.unsigned().ok()?;
            ParameterKind::TypeParameter
        }
        // 多次元の配列: 要素の型、次元の数、大きさの並び、下限の並び
        0x14 => {
            read_type(r)?;
            r.unsigned().ok()?;
            for _ in 0..2 {
                for _ in 0..r.unsigned().ok()? {
                    r.unsigned().ok()?;
                }
            }
            ParameterKind::Reference
        }
        // ジェネリック型のインスタンス: クラスか構造体か、トークン、型引数の並び
        0x15 => {
            let kind = if r.u8().ok()? == 0x11 { ParameterKind::ValueType } else { ParameterKind::Reference };
            r.unsigned().ok()?;
            for _ in 0..r.unsigned().ok()? {
                read_type(r)?;
            }
            kind
        }
        // 一次元の配列
        0x1D => {
            read_type(r)?;
            ParameterKind::Reference
        }
        // 修飾子（modreq / modopt）: トークンのあとに本体の型
        0x1F | 0x20 => {
            r.unsigned().ok()?;
            return read_type(r);
        }
        // 関数ポインターなど
        _ => return None,
    })
}

#[derive(Debug, PartialEq)]
pub struct Field<'a> {
    /// FieldDef トークン（0x04xxxxxx）。
    pub token: u32,
    pub name: &'a str,
    pub is_static: bool,
    /// `const`。実体が無く、値はメタデータの定数表にある。
    pub is_literal: bool,
}

/// GUID を `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` の形（.NET の "D" 書式）にする。先頭の 3 つの部分はリトルエンディアン。
pub fn format_guid(guid: &[u8; 16]) -> String {
    let hex = |bytes: &mut dyn Iterator<Item = &u8>| bytes.map(|b| format!("{b:02x}")).collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        hex(&mut guid[0..4].iter().rev()),
        hex(&mut guid[4..6].iter().rev()),
        hex(&mut guid[6..8].iter().rev()),
        hex(&mut guid[8..10].iter()),
        hex(&mut guid[10..16].iter())
    )
}

fn push_generic_params(name: &mut String, params: &[&str]) {
    if !params.is_empty() {
        name.push('<');
        name.push_str(&params.join(", "));
        name.push('>');
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 試験用にメタデータを組み立てる。添字はすべて 2 バイト。
    #[derive(Default)]
    pub(crate) struct Builder {
        strings: Vec<u8>,
        pub blobs: Vec<u8>,
        tables: Vec<(usize, u32, Vec<u8>)>,
    }

    impl Builder {
        pub fn new() -> Builder {
            Builder { strings: vec![0], blobs: vec![0], tables: Vec::new() }
        }
        pub fn string(&mut self, text: &str) -> u16 {
            let at = self.strings.len() as u16;
            self.strings.extend(text.as_bytes());
            self.strings.push(0);
            at
        }
        /// 表を足す。番号の小さい順に呼ぶこと。`rows` は行ごとの 16bit の値の並び（32bit の列は 2 つに分けて渡す）。
        pub fn table(&mut self, table: usize, rows: &[&[u16]]) {
            let bytes = rows.iter().flat_map(|row| row.iter().flat_map(|v| v.to_le_bytes())).collect();
            self.tables.push((table, rows.len() as u32, bytes));
        }
        pub fn build(self) -> Vec<u8> {
            let mut stream = vec![0, 0, 0, 0, 2, 0, 0, 1];
            let valid = self.tables.iter().fold(0u64, |mask, (table, _, _)| mask | 1 << table);
            stream.extend(valid.to_le_bytes());
            stream.extend(0u64.to_le_bytes());
            for (_, rows, _) in &self.tables {
                stream.extend(rows.to_le_bytes());
            }
            for (_, _, bytes) in &self.tables {
                stream.extend(bytes);
            }

            let version = b"v4.0.30319\0\0";
            let mut file = Vec::new();
            file.extend(0x424A_5342u32.to_le_bytes());
            file.extend([1, 0, 1, 0, 0, 0, 0, 0]);
            file.extend((version.len() as u32).to_le_bytes());
            file.extend(version);
            file.extend([0, 0, 3, 0]);
            let mut offset = file.len() + (8 + 4) + (8 + 12) + (8 + 8);
            for (name, stream) in [(&b"#~\0\0"[..], &stream), (&b"#Strings\0\0\0\0"[..], &self.strings), (&b"#Blob\0\0\0"[..], &self.blobs)] {
                file.extend((offset as u32).to_le_bytes());
                file.extend((stream.len() as u32).to_le_bytes());
                file.extend(name);
                offset += stream.len();
            }
            file.extend(&stream);
            file.extend(&self.strings);
            file.extend(&self.blobs);
            file
        }
    }

    /// `App.Outer`（Run）、その中の `Inner`2`（Map`1）、名前空間の無い `Program`（Main, Work）、メソッドの無い `Empty`。
    fn sample() -> Metadata {
        let mut b = Builder::new();
        let (app, outer, inner, program, empty) = (b.string("App"), b.string("Outer`1"), b.string("Inner`2"), b.string("Program"), b.string("Empty"));
        let (run, map, main, work) = (b.string("Run"), b.string("Map"), b.string("Main"), b.string("Work"));
        let (t, k, v, r) = (b.string("T"), b.string("TKey"), b.string("TValue"), b.string("TResult"));
        // TypeDef: Flags(4), Name, Namespace, Extends, FieldList, MethodList
        b.table(TYPE_DEF, &[&[0, 0, outer, app, 0, 1, 1], &[0, 0, inner, 0, 0, 1, 2], &[0, 0, empty, app, 0, 1, 3], &[0, 0, program, 0, 0, 1, 3]]);
        // MethodDef: RVA(4), ImplFlags, Flags, Name, Signature, ParamList
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, run, 0, 1], &[0, 0, 0, 0, map, 0, 1], &[0, 0, 0, 0, main, 0, 1], &[0, 0, 0, 0, work, 0, 1]]);
        b.table(NESTED_CLASS, &[&[2, 1]]);
        // GenericParam: Number, Flags, Owner（TypeDef は 行 << 1、MethodDef は 行 << 1 | 1）, Name
        b.table(GENERIC_PARAM, &[&[0, 0, 1 << 1, t], &[0, 0, 2 << 1, t], &[1, 0, 2 << 1, k], &[2, 0, 2 << 1, v], &[0, 0, 2 << 1 | 1, r]]);
        Metadata::parse(b.build()).unwrap()
    }

    #[test]
    fn a_method_is_named_with_its_namespace_and_type() {
        assert_eq!(sample().method_name(0x0600_0001).as_deref(), Some("App.Outer<T>.Run"));
    }

    #[test]
    fn a_type_without_a_namespace_has_no_leading_dot() {
        let metadata = sample();
        assert_eq!(metadata.method_name(0x0600_0003).as_deref(), Some("Program.Main"));
        assert_eq!(metadata.method_name(0x0600_0004).as_deref(), Some("Program.Work"));
    }

    #[test]
    fn a_nested_generic_type_shows_only_its_own_type_parameters() {
        assert_eq!(sample().method_name(0x0600_0002).as_deref(), Some("App.Outer<T>.Inner<TKey, TValue>.Map<TResult>"));
    }

    #[test]
    fn a_type_without_methods_does_not_claim_the_next_types_methods() {
        let metadata = sample();
        assert_eq!(metadata.declaring_type(3), Some(4));
        assert_eq!(metadata.type_name(3), "App.Empty");
    }

    #[test]
    fn a_token_that_is_not_a_known_method_has_no_name() {
        let metadata = sample();
        assert_eq!(metadata.method_name(0x0600_0005), None);
        assert_eq!(metadata.method_name(0x0600_0000), None);
        assert_eq!(metadata.method_name(0x0A00_0001), None);
    }

    #[test]
    fn an_index_widens_when_its_target_table_is_large() {
        let mut rows = [0u32; TABLE_COUNT];
        assert_eq!(column_width(Table(0x06), &rows, 0, false), 2);
        // TypeDefOrRef は種類に 2 ビット使うので、行数が 2^14 以上で 4 バイトになる。
        rows[0x01] = (1 << 14) - 1;
        assert_eq!(column_width(TYPE_DEF_OR_REF, &rows, 0, false), 2);
        rows[0x01] = 1 << 14;
        assert_eq!(column_width(TYPE_DEF_OR_REF, &rows, 0, false), 4);
        rows[0x06] = 0x1_0000;
        assert_eq!(column_width(Table(0x06), &rows, 0, false), 4);
        assert_eq!(column_width(Str, &rows, 1, false), 4);
        assert_eq!(column_width(Blob, &rows, 1, false), 2);
    }

    #[test]
    fn type_arguments_replace_the_declared_names_from_the_outside_in() {
        let arguments = ["int".to_string(), "string".to_string(), "bool".to_string()];
        assert_eq!(sample().type_name_with(2, &arguments), "App.Outer<int>.Inner<string, bool>");
        assert_eq!(sample().type_name_with(2, &arguments[..1]), "App.Outer<int>.Inner<TKey, TValue>");
    }

    /// `Point`（x, y, static Origin, const Max）と `Program`（static Main(args)、インスタンスの Move(dx, dy)）。
    fn members() -> Metadata {
        let mut b = Builder::new();
        let (point, program) = (b.string("Point"), b.string("Program"));
        let (x, y, origin, max) = (b.string("x"), b.string("y"), b.string("Origin"), b.string("Max"));
        let (main, mv, args, dx, dy) = (b.string("Main"), b.string("Move"), b.string("args"), b.string("dx"), b.string("dy"));
        b.table(TYPE_DEF, &[&[0, 0, point, 0, 0, 1, 1], &[0, 0, program, 0, 0, 5, 1]]);
        // Field: Flags, Name, Signature
        b.table(FIELD, &[&[0x0001, x, 0], &[0x0001, y, 0], &[0x0016, origin, 0], &[0x0056, max, 0]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0x0016, main, 0, 1], &[0, 0, 0, 0x0006, mv, 0, 2]]);
        // Param: Flags, Sequence, Name。Move には戻り値の行（Sequence 0）がある。
        b.table(PARAM, &[&[0, 1, args], &[0, 0, 0], &[0, 1, dx], &[0, 2, dy]]);
        Metadata::parse(b.build()).unwrap()
    }

    #[test]
    fn parameters_are_listed_without_the_return_value_row() {
        let metadata = members();
        assert_eq!(metadata.parameter_names(1), ["args"]);
        assert_eq!(metadata.parameter_names(2), ["dx", "dy"]);
    }

    #[test]
    fn only_an_instance_method_has_this() {
        let metadata = members();
        assert!(!metadata.method_has_this(1));
        assert!(metadata.method_has_this(2));
    }

    #[test]
    fn fields_belong_to_the_type_that_declares_them() {
        let metadata = members();
        let fields = metadata.fields(1);
        assert_eq!(fields.iter().map(|f| f.name).collect::<Vec<_>>(), ["x", "y", "Origin", "Max"]);
        assert_eq!(fields[1], Field { token: 0x0400_0002, name: "y", is_static: false, is_literal: false });
        assert!(fields[2].is_static && !fields[2].is_literal);
        assert!(fields[3].is_literal);
        assert!(metadata.fields(2).is_empty());
    }

    #[test]
    fn methods_are_found_by_full_or_partial_name() {
        let metadata = sample();
        assert_eq!(metadata.find_methods("Program.Work"), [0x0600_0004]);
        assert_eq!(metadata.find_methods("Work"), [0x0600_0004]);
        assert_eq!(metadata.find_methods("App.Outer.Run"), [0x0600_0001]);
        assert_eq!(metadata.find_methods("Outer.Run"), [0x0600_0001]);
        assert_eq!(metadata.find_methods("App.Outer.Inner.Map"), [0x0600_0002], "型引数は書かない");
    }

    #[test]
    fn a_partial_type_name_must_match_whole_segments() {
        let metadata = sample();
        assert!(metadata.find_methods("uter.Run").is_empty());
        assert!(metadata.find_methods("Other.Program.Work").is_empty());
        assert!(metadata.find_methods("Program.Nothing").is_empty());
    }

    #[test]
    fn methods_marked_as_not_user_code_are_listed() {
        let mut b = Builder::new();
        let (diagnostics, step_through, hidden, other) = (b.string("System.Diagnostics"), b.string("DebuggerStepThroughAttribute"), b.string("DebuggerHiddenAttribute"), b.string("ObsoleteAttribute"));
        let (program, generated, name) = (b.string("Program"), b.string("Generated"), b.string("M"));
        // TypeRef: ResolutionScope, Name, Namespace
        b.table(0x01, &[&[0, step_through, diagnostics], &[0, hidden, diagnostics], &[0, other, diagnostics]]);
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1], &[0, 0, generated, 0, 0, 1, 4]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, name, 0, 1], &[0, 0, 0, 0, name, 0, 1], &[0, 0, 0, 0, name, 0, 1], &[0, 0, 0, 0, name, 0, 1], &[0, 0, 0, 0, name, 0, 1]]);
        // MemberRef: Class（TypeRef は 行 << 3 | 1）, Name, Signature。3 つの属性のコンストラクター。
        b.table(0x0A, &[&[1 << 3 | 1, 0, 0], &[2 << 3 | 1, 0, 0], &[3 << 3 | 1, 0, 0]]);
        // CustomAttribute: Parent（MethodDef は 行 << 5、TypeDef は 行 << 5 | 3）, Type（MemberRef は 行 << 3 | 3）, Value
        b.table(0x0C, &[
            &[1 << 5, 1 << 3 | 3, 0],     // メソッド 1 に StepThrough
            &[2 << 5, 3 << 3 | 3, 0],     // メソッド 2 に Obsolete（関係ない属性）
            &[3 << 5, 2 << 3 | 3, 0],     // メソッド 3 に Hidden
            &[2 << 5 | 3, 1 << 3 | 3, 0], // 型 Generated（メソッド 4、5）に StepThrough
        ]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.non_user_methods(), [0x0600_0001, 0x0600_0003, 0x0600_0004, 0x0600_0005]);
    }

    #[test]
    fn metadata_without_attributes_has_no_non_user_methods() {
        assert!(sample().non_user_methods().is_empty());
    }

    #[test]
    fn a_call_is_named_by_its_type_and_method() {
        let mut b = Builder::new();
        let (console, program, write, work, join) = (b.string("Console"), b.string("Program"), b.string("WriteLine"), b.string("Work"), b.string("Join"));
        b.table(0x01, &[&[0, console, 0]]);
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, work, 0, 1]]);
        // MemberRef: Class, Name, Signature。1: Console.WriteLine（TypeRef）、2: 型が TypeSpec（ジェネリック型）の Join。
        b.table(0x0A, &[&[1 << 3 | 1, write, 0], &[1 << 3 | 4, join, 0]]);
        // TypeSpec: Signature。MethodSpec: Method（MethodDef は 行 << 1、MemberRef は 行 << 1 | 1）, Instantiation
        b.table(0x1B, &[&[0]]);
        b.table(0x2B, &[&[1 << 1, 0], &[1 << 1 | 1, 0]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.call_name(0x0600_0001), "Program.Work");
        assert_eq!(metadata.call_name(0x0A00_0001), "Console.WriteLine");
        assert_eq!(metadata.call_name(0x0A00_0002), "Join", "型が分からなければメソッド名だけ");
        assert_eq!(metadata.call_name(0x2B00_0001), "Program.Work", "ジェネリックメソッドは、もとのメソッドの名前");
        assert_eq!(metadata.call_name(0x2B00_0002), "Console.WriteLine");
        assert_eq!(metadata.call_name(0x1100_0001), "?");
    }

    #[test]
    fn accessors_and_operators_are_recognized_by_name_and_flag() {
        let mut b = Builder::new();
        let (program, getter, operator, lookalike, plain) = (b.string("Program"), b.string("get_Total"), b.string("op_Addition"), b.string("get_Lost"), b.string("Run"));
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1]]);
        // 3 つ目は、名前は似ているが特別な名前の印（0x0800）が無い、ふつうのメソッド。
        b.table(METHOD_DEF, &[&[0, 0, 0, 0x0886, getter, 0, 1], &[0, 0, 0, 0x0896, operator, 0, 1], &[0, 0, 0, 0x0086, lookalike, 0, 1], &[0, 0, 0, 0x0086, plain, 0, 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert!(metadata.is_property_or_operator(1));
        assert!(metadata.is_property_or_operator(2));
        assert!(!metadata.is_property_or_operator(3));
        assert!(!metadata.is_property_or_operator(4));
    }

    #[test]
    fn a_parameterless_instance_method_is_found_by_name() {
        let mut b = Builder::new();
        let (program, getter, name) = (b.string("Program"), b.string("get_Message"), b.string("Format"));
        let mut blobs = Vec::new();
        // インスタンスで引数 0、インスタンスで引数 1、静的で引数 0、ジェネリックのインスタンスで引数 0。
        for signature in [&[0x20u8, 0, 0x0E][..], &[0x20, 1, 0x0E, 0x08], &[0x00, 0, 0x0E], &[0x30, 1, 0, 0x0E]] {
            blobs.push(b.blobs.len() as u16);
            b.blobs.push(signature.len() as u8);
            b.blobs.extend(signature);
        }
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, name, blobs[1], 1], &[0, 0, 0, 0, name, blobs[2], 1], &[0, 0, 0, 0, name, blobs[3], 1], &[0, 0, 0, 0, getter, blobs[0], 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.find_method(1, "get_Message", false, 0), Some(0x0600_0004));
        assert_eq!(metadata.find_method(1, "Format", false, 0), None, "引数あり、静的、ジェネリックは対象外");
        assert_eq!(metadata.find_method(1, "Format", false, 1), Some(0x0600_0001), "引数の数を指定すれば見つかる");
        assert_eq!(metadata.find_method(1, "Format", true, 0), Some(0x0600_0002));
        assert_eq!(metadata.find_method(1, "Missing", false, 0), None);
    }

    #[test]
    fn readable_instance_properties_are_listed_with_their_getters() {
        let mut b = Builder::new();
        let (order, other, total, item, shared, write_only) = (b.string("Order"), b.string("Other"), b.string("Total"), b.string("Item"), b.string("Shared"), b.string("WriteOnly"));
        let mut blobs = Vec::new();
        // インスタンスで引数 0、インスタンスで引数 1（インデクサー）、静的で引数 0。
        for signature in [&[0x20u8, 0, 0x08][..], &[0x20, 1, 0x08, 0x08], &[0x00, 0, 0x08]] {
            blobs.push(b.blobs.len() as u16);
            b.blobs.push(signature.len() as u8);
            b.blobs.extend(signature);
        }
        b.table(TYPE_DEF, &[&[0, 0, other, 0, 0, 1, 1], &[0, 0, order, 0, 0, 1, 1]]);
        // メソッド 1: get_Total、2: get_Item(int)、3: 静的な get_Shared、4: set_WriteOnly（中身は見ない）
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, 0, blobs[0], 1], &[0, 0, 0, 0, 0, blobs[1], 1], &[0, 0, 0, 0, 0, blobs[2], 1], &[0, 0, 0, 0, 0, blobs[1], 1]]);
        // PropertyMap: Parent, PropertyList。Property: Flags, Name, Type
        b.table(0x15, &[&[2, 1]]);
        b.table(0x17, &[&[0, total, 0], &[0, item, 0], &[0, shared, 0], &[0, write_only, 0]]);
        // MethodSemantics: Semantics（1 = setter、2 = getter）, Method, Association（プロパティは 行 << 1 | 1）
        b.table(0x18, &[&[2, 1, 1 << 1 | 1], &[2, 2, 2 << 1 | 1], &[2, 3, 3 << 1 | 1], &[1, 4, 4 << 1 | 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.instance_properties(2), [("Total", 0x0600_0001)], "インデクサー、静的、書き込み専用は含めない");
        assert!(metadata.instance_properties(1).is_empty());
    }

    #[test]
    fn the_debugger_display_format_of_a_type_is_read() {
        let mut b = Builder::new();
        let (diagnostics, display, list, plain) = (b.string("System.Diagnostics"), b.string("DebuggerDisplayAttribute"), b.string("List`1"), b.string("Plain"));
        let value = b.blobs.len() as u16;
        let format = b"Count = {Count}";
        b.blobs.push(format.len() as u8 + 3);
        b.blobs.extend([1, 0, format.len() as u8]);
        b.blobs.extend(format);
        b.table(0x01, &[&[0, display, diagnostics]]);
        b.table(TYPE_DEF, &[&[0, 0, plain, 0, 0, 1, 1], &[0, 0, list, 0, 0, 1, 1]]);
        b.table(0x0A, &[&[1 << 3 | 1, 0, 0]]);
        b.table(0x0C, &[&[2 << 5 | 3, 1 << 3 | 3, value]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.debugger_display(2).as_deref(), Some("Count = {Count}"));
        assert_eq!(metadata.debugger_display(1), None);
    }

    #[test]
    fn a_type_is_found_by_its_full_name() {
        let metadata = sample();
        assert_eq!(metadata.find_type("Program"), Some(4));
        assert_eq!(metadata.find_type("App.Outer"), Some(1));
        assert_eq!(metadata.find_type("App.Outer.Inner"), Some(2), "入れ子の型。型引数は書かない");
        assert_eq!(metadata.find_type("Outer"), None, "名前空間を省くと見つからない");
        assert_eq!(metadata.find_type("App.Program"), None);
    }

    #[test]
    fn a_nested_type_has_the_namespace_of_its_outermost_type() {
        let metadata = sample();
        assert_eq!(metadata.namespace_of(2), "App");
        assert_eq!(metadata.namespace_of(4), "");
    }

    #[test]
    fn parameter_kinds_are_read_from_the_signature() {
        let mut b = Builder::new();
        let (program, name) = (b.string("Program"), b.string("M"));
        let mut add = |signature: &[u8]| {
            let at = b.blobs.len() as u16;
            b.blobs.push(signature.len() as u8);
            b.blobs.extend(signature);
            at
        };
        // static void M(int, string, object, List<int>, int[], T, ref int, MyStruct)
        let all = add(&[0x00, 8, 0x01, 0x08, 0x0E, 0x1C, 0x15, 0x12, 0x09, 1, 0x08, 0x1D, 0x08, 0x13, 0, 0x10, 0x08, 0x11, 0x0D]);
        // ジェネリックメソッド T M<T>(T, long)
        let generic = add(&[0x10, 1, 2, 0x1E, 0, 0x1E, 0, 0x0A]);
        // 関数ポインターを取るメソッド（読めない）
        let function_pointer = add(&[0x00, 1, 0x01, 0x1B, 0x00, 0, 0x01]);
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, name, all, 1], &[0, 0, 0, 0, name, generic, 1], &[0, 0, 0, 0, name, function_pointer, 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        use ParameterKind::*;
        assert_eq!(metadata.parameter_kinds(0x0600_0001), Some(vec![Primitive(0x08), String, Object, Reference, Reference, TypeParameter, ByRef, ValueType]));
        assert_eq!(metadata.parameter_kinds(0x0600_0002), Some(vec![TypeParameter, Primitive(0x0A)]));
        assert_eq!(metadata.parameter_kinds(0x0600_0003), None);
    }

    #[test]
    fn overloads_with_the_same_shape_are_all_listed() {
        let mut b = Builder::new();
        let (program, max) = (b.string("Program"), b.string("Max"));
        let mut add = |signature: &[u8]| {
            let at = b.blobs.len() as u16;
            b.blobs.push(signature.len() as u8);
            b.blobs.extend(signature);
            at
        };
        let (bytes, ints, three) = (add(&[0x00, 2, 0x05, 0x05, 0x05]), add(&[0x00, 2, 0x08, 0x08, 0x08]), add(&[0x00, 3, 0x08, 0x08, 0x08, 0x08]));
        b.table(TYPE_DEF, &[&[0, 0, program, 0, 0, 1, 1]]);
        b.table(METHOD_DEF, &[&[0, 0, 0, 0, max, bytes, 1], &[0, 0, 0, 0, max, ints, 1], &[0, 0, 0, 0, max, three, 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.find_methods_shaped(1, "Max", true, 2), [0x0600_0001, 0x0600_0002]);
        assert!(metadata.find_methods_shaped(1, "Max", false, 2).is_empty());
    }

    #[test]
    fn the_base_type_is_followed_inside_the_module() {
        let mut b = Builder::new();
        let (object, value_type, int32) = (b.string("Object"), b.string("ValueType"), b.string("Int32"));
        // Extends: TypeDef は 行 << 2。3 行目は ValueType（2 行目）、2 行目は Object（1 行目）、1 行目は基底なし。
        b.table(TYPE_DEF, &[&[0, 0, object, 0, 0, 1, 1], &[0, 0, value_type, 0, 1 << 2, 1, 1], &[0, 0, int32, 0, 2 << 2, 1, 1]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.base_type(3), Some(2));
        assert_eq!(metadata.base_type(2), Some(1));
        assert_eq!(metadata.base_type(1), None);
    }

    #[test]
    fn the_value_of_a_constant_field_is_read() {
        let mut b = Builder::new();
        let (order, max, label) = (b.string("Order"), b.string("Max"), b.string("Label"));
        let number = b.blobs.len() as u16;
        b.blobs.extend([4, 99, 0, 0, 0]);
        let text = b.blobs.len() as u16;
        b.blobs.extend([4, b'o', 0, b'k', 0]);
        b.table(TYPE_DEF, &[&[0, 0, order, 0, 0, 1, 1]]);
        b.table(FIELD, &[&[0x0056, max, 0], &[0x0056, label, 0], &[0x0006, label, 0]]);
        // Constant: Type（1 バイトと詰め物）, Parent（フィールドは 行 << 2）, Value
        b.table(0x0B, &[&[0x08, 1 << 2, number], &[0x0E, 2 << 2, text]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.constant(0x0400_0001), Some((0x08, &[99u8, 0, 0, 0][..])));
        assert_eq!(metadata.constant(0x0400_0002), Some((0x0E, &[b'o', 0, b'k', 0][..])));
        assert_eq!(metadata.constant(0x0400_0003), None, "定数でないフィールド");
    }

    #[test]
    fn base_and_interface_names_are_read_from_type_refs() {
        let mut b = Builder::new();
        let (system, generic, value_type, comparable, list, point, shape) =
            (b.string("System"), b.string("System.Collections.Generic"), b.string("ValueType"), b.string("IComparable"), b.string("IList`1"), b.string("Point"), b.string("Shape"));
        // TypeSpec の中身: GENERICINST CLASS <TypeRef 3> 1 個の型引数（int）
        let spec = b.blobs.len() as u16;
        b.blobs.extend([5, 0x15, 0x12, 3 << 2 | 1, 1, 0x08]);
        b.table(0x01, &[&[0, value_type, system], &[0, comparable, system], &[0, list, generic]]);
        // Point: 基底は TypeRef 1（ValueType）。Shape: 基底は TypeDef 1（Point。あり得ない継承だが、行の指し方の確認）。
        b.table(TYPE_DEF, &[&[0, 0, point, 0, 1 << 2 | 1, 1, 1], &[0, 0, shape, 0, 1 << 2, 1, 1]]);
        // InterfaceImpl: Class, Interface。Point は IComparable（TypeRef 2）と IList<int>（TypeSpec 1）。
        b.table(0x09, &[&[1, 2 << 2 | 1], &[1, 1 << 2 | 2]]);
        b.table(0x1B, &[&[spec]]);
        let metadata = Metadata::parse(b.build()).unwrap();
        assert_eq!(metadata.base_type_name(1).as_deref(), Some("System.ValueType"));
        assert_eq!(metadata.base_type_name(2).as_deref(), Some("Point"));
        assert_eq!(metadata.interface_names(1), ["System.IComparable", "System.Collections.Generic.IList"]);
        assert!(metadata.interface_names(2).is_empty());
    }

    #[test]
    fn a_guid_is_formatted_like_dotnet_does() {
        let guid = [0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        assert_eq!(format_guid(&guid), "00112233-4455-6677-8899-aabbccddeeff");
    }

    #[test]
    fn metadata_without_a_module_row_has_no_mvid() {
        assert_eq!(sample().mvid(), None);
        assert_eq!(sample().pdb_id(), None);
    }

    #[test]
    fn truncated_metadata_is_an_error_not_a_panic() {
        let mut b = Builder::new();
        let name = b.string("Program");
        b.table(TYPE_DEF, &[&[0, 0, name, 0, 0, 1, 1]]);
        let full = b.build();
        for len in [0, 3, 20, full.len() / 2, full.len() - 1] {
            assert!(Metadata::parse(full[..len].to_vec()).is_err(), "len {len}");
        }
    }

    #[test]
    fn type_shapes_are_written_like_displayed_types() {
        assert_eq!(without_arity("System.Collections.Generic.Dictionary`2.Enumerator"), "System.Collections.Generic.Dictionary.Enumerator");
        assert_eq!(without_arity("System.String"), "System.String");
        let shape = TypeShape { name: "System.Collections.Generic.IEnumerable".into(), arguments: vec!["string".into()] };
        assert_eq!(shape.display(), "System.Collections.Generic.IEnumerable<string>");
        assert_eq!(TypeShape { name: "int[]".into(), arguments: Vec::new() }.display(), "int[]");
    }
    // Edit and Continue の差分（tests/fixtures/enc）: EncTarget.dll に、Roslyn の EmitDifference が書いた差分
    // EncTarget.1.meta（Compute と Main の更新、Helper の追加。ソースは Program.cs.txt → Program.1.cs.txt）を重ねる。
    fn enc_fixture() -> (Metadata, Metadata) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enc");
        let base = Metadata::from_pe(&std::fs::read(dir.join("EncTarget.dll")).unwrap()).unwrap();
        let mut updated = Metadata::from_pe(&std::fs::read(dir.join("EncTarget.dll")).unwrap()).unwrap();
        updated.apply_delta(std::fs::read(dir.join("EncTarget.1.meta")).unwrap()).unwrap();
        (base, updated)
    }

    #[test]
    fn a_delta_adds_the_new_method_to_its_type() {
        let (base, updated) = enc_fixture();
        let program = updated.find_type("Program").unwrap();
        assert!(base.find_method(program, "Helper", true, 1).is_none());
        let helper = updated.find_method(program, "Helper", true, 1).expect("足したメソッド");
        assert_eq!(updated.rows(METHOD_DEF), base.rows(METHOD_DEF) + 1);
        assert_eq!(updated.declaring_type(helper & 0x00FF_FFFF), Some(program));
        assert_eq!(updated.method_name(helper).as_deref(), Some("Program.Helper"));
        assert_eq!(updated.parameter_names(helper & 0x00FF_FFFF), ["x"]);
    }

    #[test]
    fn a_delta_keeps_the_existing_methods_and_the_module_identity() {
        let (base, updated) = enc_fixture();
        let program = updated.find_type("Program").unwrap();
        for name in ["Compute", "Main"] {
            assert_eq!(updated.methods_named(program, name).len(), 1, "{name}");
        }
        assert_eq!(updated.method_name(updated.find_method(program, "Compute", true, 1).unwrap()).as_deref(), Some("Program.Compute"));
        assert_eq!(updated.mvid(), base.mvid());
        assert_eq!(updated.find_type("Program"), base.find_type("Program"));
    }

    #[test]
    fn a_delta_that_is_not_one_is_refused_and_changes_nothing() {
        let (base, mut updated) = enc_fixture();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enc");
        let full = std::fs::read(dir.join("EncTarget.dll")).unwrap();
        let range = crate::debugger::target::metadata_range(&full).unwrap();
        let rows = updated.rows(METHOD_DEF);
        assert!(updated.apply_delta(full[range].to_vec()).is_err(), "ふつうのメタデータは差分ではない");
        assert_eq!(updated.rows(METHOD_DEF), rows);
        assert!(base.rows(METHOD_DEF) < rows);
    }
}
