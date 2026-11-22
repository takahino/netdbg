//! 成績書の記録（B3、D-081、D-082）。ハブが持つ（専用の画面は対象が終わるたびにエンジンを作り直すので、項目をまたいで残す）。
//!
//! 要求 `netdbg.testItem` / `netdbg.testReport` は、DAP のクライアント（画面）からも MCP からも、ここで受ける
//! （エンジンには渡さない）。材料は既存の口から集める:
//! - SQL、ダイアログ、例外: エンジンの出来事の列（`netdbg.journal`）の、項目を始めたあとの分。
//! - 画面の写しと木: `gui`（`ui_screenshot` / `ui_tree` と同じ）。
//! - 操作: AI の操作の知らせ（`netdbg.agentAction`）と、人の手の操作（`gui::recorder`）。
//! - exe: いまのプログラムのファイルの版と更新日時。
//!
//! 操作ごとの画面（D-082）: AI と手の操作には、2 枚を付ける。
//! - 操作の前の画面に、操作した箇所を塗ったもの。AI は操作を始める前に写す。手は、操作はもう済んでいるので、前の
//!   操作のあとに写した画面を使う（その画面に、操作した窓が写っていなければ塗らない）。
//! - 操作のあとの画面。画面が変わり終えるのを少し待ってから写す（待つ間に次の操作が来たら、その前に写す）。

use crate::debugger::engine::Engine;
use crate::debugger::session::{Event, Owner};
use crate::gui;
use crate::report::operations::Recorded;
use crate::report::paint::{self, Screen};
use crate::report::{self, tree, Actor, Exe, Item, Operation, Picture, Report, Spot};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 項目の記録が出来事の列を読むときの、要求の番号（取り消しの照合には使わない）。
const JOURNAL_SEQ: i64 = -1_000_000_000;
/// 画面を読む・写すときの、GUI のスレッドの答えを待つ上限。
const GUI_TIMEOUT: Duration = Duration::from_secs(15);
/// 画面に送る、開いている項目の操作の数（新しいものから）。
const LIVE_OPERATIONS: usize = 100;
/// 操作のあと、画面が変わり終えるのを待つ時間（ダイアログが開く、文字が変わる）。
const AFTER_SETTLE: Duration = Duration::from_millis(300);

/// 開いている（まだ終えていない）項目の、記録のための状態。
struct Open {
    /// 出来事の列の、この番号より後を読む（エンジンを作り直したら 0 から）。
    journal_after: u64,
    /// 項目の始めの画面の木（読めなかったら、その理由）。
    before: Result<Vec<tree::Control>, String>,
    /// 手の操作の記録と、その対象のプロセス。
    recording: Option<(u32, gui::recorder::Recording)>,
    /// 最後に写した画面（手の操作の「前」として、箇所を塗る）。
    last_screen: Option<Screen>,
    /// 操作のあとの画面を写す操作（添字）と、その時刻。
    pending: Option<(usize, Instant)>,
    /// 終わりを待っている AI の操作: 知らせの番号、操作の添字、始める前に写した画面。
    ai: Option<(u64, usize, Option<Screen>)>,
}

#[derive(Default)]
pub struct TestRecorder {
    report: Report,
    open: Option<Open>,
    /// 書き出す先の親のフォルダー（`folder` で指定。無ければ `%LOCALAPPDATA%\netdbg\reports`）。
    base: Option<PathBuf>,
    /// 現場のテンプレートのフォルダー（`template` で指定。中の `report.html` / `report.xlsx` を使う。無いものは既定。D-083）。
    template: Option<PathBuf>,
}

/// テンプレートのフォルダーの中のファイルの名前（言語によらない。D-084）。
const HTML_TEMPLATE: &str = "report.html";
const XLSX_TEMPLATE: &str = "report.xlsx";
const NAMES_FILE: &str = "names.txt";

/// 書き出す成績書のファイルの名前（拡張子を除く。いまの言語）。
fn report_file_stem() -> &'static str {
    tr_str!("TestReport")
}

/// 成績書の要求か。
pub fn handles(command: &str) -> bool {
    matches!(command, "netdbg.testItem" | "netdbg.testReport" | "netdbg.testTemplate")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn now_text() -> String {
    crate::win32::local_time_text(SystemTime::now())
}

/// 時刻だけ（`09:41:44`）。
fn clock() -> String {
    now_text().get(11..).unwrap_or_default().to_string()
}

/// 引数のフォルダー（`folder` は保存先、`output` はそのまま書くフォルダー）。
fn folder_argument(arguments: &Value, key: &str) -> Result<Option<PathBuf>, String> {
    crate::folder::argument(arguments.get(key), &tr!("the report save folder ({key})", key = key))
}

impl TestRecorder {
    /// 要求を処理する。答えと、画面に送る出来事。
    pub fn request(&mut self, engine: &mut Engine, command: &str, arguments: &Value) -> (Result<Value, String>, Vec<Event>) {
        let result = self.use_folder(arguments).and_then(|()| match command {
            "netdbg.testReport" => folder_argument(arguments, "output").and_then(|output| self.write_report(output)),
            "netdbg.testTemplate" => export_templates(arguments),
            _ => match arguments.get("action").and_then(Value::as_str).unwrap_or("") {
                "start" => self.start(engine, arguments),
                "step" => self.step(arguments),
                "capture" => self.capture(engine),
                "end" => self.end(engine, arguments),
                other => Err(tr!("Unknown action for netdbg.testItem: {action} (start, step, capture, end)", action = other)),
            },
        });
        let events = if result.is_ok() { vec![self.changed()] } else { Vec::new() };
        (result, events)
    }

    /// `folder`（保存先の親のフォルダー）と `template`（テンプレートのフォルダー）。保存先が変わったら、次に書くときに、
    /// その下にフォルダーを作り直す。
    fn use_folder(&mut self, arguments: &Value) -> Result<(), String> {
        if let Some(template) = crate::folder::argument(arguments.get("template"), &tr!("the template folder (template)"))? {
            if !template.is_dir() {
                return Err(tr!("The template folder {folder} does not exist (netdbg.testTemplate exports the default templates).", folder = template.display()));
            }
            self.template = Some(template);
        }
        if let Some(folder) = folder_argument(arguments, "folder")? {
            if self.base.as_ref() != Some(&folder) {
                self.base = Some(folder);
                self.report.folder = None;
            }
        }
        Ok(())
    }

    fn start(&mut self, engine: &mut Engine, arguments: &Value) -> Result<Value, String> {
        if let Some(open) = self.report.open_item() {
            return Err(tr!("Item {id} has not ended. End it first (action: end).", id = open.id));
        }
        let id = arguments.get("id").and_then(Value::as_str).map(str::trim).filter(|i| !i.is_empty()).ok_or_else(|| tr!("There is no item number (id)."))?;
        let program = engine.program();
        let exe = program.as_deref().map(exe_info);
        let mut item = Item { id: id.to_string(), title: arguments.get("title").and_then(Value::as_str).unwrap_or("").to_string(), started: now_text(), started_ms: now_ms(), exe, ..Item::default() };
        // 始めの画面（最初の操作の「前」にもなる）。対象がまだ動いていなければ、写さない。
        let last_screen = engine.target_process_id().and_then(|_| capture_app(engine).ok()).map(|(screen, png, title)| {
            item.pictures.push(Picture { caption: tr!("{title} (start)", title = title), png });
            screen
        });
        self.report.items.push(item);
        let before = read_tree(engine);
        self.open = Some(Open { journal_after: journal_last(engine), before, recording: None, last_screen, pending: None, ai: None });
        self.follow_target(engine);
        Ok(self.last_json())
    }

    fn step(&mut self, arguments: &Value) -> Result<Value, String> {
        let text = arguments.get("text").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| tr!("There is no step text (text)."))?;
        let item = self.report.open_item().ok_or_else(|| tr!("No item is open (action: start)."))?;
        item.operations.push(Operation::new(clock(), Actor::Note, text.to_string()));
        Ok(self.last_json())
    }

    fn capture(&mut self, engine: &Engine) -> Result<Value, String> {
        if self.report.open_item().is_none() {
            return Err(tr!("No item is open (action: start)."));
        }
        self.flush(engine);
        self.capture_into_item(engine, &tr!("capture"));
        Ok(self.last_json())
    }

    /// 対象の窓をまとめて写し、項目の画面に足す（写せなければ、その理由）。
    fn capture_into_item(&mut self, engine: &Engine, when: &str) {
        let shot = capture_app(engine);
        let Some(item) = self.report.open_item() else { return };
        match shot {
            Ok((screen, png, title)) => {
                item.pictures.push(Picture { caption: format!("{title}（{when}）"), png });
                if let Some(open) = self.open.as_mut() {
                    open.last_screen = Some(screen);
                }
            }
            Err(e) => item.picture_problems.push(format!("{when}: {e}")),
        }
    }

    fn end(&mut self, engine: &mut Engine, arguments: &Value) -> Result<Value, String> {
        if self.report.open_item().is_none() {
            return Err(tr!("No item is open (action: start)."));
        }
        let result = match arguments.get("result").and_then(Value::as_str).unwrap_or("OK").to_ascii_uppercase().as_str() {
            "OK" => "OK",
            "NG" => "NG",
            other => return Err(tr!("The result must be OK or NG: {result}", result = other)),
        };
        // 終わりの時刻は、写したり読んだりする前（所要時間に、記録の手間を入れない）。
        let (ended, ended_ms) = (now_text(), now_ms());
        // 打っている途中の手の入力も、操作にする。
        if let Some((_, recording)) = self.open.as_mut().and_then(|o| o.recording.take()) {
            for recorded in recording.stop() {
                self.hand(engine, recorded);
            }
        }
        self.harvest(engine);
        // 最後の操作のあとの画面を待っていたなら、それが終わりの画面になる（同じ画面を 2 枚にしない）。
        if !self.flush(engine) {
            self.capture_into_item(engine, &tr!("end"));
        }
        let after = read_tree(engine);
        let open = self.open.take().expect("an open item has its state");
        let item = self.report.open_item().expect("checked above");
        match (&open.before, &after) {
            (Ok(before), Ok(after)) => item.changes = tree::diff(before, after),
            (Err(problem), _) | (_, Err(problem)) => item.change_problem = Some(problem.clone()),
        }
        if let Some(note) = arguments.get("note").and_then(Value::as_str) {
            item.note = note.to_string();
        }
        item.ended = Some(ended);
        item.duration_ms = Some(ended_ms.saturating_sub(item.started_ms));
        item.result = Some(result.to_string());
        // 項目を終えるたびに書き直す（エンジンが落ちても、それまでの分が残る）。
        let written = self.write_report(None)?;
        let mut body = self.last_json();
        body["html"] = written["html"].clone();
        body["xlsx"] = written["xlsx"].clone();
        body["steps"] = written["steps"].clone();
        Ok(body)
    }

    /// AI の操作の知らせ（`netdbg.agentAction`）。画面を変える ui_* の操作を、開いている項目に足す。
    /// 始まりで前の画面を写し、終わりで操作した箇所を塗って、あとの画面を待つ。
    pub fn agent_action(&mut self, engine: &Engine, body: &Value) -> Option<Event> {
        let tool = body["tool"].as_str()?;
        if !tool.starts_with("ui_") || matches!(tool, "ui_tree" | "ui_find" | "ui_screenshot" | "ui_wait_for") {
            return None;
        }
        self.report.open_item()?;
        let id = body["id"].as_u64().unwrap_or(0);
        if body["state"] == "finished" {
            let (_, index, before) = self.open.as_mut()?.ai.take().filter(|(number, ..)| *number == id)?;
            // もう一度流すときの手順（D-086。失敗した操作と、流せない操作には無い）。
            if let Some(operation) = self.report.open_item().and_then(|item| item.operations.get_mut(index)) {
                operation.step = body.get("step").filter(|s| s.is_object()).cloned();
            }
            let spot = body.get("spot").and_then(Spot::from_json);
            self.mark(index, before.as_ref(), spot);
            self.wait_for_after(index);
            return Some(self.changed());
        }
        if body["state"] != "started" {
            return None;
        }
        let arguments: Vec<String> = body["arguments"].as_object().into_iter().flatten().filter(|(key, _)| *key != "waitMs").map(|(key, value)| format!("{key}={}", value.as_str().map_or_else(|| value.to_string(), str::to_string))).collect();
        // 前の画面: 前の操作のあとを待っていればそれを写し（同じ 1 枚を、前の操作の「あと」と、この操作の「前」に使う）、
        // 待っていなければ、いま写す。
        let before = if self.flush(engine) { self.open.as_ref().and_then(|o| o.last_screen.clone()) } else { capture_app(engine).ok().map(|(screen, ..)| screen) };
        // 手順は、終わりの知らせで付ける（相手の名前は、操作の答えで分かる）。
        let index = self.push(Actor::Ai, format!("{tool} {}", arguments.join(" ")).trim_end().to_string(), None)?;
        self.open.as_mut()?.ai = Some((id, index, before));
        Some(self.changed())
    }

    /// 人の手の操作（記録のスレッドから、入力の列で届く）。
    pub fn hand_operation(&mut self, engine: &Engine, recorded: Recorded) -> Option<Event> {
        self.report.open_item()?;
        self.hand(engine, recorded);
        Some(self.changed())
    }

    fn hand(&mut self, engine: &Engine, recorded: Recorded) {
        // 操作はもう済んでいる。前の操作のあとの画面を待っていれば、いま写して、それを「前」にする。
        self.flush(engine);
        let Some(index) = self.push(Actor::Hand, recorded.text, recorded.step) else { return };
        let before = self.open.as_ref().and_then(|o| o.last_screen.clone());
        // 操作した窓が写っていない画面（あとから開いたダイアログを押した、など）には塗らない（違う所を塗ってしまう）。
        let before = before.filter(|screen| recorded.window.is_none_or(|window| screen.windows.contains(&window)));
        self.mark(index, before.as_ref(), recorded.spot);
        self.wait_for_after(index);
    }

    /// 開いている項目に操作を足す（`step` はもう一度流すときの手順。D-086）。足した操作の添字。
    fn push(&mut self, actor: Actor, text: String, step: Option<Value>) -> Option<usize> {
        let item = self.report.open_item()?;
        item.operations.push(Operation { step, ..Operation::new(clock(), actor, text) });
        Some(item.operations.len() - 1)
    }

    /// 操作 `index` に、前の画面の、操作した箇所を塗ったものを付ける。
    fn mark(&mut self, index: usize, before: Option<&Screen>, spot: Option<Spot>) {
        let (Some(screen), Some(spot)) = (before, spot) else { return };
        let Some(pixels) = paint::painted(screen, spot) else { return };
        let png = gui::png::encode(screen.width, screen.height, &pixels);
        if let Some(operation) = self.report.open_item().and_then(|item| item.operations.get_mut(index)) {
            operation.marked = Some(Picture { caption: tr!("Action {n}: the spot", n = index + 1), png });
        }
    }

    /// 操作 `index` のあとの画面を、少し待ってから写す。
    fn wait_for_after(&mut self, index: usize) {
        if let Some(open) = self.open.as_mut() {
            open.pending = Some((index, Instant::now() + AFTER_SETTLE));
        }
    }

    /// 待っている「操作のあとの画面」を、いま写す。写した（写そうとした）なら true。
    fn flush(&mut self, engine: &Engine) -> bool {
        let Some((index, _)) = self.open.as_mut().and_then(|o| o.pending.take()) else { return false };
        let shot = capture_app(engine);
        let Some(item) = self.report.open_item() else { return true };
        match shot {
            Ok((screen, png, title)) => {
                if let Some(operation) = item.operations.get_mut(index) {
                    operation.picture = Some(Picture { caption: tr!("{title} (after action {n})", title = title, n = index + 1), png });
                }
                if let Some(open) = self.open.as_mut() {
                    open.last_screen = Some(screen);
                }
            }
            Err(e) => {
                item.picture_problems.push(tr!("After action {n}: {error}", n = index + 1, error = e));
                if let Some(open) = self.open.as_mut() {
                    open.last_screen = None;
                }
            }
        }
        true
    }

    /// 操作のあとの画面を写す時刻（入力のループは、それまでに入力が無ければ `capture_due` を呼ぶ）。
    pub fn deadline(&self) -> Option<Instant> {
        self.open.as_ref().and_then(|o| o.pending).map(|(_, at)| at)
    }

    /// 時刻の来た「操作のあとの画面」を写す。写したら、画面に送る出来事。
    pub fn capture_due(&mut self, engine: &Engine) -> Option<Event> {
        if self.deadline().is_none_or(|at| at > Instant::now()) {
            return None;
        }
        self.flush(engine);
        Some(self.changed())
    }

    /// 入力を 1 つ処理したあと: 開いている項目の間、いまの対象のプロセスの手の操作を記録する（項目の途中で対象を
    /// 起動し直したら、新しいプロセスに付け替える）。
    pub fn follow_target(&mut self, engine: &Engine) {
        let Some(open) = self.open.as_mut() else { return };
        let process_id = engine.target_process_id();
        if open.recording.as_ref().map(|(p, _)| *p) == process_id {
            return;
        }
        if let Some((_, recording)) = open.recording.take() {
            // 前のプロセスはもう無い（画面は写せない）。言葉だけ残す。
            for recorded in recording.stop() {
                self.push(Actor::Hand, recorded.text, recorded.step);
            }
        }
        let Some(process_id) = process_id else { return };
        match gui::recorder::start(process_id, |recorded| crate::events::post(crate::events::Input::HandOperation(recorded))) {
            Ok(recording) => {
                if let Some(open) = self.open.as_mut() {
                    open.recording = Some((process_id, recording));
                }
            }
            Err(e) => crate::log!("hand recording: {e}"),
        }
    }

    /// エンジンを作り直す前に、出来事の列の分を項目に移す（作り直すと、出来事の列は 0 からになる）。
    pub fn before_engine_replaced(&mut self, engine: &mut Engine) {
        if let Some(open) = self.open.as_mut() {
            // 対象はもう無い（あとの画面は写せない）。
            open.pending = None;
            open.ai = None;
            open.last_screen = None;
            self.harvest(engine);
            if let Some(open) = self.open.as_mut() {
                open.journal_after = 0;
            }
        }
    }

    /// 出来事の列の、前に読んだあとの分を、開いている項目に足す。
    fn harvest(&mut self, engine: &mut Engine) {
        let Some(open) = self.open.as_mut() else { return };
        let reply = engine.request(JOURNAL_SEQ, "netdbg.journal", &json!({ "after": open.journal_after, "limit": 100_000 }), Owner::Agent);
        let Ok(body) = reply.body else { return };
        open.journal_after = body["last"].as_u64().unwrap_or(open.journal_after);
        let Some(item) = self.report.open_item() else { return };
        for record in body["records"].as_array().into_iter().flatten() {
            match record["kind"].as_str().unwrap_or("") {
                "sql" => item.sql.push(record["sql"].as_str().unwrap_or("").to_string()),
                // ダイアログの記録は、本文の kind（messageBox / dialog）が種類になる。
                "dialog" | "messageBox" => item.messages.push(dialog_text(record)),
                "exception" => item.exceptions.push(format!("{}: {}", record["typeName"].as_str().unwrap_or(""), record["message"].as_str().unwrap_or(""))),
                _ => {}
            }
        }
    }

    /// 成績書を書く。`output` はそのまま書くフォルダー（無ければ、保存先の下に `<プログラム名>-<日時>` を作る）。
    /// 書いたファイルと項目の一覧を返す。
    fn write_report(&mut self, output: Option<PathBuf>) -> Result<Value, String> {
        if self.report.items.is_empty() {
            return Err(tr!("There are no items to write (start one with netdbg.testItem, action: start)."));
        }
        if let Some(output) = output {
            self.report.folder = Some(output);
        }
        let base = self.base.clone();
        let folder = self.report.folder.get_or_insert_with(|| default_folder(base.as_deref(), &self.report.items[0])).clone();
        std::fs::create_dir_all(&folder).map_err(|e| tr!("Cannot create {folder} (check that it is writable and the drive exists): {error}", folder = folder.display(), error = e))?;
        let stem = report_file_stem();
        let (html, xlsx, steps) = (folder.join(format!("{stem}.html")), folder.join(format!("{stem}.xlsx")), folder.join(format!("{stem}.steps.json")));
        let generated = now_text();
        let (html_template, xlsx_template) = self.templates()?;
        std::fs::write(&html, report::html::write(&self.report, &generated, html_template.as_deref())?).map_err(|e| tr!("Cannot write {path}: {error}", path = html.display(), error = e))?;
        std::fs::write(&xlsx, report::xlsx::write(&self.report, &generated, xlsx_template.as_deref())?).map_err(|e| tr!("Cannot write {path} (is it open in Excel?): {error}", path = xlsx.display(), error = e))?;
        // 流せる手順（D-086。`ui_replay` の `file`）。
        let steps_json = serde_json::to_string_pretty(&steps_file(&self.report)).unwrap_or_default();
        std::fs::write(&steps, steps_json).map_err(|e| tr!("Cannot write {path}: {error}", path = steps.display(), error = e))?;
        crate::log!("report written: {}", folder.display());
        let items: Vec<Value> = (0..self.report.items.len()).map(|i| self.item_json(i)).collect();
        Ok(json!({ "folder": folder.to_string_lossy(), "html": html.to_string_lossy(), "xlsx": xlsx.to_string_lossy(), "steps": steps.to_string_lossy(), "items": items }))
    }

    /// 現場のテンプレート（フォルダーにあるものだけ。無いものは既定）。
    fn templates(&self) -> Result<(Option<String>, Option<Vec<u8>>), String> {
        let Some(folder) = &self.template else { return Ok((None, None)) };
        let (html, xlsx) = (folder.join(HTML_TEMPLATE), folder.join(XLSX_TEMPLATE));
        let html = html.is_file().then(|| std::fs::read_to_string(&html).map_err(|e| tr!("Cannot read {path} (save it as UTF-8): {error}", path = html.display(), error = e))).transpose()?;
        let xlsx = xlsx.is_file().then(|| std::fs::read(&xlsx).map_err(|e| tr!("Cannot read {path} (is it open in Excel?): {error}", path = xlsx.display(), error = e))).transpose()?;
        Ok((html, xlsx))
    }

    /// いちばん新しい項目（始めた・終えた項目）。
    fn last_json(&self) -> Value {
        self.item_json(self.report.items.len() - 1)
    }

    fn item_json(&self, index: usize) -> Value {
        let item = &self.report.items[index];
        json!({
            "id": item.id,
            "title": item.title,
            "result": item.result,
            "started": item.started,
            "durationMs": item.duration_ms,
            "operations": item.operations.len(),
            "pictures": item.picture_count(),
            "pictureProblems": item.picture_problems,
            "changes": item.changes.len(),
            "sql": item.sql.len(),
            "messages": item.messages.len(),
        })
    }

    /// 画面に送る、項目の一覧と、開いている項目の操作（新しいものから）。
    fn changed(&self) -> Event {
        let items: Vec<Value> = (0..self.report.items.len()).map(|i| self.item_json(i)).collect();
        let open = self.report.items.last().filter(|i| i.result.is_none()).map(|item| {
            let operations: Vec<Value> = item
                .operations
                .iter()
                .rev()
                .take(LIVE_OPERATIONS)
                .map(|o| json!({ "time": o.time, "actor": o.actor.label(), "kind": o.actor.kind(), "text": o.text, "pictures": usize::from(o.marked.is_some()) + usize::from(o.picture.is_some()), "replayable": o.step.is_some() }))
                .collect();
            json!({ "id": item.id, "title": item.title, "operations": operations })
        });
        let folder = self.report.folder.as_ref().map(|f| f.to_string_lossy().into_owned());
        ("netdbg.testItemChanged", json!({ "items": items, "open": open, "folder": folder, "base": self.base.as_ref().map(|b| b.to_string_lossy().into_owned()) }))
    }
}

/// 手順のファイルの中身: 項目ごとに、流せる操作の手順を順に（D-086）。
fn steps_file(report: &Report) -> Value {
    let items: Vec<(String, String, Vec<Value>)> = report.items.iter().map(|item| (item.id.clone(), item.title.clone(), item.operations.iter().filter_map(|o| o.step.clone()).collect())).collect();
    report::steps::file(&items)
}

/// `netdbg.testTemplate { output }`: 既定のテンプレート（HTML、Excel）と、使える名前の説明を書き出す。現場はこれを
/// 直して `template` に渡す。手を入れたテンプレートを消さないよう、同じ名前のファイルがあれば書かない。
fn export_templates(arguments: &Value) -> Result<Value, String> {
    let folder = crate::folder::argument(arguments.get("output"), &tr!("the folder to write to (output)"))?.ok_or_else(|| tr!("There is no folder to write to (output)."))?;
    std::fs::create_dir_all(&folder).map_err(|e| tr!("Cannot create {folder}: {error}", folder = folder.display(), error = e))?;
    let files: [(&str, Vec<u8>); 3] = [(HTML_TEMPLATE, report::html::default_template().as_bytes().to_vec()), (XLSX_TEMPLATE, report::xlsx::default_template()), (NAMES_FILE, report::data::names_text().into_bytes())];
    if let Some((name, _)) = files.iter().find(|(name, _)| folder.join(name).exists()) {
        return Err(tr!("{path} already exists (it is not overwritten, so that edited templates are not lost). Choose another folder, or delete it and export again.", path = folder.join(name).display()));
    }
    for (name, data) in &files {
        let path = folder.join(name);
        std::fs::write(&path, data).map_err(|e| tr!("Cannot write {path}: {error}", path = path.display(), error = e))?;
    }
    Ok(json!({ "folder": folder.to_string_lossy(), "html": folder.join(HTML_TEMPLATE).to_string_lossy(), "xlsx": folder.join(XLSX_TEMPLATE).to_string_lossy(), "names": folder.join(NAMES_FILE).to_string_lossy() }))
}

/// ダイアログの記録の文（`確認: 保存しますか`、フォームなら `ShowDialog AboutForm`）。デバッグ コンソールの 1 行と同じ並び。
fn dialog_text(record: &Value) -> String {
    let text = record["text"].as_str().unwrap_or("");
    let caption = record["caption"].as_str().unwrap_or("");
    let method = record["method"].as_str().unwrap_or("ShowDialog");
    let method = method.rsplit('.').next().unwrap_or(method);
    match (caption.is_empty(), text.is_empty()) {
        (false, false) => format!("{caption}: {text}"),
        (true, false) => text.to_string(),
        _ => format!("{method} {}", record["form"].as_str().unwrap_or("")).trim().to_string(),
    }
}

/// exe の版と更新日時。
fn exe_info(path: &str) -> Exe {
    let file = Path::new(path);
    let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok().map(crate::win32::local_time_text);
    Exe { path: path.to_string(), version: crate::win32::file_version(file), modified }
}

/// 出来事の列の、いまの最後の番号。
fn journal_last(engine: &mut Engine) -> u64 {
    let reply = engine.request(JOURNAL_SEQ, "netdbg.journal", &json!({ "limit": 0 }), Owner::Agent);
    reply.body.ok().and_then(|b| b["last"].as_u64()).unwrap_or(0)
}

/// 既定の書き出す先: `<保存先>\<プログラム名>-<日時>`。保存先の既定は `%LOCALAPPDATA%\netdbg\reports`。
fn default_folder(base: Option<&Path>, first: &Item) -> PathBuf {
    let base = base.map_or_else(|| crate::folder::app_data("reports"), Path::to_path_buf);
    let name = first.exe.as_ref().and_then(|e| Path::new(&e.path).file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "report".into());
    let stamp: String = first.started.chars().filter(char::is_ascii_digit).collect();
    let stamp = if stamp.len() >= 14 { format!("{}-{}", &stamp[..8], &stamp[8..14]) } else { stamp };
    base.join(format!("{name}-{stamp}"))
}

/// 対象の画面の木（比べる値だけ）。止まっていれば、名前と文字は読めない（そのことを理由として返す）。
fn read_tree(engine: &Engine) -> Result<Vec<tree::Control>, String> {
    let process_id = engine.target_process_id().ok_or_else(|| tr!("The target is not running, so the window tree could not be read."))?;
    if engine.is_stopped() {
        return Err(tr!("The target was stopped, so the names and text of the windows could not be read."));
    }
    gui::on_gui_thread(move || gui::watch::read_controls(process_id), GUI_TIMEOUT)

}

/// 対象の見えている窓（ダイアログも）を、重なりのとおりに 1 枚に写す。画素（塗るため）、PNG、前の窓の見出し。
fn capture_app(engine: &Engine) -> Result<(Screen, Vec<u8>, String), String> {
    let process_id = engine.target_process_id().ok_or_else(|| tr!("The target is not running."))?;
    gui::on_gui_thread(
        move || {
            let windows: Vec<usize> = gui::top_windows(process_id);
            let front = *windows.first().ok_or_else(|| tr!("No window is visible."))?;
            let title = gui::element_of(process_id, front).and_then(|e| e.text).filter(|t| !t.is_empty()).unwrap_or_else(|| tr!("Window"));
            let shot = gui::capture::capture(&windows, None, &[], gui::dpi_scales(front).0)?;
            let screen = Screen { pixels: shot.pixels, left: shot.origin.0, top: shot.origin.1, width: shot.width as usize, height: shot.height as usize, windows };
            Ok((screen, shot.png, title))
        },
        GUI_TIMEOUT,
    )?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_records_read_as_one_line() {
        assert_eq!(dialog_text(&json!({ "caption": "確認", "text": "保存しますか" })), "確認: 保存しますか");
        assert_eq!(dialog_text(&json!({ "method": "System.Windows.Forms.Form.ShowDialog", "form": "AboutForm" })), "ShowDialog AboutForm");
    }

    #[test]
    fn the_default_folder_is_named_after_the_program_and_time() {
        let item = Item { started: "2026-11-21 09:41:44".into(), exe: Some(Exe { path: "C:\\app\\Shop.exe".into(), ..Exe::default() }), ..Item::default() };
        assert!(default_folder(None, &item).ends_with("netdbg\\reports\\Shop-20261121-094144"));
        assert_eq!(default_folder(Some(Path::new("D:\\試験\\結果")), &item), PathBuf::from("D:\\試験\\結果\\Shop-20261121-094144"), "保存先の下");
    }
}
