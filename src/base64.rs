//! Base64（標準の文字の並び、`=` の詰め物あり）。MCP の画像、Edit and Continue の差分、成績書の画像（data URI）が使う。

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 に書く。
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let buffer = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(buffer >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 を読む（詰め物と空白はあってもなくてもよい）。読めない文字があれば `None`。
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = text.bytes().filter(|&c| c != b'=' && !c.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut buffer = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            buffer |= value(c)? << (18 - 6 * i);
        }
        out.push((buffer >> 16) as u8);
        if chunk.len() > 2 {
            out.push((buffer >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(buffer as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_matches_the_rfc_examples() {
        for text in ["", "M", "Ma", "Man", "日本語のソース"] {
            assert_eq!(decode(&encode(text.as_bytes())).unwrap(), text.as_bytes(), "{text}");
        }
        for (plain, encoded) in [("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(encode(plain.as_bytes()), encoded);
        }
        assert_eq!(encode(b"Ma"), "TWE=");
    }

    #[test]
    fn reads_with_and_without_padding() {
        assert_eq!(decode("TWFu").unwrap(), b"Man");
        assert_eq!(decode("TQ==").unwrap(), b"M");
        assert!(decode("T@==").is_none());
    }
}
