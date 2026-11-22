//! MCP の側から見た、対象の様子。本体の答えと出来事から追う（DAP のクライアントの操作でも、MCP の操作でも）。

use crate::debugger::session::Event;
use serde_json::Value;
use std::collections::VecDeque;

/// 覚えておく対象の出力の行数。
pub(super) const OUTPUT_LINES: usize = 2000;

/// 対象の状態。
pub(super) enum Target {
    NotStarted,
    Running,
    /// `stopped` の出来事の本文。
    Stopped(Value),
    Exited(Option<i64>),
}

pub(super) struct State {
    pub target: Target,
    pub process_id: Option<u64>,
    /// 対象の出力の最後の行と、これまでに書かれた行の数（`debug_output` のカーソル）。
    pub output: VecDeque<String>,
    pub output_total: u64,
    /// 読み込まれたモジュール（`module` の出来事の `module`）。
    pub modules: Vec<Value>,
    /// `--mcp` で、本体に `initialize` を送ったか。
    pub initialized: bool,
    /// 観察専用のアタッチ（`process` の出来事の `netdbgObserveOnly`。画面の入力を断る。D-089）。
    pub observe_only: bool,
}

impl State {
    pub fn new() -> State {
        State { target: Target::NotStarted, process_id: None, output: VecDeque::new(), output_total: 0, modules: Vec::new(), initialized: false, observe_only: false }
    }

    /// 本体の答え（`answered`: 要求と、成功したか）と出来事を見て、様子を追う。答えを先に見る（`pause` の答えのあとに
    /// 止まった知らせが来る、の順のため）。
    pub fn observe(&mut self, answered: Option<(&str, bool)>, events: &[Event]) {
        // 対象を進めた（DAP のクライアントの操作も、MCP の操作も）。
        if let Some((command, true)) = answered {
            // 新しい対象（終わったあとに、DAP のクライアントが同じエンジンで次を始めた。専用の画面の「もう一度」など）。
            if matches!(command, "launch" | "attach") {
                self.target = Target::NotStarted;
            }
            if matches!(command, "continue" | "next" | "stepIn" | "stepOut" | "configurationDone") && !matches!(self.target, Target::Exited(_)) {
                self.target = Target::Running;
            }
        }
        for (name, body) in events {
            match *name {
                "stopped" => self.target = Target::Stopped(body.clone()),
                "continued" => self.target = Target::Running,
                "exited" => self.target = Target::Exited(body.get("exitCode").and_then(Value::as_i64)),
                "terminated" if !matches!(self.target, Target::Exited(_)) => self.target = Target::Exited(None),
                "process" => {
                    self.process_id = body.get("systemProcessId").and_then(Value::as_u64);
                    self.observe_only = body.get("netdbgObserveOnly").and_then(Value::as_bool) == Some(true);
                }
                "output" => {
                    let category = body.get("category").and_then(Value::as_str).unwrap_or("console");
                    for line in body.get("output").and_then(Value::as_str).unwrap_or("").lines() {
                        if self.output.len() == OUTPUT_LINES {
                            self.output.pop_front();
                        }
                        self.output_total += 1;
                        self.output.push_back(if category == "stdout" { line.to_string() } else { format!("[{category}] {line}") });
                    }
                }
                "module" => {
                    let module = body.get("module").cloned().unwrap_or(Value::Null);
                    let path = module.get("path").cloned();
                    self.modules.retain(|m| m.get("path").cloned() != path);
                    if body.get("reason").and_then(Value::as_str) != Some("removed") {
                        self.modules.push(module);
                    }
                }
                _ => {}
            }
        }
    }

    /// 新しいセッション（`--mcp` で、終わったあとにまた起動する）。出力とモジュールの記録は残す。
    pub fn restart(&mut self) {
        self.initialized = false;
        self.process_id = None;
    }

    /// 動いていない理由の書き出し。
    pub fn not_running(&self) -> String {
        match self.target {
            Target::NotStarted => tr!("Not started yet. "),
            Target::Exited(_) => tr!("The target has ended. "),
            Target::Stopped(_) => tr!("The target is stopped. "),
            Target::Running => tr!("The target is running. "),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_target_follows_answers_and_events() {
        let mut state = State::new();
        state.observe(Some(("configurationDone", true)), &[("process", json!({ "systemProcessId": 42 }))]);
        assert!(matches!(state.target, Target::Running));
        assert_eq!(state.process_id, Some(42));
        state.observe(None, &[("stopped", json!({ "reason": "breakpoint", "threadId": 7 }))]);
        assert!(matches!(&state.target, Target::Stopped(body) if body["threadId"] == 7));
        // 答えのあとに止まった知らせが来る（pause）。
        state.observe(Some(("pause", true)), &[("stopped", json!({ "reason": "pause" }))]);
        assert!(matches!(state.target, Target::Stopped(_)));
        state.observe(None, &[("exited", json!({ "exitCode": 3 })), ("terminated", Value::Null)]);
        assert!(matches!(state.target, Target::Exited(Some(3))));
        // 同じエンジンで、DAP のクライアントが次の対象を始めた。
        state.observe(Some(("launch", true)), &[]);
        state.observe(Some(("configurationDone", true)), &[]);
        assert!(matches!(state.target, Target::Running));
    }

    #[test]
    fn output_is_kept_by_line_with_its_category() {
        let mut state = State::new();
        state.observe(None, &[("output", json!({ "category": "stdout", "output": "a\nb\n" })), ("output", json!({ "category": "stderr", "output": "c" }))]);
        assert_eq!(state.output, ["a", "b", "[stderr] c"]);
        assert_eq!(state.output_total, 3);
    }
}
