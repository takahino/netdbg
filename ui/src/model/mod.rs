//! 画面の状態の、ブラウザ無しで動く部分（DAP の答えと出来事から状態を作る、色付け、パス）。単体試験はここで、
//! ホストで回す（`cargo test --manifest-path ui/Cargo.toml --target x86_64-pc-windows-msvc`）。

pub mod breakpoints;
pub mod coverage;
pub mod csharp;
pub mod diff;
pub mod exceptions;
pub mod inline;
pub mod paths;
pub mod records;
pub mod report;
pub mod session;
pub mod sources;
pub mod stack;
pub mod variables;
pub mod windows;
