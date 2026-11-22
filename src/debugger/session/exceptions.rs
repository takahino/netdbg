//! 例外で止まる条件と、止まった例外の説明（exceptionInfo）。

use super::*;

/// 例外で止まる条件（`setExceptionBreakpoints`）。誰も捕まえない例外では、条件に関係なく止まる。
#[derive(Debug, Default, PartialEq)]
pub struct ExceptionFilters {
    /// フィルター `all`: 投げられたらすぐ止まる。
    all: bool,
    /// `all` に付いた条件。型名の一覧と、「それ以外」を表す否定。
    conditions: Vec<(bool, Vec<String>)>,
    /// フィルター `user-unhandled`: 利用者のコードで投げられ（か、利用者のコードを通り）、利用者のコードではない所で
    /// 捕まえられる例外で止まる（Visual Studio、netcoredbg と同じ）。
    pub user_unhandled: bool,
}

impl ExceptionFilters {
    /// `setExceptionBreakpoints` の引数を読む。条件の書き方は netcoredbg と同じ:
    /// `A, B` はその型で止まり、`!A, B` はその型以外で止まる。空は「すべて」。
    pub fn parse(arguments: &Value) -> ExceptionFilters {
        let mut filters = ExceptionFilters::default();
        if let Some(Value::Array(ids)) = arguments.get("filters") {
            filters.all = ids.iter().any(|id| id.as_str() == Some("all"));
            filters.user_unhandled = ids.iter().any(|id| id.as_str() == Some("user-unhandled"));
        }
        if let Some(Value::Array(options)) = arguments.get("filterOptions") {
            filters.user_unhandled |= options.iter().any(|o| o.get("filterId").and_then(Value::as_str) == Some("user-unhandled"));
            for option in options.iter().filter(|o| o.get("filterId").and_then(Value::as_str) == Some("all")) {
                let condition = option.get("condition").and_then(Value::as_str).unwrap_or("").trim();
                let (negate, list) = match condition.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, condition),
                };
                let types: Vec<String> = list.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).map(str::to_string).collect();
                if types.is_empty() {
                    filters.all = true;
                } else {
                    filters.conditions.push((negate, types));
                }
            }
        }
        filters
    }

    /// この型の例外が投げられたときに止まるか。
    pub fn breaks_on_throw(&self, type_name: &str) -> bool {
        self.all || self.conditions.iter().any(|(negate, types)| types.iter().any(|t| t == type_name) != *negate)
    }
}

impl Session {
    /// `setExceptionBreakpoints`。
    pub fn set_exception_breakpoints(&mut self, arguments: &Value) -> Result<Value, String> {
        self.exception_filters = ExceptionFilters::parse(arguments);
        Ok(json!({}))
    }

    /// スレッドで処理中の例外のオブジェクト。
    pub(super) fn current_exception(&self, thread_id: u32) -> Option<ComPtr<ICorDebugValue>> {
        let process = self.process.as_ref()?;
        unsafe {
            let mut raw = null_mut();
            process.GetThread(thread_id, &mut raw);
            let thread = ComPtr::<ICorDebugThread>::from_raw(raw)?;
            let mut exception = null_mut();
            thread.GetCurrentException(&mut exception);
            ComPtr::from_raw(exception)
        }
    }

    /// `exceptionInfo`: 止まる理由になった例外の説明。形は netcoredbg の応答に合わせてある（tests/fixtures）。
    pub fn exception_info(&mut self, arguments: &Value) -> Result<Value, String> {
        let thread = arguments.get("threadId").and_then(Value::as_u64).ok_or_else(|| tr!("exceptionInfo has no threadId."))? as u32;
        let stopped = self.stopped_exception.as_ref().filter(|e| e.thread == thread && self.state == State::Stopped).ok_or_else(|| tr!("This thread is not stopped on an exception."))?;
        let exception = self.current_exception(thread).ok_or_else(|| tr!("Cannot read the exception object."))?;
        let full_name = unsafe { value::describe(&exception, &self.modules) }.type_name;
        let (break_mode, text) = (stopped.break_mode, stopped.text.clone());
        let mut details = self.exception_details(thread, &exception, 0);
        // 止まった場所のモジュール（`text` の「in <モジュール>」。stopped の通知と同じ）。
        let stopped_module = text.rsplit(" in ").next().map(|rest| rest.trim_end_matches(" but was not handled in user code").to_string()).unwrap_or_default();
        // Source が読めないとき（評価できない停止）は、止まった場所のモジュールの名前から拡張子を落としたもの。
        if details["source"].as_str().is_none_or(str::is_empty) {
            details["source"] = json!(stopped_module.trim_end_matches(".dll").trim_end_matches(".exe"));
        }
        // description のモジュールは、netcoredbg と同じく例外の Source（投げ直された例外は、最初に投げた所）。
        // 止まった場所のモジュールと同じアセンブリなら、そのファイル名（.exe も）を使う。
        let source = details["source"].as_str().unwrap_or_default().to_string();
        let same_assembly = stopped_module.strip_suffix(".dll").or_else(|| stopped_module.strip_suffix(".exe")) == Some(source.as_str());
        let module = if same_assembly || source.is_empty() { stopped_module } else { format!("{source}.dll") };
        let head = match break_mode {
            "unhandled" => format!("An unhandled exception of type '{full_name}' occurred in {module}"),
            "userUnhandled" => format!("An exception of type '{full_name}' occurred in {module} but was not handled in user code"),
            _ => format!("Exception thrown: '{full_name}' in {module}"),
        };
        let message = details["message"].as_str().unwrap_or_default();
        let quoted = if message.is_empty() { String::new() } else { format!(": '{message}'") };
        // 内側の例外があれば、いちばん内側の型を書き足す（文言は netcoredbg と同じ）。
        let mut innermost = None;
        let mut current = &details;
        while let Some(inner) = current.get("innerException").and_then(|inner| inner.get(0)) {
            innermost = inner["fullTypeName"].as_str();
            current = inner;
        }
        let inner_note = innermost.map_or(String::new(), |name| format!("\n Inner exceptions found, see $exception in variables window for more details.\n Innermost exception: {name}"));
        let description = format!("{head}{quoted}{inner_note}");
        Ok(json!({
            "exceptionId": format!("CLR/{full_name}"),
            "description": description,
            "breakMode": break_mode,
            "details": details,
        }))
    }

    /// 例外の説明（DAP の ExceptionDetails）。内側の例外（`InnerException`）は `innerException` に入れ子にする。
    /// 形と読み方は netcoredbg と同じ: 文言は `_message` のフィールド（無ければ `Message`。AggregateException は、内側の
    /// 例外の文言を足さない基の文言になる）、`Source` と `StackTrace` はプロパティ（対象のコードを実行して読む）。
    fn exception_details(&mut self, thread: u32, exception: &ComPtr<ICorDebugValue>, depth: u32) -> Value {
        const MAX_INNER: u32 = 8;
        let full_name = unsafe { value::describe(exception, &self.modules) }.type_name;
        let message = unsafe { value::string_field(exception, &self.modules, &["_message"]) }.or_else(|| self.string_property(thread, exception, "get_Message")).unwrap_or_default();
        let short_name = full_name.rsplit('.').next().unwrap_or(&full_name).to_string();
        let mut details = json!({
            "message": message,
            "typeName": short_name,
            "fullTypeName": full_name,
            "evaluateName": "$exception",
            "source": self.string_property(thread, exception, "get_Source").unwrap_or_default(),
            "formattedDescription": if message.is_empty() { format!("**{full_name}**") } else { format!("**{full_name}** '{message}'") },
        });
        // 内側の例外は、投げられていなければスタックトレースが無い（netcoredbg は空の文字列で返す）。
        match self.string_property(thread, exception, "get_StackTrace") {
            Some(stack_trace) => details["stackTrace"] = json!(stack_trace),
            None if depth > 0 => details["stackTrace"] = json!(""),
            None => {}
        }
        if depth < MAX_INNER {
            let inner = self.call_parameterless(thread, exception, "get_InnerException").and_then(|result| result.ok()).flatten();
            if let Some(inner) = inner.filter(|inner| unsafe { value::argument_kind(inner) } != value::ArgumentKind::Null) {
                details["innerException"] = json!([self.exception_details(thread, &inner, depth + 1)]);
            }
        }
        details
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_filters_a_thrown_exception_does_not_stop() {
        let filters = ExceptionFilters::parse(&json!({ "filters": [] }));
        assert!(!filters.breaks_on_throw("System.FormatException"));
    }

    #[test]
    fn the_all_filter_stops_on_every_type() {
        let filters = ExceptionFilters::parse(&json!({ "filters": ["all", "user-unhandled"] }));
        assert!(filters.breaks_on_throw("System.FormatException"));
    }

    #[test]
    fn a_condition_lists_the_types_to_stop_on() {
        let filters = ExceptionFilters::parse(&json!({ "filters": [], "filterOptions": [{ "filterId": "all", "condition": "System.FormatException, System.IO.IOException" }] }));
        assert!(filters.breaks_on_throw("System.FormatException"));
        assert!(filters.breaks_on_throw("System.IO.IOException"));
        assert!(!filters.breaks_on_throw("System.InvalidOperationException"));
    }

    #[test]
    fn a_negated_condition_lists_the_types_to_run_through() {
        let filters = ExceptionFilters::parse(&json!({ "filterOptions": [{ "filterId": "all", "condition": "!System.FormatException" }] }));
        assert!(!filters.breaks_on_throw("System.FormatException"));
        assert!(filters.breaks_on_throw("System.InvalidOperationException"));
    }

    #[test]
    fn an_empty_condition_means_every_type() {
        let filters = ExceptionFilters::parse(&json!({ "filterOptions": [{ "filterId": "all", "condition": "  " }] }));
        assert!(filters.breaks_on_throw("System.FormatException"));
    }

    #[test]
    fn the_user_unhandled_filter_is_read_from_filters_and_options() {
        assert!(ExceptionFilters::parse(&json!({ "filters": ["user-unhandled"] })).user_unhandled);
        assert!(ExceptionFilters::parse(&json!({ "filterOptions": [{ "filterId": "user-unhandled" }] })).user_unhandled);
        assert!(!ExceptionFilters::parse(&json!({ "filters": ["all"] })).user_unhandled);
    }

    #[test]
    fn options_of_other_filters_are_ignored() {
        let filters = ExceptionFilters::parse(&json!({ "filterOptions": [{ "filterId": "user-unhandled", "condition": "System.FormatException" }] }));
        assert!(!filters.breaks_on_throw("System.FormatException"));
    }
}
