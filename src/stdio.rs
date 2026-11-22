//! 標準入出力の付け替え。
//!
//! エンジンの標準入出力は DAP の通り道で、デバッグ対象は起動時に親の標準ハンドルを引き継ぐ。そのままだと
//! 対象の `Console.WriteLine` が DAP の流れに混ざって壊れる。そこで起動直後に、
//!   1. もとの標準入力・標準出力を、引き継がれない複製として自分だけが持つ（DAP 用）
//!   2. プロセスの標準出力・標準エラーをパイプに、標準入力を NUL に付け替える（対象が引き継ぐ）
//!
//! とし、パイプの読み側を `output` イベントにする。

use crate::win32::*;
use std::fs::File;
use std::io;
use std::os::windows::io::FromRawHandle;
use std::ptr::null_mut;

pub struct Redirected {
    /// DAP の入力（もとの標準入力）。
    pub dap_in: File,
    /// DAP の出力（もとの標準出力）。
    pub dap_out: File,
    /// デバッグ対象の標準出力の読み側。
    pub child_stdout: File,
    /// デバッグ対象の標準エラーの読み側。
    pub child_stderr: File,
}

fn last_error(what: &str) -> io::Error {
    let code = unsafe { GetLastError() };
    io::Error::other(tr!("{what} failed (Win32 error {code})", what = what, code = code))
}

/// 引き継がれない複製を作り、もとのハンドルを閉じる。
unsafe fn take_private(which: u32, what: &str) -> io::Result<File> {
    let original = GetStdHandle(which);
    if original.is_null() || original == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(tr!("There is no {what}. Start this from a DAP client.", what = what)));
    }
    let mut private = null_mut();
    let me = GetCurrentProcess();
    if DuplicateHandle(me, original, me, &mut private, 0, 0, DUPLICATE_SAME_ACCESS) == 0 {
        return Err(last_error(&tr!("Duplicating {what}", what = what)));
    }
    CloseHandle(original);
    Ok(File::from_raw_handle(private))
}

/// 対象に引き継がせる書き側と、自分だけが持つ読み側のパイプを作り、書き側を標準ハンドルにする。
unsafe fn pipe_for_children(which: u32, what: &str) -> io::Result<File> {
    let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: null_mut(), bInheritHandle: 1 };
    let (mut read, mut write) = (null_mut(), null_mut());
    if CreatePipe(&mut read, &mut write, &attributes, 0) == 0 {
        return Err(last_error(&tr!("Creating the pipe for {what}", what = what)));
    }
    // 読み側まで引き継がれると、対象が生きている限りパイプが閉じない。
    SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0);
    if SetStdHandle(which, write) == 0 {
        return Err(last_error(&tr!("Redirecting {what}", what = what)));
    }
    Ok(File::from_raw_handle(read))
}

/// プロセスの標準入出力を付け替える。`main` の最初、ほかのスレッドを作る前に一度だけ呼ぶ。
pub fn redirect() -> io::Result<Redirected> {
    unsafe {
        let dap_in = take_private(STD_INPUT_HANDLE, tr_str!("standard input"))?;
        let dap_out = take_private(STD_OUTPUT_HANDLE, tr_str!("standard output"))?;
        let child_stdout = pipe_for_children(STD_OUTPUT_HANDLE, tr_str!("standard output"))?;
        let child_stderr = pipe_for_children(STD_ERROR_HANDLE, tr_str!("standard error"))?;

        // 対象の標準入力は NUL。読もうとするとすぐ EOF になる。
        const GENERIC_READ: u32 = 0x8000_0000;
        const FILE_SHARE_READ_WRITE: u32 = 3;
        const OPEN_EXISTING: u32 = 3;
        let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: null_mut(), bInheritHandle: 1 };
        let nul = CreateFileW(wide("NUL").as_ptr(), GENERIC_READ, FILE_SHARE_READ_WRITE, &attributes, OPEN_EXISTING, 0, null_mut());
        if nul == INVALID_HANDLE_VALUE {
            return Err(last_error(tr_str!("Opening NUL")));
        }
        SetStdHandle(STD_INPUT_HANDLE, nul);
        Ok(Redirected { dap_in, dap_out, child_stdout, child_stderr })
    }
}

/// 対象の出力（標準出力か標準エラーの一つ）のバイト列を文字列にする。
///
/// 対象が書く文字コードは決まっていない。.NET / .NET Framework の `Console` は、コンソールの出力のコードページ
/// （対象はエンジンのコンソールを引き継ぐ）で書き、コンソールが無ければ .NET は UTF-8、.NET Framework は ANSI の
/// コードページで書く。アプリが `Console.OutputEncoding = UTF8` にしていることもある。そこで、UTF-8 として正しい
/// うちは UTF-8 とみなし、正しくないバイトが来たら、以後その流れはコードページ（`code_page`）で読む。
pub struct Decoder {
    pending: Vec<u8>,
    code_page: u32,
    /// UTF-8 ではないと分かった（以後はコードページで読む）。
    legacy: bool,
}

impl Decoder {
    /// 対象が書くコードページ: エンジンのコンソールの出力のコードページ。コンソールが無ければ ANSI のコードページ。
    pub fn for_target() -> Decoder {
        let code_page = match unsafe { GetConsoleOutputCP() } {
            0 => unsafe { GetACP() },
            code_page => code_page,
        };
        Decoder::with_code_page(code_page)
    }

    pub fn with_code_page(code_page: u32) -> Decoder {
        Decoder { pending: Vec::new(), code_page, legacy: false }
    }

    /// 読んだバイト列を足し、文字にできる分を返す。末尾が文字の途中なら、その分を残して次に回す。
    pub fn decode(&mut self, chunk: &[u8]) -> String {
        const UTF8: u32 = 65001;
        self.pending.extend_from_slice(chunk);
        if !self.legacy {
            match std::str::from_utf8(&self.pending) {
                Ok(_) => return String::from_utf8(std::mem::take(&mut self.pending)).unwrap_or_default(),
                // 途中で切れた文字（error_len が None）は、次の読み取りを待つ。
                Err(e) if e.error_len().is_none() => {
                    let text = String::from_utf8_lossy(&self.pending[..e.valid_up_to()]).into_owned();
                    self.pending.drain(..e.valid_up_to());
                    return text;
                }
                // UTF-8 ではない。コードページが UTF-8 なら壊れたバイトとして置き換え、そうでなければコードページで読む。
                Err(_) if self.code_page == UTF8 => return String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned(),
                Err(_) => self.legacy = true,
            }
        }
        // 2 バイトの文字（Shift_JIS など）の 1 バイト目で終わっていれば、それは次に回す。
        let mut end = 0;
        while end < self.pending.len() {
            let lead = unsafe { IsDBCSLeadByteEx(self.code_page, self.pending[end]) } != 0;
            if lead && end + 1 == self.pending.len() {
                break;
            }
            end += if lead { 2 } else { 1 };
        }
        let text = multi_byte_to_string(self.code_page, &self.pending[..end]);
        self.pending.drain(..end);
        text
    }
}

/// コードページのバイト列を文字列にする（読めない文字は、Windows が置き換える）。
fn multi_byte_to_string(code_page: u32, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let length = unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), bytes.len() as i32, null_mut(), 0) };
    if length <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; length as usize];
    unsafe { MultiByteToWideChar(code_page, 0, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), length) };
    String::from_utf16_lossy(&wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT_JIS: u32 = 932;
    const UTF8: u32 = 65001;

    #[test]
    fn a_character_split_across_reads_is_joined() {
        let bytes = "日本".as_bytes();
        let mut decoder = Decoder::with_code_page(SHIFT_JIS);
        assert_eq!(decoder.decode(&bytes[..4]), "日");
        assert_eq!(decoder.decode(&bytes[4..]), "本");
        assert!(decoder.pending.is_empty());
    }

    #[test]
    fn invalid_bytes_are_replaced_instead_of_stalling_when_the_code_page_is_utf8() {
        let mut decoder = Decoder::with_code_page(UTF8);
        assert_eq!(decoder.decode(&[b'a', 0xFF, b'b']), "a\u{fffd}b");
        assert!(decoder.pending.is_empty());
    }

    #[test]
    fn bytes_that_are_not_utf8_are_read_in_the_code_page() {
        // 「日本 é」の Shift_JIS（é は無いので Windows の置き換え。ここでは日本だけ確かめる）。
        let mut decoder = Decoder::with_code_page(SHIFT_JIS);
        assert_eq!(decoder.decode(&[0x93, 0xFA, 0x96, 0x7B, b'!']), "日本!");
        // 一度コードページで読んだ流れは、そのまま（ASCII だけの読み取りも同じ）。
        assert_eq!(decoder.decode(b"ok"), "ok");
        assert!(decoder.legacy);
    }

    #[test]
    fn a_double_byte_character_split_across_reads_is_joined() {
        let mut decoder = Decoder::with_code_page(SHIFT_JIS);
        assert_eq!(decoder.decode(&[0x93, 0xFA, 0x96]), "日");
        assert_eq!(decoder.decode(&[0x7B]), "本");
        assert!(decoder.pending.is_empty());
    }
}
