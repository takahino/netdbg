//! メイン層: デバッガーの本体。対象の起動とアタッチ（ICorDebug）、ブレークポイント、ステップ、スタック、値、式の評価。
//!
//! どの IF（`crate::interface` の DAP、MCP）から使われるかを知らない。IF の層は、本体の要求の口（`engine::Engine`）に
//! 要求を渡し、答えと、本体が返す出来事（`session::Event`）を自分の相手に伝える。

pub mod anonymous;
pub mod builtin;
pub mod callback;
pub mod coreclr;
pub mod coverage;
pub mod decompile;
pub mod diff;
pub mod dump;
pub mod engine;
pub mod environment;
pub mod framework;
pub mod il;
pub mod journal;
pub mod libraries;
pub mod metadata;
pub mod minidump;
pub mod pdb;
pub mod session;
pub mod sqllog;
pub mod symbols;
pub mod target;
pub mod usage;
pub mod value;
