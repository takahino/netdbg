//! netdbg: .NET / .NET Framework のデバッグエンジン。標準入出力で DAP を話す。
//!
//! 計画は docs/plan.html、現在地は docs/STATUS.md。

// 表示の言語（tr! / tr_str!）。ほかのモジュールより先に置き、どこでも tr! と書けるようにする。
#[macro_use]
mod i18n;
mod base64;
mod com;
mod cordebug;
#[cfg(test)]
mod cordebug_layout_test;
mod debugger;
mod deflate;
mod events;
mod folder;
mod gui;
mod inflate;
mod interface;
mod log;
mod metahost;
mod oleacc;
mod report;
mod stdio;
mod uiautomation;
mod win32;

use debugger::engine::Flow;
use interface::dap::DapClient;
use interface::hub::Hub;
use interface::mcp::{Mode, Server};
use interface::ui::sse::{Broadcaster, SseSink};
use events::Input;
use serde_json::json;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// 使い方の文（いまの言語）。
fn usage() -> String {
    tr!("netdbg [--interpreter=vscode | --mcp | --mcp-listen | --mcp-connect[=<PID>]] [--log=<file>]\nnetdbg [--log=<file>] --ui [--no-open] [--attach <PID> | <program> [<arguments>...]]\n\n  --interpreter=vscode  Speak DAP over standard input and output (the default; accepted for compatibility with netcoredbg)\n  --mcp                 Speak MCP over standard input and output (an AI client starts the engine directly and uses it)\n  --mcp-listen          Speak DAP, and also open MCP on 127.0.0.1 (AI works with the VS Code session too)\n  --mcp-connect[=<PID>] Bridge to an engine started with --mcp-listen (passes MCP on standard input and output to HTTP)\n  --ui                  Open the debugger's own screen in the browser (debug without VS Code; MCP is opened on the same port).\n                        With <program>, it is launched once the screen opens. --attach <PID> attaches to that process.\n                        --no-open does not open the browser (the URL is on standard output and in ui of the MCP endpoint file)\n  --log=<file>          Write the DAP traffic and internal events to a file\n  --decompile <dll> <dir>  Decompile an assembly without a PDB into a folder (for tests; no debugging)\n  --diff <old> <new> [--coverage <xml>]\n                        Print the methods that changed between the two exes (dlls, folders) as Markdown. With a Cobertura XML,\n                        also print the methods that changed but were not run (the retest scope) (no debugging)\n  --version             Print the version and bitness, then exit")
}

/// `disconnect` などで対象を止めたあと、終了の通知を待つ時間。
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// `--ui` の引数。
struct UiStart {
    /// 画面が開いたら起動するもの（`{ program, args }`）か、つなぐプロセス（`{ attach }`）。どちらも無ければ null。
    startup: serde_json::Value,
    /// 既定のブラウザで開く（`--no-open` で開かない。試験用）。
    open: bool,
}

impl UiStart {
    fn parse(args: &[String]) -> Result<UiStart, String> {
        let mut open = true;
        let mut rest = args;
        loop {
            match rest.first().map(String::as_str) {
                Some("--no-open") => {
                    open = false;
                    rest = &rest[1..];
                }
                Some("--attach") => {
                    let pid = rest.get(1).and_then(|p| p.parse::<u32>().ok()).ok_or_else(|| tr!("Write the process number (PID) after --attach."))?;
                    if rest.len() > 2 {
                        return Err(tr!("Write nothing after --attach <PID>."));
                    }
                    return Ok(UiStart { startup: json!({ "attach": pid }), open });
                }
                Some(program) => return Ok(UiStart { startup: json!({ "program": program, "args": rest[1..] }), open }),
                None => return Ok(UiStart { startup: serde_json::Value::Null, open }),
            }
        }
    }
}

impl UiStart {
    /// 対象のビット数がこのエンジンと違えば、隣のビット数のエンジンに同じ引数で任せ、その終了コードを返す。
    fn run_on_sibling(&self) -> Option<ExitCode> {
        let program = self.startup["program"].as_str();
        let attach = self.startup["attach"].as_u64().and_then(|p| u32::try_from(p).ok());
        let bits = interface::ui::relaunch::target_bits(program, attach)?;
        if bits == debugger::target::Bits::of_this_process() {
            return None;
        }
        let engine = match interface::ui::relaunch::sibling_engine(bits) {
            Ok(engine) => engine,
            Err(e) => {
                // 隣が無ければ、このエンジンで開く（起動のときに、合わない理由を画面に出す）。
                log!("{e}");
                return None;
            }
        };
        log!("the target is {}; handing over to {}", bits.name(), engine.display());
        match std::process::Command::new(&engine).args(std::env::args_os().skip(1)).status() {
            Ok(status) => Some(ExitCode::from(status.code().unwrap_or(1) as u8)),
            Err(e) => {
                eprintln!("{}", tr!("Cannot start {program}: {error}", program = engine.display(), error = e));
                Some(ExitCode::FAILURE)
            }
        }
    }
}

fn main() -> ExitCode {
    let mut mode = Mode::Off;
    let mut ui: Option<UiStart> = None;
    // 試験と調べもののための口: PDB の無いアセンブリを逆コンパイルして、フォルダーに書く（デバッグはしない）。
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--decompile") {
        let [_, assembly, output] = args.as_slice() else {
            eprintln!("{}", tr!("Usage: netdbg --decompile <assembly> <output folder>"));
            return ExitCode::from(2);
        };
        return match debugger::decompile::to_folder(Path::new(assembly), Path::new(output)) {
            Ok(summary) => {
                println!("{summary}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("netdbg --decompile: {e}");
                ExitCode::FAILURE
            }
        };
    }
    // 差し替わった exe の差分（B2、D-080）。デバッグはしない。
    if args.first().map(String::as_str) == Some("--diff") {
        return diff_command(&args[1..]);
    }
    for (index, arg) in args.iter().enumerate() {
        if arg == "--ui" {
            // `--ui` のあとは、画面の引数と、起動するプログラムとその引数。
            match UiStart::parse(&args[index + 1..]) {
                Ok(start) => ui = Some(start),
                Err(e) => {
                    eprintln!("{e}\n\n{}", usage());
                    return ExitCode::from(2);
                }
            }
            break;
        } else if arg == "--interpreter=vscode" {
            // DAP が既定なので、指定があっても無くても同じ。
        } else if arg == "--mcp" {
            mode = Mode::Standalone;
        } else if arg == "--mcp-listen" {
            mode = Mode::Listening;
        } else if arg == "--mcp-connect" || arg.starts_with("--mcp-connect=") {
            // 橋渡しはデバッグをしないので、標準入出力を付け替えずに、そのまま動かす。
            let engine = match arg.strip_prefix("--mcp-connect=").map(str::parse::<u32>) {
                None => None,
                Some(Ok(pid)) => Some(pid),
                Some(Err(_)) => {
                    eprintln!("{}", tr!("Write the engine's PID after --mcp-connect=: {arg}", arg = arg));
                    return ExitCode::from(2);
                }
            };
            return match interface::mcp::connect::run(engine) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("netdbg --mcp-connect: {e}");
                    ExitCode::FAILURE
                }
            };
        } else if let Some(path) = arg.strip_prefix("--log=") {
            if let Err(e) = log::open(path) {
                eprintln!("{}", tr!("Cannot open the log file: {path}: {error}. Check that the folder exists and is writable.", path = path, error = e));
                return ExitCode::from(2);
            }
        } else if arg == "--version" {
            println!("netdbg {} ({} bit)", env!("CARGO_PKG_VERSION"), usize::BITS);
            return ExitCode::SUCCESS;
        } else if arg == "--help" || arg == "-h" {
            println!("{}", usage());
            return ExitCode::SUCCESS;
        } else {
            eprintln!("{}\n\n{}", tr!("Unknown argument: {arg}", arg = arg), usage());
            return ExitCode::from(2);
        }
    }
    log!("netdbg {} ({} bit) started", env!("CARGO_PKG_VERSION"), usize::BITS);
    if let Some(code) = ui.as_ref().and_then(UiStart::run_on_sibling) {
        return code;
    }

    // ここから先、プロセスの標準出力はデバッグ対象のためのパイプになる。println! は使わない。
    let redirected = match stdio::redirect() {
        Ok(r) => r,
        Err(e) => {
            log!("fatal: {e}");
            eprintln!("netdbg: {e}");
            return ExitCode::FAILURE;
        }
    };
    let served = match ui {
        Some(ui) => serve_ui(redirected, ui),
        None => serve(redirected, mode),
    };
    let code = match served {
        Ok(()) => 0,
        Err(e) => {
            log!("fatal: {e}");
            1
        }
    };
    log!("exit {code}");
    // パイプを読むスレッドは読み取りで止まったままなので、プロセスごと終わらせる。
    std::process::exit(code);
}

/// `--diff <old> <new> [--coverage <xml>]`: 変わったメソッド（と再試験の範囲）を Markdown で標準出力に書く。
fn diff_command(args: &[String]) -> ExitCode {
    let usage = || {
        eprintln!("{}", tr!("Usage: netdbg --diff <previous exe, dll or folder> <later exe, dll or folder> [--coverage <Cobertura XML>]"));
        ExitCode::from(2)
    };
    let (old, new, coverage) = match args {
        [old, new] => (old, new, None),
        [old, new, flag, xml] if flag == "--coverage" => (old, new, Some(xml)),
        _ => return usage(),
    };
    let compared = (|| -> Result<String, String> {
        let mut changes = debugger::diff::compare(&debugger::diff::read(Path::new(old))?, &debugger::diff::read(Path::new(new))?);
        if let Some(xml) = coverage {
            let text = std::fs::read_to_string(xml).map_err(|e| tr!("Cannot read {path}: {error}", path = xml, error = e))?;
            debugger::diff::overlay(&mut changes, &debugger::diff::Executed::Cobertura(debugger::diff::read_cobertura(&text)));
        }
        Ok(debugger::diff::to_markdown(old, new, &changes))
    })();
    match compared {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("netdbg --diff: {e}");
            ExitCode::FAILURE
        }
    }
}

/// デバッグ対象の標準出力（または標準エラー）を読むスレッド。
fn read_target_output(mut pipe: File, category: &'static str) {
    let mut buffer = [0u8; 4096];
    let mut decoder = stdio::Decoder::for_target();
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                let text = decoder.decode(&buffer[..count]);
                if !text.is_empty() {
                    events::post(Input::Output { category, text });
                }
            }
        }
    }
}

/// 入力の読み手を起こし、入力を一つずつ処理する。入力が閉じるか `disconnect` で終わる。
fn serve(redirected: stdio::Redirected, mode: Mode) -> io::Result<()> {
    let stdio::Redirected { dap_in, dap_out, child_stdout, child_stderr } = redirected;
    std::thread::spawn(move || read_target_output(child_stdout, "stdout"));
    std::thread::spawn(move || read_target_output(child_stderr, "stderr"));
    if mode == Mode::Standalone {
        // 標準入出力は MCP（1 行に 1 つの JSON-RPC）。DAP のクライアントはいない。
        let (reply, replies) = std::sync::mpsc::channel();
        std::thread::spawn(move || interface::mcp::stdio::read_lines(BufReader::new(dap_in), reply));
        std::thread::spawn(move || interface::mcp::stdio::write_lines(dap_out, replies));
        return run(Hub::<io::Sink>::new(None, Some(Server::new(mode))));
    }
    std::thread::spawn(move || interface::dap::read_requests(dap_in));
    // `--mcp-listen`: MCP の口を開く。終わるときに、接続先のファイルを消す（`Endpoint` を落とす）。
    let endpoint = if mode == Mode::Listening { Some(interface::mcp::http::listen()?) } else { None };
    let mut hub = Hub::new(Some(DapClient::new(dap_out)), endpoint.is_some().then(|| Server::new(mode)));
    if let Some(endpoint) = &endpoint {
        hub.announce(&endpoint.url, &endpoint.token);
    }
    run(hub)
}

/// `--ui`: 専用の画面の口（と、同じ口の MCP）を開き、ブラウザで開く。標準入力は読まない。
fn serve_ui(redirected: stdio::Redirected, start: UiStart) -> io::Result<()> {
    let stdio::Redirected { dap_out: mut console, child_stdout, child_stderr, .. } = redirected;
    std::thread::spawn(move || read_target_output(child_stdout, "stdout"));
    std::thread::spawn(move || read_target_output(child_stderr, "stderr"));
    let broadcaster = Broadcaster::new();
    // 終わるときに、MCP の接続先のファイルを消す（`started.mcp` を落とす）。
    let started = interface::ui::start(broadcaster.clone(), start.startup)?;
    let mut hub = Hub::new(Some(DapClient::new(SseSink::new(broadcaster))), Some(Server::new(Mode::Listening)));
    hub.announce(&started.mcp.url, &started.mcp.token);
    hub.keep_alive();
    // もとの標準出力（コンソール）に URL を出す。ブラウザが開けなかったときや、試験の道具が読む。
    let _ = writeln!(console, "netdbg UI: {}", started.url);
    if start.open {
        if let Err(e) = interface::ui::open_browser(&started.url) {
            log!("{e}");
            let _ = writeln!(console, "{e}");
        }
    }
    run(hub)
}

fn run<W: io::Write>(mut hub: Hub<W>) -> io::Result<()> {
    // `disconnect` で止めた対象の終了を待つ期限。
    let mut exit_deadline: Option<Instant> = None;
    loop {
        // MCP の呼び出しが止まるのを待っていれば、その期限でも起きる。
        let deadline = [exit_deadline, hub.deadline()].into_iter().flatten().min();
        let Some(input) = events::next(deadline) else {
            if exit_deadline.is_some_and(|d| d <= Instant::now()) {
                log!("the debuggee did not exit in time");
                hub.finish_pending_disconnect()?;
                if hub.restart_if_kept() {
                    exit_deadline = None;
                    continue;
                }
                break;
            }
            hub.expire()?;
            continue;
        };
        match hub.handle(input)? {
            Flow::Continue if !hub.waits_for_exit() => exit_deadline = None,
            Flow::Continue => {}
            Flow::ExitAfterProcess => exit_deadline = Some(Instant::now() + PROCESS_EXIT_TIMEOUT),
            Flow::Exit => break,
        }
    }
    log!("session ended");
    Ok(())
}
