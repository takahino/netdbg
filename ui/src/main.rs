//! 専用の画面（段階 13、D-062）。エンジンの exe に埋め込まれ、`netdbg --ui` が開く。VS Code と同じ DAP を
//! 話す（`api`）。画面の処理はすべて Rust（Leptos の CSR）で、手で書く JavaScript は無い。
//!
//! - `model`: ブラウザ無しで動く状態の扱い（単体試験はここ）。
//! - `state`: 画面の状態（シグナル）。`actions`: DAP の要求の並び。`view`: 部品と配置。

mod actions;
mod api;
mod i18n;
mod model;
mod state;
mod text;
mod view;

use leptos::prelude::*;
use leptos::task::spawn_local;
use state::Store;

fn main() {
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let store = Store::new();
    provide_context(store);
    api::subscribe(move |message| actions::on_message(store, message));
    // 起動の引数で渡されたもの（`--ui <program>`、`--attach`）があれば始める。無く、何も動いていなければ開く画面を出す。
    spawn_local(async move {
        match api::get("/startup").await {
            Ok(body) if !body["live"].as_bool().unwrap_or(false) => {
                let startup = &body["startup"];
                if startup.get("program").is_some() || startup.get("attach").is_some() {
                    let what = match startup["program"].as_str() {
                        Some(program) if program.to_lowercase().ends_with(".dmp") => serde_json::json!({ "dump": program }),
                        _ => startup.clone(),
                    };
                    if let Some(program) = startup["program"].as_str() {
                        store.folder.set(model::paths::parent(program).to_string());
                    }
                    actions::start(store, what);
                } else {
                    store.dialog.set(true);
                }
            }
            Ok(_) => {}
            Err(message) => store.fail(message),
        }
    });
    view! { <view::Layout/> }
}
