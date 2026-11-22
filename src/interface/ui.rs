//! 専用の画面（ブラウザ。段階 13、D-062）: `netdbg --ui` で、VS Code 無しにデバッグする。
//!
//! 画面は Rust（Leptos）を WebAssembly にしたもの（`ui/`）で、エンジンの exe に埋め込んで 127.0.0.1 で出す。画面は
//! DAP を話す: 要求は `POST /dap`、出来事は `GET /events`（Server-Sent Events）。hub から見ると、画面は VS Code と
//! 同じ DAP のクライアント（`DapClient<SseSink>`）で、AI（MCP）の操作で起きた出来事も、そのまま画面に届く。MCP は
//! 同じ口の `/mcp` で開くので、AI は画面で開いたセッションにそのままつなげる。

mod assets;
mod files;
mod processes;
pub mod relaunch;
mod server;
pub mod sse;
mod windows;

pub use server::start;

/// 既定のブラウザで URL を開く。
pub fn open_browser(url: &str) -> Result<(), String> {
    use crate::win32::{wide, ShellExecuteW};
    let (verb, target) = (wide("open"), wide(url));
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    // 32 以下はエラー（ShellExecute の決まり）。
    if result as usize <= 32 {
        return Err(tr!("Could not open the browser (ShellExecute {code}). Open this URL yourself: {url}", code = result as usize, url = url));
    }
    Ok(())
}
