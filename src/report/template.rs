//! 成績書のテンプレートの小さな言語（現場ごとの書式。D-083）。Mustache に似た形で、HTML のテンプレートにも、Excel の
//! テンプレートのセルにも使う:
//! - `{{名前}}`: 値。画像は、HTML では押すと広がる縮小、Excel ではそのセルに貼る画像。`{{名前:uri}}` は画像の data URI。
//! - `{{#名前}}…{{/名前}}`: 一覧なら要素ごとに繰り返す（中では要素の名前も使える）。一覧でなければ、値があるときに 1 回。
//! - `{{^名前}}…{{/名前}}`: 値が無いとき（空の一覧、空の文字、偽）だけ。
//!
//! 名前は内側（一覧の要素）から外側へ順に探す。知らない名前は誤りにする（打ち間違いが黙って消えないように）。

use super::Picture;

/// テンプレートに渡す値。
#[derive(Debug, Clone)]
pub enum Value<'a> {
    Text(String),
    Number(f64),
    Bool(bool),
    List(Vec<Scope<'a>>),
    Image(&'a Picture),
    /// 成績書の中の行き先へのリンク（HTML は頁の中の印、Excel はシートのセル）。
    Link { text: String, target: Target },
}

/// リンクの行き先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// 項目 n（0 始まり）の頁・シート。
    Item(usize),
    /// 一覧の、項目 n の所。
    List(usize),
}

impl Value<'_> {
    /// 値があるか（`{{#…}}` で書くか）。
    pub fn truthy(&self) -> bool {
        match self {
            Value::Text(text) => !text.is_empty(),
            Value::Bool(value) => *value,
            Value::List(items) => !items.is_empty(),
            Value::Number(_) | Value::Image(_) | Value::Link { .. } => true,
        }
    }

    /// 文字にしたもの（画像は説明、リンクは見せる文字）。
    pub fn text(&self) -> String {
        match self {
            Value::Text(text) => text.clone(),
            Value::Number(value) => number_text(*value),
            Value::Bool(_) | Value::List(_) => String::new(),
            Value::Image(picture) => picture.caption.clone(),
            Value::Link { text, .. } => text.clone(),
        }
    }
}

/// 数の書き方（整数は小数点なし）。
pub fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { format!("{value}") }
}

/// 名前と値の組（一覧の要素 1 つ、または全体）。
#[derive(Debug, Clone, Default)]
pub struct Scope<'a>(Vec<(&'static str, Value<'a>)>);

impl<'a> Scope<'a> {
    pub fn set(&mut self, name: &'static str, value: Value<'a>) -> &mut Self {
        self.0.push((name, value));
        self
    }

    pub fn text(&mut self, name: &'static str, value: impl Into<String>) -> &mut Self {
        self.set(name, Value::Text(value.into()))
    }

    /// 名前の値。日本語の別名（`番号` など）は、英語の名前で探す（`data::ALIASES`）。
    pub fn get(&self, name: &str) -> Option<&Value<'a>> {
        let name = super::data::canonical(name);
        self.0.iter().find(|(key, _)| *key == name).map(|(_, value)| value)
    }

    /// 名前の一覧（試験で、説明の一覧と比べる）。
    #[cfg(test)]
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.iter().map(|(key, _)| *key)
    }
}

/// 内側から外側への並び（いちばん内側が最後）。
pub type Scopes<'s, 'a> = Vec<&'s Scope<'a>>;

/// 名前を、内側から外側へ探す。
pub fn lookup<'s, 'a>(scopes: &[&'s Scope<'a>], name: &str) -> Result<&'s Value<'a>, String> {
    scopes.iter().rev().find_map(|scope| scope.get(name)).ok_or_else(|| tr!("The template's {{{{{name}}}}} is not known (see the names file exported with the templates for the names you can use).", name = name))
}

/// テンプレートを読んだもの。
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Text(String),
    Var { name: String, modifier: Option<String> },
    Section { name: String, inverted: bool, children: Vec<Node> },
}

/// テンプレートを読む。閉じていない印や、合わない `{{/…}}` は、行の番号を付けて誤りにする。
pub fn parse(template: &str) -> Result<Vec<Node>, String> {
    // 開いている区切り（名前、反対か、始まりの行、それまでの中身）。
    let mut stack: Vec<(String, bool, usize, Vec<Node>)> = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    let mut rest = template;
    let line_of = |rest: &str| template[..template.len() - rest.len()].matches('\n').count() + 1;
    while let Some(start) = rest.find("{{") {
        if start > 0 {
            current.push(Node::Text(rest[..start].to_string()));
        }
        let line = line_of(&rest[start..]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or_else(|| tr!("The {{{{ on line {line} is not closed with }}}}.", line = line))?;
        let tag = after[..end].trim();
        rest = &after[end + 2..];
        match tag.chars().next() {
            Some(kind @ ('#' | '^')) => {
                let name = tag[1..].trim().to_string();
                stack.push((name, kind == '^', line, std::mem::take(&mut current)));
            }
            Some('/') => {
                let name = tag[1..].trim();
                let (open, inverted, _, outer) = stack.pop().ok_or_else(|| tr!("The {{{{/{name}}}}} on line {line} has no matching {{{{#{name}}}}}.", line = line, name = name))?;
                // 開きと閉じは、別名でもよい（{{#項目}}…{{/items}}）。
                if super::data::canonical(&open) != super::data::canonical(name) {
                    return Err(tr!("The {{{{/{name}}}}} on line {line} does not close {{{{#{open}}}}} (the nesting order is wrong).", line = line, name = name, open = open));
                }
                let children = std::mem::replace(&mut current, outer);
                current.push(Node::Section { name: open, inverted, children });
            }
            Some(_) => {
                let (name, modifier) = match tag.split_once(':') {
                    Some((name, modifier)) => (name.trim().to_string(), Some(modifier.trim().to_string())),
                    None => (tag.to_string(), None),
                };
                current.push(Node::Var { name, modifier });
            }
            None => return Err(tr!("Line {line} has a {{{{}}}} without a name.", line = line)),
        }
    }
    if !rest.is_empty() {
        current.push(Node::Text(rest.to_string()));
    }
    if let Some((name, _, line, _)) = stack.pop() {
        return Err(tr!("The {{{{#{name}}}}} on line {line} is not closed with {{{{/{name}}}}}.", line = line, name = name));
    }
    Ok(current)
}

/// 書き出す先（HTML は値を逃がし、Excel のセルは文字のまま）。
pub trait Sink {
    fn text(&mut self, text: &str);
    fn value(&mut self, value: &Value, modifier: Option<&str>) -> Result<(), String>;
}

/// `nodes` を、`scopes` の値で書く。
pub fn render(nodes: &[Node], scopes: &mut Scopes, sink: &mut dyn Sink) -> Result<(), String> {
    for node in nodes {
        match node {
            Node::Text(text) => sink.text(text),
            Node::Var { name, modifier } => sink.value(lookup(scopes, name)?, modifier.as_deref())?,
            Node::Section { name, inverted, children } => {
                let value = lookup(scopes, name)?;
                match (value, inverted) {
                    (Value::List(items), false) => {
                        for item in items {
                            scopes.push(item);
                            let result = render(children, scopes, sink);
                            scopes.pop();
                            result?;
                        }
                    }
                    (value, inverted) if value.truthy() != *inverted => render(children, scopes, sink)?,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// 文字だけを書く先（Excel のセル、シートの名前）。
#[derive(Default)]
pub struct Plain(pub String);

impl Sink for Plain {
    fn text(&mut self, text: &str) {
        self.0.push_str(text);
    }

    fn value(&mut self, value: &Value, _: Option<&str>) -> Result<(), String> {
        self.0.push_str(&value.text());
        Ok(())
    }
}

/// 文字のテンプレートを、値で埋めた文字にする。
pub fn fill_text(template: &str, scopes: &mut Scopes) -> Result<String, String> {
    let nodes = parse(template)?;
    let mut plain = Plain::default();
    render(&nodes, scopes, &mut plain)?;
    Ok(plain.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root<'a>() -> Scope<'a> {
        let mut first = Scope::default();
        first.text("id", "1-1").set("index", Value::Number(1.0));
        let mut second = Scope::default();
        second.text("id", "1-2").set("index", Value::Number(2.0));
        let mut root = Scope::default();
        root.text("名前", "成績書").set("items", Value::List(vec![first, second])).set("空", Value::List(Vec::new())).set("偽", Value::Bool(false));
        root
    }

    #[test]
    fn lists_repeat_and_names_are_found_from_inside_out() {
        let root = root();
        let text = fill_text("{{名前}}: {{#項目}}[{{連番}} {{番号}} {{名前}}]{{/項目}}", &mut vec![&root]).unwrap();
        assert_eq!(text, "成績書: [1 1-1 成績書][2 1-2 成績書]");
    }

    #[test]
    fn inverted_sections_show_only_when_empty() {
        let root = root();
        assert_eq!(fill_text("{{^空}}なし{{/空}}{{#空}}あり{{/空}}{{^偽}}偽{{/偽}}{{#名前}}有{{/名前}}", &mut vec![&root]).unwrap(), "なし偽有");
    }

    #[test]
    fn mistakes_are_reported_with_the_line() {
        let root = root();
        assert!(fill_text("{{番号違い}}", &mut vec![&root]).unwrap_err().contains("{{番号違い}} が分かりません"));
        assert!(parse("a\n{{#項目}}x").unwrap_err().starts_with("2 行目の {{#項目}} が {{/項目}} で閉じていません"));
        assert!(parse("{{#項目}}{{/空}}").unwrap_err().contains("入れ子の順が違います"));
        assert!(parse("{{名前").unwrap_err().contains("閉じていません"));
    }

    #[test]
    fn numbers_are_written_without_needless_decimals() {
        assert_eq!((number_text(4.0), number_text(17.26)), ("4".to_string(), "17.26".to_string()));
    }
}
