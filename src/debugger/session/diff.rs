//! 差し替わった exe の差分（B2、D-080）の要求。比べるのは `crate::debugger::diff`。ここは、いまデバッグしているプログラムを
//! 新しい方にする、いまのセッションのカバレッジと重ねる、メソッドのソースの場所を付ける、をする。

use super::*;
use crate::debugger::diff::{self, Executed, Kind};
use std::path::PathBuf;

impl Session {
    /// `netdbg.diffAssemblies { old, new?, coverage? }`: 前（`old`）と後（`new`。省くと、いまデバッグしているプログラム。
    /// `old` がフォルダーなら、そのフォルダー）のアセンブリのメソッドを比べる。`coverage` は Cobertura XML のパス。省くと、
    /// いまのセッションのカバレッジ（新しい方がデバッグしているものなら）と重ねて、再試験の範囲を出す。
    pub fn diff_assemblies(&mut self, arguments: &Value) -> Result<Value, String> {
        let old = arguments.get("old").and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or_else(|| tr!("netdbg.diffAssemblies has no old (the previous exe or folder)."))?;
        let old = Path::new(old);
        // いまデバッグしているプログラム（起動したものか、アタッチした先の exe）。
        let program = self.config.as_ref().map(|c| PathBuf::from(&c.program)).or_else(|| self.attach_to.and_then(target::process_image_path).map(PathBuf::from));
        let new = match arguments.get("new").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            Some(new) => PathBuf::from(new),
            None => {
                let program = program.clone().ok_or_else(|| tr!("Specify new (the later exe or folder), or debug the exe to compare."))?;
                if old.is_dir() { program.parent().map(Path::to_path_buf).unwrap_or(program) } else { program }
            }
        };
        let started = std::time::Instant::now();
        let mut changes = diff::compare(&diff::read(old)?, &diff::read(&new)?);
        // 新しい方が、いまデバッグしているもの（か、そのフォルダー）なら、いまのセッションのカバレッジとソースを使える。
        let running = program.as_deref().is_some_and(|p| same_path(p, &new) || p.parent().is_some_and(|dir| same_path(dir, &new)));
        match arguments.get("coverage").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            Some(path) => {
                let xml = std::fs::read_to_string(path).map_err(|e| tr!("Cannot read {path}: {error}", path = path, error = e))?;
                diff::overlay(&mut changes, &Executed::Cobertura(diff::read_cobertura(&xml)));
            }
            None if running && !self.coverage_data().modules.is_empty() => diff::overlay(&mut changes, &Executed::Session(self.coverage_data())),
            None => {}
        }
        crate::log!("diff {} → {}: {} changes in {:.1} ms", old.display(), new.display(), changes.len(), started.elapsed().as_secs_f64() * 1000.0);
        let mut list = Vec::with_capacity(changes.len());
        for change in &changes {
            let mut entry = diff::change_json(change);
            if running && change.kind != Kind::Removed {
                if let Some((path, line)) = self.method_source(&change.assembly, change.token) {
                    entry["source"] = json!({ "path": path });
                    entry["line"] = json!(line);
                }
            }
            list.push(entry);
        }
        Ok(json!({
            "old": old.to_string_lossy(),
            "new": new.to_string_lossy(),
            "summary": diff::summary(&changes),
            "coverage": changes.iter().any(|c| c.executed.is_some()),
            "changes": list,
        }))
    }

    /// 読み込まれているアセンブリ（名前）のメソッドの、ソースのファイルと最初の行。PDB が無ければ逆コンパイルする。
    fn method_source(&mut self, assembly: &str, token: u32) -> Option<(String, u32)> {
        let index = self.modules.iter().position(|m| Path::new(&m.path).file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(assembly)))?;
        let events = self.ensure_decompiled(index);
        self.deferred.extend(events);
        let pdb = self.modules[index].pdb.as_ref()?;
        let point = pdb.first_visible_point(token)?;
        Some((pdb.document(point.document)?.to_string(), point.start_line))
    }
}

/// 同じパスか（大文字と小文字、区切りの違いを除く）。
fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |p: &Path| crate::debugger::coverage::normalize(&p.to_string_lossy());
    normalize(a) == normalize(b)
}
