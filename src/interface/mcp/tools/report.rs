//! 成績書のツール（B3、D-081）。`test_item`: テスト項目を始める・手順を書く・画面を写す・終える。`test_report`: 成績書
//! （Excel と、1 ファイルで完結する HTML）を書き、そのパスを返す。中身はハブの記録（`interface::report`）が作る。

use super::{tool, Context, Outcome};
use serde_json::{json, Value};
use std::io;

pub(in crate::interface::mcp) fn tools() -> Vec<Value> {
    vec![
        tool(
            "test_item",
            "Record a test item of the program for the test report. action start (id like 1-2-3, title) begins an item: from \
             then on the item collects your ui_* actions and the tester's own clicks and typing (recorded automatically), the \
             SQL statements, dialogs and exceptions, and the time. Each ui_* action and click gets two pictures: the screen \
             before it with the control painted, and the screen after it. step adds a written test step (text), capture adds \
             a picture of the program's windows, end (result OK or NG, note) finishes it: it lists the controls whose text, \
             check, enabled or visible state changed since start, and rewrites the report files.",
            json!({
                "action": { "type": "string", "enum": ["start", "step", "capture", "end"] },
                "folder": { "type": "string", "description": "The folder (absolute) to write the reports into, as <program>-<time>\\ (default %LOCALAPPDATA%\\netdbg\\reports)." },
                "template": { "type": "string", "description": "A folder (absolute) with the site's own report.html / report.xlsx templates (see test_report exportTemplate)." },
                "id": { "type": "string", "description": "With start: the test item number." },
                "title": { "type": "string", "description": "With start: what the item tests." },
                "text": { "type": "string", "description": "With step: the test step." },
                "result": { "type": "string", "enum": ["OK", "NG"], "description": "With end: the result (default OK)." },
                "note": { "type": "string", "description": "With end: a remark." },
            }),
            &["action"],
        ),
        tool(
            "test_report",
            "Write the test report of the items recorded with test_item: an Excel workbook (.xlsx; the list sheet has one row per \
             item whose number links to the item's own sheet: one row per action with its pictures, and a link back; text too long for a cell continues on the next row) and one \
             self-contained HTML file (pictures embedded, no scripts or outside references; open it in a browser as is), and a steps file (.steps.json) that ui_replay can run again on another exe. \
             Returns the paths. folder sets the parent folder (a <program>-<time> folder is made in it), output the exact folder. \
             The layout follows templates: the built-in ones by default, or a site's own (template: a folder with report.html \
             and/or report.xlsx, marked with {{name}}, {{#list}}…{{/list}}). exportTemplate writes the built-in templates and the \
             list of names to a folder instead of writing the report.",
            json!({
                "folder": { "type": "string", "description": "The parent folder (absolute; default %LOCALAPPDATA%\\netdbg\\reports)." },
                "template": { "type": "string", "description": "A folder (absolute) with the site's own templates." },
                "exportTemplate": { "type": "string", "description": "Write the built-in templates to this folder (absolute) instead." },
                "output": { "type": "string", "description": "The exact folder (absolute) to write to." },
            }),
            &[],
        ),
    ]
}

/// このモジュールのツールなら処理する。
pub(in crate::interface::mcp) fn call(ctx: &mut Context, name: &str, arguments: &Value) -> io::Result<Option<Outcome>> {
    let (command, arguments) = match (name, arguments.get("exportTemplate")) {
        ("test_item", _) => ("netdbg.testItem", arguments.clone()),
        ("test_report", Some(output)) => ("netdbg.testTemplate", json!({ "output": output })),
        ("test_report", None) => ("netdbg.testReport", arguments.clone()),
        _ => return Ok(None),
    };
    Ok(Some(Outcome::Done(ctx.request(command, arguments)?)))
}
