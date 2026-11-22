//! ソースの色付けの、小さな字句解析（C#。逆コンパイルした C# と IL の行も、この形で読める）。
//!
//! 行ごとの字句の並びを返す。複数行にまたがるもの（`/* */`、`@"..."`、`"""..."""`）は、次の行に状態を持ち越す。
//! 正確な構文解析はしない（色を付けるだけ）。

/// 字句の種類（CSS の class に使う）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Type,
    String,
    Comment,
    Number,
    Preprocessor,
}

impl Kind {
    pub fn class(self) -> &'static str {
        match self {
            Kind::Plain => "",
            Kind::Keyword => "kw",
            Kind::Type => "ty",
            Kind::String => "st",
            Kind::Comment => "cm",
            Kind::Number => "nu",
            Kind::Preprocessor => "pp",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
}

const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "base", "break", "case", "catch", "checked", "class", "const", "continue", "default", "delegate", "do", "else", "enum", "event", "explicit",
    "extern", "false", "finally", "fixed", "for", "foreach", "get", "goto", "if", "implicit", "in", "init", "interface", "internal", "is", "lock", "namespace", "new", "null", "operator",
    "out", "override", "params", "partial", "private", "protected", "public", "readonly", "record", "ref", "return", "sealed", "set", "sizeof", "stackalloc", "static", "struct",
    "switch", "this", "throw", "true", "try", "typeof", "unchecked", "unsafe", "using", "value", "var", "virtual", "volatile", "when", "where", "while", "yield",
];

const TYPES: &[&str] = &["bool", "byte", "char", "decimal", "double", "dynamic", "float", "int", "long", "nint", "nuint", "object", "sbyte", "short", "string", "uint", "ulong", "ushort", "void"];

/// 行をまたぐ状態。
#[derive(Clone, Copy, PartialEq)]
enum Carry {
    None,
    BlockComment,
    /// `@"..."`（`""` が引用符 1 つ）。
    Verbatim,
    /// `"""..."""`。
    Raw,
}

/// ファイル全体を、行ごとの字句に分ける。
pub fn highlight(text: &str) -> Vec<Vec<Token>> {
    let mut carry = Carry::None;
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).map(|line| line_tokens(line, &mut carry)).collect()
}

fn line_tokens(line: &str, carry: &mut Carry) -> Vec<Token> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens: Vec<Token> = Vec::new();
    let mut push = |kind: Kind, text: String| {
        if text.is_empty() {
            return;
        }
        match tokens.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(&text),
            _ => tokens.push(Token { kind, text }),
        }
    };
    let mut i = 0;
    let take = |from: usize, to: usize| chars[from..to.min(chars.len())].iter().collect::<String>();
    // 前の行から続くもの。
    match *carry {
        Carry::BlockComment => {
            let end = find(&chars, 0, &['*', '/']);
            let to = end.map_or(chars.len(), |e| e + 2);
            push(Kind::Comment, take(0, to));
            if end.is_some() {
                *carry = Carry::None;
            }
            i = to;
        }
        Carry::Verbatim => {
            let (to, closed) = verbatim_end(&chars, 0);
            push(Kind::String, take(0, to));
            if closed {
                *carry = Carry::None;
            }
            i = to;
        }
        Carry::Raw => {
            let end = find(&chars, 0, &['"', '"', '"']);
            let to = end.map_or(chars.len(), |e| e + 3);
            push(Kind::String, take(0, to));
            if end.is_some() {
                *carry = Carry::None;
            }
            i = to;
        }
        Carry::None => {}
    }
    if i == 0 && chars.iter().find(|c| !c.is_whitespace()) == Some(&'#') {
        push(Kind::Preprocessor, line.to_string());
        return tokens;
    }
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            push(Kind::Comment, take(i, chars.len()));
            break;
        }
        if c == '/' && next == Some('*') {
            match find(&chars, i + 2, &['*', '/']) {
                Some(end) => {
                    push(Kind::Comment, take(i, end + 2));
                    i = end + 2;
                }
                None => {
                    push(Kind::Comment, take(i, chars.len()));
                    *carry = Carry::BlockComment;
                    break;
                }
            }
            continue;
        }
        // 文字列: `"`、`@"`、`$"`、`$@"`、`@$"`、`"""`。
        let prefix = chars[i..].iter().take_while(|c| matches!(c, '@' | '$')).count();
        if chars.get(i + prefix) == Some(&'"') && (prefix == 0 || c == '@' || c == '$') {
            let start = i;
            let quote = i + prefix;
            if chars.get(quote + 1) == Some(&'"') && chars.get(quote + 2) == Some(&'"') {
                match find(&chars, quote + 3, &['"', '"', '"']) {
                    Some(end) => {
                        push(Kind::String, take(start, end + 3));
                        i = end + 3;
                    }
                    None => {
                        push(Kind::String, take(start, chars.len()));
                        *carry = Carry::Raw;
                        i = chars.len();
                    }
                }
                continue;
            }
            if chars[start..quote].contains(&'@') {
                let (to, closed) = verbatim_end(&chars, quote + 1);
                push(Kind::String, take(start, to));
                if !closed {
                    *carry = Carry::Verbatim;
                }
                i = to;
            } else {
                let to = quoted_end(&chars, quote + 1, '"');
                push(Kind::String, take(start, to));
                i = to;
            }
            continue;
        }
        if c == '\'' {
            let to = quoted_end(&chars, i + 1, '\'');
            push(Kind::String, take(i, to));
            i = to;
            continue;
        }
        if c.is_ascii_digit() {
            let to = i + chars[i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == '.' || **c == '_').count();
            push(Kind::Number, take(i, to));
            i = to;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let to = i + chars[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').count();
            let word = take(i, to);
            let kind = if KEYWORDS.contains(&word.as_str()) {
                Kind::Keyword
            } else if TYPES.contains(&word.as_str()) {
                Kind::Type
            } else {
                Kind::Plain
            };
            push(kind, word);
            i = to;
            continue;
        }
        push(Kind::Plain, c.to_string());
        i += 1;
    }
    tokens
}

/// `from` から探して、`pattern` の始まりの位置。
fn find(chars: &[char], from: usize, pattern: &[char]) -> Option<usize> {
    (from..chars.len()).find(|&at| chars[at..].starts_with(pattern))
}

/// 引用符で閉じる文字列の終わり（閉じる引用符のあと）。`\` は次の 1 文字を飛ばす。閉じなければ行末。
fn quoted_end(chars: &[char], mut at: usize, quote: char) -> usize {
    while at < chars.len() {
        match chars[at] {
            '\\' => at += 2,
            c if c == quote => return at + 1,
            _ => at += 1,
        }
    }
    chars.len()
}

/// `@"..."` の終わり（`""` は引用符 1 つ）。閉じたか。
fn verbatim_end(chars: &[char], mut at: usize) -> (usize, bool) {
    while at < chars.len() {
        if chars[at] == '"' {
            if chars.get(at + 1) == Some(&'"') {
                at += 2;
                continue;
            }
            return (at + 1, true);
        }
        at += 1;
    }
    (chars.len(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(Kind, String)> {
        highlight(line).remove(0).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn keywords_types_strings_numbers_and_comments() {
        assert_eq!(
            kinds(r#"int x = 42; // c"#),
            vec![(Kind::Type, "int".into()), (Kind::Plain, " x = ".into()), (Kind::Number, "42".into()), (Kind::Plain, "; ".into()), (Kind::Comment, "// c".into())]
        );
        assert_eq!(kinds(r#"return "a\"b";"#)[2], (Kind::String, r#""a\"b""#.into()));
        assert_eq!(kinds(r#"var s = $"{x}";"#)[2], (Kind::String, r#"$"{x}""#.into()));
        assert_eq!(kinds("'\\''")[0], (Kind::String, "'\\''".into()));
    }

    #[test]
    fn block_comments_and_verbatim_strings_carry_over_lines() {
        let lines = highlight("a /* b\nc */ d\nx = @\"p\n\"\"q\"\" r\";");
        assert_eq!(lines[0][1], Token { kind: Kind::Comment, text: "/* b".into() });
        assert_eq!(lines[1][0], Token { kind: Kind::Comment, text: "c */".into() });
        assert_eq!(lines[1][1].kind, Kind::Plain);
        assert_eq!(lines[3][0], Token { kind: Kind::String, text: "\"\"q\"\" r\"".into() });
        assert_eq!(lines[3][1], Token { kind: Kind::Plain, text: ";".into() });
    }

    #[test]
    fn preprocessor_lines_and_crlf() {
        let lines = highlight("#region X\r\nint a;\r\n");
        assert_eq!(lines[0], vec![Token { kind: Kind::Preprocessor, text: "#region X".into() }]);
        assert_eq!(lines[1][0].text, "int");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn japanese_text_is_kept_whole() {
        assert_eq!(kinds("// 日本語").len(), 1);
        assert_eq!(kinds("名前 = 1")[0], (Kind::Plain, "名前 = ".into()));
    }
}
