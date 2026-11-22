//! ビット数の違う対象を、隣のビット数のエンジンで開き直す。
//!
//! ICorDebug は、エンジンと対象のビット数が同じでないと動かない（target.rs）。拡張はプログラムのビット数でエンジンを
//! 選ぶが、`--ui` は利用者がエンジンを直接起動するので、エンジンが選び直す: 配布の形（`<dir>\x86\netdbg.exe`
//! と `<dir>\x64\netdbg.exe`）の、隣のフォルダーのエンジンを、同じ引数で起動する。

use crate::debugger::target::{self, Bits};
use std::path::{Path, PathBuf};

/// 対象（`--ui` の引数のプログラム、またはアタッチするプロセス）のビット数。調べられなければ `None`（このエンジンで
/// 開き、理由は起動のときに出る）。
pub fn target_bits(program: Option<&str>, attach: Option<u32>) -> Option<Bits> {
    if let Some(process_id) = attach {
        return target::bits_of_process(process_id).ok();
    }
    let data = std::fs::read(program?).ok()?;
    target::inspect(&data).map(|e| e.bits)
}

/// 隣のビット数のエンジン。
pub fn sibling_engine(bits: Bits) -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| tr!("Cannot tell where the engine is: {error}", error = e))?;
    let sibling = me.parent().and_then(Path::parent).map(|dir| dir.join(bits.name()).join("netdbg.exe"));
    match sibling {
        Some(path) if path.is_file() => Ok(path),
        _ => Err(tr!("The {bits} engine is not found (put netdbg.exe in the folder {bits} next to {engine}).", bits = bits.name(), engine = me.display())),
    }
}

/// 画面の中から: 隣のビット数のエンジンを `--ui` で起動する（待たない。新しいエンジンが、自分のブラウザのタブを
/// 開く）。このエンジンの標準入出力は対象のためのパイプなので、引き継がせない。
pub fn start(bits: Bits, arguments: &[String]) -> Result<(), String> {
    use std::process::Stdio;
    let engine = sibling_engine(bits)?;
    std::process::Command::new(&engine)
        .arg("--ui")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| tr!("Cannot start {program}: {error}", program = engine.display(), error = e))
}
