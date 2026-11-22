//! 値の変化で止める（VS Code の Break on Value Change。段階 11）。拡張の中継から移した判定。
//!
//! 代入している行（書き込みの場所）は、拡張がワークスペースの C# から探して `netdbg.setValueWatches` で渡す
//! （ソースはエンジンに無い）。エンジンは、その行に隠しのブレークポイント（持ち主 `Owner::Watch`。利用者には見せない）を
//! 置く。当たったら、名前の値を読み、その行をステップ オーバーして、もう一度読む。変わっていれば理由 `data breakpoint`
//! で止まり、変わっていなければ、何も無かったことにして続ける。

use super::*;

/// 進行中の、書き込みの行のステップ オーバー（変わったかを確かめる）。
pub(super) struct WatchStep {
    thread: u32,
    /// 行の前の値（名前、値）。読めなかった名前は入れない。
    before: Vec<(String, String)>,
    /// 利用者のステップの途中で当たった（変わらなくても、ステップの終わりとして止まる。続けると、ステップが消える）。
    interrupted_step: bool,
}

impl Session {
    /// `netdbg.setValueWatches { watches: [{ name, sites: [{ path, line }] }] }`: 見張る名前と、その書き込みの行を、
    /// 送られてきた一覧で置き換える。名前ごとに、置けた行があるか（`verified`）と、行の数を返す。
    pub fn set_value_watches(&mut self, arguments: &Value) -> Result<Value, String> {
        let watches = arguments.get("watches").and_then(Value::as_array).cloned().unwrap_or_default();
        // ファイルごとの「行、その行に書き込む名前」。同じ行に書き込む名前が複数あれば、一つのブレークポイントにまとめる。
        let mut by_path: BTreeMap<String, BTreeMap<u32, Vec<String>>> = BTreeMap::new();
        for watch in &watches {
            let Some(name) = watch.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else { continue };
            for site in watch.get("sites").and_then(Value::as_array).into_iter().flatten() {
                let (Some(path), Some(line)) = (site.get("path").and_then(Value::as_str), site.get("line").and_then(Value::as_u64)) else { continue };
                by_path.entry(path.to_string()).or_default().entry(line as u32).or_default().push(name.to_string());
            }
        }
        // 前の監視の行のうち、今回は無いファイルの分は外す。
        let stale: Vec<String> = self.breakpoints.keys().filter(|(owner, path)| *owner == Owner::Watch && !by_path.contains_key(path)).map(|(_, path)| path.clone()).collect();
        for path in stale {
            self.set_breakpoints(Owner::Watch, &json!({ "source": { "path": path }, "breakpoints": [] }))?;
        }
        self.watch_sites.clear();
        let mut verified: BTreeMap<String, (bool, usize)> = BTreeMap::new();
        for (path, lines) in &by_path {
            let body = self.set_breakpoints(Owner::Watch, &json!({ "source": { "path": path }, "breakpoints": lines.keys().map(|line| json!({ "line": line })).collect::<Vec<_>>() }))?;
            for (placed, names) in body["breakpoints"].as_array().into_iter().flatten().zip(lines.values()) {
                let ok = placed["verified"].as_bool().unwrap_or(false);
                for name in names {
                    let entry = verified.entry(name.clone()).or_default();
                    entry.0 |= ok;
                    entry.1 += 1;
                }
                if let Some(id) = placed["id"].as_i64() {
                    self.watch_sites.insert(id, names.clone());
                }
            }
        }
        let results: Vec<Value> = watches
            .iter()
            .filter_map(|w| w.get("name").and_then(Value::as_str).map(str::trim))
            .map(|name| {
                let (ok, sites) = verified.get(name).copied().unwrap_or_default();
                json!({ "name": name, "verified": ok, "sites": sites })
            })
            .collect();
        Ok(json!({ "watches": results }))
    }

    /// ブレークポイントが、値の監視の書き込みの行か。
    pub(super) fn is_watch_site(&self, id: i64) -> bool {
        self.watch_sites.contains_key(&id)
    }

    /// 書き込みの行に当たった（止めた形にしてある）。名前の値を読み、行のステップ オーバーを仕掛ける。仕掛けたら `true`
    /// （呼び出し側が対象を動かす）。どの名前も読めなければ `false`（何もしないで続ける）。
    pub(super) fn start_value_watch(&mut self, thread: u32, sites: &[i64]) -> bool {
        let mut names: Vec<String> = sites.iter().filter_map(|id| self.watch_sites.get(id)).flatten().cloned().collect();
        names.dedup();
        let before: Vec<(String, String)> = names.into_iter().filter_map(|name| self.evaluate_shown(thread, &name).ok().map(|(value, _)| (name, value))).collect();
        if before.is_empty() {
            return false;
        }
        let interrupted_step = self.stepper.is_some();
        if let Err(e) = self.start_step(thread, StepKind::Over) {
            crate::log!("value watch: could not step over the write: {e}");
            return false;
        }
        self.stepping_in = false;
        self.leaving_filtered = false;
        self.watch_step = Some(WatchStep { thread, before, interrupted_step });
        true
    }

    /// ステップが終わった所が、書き込みの行のステップ オーバーの終わりか。
    pub(super) fn is_watch_step(&self, thread: u32) -> bool {
        self.watch_step.as_ref().is_some_and(|w| w.thread == thread)
    }

    /// 書き込みの行を越えた（止めた形にしてある）。値が変わっていれば（利用者のステップの途中なら、変わらなくても）、
    /// 止まる通知を返す。`None` なら、何も無かったことにして続ける。
    pub(super) fn finish_value_watch(&mut self, thread: u32) -> Option<Event> {
        let watch = self.watch_step.take()?;
        let mut changes = Vec::new();
        for (name, old) in watch.before {
            if let Ok((now, _)) = self.evaluate_shown(thread, &name) {
                if now != old {
                    changes.push(format!("{name}: {old} → {now}"));
                }
            }
        }
        if changes.is_empty() {
            return watch.interrupted_step.then(|| ("stopped", json!({ "reason": "step", "threadId": thread, "allThreadsStopped": true })));
        }
        let text = changes.join(", ");
        Some(("stopped", json!({ "reason": "data breakpoint", "description": text, "text": text, "threadId": thread, "allThreadsStopped": true })))
    }
}
