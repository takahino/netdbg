//! 止まったときに、ソースの上に変数の値を出す（Visual Studio のインライン値とデータヒント）。
//!
//! - 行の字句を、名前（とその前に `.` で続く名前を含めた式。`resultLabel.Text`）と、それ以外に分ける（`segments`）。
//!   マウスを載せた名前は、その式を評価する。
//! - 止まったメソッドの最初の行から止まった行までの各行に、その行に出てくるローカル変数の値を `// 名前 = 値` で
//!   添える（`inline_values`）。止まった行より下は、まだ通っていないので出さない。

use super::csharp::{Kind, Token};
use std::collections::HashMap;

/// 描く字句 1 つ。名前なら、マウスを載せたときに評価する式。
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub kind: Kind,
    pub text: String,
    pub expression: Option<String>,
}

/// 1 つの値の長さと、1 行に添える文字の長さの上限。
const VALUE_LENGTH: usize = 60;
const LINE_LENGTH: usize = 160;

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 行の字句を、名前とそれ以外に分ける。名前には、前に `.` で続く名前を含めた式を付ける（`this.a.b`）。
pub fn segments(tokens: &[Token]) -> Vec<Segment> {
    let mut out = Vec::new();
    // いま続いている式と、直前が `.` だったか。
    let mut chain: Option<String> = None;
    let mut after_dot = false;
    for token in tokens {
        match token.kind {
            Kind::Plain => {
                let chars: Vec<char> = token.text.chars().collect();
                let mut i = 0;
                let mut other = String::new();
                while i < chars.len() {
                    if is_name_start(chars[i]) {
                        if !other.is_empty() {
                            out.push(Segment { kind: Kind::Plain, text: std::mem::take(&mut other), expression: None });
                        }
                        let end = i + chars[i..].iter().take_while(|c| is_name_part(**c)).count();
                        let name: String = chars[i..end].iter().collect();
                        let expression = match (&chain, after_dot) {
                            (Some(chain), true) => format!("{chain}.{name}"),
                            _ => name.clone(),
                        };
                        chain = Some(expression.clone());
                        after_dot = false;
                        out.push(Segment { kind: Kind::Plain, text: name, expression: Some(expression) });
                        i = end;
                        continue;
                    }
                    match chars[i] {
                        '.' if chain.is_some() => after_dot = true,
                        _ => {
                            chain = None;
                            after_dot = false;
                        }
                    }
                    other.push(chars[i]);
                    i += 1;
                }
                if !other.is_empty() {
                    out.push(Segment { kind: Kind::Plain, text: other, expression: None });
                }
            }
            // `this.x`、`base.x` は式の始まりになる。
            Kind::Keyword if token.text == "this" || token.text == "base" => {
                chain = Some(token.text.clone());
                after_dot = false;
                out.push(Segment { kind: token.kind, text: token.text.clone(), expression: Some(token.text.clone()) });
            }
            _ => {
                chain = None;
                after_dot = false;
                out.push(Segment { kind: token.kind, text: token.text.clone(), expression: None });
            }
        }
    }
    out
}

fn shorten(text: &str, length: usize) -> String {
    if text.chars().count() <= length {
        return text.to_string();
    }
    text.chars().take(length).collect::<String>() + "…"
}

/// 行（1 始まり）ごとに添える文字（`a = 1, text = "x"`）。`first` から `current` までの行で、`values`（名前と値）に
/// ある名前を、出てきた順に 1 回ずつ。
pub fn inline_values(lines: &[Vec<Token>], first: u32, current: u32, values: &HashMap<String, String>) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if values.is_empty() || first == 0 || current < first {
        return out;
    }
    for number in first..=current {
        let Some(tokens) = lines.get(number as usize - 1) else { break };
        let mut seen: Vec<String> = Vec::new();
        let mut parts = Vec::new();
        for segment in segments(tokens) {
            // 単独の名前だけ（`a.b` の `b` は、ローカル変数ではない）。
            let Some(expression) = &segment.expression else { continue };
            if expression.contains('.') || seen.contains(expression) {
                continue;
            }
            if let Some(value) = values.get(expression) {
                parts.push(format!("{expression} = {}", shorten(value, VALUE_LENGTH)));
                seen.push(expression.clone());
            }
        }
        if !parts.is_empty() {
            out.insert(number, shorten(&parts.join(", "), LINE_LENGTH));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::csharp::highlight;

    fn expressions(line: &str) -> Vec<String> {
        segments(&highlight(line)[0]).into_iter().filter_map(|s| s.expression).collect()
    }

    #[test]
    fn names_carry_the_member_chain_before_them() {
        assert_eq!(expressions("resultLabel.Text = text.Trim();"), ["resultLabel", "resultLabel.Text", "text", "text.Trim"]);
        assert_eq!(expressions("this.count += order.Lines[0].Price;"), ["this", "this.count", "order", "order.Lines", "Price"]);
        assert_eq!(expressions("var s = \"a.b\" + x;"), ["s", "x"]);
    }

    #[test]
    fn segments_keep_the_whole_text() {
        let line = "if (a.B > 1) // c";
        let joined: String = segments(&highlight(line)[0]).into_iter().map(|s| s.text).collect();
        assert_eq!(joined, line);
    }

    #[test]
    fn values_are_added_from_the_method_start_to_the_stopped_line() {
        let lines = highlight("void M(int a)\n{\n    int b = a + 1;\n    var t = b.ToString() + a;\n    Use(t);\n}");
        let values: HashMap<String, String> = [("a", "3"), ("b", "4"), ("t", "\"43\"")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let shown = inline_values(&lines, 1, 4, &values);
        assert_eq!(shown.get(&1).map(String::as_str), Some("a = 3"));
        assert_eq!(shown.get(&3).map(String::as_str), Some("b = 4, a = 3"));
        assert_eq!(shown.get(&4).map(String::as_str), Some("t = \"43\", b = 4, a = 3"));
        assert_eq!(shown.get(&5), None, "lines below the stopped line are not run yet");
    }
}
