//! 文字列のリテラル: ふつうの文字列、逐語的（@）、生（"""）、補間（$）と、その中の穴。

use super::*;

impl Parser<'_> {
    /// 読む位置が文字列の始まりなら、「逐語的（@）か、補間の `$` の数（補間でなければ 0）、引用符の数（生文字列は 3 以上）」。
    pub(super) fn string_start(&self) -> Option<(bool, usize, usize)> {
        let mut ahead = self.chars.clone();
        let (mut verbatim, mut interpolated) = (false, 0);
        loop {
            match ahead.peek() {
                Some('@') if !verbatim => verbatim = true,
                Some('$') => interpolated += 1,
                Some('"') => break,
                _ => return None,
            }
            ahead.next();
        }
        let quotes = ahead.clone().take_while(|c| *c == '"').count();
        // `""` は空の文字列。3 つ以上続けば、生文字列（その数の引用符で閉じる）。
        Some((verbatim, interpolated, if quotes >= 3 && !verbatim { quotes } else { 1 }))
    }

    /// 読む位置が `\uDC00`〜`\uDFFF`（下位サロゲート）なら、その値。
    pub(super) fn low_surrogate_follows(&self) -> Option<u16> {
        let mut ahead = self.chars.clone();
        if (ahead.next(), ahead.next()) != (Some('\\'), Some('u')) {
            return None;
        }
        let digits: String = ahead.take(4).collect();
        u16::from_str_radix(&digits, 16).ok().filter(|unit| (0xDC00..0xE000).contains(unit))
    }

    /// 文字列の中身を、閉じる引用符まで読む。補間文字列なら、`{式}` の部分を式として読む。
    pub(super) fn string_body(&mut self, verbatim: bool, dollars: usize, quotes: usize) -> Result<Vec<Piece>, String> {
        let raw = quotes >= 3;
        let interpolated = dollars > 0;
        let (mut pieces, mut text) = (Vec::new(), String::new());
        let mut lone: Vec<(usize, u16)> = Vec::new();
        loop {
            let Some(c) = self.chars.next() else { return Err(tr!("The string is not closed with '\"'.")) };
            match c {
                '"' if raw => {
                    // 生文字列は、開きと同じ数の引用符で閉じる。それより少ない並びは、中身。
                    let mut run = 1;
                    while run < quotes && self.chars.next_if_eq(&'"').is_some() {
                        run += 1;
                    }
                    if run == quotes {
                        break;
                    }
                    text.push_str(&"\"".repeat(run));
                }
                // 逐語的文字列の中の `""` は、引用符 1 つ。
                '"' if verbatim && self.chars.next_if_eq(&'"').is_some() => text.push('"'),
                '"' => break,
                '\\' if !verbatim && !raw => {
                    let unit = self.escape()?;
                    match char::from_u32(unit) {
                        Some(c) => text.push(c),
                        // 上位サロゲート（\ud83d）は、続く下位サロゲート（\udcac）と組にして 1 文字にする。
                        None if (0xD800..0xDC00).contains(&unit) && self.low_surrogate_follows().is_some() => {
                            let low = self.low_surrogate_follows().unwrap_or_default();
                            for _ in 0.."\\uXXXX".len() {
                                self.chars.next();
                            }
                            let pair = char::decode_utf16([unit as u16, low]).next().and_then(Result::ok).ok_or_else(|| tr!("The \\u pair in the string does not make a character."))?;
                            text.push(pair);
                        }
                        // 組にならないサロゲートは、Rust の文字列に入らない。場所と値を覚えておき、置き場には U+FFFD を入れる。
                        None if !interpolated => {
                            lone.push((text.chars().count(), unit as u16));
                            text.push('\u{FFFD}');
                        }
                        None => return Err(tr!("A \\u in the interpolated string is a value that is not a character on its own (a surrogate).")),
                    }
                }
                // 生の補間文字列（`$$"""…"""`）は、`$` の数だけ並んだ `{` が穴の始まり。それより前の `{` と、`}` は、そのままの文字。
                '{' if dollars > 1 => {
                    let mut run = 1;
                    while self.chars.next_if_eq(&'{').is_some() {
                        run += 1;
                    }
                    if run < dollars {
                        text.push_str(&"{".repeat(run));
                        continue;
                    }
                    text.push_str(&"{".repeat(run - dollars));
                    if !text.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut text)));
                    }
                    pieces.push(self.hole()?);
                    for _ in 1..dollars {
                        if self.chars.next_if_eq(&'}').is_none() {
                            return Err(tr!("A hole in the interpolated string is not closed with as many } as it was opened with."));
                        }
                    }
                }
                '}' if dollars > 1 => text.push('}'),
                '{' if interpolated && self.chars.next_if_eq(&'{').is_some() => text.push('{'),
                '}' if interpolated && self.chars.next_if_eq(&'}').is_some() => text.push('}'),
                '{' if interpolated => {
                    if !text.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut text)));
                    }
                    pieces.push(self.hole()?);
                }
                c => text.push(c),
            }
        }
        if !lone.is_empty() {
            // UTF-16 の符号単位の並びに直す。覚えておいた場所の U+FFFD を、もとのサロゲートに戻す。
            let mut units = Vec::new();
            for (index, c) in text.chars().enumerate() {
                match lone.iter().find(|(at, _)| *at == index) {
                    Some((_, unit)) => units.push(*unit),
                    None => units.extend(c.encode_utf16(&mut [0; 2]).iter()),
                }
            }
            self.wide = Some(units);
        }
        if !text.is_empty() || pieces.is_empty() {
            pieces.push(Piece::Text(text));
        }
        Ok(pieces)
    }

    /// 補間文字列の `{` のあと。`式`、`式,幅`、`式:書式`、`式,幅:書式`。
    pub(super) fn hole(&mut self) -> Result<Piece, String> {
        // 式の終わり（括弧の外の `,`、`:`、`}`）を探す。文字列や文字の中の記号は数えない。
        let (mut source, mut depth, mut quote) = (String::new(), 0usize, None::<char>);
        // 穴の中の文字列が逐語的（`@"…"`）か。その中では `\` は文字そのもので、`""` が引用符。
        let mut verbatim = false;
        let end = loop {
            let Some(c) = self.chars.next() else { return Err(tr!("A { in the interpolated string is not closed with }.")) };
            match (quote, c) {
                (Some(_), '\\') if !verbatim => {
                    source.push(c);
                    source.extend(self.chars.next());
                    continue;
                }
                (Some('"'), '"') if verbatim && self.chars.peek() == Some(&'"') => {
                    source.push(c);
                    source.extend(self.chars.next());
                    continue;
                }
                (Some(open), c) if c == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => {
                    verbatim = c == '"' && (source.ends_with('@') || source.ends_with("@$"));
                    quote = Some(c);
                }
                (None, '(' | '[' | '{') => depth += 1,
                (None, ')' | ']') => depth = depth.saturating_sub(1),
                (None, '}') if depth > 0 => depth -= 1,
                (None, ',' | ':' | '}') if depth == 0 => break c,
                _ => {}
            }
            source.push(c);
        };
        let expr = parse(&source)?;
        let until = |parser: &mut Self, stops: &[char]| -> Result<(String, char), String> {
            let mut part = String::new();
            loop {
                match parser.chars.next() {
                    None => return Err(tr!("A { in the interpolated string is not closed with }.")),
                    Some(c) if stops.contains(&c) => return Ok((part, c)),
                    Some(c) => part.push(c),
                }
            }
        };
        let (mut alignment, mut format, mut last) = (None, None, end);
        if last == ',' {
            let (part, stop) = until(self, &[':', '}'])?;
            alignment = Some(part.trim().to_string());
            last = stop;
        }
        if last == ':' {
            format = Some(until(self, &['}'])?.0);
        }
        Ok(Piece::Hole { expr, alignment, format })
    }

    /// 文字列や文字の中の `\` に続く部分を読んで、文字の番号にする。
    pub(super) fn escape(&mut self) -> Result<u32, String> {
        let hex = |parser: &mut Self, least: usize, most: usize| -> Result<u32, String> {
            let mut digits = String::new();
            while digits.len() < most {
                match parser.chars.next_if(|c| c.is_ascii_hexdigit()) {
                    Some(c) => digits.push(c),
                    None => break,
                }
            }
            if digits.len() < least {
                return Err(tr!("Not enough hexadecimal digits after \\ ({count} are needed).", count = least));
            }
            u32::from_str_radix(&digits, 16).map_err(|_| tr!("Cannot read {digits} after \\.", digits = digits))
        };
        Ok(match self.chars.next() {
            Some('n') => '\n' as u32,
            Some('r') => '\r' as u32,
            Some('t') => '\t' as u32,
            Some('0') => 0,
            Some('a') => 0x07,
            Some('b') => 0x08,
            Some('f') => 0x0C,
            Some('v') => 0x0B,
            Some(c @ ('\\' | '"' | '\'')) => c as u32,
            Some('u') => hex(self, 4, 4)?,
            Some('U') => hex(self, 8, 8)?,
            // \x は、1 桁から 4 桁。
            Some('x') => hex(self, 1, 4)?,
            other => return Err(tr!("There is no escape \\{escape}.", escape = other.map(String::from).unwrap_or_default())),
        })
    }
}
