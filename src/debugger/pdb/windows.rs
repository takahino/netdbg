//! Windows PDB（MSF 7.00。古い形式のプロジェクトの既定の `DebugType=full` / `pdbonly`）を読む（B7）。
//!
//! 自分でビルドし直せないベンダーの 32bit DLL や、古い Visual Studio の既定のままの exe は、この形の PDB を持つ。
//! Portable PDB と同じ表（ドキュメント、メソッドごとの系列点、ローカル変数のスコープ、using）を作り、以降は同じ口で
//! 使う（ブレークポイント、ステップ、変数）。読むもの:
//!
//! - MSF: スーパーブロック、ストリームのディレクトリ、ストリーム（ブロックの並び）。
//! - PDB 情報ストリーム（1）: GUID（exe の CodeView の RSDS と照らす）と、名前付きストリームの表（`/names`）。
//! - DBI ストリーム（3）: モジュールごとのシンボルのストリームと、その中の C13 の行の情報の大きさ。
//! - モジュールのシンボル: `S_GMANPROC` / `S_LMANPROC`（メソッドのトークンと、行の情報が指す番地）、`S_BLOCK32`
//!   （スコープ）、`S_MANSLOT`（ローカル変数のスロットと名前）、`S_UNAMESPACE`（using）、`S_END`。
//! - C13 の小区分: `DEBUG_S_FILECHKSMS`（ファイルの名前の位置）と `DEBUG_S_LINES`（IL オフセットと行）。
//!
//! - `S_OEM`: コンパイラの付加情報。`MD2`（C# のコンパイラの形）の、using をほかのメソッドに任せる印（多くのメソッドは、
//!   型の最初のメソッドの using を指す）と、`asyncMethodInfo`（async の MoveNext の、待つ位置と再開する位置）。
//!
//! マネージドの PDB では、メソッドの「番地」は、そのメソッドの `S_*MANPROC` の (セクション, オフセット) で、行の情報の
//! オフセットと、スコープの範囲は、そこからの IL オフセットになる。

use super::{Await, LocalScope, LocalVariable, Pdb, SequencePoint, HIDDEN_LINE};
use std::collections::{BTreeMap, HashMap};

/// MSF 7.00 のファイルの頭。
pub const MAGIC: &[u8] = b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0";

const S_END: u16 = 0x0006;
const S_OEM: u16 = 0x0404;
const S_BLOCK32: u16 = 0x1103;
const S_MANSLOT: u16 = 0x1120;
const S_UNAMESPACE: u16 = 0x1124;
const S_GMANPROC: u16 = 0x112A;
const S_LMANPROC: u16 = 0x112B;
const DEBUG_S_LINES: u32 = 0xF2;
const DEBUG_S_FILECHKSMS: u32 = 0xF4;
/// コンパイラが作ったコード（ステップで止まらない）の行番号。
const PDB_HIDDEN_LINE: u32 = 0x00FE_EFEE;

fn u16_at(data: &[u8], at: usize) -> Result<u16, String> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| tr!("Byte {at} is out of range", at = at))
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| tr!("Byte {at} is out of range", at = at))
}

/// NUL で終わる文字列（UTF-8）。
fn c_string(data: &[u8], at: usize) -> String {
    let rest = data.get(at..).unwrap_or(&[]);
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// MSF のストリームを読む口。
struct Msf<'a> {
    data: &'a [u8],
    block: usize,
    /// ストリームごとの大きさと、ブロックの番号の並び（無いストリームは `None`）。
    streams: Vec<Option<(usize, Vec<u32>)>>,
}

impl<'a> Msf<'a> {
    fn open(data: &'a [u8]) -> Result<Msf<'a>, String> {
        if !data.starts_with(MAGIC) {
            return Err(tr!("Not an MSF 7.00 header"));
        }
        let block = u32_at(data, 32)? as usize;
        if !matches!(block, 512 | 1024 | 2048 | 4096) {
            return Err(tr!("The block size is not valid ({size})", size = block));
        }
        let directory_bytes = u32_at(data, 44)? as usize;
        let map_block = u32_at(data, 52)? as usize;
        // ディレクトリのブロックの番号の並びは、`map_block` のブロックにある。
        let directory_blocks = directory_bytes.div_ceil(block);
        let map = data.get(map_block * block..).ok_or_else(|| tr!("The directory table is out of range"))?;
        let mut directory = Vec::with_capacity(directory_blocks * block);
        for i in 0..directory_blocks {
            let index = u32_at(map, i * 4)? as usize;
            directory.extend_from_slice(data.get(index * block..(index + 1) * block).ok_or_else(|| tr!("A directory block is out of range"))?);
        }
        directory.truncate(directory_bytes);
        let count = u32_at(&directory, 0)? as usize;
        if count > 100_000 {
            return Err(tr!("There are too many streams ({count})", count = count));
        }
        let mut at = 4 + count * 4;
        let mut streams = Vec::with_capacity(count);
        for i in 0..count {
            let size = u32_at(&directory, 4 + i * 4)?;
            if size == u32::MAX {
                streams.push(None);
                continue;
            }
            let blocks = (size as usize).div_ceil(block);
            let mut indices = Vec::with_capacity(blocks);
            for _ in 0..blocks {
                indices.push(u32_at(&directory, at)?);
                at += 4;
            }
            streams.push(Some((size as usize, indices)));
        }
        Ok(Msf { data, block, streams })
    }

    /// ストリーム `index` の中身。無ければ `None`。
    fn stream(&self, index: usize) -> Option<Vec<u8>> {
        let (size, blocks) = self.streams.get(index)?.as_ref()?;
        let mut out = Vec::with_capacity(blocks.len() * self.block);
        for &b in blocks {
            out.extend_from_slice(self.data.get(b as usize * self.block..(b as usize + 1) * self.block)?);
        }
        out.truncate(*size);
        Some(out)
    }
}

/// PDB 情報ストリームの、GUID と、名前付きストリーム `/names` の番号。
fn read_info(info: &[u8]) -> Result<([u8; 16], Option<usize>), String> {
    let guid: [u8; 16] = info.get(12..28).and_then(|g| g.try_into().ok()).ok_or_else(|| tr!("The PDB info stream is too short"))?;
    // 名前付きストリームの表: 名前の並び（長さつき）と、ハッシュ表（大きさ、容量、使っている所の印、消した所の印、
    // 使っている所ごとの (名前の位置, ストリームの番号)）。
    let names_length = u32_at(info, 28)? as usize;
    let names = info.get(32..32 + names_length).ok_or_else(|| tr!("The names of the named streams are out of range"))?;
    let mut at = 32 + names_length;
    let _size = u32_at(info, at)?;
    let capacity = u32_at(info, at + 4)? as usize;
    at += 8;
    let present_words = u32_at(info, at)? as usize;
    let mut present = Vec::with_capacity(present_words);
    for i in 0..present_words {
        present.push(u32_at(info, at + 4 + i * 4)?);
    }
    at += 4 + present_words * 4;
    let deleted_words = u32_at(info, at)? as usize;
    at += 4 + deleted_words * 4;
    let mut names_stream = None;
    for bucket in 0..capacity.min(present_words * 32) {
        if present[bucket / 32] & (1 << (bucket % 32)) == 0 {
            continue;
        }
        let (key, value) = (u32_at(info, at)? as usize, u32_at(info, at + 4)? as usize);
        at += 8;
        if c_string(names, key) == "/names" {
            names_stream = Some(value);
        }
    }
    Ok((guid, names_stream))
}

/// `/names` ストリームの文字列の並び（位置で引く）。
fn read_names(stream: &[u8]) -> Result<&[u8], String> {
    if u32_at(stream, 0)? != 0xEFFE_EFFE {
        return Err(tr!("The header of /names is not valid"));
    }
    let length = u32_at(stream, 8)? as usize;
    stream.get(12..12 + length).ok_or_else(|| tr!("A string of /names is out of range"))
}

/// DBI ストリームのモジュールの情報 1 つ。
struct ModuleInfo {
    stream: usize,
    symbols: usize,
    c11: usize,
    c13: usize,
}

fn read_modules(dbi: &[u8]) -> Result<Vec<ModuleInfo>, String> {
    if dbi.len() < 64 {
        return Err(tr!("The DBI stream is too short"));
    }
    let size = u32_at(dbi, 24)? as usize;
    let info = dbi.get(64..64 + size).ok_or_else(|| tr!("The module information is out of range"))?;
    let mut modules = Vec::new();
    let mut at = 0;
    while at + 64 <= info.len() {
        let stream = u16_at(info, at + 34)?;
        let module = ModuleInfo { stream: stream as usize, symbols: u32_at(info, at + 36)? as usize, c11: u32_at(info, at + 40)? as usize, c13: u32_at(info, at + 44)? as usize };
        // 名前と、obj の名前（どちらも NUL で終わる）のあと、4 バイトにそろえる。
        let mut end = at + 64;
        for _ in 0..2 {
            end += info.get(end..).and_then(|rest| rest.iter().position(|&b| b == 0)).ok_or_else(|| tr!("A module name does not end"))? + 1;
        }
        at = end.next_multiple_of(4);
        if stream != u16::MAX {
            modules.push(module);
        }
    }
    Ok(modules)
}

/// 読んでいる途中のスコープ（メソッドか、その中のブロック）。
struct OpenScope {
    range: std::ops::Range<u32>,
    variables: Vec<LocalVariable>,
}

/// 読んでいる途中のメソッド。
struct OpenMethod {
    row: u32,
    /// 行の情報とスコープが指す番地（セクション, オフセット）。
    address: (u16, u32),
    scopes: Vec<OpenScope>,
    closed: Vec<OpenScope>,
    namespaces: Vec<String>,
    /// using を任せた先のメソッドの行（付加情報の ForwardInfo）。
    forward: Option<u32>,
}

/// 読み終えた分。
#[derive(Default)]
struct Collected {
    documents: Vec<String>,
    document_index: HashMap<String, u32>,
    /// メソッドの行ごとの系列点。
    points: HashMap<u32, Vec<SequencePoint>>,
    scopes: Vec<LocalScope>,
    namespaces: Vec<Vec<String>>,
    /// 番地（セクション, オフセット）からメソッドの行へ。
    methods: HashMap<(u16, u32), u32>,
    /// using を自分で持つメソッドの行と、その名前空間の番号。
    own_imports: HashMap<u32, u32>,
    /// using を任せたメソッドの行と、任せた先。
    forwards: HashMap<u32, u32>,
    /// async の MoveNext の行ごとの、待つ位置と再開する位置。
    awaits: BTreeMap<u32, Vec<Await>>,
    /// もとの（async の）メソッドの行から、MoveNext の行へ（Portable PDB の StateMachineMethod と同じ向き）。
    state_machines: BTreeMap<u32, u32>,
}

impl Collected {
    fn document(&mut self, name: &str) -> u32 {
        if let Some(&index) = self.document_index.get(name) {
            return index;
        }
        self.documents.push(name.to_string());
        let index = self.documents.len() as u32;
        self.document_index.insert(name.to_string(), index);
        index
    }
}

/// モジュールのシンボルから、メソッド、スコープ、ローカル変数、using を読む。
fn read_symbols(symbols: &[u8], out: &mut Collected) -> Result<(), String> {
    let mut method: Option<OpenMethod> = None;
    // シンボルは頭の 4 バイト（署名）のあと。
    let mut at = 4;
    while at + 4 <= symbols.len() {
        let length = u16_at(symbols, at)? as usize;
        let kind = u16_at(symbols, at + 2)?;
        let data = symbols.get(at + 4..at + 2 + length).ok_or_else(|| tr!("A symbol is out of range"))?;
        at += 2 + length;
        match kind {
            S_GMANPROC | S_LMANPROC => {
                let token = u32_at(data, 24)?;
                let address = (u16_at(data, 32)?, u32_at(data, 28)?);
                let row = token & 0x00FF_FFFF;
                out.methods.insert(address, row);
                method = Some(OpenMethod { row, address, scopes: vec![OpenScope { range: 0..u32_at(data, 12)?, variables: Vec::new() }], closed: Vec::new(), namespaces: Vec::new(), forward: None });
            }
            S_BLOCK32 => {
                if let Some(m) = method.as_mut() {
                    let (start, length) = (u32_at(data, 12)?.wrapping_sub(m.address.1), u32_at(data, 8)?);
                    m.scopes.push(OpenScope { range: start..start.saturating_add(length), variables: Vec::new() });
                }
            }
            S_MANSLOT => {
                if let Some(scope) = method.as_mut().and_then(|m| m.scopes.last_mut()) {
                    scope.variables.push(LocalVariable { index: u32_at(data, 0)?, name: c_string(data, 16), tuple_names: Vec::new() });
                }
            }
            S_UNAMESPACE => {
                // C# のコンパイラは、using の種類を頭の 1 文字で書く（`U` は名前空間の using）。ほかの種類は読み飛ばす。
                if let (Some(m), Some(name)) = (method.as_mut(), c_string(data, 0).strip_prefix('U').map(String::from)) {
                    if !name.is_empty() {
                        m.namespaces.push(name);
                    }
                }
            }
            S_OEM => {
                if let Some(m) = method.as_mut() {
                    read_oem(data, m, out)?;
                }
            }
            S_END => {
                let Some(m) = method.as_mut() else { continue };
                if let Some(scope) = m.scopes.pop() {
                    m.closed.push(scope);
                }
                if m.scopes.is_empty() {
                    let m = method.take().expect("method");
                    finish_method(m, out);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// コンパイラの付加情報（`S_OEM`）: 名前（UTF-16）のあとに中身。
fn read_oem(data: &[u8], method: &mut OpenMethod, out: &mut Collected) -> Result<(), String> {
    let name: Vec<u16> = data.get(20..).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    let payload = data.get(20 + (name.len() + 1) * 2..).unwrap_or(&[]);
    match String::from_utf16_lossy(&name).as_str() {
        // C# のコンパイラの付加情報: 版（1 バイト）、数（1 バイト）、そろえ（2 バイト）のあとに、記録（版、種類、そろえ、
        // 大きさ（頭を含む）、中身）が並ぶ。種類 1（ForwardInfo）と 2（ForwardToModuleInfo）は、using を任せた先の
        // メソッドのトークン。
        "MD2" => {
            let mut at = 4;
            while at + 8 <= payload.len() {
                let kind = payload[at + 1];
                let size = u32_at(payload, at + 4)? as usize;
                if size < 8 {
                    break;
                }
                if matches!(kind, 1 | 2) {
                    method.forward = Some(u32_at(payload, at + 8)? & 0x00FF_FFFF);
                }
                at += size;
            }
        }
        // async の MoveNext: もとのメソッドのトークン、例外を受ける位置、数のあとに、(待つ位置, 再開する位置, 再開する
        // メソッドのトークン) が並ぶ（Portable PDB の AsyncMethodSteppingInformation と同じ中身）。
        "asyncMethodInfo" => {
            let kickoff = u32_at(payload, 0)? & 0x00FF_FFFF;
            let count = u32_at(payload, 8)? as usize;
            let mut awaits = Vec::with_capacity(count.min(1024));
            for i in 0..count {
                awaits.push(Await { yield_offset: u32_at(payload, 12 + i * 12)?, resume_offset: u32_at(payload, 16 + i * 12)? });
            }
            out.awaits.insert(method.row, awaits);
            out.state_machines.insert(kickoff, method.row);
        }
        _ => {}
    }
    Ok(())
}

/// 読み終えたメソッドのスコープを、表に足す。メソッド全体のスコープは、ローカル変数が無くても残す（using を持たせるため）。
fn finish_method(method: OpenMethod, out: &mut Collected) {
    if !method.namespaces.is_empty() {
        out.namespaces.push(method.namespaces);
        out.own_imports.insert(method.row, out.namespaces.len() as u32 - 1);
    }
    if let Some(target) = method.forward {
        out.forwards.insert(method.row, target);
    }
    let whole = method.closed.len().saturating_sub(1);
    for (index, scope) in method.closed.into_iter().enumerate() {
        if index == whole || !scope.variables.is_empty() {
            out.scopes.push(LocalScope { method: method.row, range: scope.range, variables: scope.variables, imports: 0 });
        }
    }
}

/// using の番号を、スコープに付ける（自分で持たなければ、任せた先をたどる）。
fn resolve_imports(out: &mut Collected) {
    let imports_of = |mut row: u32| -> u32 {
        for _ in 0..8 {
            if let Some(&index) = out.own_imports.get(&row) {
                return index;
            }
            match out.forwards.get(&row) {
                Some(&next) => row = next,
                None => break,
            }
        }
        0
    };
    let resolved: Vec<u32> = out.scopes.iter().map(|s| imports_of(s.method)).collect();
    for (scope, imports) in out.scopes.iter_mut().zip(resolved) {
        scope.imports = imports;
    }
}

/// C13 の行の情報から、メソッドごとの系列点を読む。行の情報のファイルの番号は、ファイルの名前の表
/// （`DEBUG_S_FILECHKSMS`）の中の位置なので、表を先に読んでから、行の小区分を読む。
fn read_lines(c13: &[u8], names: &[u8], out: &mut Collected) -> Result<(), String> {
    let mut files = HashMap::new();
    let mut sections = Vec::new();
    let mut at = 0;
    while at + 8 <= c13.len() {
        let (kind, length) = (u32_at(c13, at)?, u32_at(c13, at + 4)? as usize);
        let data = c13.get(at + 8..at + 8 + length).ok_or_else(|| tr!("A C13 subsection is out of range"))?;
        at = (at + 8 + length).next_multiple_of(4);
        match kind {
            DEBUG_S_FILECHKSMS => read_files(data, names, out, &mut files)?,
            DEBUG_S_LINES => sections.push(data),
            _ => {}
        }
    }
    for data in sections {
        read_line_section(data, &files, out)?;
    }
    Ok(())
}

/// ファイルの名前の表: 項目ごとに、`/names` の中の名前の位置、チェックサムの大きさと種類、チェックサム（4 バイトに
/// そろえる）。表の中の位置から、ドキュメントの番号へ。
fn read_files(data: &[u8], names: &[u8], out: &mut Collected, files: &mut HashMap<u32, u32>) -> Result<(), String> {
    let mut offset = 0;
    while offset + 6 <= data.len() {
        let name = c_string(names, u32_at(data, offset)? as usize);
        let checksum = data[offset + 4] as usize;
        files.insert(offset as u32, out.document(&name));
        offset = (offset + 6 + checksum).next_multiple_of(4);
    }
    Ok(())
}

/// 行の小区分 1 つ（1 つのメソッドの行）: 頭（番地、印、大きさ）のあとに、ファイルごとの塊（ファイル、数、塊の
/// 大きさ、(IL オフセット, 行) の並び、印が立っていれば列の並び）。
fn read_line_section(data: &[u8], files: &HashMap<u32, u32>, out: &mut Collected) -> Result<(), String> {
    let address = (u16_at(data, 4)?, u32_at(data, 0)?);
    let columns = u16_at(data, 6)? & 1 != 0;
    let Some(&row) = out.methods.get(&address) else { return Ok(()) };
    let mut points = Vec::new();
    let mut at = 12;
    while at + 12 <= data.len() {
        let document = *files.get(&u32_at(data, at)?).ok_or_else(|| tr!("A file of the line information is not in the names table"))?;
        let count = u32_at(data, at + 4)? as usize;
        let block = u32_at(data, at + 8)? as usize;
        for i in 0..count {
            let il_offset = u32_at(data, at + 12 + i * 8)?;
            let info = u32_at(data, at + 16 + i * 8)?;
            let start_line = info & 0x00FF_FFFF;
            if start_line == PDB_HIDDEN_LINE {
                points.push(SequencePoint { il_offset, document, start_line: HIDDEN_LINE, start_column: 0, end_line: HIDDEN_LINE, end_column: 0 });
                continue;
            }
            let (start_column, end_column) = if columns {
                let column = at + 12 + count * 8 + i * 4;
                (u16_at(data, column)? as u32, u16_at(data, column + 2)? as u32)
            } else {
                (0, 0)
            };
            points.push(SequencePoint { il_offset, document, start_line, start_column, end_line: start_line + ((info >> 24) & 0x7F), end_column });
        }
        at += block.max(12);
    }
    out.points.entry(row).or_default().extend(points);
    Ok(())
}
/// Windows PDB を読み、Portable PDB と同じ表にする。GUID は、exe の CodeView と照らすために返す。
pub fn parse(data: &[u8]) -> Result<(Pdb, [u8; 16]), String> {
    let msf = Msf::open(data)?;
    let info = msf.stream(1).ok_or_else(|| tr!("There is no PDB info stream"))?;
    let (guid, names_stream) = read_info(&info)?;
    let names_data = names_stream.and_then(|s| msf.stream(s)).ok_or_else(|| tr!("There is no /names stream"))?;
    let names = read_names(&names_data)?;
    let dbi = msf.stream(3).ok_or_else(|| tr!("There is no DBI stream"))?;
    let mut out = Collected { namespaces: vec![Vec::new()], ..Collected::default() };
    for module in read_modules(&dbi)? {
        let Some(stream) = msf.stream(module.stream) else { continue };
        let symbols = stream.get(..module.symbols).ok_or_else(|| tr!("A module's symbols are out of range"))?;
        read_symbols(symbols, &mut out)?;
        let c13_start = module.symbols + module.c11;
        if let Some(c13) = stream.get(c13_start..c13_start + module.c13) {
            read_lines(c13, names, &mut out)?;
        }
    }
    if out.points.is_empty() {
        // ネイティブのコードの PDB など、マネージドのメソッドが 1 つも無いもの。
        return Err(tr!("There is no line information for managed methods"));
    }
    resolve_imports(&mut out);
    let rows = out.points.keys().copied().max().unwrap_or(0) as usize;
    let mut methods = vec![Vec::new(); rows];
    for (row, mut points) in out.points {
        points.sort_by_key(|p| p.il_offset);
        methods[row as usize - 1] = points;
    }
    // LocalScope はメソッドの行の順に並べる（`scopes` の引き方がそれを前提にしている）。
    out.scopes.sort_by_key(|s| (s.method, s.range.start));
    let pdb = Pdb { documents: out.documents, methods, scopes: out.scopes, namespaces: out.namespaces, awaits: out.awaits, state_machines: out.state_machines, ..Pdb::default() };
    Ok((pdb, guid))
}


#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 512;

    /// ストリームを並べて、MSF 7.00 のファイルを組み立てる（ブロック 0 がスーパーブロック。ストリーム、ディレクトリ、
    /// ディレクトリのブロックの表の順に、ブロックを続けて使う）。
    fn msf(streams: &[Vec<u8>]) -> Vec<u8> {
        let mut blocks: Vec<Vec<u8>> = vec![Vec::new()];
        let mut directory = (streams.len() as u32).to_le_bytes().to_vec();
        for s in streams {
            directory.extend((s.len() as u32).to_le_bytes());
        }
        for s in streams {
            for chunk in s.chunks(BLOCK) {
                directory.extend((blocks.len() as u32).to_le_bytes());
                blocks.push(chunk.to_vec());
            }
        }
        let mut map = Vec::new();
        for chunk in directory.chunks(BLOCK) {
            map.extend((blocks.len() as u32).to_le_bytes());
            blocks.push(chunk.to_vec());
        }
        let map_block = blocks.len() as u32;
        blocks.push(map);
        let mut superblock = MAGIC.to_vec();
        for value in [BLOCK as u32, 1, blocks.len() as u32 + 1, directory.len() as u32, 0, map_block] {
            superblock.extend(value.to_le_bytes());
        }
        blocks[0] = superblock;
        blocks.into_iter().flat_map(|mut b| {
            b.resize(BLOCK, 0);
            b
        }).collect()
    }

    fn symbol(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut out = ((data.len() + 2) as u16).to_le_bytes().to_vec();
        out.extend(kind.to_le_bytes());
        out.extend(data);
        out
    }

    fn le(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// `S_GMANPROC`（トークン、番地、長さ、名前）。
    fn proc(token: u32, offset: u32, length: u32) -> Vec<u8> {
        let mut data = le(&[0, 0, 0, length, 0, length, token, offset]);
        data.extend(1u16.to_le_bytes());
        data.push(0);
        data.extend(0u16.to_le_bytes());
        data.extend(b"M\0");
        symbol(S_GMANPROC, &data)
    }

    fn oem(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 16];
        data.extend(le(&[0]));
        data.extend(name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        data.extend(payload);
        symbol(S_OEM, &data)
    }

    /// 1 つのファイル、2 つのメソッド（2 番目は using を 1 番目に任せ、async の MoveNext）の PDB。
    fn sample() -> Vec<u8> {
        let mut symbols = le(&[4]);
        symbols.extend(proc(0x0600_0001, 0x100, 20));
        let mut slot = le(&[0, 0, 0]);
        slot.extend([0, 0, 0, 0]);
        slot.extend(b"count\0");
        symbols.extend(symbol(S_MANSLOT, &slot));
        symbols.extend(symbol(S_UNAMESPACE, b"USystem.Linq\0"));
        symbols.extend(symbol(S_END, &[]));
        symbols.extend(proc(0x0600_0002, 0x200, 40));
        // MD2: 版 4、数 1、記録（版 4、種類 1 = ForwardInfo、大きさ 12、トークン）。
        let mut md2 = vec![4, 1, 0, 0, 4, 1, 0, 0];
        md2.extend(le(&[12, 0x0600_0001]));
        symbols.extend(oem("MD2", &md2));
        // asyncMethodInfo: もとのメソッド 0x06000003、例外を受ける位置、数 1、(待つ 8, 再開 12, MoveNext)。
        symbols.extend(oem("asyncMethodInfo", &le(&[0x0600_0003, 0xFFFF_FFFF, 1, 8, 12, 0x0600_0002])));
        symbols.extend(symbol(S_END, &[]));

        let names = b"\0C:\\app\\Program.cs\0".to_vec();
        let mut checksums = le(&[1]);
        checksums.extend([0, 0, 0, 0]);
        let lines = |offset: u32, points: &[(u32, u32)]| {
            let mut data = le(&[offset]);
            data.extend(1u16.to_le_bytes());
            data.extend(0u16.to_le_bytes());
            data.extend(le(&[64, 0, points.len() as u32, 12 + points.len() as u32 * 8]));
            for &(il, line) in points {
                data.extend(le(&[il, line | 0x8000_0000]));
            }
            data
        };
        let mut c13 = Vec::new();
        for (kind, data) in [(DEBUG_S_FILECHKSMS, checksums), (DEBUG_S_LINES, lines(0x100, &[(0, 10), (5, PDB_HIDDEN_LINE), (7, 11)])), (DEBUG_S_LINES, lines(0x200, &[(0, 20)]))] {
            c13.extend(le(&[kind, data.len() as u32]));
            c13.extend(&data);
            c13.resize(c13.len().next_multiple_of(4), 0);
        }
        let mut module = symbols.clone();
        module.extend(&c13);

        let mut names_stream = le(&[0xEFFE_EFFE, 1, names.len() as u32]);
        names_stream.extend(&names);

        // 名前付きストリームの表: "/names" → 4。
        let mut info = le(&[20000404, 0, 1]);
        info.extend([9u8; 16]);
        let map_names = b"/names\0";
        info.extend(le(&[map_names.len() as u32]));
        info.extend(map_names);
        info.extend(le(&[1, 1, 1, 1, 0, 0, 4]));

        let mut dbi = vec![0u8; 64];
        let mut entry = vec![0u8; 64];
        entry[34..36].copy_from_slice(&5u16.to_le_bytes());
        entry[36..40].copy_from_slice(&(symbols.len() as u32).to_le_bytes());
        entry[44..48].copy_from_slice(&(c13.len() as u32).to_le_bytes());
        entry.extend(b"m\0o\0");
        entry.resize(entry.len().next_multiple_of(4), 0);
        dbi[24..28].copy_from_slice(&(entry.len() as u32).to_le_bytes());
        dbi.extend(entry);

        msf(&[Vec::new(), info, Vec::new(), dbi, names_stream, module])
    }

    #[test]
    fn a_managed_windows_pdb_becomes_the_same_tables() {
        let (pdb, guid) = parse(&sample()).unwrap();
        assert_eq!(guid, [9; 16]);
        assert_eq!(pdb.documents, vec!["C:\\app\\Program.cs".to_string()]);
        let points: Vec<(u32, u32)> = pdb.methods[0].iter().map(|p| (p.il_offset, p.start_line)).collect();
        assert_eq!(points, vec![(0, 10), (5, HIDDEN_LINE), (7, 11)]);
        assert_eq!(pdb.resolve_line(1, 11).map(|l| (l.method, l.il_offset)), Some((0x0600_0001, 7)));
        let locals: Vec<String> = pdb.locals(0x0600_0001, 3, super::super::LATEST).iter().map(|v| v.name.clone()).collect();
        assert_eq!(locals, vec!["count".to_string()]);
        // 2 番目のメソッドは、using を 1 番目に任せている。
        assert_eq!(pdb.namespaces(0x0600_0002, super::super::LATEST), ["System.Linq".to_string()]);
        assert_eq!(pdb.awaits(0x0600_0002, super::super::LATEST).iter().map(|a| (a.yield_offset, a.resume_offset)).collect::<Vec<_>>(), vec![(8, 12)]);
        assert_eq!(pdb.state_machine_start(0x0600_0003).map(|(method, _)| method), Some(0x0600_0002));
    }

    #[test]
    fn a_broken_or_native_pdb_is_refused_with_the_reason() {
        assert!(parse(b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0").is_err());
        let mut broken = sample();
        broken.truncate(BLOCK * 2);
        assert!(parse(&broken).is_err());
    }
}
