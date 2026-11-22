//! Deflate（RFC 1951）の圧縮。スクリーンショットの PNG を書くためだけに使う（D-053）。
//!
//! 固定ハフマンのブロック 1 つと、LZ77（3 バイトのハッシュの連鎖で、前に出た並びを探す）。動的ハフマンほどは縮まないが、
//! 画面の画像（同じ色が続く）なら十分に縮み、短く確かに書ける。展開は `inflate.rs` で試験する。

use crate::inflate::{DISTANCE_BASE, DISTANCE_EXTRA, LENGTH_BASE, LENGTH_EXTRA};

const WINDOW: usize = 32 * 1024;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: u32 = 15;
/// 連鎖をたどる回数の上限（速さとのかね合い）。
const MAX_CHAIN: usize = 48;
/// これより長い一致が見つかれば、そこで探すのをやめる。
const GOOD_ENOUGH: usize = 64;

/// 下位のビットから順に詰める書き手。
struct BitWriter {
    bytes: Vec<u8>,
    buffer: u64,
    count: u32,
}

impl BitWriter {
    fn bits(&mut self, value: u32, count: u32) {
        self.buffer |= (value as u64) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.bytes.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    /// ハフマン符号は上位のビットから書く（ほかの値は下位から）。
    fn code(&mut self, code: u32, length: u32) {
        self.bits(code.reverse_bits() >> (32 - length), length);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push(self.buffer as u8);
        }
        self.bytes
    }
}

/// 固定ハフマンの文字・長さの符号（RFC 1951 3.2.6）。
fn literal(writer: &mut BitWriter, symbol: u32) {
    match symbol {
        0..=143 => writer.code(0x30 + symbol, 8),
        144..=255 => writer.code(0x190 + symbol - 144, 9),
        256..=279 => writer.code(symbol - 256, 7),
        _ => writer.code(0xC0 + symbol - 280, 8),
    }
}

/// `table` の中で、`value` を含む区間の番号（`table` は区間の始まりの昇順）。
fn bucket(table: &[u16], value: usize) -> usize {
    table.iter().rposition(|&base| base as usize <= value).unwrap_or(0)
}

fn copy(writer: &mut BitWriter, length: usize, distance: usize) {
    // 長さ 258 は専用の符号（285）。227 からの 5 ビットの追加でも表せるが、展開する側は 285 を 258 と読む。
    let l = if length == MAX_MATCH { 28 } else { bucket(&LENGTH_BASE[..28], length) };
    literal(writer, 257 + l as u32);
    writer.bits((length - LENGTH_BASE[l] as usize) as u32, LENGTH_EXTRA[l] as u32);
    let d = bucket(&DISTANCE_BASE, distance);
    writer.code(d as u32, 5);
    writer.bits((distance - DISTANCE_BASE[d] as usize) as u32, DISTANCE_EXTRA[d] as u32);
}

fn hash(data: &[u8], at: usize) -> usize {
    let value = (data[at] as u32) << 16 | (data[at + 1] as u32) << 8 | data[at + 2] as u32;
    (value.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// `data` を圧縮する（zlib の包みは付けない。PNG の側で付ける）。
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter { bytes: Vec::with_capacity(data.len() / 4 + 64), buffer: 0, count: 0 };
    writer.bits(1, 1); // 最後のブロック
    writer.bits(1, 2); // 固定ハフマン
    // head[h]: ハッシュ h の、いちばん新しい位置 + 1（0 は無し）。previous[i % WINDOW]: 同じハッシュの、1 つ前の位置 + 1。
    let mut head = vec![0usize; 1 << HASH_BITS];
    let mut previous = vec![0usize; WINDOW];
    let insert = |head: &mut Vec<usize>, previous: &mut Vec<usize>, at: usize| {
        if at + MIN_MATCH <= data.len() {
            let h = hash(data, at);
            previous[at % WINDOW] = head[h];
            head[h] = at + 1;
        }
    };
    let mut at = 0;
    while at < data.len() {
        let (mut best_length, mut best_distance) = (0, 0);
        if at + MIN_MATCH <= data.len() {
            let limit = (data.len() - at).min(MAX_MATCH);
            let mut candidate = head[hash(data, at)];
            let mut chain = 0;
            while candidate > 0 && chain < MAX_CHAIN {
                let from = candidate - 1;
                if at - from > WINDOW - 1 {
                    break;
                }
                let length = data[from..].iter().zip(&data[at..at + limit]).take_while(|(a, b)| a == b).count();
                if length > best_length {
                    (best_length, best_distance) = (length, at - from);
                    if length >= GOOD_ENOUGH.min(limit) {
                        break;
                    }
                }
                candidate = previous[from % WINDOW];
                chain += 1;
            }
        }
        if best_length >= MIN_MATCH {
            copy(&mut writer, best_length, best_distance);
            for i in at..at + best_length {
                insert(&mut head, &mut previous, i);
            }
            at += best_length;
        } else {
            literal(&mut writer, data[at] as u32);
            insert(&mut head, &mut previous, at);
            at += 1;
        }
    }
    literal(&mut writer, 256);
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate;

    fn round_trip(data: &[u8]) -> usize {
        let packed = deflate(data);
        assert_eq!(inflate(&packed, data.len()).unwrap(), data);
        packed.len()
    }

    #[test]
    fn data_comes_back_unchanged() {
        round_trip(b"");
        round_trip(b"a");
        round_trip(b"abcabcabcabcabcabcabcabc");
        round_trip(&[0u8; 1000]);
        // 長さ 258 ちょうど、と距離の上限に近い一致。
        let mut long: Vec<u8> = (0..40_000u32).map(|i| (i * 7 % 251) as u8).collect();
        long.extend(std::iter::repeat_n(9u8, 258 + 3));
        round_trip(&long);
        // ばらばらのデータ（一致がほとんど無い）。
        let mut x = 12345u32;
        let noise: Vec<u8> = (0..5000).map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (x >> 16) as u8
        }).collect();
        round_trip(&noise);
    }

    #[test]
    fn repeated_rows_shrink_a_lot() {
        let row: Vec<u8> = (0..400u32).flat_map(|i| [(i % 7) as u8, 200, 30, 255]).collect();
        let image: Vec<u8> = row.iter().copied().cycle().take(row.len() * 100).collect();
        assert!(round_trip(&image) < image.len() / 20);
    }
}
