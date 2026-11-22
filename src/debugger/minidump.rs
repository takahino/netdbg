//! Windows のミニダンプ（`.dmp`）を読む（クラッシュダンプ。段階 12）。DAC に渡すのは、メモリー、スレッドの
//! レジスター、モジュールの一覧だけなので、そのための流れ（stream）だけを読む。形式は minidumpapiset.h のもの。
//!
//! 64bit のツール（タスク マネージャー、WER、64bit の procdump）で 32bit のプロセスを書き出すと、64bit のダンプに
//! なる（WOW64）。そのスレッドのレジスターは 64bit のもので、32bit のものは、64bit の TEB から WOW64 の作業領域を
//! たどって読む（`x86_context`）。メモリーの番地は 32bit のプロセスと同じなので、そのまま読める。
//!
//! ファイルは丸ごと読み込まない（数 GB になる。x86 のエンジンでは写像もできない）。読むたびに、その位置を読む。

use std::path::Path;

const SIGNATURE: u32 = 0x504D_444D; // "MDMP"
const THREAD_LIST: u32 = 3;
const MODULE_LIST: u32 = 4;
const MEMORY_LIST: u32 = 5;
const EXCEPTION: u32 = 6;
const SYSTEM_INFO: u32 = 7;
const MEMORY64_LIST: u32 = 9;
const MISC_INFO: u32 = 15;

pub const ARCHITECTURE_X86: u16 = 0;
pub const ARCHITECTURE_AMD64: u16 = 9;
/// x86 の CONTEXT の大きさと、その先頭の ContextFlags に必ず立つ印（CONTEXT_i386）。
pub const X86_CONTEXT_SIZE: usize = 716;
const CONTEXT_I386: u32 = 0x0001_0000;
/// 64bit の TEB の TlsSlots の位置と、WOW64 の作業領域（WOW64_TLS_CPURESERVED）の番号。
const TEB64_TLS_SLOTS: u64 = 0x1480;
const WOW64_TLS_CPURESERVED: u64 = 1;

/// ファイルの位置を指定して読めるもの（試験では、メモリーの上のバイト列）。
pub trait ReadAt {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<usize>;
}

impl ReadAt for std::fs::File {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::os::windows::fs::FileExt::seek_read(self, buffer, offset)
    }
}

impl ReadAt for Vec<u8> {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.len());
        let count = buffer.len().min(self.len() - start);
        buffer[..count].copy_from_slice(&self[start..start + count]);
        Ok(count)
    }
}

pub struct Thread {
    pub id: u32,
    pub teb: u64,
    /// ダンプに書かれたレジスター（CONTEXT）のファイルの中の位置と大きさ。
    context: (u64, u32),
}

pub struct Module {
    pub base: u64,
    pub path: String,
}

/// ダンプが書かれたときの例外（WER などがクラッシュの瞬間に書いたもの）。
pub struct Exception {
    pub thread: u32,
    pub code: u32,
}

/// 読み込んだメモリーの範囲（番地の順）。
struct Range {
    start: u64,
    size: u64,
    file_offset: u64,
}

pub struct Minidump<R: ReadAt = std::fs::File> {
    source: R,
    pub architecture: u16,
    pub threads: Vec<Thread>,
    pub modules: Vec<Module>,
    pub exception: Option<Exception>,
    pub process_id: Option<u32>,
    memory: Vec<Range>,
}

impl Minidump {
    pub fn open(path: &Path) -> Result<Minidump, String> {
        let file = std::fs::File::open(path).map_err(|e| tr!("Cannot open the dump file: {path} ({error})", path = path.display(), error = e))?;
        Minidump::parse(file).map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl<R: ReadAt> Minidump<R> {
    pub fn parse(source: R) -> Result<Minidump<R>, String> {
        let header = read_bytes(&source, 0, 32)?;
        if u32_at(&header, 0) != SIGNATURE {
            return Err(tr!("Not a minidump (.dmp) file."));
        }
        let (count, directory) = (u32_at(&header, 8) as usize, u64::from(u32_at(&header, 12)));
        let entries = read_bytes(&source, directory, count * 12)?;
        let mut dump = Minidump { source, architecture: u16::MAX, threads: Vec::new(), modules: Vec::new(), exception: None, process_id: None, memory: Vec::new() };
        for entry in entries.chunks_exact(12) {
            let (kind, size, rva) = (u32_at(entry, 0), u32_at(entry, 4) as usize, u64::from(u32_at(entry, 8)));
            if size == 0 {
                continue;
            }
            let data = read_bytes(&dump.source, rva, size)?;
            match kind {
                SYSTEM_INFO => dump.architecture = u16_at(&data, 0),
                THREAD_LIST => dump.threads = parse_threads(&data),
                MODULE_LIST => dump.modules = parse_modules(&dump.source, &data),
                MEMORY_LIST => dump.memory.extend(parse_memory(&data)),
                MEMORY64_LIST => dump.memory.extend(parse_memory64(&data)),
                EXCEPTION if data.len() >= 12 => dump.exception = Some(Exception { thread: u32_at(&data, 0), code: u32_at(&data, 8) }),
                // MINIDUMP_MISC_INFO: Flags1 の MINIDUMP_MISC1_PROCESS_ID が立っていれば、プロセス番号がある。
                MISC_INFO if data.len() >= 12 && u32_at(&data, 4) & 1 != 0 => dump.process_id = Some(u32_at(&data, 8)),
                _ => {}
            }
        }
        if dump.memory.is_empty() {
            return Err(tr!("The dump has no memory. Use a dump written with memory (a full dump)."));
        }
        dump.memory.sort_by_key(|r| r.start);
        Ok(dump)
    }

    /// 対象の番地から読む。続いた範囲にある分だけを読み、読めたバイト数を返す（範囲の外なら 0）。
    pub fn read(&self, address: u64, buffer: &mut [u8]) -> usize {
        let mut done = 0;
        while done < buffer.len() {
            let at = address + done as u64;
            // 番地を含む範囲（start <= at の最後のもの）。
            let index = self.memory.partition_point(|r| r.start <= at);
            let Some(range) = index.checked_sub(1).map(|i| &self.memory[i]).filter(|r| at - r.start < r.size) else { break };
            let offset = at - range.start;
            let count = ((range.size - offset) as usize).min(buffer.len() - done);
            match self.source.read_at(range.file_offset + offset, &mut buffer[done..done + count]) {
                Ok(read) if read > 0 => done += read,
                _ => break,
            }
        }
        done
    }

    fn read_u64(&self, address: u64) -> Option<u64> {
        let mut bytes = [0u8; 8];
        (self.read(address, &mut bytes) == 8).then(|| u64::from_le_bytes(bytes))
    }

    /// スレッドのレジスター（ダンプに書かれたまま。WOW64 なら 64bit のもの）。
    pub fn context(&self, thread: &Thread) -> Option<Vec<u8>> {
        let (offset, size) = thread.context;
        (size > 0).then(|| read_bytes(&self.source, offset, size as usize).ok()).flatten()
    }

    /// 32bit のスレッドのレジスター（x86 の CONTEXT）。32bit のダンプならダンプのもの、WOW64 のダンプなら WOW64 の
    /// 作業領域のもの（64bit の TEB の TlsSlots[WOW64_TLS_CPURESERVED] が指す WOW64_CPURESERVED の、Flags と
    /// Machine の 4 バイトのあと）。どちらも、先頭の ContextFlags に CONTEXT_i386 が立っていることを確かめる。
    pub fn x86_context(&self, thread: &Thread) -> Option<Vec<u8>> {
        let is_x86 = |context: &[u8]| context.len() >= X86_CONTEXT_SIZE && u32_at(context, 0) & CONTEXT_I386 != 0;
        if self.architecture == ARCHITECTURE_X86 {
            return self.context(thread).filter(|c| is_x86(c));
        }
        let reserved = self.read_u64(thread.teb + TEB64_TLS_SLOTS + WOW64_TLS_CPURESERVED * 8).filter(|&p| p != 0)?;
        let mut context = vec![0u8; X86_CONTEXT_SIZE];
        (self.read(reserved + 4, &mut context) == X86_CONTEXT_SIZE && is_x86(&context)).then_some(context)
    }

    /// 名前（パスの最後）が `file_name` のモジュール（大文字と小文字は区別しない）。
    pub fn module_named(&self, file_name: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.path.rsplit(['\\', '/']).next().is_some_and(|name| name.eq_ignore_ascii_case(file_name)))
    }
}

fn read_bytes<R: ReadAt>(source: &R, offset: u64, size: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0u8; size];
    let mut done = 0;
    while done < size {
        match source.read_at(offset + done as u64, &mut buffer[done..]) {
            Ok(0) | Err(_) => return Err(tr!("The dump file is cut short.")),
            Ok(read) => done += read,
        }
    }
    Ok(buffer)
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    data.get(at..at + 8).map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// 数（u32）と、`size` バイトずつの並び。
fn records(data: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    let count = u32_at(data, 0) as usize;
    data.get(4..).unwrap_or_default().chunks_exact(size).take(count)
}

/// MINIDUMP_THREAD（48 バイト）: ThreadId、…、Teb（+16）、Stack（+24、16 バイト）、ThreadContext（+40: 大きさ、位置）。
fn parse_threads(data: &[u8]) -> Vec<Thread> {
    records(data, 48).map(|t| Thread { id: u32_at(t, 0), teb: u64_at(t, 16), context: (u64::from(u32_at(t, 44)), u32_at(t, 40)) }).collect()
}

/// MINIDUMP_MODULE（108 バイト）: BaseOfImage、…、ModuleNameRva（+20）。
fn parse_modules<R: ReadAt>(source: &R, data: &[u8]) -> Vec<Module> {
    records(data, 108)
        .map(|m| {
            // MINIDUMP_STRING: 長さ（バイト）と UTF-16。
            let name_rva = u64::from(u32_at(m, 20));
            let length = read_bytes(source, name_rva, 4).map_or(0, |b| u32_at(&b, 0) as usize);
            let path = read_bytes(source, name_rva + 4, length)
                .map(|b| String::from_utf16_lossy(&b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
                .unwrap_or_default();
            Module { base: u64_at(m, 0), path }
        })
        .collect()
}

/// MINIDUMP_MEMORY_DESCRIPTOR（16 バイト）: 番地、大きさ、ファイルの位置。
fn parse_memory(data: &[u8]) -> Vec<Range> {
    records(data, 16).map(|d| Range { start: u64_at(d, 0), size: u64::from(u32_at(d, 8)), file_offset: u64::from(u32_at(d, 12)) }).collect()
}

/// MINIDUMP_MEMORY64_LIST: 数（u64）、中身の始まりの位置（u64）、番地と大きさ（16 バイトずつ）。中身は続けて並ぶ。
fn parse_memory64(data: &[u8]) -> Vec<Range> {
    let count = u64_at(data, 0) as usize;
    let mut file_offset = u64_at(data, 8);
    let mut ranges = Vec::new();
    for d in data.get(16..).unwrap_or_default().chunks_exact(16).take(count) {
        let (start, size) = (u64_at(d, 0), u64_at(d, 8));
        ranges.push(Range { start, size, file_offset });
        file_offset += size;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 流れを並べたダンプを作る（ヘッダー、目録、中身の順）。
    fn dump(streams: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let directory = 32u32;
        let mut data_at = directory + 12 * streams.len() as u32;
        let mut file = Vec::new();
        for value in [SIGNATURE, 0xA793, streams.len() as u32, directory, 0, 0, 0, 0] {
            file.extend(value.to_le_bytes());
        }
        let mut body: Vec<u8> = Vec::new();
        for (kind, data) in streams {
            file.extend(kind.to_le_bytes());
            file.extend((data.len() as u32).to_le_bytes());
            file.extend(data_at.to_le_bytes());
            data_at += data.len() as u32;
            body.extend(data);
        }
        file.extend(body);
        file
    }

    fn memory64(base_rva: u64, ranges: &[(u64, u64)]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend((ranges.len() as u64).to_le_bytes());
        data.extend(base_rva.to_le_bytes());
        for (start, size) in ranges {
            data.extend(start.to_le_bytes());
            data.extend(size.to_le_bytes());
        }
        data
    }

    #[test]
    fn reads_memory_across_adjacent_ranges_and_stops_at_a_gap() {
        // 中身は目録のあと、流れの中身（システム情報 4 バイトと、Memory64 の 16 + 32 バイト）のあとに置く。
        let system = vec![0u8; 4];
        let ranges = [(0x1000, 4), (0x1004, 4), (0x2000, 2)];
        let header_and_streams = 32 + 12 * 2 + system.len() as u64 + 16 + 16 * ranges.len() as u64;
        let mut file = dump(&[(SYSTEM_INFO, system), (MEMORY64_LIST, memory64(header_and_streams, &ranges))]);
        file.extend([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let dump = Minidump::parse(file).unwrap();
        assert_eq!(dump.architecture, ARCHITECTURE_X86);
        let mut buffer = [0u8; 10];
        assert_eq!(dump.read(0x1002, &mut buffer), 6);
        assert_eq!(&buffer[..6], &[3, 4, 5, 6, 7, 8]);
        assert_eq!(dump.read(0x2000, &mut buffer[..2]), 2);
        assert_eq!(&buffer[..2], &[9, 10]);
        assert_eq!(dump.read(0x3000, &mut buffer), 0);
    }

    #[test]
    fn a_file_that_is_not_a_minidump_is_refused() {
        assert!(Minidump::parse(vec![0u8; 64]).err().unwrap().contains("ミニダンプ"));
    }

    #[test]
    fn a_dump_without_memory_says_to_write_a_full_dump() {
        assert!(Minidump::parse(dump(&[(SYSTEM_INFO, vec![0u8; 4])])).err().unwrap().contains("フル ダンプ"));
    }
}
