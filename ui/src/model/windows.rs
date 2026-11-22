//! 対象の画面の木（エンジンの `GET /ui/tree` の答え）。選んだコントロールの枠を、窓の写しの上に描く位置も計る。

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn rect(value: &Value) -> Rect {
    let n = |k: &str| value[k].as_f64().unwrap_or(0.0);
    Rect { x: n("x"), y: n("y"), width: n("width"), height: n("height") }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub mark: u64,
    pub id: u64,
    pub window: u64,
    pub depth: usize,
    pub kind: String,
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub text: Option<String>,
    pub enabled: bool,
    /// 窓の写しの左上からの位置（物理ピクセル）。
    pub in_window: Rect,
}

impl Element {
    /// 木に出す 1 行の文字（種類、名前、文字）。
    pub fn label(&self) -> String {
        let mut label = self.type_name.clone().unwrap_or_else(|| self.kind.clone());
        if let Some(name) = &self.name {
            label.push_str(&format!(" {name}"));
        }
        if let Some(text) = self.text.as_ref().filter(|t| !t.is_empty()) {
            let short: String = text.chars().take(40).collect();
            label.push_str(&format!(" \u{201C}{short}\u{201D}"));
        }
        label
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub windows: Vec<Window>,
    pub elements: Vec<Element>,
    /// 動いている間に読んだか（止まっている間は、名前と文字が無い）。
    pub running: bool,
}

pub fn parse(body: &Value) -> Tree {
    let windows = body["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| Window { id: w["id"].as_u64().unwrap_or(0), title: w["title"].as_str().unwrap_or("").to_string(), rect: rect(&w["rect"]) })
        .collect();
    let text = |v: &Value| v.as_str().map(String::from);
    let elements = body["elements"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Element {
            mark: e["mark"].as_u64().unwrap_or(0),
            id: e["id"].as_u64().unwrap_or(0),
            window: e["window"].as_u64().unwrap_or(0),
            depth: e["depth"].as_u64().unwrap_or(0) as usize,
            kind: e["kind"].as_str().unwrap_or("").to_string(),
            name: text(&e["name"]),
            type_name: text(&e["type"]),
            text: text(&e["text"]),
            enabled: e["enabled"].as_bool().unwrap_or(true),
            in_window: rect(&e["inWindow"]),
        })
        .collect();
    Tree { windows, elements, running: body["running"].as_bool().unwrap_or(false) }
}

impl Tree {
    pub fn window(&self, id: u64) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// 選んだコントロールの枠の、窓の写しに対する割合（CSS の % にする。写しを縮めて見せても合う）。
    pub fn overlay(&self, element: &Element) -> Option<Rect> {
        let window = self.window(element.window)?;
        let (w, h) = (window.rect.width, window.rect.height);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let r = &element.in_window;
        Some(Rect { x: r.x / w * 100.0, y: r.y / h * 100.0, width: r.width / w * 100.0, height: r.height / h * 100.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_control_is_placed_on_its_window_in_percent() {
        let tree = parse(&json!({
            "running": true,
            "windows": [{ "id": 10, "title": "Main", "rect": { "x": 100, "y": 100, "width": 400, "height": 200 } }],
            "elements": [
                { "mark": 1, "id": 10, "window": 10, "kind": "Form", "text": "Main", "inWindow": { "x": 0, "y": 0, "width": 400, "height": 200 } },
                { "mark": 2, "id": 11, "window": 10, "depth": 1, "kind": "Button", "type": "System.Windows.Forms.Button", "name": "okButton", "text": "OK", "inWindow": { "x": 40, "y": 20, "width": 100, "height": 50 } }
            ]
        }));
        assert_eq!(tree.elements[1].label(), "System.Windows.Forms.Button okButton \u{201C}OK\u{201D}");
        assert_eq!(tree.overlay(&tree.elements[1]), Some(Rect { x: 10.0, y: 10.0, width: 25.0, height: 25.0 }));
        assert_eq!(tree.elements[0].label(), "Form \u{201C}Main\u{201D}");
    }
}
