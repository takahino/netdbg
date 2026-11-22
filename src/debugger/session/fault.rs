//! 障害の一連の流れ（段階 12）: 障害票用の環境と、例外に至るまでの流れ。どちらも、出来事の列（journal.rs）を一つの
//! 列として使う（SQL、例外、ダイアログ、トランザクション、イベントを、別々の一覧にしない）。
//!
//! - 環境: 対象のランタイムが始まったとき（最初のモジュールの読み込み）に、1 つ積む（種類 `environment`）。読むのは
//!   エンジンの側（environment.rs）。例外とダイアログの記録には、その番号（`environmentSeq`）を付ける。
//! - 例外の前: 例外の記録に、同じスレッドの、直前の利用者の操作（イベント トレースのハンドラー、ダイアログ）からの
//!   出来事を、要約して添える（`context`）。最後の SQL だけでなく、例外に至るまでの流れにする。

use super::*;
use crate::debugger::environment;

/// 例外の前の流れとして添える出来事の数と、さかのぼる時間（利用者の操作が見つからないとき）の上限。
const CONTEXT_RECORDS: usize = 30;
const CONTEXT_MS: u64 = 60_000;
/// 要約の文字数。
const SUMMARY_LENGTH: usize = 120;

impl Session {
    /// モジュールが読み込まれた: 最初のもの（System.Private.CoreLib / mscorlib）なら、環境を記録する。
    pub(super) fn record_environment_once(&mut self, corelib: &str) -> Vec<Event> {
        if self.environment_seq.is_some() {
            return Vec::new();
        }
        let lower = corelib.to_lowercase();
        if !(lower.ends_with("system.private.corelib.dll") || lower.ends_with("mscorlib.dll")) {
            return Vec::new();
        }
        let program = self.config.as_ref().map(|c| c.program.clone()).or_else(|| self.process_image());
        let mut body = environment::machine_facts();
        body["bitness"] = json!(if std::mem::size_of::<usize>() == 8 { "x64" } else { "x86" });
        body["runtime"] = json!(environment::runtime_description(corelib));
        if let Some(program) = &program {
            body["program"] = json!(program);
            let strings: Vec<Value> = environment::connection_strings(Path::new(program))
                .into_iter()
                .map(|c| json!({ "name": c.name, "connectionString": c.value, "providerName": c.provider, "file": c.file }))
                .collect();
            body["connectionStrings"] = json!(strings);
        }
        if let Some(config) = &self.config {
            body["commandLine"] = json!(config.command_line);
            let cwd = config.cwd.clone().or_else(|| Path::new(&config.program).parent().map(|p| p.display().to_string()));
            body["workingDirectory"] = json!(cwd);
        }
        let record = self.probes.journal.push("environment", body);
        self.environment_seq = Some(record.seq);
        vec![("netdbg.environment", record.to_json())]
    }

    /// 対象のプロセスの exe（アタッチしたとき）。
    fn process_image(&self) -> Option<String> {
        if self.process_handle.is_null() {
            return None;
        }
        let mut name = vec![0u16; 1024];
        let mut size = name.len() as u32;
        (unsafe { crate::win32::QueryFullProcessImageNameW(self.process_handle, 0, name.as_mut_ptr(), &mut size) } != 0).then(|| crate::win32::from_wide(&name[..size as usize]))
    }

    /// 記録に、環境の番号を付ける（例外とダイアログ）。
    pub(super) fn mark_environment(&self, body: &mut Value) {
        if let Some(seq) = self.environment_seq {
            body["environmentSeq"] = json!(seq);
        }
    }

    /// 例外の前の流れ: 同じスレッドの出来事を新しい方からさかのぼり、利用者の操作（イベント、ダイアログ）に着いたら
    /// そこまで（無ければ、数と時間の上限まで）。古い順に、番号、種類、時刻、要約。
    pub(super) fn exception_context(&self, thread: u32) -> Value {
        let now = crate::debugger::journal::now_ms();
        let mut context = Vec::new();
        for record in self.probes.journal.recent() {
            if context.len() >= CONTEXT_RECORDS || now.saturating_sub(record.time) > CONTEXT_MS {
                break;
            }
            if record.body["threadId"].as_u64() != Some(u64::from(thread)) {
                continue;
            }
            context.push(json!({ "seq": record.seq, "kind": record.kind, "time": record.time, "summary": summary(record.kind, &record.body) }));
            if matches!(record.kind, "event" | "dialog") {
                break;
            }
        }
        context.reverse();
        json!(context)
    }
}

/// 出来事の要約（1 行）。
fn summary(kind: &str, body: &Value) -> String {
    let text = |key: &str| body[key].as_str().unwrap_or("").to_string();
    let line = match kind {
        "sql" => {
            let mut s = crate::debugger::sqllog::normalize(body["sql"].as_str().or_else(|| body["commandText"].as_str()).unwrap_or(""));
            if let Some(ms) = body["durationMs"].as_f64() {
                s.push_str(&format!(" ({ms} ms)"));
            }
            s
        }
        "dialog" => [text("caption"), text("text"), text("form")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(": "),
        "event" => format!("{} (sender {})", text("handler"), text("sender")),
        "transaction" => format!("{} #{}", text("event"), body["transactionId"]),
        "binding" => format!("{} {}.{} ↔ {}", text("direction"), text("control"), text("property"), text("member")),
        "exception" => format!("{}: {}", text("typeName"), text("message")),
        _ => String::new(),
    };
    if line.chars().count() > SUMMARY_LENGTH {
        line.chars().take(SUMMARY_LENGTH).collect::<String>() + "…"
    } else {
        line
    }
}
