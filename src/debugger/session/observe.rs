//! 観察専用のアタッチ（D-089）: `attach` の `observeOnly` で繋いだセッションは、記録だけを取り、対象のデータを変えない。
//!
//! 共有の試験環境で「デバッガーがデータを変えていない」ことを証跡の条件にするため、次を理由つきで断る:
//! - 値の書き換え（`setVariable`、`setExpression`、式の評価の中の代入 `x = 5`、`x++`）、Edit and Continue、次の文の設定（`goto`）。
//! - 利用者が頼む関数の評価（式の評価、変数の木のプロパティ、ブレークポイントの条件とログの式の中の getter、
//!   `ToString`、メソッドの呼び出し）。フィールドの値は読める（対象のコードを動かさない）。
//!
//! エンジン自身の記録のための読み取り（SQL のコマンド、例外の文言、データ バインディング、イベントの送り主）は断らない
//! （どれも読むだけで、記録が残ることも観察の条件）。画面の入力（MCP の `ui_invoke` など）は、MCP の側が `process` の
//! 出来事の印（`netdbgObserveOnly`）を見て断る。

use super::*;

/// 観察専用で断る要求。
const REFUSED_WHILE_OBSERVING: [&str; 4] = ["setVariable", "setExpression", "goto", "netdbg.applyChanges"];

pub(super) fn refusal() -> String {
    tr!("This session only observes (attached with observeOnly): changing values, Edit and Continue, moving the next statement and running the program's functions (property getters, ToString, method calls) are refused, so that the debugger changes no data. Fields can still be read.")
}

impl Session {
    /// 観察専用で断る要求なら、その理由。
    pub fn refuse_while_observing(&self, command: &str) -> Option<String> {
        (self.observe_only && REFUSED_WHILE_OBSERVING.contains(&command)).then(refusal)
    }

    /// 利用者が頼んだ評価として `work` を行う（観察専用なら、その中の関数の評価を断る）。
    pub fn as_user_evaluation<T>(&mut self, work: impl FnOnce(&mut Session) -> T) -> T {
        let before = std::mem::replace(&mut self.user_evaluation, true);
        let result = work(self);
        self.user_evaluation = before;
        result
    }

    /// 関数の評価を断るか（観察専用で、利用者が頼んだ評価の中）。
    pub(super) fn evaluation_refused(&self) -> bool {
        self.observe_only && self.user_evaluation
    }
    /// 式の評価の中の代入（場所へ直接書く）を断るか。断るなら、その理由。関数の評価と同じく、利用者が頼んだ評価の中だけ。
    pub(super) fn refuse_write_while_observing(&self) -> Result<(), String> {
        if self.evaluation_refused() { Err(refusal()) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changes_are_refused_while_observing() {
        let mut session = Session::new();
        assert_eq!(session.refuse_while_observing("setVariable"), None, "観察専用でなければ断らない");
        session.observe_only = true;
        for command in REFUSED_WHILE_OBSERVING {
            assert!(session.refuse_while_observing(command).is_some(), "{command}");
        }
        for command in ["evaluate", "variables", "stackTrace", "continue", "netdbg.sqlBreakpoints"] {
            assert_eq!(session.refuse_while_observing(command), None, "{command}");
        }
    }

    #[test]
    fn functions_are_refused_only_inside_user_evaluations() {
        let mut session = Session::new();
        session.observe_only = true;
        assert!(!session.evaluation_refused(), "エンジン自身の読み取り（SQL など）は断らない");
        assert!(session.as_user_evaluation(|s| s.evaluation_refused()));
        assert!(!session.evaluation_refused(), "終われば元に戻る");
    }
    #[test]
    fn writes_are_refused_only_inside_user_evaluations() {
        let mut session = Session::new();
        assert!(session.as_user_evaluation(|s| s.refuse_write_while_observing()).is_ok(), "観察専用でなければ書ける");
        session.observe_only = true;
        assert!(session.refuse_write_while_observing().is_ok(), "エンジン自身の処理は断らない");
        assert!(session.as_user_evaluation(|s| s.refuse_write_while_observing()).is_err(), "利用者の評価の中の代入は断る");
    }
}
