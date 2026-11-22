//! 画面のファイル（HTML、CSS、wasm、wasm-bindgen が作るつなぎの JS）。ビルドのときに exe に埋め込む。
//!
//! 表は `build.rs` が作る。`ui/build.ps1` で画面をビルドしてあれば、その出力（`target/ui/dist`）を埋め込む。無ければ、
//! ビルドのしかたを書いたページだけを埋め込む（`cargo test` は、wasm の道具が無くても通る）。

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

/// 道に当たるファイルの、種類と中身。
pub fn find(path: &str) -> Option<(&'static str, &'static [u8])> {
    ASSETS.iter().find(|(p, _, _)| *p == path).map(|(_, kind, bytes)| (*kind, *bytes))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_page_is_always_there() {
        let (kind, bytes) = super::find("/index.html").expect("index.html");
        assert!(kind.starts_with("text/html"));
        assert!(!bytes.is_empty());
        assert!(super::find("/../Cargo.toml").is_none());
    }
}
