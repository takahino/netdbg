//! PDB の無いモジュールを、エンジンの中で逆コンパイルしてデバッグする（D-060）。
//!
//! - 対象: PDB が無く、アプリのフォルダーの下にあるモジュール（.NET と .NET Framework の部品は除く）。
//! - 読み込みのとき: JIT の最適化を切る（止まる場所と変数を正しくする）。起動したプログラム自体に PDB が無ければ、
//!   対象のモジュールを「自分のコード」にする（ステップで中に入る）。逆コンパイルはまだしない。
//! - 必要になったとき（フレームを見せる、ステップする、キャッシュのファイルにブレークポイントがある、頼まれた）:
//!   逆コンパイルしてキャッシュのフォルダーに書き、合成した PDB を `Module::pdb` に入れる。以降は、PDB のある
//!   モジュールと同じ道を通る。

use super::*;
use crate::debugger::decompile;
use std::path::PathBuf;

/// モジュールの、逆コンパイルの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decompile {
    /// 対象でない（PDB がある、アプリの外、無効）。
    NotTarget,
    /// 対象で、まだ逆コンパイルしていない。
    Pending,
    /// 逆コンパイルして、合成した PDB を付けた。
    Done,
    /// 逆コンパイルできなかった（理由はログ）。
    Failed,
}

/// 起動の引数 `decompile` と、対象を決めるための情報。
pub(super) struct DecompileState {
    /// `decompile` が `"off"` でない（既定は `"app"`）。
    pub enabled: bool,
    /// 起動したプログラム（アタッチなら exe）のフォルダー。この下のモジュールが対象。
    pub app_directory: Option<PathBuf>,
    /// 起動したプログラムのファイル名（拡張子なし、小文字）。そのモジュールに PDB があるかで、JMC を決める。
    pub program_stem: Option<String>,
    /// 起動したプログラムのモジュールに PDB があったか（まだ読み込まれていなければ `None`）。
    pub program_has_symbols: Option<bool>,
}

impl Default for DecompileState {
    fn default() -> DecompileState {
        DecompileState { enabled: true, app_directory: None, program_stem: None, program_has_symbols: None }
    }
}

impl DecompileState {
    /// 起動か接続のときに、プログラムのパスを覚える。
    pub fn set_program(&mut self, program: &str) {
        let path = Path::new(program);
        self.app_directory = path.parent().map(Path::to_path_buf);
        self.program_stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase());
        self.program_has_symbols = None;
    }
}

pub(super) use decompile::is_platform_assembly;

/// 逆コンパイルしたソースを置くフォルダー: `%LOCALAPPDATA%\netdbg\decompiled\<MVID>-v<版>`。
fn cache_root(md: &Metadata) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let mvid = crate::debugger::metadata::format_guid(&md.mvid()?);
    Some(base.join("netdbg").join("decompiled").join(format!("{mvid}-v{}", decompile::VERSION)))
}

/// 書き終えたことの印のファイル。途中で終わったフォルダーは書き直す。
const COMPLETE: &str = ".complete";

impl Session {
    /// 読み込んだモジュールが、逆コンパイルの対象か。
    pub(super) fn is_decompile_target(&self, path: &str, has_symbols: bool) -> bool {
        if has_symbols || !self.decompile.enabled {
            return false;
        }
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        if is_platform_assembly(&name) {
            return false;
        }
        // ディスクに無いモジュール（単一ファイルのアプリの中身）は、アプリのもの。
        if !Path::new(path).is_file() {
            return true;
        }
        let Some(app) = &self.decompile.app_directory else { return false };
        let lower = path.to_lowercase();
        // 区切りまで比べる（`C:\app` が `C:\app2\x.dll` を含まないように）。
        let app = app.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string() + "\\";
        lower.starts_with(&app) && !lower.contains("\\dotnet\\shared\\")
    }

    /// 読み込みの通知の中で、起動したプログラム自体のモジュールなら、PDB の有無を覚える。
    pub(super) fn note_program_module(&mut self, path: &str, has_symbols: bool) {
        let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_lowercase());
        if stem.is_some() && stem == self.decompile.program_stem && self.decompile.program_has_symbols.is_none() {
            self.decompile.program_has_symbols = Some(has_symbols);
        }
    }

    /// 対象のモジュールを、読み込みの時点で整える。まだ何もコンパイルされていない今のうちに JIT の最適化を切る。
    /// 起動したプログラム自体に PDB が無ければ、「自分のコード」にする（ステップで中に入る）。
    pub(super) fn prepare_decompile_target(&self, module: &ComPtr<ICorDebugModule>, path: &str) {
        let Ok(module2) = module.cast::<ICorDebugModule2>() else { return };
        let hr = unsafe { module2.SetJITCompilerFlags(CORDEBUG_JIT_DISABLE_OPTIMIZATION) };
        crate::log!("decompile target {path}: SetJITCompilerFlags {hr}");
        if self.decompile.program_has_symbols == Some(false) {
            let hr = unsafe { module2.SetJMCStatus(1, 0, null_mut()) };
            if !hr.is_ok() {
                crate::log!("SetJMCStatus {path}: {hr}");
            }
        }
    }

    /// モジュールを逆コンパイルして、合成した PDB を付ける（まだなら）。待っていたブレークポイントを置き直し、
    /// クライアントへの知らせを返す。
    pub(super) fn ensure_decompiled(&mut self, index: usize) -> Vec<Event> {
        if self.modules.get(index).map(|m| m.decompile) != Some(Decompile::Pending) {
            return Vec::new();
        }
        let started = std::time::Instant::now();
        match self.decompile_module(index) {
            Ok((root, count)) => {
                self.modules[index].decompile = Decompile::Done;
                crate::log!("decompiled {} in {:.1} ms: {count} files in {}", self.modules[index].path, started.elapsed().as_secs_f64() * 1000.0, root.display());
                let module = &self.modules[index];
                let name = Path::new(&module.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| module.path.clone());
                let mut out = vec![
                    ("module", json!({ "reason": "changed", "module": { "id": module.path, "name": name, "path": module.path, "symbolStatus": "Symbols decompiled." } })),
                    ("output", json!({ "category": "console", "output": tr!("{name} has no symbols, so it was decompiled ({folder}).\n", name = name, folder = root.display()) })),
                ];
                out.extend(self.bind_waiting_breakpoints(index));
                out
            }
            Err(e) => {
                self.modules[index].decompile = Decompile::Failed;
                crate::log!("decompile {}: {e}", self.modules[index].path);
                vec![("output", json!({ "category": "console", "output": tr!("Could not decompile {path}: {error}\n", path = self.modules[index].path, error = e) }))]
            }
        }
    }

    /// 逆コンパイルしてファイルに書き、合成した PDB を付ける。書いたフォルダーとファイルの数を返す。
    fn decompile_module(&mut self, index: usize) -> Result<(PathBuf, usize), String> {
        let module = &self.modules[index];
        let image = if Path::new(&module.path).is_file() { std::fs::read(&module.path).map_err(|e| e.to_string())? } else { unsafe { module_image(&module.module) }.ok_or_else(|| tr!("Cannot read the module image"))? };
        let md = Metadata::from_pe(&image)?;
        let root = cache_root(&md).ok_or_else(|| tr!("Cannot read the module's MVID"))?;
        let decompiled = decompile::module(&md, &image);
        let paths: Vec<PathBuf> = decompiled.documents.iter().map(|d| root.join(&d.path)).collect();
        // 同じ MVID と版のソースは同じ。書き終えていれば、書き直さない。
        if !root.join(COMPLETE).is_file() {
            decompile::write(&decompiled, &root).map_err(|e| tr!("Cannot write {path}: {error}", path = root.display(), error = e))?;
            std::fs::write(root.join(COMPLETE), b"").map_err(|e| e.to_string())?;
        }
        let documents = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.modules[index].pdb = Some(Pdb::synthetic(documents, &decompiled));
        Ok((root, paths.len()))
    }

    /// 合成した PDB を付けたモジュールに、待っていた行ブレークポイントと関数ブレークポイントを置く。
    fn bind_waiting_breakpoints(&mut self, index: usize) -> Vec<Event> {
        let mut out = Vec::new();
        let module = &self.modules[index];
        for ((owner, source), breakpoints) in self.breakpoints.iter_mut() {
            for breakpoint in breakpoints.iter_mut() {
                let was_verified = breakpoint.verified_line.is_some();
                bind(module, source, breakpoint);
                if !was_verified && breakpoint.verified_line.is_some() && owner.is_reported() {
                    out.push(("breakpoint", json!({ "reason": "changed", "breakpoint": describe(breakpoint) })));
                }
            }
        }
        // シンボルのあるモジュールにだけ置く関数ブレークポイント（anyCode でないもの）は、ここで初めて置ける。
        for breakpoint in self.function_breakpoints.iter_mut().filter(|b| !b.any_code) {
            let was_verified = !breakpoint.bound.is_empty();
            bind_function(module, breakpoint);
            if !was_verified && !breakpoint.bound.is_empty() {
                out.push(("breakpoint", json!({ "reason": "changed", "breakpoint": describe_function(breakpoint) })));
            }
        }
        out
    }

    /// フレームのモジュールが逆コンパイルの対象なら、逆コンパイルする（知らせは要求の応答のあとで送る）。
    pub(super) fn decompile_for_frame(&mut self, frame: &ComPtr<ICorDebugFrame>) {
        let index = unsafe { self.frame_module(frame) }.and_then(|module| self.modules.iter().position(|m| std::ptr::eq(m, module)));
        if let Some(index) = index {
            let events = self.ensure_decompiled(index);
            self.deferred.extend(events);
        }
    }

    /// スレッドのいちばん上のフレームについて `decompile_for_frame`。
    pub(super) fn decompile_for_top_frame(&mut self, thread: u32) {
        if let Some(frame) = self.walk_frames(thread).ok().and_then(|frames| frames.into_iter().next()) {
            self.decompile_for_frame(&frame);
        }
    }

    /// ソースのパスが、まだ逆コンパイルしていない対象のモジュールのキャッシュのファイルなら、逆コンパイルする
    /// （前のセッションで置いたブレークポイントを、ここで置けるようにする）。
    pub(super) fn decompile_for_source(&mut self, source: &str) {
        let events = self.decompile_matching_source(source);
        self.deferred.extend(events);
    }

    /// `decompile_for_source` の本体。知らせを返す（モジュールの読み込みの通知の中では、そのまま送る）。
    pub(super) fn decompile_matching_source(&mut self, source: &str) -> Vec<Event> {
        let lower = source.to_lowercase();
        if !lower.contains("\\netdbg\\decompiled\\") {
            return Vec::new();
        }
        let mut out = Vec::new();
        let pending: Vec<usize> = (0..self.modules.len()).filter(|&i| self.modules[i].decompile == Decompile::Pending).collect();
        for index in pending {
            let root = self.modules[index].metadata().and_then(cache_root).map(|r| r.to_string_lossy().to_lowercase());
            if root.is_some_and(|root| lower.starts_with(&root)) {
                out.extend(self.ensure_decompiled(index));
            }
        }
        out
    }

    /// `netdbg.decompile {module}`: モジュール（パスか名前）を逆コンパイルして、ソースのファイルの一覧を返す。
    /// 対象でないモジュール（PDB のあるもの、.NET の部品）は断る。
    ///
    /// `module` が無ければ、対象のモジュールの一覧（`modules: [{ path, state }]`）。`type` があれば、その型を書いたファイルの
    /// 行（`method` があれば、その名前のメンバーの行だけ）を返す。行は `maxLines`（既定 400）まで。
    pub fn decompile_request(&mut self, arguments: &Value) -> Result<Value, String> {
        let Some(wanted) = arguments.get("module").and_then(Value::as_str) else {
            return Ok(self.decompile_targets());
        };
        let lower = wanted.to_lowercase();
        let index = self
            .modules
            .iter()
            .position(|m| m.path.to_lowercase() == lower || Path::new(&m.path).file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == lower))
            .ok_or_else(|| tr!("The module {module} is not loaded.", module = wanted))?;
        match self.modules[index].decompile {
            Decompile::NotTarget => return Err(tr!("{module} is not decompiled (it has symbols, or it is part of .NET).", module = wanted)),
            Decompile::Failed => return Err(tr!("{module} could not be decompiled (see the log).", module = wanted)),
            Decompile::Pending | Decompile::Done => {}
        }
        let events = self.ensure_decompiled(index);
        self.deferred.extend(events);
        let pdb = self.modules[index].pdb.as_ref().ok_or_else(|| tr!("Could not decompile it."))?;
        let module = &self.modules[index].path;
        let Some(type_name) = arguments.get("type").and_then(Value::as_str) else {
            return Ok(json!({ "module": module, "files": pdb.documents }));
        };
        let file = decompile::lookup::type_document(&pdb.documents, type_name)?;
        let bytes = std::fs::read(file).map_err(|e| tr!("Cannot read {path}: {error}", path = file, error = e))?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.trim_start_matches('\u{feff}').lines().collect();
        let ranges = match arguments.get("method").and_then(Value::as_str) {
            Some(method) => {
                let found = decompile::lookup::member_ranges(&lines, method);
                if found.is_empty() {
                    return Err(tr!("{type} has no {method} ({file}).", type = type_name, method = method, file = file));
                }
                found
            }
            None => vec![(1, lines.len())],
        };
        let max = arguments.get("maxLines").and_then(Value::as_u64).unwrap_or(400) as usize;
        let numbered: Vec<Value> = ranges.iter().flat_map(|&(first, last)| first..=last).filter_map(|n| Some(json!({ "line": n, "text": lines.get(n - 1)? }))).collect();
        let truncated = numbered.len() > max;
        Ok(json!({
            "module": module,
            "file": file,
            "ranges": ranges.iter().map(|&(first, last)| json!({ "startLine": first, "endLine": last })).collect::<Vec<_>>(),
            "lines": numbered.into_iter().take(max).collect::<Vec<_>>(),
            "truncated": truncated,
        }))
    }

    /// `loadedSources`: 読み込んだモジュールの PDB（逆コンパイルで合成したものも）が知っているソースのファイル。止まる前に
    /// ソースを開いて、止める場所を決めるため。逆コンパイルがまだのアプリのモジュールは、`netdbg.decompile` で
    /// ソースにできるので、`netdbgPendingDecompile` に並べる（逆コンパイルは、頼まれるまでしない）。
    pub fn loaded_sources(&self) -> Result<Value, String> {
        let mut seen = std::collections::HashSet::new();
        let mut sources = Vec::new();
        for module in &self.modules {
            let Some(pdb) = &module.pdb else { continue };
            let decompiled = module.decompile == Decompile::Done;
            for document in pdb.documents.iter().filter(|d| !d.is_empty()) {
                if !seen.insert(document.to_lowercase()) {
                    continue;
                }
                let mut source = json!({ "name": file_name(document), "path": document });
                if decompiled {
                    source["origin"] = json!("decompiled");
                }
                sources.push(source);
            }
        }
        sources.sort_by_key(|s| s["path"].as_str().unwrap_or("").to_lowercase());
        let pending: Vec<&str> = self.modules.iter().filter(|m| m.decompile == Decompile::Pending).map(|m| m.path.as_str()).collect();
        Ok(json!({ "sources": sources, "netdbgPendingDecompile": pending }))
    }

    /// 逆コンパイルの対象のモジュールと、その状態。
    fn decompile_targets(&self) -> Value {
        let modules: Vec<Value> = self
            .modules
            .iter()
            .filter_map(|m| {
                let state = match m.decompile {
                    Decompile::NotTarget => return None,
                    Decompile::Pending => "notYet",
                    Decompile::Done => "decompiled",
                    Decompile::Failed => "failed",
                };
                Some(json!({ "path": m.path, "state": state }))
            })
            .collect();
        json!({ "modules": modules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_assemblies_are_not_decompiled() {
        assert!(is_platform_assembly("System.Private.CoreLib.dll"));
        assert!(is_platform_assembly("Microsoft.Extensions.Logging.dll"));
        assert!(is_platform_assembly("mscorlib.dll"));
        assert!(!is_platform_assembly("MyApp.dll"));
        assert!(!is_platform_assembly("Newtonsoft.Json.dll"));
    }
}
