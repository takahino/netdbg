//! Edit and Continue（docs/DECISIONS.md D-059）: デバッグ中に書き換えたソースの差分を、対象に当てる。
//!
//! - 起動の引数 `editAndContinue`（既定 true）なら、シンボルのあるモジュールを、読み込みの通知の中で EnC の形にする
//!   （`CORDEBUG_JIT_ENABLE_ENC`。読み込みのあとでは変えられないので、アタッチした対象では使えない）。
//! - 要求 `netdbg.applyChanges {files?}`: 補助（`tools/encdelta`。利用者の SDK の Roslyn で動く別のプロセス）に
//!   差分を作らせ、`ICorDebugModule2::ApplyChanges` で当てる。自前のメタデータと PDB にも差分を重ね、ブレークポイントを
//!   新しい版に置き直す。走っている間に頼まれたら、いったん止めて当て、続ける。
//! - 第 2 段階: 古い版を実行中のフレームが系列点に来たら（`FunctionRemapOpportunity`）、補助が返した新旧の行の対応で
//!   新しい版の位置を決め、`RemapFunction` で乗り換える。止まっていた行そのものを変えたときは乗り換えず、古いコードの
//!   まま最後まで走らせる（次の呼び出しから新しいコード）。

use super::*;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// 子プロセスにコンソールの窓を出させない（CREATE_NO_WINDOW）。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// .NET Framework のランタイムが受け付ける書き換え（Roslyn の EditAndContinueCapabilities の名前）。
const FRAMEWORK_CAPABILITIES: &str = "Baseline AddMethodToExistingType AddStaticFieldToExistingType AddInstanceFieldToExistingType NewTypeDefinition";
/// .NET 6 / 7。
const NET6_CAPABILITIES: &str = "Baseline AddMethodToExistingType AddStaticFieldToExistingType AddInstanceFieldToExistingType NewTypeDefinition ChangeCustomAttributes UpdateParameters";
/// .NET 8 から、ジェネリックの書き換えも。
const NET8_CAPABILITIES: &str = "Baseline AddMethodToExistingType AddStaticFieldToExistingType AddInstanceFieldToExistingType NewTypeDefinition ChangeCustomAttributes UpdateParameters GenericUpdateMethod GenericAddMethodToExistingType GenericAddFieldToExistingType";

pub(super) struct EncState {
    /// 起動の引数 `editAndContinue`（既定 true）。
    pub enabled: bool,
    helper: Option<Helper>,
    /// 補助を起動できなかった理由（当てようとしたときに伝える）。
    start_error: Option<String>,
    /// モジュール読み込み時に撮った `open`（補助の起動は `applyChanges` まで遅らせ、読み込みのコールバックを塞がない）。
    pending_opens: Vec<Value>,
    /// 乗り換えの対応: （モジュールのパス、メソッドのトークン、古い版）→ 古い行 → 新しい行。
    remaps: HashMap<(String, u32, u32), Vec<(u32, u32)>>,
    /// 乗り換えないと決めた（スレッド、メソッド、古い版）。行の対応が無い、または `RemapFunction` が断った。系列点のたびに問い直さない。
    declined: HashSet<(u32, u32, u32)>,
    /// 「書き換えた所を実行している」と知らせた（スレッド、メソッド、古い版）。知らせるのは 1 度だけ。
    notified: HashSet<(u32, u32, u32)>,
}

impl Default for EncState {
    fn default() -> EncState {
        EncState {
            enabled: true,
            helper: None,
            start_error: None,
            pending_opens: Vec::new(),
            remaps: HashMap::new(),
            declined: HashSet::new(),
            notified: HashSet::new(),
        }
    }
}

impl EncState {
    /// 対象が終わった。補助も止める（対象の出力のパイプの書き側を、補助が受け継いでいるため）。
    pub(super) fn reset(&mut self) {
        self.helper = None;
        self.start_error = None;
        self.pending_opens.clear();
        self.remaps.clear();
        self.declined.clear();
        self.notified.clear();
    }
}

/// 差分を作る補助のプロセス。1 行 1 JSON で要求と応答をやり取りする。
struct Helper {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
    /// 応答を待たずに送った要求（`open`）の数。次の要求の前に、その応答を読み捨てる。
    unread: usize,
    /// 応答を待たなかった要求のうち、失敗したものの理由。
    failures: Vec<String>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Helper {
    fn start() -> Result<Helper, String> {
        let roslyn = sdk_roslyn()?;
        let exe = std::env::current_exe().map_err(|e| tr!("Cannot tell where the engine is: {error}", error = e))?;
        let dll = exe.parent().map(|dir| dir.join("encdelta").join("EncDelta.dll")).filter(|p| p.is_file()).ok_or_else(|| tr!("The delta helper (encdelta\\EncDelta.dll) is not next to the engine. Reinstall the extension."))?;
        // エンジンの標準入出力は対象の出力に付け替えてあるので、補助の入出力は必ずパイプにする。
        let mut child = Command::new("dotnet")
            .arg(&dll)
            .arg("--roslyn")
            .arg(&roslyn)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| tr!("Cannot start the delta helper: {error}", error = e))?;
        let input = child.stdin.take().ok_or_else(|| tr!("The helper has no standard input"))?;
        let output = BufReader::new(child.stdout.take().ok_or_else(|| tr!("The helper has no standard output"))?);
        crate::log!("encdelta started: {} --roslyn {}", dll.display(), roslyn.display());
        Ok(Helper { child, input, output, next_id: 1, unread: 0, failures: Vec::new() })
    }

    /// 応答を待たずに送る（デバッグの始めの `open`。対象の起動を待たせない）。
    fn send(&mut self, mut request: Value) -> Result<(), String> {
        request["id"] = json!(self.next_id);
        self.next_id += 1;
        let lost = |e: std::io::Error| tr!("Talking with the delta helper failed: {error}", error = e);
        writeln!(self.input, "{request}").map_err(lost)?;
        self.input.flush().map_err(lost)?;
        self.unread += 1;
        Ok(())
    }

    fn read_reply(&mut self) -> Result<Value, String> {
        let mut line = String::new();
        if self.output.read_line(&mut line).map_err(|e| tr!("Talking with the delta helper failed: {error}", error = e))? == 0 {
            return Err(tr!("The delta helper has ended."));
        }
        serde_json::from_str(&line).map_err(|e| tr!("Cannot read the helper's answer: {error}", error = e))
    }

    fn request(&mut self, mut request: Value) -> Result<Value, String> {
        while self.unread > 0 {
            let reply = self.read_reply()?;
            self.unread -= 1;
            if reply.get("status").and_then(Value::as_str) != Some("ok") {
                let message = reply.get("message").and_then(Value::as_str).map_or_else(|| tr!("The helper failed"), str::to_string);
                crate::log!("encdelta open: {message}");
                self.failures.push(message);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = json!(id);
        let lost = |e: std::io::Error| tr!("Talking with the delta helper failed: {error}", error = e);
        writeln!(self.input, "{request}").map_err(lost)?;
        self.input.flush().map_err(lost)?;
        let reply = self.read_reply()?;
        if reply.get("id") != Some(&json!(id)) {
            return Err(tr!("The helper's answer has the wrong number."));
        }
        Ok(reply)
    }
}

/// 利用者の .NET SDK のうち、いちばん新しいものの Roslyn（`sdk\<版>\Roslyn\bincore`）。
fn sdk_roslyn() -> Result<std::path::PathBuf, String> {
    let output = Command::new("dotnet")
        .arg("--list-sdks")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| tr!("Cannot start dotnet ({error}). Edit and Continue needs .NET SDK 9.0.200 or later.", error = e))?;
    let text = String::from_utf8_lossy(&output.stdout);
    // 1 行が「10.0.201 [C:\Program Files\dotnet\sdk]」。
    let newest = text
        .lines()
        .filter_map(|line| {
            let (version, rest) = line.split_once(" [")?;
            let directory = rest.strip_suffix(']')?;
            let numbers: Vec<u32> = version.split(['.', '-']).take(3).filter_map(|n| n.parse().ok()).collect();
            Some((numbers, version.to_string(), directory.to_string()))
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    let (numbers, version, directory) = newest.ok_or_else(|| tr!("No installed .NET SDK was found. Edit and Continue needs .NET SDK 9.0.200 or later."))?;
    // 差分を作る補助 (tools/encdelta) は Roslyn 4.13 (SDK 9.0.200) の API を使う。
    if numbers < vec![9, 0, 200] {
        return Err(tr!("Edit and Continue needs .NET SDK 9.0.200 or later (the newest is {version}).", version = version));
    }
    let roslyn = Path::new(&directory).join(&version).join("Roslyn").join("bincore");
    if !roslyn.join("Microsoft.CodeAnalysis.dll").is_file() {
        return Err(tr!("{path} has no Roslyn.", path = roslyn.display()));
    }
    Ok(roslyn)
}

/// 当てた結果のまとめ。強いものが勝つ（失敗 > コンパイルのエラー > 再起動が要る > 当てた > 変更なし）。
#[derive(Default)]
struct Outcome {
    status: &'static str,
    applied: Vec<Value>,
    problems: Vec<Value>,
    messages: Vec<String>,
}

impl Outcome {
    fn raise(&mut self, status: &'static str) {
        let rank = |s: &str| ["", "noChanges", "applied", "rude", "errors", "failed"].iter().position(|x| *x == s).unwrap_or(0);
        if rank(status) > rank(self.status) {
            self.status = status;
        }
    }
}

impl Session {
    /// 読み込んだモジュールを EnC の形にするか（起動した対象で、`editAndContinue` が有効）。
    pub(super) fn wants_enc(&self) -> bool {
        self.enc.enabled && !self.is_attached()
    }

    /// EnC の形にしたモジュールの、いまのソースを差分の基準として覚える。
    /// 補助プロセスはここでは起動しない（`dotnet` / EncDelta の初回起動が読み込みコールバックを数十秒塞ぐことがあるため）。
    /// ソースの中身だけここで読み、`applyChanges` の直前に `open` として送る。
    pub(super) fn enc_open(&mut self, index: usize) {
        let module = &self.modules[index];
        let mut request = json!({ "command": "open", "module": module.path });
        if let Some(pdb) = &module.pdb_path {
            request["pdb"] = json!(pdb.to_string_lossy());
        }
        // いまのソースの中身を、ここで読んで渡す。補助が読むのを待つと、そのあいだに利用者（や試験）が書き換えたソースを
        // 基準にしてしまう。モジュールの読み込みの通知の中なので、対象のコードはまだ動いていない。
        let sources: Vec<Value> = module
            .pdb
            .as_ref()
            .map(|pdb| pdb.documents.iter().filter_map(|path| Some(json!({ "path": path, "bytes": crate::base64::encode(&std::fs::read(path).ok()?) }))).collect())
            .unwrap_or_default();
        request["sources"] = json!(sources);
        self.enc.pending_opens.push(request);
    }

    /// 補助を起動し、溜めておいた `open` を送る。起動済みなら溜めた分だけ送る。
    fn ensure_enc_helper(&mut self) -> Result<(), String> {
        if self.enc.helper.is_some() {
            return self.flush_pending_opens();
        }
        if let Some(message) = &self.enc.start_error {
            return Err(message.clone());
        }
        match Helper::start() {
            Ok(helper) => {
                self.enc.helper = Some(helper);
                self.flush_pending_opens()
            }
            Err(message) => {
                crate::log!("encdelta: {message}");
                self.enc.start_error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn flush_pending_opens(&mut self) -> Result<(), String> {
        let opens = std::mem::take(&mut self.enc.pending_opens);
        for request in opens {
            if let Some(helper) = self.enc.helper.as_mut() {
                if let Err(message) = helper.send(request) {
                    crate::log!("encdelta: {message}");
                    self.enc.helper = None;
                    self.enc.start_error = Some(message.clone());
                    return Err(message);
                }
            }
        }
        Ok(())
    }

    /// `netdbg.applyChanges`: 書き換えたソースの差分を当てる。`files` を渡せば、そのファイルだけを見る。
    pub fn apply_changes(&mut self, arguments: &Value) -> Result<Value, String> {
        if self.is_attached() {
            return Err(tr!("Code changes cannot be applied to an attached process (Edit and Continue works only when launched from the debugger)."));
        }
        if !self.enc.enabled {
            return Err(tr!("Edit and Continue is off (editAndContinue in launch.json)."));
        }
        if !self.is_live() {
            return Err(tr!("Nothing is being debugged."));
        }
        let files: Option<Vec<String>> = arguments.get("files").and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect());
        let targets: Vec<usize> = (0..self.modules.len())
            .filter(|&i| {
                let module = &self.modules[i];
                module.enc && module.pdb.as_ref().is_some_and(|pdb| files.as_ref().is_none_or(|files| files.iter().any(|f| pdb.find_document(f).is_some())))
            })
            .collect();
        let mut outcome = Outcome::default();
        if targets.is_empty() {
            outcome.raise("noChanges");
            outcome.messages.push(match &files {
                Some(_) => tr!("The edited file is not a source of a module that supports Edit and Continue (a Debug build with symbols)."),
                None => tr!("There is no module that supports Edit and Continue (a Debug build with symbols)."),
            });
            return Ok(self.finish_apply(outcome));
        }
        // 走っていれば、当てる間だけ止める（ApplyChanges は、対象が止まっている間だけ）。
        let running = self.process.clone().filter(|_| self.state == State::Running);
        if let Some(process) = &running {
            unsafe { process.Stop(0) }.ok().map_err(|hr| failed(tr_str!("Stopping the target"), hr))?;
        }
        if let Err(message) = self.ensure_enc_helper() {
            if let Some(process) = &running {
                unsafe { process.Continue(0) };
            }
            return Err(message);
        }
        let capabilities = self.enc_capabilities();
        for index in targets {
            self.apply_to_module(index, files.as_deref(), &capabilities, &mut outcome);
        }
        if outcome.status.is_empty() {
            outcome.raise("noChanges");
        }
        if let Some(process) = &running {
            unsafe { process.Continue(0) };
        }
        Ok(self.finish_apply(outcome))
    }

    /// 結果を、応答と通知（`output`、`netdbg.changesApplied`）にする。
    fn finish_apply(&mut self, outcome: Outcome) -> Value {
        let message = match outcome.status {
            "applied" => tr!("Applied the code changes ({modules}).", modules = outcome.applied.iter().filter_map(|a| a.get("module").and_then(Value::as_str)).collect::<Vec<_>>().join(tr_str!(", "))),
            "noChanges" => tr!("There are no changes to apply."),
            "rude" => tr!("This change cannot be applied to the running program. Restart debugging."),
            "errors" => tr!("There are compile errors, so the changes were not applied."),
            _ => tr!("Could not apply the changes."),
        };
        let mut text = message.clone();
        for line in outcome.messages.iter().cloned().chain(outcome.problems.iter().map(describe_problem)) {
            text.push_str("\n  ");
            text.push_str(&line);
        }
        let body = json!({ "status": outcome.status, "message": message, "applied": outcome.applied, "problems": outcome.problems, "details": outcome.messages });
        // 変更の無い保存（コメントだけの保存、ほかのプロジェクトのファイル）のたびには、デバッグ コンソールに書かない（理由は応答の details）。
        if outcome.status != "noChanges" {
            self.deferred.push(("output", json!({ "category": "console", "output": format!("{text}\n") })));
        }
        self.deferred.push(("netdbg.changesApplied", body.clone()));
        if outcome.status == "applied" {
            // コールスタックの行と変数が変わり得る。
            self.deferred.push(("invalidated", json!({ "areas": ["stacks", "variables"] })));
        }
        body
    }

    fn apply_to_module(&mut self, index: usize, files: Option<&[String]>, capabilities: &str, outcome: &mut Outcome) {
        let module_path = self.modules[index].path.clone();
        let name = Path::new(&module_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut request = json!({ "command": "update", "module": module_path, "capabilities": capabilities });
        if let Some(files) = files {
            request["files"] = json!(files);
        }
        let reply = match self.enc.helper.as_mut().map(|helper| helper.request(request)) {
            Some(Ok(reply)) => reply,
            Some(Err(message)) => {
                // 補助が壊れたら、次の要求で起動し直す。
                self.enc.helper = None;
                outcome.raise("failed");
                outcome.messages.push(message);
                return;
            }
            None => return,
        };
        let problems = |key: &str| reply.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        // 当てられても伝えること（ソース生成器を走らせ直せず、生成されたコードが古いまま、など）。
        for warning in reply.get("warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            outcome.messages.push(format!("{name}: {warning}"));
        }
        match reply.get("status").and_then(Value::as_str).unwrap_or("failed") {
            "noChanges" => outcome.raise("noChanges"),
            "rude" => {
                outcome.raise("rude");
                outcome.problems.extend(problems("rudeEdits"));
            }
            "errors" => {
                outcome.raise("errors");
                outcome.problems.extend(problems("diagnostics"));
            }
            "ready" => match self.apply_delta(index, &reply) {
                Ok(methods) => {
                    self.helper_request(json!({ "command": "commit", "module": module_path }));
                    outcome.raise("applied");
                    outcome.applied.push(json!({ "module": name, "methods": methods }));
                }
                Err(message) => {
                    self.helper_request(json!({ "command": "discard", "module": module_path }));
                    outcome.raise("failed");
                    outcome.messages.push(format!("{name}: {message}"));
                }
            },
            _ => {
                outcome.raise("failed");
                outcome.messages.push(format!("{name}: {}", reply.get("message").and_then(Value::as_str).map_or_else(|| tr!("Could not create the delta."), str::to_string)));
                // デバッグの始めに覚えさせられなかった理由（ソースの写し、参照の解決）があれば、それも伝える。
                if let Some(helper) = self.enc.helper.as_mut() {
                    // 補助が同じ理由を返したもの（Windows PDB など）は、二度並べない。
                    for failure in std::mem::take(&mut helper.failures) {
                        if !outcome.messages.iter().any(|m| m.ends_with(failure.as_str())) {
                            outcome.messages.push(failure);
                        }
                    }
                }
            }
        }
    }

    fn helper_request(&mut self, request: Value) {
        if let Some(Err(message)) = self.enc.helper.as_mut().map(|helper| helper.request(request)) {
            crate::log!("encdelta: {message}");
            self.enc.helper = None;
        }
    }

    /// 補助の差分（`ready` の応答）を当てる。変えたメソッドの数を返す。
    fn apply_delta(&mut self, index: usize, reply: &Value) -> Result<usize, String> {
        let decode = |key: &str| -> Result<Option<Vec<u8>>, String> {
            match reply.get("delta").and_then(|d| d.get(key)).and_then(Value::as_str) {
                Some(text) => crate::base64::decode(text).map(Some).ok_or_else(|| tr!("Cannot read the helper's delta")),
                None => Ok(None),
            }
        };
        let (metadata, il, pdb_delta) = (decode("metadata")?, decode("il")?, decode("pdb")?);
        let module = self.modules[index].module.clone();
        let module_path = self.modules[index].path.clone();
        let pdb = self.modules[index].pdb.as_ref().ok_or_else(|| tr!("There are no symbols"))?;

        // 当てる前の PDB で、行のずれと乗り換えの対応を、メソッドに結びつける（補助の行は、前に当てたソースの行）。
        let updated: HashSet<u32> = reply.get("updatedMethods").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).map(|t| t as u32).collect();
        let mut shifts = Vec::new();
        for update in reply.get("lineUpdates").and_then(Value::as_array).into_iter().flatten() {
            let (Some(file), Some(start), Some(end), Some(delta)) = (update["file"].as_str(), update["startLine"].as_u64(), update["endLine"].as_u64(), update["delta"].as_i64()) else { continue };
            let Some(document) = pdb.find_document(file) else { continue };
            for method in pdb.methods_between(document, start as u32, end as u32) {
                if !updated.contains(&method) {
                    shifts.push((method, delta as i32));
                }
            }
        }
        let mut remaps = Vec::new();
        for map in reply.get("lineMaps").and_then(Value::as_array).into_iter().flatten() {
            let (Some(file), Some(start), Some(end)) = (map["file"].as_str(), map["oldStartLine"].as_u64(), map["oldEndLine"].as_u64()) else { continue };
            let Some(document) = pdb.find_document(file) else { continue };
            let lines: Vec<(u32, u32)> = map["lines"].as_array().into_iter().flatten().filter_map(|pair| Some((pair.get(0)?.as_u64()? as u32, pair.get(1)?.as_u64()? as u32))).collect();
            for method in pdb.methods_between(document, start as u32, end as u32) {
                let version = unsafe { current_version(&module, method) };
                remaps.push(((module_path.clone(), method, version), lines.clone()));
            }
        }

        let mut changed: HashSet<u32> = updated.clone();
        if let (Some(mut metadata_delta), Some(mut il)) = (metadata, il) {
            let module2 = module.cast::<ICorDebugModule2>().map_err(|hr| failed(tr_str!("Getting ICorDebugModule2"), hr))?;
            // SAFETY: 差分の配列は、呼び出しの間だけ生きていればよい（ランタイムが写しを取る）。
            let hr = unsafe { module2.ApplyChanges(metadata_delta.len() as u32, metadata_delta.as_mut_ptr(), il.len() as u32, il.as_mut_ptr()) };
            crate::log!("ApplyChanges {module_path}: {hr} (metadata {} bytes, il {} bytes)", metadata_delta.len(), il.len());
            if !hr.is_ok() {
                return Err(tr!("The runtime did not accept the changes ({code}). Restart debugging.", code = hr));
            }
            self.touched_target = true;
            // 自前のメタデータにも重ねる（名前、足したメソッドとフィールド、新しい MemberRef を引けるように）。
            let entry = &mut self.modules[index];
            let _ = entry.metadata();
            if let Some(Some(meta)) = entry.metadata.get_mut() {
                if let Err(e) = meta.apply_delta(metadata_delta) {
                    crate::log!("metadata delta {module_path}: {e}");
                }
            }
            if let (Some(bytes), Some(pdb)) = (pdb_delta, entry.pdb.as_mut()) {
                let version_of = |token: u32| unsafe { current_version(&module, token) };
                match pdb.apply_delta(bytes, &version_of) {
                    Ok(methods) => changed.extend(methods),
                    Err(e) => crate::log!("pdb delta {module_path}: {e}"),
                }
            }
        }
        if let Some(pdb) = self.modules[index].pdb.as_mut() {
            for (method, delta) in shifts {
                pdb.shift_lines(method, delta);
            }
        }
        for (key, lines) in remaps {
            self.enc.remaps.insert(key, lines);
        }
        self.rebind_after_changes(index, &changed);
        Ok(updated.len().max(changed.len()))
    }

    /// 差分を当てたモジュールのブレークポイントを置き直す。変えたメソッドと足したメソッドは、新しい版に置く
    /// （古い版に置いたものは、古いコードを実行中のフレームのために残す）。行が動いたものは、止まる行を直して知らせる。
    fn rebind_after_changes(&mut self, index: usize, changed: &HashSet<u32>) {
        let module = &self.modules[index];
        let Some(pdb) = module.pdb.as_ref() else { return };
        let mut events = Vec::new();
        for ((owner, source), breakpoints) in self.breakpoints.iter_mut() {
            let Some(document) = pdb.find_document(source) else { continue };
            for breakpoint in breakpoints.iter_mut() {
                let Some(location) = pdb.resolve_line(document, breakpoint.line) else { continue };
                if changed.contains(&location.method) {
                    match unsafe { breakpoints::break_at(&module.module, location.method, location.il_offset) } {
                        Ok(created) => breakpoint.bound.push(created),
                        Err(e) => crate::log!("breakpoint {} after edit: {e}", breakpoint.id),
                    }
                }
                if breakpoint.verified_line != Some(location.line) {
                    breakpoint.verified_line = Some(location.line);
                    if !owner.is_reported() {
                        continue;
                    }
                    events.push(("breakpoint", json!({ "reason": "changed", "breakpoint": describe(breakpoint) })));
                }
            }
        }
        if let Some(metadata) = module.metadata() {
            for breakpoint in self.function_breakpoints.iter_mut() {
                for method in metadata.find_methods(&breakpoint.name).into_iter().filter(|m| changed.contains(m)) {
                    match unsafe { breakpoints::break_at(&module.module, method, 0) } {
                        Ok(created) => breakpoint.bound.push(created),
                        Err(e) => crate::log!("function breakpoint {} after edit: {e}", breakpoint.name),
                    }
                }
            }
        }
        self.deferred.extend(events);
    }

    /// 対象のランタイムが受け付ける書き換え。.NET Framework は固定、.NET は版（System.Private.CoreLib のフォルダー）で決める。
    fn enc_capabilities(&self) -> String {
        if self.config.as_ref().is_some_and(|config| config.framework) {
            return FRAMEWORK_CAPABILITIES.into();
        }
        let major = self
            .modules
            .iter()
            .find(|m| m.path.to_ascii_lowercase().ends_with("system.private.corelib.dll"))
            .and_then(|m| Path::new(&m.path).parent()?.file_name()?.to_str()?.split('.').next()?.parse::<u32>().ok());
        match major {
            Some(6 | 7) => NET6_CAPABILITIES.into(),
            _ => NET8_CAPABILITIES.into(),
        }
    }

    /// 古い版を実行しているフレームが系列点に来た。対応する新しい位置があれば、葉のフレームを乗り換える。
    pub(super) fn remap_opportunity(&mut self, thread: &ComPtr<ICorDebugThread>, old_function: &ComPtr<ICorDebugFunction>, new_function: &ComPtr<ICorDebugFunction>, old_il_offset: u32) -> Vec<Event> {
        let thread = thread.clone();
        let thread_number = thread_id(&thread);
        let mut token = 0u32;
        unsafe { old_function.GetToken(&mut token) };
        let (old_version, new_version) = (function_version(old_function), function_version(new_function));
        if self.enc.declined.contains(&(thread_number, token, old_version)) {
            return Vec::new();
        }
        let module_path = unsafe { take::<ICorDebugModule>("GetModule", |out| old_function.GetModule(out)) }.map(|m| module_path(&m)).unwrap_or_default();
        let key = (thread_number, token, old_version);
        let Some(lines) = self.enc.remaps.get(&(module_path.clone(), token, old_version)) else {
            self.enc.declined.insert(key);
            crate::log!("FunctionRemapOpportunity 0x{token:08X} v{old_version}: no line map, stay on the old version");
            return Vec::new();
        };
        let Some(new_offset) = self.remap_target(&module_path, lines, token, old_version, new_version, old_il_offset) else {
            // 書き換えた文の中にいる。断らずに、次の系列点で問い直す（書き換えていない文に来たら乗り換える）。
            crate::log!("FunctionRemapOpportunity 0x{token:08X} v{old_version} IL {old_il_offset:#x}: no counterpart, wait for the next one");
            if !self.enc.notified.insert(key) {
                return Vec::new();
            }
            let name = self.modules.iter().find(|m| m.path == module_path).and_then(|m| m.metadata()?.method_name(token)).unwrap_or_default();
            return vec![(
                "output",
                json!({ "category": "console", "output": tr!("{name} is running inside the edited part, so it keeps running the old code until it reaches an unchanged statement (if it does not, the new code is used from its next call).\n", name = name) }),
            )];
        };
        let result = unsafe {
            take::<ICorDebugFrame>(tr_str!("Getting the frame"), |out| thread.GetActiveFrame(out))
                .and_then(|frame| frame.cast::<ICorDebugILFrame2>().map_err(|hr| failed(tr_str!("Getting ICorDebugILFrame2"), hr)))
                .and_then(|frame| frame.RemapFunction(new_offset).ok().map_err(|hr| failed("RemapFunction", hr)))
        };
        crate::log!("RemapFunction 0x{token:08X} v{old_version} IL {old_il_offset:#x} -> v{new_version} IL {new_offset:#x}: {result:?}");
        if result.is_err() {
            self.enc.declined.insert(key);
        }
        Vec::new()
    }

    /// 乗り換え先の新しい版の IL の位置: 古い版で `old_il_offset` を含む文 → 補助の行の対応 → 新しい版の、同じ行と列で
    /// 始まる文。文の途中（呼び出しから戻る所。止めずに当てたときの、葉でないフレーム）なら、文の頭からの距離を保つ。
    /// 書き換えていない文は同じ IL になるので、文のすべての行が対応し、IL の長さが同じときだけそうする。
    fn remap_target(&self, module_path: &str, lines: &[(u32, u32)], token: u32, old_version: u32, new_version: u32, old_il_offset: u32) -> Option<u32> {
        let pdb = self.modules.iter().find(|m| m.path == module_path)?.pdb.as_ref()?;
        let (old, old_end) = pdb.statement_span(token, old_version, old_il_offset)?;
        let new_line = |line: u32| lines.iter().find(|(o, _)| *o == line).map(|(_, n)| *n);
        let first = new_line(old.start_line)?;
        let (start, end) = pdb.statement_at(token, new_version, first, old.start_column)?;
        let distance = old_il_offset - old.il_offset;
        if distance == 0 {
            return Some(start);
        }
        let unchanged = (old.start_line..=old.end_line).all(|line| new_line(line) == Some(first + (line - old.start_line)));
        match (old_end, end) {
            (Some(old_end), Some(end)) if unchanged && old_end - old.il_offset == end - start => Some(start + distance),
            _ => None,
        }
    }

    /// 乗り換えが終わった。ステップの途中なら、新しい版でステップをやり直す（古い版に作ったステップは使えない）。
    pub(super) fn remap_complete(&mut self, thread: &ComPtr<ICorDebugThread>) -> Vec<Event> {
        crate::log!("FunctionRemapComplete");
        if let Some(stepper) = self.stepper.take() {
            unsafe { stepper.Deactivate() };
            let kind = if self.stepping_in { StepKind::In } else { StepKind::Over };
            if let Err(e) = self.start_step(thread_id(thread), kind) {
                crate::log!("step after remap: {e}");
            }
        }
        Vec::new()
    }
}

/// メソッドのいまの版（当てた差分の数で増える）。
unsafe fn current_version(module: &ComPtr<ICorDebugModule>, token: u32) -> u32 {
    let Ok(function) = take::<ICorDebugFunction>("GetFunctionFromToken", |out| module.GetFunctionFromToken(token, out)) else { return crate::debugger::pdb::LATEST };
    let mut version = 0u32;
    if function.GetCurrentVersionNumber(&mut version).is_ok() {
        version
    } else {
        crate::debugger::pdb::LATEST
    }
}

/// 場所つきの知らせを 1 行にする（`Program.cs(12,5): …`）。
fn describe_problem(problem: &Value) -> String {
    let file = problem.get("file").and_then(Value::as_str).map(|f| Path::new(f).file_name().map_or(f.to_string(), |n| n.to_string_lossy().into_owned())).unwrap_or_default();
    format!("{file}({},{}): {}", problem["line"], problem["column"], problem.get("message").and_then(Value::as_str).unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stronger_outcome_wins() {
        let mut outcome = Outcome::default();
        outcome.raise("applied");
        outcome.raise("noChanges");
        assert_eq!(outcome.status, "applied");
        outcome.raise("rude");
        outcome.raise("applied");
        assert_eq!(outcome.status, "rude");
    }
}
