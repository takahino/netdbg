//! デバッグ対象のビット数の判定。
//!
//! ICorDebug は、デバッガーと対象のビット数が同じでないと動かない。しかも合わないまま起動すると、
//! x64 のエンジンは x86 の対象をデバッグ無しで走らせてしまう（spike で観測）。そのため起動の前に
//! 実行ファイルを調べ、合わなければ対象を走らせずにエラーにする。

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bits {
    X86,
    X64,
}

impl Bits {
    pub fn of_this_process() -> Bits {
        if cfg!(target_pointer_width = "64") {
            Bits::X64
        } else {
            Bits::X86
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Bits::X86 => "x86",
            Bits::X64 => "x64",
        }
    }
}

/// 実行ファイルの種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Executable {
    /// 64bit の Windows で起動したときのビット数。
    pub bits: Bits,
    /// CLR ヘッダーを持つ（.NET Framework の exe、または .NET の dll）。持たなければネイティブ（apphost など）。
    pub managed: bool,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// PE ファイルのヘッダー。
struct Pe<'a> {
    data: &'a [u8],
    /// PE ヘッダー（`PE\0\0`）の位置。COFF ヘッダーはその 4 バイトあと、オプション ヘッダーは 24 バイトあと。
    header: usize,
    machine: u16,
    pe32_plus: bool,
    /// データディレクトリの先頭。
    directories: usize,
    /// セクション表の先頭と数。
    sections: usize,
    section_count: usize,
}

impl<'a> Pe<'a> {
    fn parse(data: &'a [u8]) -> Option<Pe<'a>> {
        if data.get(..2)? != b"MZ" {
            return None;
        }
        let pe = u32_at(data, 0x3C)? as usize;
        if data.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
            return None;
        }
        let optional = pe + 24;
        let pe32_plus = match u16_at(data, optional)? {
            0x10B => false,
            0x20B => true,
            _ => return None,
        };
        Some(Pe {
            data,
            header: pe,
            machine: u16_at(data, pe + 4)?,
            pe32_plus,
            directories: optional + if pe32_plus { 112 } else { 96 },
            sections: optional + u16_at(data, pe + 20)? as usize,
            section_count: u16_at(data, pe + 6)? as usize,
        })
    }

    /// RVA（読み込まれたときの位置）を、ファイル上の位置に直す。
    fn offset(&self, rva: u32) -> Option<usize> {
        for i in 0..self.section_count {
            let s = self.sections + i * 40;
            let (virtual_size, virtual_address) = (u32_at(self.data, s + 8)?, u32_at(self.data, s + 12)?);
            let (raw_size, raw_pointer) = (u32_at(self.data, s + 16)?, u32_at(self.data, s + 20)?);
            if rva >= virtual_address && rva - virtual_address < virtual_size.max(raw_size) {
                return Some((rva - virtual_address) as usize + raw_pointer as usize);
            }
        }
        None
    }

    /// CLR ヘッダーのファイル上の位置（データディレクトリ 14 番）。ネイティブの PE なら `None`。
    fn clr_header(&self) -> Option<usize> {
        match u32_at(self.data, self.directories + 14 * 8) {
            Some(rva) if rva != 0 => self.offset(rva),
            _ => None,
        }
    }
}

/// PE の識別（ヘッダーの TimeDateStamp と SizeOfImage）。ランタイムが、自分に合う mscordbi.dll / mscordaccore.dll を
/// この 2 つで指定してくる。`data` はファイルの先頭（ヘッダーの分）だけでよい。
pub fn image_identity(data: &[u8]) -> Option<(u32, u32)> {
    let pe = Pe::parse(data)?;
    let header = pe.header;
    // TimeDateStamp は COFF ヘッダーの 4 バイト目から。SizeOfImage は、オプションヘッダーの 56 バイト目から（PE32 / PE32+ 共通）。
    Some((u32_at(data, header + 8)?, u32_at(data, header + 24 + 56).filter(|_| pe.data.len() > header + 24 + 60)?))
}

/// 対象のメモリから読んだモジュールの像を、ファイルの並びにする。読み込まれた像は、セクションが RVA の位置にある
/// （ファイルでは PointerToRawData の位置）。もとからファイルの並び（単一ファイルのアプリが、そのまま写したもの）なら
/// そのまま返す。どちらの並びかは、メタデータの `BSJB` がどちらの位置にあるかで決める。
pub fn file_layout(image: &[u8]) -> Option<Vec<u8>> {
    let has_metadata = |data: &[u8]| metadata_range(data).is_some_and(|range| data.get(range.start..range.start + 4) == Some(b"BSJB"));
    if has_metadata(image) {
        return Some(image.to_vec());
    }
    let pe = Pe::parse(image)?;
    let size_of_headers = u32_at(image, pe.header + 24 + 60)? as usize;
    let mut sections = Vec::new();
    for i in 0..pe.section_count {
        let s = pe.sections + i * 40;
        let (virtual_address, raw_size, raw_pointer) = (u32_at(image, s + 12)? as usize, u32_at(image, s + 16)? as usize, u32_at(image, s + 20)? as usize);
        sections.push((virtual_address, raw_size, raw_pointer));
    }
    // 壊れた値で巨大な領域を取らないよう、ファイルの大きさは読んだ像の大きさまでに限る。
    let size = sections.iter().map(|&(_, raw_size, raw_pointer)| raw_pointer.saturating_add(raw_size)).max()?.max(size_of_headers);
    if size > image.len().saturating_mul(2) {
        return None;
    }
    let mut file = vec![0u8; size];
    let headers = size_of_headers.min(image.len());
    file[..headers].copy_from_slice(&image[..headers]);
    for (virtual_address, raw_size, raw_pointer) in sections {
        let length = raw_size.min(image.len().saturating_sub(virtual_address));
        if length > 0 {
            file.get_mut(raw_pointer..raw_pointer + length)?.copy_from_slice(image.get(virtual_address..virtual_address + length)?);
        }
    }
    has_metadata(&file).then_some(file)
}

/// PE ファイルの、RVA（読み込まれたときの位置）に当たるファイル上の位置。メソッドの本体を読むのに使う。
pub fn file_offset(data: &[u8], rva: u32) -> Option<usize> {
    Pe::parse(data)?.offset(rva)
}

/// アセンブリの中の、メタデータ（`BSJB` で始まる部分）の範囲。マネージドの PE でなければ `None`。
pub fn metadata_range(data: &[u8]) -> Option<std::ops::Range<usize>> {
    let pe = Pe::parse(data)?;
    let clr = pe.clr_header()?;
    let start = pe.offset(u32_at(data, clr + 8)?)?;
    let end = start.checked_add(u32_at(data, clr + 12)? as usize)?;
    (end <= data.len()).then_some(start..end)
}

/// アセンブリが覚えている、自分の PDB の ID（デバッグ ディレクトリの CodeView の項目）。
#[derive(Debug, Clone, PartialEq)]
pub struct CodeView {
    pub guid: [u8; 16],
    /// 項目の日時印。Portable PDB では PDB の ID の後ろ 4 バイトと同じ値が入る。
    pub stamp: u32,
}

const DEBUG_CODEVIEW: u32 = 2;
pub const DEBUG_EMBEDDED_PDB: u32 = 17;

/// デバッグ ディレクトリの項目。
pub struct DebugEntry {
    pub kind: u32,
    pub stamp: u32,
    pub size: usize,
    /// 中身のファイル上の位置。
    pub pointer: usize,
}

/// デバッグ ディレクトリの表の、ファイル上の位置と大きさ。`data` はファイルの先頭からヘッダーまで（セクションの表を
/// 含む）あればよい。デバッグ ディレクトリが無ければ `None`。
pub fn debug_directory(data: &[u8]) -> Option<(usize, usize)> {
    const DEBUG_DIRECTORY: usize = 6;
    let pe = Pe::parse(data)?;
    let rva = u32_at(data, pe.directories + DEBUG_DIRECTORY * 8)?;
    let size = u32_at(data, pe.directories + DEBUG_DIRECTORY * 8 + 4)? as usize;
    if rva == 0 {
        return None;
    }
    Some((pe.offset(rva)?, size))
}

/// デバッグ ディレクトリの表（`debug_directory` の範囲のバイト列）の項目を並べる。
pub fn debug_entries(table: &[u8]) -> impl Iterator<Item = DebugEntry> + '_ {
    // 項目は 28 バイト: Characteristics, TimeDateStamp, 版, Type, SizeOfData, AddressOfRawData, PointerToRawData。
    table.chunks_exact(28).filter_map(|entry| {
        Some(DebugEntry { kind: u32_at(entry, 12)?, stamp: u32_at(entry, 4)?, size: u32_at(entry, 16)? as usize, pointer: u32_at(entry, 24)? as usize })
    })
}

/// アセンブリ全体から、デバッグ ディレクトリの項目を並べる。
fn debug_entries_of(data: &[u8]) -> Option<impl Iterator<Item = DebugEntry> + '_> {
    let (table, size) = debug_directory(data)?;
    Some(debug_entries(data.get(table..table.checked_add(size)?)?))
}

/// PDB の ID を持たない（PDB 無しでビルドした）アセンブリ、PE でないデータは `None`。
pub fn codeview(data: &[u8]) -> Option<CodeView> {
    for entry in debug_entries_of(data)?.filter(|e| e.kind == DEBUG_CODEVIEW) {
        let raw = entry.pointer;
        // 中身は "RSDS"、GUID、age、PDB のパス。
        if data.get(raw..raw.checked_add(4)?)? == b"RSDS" {
            return Some(CodeView { guid: data.get(raw + 4..raw + 20)?.try_into().ok()?, stamp: entry.stamp });
        }
    }
    None
}

/// アセンブリに埋め込み PDB があるか。ファイル全体は読まず、ヘッダーとデバッグ ディレクトリの表だけを読む
/// （どのモジュールにも尋ねるので、ランタイムの大きなモジュールを丸ごと読まないため）。
pub fn has_embedded_pdb(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    // ヘッダー（セクションの表まで）は、ふつう 512 バイト。ここに収まらない PE は、埋め込みが無いものと同じに扱う。
    const HEADERS: u64 = 4096;
    // 表の項目はふつう 4 つほど。壊れた大きさで大量に読まないよう、上限を置く。
    const TABLE_LIMIT: usize = 28 * 64;
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut headers = Vec::new();
    if file.by_ref().take(HEADERS).read_to_end(&mut headers).is_err() {
        return false;
    }
    let Some((table, size)) = debug_directory(&headers) else { return false };
    let mut entries = Vec::new();
    if file.seek(SeekFrom::Start(table as u64)).is_err() || file.take(size.min(TABLE_LIMIT) as u64).read_to_end(&mut entries).is_err() {
        return false;
    }
    let found = debug_entries(&entries).any(|e| e.kind == DEBUG_EMBEDDED_PDB);
    found
}

/// 埋め込み PDB の項目の中身（"MPDB"、展開後の大きさ、Deflate で圧縮した Portable PDB）。無ければ `None`。
pub fn embedded_pdb(data: &[u8]) -> Option<&[u8]> {
    let entry = debug_entries_of(data)?.find(|e| e.kind == DEBUG_EMBEDDED_PDB)?;
    data.get(entry.pointer..entry.pointer.checked_add(entry.size)?)
}

/// アセンブリの入口のメソッド（`Main`）の MethodDef トークン。入口を持たない（ライブラリ）なら `None`。
pub fn entry_point(data: &[u8]) -> Option<u32> {
    const NATIVE_ENTRYPOINT: u32 = 0x10;
    let clr = Pe::parse(data)?.clr_header()?;
    let token = u32_at(data, clr + 20)?;
    (u32_at(data, clr + 16)? & NATIVE_ENTRYPOINT == 0 && token >> 24 == 0x06).then_some(token)
}

/// PE ヘッダーと CLR ヘッダーを読む。PE でなければ `None`。
pub fn inspect(data: &[u8]) -> Option<Executable> {
    let pe = Pe::parse(data)?;
    let native_bits = match pe.machine {
        0x014C => Bits::X86,
        0x8664 => Bits::X64,
        _ => return None, // ARM64 などは対象外
    };
    if u32_at(data, pe.directories + 14 * 8).unwrap_or(0) == 0 {
        return Some(Executable { bits: native_bits, managed: false });
    }

    const ILONLY: u32 = 0x1;
    const REQUIRED_32BIT: u32 = 0x2;
    const PREFERRED_32BIT: u32 = 0x2_0000;
    let flags = u32_at(data, pe.clr_header()? + 16)?;
    let pe32_plus = pe.pe32_plus;
    let bits = if pe32_plus {
        Bits::X64
    } else if flags & ILONLY != 0 && flags & (REQUIRED_32BIT | PREFERRED_32BIT) == 0 {
        // AnyCPU: 64bit の Windows では 64bit で動く。
        Bits::X64
    } else {
        Bits::X86
    };
    Some(Executable { bits, managed: true })
}

/// このエンジンと同じビット数の .NET のインストール先の候補（環境変数 `DOTNET_ROOT(x86)` / `DOTNET_ROOT` と、既定の
/// 置き場所。x86 は `Program Files (x86)\dotnet`、x64 は `Program Files\dotnet`）。あるかどうかは確かめない。
pub fn dotnet_roots() -> Vec<std::path::PathBuf> {
    let mine = Bits::of_this_process();
    let root_variables: &[&str] = match mine {
        Bits::X86 => &["DOTNET_ROOT(x86)", "DOTNET_ROOT_X86", "DOTNET_ROOT"],
        Bits::X64 => &["DOTNET_ROOT_X64", "DOTNET_ROOT"],
    };
    let mut roots: Vec<std::path::PathBuf> = root_variables.iter().filter_map(std::env::var_os).map(std::path::PathBuf::from).collect();
    let program_files = match mine {
        Bits::X86 => std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles")),
        Bits::X64 => std::env::var_os("ProgramW6432").or_else(|| std::env::var_os("ProgramFiles")),
    };
    roots.extend(program_files.map(|folder| Path::new(&folder).join("dotnet")));
    roots
}

/// 起動しようとしている実行ファイルが、このエンジンで扱えるビット数かを確かめる。
/// このエンジンと同じビット数の `dotnet.exe`（dll を起動するのに使う）。探す順: 環境変数 `DOTNET_ROOT(x86)` / `DOTNET_ROOT`、
/// 既定の置き場所（x86 は `Program Files (x86)\dotnet`、x64 は `Program Files\dotnet`）、`PATH`。ビット数の違うものは使わない。
pub fn dotnet_host() -> Result<String, String> {
    let mine = Bits::of_this_process();
    let mut candidates: Vec<std::path::PathBuf> = dotnet_roots().into_iter().map(|root| root.join("dotnet.exe")).collect();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join("dotnet.exe")));
    }
    candidates
        .into_iter()
        .find(|candidate| std::fs::read(candidate).ok().and_then(|data| inspect(&data)).is_some_and(|host| host.bits == mine))
        .map(|host| host.to_string_lossy().into_owned())
        .ok_or_else(|| tr!("The {bits} dotnet.exe to launch the program dll is not found. Install the {bits} .NET, or set an executable (.exe) as program.", bits = mine.name()))
}

pub fn check_launch(program: &Path) -> Result<Executable, String> {
    let data = std::fs::read(program).map_err(|e| tr!("Cannot read program: {path}: {error}. Check the path and the build output.", path = program.display(), error = e))?;
    let executable = inspect(&data).ok_or_else(|| tr!("program is not an x86 / x64 executable: {path}", path = program.display()))?;
    let mine = Bits::of_this_process();
    if executable.bits != mine {
        return Err(tr!(
            "{program} runs as {bits}, but this engine is {engine}. Launch it with the {bits} netdbg.exe.",
            program = program.display(),
            bits = executable.bits.name(),
            engine = mine.name()
        ));
    }
    Ok(executable)
}

/// 実行中のプロセスのビット数。調べられなければ（権限が無い、もう終わっている）エラー。
pub fn bits_of_process(process_id: u32) -> Result<Bits, String> {
    use crate::win32::{open_process_for_query, GetCurrentProcess, GetLastError, IsWow64Process};
    unsafe {
        let Some(process) = open_process_for_query(process_id) else {
            return Err(tr!(
                "Cannot open process {pid} (Win32 error {code}). Check that the number is right and that it runs as the same user.",
                pid = process_id,
                code = GetLastError()
            ));
        };
        let (mut target_is_wow64, mut engine_is_wow64) = (0, 0);
        let ok = IsWow64Process(process.raw(), &mut target_is_wow64) != 0 && IsWow64Process(GetCurrentProcess(), &mut engine_is_wow64) != 0;
        if !ok {
            return Err(tr!("Cannot tell the bitness of process {pid} (Win32 error {code}).", pid = process_id, code = GetLastError()));
        }
        // WOW64 で動いているのは、64bit の Windows の上の 32bit のプロセス。どちらも WOW64 でなければ、エンジンと同じビット数。
        Ok(if target_is_wow64 != 0 {
            Bits::X86
        } else if engine_is_wow64 != 0 {
            Bits::X64
        } else {
            Bits::of_this_process()
        })
    }
}

/// 実行中のプロセスの exe のフルパス。調べられなければ（権限が無い、もう終わっている）`None`。
pub fn process_image_path(process_id: u32) -> Option<String> {
    use crate::win32::{open_process_for_query, QueryFullProcessImageNameW};
    let process = open_process_for_query(process_id)?;
    let mut name = [0u16; 32768];
    let mut size = name.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process.raw(), 0, name.as_mut_ptr(), &mut size) } != 0;
    ok.then(|| String::from_utf16_lossy(&name[..size as usize]))
}

/// プロセスの最初のスレッド（プロセスを作ったときのスレッド。`Main` を実行する）の番号。スレッドのうち作られたのが
/// いちばん早いもの。それがプロセスと同時に作られていなければ（最初のスレッドがもう終わっている）`None`。
pub fn primary_thread(process_id: u32) -> Option<u32> {
    use crate::win32::*;
    // プロセスを作ってから最初のスレッドが動き出すまでの間（100 ナノ秒単位で 1 秒）。
    const SAME_TIME: u64 = 10_000_000;
    unsafe {
        let process = open_process_for_query(process_id)?;
        let (mut process_created, mut ignored) = (0u64, [0u64; 3]);
        if GetProcessTimes(process.raw(), &mut process_created, &mut ignored[0], &mut ignored[1], &mut ignored[2]) == 0 {
            return None;
        }
        let snapshot = OwnedHandle::new(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0))?;
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut earliest: Option<(u64, u32)> = None;
        let mut more = Thread32First(snapshot.raw(), &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == process_id {
                if let Some(thread) = OwnedHandle::new(OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, entry.th32ThreadID)) {
                    let mut created = 0u64;
                    if GetThreadTimes(thread.raw(), &mut created, &mut ignored[0], &mut ignored[1], &mut ignored[2]) != 0 && earliest.is_none_or(|(time, _)| created < time) {
                        earliest = Some((created, entry.th32ThreadID));
                    }
                }
            }
            more = Thread32Next(snapshot.raw(), &mut entry) != 0;
        }
        earliest.filter(|(created, _)| created.saturating_sub(process_created) < SAME_TIME).map(|(_, id)| id)
    }
}

/// アタッチしようとしているプロセスが、このエンジンで扱えるビット数かを確かめる。
pub fn check_attach(process_id: u32) -> Result<(), String> {
    let (target, mine) = (bits_of_process(process_id)?, Bits::of_this_process());
    if target != mine {
        return Err(tr!(
            "Process {pid} runs as {bits}, but this engine is {engine}. Attach with the {bits} netdbg.exe.",
            pid = process_id,
            bits = target.name(),
            engine = mine.name()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小限の PE を組み立てる。`clr_flags` が `Some` なら CLR ヘッダーを付ける。
    fn pe(machine: u16, pe32_plus: bool, clr_flags: Option<u32>) -> Vec<u8> {
        let mut data = vec![0u8; 0x400];
        data[..2].copy_from_slice(b"MZ");
        data[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80;
        data[pe..pe + 4].copy_from_slice(b"PE\0\0");
        data[pe + 4..pe + 6].copy_from_slice(&machine.to_le_bytes());
        data[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes());
        let optional_size: u16 = if pe32_plus { 240 } else { 224 };
        data[pe + 20..pe + 22].copy_from_slice(&optional_size.to_le_bytes());
        let optional = pe + 24;
        data[optional..optional + 2].copy_from_slice(&(if pe32_plus { 0x20Bu16 } else { 0x10B }).to_le_bytes());
        let section = optional + optional_size as usize;
        // .text: RVA 0x2000、ファイル上は 0x200 から。
        data[section + 8..section + 12].copy_from_slice(&0x200u32.to_le_bytes());
        data[section + 12..section + 16].copy_from_slice(&0x2000u32.to_le_bytes());
        data[section + 16..section + 20].copy_from_slice(&0x200u32.to_le_bytes());
        data[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
        if let Some(flags) = clr_flags {
            let directories = optional + if pe32_plus { 112 } else { 96 };
            data[directories + 14 * 8..directories + 14 * 8 + 4].copy_from_slice(&0x2008u32.to_le_bytes());
            data[0x208 + 16..0x208 + 20].copy_from_slice(&flags.to_le_bytes());
        }
        data
    }

    #[test]
    fn a_native_exe_has_the_bitness_of_its_machine() {
        assert_eq!(inspect(&pe(0x014C, false, None)), Some(Executable { bits: Bits::X86, managed: false }));
        assert_eq!(inspect(&pe(0x8664, true, None)), Some(Executable { bits: Bits::X64, managed: false }));
    }

    #[test]
    fn a_managed_exe_follows_its_clr_flags() {
        let x86 = Some(Executable { bits: Bits::X86, managed: true });
        let x64 = Some(Executable { bits: Bits::X64, managed: true });
        assert_eq!(inspect(&pe(0x014C, false, Some(0x1 | 0x2))), x86, "x86 指定");
        assert_eq!(inspect(&pe(0x014C, false, Some(0x1 | 0x2 | 0x2_0000))), x86, "AnyCPU、32 ビットを優先");
        assert_eq!(inspect(&pe(0x014C, false, Some(0x1))), x64, "AnyCPU");
        assert_eq!(inspect(&pe(0x8664, true, Some(0x1))), x64, "x64 指定");
    }

    #[test]
    fn the_entry_point_is_a_method_token_of_a_managed_exe() {
        let mut exe = pe(0x014C, false, Some(0x1));
        exe[0x208 + 20..0x208 + 24].copy_from_slice(&0x0600_0003u32.to_le_bytes());
        assert_eq!(entry_point(&exe), Some(0x0600_0003));
        assert_eq!(entry_point(&pe(0x014C, false, Some(0x1))), None, "ライブラリ");
        assert_eq!(entry_point(&pe(0x014C, false, None)), None, "ネイティブ");
    }

    #[test]
    fn this_process_can_be_attached_by_an_engine_of_its_own_bitness() {
        assert_eq!(check_attach(std::process::id()), Ok(()));
    }

    #[test]
    fn a_process_that_does_not_exist_is_an_error() {
        // プロセス番号は 4 の倍数なので、この番号のプロセスは無い。
        assert!(check_attach(0xFFFF_FFF1).unwrap_err().contains("開けません"));
    }

    #[test]
    fn the_codeview_entry_gives_the_id_of_the_pdb() {
        let mut exe = pe(0x014C, false, Some(0x1));
        let directories = 0x80 + 24 + 96;
        // デバッグ ディレクトリは RVA 0x2100（ファイル上 0x300）に 2 項目。2 つ目が CodeView で、中身はファイル上 0x340。
        exe[directories + 6 * 8..directories + 6 * 8 + 4].copy_from_slice(&0x2100u32.to_le_bytes());
        exe[directories + 6 * 8 + 4..directories + 6 * 8 + 8].copy_from_slice(&56u32.to_le_bytes());
        exe[0x300 + 12..0x300 + 16].copy_from_slice(&16u32.to_le_bytes());
        let entry = 0x300 + 28;
        exe[entry + 4..entry + 8].copy_from_slice(&0xA1B2_C3D4u32.to_le_bytes());
        exe[entry + 12..entry + 16].copy_from_slice(&2u32.to_le_bytes());
        exe[entry + 24..entry + 28].copy_from_slice(&0x340u32.to_le_bytes());
        exe[0x340..0x344].copy_from_slice(b"RSDS");
        exe[0x344..0x354].copy_from_slice(&[7u8; 16]);
        assert_eq!(codeview(&exe), Some(CodeView { guid: [7; 16], stamp: 0xA1B2_C3D4 }));
        assert_eq!(codeview(&pe(0x014C, false, Some(0x1))), None, "デバッグ ディレクトリが無い");
    }

    /// デバッグ ディレクトリ（RVA 0x2100、ファイル上 0x300）に、種類 `kind` の項目を 1 つ置く。中身はファイル上 0x340 から。
    fn with_debug_entry(kind: u32, content: &[u8]) -> Vec<u8> {
        let mut exe = pe(0x014C, false, Some(0x1));
        let directories = 0x80 + 24 + 96;
        exe[directories + 6 * 8..directories + 6 * 8 + 4].copy_from_slice(&0x2100u32.to_le_bytes());
        exe[directories + 6 * 8 + 4..directories + 6 * 8 + 8].copy_from_slice(&28u32.to_le_bytes());
        exe[0x300 + 12..0x300 + 16].copy_from_slice(&kind.to_le_bytes());
        exe[0x300 + 16..0x300 + 20].copy_from_slice(&(content.len() as u32).to_le_bytes());
        exe[0x300 + 24..0x300 + 28].copy_from_slice(&0x340u32.to_le_bytes());
        exe[0x340..0x340 + content.len()].copy_from_slice(content);
        exe
    }

    #[test]
    fn a_mapped_image_is_laid_out_like_the_file() {
        let mut file = pe(0x014C, false, Some(0x1));
        // メタデータは RVA 0x2100（ファイル上 0x300）に 4 バイト。ヘッダーは 0x200 バイト。
        file[0x208 + 8..0x208 + 12].copy_from_slice(&0x2100u32.to_le_bytes());
        file[0x208 + 12..0x208 + 16].copy_from_slice(&4u32.to_le_bytes());
        file[0x300..0x304].copy_from_slice(b"BSJB");
        let optional = 0x80 + 24;
        file[optional + 60..optional + 64].copy_from_slice(&0x200u32.to_le_bytes());
        assert_eq!(file_layout(&file).as_deref(), Some(&file[..]), "もとからファイルの並び");
        // 読み込まれた像: セクションは RVA 0x2000 の位置にある。
        let mut mapped = vec![0u8; 0x2400];
        mapped[..0x200].copy_from_slice(&file[..0x200]);
        mapped[0x2000..0x2200].copy_from_slice(&file[0x200..0x400]);
        assert_eq!(file_layout(&mapped).as_deref(), Some(&file[..]));
        assert_eq!(file_layout(b"not a PE"), None);
    }

    #[test]
    fn the_embedded_pdb_entry_gives_its_content() {
        let exe = with_debug_entry(DEBUG_EMBEDDED_PDB, b"MPDB\x03\0\0\0abc");
        assert_eq!(embedded_pdb(&exe), Some(&b"MPDB\x03\0\0\0abc"[..]));
        assert_eq!(embedded_pdb(&with_debug_entry(16, b"")), None, "Reproducible の項目だけ");
        assert_eq!(embedded_pdb(&pe(0x014C, false, Some(0x1))), None, "デバッグ ディレクトリが無い");
    }

    #[test]
    fn the_embedded_pdb_is_found_from_the_headers_and_the_table_only() {
        let path = std::env::temp_dir().join(format!("netdbg-embedded-test-{}.dll", std::process::id()));
        // 表のあとを切り落としても見つかる（中身までは読まない）。
        std::fs::write(&path, &with_debug_entry(DEBUG_EMBEDDED_PDB, b"MPDB")[..0x300 + 28]).unwrap();
        let embedded = has_embedded_pdb(&path);
        std::fs::write(&path, with_debug_entry(DEBUG_CODEVIEW, b"RSDS")).unwrap();
        let codeview_only = has_embedded_pdb(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(embedded);
        assert!(!codeview_only);
        assert!(!has_embedded_pdb(Path::new("Z:\\no\\such\\module.dll")));
    }

    #[test]
    fn something_that_is_not_a_pe_is_none() {
        assert_eq!(inspect(b"#!/bin/sh\n"), None);
        assert_eq!(inspect(&[]), None);
        assert_eq!(inspect(&pe(0xAA64, true, None)), None, "ARM64");
    }

    #[test]
    fn a_mismatch_names_the_engine_to_use() {
        let other: (u16, bool) = if cfg!(target_pointer_width = "64") { (0x014C, false) } else { (0x8664, true) };
        let path = std::env::temp_dir().join(format!("netdbg-target-test-{}.exe", std::process::id()));
        std::fs::write(&path, pe(other.0, other.1, None)).unwrap();
        let error = check_launch(&path).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        let wanted = if cfg!(target_pointer_width = "64") { "x86 の netdbg.exe" } else { "x64 の netdbg.exe" };
        assert!(error.contains(wanted), "{error}");
    }
}
