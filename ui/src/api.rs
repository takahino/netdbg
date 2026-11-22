//! エンジンとのつなぎ: DAP の要求（`POST /dap`）、出来事（`GET /events`、Server-Sent Events）、画面だけの口
//! （`/fs`、`/sys`、`/ui`、`/startup`、`/relaunch`、`/quit`）。
//!
//! トークンは、エンジンが開いたページの URL（`?token=`）から取る。`fetch` には `Authorization: Bearer` で付け、
//! `EventSource` と `<img>` は頭を付けられないので URL で渡す。

use serde_json::{json, Value};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{EventSource, Headers, MessageEvent, Request, RequestInit, Response};

fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

/// ページの URL のトークン。
pub fn token() -> String {
    let search = window().location().search().unwrap_or_default();
    web_sys::UrlSearchParams::new_with_str(&search).ok().and_then(|p| p.get("token")).unwrap_or_default()
}

/// ブラウザに覚えておいた文字（`localStorage`）。
pub fn remembered_text(name: &str) -> Option<String> {
    window().local_storage().ok().flatten().and_then(|s| s.get_item(&format!("netdbg.{name}")).ok().flatten())
}

/// 文字をブラウザに覚えておく。
pub fn remember_text(name: &str, value: &str) {
    if let Some(storage) = window().local_storage().ok().flatten() {
        let _ = storage.set_item(&format!("netdbg.{name}"), value);
    }
}

/// ブラウザに覚えておいた切り替え。無ければ `default`。
pub fn remembered(name: &str, default: bool) -> bool {
    remembered_text(name).map_or(default, |v| v == "true")
}

/// 切り替えをブラウザに覚えておく。
pub fn remember(name: &str, value: bool) {
    remember_text(name, if value { "true" } else { "false" });
}

/// URL の問い合わせに入れる形（`%xx`）。
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 窓の写しの URL（`<img>` の `src`）。`nonce` は、撮り直しのたびに変えて、ブラウザの覚えを使わせない。
pub fn capture_url(window: u64, nonce: u64) -> String {
    format!("/ui/capture?window={window}&n={nonce}&token={}", token())
}

/// 出来事を受け取り始める。`on_message` は、DAP のメッセージ（JSON）を 1 つずつ受け取る。
pub fn subscribe(on_message: impl Fn(Value) + 'static) {
    let Ok(source) = EventSource::new(&format!("/events?token={}", token())) else { return };
    let handler = Closure::<dyn Fn(MessageEvent)>::new(move |event: MessageEvent| {
        if let Some(message) = event.data().as_string().and_then(|text| serde_json::from_str(&text).ok()) {
            on_message(message);
        }
    });
    source.set_onmessage(Some(handler.as_ref().unchecked_ref()));
    // 画面が開いている間ずっと使うので、手放さない。
    handler.forget();
}

/// DAP の要求を送る。成功なら本文（無ければ null）、失敗なら利用者に見せる理由。
pub async fn dap(command: &str, arguments: Value) -> Result<Value, String> {
    let response = send("POST", "/dap", Some(json!({ "command": command, "arguments": arguments }).to_string())).await?;
    if response["success"].as_bool() == Some(true) {
        Ok(response.get("body").cloned().unwrap_or(Value::Null))
    } else {
        Err(response["message"].as_str().unwrap_or(crate::i18n::tr("Failed.")).to_string())
    }
}

/// 同じ口の MCP のツールを呼ぶ（`ui_replay` など、MCP にだけあるもの）。答えの文が JSON なら、その値。
pub async fn mcp(tool: &str, arguments: Value) -> Result<Value, String> {
    let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": arguments } });
    let answer = send("POST", "/mcp", Some(call.to_string())).await?;
    let text = answer["result"]["content"][0]["text"].as_str().or(answer["error"]["message"].as_str()).unwrap_or("").to_string();
    if answer["result"]["isError"] == true || answer.get("error").is_some() {
        return Err(text);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}
/// 画面だけの口（GET）。
pub async fn get(path: &str) -> Result<Value, String> {
    send("GET", path, None).await
}

/// 画面だけの口（POST）。
pub async fn post(path: &str, body: Value) -> Result<Value, String> {
    send("POST", path, Some(body.to_string())).await
}

async fn send(method: &str, path: &str, body: Option<String>) -> Result<Value, String> {
    let describe = |e: JsValue| format!("{path}: {}", e.as_string().unwrap_or_else(|| format!("{e:?}")));
    let headers = Headers::new().map_err(describe)?;
    headers.set("Authorization", &format!("Bearer {}", token())).map_err(describe)?;
    let init = RequestInit::new();
    init.set_method(method);
    if let Some(body) = &body {
        headers.set("Content-Type", "application/json").map_err(describe)?;
        init.set_body(&JsValue::from_str(body));
    }
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(path, &init).map_err(describe)?;
    let response: Response = JsFuture::from(window().fetch_with_request(&request)).await.map_err(|_| crate::i18n::tr("Cannot reach the engine (it may have ended).").to_string())?.dyn_into().map_err(describe)?;
    let text = JsFuture::from(response.text().map_err(describe)?).await.map_err(describe)?.as_string().unwrap_or_default();
    let value: Value = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))? };
    if !response.ok() {
        return Err(value["message"].as_str().map(String::from).unwrap_or_else(|| format!("{path}: {}", response.status())));
    }
    Ok(value)
}
