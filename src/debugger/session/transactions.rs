//! トランザクションの全景（段階 12。障害の一連の流れの一つ）: `BeginTransaction` から `Commit` / `Rollback` までを
//! 一つのまとまりにし、その中で走った SQL（SQL ログの記録）を実行順に結び付ける。開いたままのものは、経過時間つきで返す。
//!
//! プローブ（probes.rs）の機能 `Transaction`。SQL ログと一緒に入る（同じプロバイダーのモジュール）。始めのプローブでは
//! `this`（接続）に弱い GC ハンドルを付けて覚える（GC でオブジェクトが動いても、同じ接続だと分かるように）。SQL と
//! 終わりでは、`this.Connection` を評価して、その接続の開いているトランザクションを探す。

use super::inspect::object_address;
use super::*;
use std::time::Instant;

/// 既定で見るメソッド（多重定義はすべて）。名前の終わりで、始めか終わりかを決める。
pub(super) const DEFAULT_METHODS: [&str; 18] = [
    "System.Data.SqlClient.SqlConnection.BeginTransaction",
    "System.Data.SqlClient.SqlTransaction.Commit",
    "System.Data.SqlClient.SqlTransaction.Rollback",
    "Microsoft.Data.SqlClient.SqlConnection.BeginTransaction",
    "Microsoft.Data.SqlClient.SqlTransaction.Commit",
    "Microsoft.Data.SqlClient.SqlTransaction.Rollback",
    "Oracle.ManagedDataAccess.Client.OracleConnection.BeginTransaction",
    "Oracle.ManagedDataAccess.Client.OracleTransaction.Commit",
    "Oracle.ManagedDataAccess.Client.OracleTransaction.Rollback",
    "Oracle.DataAccess.Client.OracleConnection.BeginTransaction",
    "Oracle.DataAccess.Client.OracleTransaction.Commit",
    "Oracle.DataAccess.Client.OracleTransaction.Rollback",
    "System.Data.OleDb.OleDbConnection.BeginTransaction",
    "System.Data.OleDb.OleDbTransaction.Commit",
    "System.Data.OleDb.OleDbTransaction.Rollback",
    "System.Data.Odbc.OdbcConnection.BeginTransaction",
    "System.Data.Odbc.OdbcTransaction.Commit",
    "System.Data.Odbc.OdbcTransaction.Rollback",
];
/// 呼び出し元として探すフレームの数。
const CALLER_SEARCH: usize = 40;
/// 同じ呼び出しの記録が続いたとみなす間（`BeginTransaction()` の中の `BeginTransaction(level)` など）。
const SAME_CALL_MS: u64 = 200;

/// 開いているトランザクション。
struct Open {
    id: u64,
    /// 接続（弱い GC ハンドル。GC で動いても同じものを指す）。
    connection: ComPtr<ICorDebugHandleValue>,
    thread: u32,
    started: Instant,
    caller: Value,
    /// 中で走った SQL の記録の番号。
    statements: Vec<u64>,
}

#[derive(Default)]
pub(super) struct TransactionState {
    open: Vec<Open>,
    next_id: u64,
}

impl TransactionState {
    pub(super) fn forget_target(&mut self) {
        self.open.clear();
    }
}

impl Session {
    /// トランザクションのプローブの記録（止めた形にしてある）。
    pub(super) fn record_transaction(&mut self, thread: u32, method: &str) -> Vec<Event> {
        let caller = stack::user_caller(&self.describe_top_frames(thread, CALLER_SEARCH), 1).unwrap_or(Value::Null);
        let event = match method.rsplit('.').next() {
            Some(name) if name.starts_with("Begin") => "begin",
            Some("Commit") => "commit",
            _ => "rollback",
        };
        // 多重定義が多重定義を呼ぶ（x64 では末尾呼び出しで外側のフレームが消える）: 直前の同じ記録なら、同じ呼び出し。
        if self.repeats_newest_record("transaction", SAME_CALL_MS, |last| last["threadId"] == json!(thread) && last["event"] == json!(event) && last["caller"] == caller) {
            return Vec::new();
        }
        let this = self.top_this(thread);
        let mut body = json!({ "event": event, "method": method, "threadId": thread, "caller": caller });
        if event == "begin" {
            let Some(connection) = this.and_then(|value| unsafe { weak_handle(&value) }) else { return Vec::new() };
            self.transactions.next_id += 1;
            let id = self.transactions.next_id;
            body["transactionId"] = json!(id);
            self.transactions.open.push(Open { id, connection, thread, started: Instant::now(), caller, statements: Vec::new() });
        } else {
            // 終わり: `this`（トランザクション）の接続の、開いているもの。見つからなければ、同じスレッドの最後のもの。
            let connection = this.and_then(|_| self.connection_address(thread, "this.Connection"));
            let index = connection.and_then(|address| self.open_index_of(address)).or_else(|| self.transactions.open.iter().rposition(|t| t.thread == thread));
            let Some(index) = index else {
                body["note"] = json!("no open transaction was found for it");
                return vec![("netdbg.transaction", self.probes.journal.push("transaction", body).to_json())];
            };
            let open = self.transactions.open.remove(index);
            body["transactionId"] = json!(open.id);
            body["durationMs"] = json!((open.started.elapsed().as_secs_f64() * 10_000.0).round() / 10.0);
            body["statements"] = json!(open.statements);
            body["begin"] = open.caller;
        }
        let record = self.probes.journal.push("transaction", body).to_json();
        vec![("netdbg.transaction", record)]
    }

    /// SQL ログの記録の前: コマンドの接続に開いているトランザクションがあれば、その番号（開いているものが無ければ、評価しない）。
    pub(super) fn transaction_of_command(&mut self, thread: u32) -> Option<u64> {
        if self.transactions.open.is_empty() {
            return None;
        }
        let address = self.connection_address(thread, "this.Connection")?;
        self.open_index_of(address).map(|index| self.transactions.open[index].id)
    }

    /// SQL ログの記録のあと: 開いているトランザクションに、記録の番号を足す。
    pub(super) fn add_transaction_statement(&mut self, id: u64, seq: u64) {
        if let Some(open) = self.transactions.open.iter_mut().find(|t| t.id == id) {
            open.statements.push(seq);
        }
    }

    /// `netdbg.transactions`: 開いているトランザクション（経過時間、始めた呼び出し元、中の SQL の番号）。
    pub fn open_transactions(&self) -> Result<Value, String> {
        let open: Vec<Value> = self
            .transactions
            .open
            .iter()
            .map(|t| json!({ "transactionId": t.id, "threadId": t.thread, "elapsedMs": t.started.elapsed().as_millis() as u64, "caller": t.caller, "statements": t.statements }))
            .collect();
        Ok(json!({ "open": open }))
    }

    /// 式（接続）を評価して、そのオブジェクトの番地。
    fn connection_address(&mut self, thread: u32, expression: &str) -> Option<u64> {
        match self.evaluate_expression(thread, 0, expression).ok()? {
            eval::Evaluated::Value(value) => Some(unsafe { object_address(&value) }).filter(|&a| a != 0),
            _ => None,
        }
    }

    /// 接続の番地に結び付いた、開いているトランザクションの位置（いちばん新しいもの）。
    fn open_index_of(&self, address: u64) -> Option<usize> {
        self.transactions.open.iter().rposition(|t| t.connection.cast::<ICorDebugValue>().is_ok_and(|v| unsafe { object_address(&v) } == address))
    }
}

/// オブジェクトに弱い GC ハンドルを付ける（回収されれば null。オブジェクトを生かし続けない）。
pub(super) unsafe fn weak_handle(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugHandleValue>> {
    let object = match value.cast::<ICorDebugReferenceValue>() {
        Ok(reference) => take::<ICorDebugValue>("ICorDebugReferenceValue::Dereference", |out| reference.Dereference(out)).ok()?,
        Err(_) => value.clone(),
    };
    let heap: ComPtr<ICorDebugHeapValue2> = object.cast().ok()?;
    take::<ICorDebugHandleValue>("ICorDebugHeapValue2::CreateHandle", |out| heap.CreateHandle(HANDLE_WEAK_TRACK_RESURRECTION, out)).ok()
}
