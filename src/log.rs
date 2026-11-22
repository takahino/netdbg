//! `--log=<ファイル>` のログ。行ごとにフラッシュするので、エンジンが固まったり落ちたりしても残る。

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

static LOG: Mutex<Option<(File, Instant)>> = Mutex::new(None);

/// ログの出力先を開く。失敗したら理由を返す（呼び出し側が標準エラーに出す）。
pub fn open(path: &str) -> std::io::Result<()> {
    let file = File::create(path)?;
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some((file, Instant::now()));
    Ok(())
}

pub fn enabled() -> bool {
    LOG.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 1 行書く。ログが開かれていなければ何もしない。
pub fn line(text: &str) {
    let mut guard = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((file, started)) = guard.as_mut() {
        // ログの書き込み失敗でデバッグを止めない。
        let _ = writeln!(file, "{:>10.3} {}", started.elapsed().as_secs_f64(), text);
        let _ = file.flush();
    }
}

/// `log!("...{}", x)`: ログが有効なときだけ文字列を組み立てる。
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        if $crate::log::enabled() {
            $crate::log::line(&format!($($arg)*));
        }
    };
}
