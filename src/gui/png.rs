//! PNG を書く（スクリーンショット用）。色は RGB の 8 ビット、透明は持たない。

use crate::deflate::deflate;

/// 上の行から並んだ BGRA（GDI の 32 ビットの DIB の並び）を PNG にする。
pub fn encode(width: usize, height: usize, bgra: &[u8]) -> Vec<u8> {
    assert_eq!(bgra.len(), width * height * 4, "pixel count does not match the size");
    let stride = width * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height);
    let mut above = vec![0u8; stride];
    let mut row = vec![0u8; stride];
    for y in 0..height {
        for (x, pixel) in bgra[y * width * 4..(y + 1) * width * 4].chunks_exact(4).enumerate() {
            row[x * 3..x * 3 + 3].copy_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        let (filter, filtered) = best_filter(&row, &above);
        raw.push(filter);
        raw.extend_from_slice(&filtered);
        std::mem::swap(&mut row, &mut above);
    }

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 ビット、RGB、Deflate、フィルターの方式 0、インターレース無し
    chunk(&mut png, b"IHDR", &header);
    // zlib の包み: ヘッダー（32K の窓、Deflate）、本体、Adler-32。
    let mut zlib = vec![0x78, 0x01];
    zlib.extend_from_slice(&deflate(&raw));
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// 行のフィルター（PNG 1.2 の 6.6 の勧め）: 5 つを試し、差の絶対値の和がいちばん小さいものを選ぶ。
fn best_filter(row: &[u8], above: &[u8]) -> (u8, Vec<u8>) {
    const BPP: usize = 3;
    let left = |i: usize| if i >= BPP { row[i - BPP] } else { 0 };
    let upper_left = |i: usize| if i >= BPP { above[i - BPP] } else { 0 };
    let mut best: Option<(u64, u8, Vec<u8>)> = None;
    for filter in 0..=4u8 {
        let filtered: Vec<u8> = (0..row.len())
            .map(|i| {
                let predicted = match filter {
                    0 => 0,
                    1 => left(i),
                    2 => above[i],
                    3 => ((left(i) as u16 + above[i] as u16) / 2) as u8,
                    _ => paeth(left(i), above[i], upper_left(i)),
                };
                row[i].wrapping_sub(predicted)
            })
            .collect();
        let cost: u64 = filtered.iter().map(|&b| (b as i8).unsigned_abs() as u64).sum();
        if best.as_ref().is_none_or(|(c, _, _)| cost < *c) {
            best = Some((cost, filter, filtered));
        }
    }
    let (_, filter, filtered) = best.unwrap_or_default();
    (filter, filtered)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let crc = crc32(&png[start..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate;

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn the_pixels_come_back_from_the_image_data() {
        let (width, height) = (5, 3);
        let bgra: Vec<u8> = (0..width * height).flat_map(|i| [i as u8, 100, 200 - i as u8, 255]).collect();
        let png = encode(width, height, &bgra);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        // IDAT を取り出して戻し、フィルターを外して元の RGB と比べる。
        let idat_at = png.windows(4).position(|w| w == b"IDAT").unwrap();
        let length = u32::from_be_bytes(png[idat_at - 4..idat_at].try_into().unwrap()) as usize;
        let zlib = &png[idat_at + 4..idat_at + 4 + length];
        let raw = inflate(&zlib[2..zlib.len() - 4], (width * 3 + 1) * height).unwrap();
        let mut above = vec![0u8; width * 3];
        for y in 0..height {
            let line = &raw[y * (width * 3 + 1)..(y + 1) * (width * 3 + 1)];
            let mut row = vec![0u8; width * 3];
            for i in 0..width * 3 {
                let a = if i >= 3 { row[i - 3] } else { 0 };
                let c = if i >= 3 { above[i - 3] } else { 0 };
                let predicted = match line[0] {
                    0 => 0,
                    1 => a,
                    2 => above[i],
                    3 => ((a as u16 + above[i] as u16) / 2) as u8,
                    _ => paeth(a, above[i], c),
                };
                row[i] = line[1 + i].wrapping_add(predicted);
            }
            for x in 0..width {
                let p = &bgra[(y * width + x) * 4..];
                assert_eq!(&row[x * 3..x * 3 + 3], &[p[2], p[1], p[0]]);
            }
            above = row;
        }
    }
}
