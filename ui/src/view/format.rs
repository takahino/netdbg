//! 画面に出す文字の整形（時刻、JSON の値）。

use serde_json::Value;

/// エンジンの時刻（UNIX 時刻のミリ秒）を、手元の時刻（時:分:秒。`millis` ならミリ秒も）にする。
pub fn clock(ms: f64, millis: bool) -> String {
    if ms <= 0.0 {
        return String::new();
    }
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    let seconds = format!("{:02}:{:02}:{:02}", date.get_hours(), date.get_minutes(), date.get_seconds());
    if millis {
        format!("{seconds}.{:03}", date.get_milliseconds())
    } else {
        seconds
    }
}

/// JSON の値を、そのまま読める文字にする（文字列は引用符なし、null は空）。
pub fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}