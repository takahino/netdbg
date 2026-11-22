//! ZIP の読み書き（.xlsx の入れ物）。依存を増やさないよう自前で書く: 書くのは Deflate（`crate::deflate`）、読むのは
//! stored と Deflate（`crate::inflate`）。CRC32 は PNG と同じもの。ZIP64 と暗号は扱わない（Excel の .xlsx は使わない）。

/// 書く ZIP（Deflate で縮めた中身と、中央の目次）。
#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
    directory: Vec<u8>,
    count: u16,
}

impl Writer {
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let compressed = crate::deflate::deflate(data);
        let crc = crate::gui::png::crc32(data);
        let offset = self.bytes.len() as u32;
        // 日時は 1980-01-01 00:00（中身が同じなら、同じバイト列になる）。名前は UTF-8（フラグの 11 ビット目）。
        let header = |signature: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(signature.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes()); // 作った版
            }
            h.extend(20u16.to_le_bytes()); // 読むのに要る版
            h.extend(0x0800u16.to_le_bytes()); // 名前は UTF-8
            h.extend(8u16.to_le_bytes()); // Deflate
            h.extend(0u16.to_le_bytes()); // 時刻
            h.extend(0x0021u16.to_le_bytes()); // 日付 1980-01-01
            h.extend(crc.to_le_bytes());
            h.extend((compressed.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes()); // 付加の欄
            if central {
                h.extend(0u16.to_le_bytes()); // 注記
                h.extend(0u16.to_le_bytes()); // ディスクの番号
                h.extend(0u16.to_le_bytes()); // 中身の属性
                h.extend(0u32.to_le_bytes()); // 外の属性
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        self.bytes.extend(header(0x0403_4B50, false));
        self.bytes.extend(&compressed);
        self.directory.extend(header(0x0201_4B50, true));
        self.count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.bytes.len() as u32;
        let size = self.directory.len() as u32;
        self.bytes.extend(&self.directory);
        self.bytes.extend(0x0605_4B50u32.to_le_bytes());
        self.bytes.extend([0u8; 4]); // ディスクの番号
        self.bytes.extend(self.count.to_le_bytes());
        self.bytes.extend(self.count.to_le_bytes());
        self.bytes.extend(size.to_le_bytes());
        self.bytes.extend(start.to_le_bytes());
        self.bytes.extend(0u16.to_le_bytes());
        self.bytes
    }
}

fn u16_at(data: &[u8], at: usize) -> Result<usize, String> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(broken)
}

fn u32_at(data: &[u8], at: usize) -> Result<usize, String> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(broken)
}

fn broken() -> String {
    tr!("Cannot read it as ZIP (it is broken or not an .xlsx).")
}

/// ZIP の中身を、目次の順に読む（名前と、展開した中身）。
pub fn read(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // 終わりの印（注記があるので、後ろから探す）。
    let end = (0..zip.len().saturating_sub(21)).rev().take(65_557).find(|&at| zip[at..].starts_with(&0x0605_4B50u32.to_le_bytes())).ok_or_else(broken)?;
    let (count, start) = (u16_at(zip, end + 10)?, u32_at(zip, end + 16)?);
    let mut at = start;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(zip, at)? != 0x0201_4B50 {
            return Err(broken());
        }
        let (flags, method, compressed, size) = (u16_at(zip, at + 8)?, u16_at(zip, at + 10)?, u32_at(zip, at + 20)?, u32_at(zip, at + 24)?);
        let (name_length, extra_length, comment_length, offset) = (u16_at(zip, at + 28)?, u16_at(zip, at + 30)?, u16_at(zip, at + 32)?, u32_at(zip, at + 42)?);
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_length).ok_or_else(broken)?).into_owned();
        if flags & 1 != 0 {
            return Err(tr!("{name} is encrypted. Use an .xlsx without encryption.", name = name));
        }
        if u32_at(zip, offset)? != 0x0403_4B50 {
            return Err(broken());
        }
        let data_at = offset + 30 + u16_at(zip, offset + 26)? + u16_at(zip, offset + 28)?;
        let data = zip.get(data_at..data_at + compressed).ok_or_else(broken)?;
        let data = match method {
            0 => data.to_vec(),
            8 => crate::inflate::inflate(data, size).map_err(|e| tr!("Cannot expand {name}: {error}", name = name, error = e))?,
            other => return Err(tr!("The compression of {name} ({method}) cannot be read.", name = name, method = other)),
        };
        out.push((name, data));
        at += 46 + name_length + extra_length + comment_length;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back() {
        let mut zip = Writer::default();
        zip.add("a.xml", b"<a/>");
        zip.add("xl/media/日本語.png", &[0u8; 1000]);
        let entries = read(&zip.finish()).unwrap();
        assert_eq!(entries, vec![("a.xml".to_string(), b"<a/>".to_vec()), ("xl/media/日本語.png".to_string(), vec![0u8; 1000])]);
        assert!(read(b"not a zip at all, sorry").is_err());
    }
}
