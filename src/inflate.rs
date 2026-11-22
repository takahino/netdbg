//! Deflate（RFC 1951）の展開。埋め込み PDB を読むためだけに使う（D-041）。
//!
//! 速さより短さと確かさを取る。ハフマン符号は 1 ビットずつたどる（puff と同じやり方）。埋め込み PDB は大きくても
//! 数 MB で、モジュールの読み込みのときに一度展開するだけなので、これで足りる。
//!
//! release は `panic = "abort"` で、panic するとデバッグのセッションごと落ちる。壊れたデータでも落ちないよう、
//! 添字はすべて確かめてから使い、だめなら `Err` を返す。

pub const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
pub const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
pub const DISTANCE_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
pub const DISTANCE_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// 動的ハフマンのブロックで、符号長の符号の長さが並ぶ順。
const CODE_LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
const MAX_BITS: usize = 15;
const END_OF_BLOCK: u16 = 256;

/// `input` を展開する。展開後の大きさは `size` ちょうどでなければならない（埋め込み PDB はヘッダーに書いてある）。
/// `size` を超えて膨らむデータは、そこで止める。
pub fn inflate(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits { data: input, position: 0, buffer: 0, count: 0 };
    let mut out = Output { bytes: Vec::new(), limit: size };
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => stored(&mut bits, &mut out)?,
            1 => codes(&mut bits, &mut out, &Huffman::fixed_literals()?, &Huffman::fixed_distances()?)?,
            2 => {
                let (literals, distances) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &literals, &distances)?;
            }
            _ => return Err(tr!("The Deflate block type is not valid")),
        }
        if last {
            break;
        }
    }
    if out.bytes.len() != size {
        return Err(tr!("The expanded size ({actual}) differs from the recorded size ({size})", actual = out.bytes.len(), size = size));
    }
    Ok(out.bytes)
}

/// 下位のビットから順に読む。
struct Bits<'a> {
    data: &'a [u8],
    position: usize,
    buffer: u32,
    count: u32,
}

impl Bits<'_> {
    /// `n` ビット（16 まで）を読む。
    fn take(&mut self, n: u32) -> Result<u32, String> {
        while self.count < n {
            let byte = *self.data.get(self.position).ok_or_else(|| tr!("The Deflate data is cut short"))?;
            self.position += 1;
            self.buffer |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.buffer & ((1u32 << n) - 1);
        self.buffer >>= n;
        self.count -= n;
        Ok(value)
    }

    /// バイトの境目まで読み飛ばす（非圧縮のブロックの前）。
    fn align(&mut self) {
        let rest = self.count % 8;
        self.buffer >>= rest;
        self.count -= rest;
    }
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
}

impl Output {
    fn push(&mut self, byte: u8) -> Result<(), String> {
        if self.bytes.len() >= self.limit {
            return Err(tr!("The expanded data exceeds the recorded size"));
        }
        self.bytes.push(byte);
        Ok(())
    }

    /// `distance` バイト前から `length` バイトを写す。写す元と先は重なってよい（同じ並びの繰り返しを表す）。
    fn copy(&mut self, distance: usize, length: usize) -> Result<(), String> {
        if distance == 0 || distance > self.bytes.len() {
            return Err(tr!("A Deflate distance points outside the expanded data"));
        }
        for _ in 0..length {
            let byte = self.bytes[self.bytes.len() - distance];
            self.push(byte)?;
        }
        Ok(())
    }
}

fn stored(bits: &mut Bits, out: &mut Output) -> Result<(), String> {
    bits.align();
    let length = bits.take(16)?;
    if bits.take(16)? != !length & 0xFFFF {
        return Err(tr!("The length of a stored Deflate block is broken"));
    }
    for _ in 0..length {
        out.push(bits.take(8)? as u8)?;
    }
    Ok(())
}

/// 正準ハフマン符号。長さごとの符号の数と、符号の順に並べた記号で表す。
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    /// 記号ごとの符号の長さ（0 は使わない記号）から作る。符号が足りない（不完全な）ことは許すが、多すぎるのは壊れている。
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in lengths {
            *counts.get_mut(length as usize).ok_or_else(|| tr!("A Deflate code length is too long"))? += 1;
        }
        let mut left: i32 = 1;
        for &count in &counts[1..] {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return Err(tr!("The Deflate code lengths are inconsistent"));
            }
        }
        let mut offsets = [0usize; MAX_BITS + 2];
        for length in 1..=MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length] as usize;
        }
        let mut symbols = vec![0u16; offsets[MAX_BITS + 1]];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize]] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn fixed_literals() -> Result<Huffman, String> {
        let mut lengths = [8u8; 288];
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        Huffman::new(&lengths)
    }

    fn fixed_distances() -> Result<Huffman, String> {
        Huffman::new(&[5u8; 30])
    }

    /// 1 ビットずつ読み、長さ `length` の符号の範囲に入ったところで記号を返す。
    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        // code: 読んだ符号、first: この長さの最初の符号、index: この長さの最初の記号の位置。
        let (mut code, mut first, mut index) = (0usize, 0usize, 0usize);
        for &count in &self.counts[1..] {
            code |= bits.take(1)? as usize;
            let count = count as usize;
            if code < first + count {
                return self.symbols.get(index + code - first).copied().ok_or_else(|| tr!("A Deflate code is broken"));
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(tr!("A Deflate code matches no symbol"))
    }
}

/// 動的ハフマンのブロックの頭にある、リテラルと長さの符号、距離の符号を読む。
fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), String> {
    let literal_count = bits.take(5)? as usize + 257;
    let distance_count = bits.take(5)? as usize + 1;
    let code_length_count = bits.take(4)? as usize + 4;
    if literal_count > 286 || distance_count > 30 {
        return Err(tr!("There are too many Deflate codes"));
    }
    let mut code_lengths = [0u8; 19];
    for &symbol in &CODE_LENGTH_ORDER[..code_length_count] {
        code_lengths[symbol] = bits.take(3)? as u8;
    }
    let code_length_code = Huffman::new(&code_lengths)?;

    let total = literal_count + distance_count;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = code_length_code.decode(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => (*lengths.last().ok_or_else(|| tr!("A repeat of Deflate code lengths has nothing to repeat"))?, 3 + bits.take(2)? as usize),
            17 => (0, 3 + bits.take(3)? as usize),
            18 => (0, 11 + bits.take(7)? as usize),
            _ => return Err(tr!("A Deflate code length symbol is not valid")),
        };
        if lengths.len() + repeat > total {
            return Err(tr!("A repeat of Deflate code lengths runs past the table"));
        }
        lengths.extend(std::iter::repeat_n(value, repeat));
    }
    if lengths[END_OF_BLOCK as usize] == 0 {
        return Err(tr!("A Deflate block has no end code"));
    }
    Ok((Huffman::new(&lengths[..literal_count])?, Huffman::new(&lengths[literal_count..])?))
}

/// 圧縮されたブロックの中身（リテラル、長さと距離の組）を、終わりの記号まで展開する。
fn codes(bits: &mut Bits, out: &mut Output, literals: &Huffman, distances: &Huffman) -> Result<(), String> {
    loop {
        let symbol = literals.decode(bits)?;
        if symbol < END_OF_BLOCK {
            out.push(symbol as u8)?;
            continue;
        }
        if symbol == END_OF_BLOCK {
            return Ok(());
        }
        let index = (symbol - 257) as usize;
        let (&base, &extra) = LENGTH_BASE.get(index).zip(LENGTH_EXTRA.get(index)).ok_or_else(|| tr!("A Deflate length symbol is not valid"))?;
        let length = base as usize + bits.take(u32::from(extra))? as usize;
        let index = distances.decode(bits)? as usize;
        let (&base, &extra) = DISTANCE_BASE.get(index).zip(DISTANCE_EXTRA.get(index)).ok_or_else(|| tr!("A Deflate distance symbol is not valid"))?;
        let distance = base as usize + bits.take(u32::from(extra))? as usize;
        out.copy(distance, length)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// .NET の DeflateStream（CompressionLevel.Optimal）で圧縮した "hello, hello, hello!"。固定ハフマンのブロック。
    const FIXED: [u8; 12] = [0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0xD7, 0x51, 0xC8, 0x40, 0xA2, 0x14, 0x01];

    /// 同じく、`dynamic_text()` を圧縮したもの。動的ハフマンのブロック。
    const DYNAMIC: [u8; 502] = [
        0x85, 0x97, 0x49, 0x8E, 0x10, 0x31, 0x0C, 0x45, 0xF7, 0x9C, 0x22, 0x47, 0x88, 0xE3, 0x78, 0xAA, 0xE3, 0x80, 0x1A, 0xD1, 0xA2, 0xD5, 0x2D, 0x10, 0x08, 0x8E,
        0x8F, 0xC3, 0xBE, 0x5E, 0xAD, 0xAD, 0x4C, 0x2F, 0x1E, 0xFE, 0x7F, 0x7B, 0x7D, 0x7F, 0x19, 0xF3, 0x1A, 0x73, 0xFC, 0xFA, 0xF6, 0x32, 0x7E, 0xFC, 0x7E, 0xFD,
        0xF2, 0x7D, 0x7C, 0xFE, 0xF9, 0xF1, 0xE7, 0x7D, 0x7C, 0xFD, 0xF8, 0xFB, 0xE9, 0xED, 0x44, 0xE5, 0x1A, 0x72, 0x1F, 0x5D, 0xD7, 0xD8, 0xF7, 0x51, 0xBD, 0x46,
        0xDD, 0x47, 0x77, 0xEF, 0xEC, 0xF7, 0x61, 0xBB, 0xC6, 0xB2, 0xFB, 0xB0, 0x5F, 0x43, 0x61, 0x75, 0xF4, 0xC5, 0xE0, 0xEC, 0xBC, 0x86, 0xC3, 0xC5, 0xEB, 0x1A,
        0x09, 0xAF, 0x96, 0x46, 0xA6, 0x10, 0x6E, 0x66, 0x0B, 0x76, 0x97, 0x43, 0x2D, 0x20, 0xDE, 0xDC, 0x62, 0x41, 0xBC, 0xC9, 0x51, 0xB8, 0xC9, 0x29, 0xDD, 0xBE,
        0xD1, 0x39, 0xAD, 0x6F, 0x76, 0x05, 0xE4, 0xA5, 0xE1, 0x29, 0x3D, 0xBF, 0xE9, 0x05, 0x64, 0xD4, 0x6A, 0x7A, 0x02, 0xE7, 0xAF, 0xC6, 0x67, 0xB0, 0xFF, 0x6A,
        0x7C, 0x05, 0x5F, 0xBF, 0x1A, 0xDF, 0x06, 0xFC, 0xAB, 0xF1, 0x15, 0xE5, 0x74, 0xF3, 0xDB, 0x74, 0x7E, 0xF3, 0x2B, 0xDA, 0xBF, 0xF9, 0x19, 0xBD, 0xBF, 0xF9,
        0x25, 0x84, 0x1B, 0x9F, 0x03, 0x7E, 0x6D, 0x7C, 0x0B, 0xB2, 0x47, 0x1B, 0x5F, 0xC2, 0xFE, 0xDA, 0xF8, 0x8C, 0x8A, 0xB6, 0xF1, 0x2D, 0xF8, 0x1E, 0x6D, 0x7C,
        0x09, 0xA5, 0xA5, 0x8D, 0xCF, 0x01, 0xAF, 0x9E, 0xCA, 0xA5, 0xF7, 0x35, 0x3E, 0xA1, 0xF5, 0x07, 0x1F, 0x7C, 0xBF, 0x1E, 0x7E, 0x10, 0xDF, 0xCD, 0x6F, 0x03,
        0x9F, 0xDD, 0xFC, 0x14, 0xDE, 0xBF, 0x9B, 0x9F, 0xD0, 0xFA, 0xE6, 0x47, 0xC7, 0x9F, 0xEC, 0x83, 0xEC, 0xDA, 0x8D, 0x2F, 0x01, 0xCF, 0x6E, 0x7C, 0x41, 0x5D,
        0xB5, 0xF1, 0x05, 0xAD, 0x6F, 0x7C, 0x41, 0xE7, 0x17, 0xC7, 0x6D, 0xF2, 0xFE, 0x26, 0x7C, 0x3F, 0x5B, 0xFC, 0x3E, 0x53, 0xE6, 0x63, 0x1B, 0xF1, 0x9A, 0xF1,
        0xEF, 0x98, 0xF3, 0xEF, 0x5A, 0x70, 0x76, 0x58, 0x72, 0x76, 0x59, 0x71, 0x76, 0xFA, 0xE4, 0xEC, 0x76, 0xE1, 0xEA, 0xF0, 0xC5, 0xD5, 0xE5, 0xCA, 0xD5, 0xE9,
        0x9B, 0xAB, 0xDB, 0x8D, 0xBB, 0x83, 0x3B, 0x77, 0x17, 0x0F, 0xEE, 0x4E, 0x9E, 0xDC, 0xDD, 0xBC, 0xB0, 0x39, 0xC6, 0xE4, 0xDE, 0x1A, 0xC2, 0xBD, 0x39, 0x16,
        0xF7, 0xF6, 0x50, 0x9E, 0x0D, 0xB1, 0x79, 0xB6, 0x84, 0xF1, 0x6C, 0x0A, 0xE7, 0xD9, 0x16, 0xC1, 0xB3, 0x31, 0x92, 0x67, 0x6B, 0x14, 0xCF, 0xE6, 0x9C, 0x3C,
        0xDB, 0x53, 0x58, 0x1B, 0xE4, 0x62, 0x6D, 0x91, 0x8A, 0xD2, 0x24, 0x37, 0x2B, 0x9B, 0x34, 0x56, 0x46, 0xE9, 0xAC, 0xAC, 0x32, 0x50, 0x98, 0x65, 0xB2, 0xAE,
        0xCB, 0x7A, 0x90, 0x85, 0x93, 0x55, 0x65, 0x09, 0x8B, 0xD2, 0x5A, 0xAC, 0x69, 0x4B, 0x59, 0x12, 0xD7, 0x46, 0x41, 0x5D, 0x86, 0x6A, 0xBC, 0x1C, 0xA5, 0x7C,
        0x05, 0xFA, 0x80, 0x4A, 0x5E, 0x5D, 0x78, 0xB6, 0xCC, 0x89, 0x57, 0x97, 0x29, 0xFC, 0x74, 0x99, 0x0F, 0xEC, 0x64, 0x2A, 0xC3, 0x97, 0xB9, 0xF9, 0xF7, 0x64,
        0x1A, 0x7F, 0xBF, 0x4C, 0x7F, 0xF2, 0x05, 0x9C, 0x7F, 0x32, 0xF3, 0xC1, 0x19, 0xCC, 0x7A, 0xB0, 0x06, 0xC7, 0x7A, 0xA0, 0x37, 0xF8, 0x6F, 0x3E, 0x28, 0xFE,
        0x50, 0xC1, 0x72, 0xDC, 0x07, 0xDA, 0x83, 0x63, 0x3F, 0xD0, 0x1F, 0xFC, 0x37, 0x20, 0xE8, 0x8F, 0x9C, 0xBB, 0x98, 0xC8, 0x43, 0x1B, 0x94, 0xE3, 0x41, 0xA8,
        0x8F, 0xCA, 0x31, 0x21, 0x37, 0x8D, 0xF8, 0x1F,
    ];

    /// `DYNAMIC` の元の文字列。PowerShell で同じものを作って圧縮した。
    fn dynamic_text() -> Vec<u8> {
        (0..120).map(|i| format!("line {i}: {} the quick brown fox\n", i * i % 97)).collect::<String>().into_bytes()
    }

    #[test]
    fn a_stored_block_is_copied() {
        let input = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&input, 3).unwrap(), b"abc");
    }

    #[test]
    fn a_fixed_huffman_block_is_expanded() {
        assert_eq!(inflate(&FIXED, 20).unwrap(), b"hello, hello, hello!");
    }

    #[test]
    fn a_dynamic_huffman_block_is_expanded() {
        assert_eq!((DYNAMIC[0] >> 1) & 3, 2, "試験のデータが動的ハフマンのブロックであること");
        let text = dynamic_text();
        assert_eq!(inflate(&DYNAMIC, text.len()).unwrap(), text);
    }

    #[test]
    fn several_blocks_are_joined() {
        // 最後ではない非圧縮のブロック ("ab") のあとに、最後の非圧縮のブロック ("c")。
        let input = [0x00, 0x02, 0x00, 0xFD, 0xFF, b'a', b'b', 0x01, 0x01, 0x00, 0xFE, 0xFF, b'c'];
        assert_eq!(inflate(&input, 3).unwrap(), b"abc");
    }

    #[test]
    fn a_size_that_differs_from_the_header_is_an_error() {
        assert!(inflate(&FIXED, 19).is_err(), "書かれている大きさを超える");
        assert!(inflate(&FIXED, 21).is_err(), "書かれている大きさに足りない");
    }

    #[test]
    fn truncated_data_is_an_error() {
        let text = dynamic_text();
        for end in 0..DYNAMIC.len() {
            assert!(inflate(&DYNAMIC[..end], text.len()).is_err(), "{end} バイトで切ったもの");
        }
    }

    #[test]
    fn broken_headers_are_errors() {
        assert!(inflate(&[0x07], 0).is_err(), "ブロックの種類 3");
        assert!(inflate(&[0x01, 0x03, 0x00, 0x00, 0x00, b'a', b'b', b'c'], 3).is_err(), "非圧縮のブロックの長さの補数が合わない");
        // 固定ハフマンで、最初の記号が長さと距離の組（距離 1）。まだ何も展開していないので、写す元が無い。
        assert!(inflate(&[0x03, 0x02, 0x00], 3).is_err(), "展開済みの範囲の外を指す距離");
    }

    #[test]
    fn random_data_never_panics() {
        // 壊れたデータで panic すると、release ではセッションごと落ちる。でたらめなデータでも Err で返ること。
        let mut seed: u32 = 12345;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as u8
        };
        for _ in 0..2000 {
            let length = next() as usize;
            let data: Vec<u8> = (0..length).map(|_| next()).collect();
            let _ = inflate(&data, 4096);
        }
        // 正しいデータの 1 バイトだけを変えたもの。
        let text = dynamic_text();
        for position in 0..DYNAMIC.len() {
            let mut data = DYNAMIC;
            data[position] ^= next() | 1;
            let _ = inflate(&data, text.len());
        }
    }
}
