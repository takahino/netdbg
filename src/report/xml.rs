//! 小さな XML の木（.xlsx の部品を読んで直して書き戻す）。要素、属性、文字、そのほか（宣言、注記、CDATA）を持つ。
//! 文字と属性の値は、逃がした形のまま持つ（触らない所は、読んだとおりに書き戻す）。読むときだけ `unescape` で戻す。

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    /// 逃がした形の文字。
    Text(String),
    /// `<?…?>`、`<!--…-->`、`<![CDATA[…]]>`、`<!…>`（そのまま書き戻す）。
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    /// 名前と、逃がした形の値。
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn new(name: &str) -> Element {
        Element { name: name.to_string(), ..Element::default() }
    }

    /// 属性の値（戻した形）。
    pub fn attribute(&self, name: &str) -> Option<String> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| unescape(value))
    }

    /// 属性を置く（値は逃がして持つ）。
    pub fn set(&mut self, name: &str, value: &str) -> &mut Self {
        let value = super::escape(value);
        match self.attributes.iter_mut().find(|(key, _)| key == name) {
            Some(entry) => entry.1 = value,
            None => self.attributes.push((name.to_string(), value)),
        }
        self
    }

    pub fn remove(&mut self, name: &str) {
        self.attributes.retain(|(key, _)| key != name);
    }

    /// 子の要素。
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| if let Node::Element(e) = node { Some(e) } else { None })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|node| if let Node::Element(e) = node { Some(e) } else { None })
    }


    /// 子孫の文字をつないだもの（戻した形）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for node in &self.children {
            match node {
                Node::Text(text) => out.push_str(&unescape(text)),
                Node::Element(element) => out.push_str(&element.text()),
                Node::Other(other) => {
                    if let Some(data) = other.strip_prefix("<![CDATA[").and_then(|o| o.strip_suffix("]]>")) {
                        out.push_str(data);
                    }
                }
            }
        }
        out
    }

    /// 子の要素を、`before` に挙げた名前のどれかより前に入れる（無ければ最後に）。部品の並びに決まりがある所で使う。
    pub fn insert_before(&mut self, element: Element, before: &[&str]) {
        let at = self.children.iter().position(|node| matches!(node, Node::Element(e) if before.contains(&e.name.as_str()))).unwrap_or(self.children.len());
        self.children.insert(at, Node::Element(element));
    }
}

/// 逃がした文字を戻す（名前の付いた 5 つと、数の参照）。
pub fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else { break };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn malformed(at: usize) -> String {
    tr!("Cannot read the XML (around byte {at}).", at = at)
}

/// XML を読む。いちばん外の並び（宣言と、根の要素）を返す。
pub fn parse(text: &str) -> Result<Vec<Node>, String> {
    let mut stack: Vec<Element> = Vec::new();
    let mut top: Vec<Node> = Vec::new();
    let push = |stack: &mut Vec<Element>, top: &mut Vec<Node>, node: Node| match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => top.push(node),
    };
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            push(&mut stack, &mut top, Node::Text(rest[..end].to_string()));
            at += end;
            continue;
        }
        let close = |marker: &str| rest.find(marker).map(|i| i + marker.len()).ok_or_else(|| malformed(at));
        if rest.starts_with("<?") || rest.starts_with("<!--") || rest.starts_with("<![CDATA[") || rest.starts_with("<!") {
            let end = if rest.starts_with("<?") {
                close("?>")?
            } else if rest.starts_with("<!--") {
                close("-->")?
            } else if rest.starts_with("<![CDATA[") {
                close("]]>")?
            } else {
                close(">")?
            };
            push(&mut stack, &mut top, Node::Other(rest[..end].to_string()));
            at += end;
            continue;
        }
        if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>').ok_or_else(|| malformed(at))?;
            let name = after[..end].trim();
            let element = stack.pop().ok_or_else(|| malformed(at))?;
            if element.name != name {
                return Err(malformed(at));
            }
            push(&mut stack, &mut top, Node::Element(element));
            at += 2 + end + 1;
            continue;
        }
        // 開く要素: 名前、属性、/> か >。
        let mut i = 1;
        let bytes = rest.as_bytes();
        while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') {
            i += 1;
        }
        let mut element = Element::new(&rest[1..i]);
        loop {
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            match bytes.get(i) {
                Some(b'/') if bytes.get(i + 1) == Some(&b'>') => {
                    push(&mut stack, &mut top, Node::Element(element));
                    at += i + 2;
                    break;
                }
                Some(b'>') => {
                    stack.push(element);
                    at += i + 1;
                    break;
                }
                Some(_) => {
                    let equals = rest[i..].find('=').ok_or_else(|| malformed(at + i))? + i;
                    let name = rest[i..equals].trim().to_string();
                    let mut j = equals + 1;
                    while matches!(bytes.get(j), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                        j += 1;
                    }
                    let quote = *bytes.get(j).ok_or_else(|| malformed(at + j))?;
                    if quote != b'"' && quote != b'\'' {
                        return Err(malformed(at + j));
                    }
                    let value_end = rest[j + 1..].find(quote as char).ok_or_else(|| malformed(at + j))? + j + 1;
                    // 一重引用符の値は、二重引用符で書き戻せるように逃がしておく。
                    let value = rest[j + 1..value_end].replace('"', "&quot;");
                    element.attributes.push((name, value));
                    i = value_end + 1;
                }
                None => return Err(malformed(at + i)),
            }
        }
    }
    if !stack.is_empty() {
        return Err(tr!("An XML element is not closed."));
    }
    Ok(top)
}

/// 読んだ木を、XML に書く。
pub fn write(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        write_node(&mut out, node);
    }
    out
}

fn write_node(out: &mut String, node: &Node) {
    match node {
        Node::Text(text) | Node::Other(text) => out.push_str(text),
        Node::Element(element) => {
            out.push('<');
            out.push_str(&element.name);
            for (name, value) in &element.attributes {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(value);
                out.push('"');
            }
            if element.children.is_empty() {
                out.push_str("/>");
                return;
            }
            out.push('>');
            for child in &element.children {
                write_node(out, child);
            }
            out.push_str("</");
            out.push_str(&element.name);
            out.push('>');
        }
    }
}

/// 根の要素（宣言などの後ろ）。
pub fn root(nodes: &mut [Node]) -> Result<&mut Element, String> {
    nodes.iter_mut().find_map(|node| if let Node::Element(e) = node { Some(e) } else { None }).ok_or_else(|| tr!("The XML has no element."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_read_writes_back_the_same() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<worksheet xmlns=\"urn:x\" a='1 \"q\"'><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c></row></sheetData><!-- note --><x/></worksheet>";
        let nodes = parse(text).unwrap();
        assert_eq!(write(&nodes), text.replace("a='1 \"q\"'", "a=\"1 &quot;q&quot;\""));
    }

    #[test]
    fn text_and_attributes_are_unescaped_when_read() {
        let mut nodes = parse("<si><r><t>{{番号}} &amp; </t></r><r><t xml:space=\"preserve\">&lt;x&gt;&#12354;&#x3042;</t></r></si>").unwrap();
        let si = root(&mut nodes).unwrap();
        assert_eq!(si.text(), "{{番号}} & <x>ああ");
        si.set("v", "a\"b<");
        assert_eq!(si.attribute("v").as_deref(), Some("a\"b<"));
        assert!(parse("<a><b></a>").is_err());
    }
}
