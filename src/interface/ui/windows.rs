//! 画面だけが使う、対象の窓の木（`GET /ui/tree`）と写し（`GET /ui/capture`）。
//!
//! MCP の `ui_tree` / `ui_screenshot` と同じ `gui` の関数を、GUI のスレッド（`gui::spawn`。座標が物理ピクセル）で呼ぶ。
//! 入力のループは通さない（本体のセッションに触らない）。MCP の印や名前の覚え（`UiMemory`）は使わない（AI の印を
//! 画面の操作で変えない）。止まっている対象はメッセージに答えないので、名前と文字は読まない（`running` が false）。

use crate::gui;
use serde_json::{json, Value};
use std::time::Duration;

/// 木に出すコントロールの数の上限。
const MAX_ELEMENTS: usize = 3000;
/// GUI のスレッドの答えを待つ上限。
const TIMEOUT: Duration = Duration::from_secs(15);

fn on_gui_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    gui::on_gui_thread(work, TIMEOUT)
}

/// プロセスの窓と、その中のコントロール（木の順、深さつき）。座標は、窓の写しの左上からの物理ピクセル（`inWindow`）。
pub fn tree(process_id: u32, running: bool) -> Result<Value, String> {
    on_gui_thread(move || {
        let snapshot = gui::snapshot(process_id, running, MAX_ELEMENTS);
        let mut windows = Vec::new();
        let mut elements = Vec::new();
        for (mark, element) in snapshot.elements.iter().enumerate() {
            let rect = gui::capture::visible_rect(element.window);
            if element.depth == 0 {
                windows.push(json!({ "id": element.window, "title": element.text, "rect": gui::rect_json(rect) }));
            }
            let mut described = element.describe(mark + 1, rect);
            described["window"] = json!(element.window);
            elements.push(described);
        }
        let unread: Vec<Value> = snapshot.unread.iter().map(|(window, reason)| json!({ "window": window, "reason": reason })).collect();
        json!({ "windows": windows, "elements": elements, "unread": unread, "running": running })
    })
}

/// トップレベルの窓 1 つの写し（PNG）。止まっている窓も、最後の描画を写す。
pub fn capture(window: usize) -> Result<Vec<u8>, String> {
    on_gui_thread(move || {
        let scale = gui::dpi_scales(window).0;
        gui::capture::capture(&[window], None, &[], scale).map(|shot| shot.png)
    })?
}
