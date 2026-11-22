//! デバッグセッションの状態と、対象プロセスへの操作。
//!
//! ここの関数はすべてセッションのスレッド（`main` のループ）から呼ばれる。コールバックのスレッドは
//! `events::post` で出来事を送ってくるだけで、対象の検査と続行はここで行う。

use crate::debugger::callback::managed_callback;
use crate::com::{ComPtr, IUnknown, Interface, HRESULT};
use crate::cordebug::*;
use crate::debugger::coreclr::{Dbgshim, Launched};
use crate::events::{DebugEvent, EventKind};
use crate::debugger::framework;
use crate::debugger::metadata::Metadata;
use crate::debugger::pdb::Pdb;
use crate::debugger::symbols;
use crate::debugger::target;
use crate::debugger::value::{self, Member};
use crate::win32::{self, from_wide, HANDLE};
use serde_json::{json, Value};
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

/// 切り離す前に、止まっていたスレッドが再開を終えるのを待つ時間（Session::detach）。
const DETACH_SETTLE_TIME: std::time::Duration = std::time::Duration::from_millis(100);

mod breakpoints;
mod decompiled;
mod enc;
mod eval;
mod exceptions;
mod funceval;
mod hits;
mod inspect;
mod launch;
mod loadfail;
mod operators;
mod probes;
mod stack;
mod stepping;
mod targets;
mod watches;
mod monitor;
mod native;
mod sqlbreak;
mod bindings;
mod retention;
mod dialogs;
mod transactions;
mod fault;
mod async_stack;
mod children;
mod databreak;
mod dumps;
mod observe;
mod variables;
mod coverage;
mod diff;

use breakpoints::{bind, bind_function, break_at, describe, describe_function};
use decompiled::{Decompile, DecompileState};
pub use exceptions::ExceptionFilters;

/// クライアントへ送るイベント（名前と本文）。
pub type Event = (&'static str, Value);

#[derive(Debug, PartialEq)]
enum State {
    /// `launch` は受けたが、まだ対象を起動していない。
    NotStarted,
    Running,
    /// 対象が止まっていて、クライアントの `continue` を待っている。
    Stopped,
    Exited,
}

struct LaunchConfig {
    program: String,
    /// .NET Framework の実行ファイル。dbgshim ではなく `framework.rs` の経路で起動する。
    framework: bool,
    command_line: String,
    /// 対象に足す環境変数。値が `None` なら消す。
    environment: Vec<(String, Option<String>)>,
    cwd: Option<String>,
    /// `stopAtEntry`: 入口のメソッド（`Main`）の先頭で止める。
    stop_at_entry: bool,
}

/// いま止まっている理由になった例外。
struct StoppedException {
    thread: u32,
    /// `exceptionInfo` の `breakMode`（`always`、`unhandled`、`userUnhandled`）。
    break_mode: &'static str,
    /// `stopped` イベントの `text` と同じ文。
    text: String,
}

/// 直前のステップで呼ばれたメソッドの戻り値。止まったフレームの変数の先頭に `Type.Method returned` として見せる。
struct ReturnValue {
    thread: u32,
    /// `Type.Method`。
    name: String,
    /// 拾ったその場で文字列にしておく（値のオブジェクトは、ステップが先へ進むと使えなくなる）。
    value: String,
    type_name: String,
    /// 参照型の値をつかんだままにするハンドル。止まったあとで中身を開ける。
    handle: Option<ComPtr<ICorDebugValue>>,
}

/// 戻り値を拾うための内部のブレークポイント。呼び出しから戻った直後（戻り値がまだ残っている場所）に置く。
struct ReturnValueBreakpoint {
    breakpoint: ComPtr<ICorDebugFunctionBreakpoint>,
    /// 呼び出し命令の IL オフセット。
    call_offset: u32,
    name: String,
    thread: u32,
    /// ステップしているフレームのスタック上の位置。同じメソッドの再帰呼び出しや、ほかのスレッドと区別する。
    frame_start: u64,
}

/// async メソッドのステップで、`await` に置く内部のブレークポイント。
struct AwaitBreakpoint {
    breakpoint: ComPtr<ICorDebugFunctionBreakpoint>,
    /// ステップしているスレッド。中断する側では、このスレッドでの当たりだけを見る（再開は別のスレッドになり得る）。
    thread: u32,
    module: ComPtr<ICorDebugModule>,
    method: u32,
    /// 中断する位置に置いたものなら、再開する位置。再開する位置に置いたものなら `None`。
    resume_offset: Option<u32>,
    /// 再開する位置に置いたものなら、待っている呼び出しの状態機械（MoveNext の this）への弱いハンドル。同じメソッドが
    /// 並行して走っているとき、別の呼び出しの続きで止まらないよう見分ける。状態機械が構造体（最適化つきのビルド）なら `None`。
    state_machine: Option<ComPtr<ICorDebugHandleValue>>,
}

/// ステップの種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepKind {
    /// `next`: 呼び出しの中には入らない。
    Over,
    /// `stepIn`。
    In,
    /// `stepOut`: いまのメソッドから戻るまで。
    Out,
}

struct Module {
    module: ComPtr<ICorDebugModule>,
    path: String,
    pdb: Option<Pdb>,
    /// PDB のファイル（埋め込み PDB なら `None`）。
    pdb_path: Option<std::path::PathBuf>,
    /// Edit and Continue の形にできた（Debug ビルドで、起動した対象）。
    enc: bool,
    /// PDB が無いモジュールの、エンジンの中での逆コンパイルの状態（D-060）。
    decompile: Decompile,
    /// アセンブリのメタデータ。大きなファイル（System.Private.CoreLib など）もあるので、名前が要るときに初めて読む。
    metadata: OnceCell<Option<Metadata>>,
}

impl Module {
    fn metadata(&self) -> Option<&Metadata> {
        self.metadata
            .get_or_init(|| {
                // いまはディスク上のファイルから読む。単一ファイルのアプリや動的に作られたモジュールは読めない（docs/TODO.md）。
                let loaded = std::fs::read(&self.path).map_err(|e| e.to_string()).and_then(|file| Metadata::from_pe(&file));
                loaded.map_err(|e| crate::log!("metadata {}: {e}", self.path)).ok()
            })
            .as_ref()
    }
}

/// 出来事の名前がこれで始まるものは、クライアントへの逆向きの要求（`request:startDebugging` など）。DAP の層が要求として書く。
pub const REVERSE_REQUEST: &str = "request:";

/// ブレークポイントを置いた側。`setBreakpoints` / `setFunctionBreakpoints` は、同じ側の分だけを置き換える
/// （VS Code の下で AI が MCP からブレークポイントを置いても、VS Code の分を消さない。逆も同じ）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    /// DAP のクライアント（VS Code など）。
    Client,
    /// MCP のツール（AI）。
    Agent,
    /// 値の変化で止めるための、書き込みの行（watches.rs）。利用者には見せない（`breakpoint` の通知を出さない）。
    Watch,
}

impl Owner {
    /// このブレークポイントの変化（置けた行など）を、`breakpoint` の通知でクライアントに伝えるか。
    pub(super) fn is_reported(self) -> bool {
        self != Owner::Watch
    }
}

/// 関数ブレークポイント（`setFunctionBreakpoints`）。名前に合うメソッドすべての先頭に置く。
struct FunctionBreakpoint {
    id: i64,
    owner: Owner,
    /// `Namespace.Type.Method`、`Type.Method`、`Method` のどれか。
    name: String,
    /// シンボルの無いモジュールのメソッドにも置く（拡張の独自の指定。ライブラリの呼び出しを捕まえるのに使う）。
    any_code: bool,
    /// 止まる条件（式）。無ければ、当たるたびに止まる。
    condition: Option<String>,
    bound: Vec<ComPtr<ICorDebugFunctionBreakpoint>>,
}

struct Breakpoint {
    id: i64,
    /// クライアントが指定した行。
    line: u32,
    /// 止まる条件（式）。無ければ、当たるたびに止まる。条件の欄の印（`#once` など）は `marker` に入れ、ここには入れない。
    condition: Option<String>,
    /// ヒットカウント（`hitCondition`）。
    hit_condition: Option<hits::HitCondition>,
    /// 条件の欄に書いた印（`#once`、`#after`、`#changed`、`#caller`）。
    marker: Option<hits::Marker>,
    /// ログポイント（`logMessage`）。あれば止まらずに出力する。
    log_message: Option<String>,
    /// 実際に置けた行。まだどのモジュールにも置けていなければ `None`。
    verified_line: Option<u32>,
    bound: Vec<ComPtr<ICorDebugFunctionBreakpoint>>,
}

pub struct Session {
    state: State,
    config: Option<LaunchConfig>,
    /// `attach` でつなぐ相手のプロセス番号。`launch` のときは `None`。
    attach_to: Option<u32>,
    /// アタッチの相手が .NET Framework のプロセス。
    attach_framework: bool,
    /// アタッチする先が、親のエンジンが止めて渡した子プロセス（`__resumeChild`）。つなぐ準備ができたら動かす。
    resume_child: bool,
    dbgshim: Option<Dbgshim>,
    launched: Option<Launched>,
    cordebug: Option<ComPtr<ICorDebug>>,
    process: Option<ComPtr<ICorDebugProcess>>,
    process_handle: HANDLE,
    modules: Vec<Module>,
    /// 置いた側とソースのパス（送ってきたまま）ごとのブレークポイント。
    breakpoints: BTreeMap<(Owner, String), Vec<Breakpoint>>,
    function_breakpoints: Vec<FunctionBreakpoint>,
    next_breakpoint_id: i64,
    /// 止まっている間、`continue` で使う相手。
    stopped_controller: Option<ComPtr<ICorDebugController>>,
    /// 生きているマネージドのスレッド（作られた順）。ランタイムが知らせてきたものだけを数える。
    threads: Vec<u32>,
    /// 利用者が凍結したスレッド（`netdbg.freezeThread`）。評価のあとも、止めたままにする。
    frozen_threads: std::collections::HashSet<u32>,
    /// いま投げられている例外が、利用者のコードを通ったスレッド（`user-unhandled` で止まるかの判定に使う）。
    exception_in_user_code: std::collections::HashSet<u32>,
    /// async メソッドからの stepOut で、待っている側が待ち終えた所（`Task.NotifyDebuggerOfWaitCompletion`）に置いたブレークポイント。
    async_step_out: Option<ComPtr<ICorDebugFunctionBreakpoint>>,
    /// いまの停止が、ブレークポイントかステップで止まったスレッド（デバッガーが置いた命令の上にいる）。切り離すときに、
    /// そのスレッドを先に進めてからにする（D-019）。
    patched_thread: Option<u32>,
    /// 同じ場所の 2 つ目のブレークポイントの知らせが、まだ届いていない（スレッドと場所）。`breakpoint_hit` のプローブと
    /// 利用者のブレークポイントの所で、そのスレッドに知らせが溜まっていれば覚え、次の知らせで使って捨てる。
    queued_breakpoint: Option<(u32, (String, u32, u32))>,
    /// いまのステップは、async メソッドからの stepOut の最後の段（待っていた側へ戻る）。
    landing_after_async_out: bool,
    /// オブジェクト ID（`netdbg.makeObjectId`）。続行しても残る。
    object_ids: Vec<inspect::ObjectId>,
    last_object_id: i64,
    /// 前のヒープの統計の COM オブジェクト（RCW）の数（漏れの増分。D-092）。
    last_com_objects: Option<u64>,
    /// 書き込みの見張り（データ ブレークポイント。D-093）。
    data_watch: Option<databreak::DataWatch>,
    /// 書き出して読み込ませた、匿名型のアセンブリの数（名前を、読み込むたびに変えるため）。
    anonymous_assemblies: u32,
    /// 最初に作られたスレッド。名前が無ければ `Main Thread` と見せる。
    main_thread: Option<u32>,
    /// クライアントに渡したフレームの番号（添字 + 1）と、その「スレッド、深さ」。続行すると無効になる。
    frames: Vec<(u32, usize)>,
    /// クライアントに渡した variablesReference（添字 + 1）の中身。続行すると無効になる。
    handles: Vec<Handle>,
    /// `stopAtEntry` のために入口のメソッドに置いたブレークポイント。一度止まったら外す。
    entry_breakpoint: Option<ComPtr<ICorDebugFunctionBreakpoint>>,
    /// 入口をまだ探している（`stopAtEntry` で、入口のあるモジュールがまだ読み込まれていない）。
    seeking_entry: bool,
    /// 進行中のステップ。終わる（StepComplete）か、ほかの理由で止まったら外す。
    stepper: Option<ComPtr<ICorDebugStepper>>,
    /// 自分のコードだけをステップする（`justMyCode`。既定は true）。シンボルのあるモジュールを「自分のコード」とみなす。
    just_my_code: bool,
    /// ステップインで、プロパティと演算子の中に入らない（`enableStepFiltering`。既定は true）。
    step_filtering: bool,
    /// 利用者が頼んだステップがステップインで、まだ終わっていない。
    stepping_in: bool,
    /// プロパティや演算子から抜けている途中。抜けたら、ステップインをやり直す。
    leaving_filtered: bool,
    /// シンボルを探すフォルダー（拡張が環境変数で渡す）。
    symbol_directories: Vec<std::path::PathBuf>,
    exception_filters: ExceptionFilters,
    /// 例外で止まっている間だけ入っている（`exceptionInfo` が使う）。
    stopped_exception: Option<StoppedException>,
    /// `gotoTargets` で渡した番号（添字 + 1）と、その「ソースのパス、行」。`goto` には番号だけが戻ってくる。
    goto_targets: Vec<(String, u32)>,
    return_values: Vec<ReturnValue>,
    return_value_breakpoints: Vec<ReturnValueBreakpoint>,
    /// `stepOut` しているスレッド。戻った先で、戻り値を読む。
    stepping_out: Option<u32>,
    /// async メソッドのステップ中に、`await` の中断と再開を捕まえるためのブレークポイント。
    await_breakpoints: Vec<AwaitBreakpoint>,
    /// 要求を処理している途中で起きた出来事（関数評価の間のモジュールの読み込みなど）。要求に答えたあとで送る。
    deferred: Vec<Event>,
    /// 式の評価の途中で、`?.` の左辺の値を右側の式に渡すための置き場（入れ子になるので積む）。
    receivers: Vec<ComPtr<ICorDebugValue>>,
    /// 式の評価の途中で、いちばん内側の `checked(…)`（`Some(true)`）か `unchecked(…)`（`Some(false)`）。
    /// checked の中では、整数のあふれを失敗にする。unchecked の中では、定数どうしの計算があふれてもよい。
    checked: Option<bool>,
    /// 式の評価の途中で、宣言の型が `object` の変数・フィールドから読んだ値（`object o = "t"` を文字列として扱わないため）。
    object_typed: Vec<ComPtr<ICorDebugValue>>,
    /// 式の評価の途中で、`dynamic` の式の値（`(dynamic)x` と、その結果）。メンバーと呼び出しを、バインダで解決する。
    dynamic_typed: Vec<ComPtr<ICorDebugValue>>,
    /// 式の評価の途中で、計算したポインター（`p + 1`、`&x`）。値は番地を入れた ulong で、ここにポインターの型を覚える（D-044）。
    pointer_typed: Vec<(ComPtr<ICorDebugValue>, ComPtr<ICorDebugType>)>,
    /// 式の評価の途中で、ポインターの指す先を対象のメモリから読んで作った写しと、もとの番地。
    pointer_copies: Vec<(ComPtr<ICorDebugValue>, u64)>,
    /// 式の評価の途中で、いま `await` の待ちの中にいる深さ。0 でなければ、関数評価の間もほかのスレッドを止めない
    /// （待っている Task を終わらせるのは、ほかのスレッド）。
    awaiting: u32,
    /// 式の評価の途中で、組み立てているラムダ式の return / break / continue の飛び先（入れ子になるので積む）。
    lambda_flows: Vec<eval::LambdaFlow>,
    /// 呼び出しの引数のラムダ式を、組み立てられなかった理由（最初のもの）。呼べるメソッドが見つからなかったときに、
    /// 「メソッドがありません」の代わりに伝える。
    lambda_failure: Option<String>,
    /// 式を一つ評価する間だけの覚え書き。
    eval_cache: EvalCache,
    /// 式の評価の途中で、パターンが作った変数（`x is int k` の `k`）。式を一つ評価し終えたら捨てる。
    pattern_locals: Vec<(String, ComPtr<ICorDebugValue>)>,
    /// `DebuggerDisplay` の式を評価している間の、`this`（表示しているオブジェクト）。名前は、まずこのメンバーから探す。
    display_this: Option<ComPtr<ICorDebugValue>>,
    /// いま処理している DAP の要求の番号（評価の途中で、その要求への `cancel` が届いたかを見るため）。
    pub current_request: Option<i64>,
    /// 暗黙の評価（変数の欄のプロパティ、`DebuggerDisplay`、代わりの型）が時間切れになったので、続行するまで暗黙の評価を
    /// しない（Visual Studio と同じ。画面の更新のたびに待たされないように）。明示の `evaluate` は行う。
    implicit_evaluation_off: bool,
    /// 暗黙の評価をしてよいか（launch.json の `expressionEvaluationOptions.allowImplicitFuncEval`。既定は true）。
    /// false なら、プロパティは開いたときに実行せず、クリックで評価する形（DAP の lazy）で見せる（D-043）。
    implicit_evaluation_allowed: bool,
    /// 観察専用のアタッチ（`attach` の `observeOnly`。D-089）: 値の書き換えと、利用者が頼む関数の評価を断る。
    observe_only: bool,
    /// いま利用者が頼んだ評価（式、変数の木、ブレークポイントの条件とログ）の中か（観察専用で関数の評価を断る範囲）。
    user_evaluation: bool,
    /// いまの要求の処理で、対象の中の値を書いたか、対象のコードを実行したか（変数の表示を取り直させる `invalidated` のため）。
    pub touched_target: bool,
    /// いまの要求の処理で実行した関数評価の回数と、その合計の時間（`--log` に要求ごとに書く。遅さの調べ物のため）。
    pub eval_stats: (u32, std::time::Duration),
    /// Edit and Continue の状態（EnC にしたモジュール、補助のプロセス、乗り換えの対応）。
    enc: enc::EncState,
    /// PDB の無いモジュールの逆コンパイル（起動の引数 `decompile`、対象を決める情報）。
    decompile: DecompileState,
    /// 止めずに読むブレークポイント（SQL ログなど）と、出来事の列（段階 11、D-063）。
    probes: probes::ProbeState,
    /// ブレークポイントに当たったときの判定の覚え（ヒットカウントの数、`#once` / `#after` / `#changed`、区間の時刻）。
    hit_state: hits::HitState,
    /// `stepInTargets` で渡した候補（Step Into Target）。
    step_targets: Vec<targets::StepTarget>,
    /// 進行中の Step Into Target（呼び先に置いた一時的なブレークポイント）。
    target_step: Option<targets::TargetStep>,
    /// 値の監視の書き込みの行（持ち主 `Owner::Watch` のブレークポイントの番号）と、その行が書き込む名前。
    watch_sites: BTreeMap<i64, Vec<String>>,
    /// 進行中の、書き込みの行のステップ オーバー。
    watch_step: Option<watches::WatchStep>,
    /// UI のフリーズの監視の設定と、監視（対象のプロセスがある間）。
    ui_freeze: monitor::FreezeSettings,
    freeze_watch: Option<monitor::FreezeWatch>,
    /// SQL で止める条件と、計っている SQL（段階 12）。
    sql_break: sqlbreak::SqlBreakState,
    /// ダイアログで止める条件（番号と条件。dialogs.rs）。
    dialog_breaks: Vec<(i64, dialogs::DialogCondition)>,
    /// 開いているトランザクション（transactions.rs）。
    transactions: transactions::TransactionState,
    /// この対象の環境の記録の番号（fault.rs）。
    environment_seq: Option<u64>,
    /// 子プロセスの扱いと、`Start` から戻るのを待っている起動（children.rs）。
    children: children::ChildState,
    /// クラッシュダンプ（起動の引数 `dumpPath`。dumps.rs）。
    dump: dumps::DumpState,
    /// 行と分岐のカバレッジ（起動の引数 `coverage`。coverage.rs、D-079）。
    coverage: coverage::CoverageState,
}

/// `variables` で開けるもの。`path` は、開いた値を指す式（中身の `evaluateName` を作る。分からなければ `None`）。
#[derive(Clone)]
enum Handle {
    /// フレームの引数とローカル変数。
    Locals { thread: u32, depth: usize },
    /// 値の中身（フィールド、プロパティ、配列の要素）。`thread` は、プロパティを読むときに対象のコードを実行するスレッド。
    Children { value: ComPtr<ICorDebugValue>, thread: u32, path: Option<String> },
    /// 型の静的メンバー。`thread` は、定数の値を対象の中に作るときに使うスレッド。
    Statics { ty: ComPtr<ICorDebugType>, thread: u32 },
    /// `DebuggerTypeProxy` の付いた値の、もとのメンバー（「Raw View」の中身）。
    Raw { value: ComPtr<ICorDebugValue>, thread: u32, path: Option<String> },
    /// 式で計算したポインターの指す先（開くと、名前の無い値が 1 つ。ポインターの変数を開いたときと同じ）。
    Pointee { value: ComPtr<ICorDebugValue>, thread: u32 },
    /// まだ実行していないプロパティ（暗黙の評価をしない設定のとき）。開くと getter を実行し、値を 1 つ返す。
    Lazy { name: String, getter: value::Callable, this: Option<ComPtr<ICorDebugValue>>, thread: u32 },
}

impl Handle {
    /// 中の値をさらに開くとき、プロパティを読むのに使うスレッド。
    fn thread(&self) -> u32 {
        match self {
            Handle::Locals { thread, .. } | Handle::Children { thread, .. } | Handle::Raw { thread, .. } => *thread,
            Handle::Statics { thread, .. } | Handle::Lazy { thread, .. } | Handle::Pointee { thread, .. } => *thread,
        }
    }
}

impl value::Modules for Vec<Module> {
    fn metadata(&self, module: &ComPtr<ICorDebugModule>) -> Option<&Metadata> {
        let path = module_path(module);
        self.iter().find(|m| m.path == path)?.metadata()
    }
}

fn failed(what: &str, hr: HRESULT) -> String {
    tr!("{what} failed ({code})", what = what, code = hr)
}

/// 出力引数でインターフェイスを受け取る呼び出しを包む。
unsafe fn take<I: Interface>(what: &str, call: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<ComPtr<I>, String> {
    let mut raw = null_mut();
    call(&mut raw).ok().map_err(|hr| failed(what, hr))?;
    ComPtr::from_raw(raw).ok_or_else(|| tr!("{what} returned null", what = what))
}

/// フレームが実行しているメソッドの版（Edit and Continue で差分を当てるたびに増える。当てていなければ 1）。PDB の
/// どの版のシーケンスポイントとローカル変数を使うかを決める。分からなければ、いちばん新しい版。
pub(super) fn frame_version(frame: &ComPtr<ICorDebugFrame>) -> u32 {
    unsafe {
        let Ok(function) = take::<ICorDebugFunction>("ICorDebugFrame::GetFunction", |out| frame.GetFunction(out)) else { return crate::debugger::pdb::LATEST };
        function_version(&function)
    }
}

/// メソッドの版（`ICorDebugFunction2::GetVersionNumber`）。
pub(super) fn function_version(function: &ComPtr<ICorDebugFunction>) -> u32 {
    let mut version = 0u32;
    match function.cast::<ICorDebugFunction2>() {
        Ok(function2) if unsafe { function2.GetVersionNumber(&mut version) }.is_ok() => version,
        _ => crate::debugger::pdb::LATEST,
    }
}

fn module_path(module: &ComPtr<ICorDebugModule>) -> String {
    let mut buffer = [0u16; 1024];
    let mut length = 0u32;
    unsafe { module.GetName(buffer.len() as u32, &mut length, buffer.as_mut_ptr()) };
    from_wide(&buffer)
}

/// フレームのメソッドの IL のバイト列。
unsafe fn method_il(frame: &ComPtr<ICorDebugFrame>) -> Option<Vec<u8>> {
    let mut function = null_mut();
    frame.GetFunction(&mut function);
    let function = ComPtr::<ICorDebugFunction>::from_raw(function)?;
    let mut code = null_mut();
    function.GetILCode(&mut code);
    let code = ComPtr::<ICorDebugCode>::from_raw(code)?;
    let mut size = 0u32;
    code.GetSize(&mut size).ok().ok()?;
    let mut il = vec![0u8; size as usize];
    let mut fetched = 0u32;
    code.GetCode(0, size, size, il.as_mut_ptr(), &mut fetched).ok().ok()?;
    il.truncate(fetched as usize);
    Some(il)
}

/// ディスクに無いモジュールの像を対象のメモリから読み、ファイルの並びにする。動的なモジュール（像が無い）は `None`。
unsafe fn module_image(module: &ComPtr<ICorDebugModule>) -> Option<Vec<u8>> {
    // 壊れた大きさで大量に読まないよう、上限を置く（System.Private.CoreLib でも十数 MB）。
    const LIMIT: u32 = 256 << 20;
    let (mut base, mut size) = (0u64, 0u32);
    module.GetBaseAddress(&mut base).ok().ok()?;
    module.GetSize(&mut size).ok().ok()?;
    if base == 0 || size == 0 || size > LIMIT {
        return None;
    }
    let process = take::<ICorDebugProcess>("ICorDebugModule::GetProcess", |out| module.GetProcess(out)).ok()?;
    let mut image = vec![0u8; size as usize];
    let mut read = 0usize;
    if !process.ReadMemory(base, size, image.as_mut_ptr(), &mut read).is_ok() || read != image.len() {
        return None;
    }
    target::file_layout(&image)
}

/// フレームのメソッドが「利用者のコード」（Just My Code の印。シンボルのあるモジュールに付けている）か。
unsafe fn is_user_code(frame: &ComPtr<ICorDebugFrame>) -> bool {
    let mut function = null_mut();
    frame.GetFunction(&mut function);
    let Some(function) = ComPtr::<ICorDebugFunction>::from_raw(function).and_then(|f| f.cast::<ICorDebugFunction2>().ok()) else { return false };
    let mut user = 0i32;
    function.GetJMCStatus(&mut user).is_ok() && user != 0
}

/// パスの最後の部分（ファイル名）。
fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn thread_id(thread: &ComPtr<ICorDebugThread>) -> u32 {
    let mut id = 0u32;
    unsafe { thread.GetID(&mut id) };
    id
}

/// 式を一つ評価する間だけの覚え書き。対象を動かす問い合わせ（関数評価）と、モジュールの走査を繰り返さないためのもの。
///
/// ラムダ式を式木に組み立てるとき、同じ名前の文字列、同じ型の `System.Type`、同じ節の型を、何度も使う。そのたびに
/// 対象を動かすと、式 1 つに数百回の関数評価になる（計測では、時間のほとんどがこれだった）。
///
/// 止まっている場所で意味の決まる名前（スレッド、フレームの深さ、名前）。
type ScopedName = (u32, usize, String);

/// 中の値は、対象を続行すると使えなくなる。式の評価の前後と、続行するときに、必ず空にする。
#[derive(Default)]
struct EvalCache {
    /// 対象の中に作った文字列（`Expression` のメソッドに渡す名前など）。.NET の文字列は書き換えられないので、使い回せる。
    strings: std::collections::HashMap<String, ComPtr<ICorDebugValue>>,
    /// 型の名前（表示の書き方）から作った `System.Type` のオブジェクト。名前の意味は、止まっている場所（スレッド、フレームの深さ）で決まる。
    type_objects: std::collections::HashMap<ScopedName, ComPtr<ICorDebugValue>>,
    /// 名前から探した型（スレッド、フレームの深さ、名前 → モジュールの番号と行）。モジュールが増減したら捨てる。
    types: std::cell::RefCell<std::collections::HashMap<ScopedName, Option<(usize, u32)>>>,
    /// 式木の節と、その型の名前。節は作ったあと変わらない。
    node_types: Vec<(ComPtr<ICorDebugValue>, String)>,
}

impl EvalCache {
    fn clear(&mut self) {
        *self = EvalCache::default();
    }
}

/// 止まる出来事ではなかったので、対象をそのまま続行させる。続行できなくても、ここからは直せないので、ログに残すだけ。
fn resume(controller: &ComPtr<ICorDebugController>) {
    let hr = unsafe { controller.Continue(0) };
    if !hr.is_ok() {
        crate::log!("Continue failed: {hr}");
    }
}

/// TypeDef のトークンの、表を表す上位のバイト。下位に、表の行の番号を入れて使う。
const TYPE_DEF: u32 = 0x0200_0000;

/// 対象の出来事を受けたあと、対象をどうするか。
enum Outcome {
    /// そのまま続行する。
    Continue,
    /// 止まって、この `stopped` を送る。
    Stop(Event),
    /// 判定の要る利用者のブレークポイント（条件、ヒットカウント、印、ログポイント、値の監視の書き込みの行）に当たった。
    /// 判定してから決める。
    Hits { thread: u32, ids: Vec<i64> },
    /// 値の監視で、書き込みの行を越えた。値が変わったかを確かめてから決める。
    Watched { thread: u32 },
    /// 止めずに読む記録（プローブ、例外ログ）。読んで記録し、そのあと `then` に従う（同じ場所に利用者のブレークポイントが
    /// 無く、例外で止まらなければ `Continue`）。
    Record { thread: u32, what: probes::Recording, then: Box<Outcome> },
}
impl Session {
    /// ICorDebug のモジュールが、読み込みを記録したモジュール（`self.modules`）の何番目か。
    fn module_index(&self, module: &ComPtr<ICorDebugModule>) -> Option<usize> {
        let path = module_path(module);
        self.modules.iter().position(|m| m.path == path)
    }

    /// 値のクラスの、モジュールの番号と、TypeDef の表の行。
    fn class_row(&self, value: &ComPtr<ICorDebugValue>) -> Option<(usize, u32)> {
        let (module, token) = unsafe { value::class_of_value(value) }?;
        Some((self.module_index(&module)?, token & 0x00FF_FFFF))
    }
}

impl Session {
    pub fn new() -> Session {
        Session {
            state: State::NotStarted,
            config: None,
            attach_to: None,
            attach_framework: false,
            resume_child: false,
            dbgshim: None,
            launched: None,
            cordebug: None,
            process: None,
            process_handle: null_mut(),
            modules: Vec::new(),
            breakpoints: BTreeMap::new(),
            function_breakpoints: Vec::new(),
            next_breakpoint_id: 1,
            stopped_controller: None,
            threads: Vec::new(),
            frozen_threads: Default::default(),
            exception_in_user_code: Default::default(),
            async_step_out: None,
            patched_thread: None,
            queued_breakpoint: None,
            landing_after_async_out: false,
            object_ids: Vec::new(),
            last_object_id: 0,
            last_com_objects: None,
            data_watch: None,
            anonymous_assemblies: 0,
            main_thread: None,
            frames: Vec::new(),
            handles: Vec::new(),
            entry_breakpoint: None,
            seeking_entry: false,
            stepper: None,
            just_my_code: true,
            step_filtering: true,
            stepping_in: false,
            leaving_filtered: false,
            symbol_directories: symbols::directories_from_environment(),
            exception_filters: ExceptionFilters::default(),
            stopped_exception: None,
            goto_targets: Vec::new(),
            return_values: Vec::new(),
            return_value_breakpoints: Vec::new(),
            stepping_out: None,
            await_breakpoints: Vec::new(),
            deferred: Vec::new(),
            receivers: Vec::new(),
            checked: None,
            object_typed: Vec::new(),
            dynamic_typed: Vec::new(),
            pointer_typed: Vec::new(),
            pointer_copies: Vec::new(),
            awaiting: 0,
            lambda_flows: Vec::new(),
            lambda_failure: None,
            eval_cache: EvalCache::default(),
            pattern_locals: Vec::new(),
            display_this: None,
            current_request: None,
            implicit_evaluation_off: false,
            implicit_evaluation_allowed: true,
            observe_only: false,
            user_evaluation: false,
            touched_target: false,
            eval_stats: Default::default(),
            enc: enc::EncState::default(),
            decompile: DecompileState::default(),
            probes: probes::ProbeState::default(),
            hit_state: hits::HitState::default(),
            step_targets: Vec::new(),
            target_step: None,
            watch_sites: BTreeMap::new(),
            watch_step: None,
            ui_freeze: monitor::FreezeSettings::default(),
            freeze_watch: None,
            sql_break: sqlbreak::SqlBreakState::default(),
            dialog_breaks: Vec::new(),
            transactions: transactions::TransactionState::default(),
            environment_seq: None,
            children: children::ChildState::default(),
            dump: dumps::DumpState::default(),
            coverage: coverage::CoverageState::default(),
        }
    }

    /// 要求の処理の途中でたまった出来事を取り出す。要求に答えたあとで呼ぶ。
    pub fn take_deferred(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.deferred)
    }

    /// 試験しているプログラム（起動したものか、アタッチした先の exe）。
    pub fn program_path(&self) -> Option<String> {
        self.config.as_ref().map(|c| c.program.clone()).or_else(|| self.attach_to.and_then(target::process_image_path))
    }

    /// 対象のプロセスの番号（ランタイムが始まっていなければ `None`）。
    pub fn target_process_id(&self) -> Option<u32> {
        self.process_id()
    }

    pub fn is_stopped(&self) -> bool {
        self.state == State::Stopped
    }

    /// 対象プロセスがまだ生きている（終了の通知を受け取っていない）。
    pub fn is_live(&self) -> bool {
        self.process.is_some() && self.state != State::Exited
    }

    /// 対象で起きた出来事を処理する。止まるべき出来事でなければ、そのまま続行する。
    pub fn debug_event(&mut self, event: DebugEvent) -> Vec<Event> {
        let mut out = Vec::new();
        // 前の知らせで覚えた「同じ場所の 2 つ目」は、すぐ次の知らせのときだけ使う（間に別の知らせが来たら、もう対象の
        // コードが動いているので、同じ場所でも新しく当たったもの）。
        let queued = self.queued_breakpoint.take();
        let outcome = match event.kind {
            EventKind::CreateProcess(process) => {
                unsafe { process.GetHandle(&mut self.process_handle) };
                let mut process_id = 0u32;
                unsafe { process.GetID(&mut process_id) };
                self.start_freeze_watch(process_id);
                self.start_coverage_ticker();
                if self.process.is_none() {
                    self.process = Some(process);
                }
                Outcome::Continue
            }
            EventKind::CreateAppDomain(appdomain) => {
                // アタッチしないと、このアプリケーションドメインの出来事が届かない。
                let hr = unsafe { appdomain.Attach() };
                crate::log!("appdomain attach: {hr}");
                Outcome::Continue
            }
            EventKind::LoadModule(module) => {
                out.extend(self.module_loaded(module));
                Outcome::Continue
            }
            EventKind::UnloadModule(module) => {
                let path = module_path(&module);
                self.modules.retain(|m| m.path != path);
                // モジュールの番号が変わる。
                self.eval_cache.types.borrow_mut().clear();
                Outcome::Continue
            }
            EventKind::CreateThread(thread) => {
                let id = thread_id(&thread);
                out.push(self.thread_started(id));
                self.data_watch_new_thread(id);
                Outcome::Continue
            }
            EventKind::BeforeGarbageCollection => {
                self.data_watch_before_gc();
                Outcome::Continue
            }
            EventKind::AfterGarbageCollection => {
                self.data_watch_after_gc();
                Outcome::Continue
            }
            EventKind::DataBreakpoint { thread } => {
                self.cancel_step();
                Outcome::Stop(self.data_watch_hit(thread_id(&thread)))
            }
            EventKind::ExitThread(thread) => {
                let id = thread_id(&thread);
                self.threads.retain(|&t| t != id);
                out.push(("thread", json!({ "reason": "exited", "threadId": id })));
                Outcome::Continue
            }
            EventKind::Breakpoint { thread, breakpoint } => {
                let id = thread_id(&thread);
                if queued.is_some_and(|(at, place)| at == id && breakpoint_place(&breakpoint).as_ref() == Some(&place)) {
                    crate::log!("second breakpoint callback for the same hit on thread {id}: continue");
                    Outcome::Continue
                } else {
                    self.breakpoint_hit(&thread, &breakpoint)
                }
            }
            EventKind::StepComplete { thread, stepper, reason } => match stepper.as_ref().and_then(|s| self.sql_timer_finished(s, reason).or_else(|| self.child_started(s, reason))) {
                // SQL の実行時間や、子プロセスの起動を見張っていたステッパー（利用者のステップではない）。
                Some(events) => {
                    out.extend(events);
                    Outcome::Continue
                }
                None => self.step_completed(thread_id(&thread), reason),
            },
            EventKind::Exception { thread, kind, frame } => {
                const FIRST_CHANCE: u32 = 1;
                let id = thread_id(&thread);
                let running = self.state == State::Running;
                let stopped = self.exception_stop(id, kind, frame.as_ref());
                let then = stopped.map_or(Outcome::Continue, Outcome::Stop);
                // 例外ログ: 投げられた例外は、止まらないものも記録する（止まるなら、記録してから止まる）。
                const CATCH_HANDLER_FOUND: u32 = 3;
                if kind == FIRST_CHANCE && running && self.probes.exception_log {
                    let stops = matches!(then, Outcome::Stop(_));
                    Outcome::Record { thread: id, what: probes::Recording::Exception { stops }, then: Box::new(then) }
                } else if let Some(method) = (kind == CATCH_HANDLER_FOUND && running)
                    .then(|| match frame.as_ref() {
                        Some(frame) => self.binding_catch(frame),
                        None => self.binding_catch_on_stack(id),
                    })
                    .flatten()
                {
                    // WinForms のバインディングが例外を飲み込む（bindings.rs）。
                    Outcome::Record { thread: id, what: probes::Recording::Binding { method }, then: Box::new(then) }
                } else {
                    then
                }
            }
            EventKind::Break { thread } => {
                self.cancel_step();
                Outcome::Stop(("stopped", json!({ "reason": "pause", "threadId": thread_id(&thread), "allThreadsStopped": true })))
            }
            EventKind::ExitProcess => {
                out.extend(self.process_exited());
                return out;
            }
            // 評価の終わりは、待っている側（funceval.rs）が受け取る。ここに来るのは、時間切れで待つのをやめたあとに
            // 終わった評価。対象を止めたままにしないよう、そのまま続行する。
            EventKind::EvalComplete { .. } => {
                crate::log!("callback EvalComplete (nobody is waiting): continue");
                Outcome::Continue
            }
            EventKind::FunctionRemapOpportunity { thread, old_function, new_function, old_il_offset } => {
                out.extend(self.remap_opportunity(&thread, &old_function, &new_function, old_il_offset));
                Outcome::Continue
            }
            EventKind::FunctionRemapComplete { thread } => {
                out.extend(self.remap_complete(&thread));
                Outcome::Continue
            }
            EventKind::Other(name) => {
                crate::log!("callback {name}: continue");
                Outcome::Continue
            }
        };
        self.finish(outcome, event.controller, &mut out);
        // 止まったら、カバレッジの変わった行をすぐ知らせる（時のスレッドを待たない）。
        if self.coverage.active() && out.iter().any(|(name, _)| *name == "stopped") {
            out.extend(self.coverage_changed());
        }
        out
    }

    /// 出来事の処理の結果に従って、続行するか止まる。
    fn finish(&mut self, outcome: Outcome, controller: Option<ComPtr<ICorDebugController>>, out: &mut Vec<Event>) {
        match (outcome, controller) {
            (Outcome::Continue, Some(controller)) => resume(&controller),
            (Outcome::Hits { thread, ids }, Some(controller)) => out.extend(self.stop_if_any_breakpoint_stops(thread, &ids, &controller)),
            (Outcome::Watched { thread }, Some(controller)) => out.extend(self.stop_if_value_changed(thread, &controller)),
            (Outcome::Record { thread, what, then }, Some(controller)) => {
                out.extend(self.run_record(thread, &what, &controller));
                // 評価のために止めた形にしていたのを戻してから、利用者のブレークポイントの扱いに進む。
                self.state = State::Running;
                self.stopped_controller = None;
                // 記録の処理が止まると決めた（SQL やダイアログの条件に合った）: 同じ場所の利用者のブレークポイントより先に、その理由で止まる。
                let then = match self.take_record_stop() {
                    Some(stopped) => {
                        self.cancel_step();
                        self.patched_thread = Some(thread);
                        Outcome::Stop(stopped)
                    }
                    None => *then,
                };
                self.finish(then, Some(controller), out);
            }
            (Outcome::Stop(stopped), controller) => {
                out.push(stopped);
                match controller {
                    // `pause` で止めたあとに、先に起きていた停止が届いた。対象は二重に止められているので、
                    // 片方をここで解く（ICorDebug の停止は回数で数えられ、同じ回数だけ Continue するまで動かない）。
                    Some(controller) if self.state == State::Stopped => unsafe {
                        controller.Continue(0);
                    },
                    Some(controller) => {
                        self.state = State::Stopped;
                        self.stopped_controller = Some(controller);
                    }
                    None => {}
                }
            }
            // 続行する相手が無い（ExitProcess のときだけ。上で返している）。
            (_, None) => {}
        }
    }

    /// スレッドができた。`Main Thread` は、プロセスの最初のスレッド（`Main` を実行する）。通知の順は当てにならない
    /// （ランタイムが先に作ったスレッドが、先に知らされることがある）。分からなければ、最初に知らされたスレッド。
    fn thread_started(&mut self, id: u32) -> Event {
        if self.main_thread.is_none() {
            self.main_thread = Some(self.process_id().and_then(target::primary_thread).unwrap_or(id));
        }
        if !self.threads.contains(&id) {
            self.threads.push(id);
        }
        ("thread", json!({ "reason": "started", "threadId": id }))
    }

    /// ブレークポイントに当たった。エンジンが自分のために置いたもの（入口、await、戻り値、async の stepOut）は、
    /// 利用者には見せずに処理する。利用者のブレークポイントは、条件があれば確かめてから止まる。
    fn breakpoint_hit(&mut self, thread: &ComPtr<ICorDebugThread>, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Outcome {
        let id = thread_id(thread);
        // カバレッジ（coverage.rs）: 通った印を付ける。カバレッジだけのものなら続ける。同じ場所にエンジンのほかの
        // ブレークポイントがあれば、そちらに当たったものとして扱う（ランタイムは、同じ場所のものを一つしか知らせない）。
        if self.coverage.active() {
            match self.coverage_hit(id, breakpoint) {
                coverage::CoverageHit::Only => return Outcome::Continue,
                coverage::CoverageHit::Shared(other) => return self.breakpoint_hit(thread, &other),
                coverage::CoverageHit::NotCoverage => {}
            }
        }
        // 種類（関数のブレークポイントなど）が違っても、同じオブジェクトなら同じポインター。
        let raw = breakpoint.as_raw();
        if self.entry_breakpoint.as_ref().is_some_and(|b| b.as_raw() == raw) {
            self.cancel_step();
            if let Some(entry) = self.entry_breakpoint.take() {
                unsafe { entry.Activate(0) };
            }
            self.patched_thread = Some(id);
            return Outcome::Stop(("stopped", json!({ "reason": "entry", "threadId": id, "allThreadsStopped": true })));
        }
        // async メソッドからの stepOut の途中: 待っている側が待ち終えた。そこから stepOut する。
        if self.async_step_out.as_ref().is_some_and(|b| b.as_raw() == raw) {
            self.async_step_out_reached(id);
            return Outcome::Continue;
        }
        // async メソッドのステップ中の、await の中断と再開。
        if self.await_breakpoints.iter().any(|b| b.breakpoint.as_raw() == raw) {
            self.await_reached(id, breakpoint);
            return Outcome::Continue;
        }
        // 戻り値を拾うためのもの。値を読んで、ステップをそのまま続けさせる。
        if self.return_value_breakpoints.iter().any(|b| b.breakpoint.as_raw() == raw) {
            self.capture_return_value(id, breakpoint);
            return Outcome::Continue;
        }
        // Step Into Target: 選んだ呼び出しの中に着いた。
        if let Some(outcome) = self.target_step_arrived(id, breakpoint) {
            return outcome;
        }
        // 止めずに読むブレークポイント（SQL ログなど）。同じ場所の利用者のブレークポイントは、記録のあとで扱う（ランタイムは
        // ふつう、同じ場所のブレークポイントを一つしか知らせてこない。両方を知らせてきたときの 2 つ目は捨てる:
        // `remember_queued_breakpoint`）。止まっている間（ほかの停止と重なった）は読まない。
        if let Some(what) = self.probe_at(breakpoint) {
            self.remember_queued_breakpoint(thread, breakpoint);
            let users = self.user_breakpoints_at(breakpoint);
            let then = if users.is_empty() { Outcome::Continue } else { self.user_stop(id, users) };
            return if self.state == State::Running { Outcome::Record { thread: id, what, then: Box::new(then) } } else { then };
        }
        let hit = self.breakpoints_bound_to(breakpoint);
        // どの利用者のブレークポイントにも結び付いていない: 外したばかりの内部のブレークポイントに、外す前に当たっていたもの。
        if hit.is_empty() {
            crate::log!("breakpoint that is no longer used: continue");
            return Outcome::Continue;
        }
        self.remember_queued_breakpoint(thread, breakpoint);
        self.user_stop(id, hit)
    }

    /// 同じ場所にブレークポイントが 2 つ（利用者のものとプローブ）あると、ランタイムは 1 回の実行を 1 つだけ知らせる
    /// こともあれば、続けて 2 つ知らせることもある（CI の x86 の .NET Framework で、入口がネイティブにコンパイル済みの
    /// とき）。1 つ目で同じ場所のものをすべて扱うので、そのスレッドにまだ知らせが溜まっていれば場所を覚え、すぐ次の
    /// 知らせが同じスレッド・同じ場所なら捨てる（`debug_event`）。溜まっていなければ覚えない（ループで同じ場所に
    /// もう一度当たったものは、間に対象のコードが動くので、溜まってはいない）。
    fn remember_queued_breakpoint(&mut self, thread: &ComPtr<ICorDebugThread>, breakpoint: &ComPtr<ICorDebugBreakpoint>) {
        let (Some(process), Some(place)) = (self.process.as_ref(), breakpoint_place(breakpoint)) else { return };
        let mut queued = 0i32;
        let asked = unsafe { process.HasQueuedCallbacks(thread.as_raw(), &mut queued) };
        if asked.is_ok() && queued != 0 {
            self.queued_breakpoint = Some((thread_id(thread), place));
        }
    }

    /// 利用者のブレークポイント（番号）に当たったとき: どれも判定の要らないものなら止まる。判定の要るもの（条件、
    /// ヒットカウント、印、ログポイント）があれば、判定してから決める（`hits.rs`）。
    fn user_stop(&mut self, id: u32, hit: Vec<i64>) -> Outcome {
        if self.state == State::Running && hit.iter().any(|&b| self.is_watch_site(b) || self.needs_judging(b)) {
            return Outcome::Hits { thread: id, ids: hit };
        }
        // 値の監視の書き込みの行は、判定できないとき（止まっている間に当たった）は、無かったことにする。
        let hit: Vec<i64> = hit.into_iter().filter(|&b| !self.is_watch_site(b)).collect();
        if hit.is_empty() {
            return Outcome::Continue;
        }
        self.cancel_step();
        self.patched_thread = Some(id);
        Outcome::Stop(("stopped", json!({ "reason": "breakpoint", "threadId": id, "allThreadsStopped": true, "hitBreakpointIds": hit })))
    }

    /// 対象のプロセスの番号（ランタイムがまだ始まっていなければ `None`）。
    fn process_id(&self) -> Option<u32> {
        self.process.as_ref().map(|process| unsafe {
            let mut id = 0u32;
            process.GetID(&mut id);
            id
        })
    }

    /// 判定のために、対象を止めた形にする（式は、止まっている対象の上で評価する。プロパティやメソッドを含むなら、
    /// 対象のコードも実行する）。コールバックの間、対象は止まっている。
    fn hold_for_judging(&mut self, controller: &ComPtr<ICorDebugController>) {
        self.state = State::Stopped;
        self.stopped_controller = Some(controller.clone());
    }

    /// 判定の結果、止まらない: 何も無かったことにして続ける（進行中のステップもそのまま。利用者には停止を見せない）。
    fn resume_unseen(&mut self) {
        self.state = State::Running;
        self.forget_stop();
        if let Some(controller) = self.stopped_controller.take() {
            resume(&controller);
        }
    }

    /// ブレークポイントに、当たったときの判定が要るか。
    fn needs_judging(&self, id: i64) -> bool {
        let line = self.breakpoints.values().flatten().find(|b| b.id == id).map(|b| b.condition.is_some() || b.hit_condition.is_some() || b.marker.is_some() || b.log_message.is_some());
        line.unwrap_or_else(|| self.function_breakpoints.iter().any(|b| b.id == id && b.condition.is_some()))
    }

    /// ステップが一歩終わった。止まれる場所（ソースのある行）に着くまで進め、着いたら止まる。
    fn step_completed(&mut self, id: u32, reason: u32) -> Outcome {
        const STEP_RETURN: u32 = 1;
        self.stepper = None;
        if reason == STEP_RETURN && self.stepping_out == Some(id) {
            self.read_return_value_after_step_out(id);
        }
        self.remove_return_value_breakpoints();
        self.remove_await_breakpoints();
        if !self.settle_step(id, reason) {
            return Outcome::Continue;
        }
        // 値の監視で、書き込みの行を越えた。
        if self.is_watch_step(id) {
            return Outcome::Watched { thread: id };
        }
        // async メソッドからの stepOut の準備が残っていれば、外す（ふつうの stepOut の方で着いた）。
        if let Some(breakpoint) = self.async_step_out.take() {
            unsafe { breakpoint.Activate(0) };
        }
        // Step Into Target で、行が選んだ呼び出しをしなかった（次の行に着いた）。
        self.end_target_step();
        self.patched_thread = Some(id);
        Outcome::Stop(("stopped", json!({ "reason": "step", "threadId": id, "allThreadsStopped": true })))
    }

    /// 対象が終わった。終了コードは符号つきの 32bit として伝える（例外で落ちたときの 0xC000.... が負になる）。
    fn process_exited(&mut self) -> Vec<Event> {
        let mut exit_code = 0u32;
        if !self.process_handle.is_null() {
            unsafe { win32::GetExitCodeProcess(self.process_handle, &mut exit_code) };
        }
        // カバレッジは、終わりの知らせより前に書き出して、書いた先を知らせる。
        let mut out = self.finish_coverage();
        self.forget_process();
        self.enc.reset();
        out.extend([("exited", json!({ "exitCode": exit_code as i32 })), ("terminated", Value::Null)]);
        out
    }
    /// 例外の通知で止まるなら、`stopped` の通知。止まるのは、処理されない例外と、利用者が選んだ型の例外が投げられたとき、
    /// フィルター `user-unhandled` で、利用者のコードを通った例外を、利用者のコードではない所が捕まえるとき。
    fn exception_stop(&mut self, thread: u32, kind: u32, frame: Option<&ComPtr<ICorDebugFrame>>) -> Option<Event> {
        // CorDebugExceptionCallbackType
        const FIRST_CHANCE: u32 = 1;
        const USER_FIRST_CHANCE: u32 = 2;
        const CATCH_HANDLER_FOUND: u32 = 3;
        const UNHANDLED: u32 = 4;
        crate::log!("exception callback: kind {kind}, thread {thread}, frame {}", frame.is_some());
        let break_mode = match kind {
            FIRST_CHANCE => {
                // 新しく投げられた例外。前の例外の「利用者のコードを通った」は捨てる。
                self.exception_in_user_code.remove(&thread);
                "always"
            }
            // 例外が、利用者のコード（Just My Code の印の付いたフレーム）に来た。
            USER_FIRST_CHANCE => {
                self.exception_in_user_code.insert(thread);
                return None;
            }
            CATCH_HANDLER_FOUND => {
                let passed = self.exception_in_user_code.remove(&thread);
                // 捕まえるフレームが無い（null）のは、ランタイムの中で捕まえるとき（タスクの仕組みなど）。利用者のコードではない。
                if !(self.exception_filters.user_unhandled && passed && frame.is_none_or(|frame| !unsafe { is_user_code(frame) })) {
                    return None;
                }
                "userUnhandled"
            }
            UNHANDLED => {
                self.exception_in_user_code.remove(&thread);
                "unhandled"
            }
            _ => return None,
        };
        let type_name = self.current_exception(thread).map(|e| unsafe { value::describe(&e, &self.modules) }.type_name).unwrap_or_else(|| "?".into());
        if kind == FIRST_CHANCE && !self.exception_filters.breaks_on_throw(&type_name) {
            return None;
        }
        self.cancel_step();
        // 例外が起きた場所のモジュール。フレームが無ければ分からない。
        let module = self.walk_frames(thread).ok().and_then(|frames| unsafe { self.frame_module(frames.first()?) }.map(|m| file_name(&m.path))).unwrap_or_else(|| "<unknown module>".into());
        // 文言は netcoredbg と同じ。
        let text = match break_mode {
            "unhandled" => format!("An unhandled exception of type '{type_name}' occurred in {module}"),
            "userUnhandled" => format!("An exception of type '{type_name}' occurred in {module} but was not handled in user code"),
            _ => format!("Exception thrown: '{type_name}' in {module}"),
        };
        let stopped = ("stopped", json!({ "reason": "exception", "threadId": thread, "text": text, "allThreadsStopped": true }));
        self.stopped_exception = Some(StoppedException { thread, break_mode, text });
        Some(stopped)
    }

    /// ICorDebug のブレークポイントに結び付いている、利用者のブレークポイント（行と関数）の「番号、条件」。
    fn breakpoints_bound_to(&self, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Vec<i64> {
        self.bindings().filter(|(_, bound)| bound.iter().any(|bound| bound.as_raw() == breakpoint.as_raw())).map(|(id, _)| id).collect()
    }

    /// 行と関数のブレークポイント（持ち主によらず）の、番号と、置いた ICorDebug のブレークポイント。
    fn bindings(&self) -> impl Iterator<Item = (i64, &[ComPtr<ICorDebugFunctionBreakpoint>])> {
        let lines = self.breakpoints.values().flatten().map(|b| (b.id, b.bound.as_slice()));
        lines.chain(self.function_breakpoints.iter().map(|b| (b.id, b.bound.as_slice())))
    }

    /// 判定の要るブレークポイントに当たったとき。どれかが止まれば止まり（`stopped` を送る）、どれも止まらなければ
    /// 何も無かったことにして続ける（進行中のステップもそのまま）。
    fn stop_if_any_breakpoint_stops(&mut self, thread: u32, ids: &[i64], controller: &ComPtr<ICorDebugController>) -> Vec<Event> {
        let mut out = Vec::new();
        self.hold_for_judging(controller);
        let (sites, users): (Vec<i64>, Vec<i64>) = ids.iter().partition(|&&id| self.is_watch_site(id));
        let stopping = self.judge_hits(thread, &users, &mut out);
        if stopping.is_empty() {
            // 利用者のブレークポイントでは止まらない。値の監視の書き込みの行なら、行を越えてから確かめる（すでに別の
            // 書き込みの行を越えている途中なら、そちらを続ける。前の値を失わないように）。
            if !sites.is_empty() && self.watch_step.is_none() {
                self.start_value_watch(thread, &sites);
            }
            self.resume_unseen();
        } else {
            self.cancel_step();
            self.patched_thread = Some(thread);
            out.push(("stopped", json!({ "reason": "breakpoint", "threadId": thread, "allThreadsStopped": true, "hitBreakpointIds": stopping })));
        }
        out.extend(self.take_deferred());
        out
    }

    /// 値の監視で、書き込みの行を越えたとき。変わっていれば止まり（`stopped` を送る）、変わっていなければ続ける。
    fn stop_if_value_changed(&mut self, thread: u32, controller: &ComPtr<ICorDebugController>) -> Vec<Event> {
        let mut out = Vec::new();
        self.hold_for_judging(controller);
        match self.finish_value_watch(thread) {
            Some(stopped) => {
                self.cancel_step();
                self.patched_thread = Some(thread);
                out.push(stopped);
            }
            None => self.resume_unseen(),
        }
        out.extend(self.take_deferred());
        out
    }

    fn forget_process(&mut self) {
        self.state = State::Exited;
        self.freeze_watch = None;
        self.sql_break.forget_target();
        self.transactions.forget_target();
        self.children.forget_target();
        self.frozen_threads.clear();
        self.object_ids.clear();
        if self.anonymous_assemblies > 0 {
            crate::debugger::anonymous::remove_written();
        }
        self.stopped_controller = None;
        self.entry_breakpoint = None;
        self.stepper = None;
        self.stopped_exception = None;
        self.return_value_breakpoints.clear();
        self.await_breakpoints.clear();
        self.return_values.clear();
        self.stepping_out = None;
        self.frames.clear();
        self.handles.clear();
        self.threads.clear();
        self.modules.clear();
        self.eval_cache.clear();
        for breakpoint in self.breakpoints.values_mut().flatten() {
            breakpoint.bound.clear();
        }
        for breakpoint in self.function_breakpoints.iter_mut() {
            breakpoint.bound.clear();
        }
        self.process = None;
        self.process_handle = null_mut();
        if let Some(cordebug) = self.cordebug.take() {
            // すべての対象が終わってからでないと失敗する。ExitProcess のあとなので呼べる。
            let hr = unsafe { cordebug.Terminate() };
            crate::log!("ICorDebug::Terminate: {hr}");
        }
    }

    fn module_loaded(&mut self, module: ComPtr<ICorDebugModule>) -> Vec<Event> {
        let started = std::time::Instant::now();
        // 名前から探した型の覚え書きは、モジュールが増えると変わり得る（評価の途中の読み込みでも、ここを通る）。
        self.eval_cache.types.borrow_mut().clear();
        let path = module_path(&module);
        let found = if Path::new(&path).is_file() {
            symbols::find(Path::new(&path), &self.symbol_directories)
        } else {
            // ディスクに無いモジュール（単一ファイルのアプリの中のアセンブリ）。名前はパスではなくアセンブリの名前になっている。
            // 像を対象のメモリから読み、PDB はアプリのフォルダーから探す。
            let app_directory = self.config.as_ref().and_then(|config| Path::new(&config.program).parent().map(Path::to_path_buf));
            match unsafe { module_image(&module) } {
                Some(image) => symbols::find_in_memory(&path, &image, app_directory.as_deref(), &self.symbol_directories),
                None => symbols::Found::default(),
            }
        };
        let symbols::Found { pdb, pdb_path, metadata, problem: unreadable } = found;
        let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
        let mut out = vec![(
            "module",
            json!({
                "reason": "new",
                "module": {
                    "id": path,
                    "name": name,
                    "path": path,
                    // 拡張はこの文字列で「シンボルあり」を判定している（docs/plan.html「表記の互換」）。
                    "symbolStatus": if pdb.is_some() { "Symbols loaded." } else { "No symbols loaded." },
                }
            }),
        )];
        // PDB があるのに読めないと、ブレークポイントが効かない理由が利用者に見えない。直し方と一緒に知らせる。
        if let Some(reason) = unreadable {
            out.push(("output", json!({ "category": "console", "output": tr!("Cannot load the symbols: {reason}\n", reason = reason) })));
        }
        // Just My Code のステップは「自分のコード」と印を付けたコードでしか止まらない。印は既定で付いていないので、
        // シンボルのあるモジュールに付ける。
        let mut enc = false;
        // ダンプでは JIT も Just My Code も無い（コードは動かない）。
        if pdb.is_some() && !self.is_dump() {
            if let Ok(module2) = module.cast::<ICorDebugModule2>() {
                // 最適化つきでビルドされたモジュールは、そのままでは「自分のコード」にできない（0x80131323）。まだ何も
                // コンパイルされていない読み込みの時点で、JIT の最適化を切っておく。変数も消えずに読めるようになる。
                // すでにコンパイル済み（アタッチ、事前コンパイル）なら失敗するが、そのまま進む。
                // Edit and Continue（D-059）が有効なら、最適化を切ったうえで EnC の形にする（EnC は最適化なしを含む）。
                // EnC にできないもの（最適化つきでビルドしたモジュール）は、ランタイムが成功コード（CORDBG_S_NOT_ALL_BITS_SET）で
                // 断るので、S_OK のときだけ「EnC できる」とする。
                let flags = if self.wants_enc() { CORDEBUG_JIT_ENABLE_ENC } else { CORDEBUG_JIT_DISABLE_OPTIMIZATION };
                let hr = unsafe { module2.SetJITCompilerFlags(flags) };
                enc = flags == CORDEBUG_JIT_ENABLE_ENC && hr == crate::com::S_OK;
                crate::log!("SetJITCompilerFlags({flags:#x}) {path}: {hr}");
                let hr = unsafe { module2.SetJMCStatus(1, 0, null_mut()) };
                if !hr.is_ok() {
                    crate::log!("SetJMCStatus {path}: {hr}");
                }
            }
        }
        // PDB の無いアプリのモジュールは、必要になったときに逆コンパイルする（D-060）。JIT の設定は今しかできない。
        self.note_program_module(&path, pdb.is_some());
        let decompile = if self.is_decompile_target(&path, pdb.is_some()) {
            if !self.is_dump() {
                self.prepare_decompile_target(&module, &path);
            }
            Decompile::Pending
        } else {
            Decompile::NotTarget
        };
        // シンボルを探すときにアセンブリを読んでいれば、そのメタデータを使い回す。
        let metadata = metadata.map_or_else(OnceCell::new, |m| OnceCell::from(Some(m)));
        self.modules.push(Module { module, path, pdb, pdb_path, metadata, enc, decompile });
        if enc {
            self.enc_open(self.modules.len() - 1);
        }
        let index = self.modules.len() - 1;
        // ダンプでは、ブレークポイントも止めずに読む記録も置けない。環境の記録も、いまの機械のものになるので残さない。
        if self.is_dump() {
            return out;
        }
        self.exclude_non_user_methods();
        if self.seeking_entry {
            self.break_at_entry();
        }

        // 読み込みを待っていたブレークポイントを、このモジュールに置く。
        self.bind_probes(index);
        // 最初のモジュール（ランタイムの基本のライブラリ）で、障害票用の環境を記録する（fault.rs）。
        let loaded = self.modules[index].path.clone();
        out.extend(self.record_environment_once(&loaded));
        for breakpoint in self.function_breakpoints.iter_mut() {
            let was_verified = !breakpoint.bound.is_empty();
            bind_function(&self.modules[index], breakpoint);
            if !was_verified && !breakpoint.bound.is_empty() {
                out.push(("breakpoint", json!({ "reason": "changed", "breakpoint": describe_function(breakpoint) })));
            }
        }
        for ((owner, source), breakpoints) in self.breakpoints.iter_mut() {
            for breakpoint in breakpoints.iter_mut() {
                let was_verified = breakpoint.verified_line.is_some();
                bind(&self.modules[index], source, breakpoint);
                if !was_verified && breakpoint.verified_line.is_some() && owner.is_reported() {
                    out.push(("breakpoint", json!({ "reason": "changed", "breakpoint": describe(breakpoint) })));
                }
            }
        }
        // カバレッジ（coverage.rs）。PDB の無いアプリのモジュールは、ここで逆コンパイルする。待っていたブレークポイントは
        // 逆コンパイルのときに置くので、上の置き直しの後で呼ぶ（上は PDB の無い今は何もしないので、二重には置かない）。
        out.extend(self.cover_module(index));
        // 前のセッションで、逆コンパイルしたソースに置いたブレークポイントがあれば、いま逆コンパイルして置く
        // （上の置き直しは PDB の無い今は何もしないので、二重には置かない）。
        if decompile == Decompile::Pending {
            let sources: Vec<String> = self.breakpoints.keys().map(|(_, source)| source.clone()).collect();
            for source in sources {
                out.extend(self.decompile_matching_source(&source));
            }
        }
        crate::log!("module loaded in {:.1} ms: {}", started.elapsed().as_secs_f64() * 1000.0, self.modules[index].path);
        out
    }

    /// いま読み込まれたモジュールの、「利用者のコードではない」と印の付いたメソッド（`DebuggerStepThrough` など）を
    /// 自分のコードから外す。ステップはその中で止まらずに通り過ぎる。
    fn exclude_non_user_methods(&self) {
        let Some(module) = self.modules.last().filter(|m| m.pdb.is_some()) else { return };
        let Some(metadata) = module.metadata() else { return };
        for method in metadata.non_user_methods() {
            unsafe {
                let mut raw = null_mut();
                module.module.GetFunctionFromToken(method, &mut raw);
                if let Some(function2) = ComPtr::<ICorDebugFunction>::from_raw(raw).and_then(|f| f.cast::<ICorDebugFunction2>().ok()) {
                    let hr = function2.SetJMCStatus(0);
                    if !hr.is_ok() {
                        crate::log!("SetJMCStatus(false) 0x{method:08X} in {}: {hr}", module.path);
                    }
                }
            }
        }
    }

    /// 対象が動いている間は ICorDebug の多くの操作ができない。動いていれば一時的に止めて `work` を行う。
    fn while_synchronized(&self, work: impl FnOnce()) {
        let running = self.process.as_ref().filter(|_| self.state == State::Running);
        if let Some(process) = running {
            let hr = unsafe { process.Stop(0) };
            if !hr.is_ok() {
                crate::log!("Stop failed: {hr}");
            }
        }
        work();
        if let Some(process) = running {
            unsafe { process.Continue(0) };
        }
    }

    /// `continue`。
    pub fn resume(&mut self) -> Result<Value, String> {
        if self.state != State::Stopped {
            return Err(tr!("The target is not stopped."));
        }
        let controller = self.stopped_controller.take().ok_or_else(|| tr!("There is nothing to continue."))?;
        unsafe { controller.Continue(0) }.ok().map_err(|hr| failed(tr_str!("Continuing"), hr))?;
        self.state = State::Running;
        self.forget_stop();
        self.forget_return_values();
        Ok(json!({ "allThreadsContinued": true }))
    }

    /// 止まっている間だけ意味のある記録（フレームや変数の番号、例外）を捨てる。対象を動かす直前に呼ぶ。
    fn forget_stop(&mut self) {
        self.frames.clear();
        self.handles.clear();
        self.stopped_exception = None;
        // 対象の中の値は、続行すると使えなくなる。
        self.eval_cache.clear();
        // 続行したので、暗黙の評価をまた行う。
        self.implicit_evaluation_off = false;
        self.patched_thread = None;
    }
}

/// ブレークポイントの場所（モジュール、メソッド、IL オフセット）。関数のブレークポイントでなければ `None`。
fn breakpoint_place(breakpoint: &ComPtr<ICorDebugBreakpoint>) -> Option<(String, u32, u32)> {
    let function_breakpoint = breakpoint.cast::<ICorDebugFunctionBreakpoint>().ok()?;
    unsafe { probes::location_of(&function_breakpoint) }
}