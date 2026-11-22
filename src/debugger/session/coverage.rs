//! カバレッジ（B1、D-079）: 系列点と分かれ道に置いたブレークポイントで、通った行と分かれ道の向きを数える。
//!
//! 置き方は 2 段: モジュールの読み込みで、メソッドごとに入口（IL 0）に 1 つ置く。入口に当たったら（JIT は済んでいる）、
//! IL とネイティブのコードの対応を見て、残りの系列点と、分かれ道を見るためのブレークポイントを置く。当たったら印を
//! 付けて外し、利用者には見せずに続ける。何をどこに置くかは `crate::debugger::coverage` が決める。
//!
//! 関数の評価の中で当たったものは数えない（評価の待ち合わせ `funceval.rs` が、ブレークポイントを続行するだけ）。外さずに
//! 残るので、あとで本当に通ったときに数える。

use super::probes::location_of;
use super::*;
use crate::debugger::coverage::{self, Coverage, Load, Role, Val};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 変わった行の知らせ（`netdbg.coverageChanged`）の間隔。
const NOTIFY_INTERVAL: Duration = Duration::from_millis(250);

/// ブレークポイントの場所（記録の中のモジュールとメソッドの添字、IL オフセット）。`returned` は、IL の位置ではなく、
/// その IL オフセットの直前の呼び出しから戻った直後の機械語の位置に置いたもの（戻り値を読む。同じ IL の位置の、
/// ほかの役目とは別に置く）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SiteKey {
    module: usize,
    method: usize,
    offset: u32,
    returned: bool,
}

/// 置いたブレークポイント（機械語の位置に置くものは、いくつかになることがある）と、まだ終えていない役目。
struct Site {
    breakpoints: Vec<ComPtr<ICorDebugFunctionBreakpoint>>,
    roles: Vec<Role>,
}

impl Site {
    fn deactivate(&self) {
        for breakpoint in &self.breakpoints {
            unsafe { breakpoint.Activate(0) };
        }
    }
}

/// 変わった行があれば、入力のループに時を知らせるスレッド（変わっていなければ、ループを起こさない）。
struct Ticker {
    stop: Arc<AtomicBool>,
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub(super) struct CoverageState {
    enabled: bool,
    /// 書き出す先（指定が無ければ、終わるときに既定の場所を決める）。
    output: Option<PathBuf>,
    /// 既定の場所の親のフォルダー（保存先。`output` が無いとき、この下に `<プログラム名>-<日時>.cobertura.xml`）。
    folder: Option<PathBuf>,
    data: Coverage,
    /// モジュールのパス（比べる形）→ `data.modules` の添字。
    modules: HashMap<String, usize>,
    /// `data.modules` ごとの、MethodDef のトークン → `methods` の添字。
    methods: Vec<HashMap<u32, usize>>,
    sites: HashMap<SiteKey, Site>,
    /// 置いたブレークポイント（生のポインター）→ 場所。外したあとも残す（外す前に積まれていた知らせを見分ける）。
    by_breakpoint: HashMap<usize, SiteKey>,
    /// 前の知らせのあとで印の変わったファイル（PDB に書かれたままのパス）。
    changed: BTreeSet<String>,
    /// 変わったファイルがあることを、時のスレッドに伝える。
    dirty: Arc<AtomicBool>,
    ticker: Option<Ticker>,
    /// 最後に書き出したファイル。
    written: Option<PathBuf>,
}

impl CoverageState {
    /// 新しい対象: 前の対象の記録を捨てる（入れるかと書き出す先の設定は残す）。
    pub(super) fn new_target(&mut self) {
        let (enabled, output, folder) = (self.enabled, self.output.take(), self.folder.take());
        *self = CoverageState { enabled, output, folder, ..CoverageState::default() };
    }

    /// 数えているか、数えた記録があるか。
    pub(super) fn active(&self) -> bool {
        self.enabled || !self.by_breakpoint.is_empty()
    }
}

/// ブレークポイントに当たったときの、カバレッジの側の答え。
pub(super) enum CoverageHit {
    /// カバレッジのブレークポイントではない（印は、同じ場所にあれば付けた）。
    NotCoverage,
    /// カバレッジだけのブレークポイント。続行してよい。
    Only,
    /// 同じ場所に、エンジンのほかのブレークポイントがある。そちらに当たったものとして扱う。
    Shared(ComPtr<ICorDebugBreakpoint>),
}

impl Session {
    /// 起動とアタッチの引数 `coverage`（`true`、`{ "output": "ファイル" }`、`{ "folder": "保存先" }`）。
    pub(super) fn read_coverage_options(&mut self, arguments: &Value) -> Result<(), String> {
        self.coverage.new_target();
        let (enabled, options) = match arguments.get("coverage") {
            Some(Value::Bool(enabled)) => (*enabled, None),
            Some(Value::Object(options)) => (options.get("enabled").and_then(Value::as_bool) != Some(false), Some(options)),
            _ => (false, None),
        };
        self.coverage.enabled = enabled;
        self.coverage.output = options.and_then(|o| o.get("output")).and_then(Value::as_str).filter(|p| !p.is_empty()).map(PathBuf::from);
        if let Some(options) = options {
            self.coverage.folder = coverage_folder(options.get("folder"))?;
        }
        Ok(())
    }

    /// `netdbg.setCoverage { enabled, output?, folder? }`: 数えるのを始める（読み込み済みのモジュールにも置く）か、止める。
    pub fn set_coverage(&mut self, arguments: &Value) -> Result<Value, String> {
        let enabled = arguments.get("enabled").and_then(Value::as_bool).ok_or_else(|| tr!("netdbg.setCoverage has no enabled."))?;
        if self.is_dump() {
            return Err(tr!("Coverage cannot be collected from a crash dump (no code runs)."));
        }
        if let Some(output) = arguments.get("output").and_then(Value::as_str).filter(|p| !p.is_empty()) {
            self.coverage.output = Some(PathBuf::from(output));
        }
        if let Some(folder) = coverage_folder(arguments.get("folder"))? {
            self.coverage.folder = Some(folder);
        }
        let was = self.coverage.enabled;
        self.coverage.enabled = enabled;
        if enabled && !was && self.process.is_some() {
            let running = self.process.clone().filter(|_| self.state == State::Running);
            if let Some(process) = &running {
                unsafe { process.Stop(0) };
            }
            for index in 0..self.modules.len() {
                let events = self.cover_module(index);
                self.deferred.extend(events);
            }
            self.enter_running_methods();
            if let Some(process) = &running {
                unsafe { process.Continue(0) };
            }
            self.start_coverage_ticker();
        }
        if !enabled && was {
            self.coverage.ticker = None;
            let sites: Vec<Site> = self.coverage.sites.drain().map(|(_, site)| site).collect();
            self.while_synchronized(|| sites.iter().for_each(Site::deactivate));
        }
        Ok(self.coverage_summary())
    }

    /// 途中から数え始めたとき: いまスタックにあるメソッドは、入口にもう当たらない。残りのブレークポイントを、その場で
    /// 置く（これから先に通る行と分かれ道は数える）。
    ///
    /// 印は、確かに実行した所にだけ付ける。下の段のフレームは呼び出しの途中なので、いまの位置を含む文は実行中で、
    /// 通ったことにする。いちばん上のフレームが止まっている文は、まだ実行していないので付けない（止まっている命令の上の
    /// ブレークポイントは続行しても当たらないので、その文の 1 回目は数えない）。数え始める前に通った所は数えない。
    fn enter_running_methods(&mut self) {
        for thread in self.threads.clone() {
            for (depth, frame) in self.walk_frames(thread).unwrap_or_default().into_iter().enumerate() {
                let Ok(function) = (unsafe { take::<ICorDebugFunction>("GetFunction", |out| frame.GetFunction(out)) }) else { continue };
                let Ok(module) = (unsafe { take::<ICorDebugModule>("GetModule", |out| function.GetModule(out)) }) else { continue };
                let mut token = 0u32;
                unsafe { function.GetToken(&mut token) };
                let Some(key) = self.site_at(&(module_path(&module), token, 0)) else { continue };
                self.enter_without_marking(key, &function);
                if depth > 0 {
                    if let Some(ip) = frame.cast::<ICorDebugILFrame>().ok().and_then(|il| unsafe { il_offset(&il) }) {
                        self.coverage.data.modules[key.module].methods[key.method].mark_running(ip);
                    }
                }
            }
        }
    }

    /// 入口に当たったときと同じく残りを置くが、入口の行（IL 0）に印は付けない（途中から数え始めたとき）。
    fn enter_without_marking(&mut self, key: SiteKey, function: &ComPtr<ICorDebugFunction>) {
        let Some(site) = self.coverage.sites.remove(&key) else { return };
        site.deactivate();
        let mapped = unsafe { self.mappable(key, function) };
        let method = &mut self.coverage.data.modules[key.module].methods[key.method];
        let before = method.hit.clone();
        let placements = method.enter(&|offset| mapped.binary_search(&offset).is_ok());
        // `enter` は IL 0 の系列点に印を付ける（入口に当たったとき用）。ここでは付けない。
        method.hit = before;
        let mut placements = placements;
        // 入口の系列点は、もう一度呼ばれたときに数えられるように、入口に置き直す。
        for (index, point) in method.points.iter().enumerate() {
            if point.il_offset == 0 && !method.hit[index] {
                placements.push((0, Role::Point(index)));
            }
        }
        self.place_sites(SiteKey { offset: 0, ..key }, function, placements);
    }

    /// 読み込んだモジュールを数え始める（数えているときだけ）。入口のブレークポイントを置く。PDB の無いアプリの
    /// モジュール（D-060）は、ここで逆コンパイルし、そのソースの上で数える（逆コンパイルの知らせを返す）。
    pub(super) fn cover_module(&mut self, index: usize) -> Vec<Event> {
        if !self.coverage.enabled || self.is_dump() {
            return Vec::new();
        }
        let started = std::time::Instant::now();
        let key = coverage::normalize(&self.modules[index].path);
        // 同じモジュールの 2 つめの AppDomain（.NET Framework）は数えない（ブレークポイントは AppDomain ごとに要る）。
        if self.coverage.modules.contains_key(&key) {
            return Vec::new();
        }
        let decompiled = self.ensure_decompiled(index);
        let module = &self.modules[index];
        if !is_covered_module(module) {
            return decompiled;
        }
        let Some(pdb) = module.pdb.as_ref() else { return decompiled };
        let image = std::fs::read(&module.path).ok().or_else(|| unsafe { module_image(&module.module) });
        let (Some(image), Some(metadata)) = (image, module.metadata()) else { return decompiled };
        let built = coverage::build(&module.path, metadata, &image, pdb);
        let analyzed = started.elapsed();
        let data_index = self.coverage.data.modules.len();
        let mut placed = 0;
        for (method_index, method) in built.methods.iter().enumerate() {
            match unsafe { break_at(&module.module, method.token, 0) } {
                Ok(breakpoint) => {
                    let site = SiteKey { module: data_index, method: method_index, offset: 0, returned: false };
                    self.coverage.by_breakpoint.insert(breakpoint.as_raw() as usize, site);
                    self.coverage.sites.insert(site, Site { breakpoints: vec![breakpoint], roles: vec![Role::Entry] });
                    placed += 1;
                }
                Err(e) => crate::log!("coverage: entry of 0x{:08X} in {}: {e}", method.token, module.path),
            }
        }
        self.coverage.methods.push(built.methods.iter().enumerate().map(|(i, m)| (m.token, i)).collect());
        self.coverage.modules.insert(key, data_index);
        let decisions: usize = built.methods.iter().map(|m| m.decisions.len()).sum();
        crate::log!(
            "coverage: {} methods ({placed} entries), {decisions} decisions in {:.1} ms (analysis {:.1} ms): {}",
            built.methods.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            analyzed.as_secs_f64() * 1000.0,
            module.path
        );
        // まだ通っていない行も、画面に「通っていない」として出す。
        self.coverage.changed.extend(built.documents.iter().cloned());
        self.coverage.dirty.store(true, Ordering::SeqCst);
        self.coverage.data.modules.push(built);
        decompiled
    }

    /// ブレークポイントに当たった（`breakpoint_hit` の頭で呼ぶ）。カバレッジのものなら印を付ける。ほかのものでも、
    /// 同じ場所にカバレッジの場所があれば印を付ける（ランタイムは、同じ場所のブレークポイントを一つしか知らせない）。
    pub(super) fn coverage_hit(&mut self, thread: u32, breakpoint: &ComPtr<ICorDebugBreakpoint>) -> CoverageHit {
        let raw = breakpoint.as_raw() as usize;
        let Some(&key) = self.coverage.by_breakpoint.get(&raw) else {
            // ほかのブレークポイント: 同じ場所のカバレッジの場所に、印を付ける。
            if !self.coverage.sites.is_empty() {
                if let Ok(other) = breakpoint.cast::<ICorDebugFunctionBreakpoint>() {
                    // 同じ場所のブレークポイントなので、メソッド（入口なら残りを置く先）も同じ。
                    if let Some(key) = unsafe { location_of(&other) }.and_then(|l| self.site_at(&l)) {
                        let function = unsafe { function_of(&other) };
                        self.reach_site(thread, key, function.as_ref());
                    }
                }
            }
            return CoverageHit::NotCoverage;
        };
        let function_breakpoint = breakpoint.cast::<ICorDebugFunctionBreakpoint>().ok();
        let function = function_breakpoint.as_ref().and_then(|b| unsafe { function_of(b) });
        self.reach_site(thread, key, function.as_ref());
        // 同じ場所の、エンジンのほかのブレークポイント（入口、利用者、プローブ、ステップの内部のもの）。
        match function_breakpoint.as_ref().and_then(|b| self.other_breakpoint_at(b)) {
            Some(other) => CoverageHit::Shared(other),
            None => CoverageHit::Only,
        }
    }

    /// 場所（モジュールのパス、メソッドのトークン、IL オフセット）にある、カバレッジの場所。
    fn site_at(&self, (path, token, offset): &(String, u32, u32)) -> Option<SiteKey> {
        let module = *self.coverage.modules.get(&coverage::normalize(path))?;
        let method = *self.coverage.methods.get(module)?.get(token)?;
        let key = SiteKey { module, method, offset: *offset, returned: false };
        self.coverage.sites.contains_key(&key).then_some(key)
    }

    /// 場所 `key` の役目を果たす。入口なら残りを置く。終えた役目は外し、役目が無くなったらブレークポイントを外す。
    fn reach_site(&mut self, thread: u32, key: SiteKey, function: Option<&ComPtr<ICorDebugFunction>>) {
        let Some(roles) = self.coverage.sites.get(&key).map(|site| site.roles.clone()) else { return };
        let mut remaining = Vec::new();
        let mut placements = Vec::new();
        for role in roles {
            let destination = match role {
                Role::Read(decision) => self.coverage.data.modules[key.module].methods[key.method].decisions.get(decision).and_then(|d| d.read.clone()).and_then(|read| self.read_destination(thread, &read)),
                _ => None,
            };
            if role == Role::Entry {
                // JIT は済んでいる。IL とネイティブのコードの対応を見て、見分け方を決める。
                let mapped = function.map(|f| unsafe { self.mappable(key, f) }).unwrap_or_default();
                let method = &mut self.coverage.data.modules[key.module].methods[key.method];
                placements = method.enter(&|offset| mapped.binary_search(&offset).is_ok());
            }
            let method = &mut self.coverage.data.modules[key.module].methods[key.method];
            if !method.reach(role, destination) {
                remaining.push(role);
            }
        }
        let documents: BTreeSet<String> = {
            let module = &self.coverage.data.modules[key.module];
            module.methods[key.method].points.iter().filter_map(|p| p.document.checked_sub(1).and_then(|i| module.documents.get(i as usize)).cloned()).collect()
        };
        self.coverage.changed.extend(documents);
        self.coverage.dirty.store(true, Ordering::SeqCst);
        if remaining.is_empty() {
            if let Some(site) = self.coverage.sites.remove(&key) {
                site.deactivate();
            }
        } else if let Some(site) = self.coverage.sites.get_mut(&key) {
            site.roles = remaining;
        }
        if !placements.is_empty() {
            if let Some(function) = function {
                self.place_sites(key, function, placements);
            }
        }
    }

    /// 入口に当たったメソッドに、残りのブレークポイントを置く。同じ場所の役目は 1 つのブレークポイントにまとめる。
    fn place_sites(&mut self, entry: SiteKey, function: &ComPtr<ICorDebugFunction>, placements: Vec<(u32, Role)>) {
        let Ok(code) = (unsafe { take::<ICorDebugCode>("GetILCode", |out| function.GetILCode(out)) }) else { return };
        for (offset, role) in placements {
            // 呼び出しの戻り値を読む分かれ道は、戻った直後の機械語の位置に置く（IL の位置では、戻り値はもう読めない）。
            let returned_call = match role {
                Role::Read(decision) => self.returned_call(entry, decision),
                _ => None,
            };
            let key = SiteKey { offset, returned: returned_call.is_some(), ..entry };
            if let Some(site) = self.coverage.sites.get_mut(&key) {
                if !site.roles.contains(&role) {
                    site.roles.push(role);
                }
                continue;
            }
            let breakpoints = match returned_call {
                Some(call) => unsafe { break_after_call(function, call) },
                None => unsafe { take::<ICorDebugFunctionBreakpoint>("CreateBreakpoint", |out| code.CreateBreakpoint(offset, out)) }
                    .and_then(|b| unsafe { b.Activate(1) }.ok().map(|()| b).map_err(|hr| failed("Activate", hr)))
                    .map(|b| vec![b])
                    .unwrap_or_else(|e| {
                        crate::log!("coverage: IL 0x{offset:X}: {e}");
                        Vec::new()
                    }),
            };
            if breakpoints.is_empty() {
                continue;
            }
            for breakpoint in &breakpoints {
                self.coverage.by_breakpoint.insert(breakpoint.as_raw() as usize, key);
            }
            self.coverage.sites.insert(key, Site { breakpoints, roles: vec![role] });
        }
    }

    /// 分かれ道の条件が呼び出しの戻り値なら、その呼び出しの IL オフセット。
    fn returned_call(&self, key: SiteKey, decision: usize) -> Option<u32> {
        let decision = self.coverage.data.modules[key.module].methods[key.method].decisions.get(decision)?;
        match decision.read.as_ref()?.loads.first()? {
            Load::Returned(call) => Some(*call),
            _ => None,
        }
    }

    /// 止めて正確に読める IL オフセット（昇順）: ネイティブのコードに対応のある所と、戻り値を読む分かれ道のうち、
    /// 戻った直後の機械語の位置が分かるものの読む所。
    unsafe fn mappable(&self, key: SiteKey, function: &ComPtr<ICorDebugFunction>) -> Vec<u32> {
        let mut offsets = native_il_offsets(function);
        let method = &self.coverage.data.modules[key.module].methods[key.method];
        for decision in &method.decisions {
            let Some(read) = &decision.read else { continue };
            if let Some(Load::Returned(call)) = read.loads.first() {
                if !return_live_offsets(function, *call).is_empty() {
                    offsets.push(read.at);
                }
            }
        }
        offsets.sort_unstable();
        offsets.dedup();
        offsets
    }

    /// 分かれ道の前で、ローカル変数と引数を読んで行き先を決める。
    fn read_destination(&self, thread: u32, read: &coverage::Read) -> Option<u32> {
        let process = self.process.as_ref()?;
        unsafe {
            let thread = take::<ICorDebugThread>("GetThread", |out| process.GetThread(thread, out)).ok()?;
            let frame = take::<ICorDebugFrame>("GetActiveFrame", |out| thread.GetActiveFrame(out)).ok()?;
            let frame = frame.cast::<ICorDebugILFrame>().ok()?;
            read.destination(&mut |load| {
                let value = match load {
                    Load::Local(index) => take::<ICorDebugValue>("GetLocalVariable", |out| frame.GetLocalVariable(index as u32, out)).ok()?,
                    Load::Arg(index) => take::<ICorDebugValue>("GetArgument", |out| frame.GetArgument(index as u32, out)).ok()?,
                    // 呼び出しから戻った所で止めているので、その呼び出しの戻り値が読める。
                    Load::Returned(call) => {
                        let frame3 = frame.cast::<ICorDebugILFrame3>().ok()?;
                        take::<ICorDebugValue>("GetReturnValueForILOffset", |out| frame3.GetReturnValueForILOffset(call, out)).ok()?
                    }
                    _ => return None,
                };
                self.comparable(&value)
            })
        }
    }

    /// 値を、分岐の判定で比べられる形にする（整数、bool、char、列挙型、参照）。浮動小数点数と構造体は `None`。
    unsafe fn comparable(&self, value: &ComPtr<ICorDebugValue>) -> Option<Val> {
        if let Some((element, bytes)) = value::primitive_bytes(value) {
            return integer(element, &bytes).map(Val::Int);
        }
        if let Some(address) = value::reference_address(value) {
            return Some(Val::Ref(address));
        }
        let (_, _, element, bytes) = value::enum_parts(value, &self.modules)?;
        integer(element, &bytes).map(Val::Int)
    }

    /// 同じ場所にある、エンジンのほかのブレークポイント（カバレッジのもの以外）。
    fn other_breakpoint_at(&self, breakpoint: &ComPtr<ICorDebugFunctionBreakpoint>) -> Option<ComPtr<ICorDebugBreakpoint>> {
        let mut candidates: Vec<&ComPtr<ICorDebugFunctionBreakpoint>> = Vec::new();
        candidates.extend(self.entry_breakpoint.iter());
        candidates.extend(self.async_step_out.iter());
        candidates.extend(self.await_breakpoints.iter().map(|b| &b.breakpoint));
        candidates.extend(self.target_step.iter().flat_map(|step| &step.breakpoints));
        candidates.extend(self.probe_breakpoints());
        candidates.extend(self.bindings().flat_map(|(_, bound)| bound));
        if candidates.is_empty() {
            return None;
        }
        let location = unsafe { location_of(breakpoint) }?;
        candidates.into_iter().find(|candidate| unsafe { location_of(candidate) }.as_ref() == Some(&location)).and_then(|found| found.cast::<ICorDebugBreakpoint>().ok())
    }

    /// 変わった行の知らせ。前の知らせから変わったファイルが無ければ空。
    pub(super) fn coverage_changed(&mut self) -> Vec<Event> {
        self.coverage.dirty.store(false, Ordering::SeqCst);
        if self.coverage.changed.is_empty() {
            return Vec::new();
        }
        let files: Vec<String> = std::mem::take(&mut self.coverage.changed).into_iter().collect();
        let mut body = self.coverage_summary();
        body["files"] = json!(files);
        vec![("netdbg.coverageChanged", body)]
    }

    /// いまのセッションのカバレッジの記録（差分の再試験の範囲に使う。diff.rs）。
    pub(super) fn coverage_data(&self) -> &Coverage {
        &self.coverage.data
    }

    /// 時のスレッドからの知らせ（`Input::CoverageTick`）。
    pub fn coverage_tick(&mut self) -> Vec<Event> {
        self.coverage_changed()
    }

    /// 対象の起動とアタッチのあと（プロセスができたとき）に、時のスレッドを始める。
    pub(super) fn start_coverage_ticker(&mut self) {
        if !self.coverage.enabled || self.coverage.ticker.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let dirty = self.coverage.dirty.clone();
        let stopping = stop.clone();
        let spawned = std::thread::Builder::new().name("coverage-ticker".into()).spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                std::thread::sleep(NOTIFY_INTERVAL);
                if dirty.swap(false, Ordering::SeqCst) {
                    crate::events::post(crate::events::Input::CoverageTick);
                }
            }
        });
        match spawned {
            Ok(_) => self.coverage.ticker = Some(Ticker { stop }),
            Err(e) => crate::log!("coverage: ticker: {e}"),
        }
    }

    /// 要約（行と分岐の数、率、数えられない分かれ道の数、書き出したファイル）。
    fn coverage_summary(&self) -> Value {
        let totals = self.coverage.data.totals();
        let mut body = json!({
            "enabled": self.coverage.enabled,
            "linesCovered": totals.lines_covered,
            "linesValid": totals.lines_valid,
            "lineRate": totals.line_rate(),
            "branchesCovered": totals.branches_covered,
            "branchesValid": totals.branches_valid,
            "branchRate": totals.branch_rate(),
            "uncountableBranches": totals.uncountable,
        });
        if let Some(path) = &self.coverage.written {
            body["path"] = json!(path.to_string_lossy());
        }
        body
    }

    /// `netdbg.coverage { write?, folder?, file? }`: 要約。`file` なら、そのファイルの行ごとの状態。`write` なら、いまの分を
    /// 書く（`folder` があれば、その下に。以後の書き出しも）。
    pub fn coverage_request(&mut self, arguments: &Value) -> Result<Value, String> {
        if let Some(folder) = coverage_folder(arguments.get("folder"))? {
            self.coverage.folder = Some(folder);
        }
        if arguments.get("write").and_then(Value::as_bool) == Some(true) {
            self.write_coverage()?;
        }
        let mut body = self.coverage_summary();
        if let Some(file) = arguments.get("file").and_then(Value::as_str) {
            body["file"] = json!(file);
            body["lines"] = json!(self.coverage.data.file(file).into_iter().map(|(line, state)| line_json(line, &state)).collect::<Vec<_>>());
        }
        if arguments.get("files").and_then(Value::as_bool) == Some(true) {
            body["files"] = json!(self
                .coverage
                .data
                .files()
                .into_iter()
                .map(|(path, lines)| {
                    let covered = lines.values().filter(|l| l.hit).count();
                    json!({ "path": path, "linesCovered": covered, "linesValid": lines.len() })
                })
                .collect::<Vec<_>>());
        }
        Ok(body)
    }

    /// いまの分を Cobertura XML に書く。書いた先を返す。
    fn write_coverage(&mut self) -> Result<PathBuf, String> {
        if self.coverage.data.modules.is_empty() {
            return Err(tr!("There is no coverage record. Launch with coverage: true, or start it with netdbg.setCoverage."));
        }
        let path = match &self.coverage.output {
            Some(path) => path.clone(),
            None => default_output(self.coverage.folder.as_deref(), self.config.as_ref().map(|c| c.program.as_str())),
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| tr!("Cannot create {folder}: {error}", folder = parent.display(), error = e))?;
        }
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        std::fs::write(&path, self.coverage.data.to_cobertura(timestamp)).map_err(|e| tr!("Cannot write the coverage to {path}: {error}", path = path.display(), error = e))?;
        crate::log!("coverage written: {}", path.display());
        self.coverage.written = Some(path.clone());
        Ok(path)
    }

    /// 対象が終わるか切り離すとき: 残りの知らせを送り、書き出して、書いた先を知らせる。
    pub(super) fn finish_coverage(&mut self) -> Vec<Event> {
        self.coverage.ticker = None;
        if self.coverage.data.modules.is_empty() {
            return Vec::new();
        }
        let mut out = self.coverage_changed();
        match self.write_coverage() {
            Ok(path) => {
                let summary = self.coverage_summary();
                let totals = self.coverage.data.totals();
                let text = tr!(
                    "Coverage: lines {lines}/{all_lines} ({line_rate}%), branches {branches}/{all_branches} ({branch_rate}%), written to {path}.\n",
                    lines = totals.lines_covered,
                    all_lines = totals.lines_valid,
                    line_rate = format!("{:.0}", totals.line_rate() * 100.0),
                    branches = totals.branches_covered,
                    all_branches = totals.branches_valid,
                    branch_rate = format!("{:.0}", totals.branch_rate() * 100.0),
                    path = path.display()
                );
                out.push(("netdbg.coverage", summary));
                out.push(("output", json!({ "category": "console", "output": text })));
            }
            Err(e) => out.push(("output", json!({ "category": "stderr", "output": format!("{e}\n") }))),
        }
        // 書いたあとは、外した印の知らせが届いても数えない（記録は次の起動まで読める）。
        self.coverage.sites.clear();
        self.coverage.by_breakpoint.clear();
        out
    }
}

/// 数えるモジュールか: PDB（本物か、アプリのモジュールを逆コンパイルして作ったもの）を持ち、ランタイムの部品ではない。
fn is_covered_module(module: &Module) -> bool {
    let name = Path::new(&module.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    module.pdb.is_some() && !decompiled::is_platform_assembly(&name) && !module.path.to_lowercase().contains("\\dotnet\\shared\\")
}

/// メソッドのネイティブのコードに対応のある IL オフセット（昇順）。そこに置いたブレークポイントは、その IL の位置で止まる。
unsafe fn native_il_offsets(function: &ComPtr<ICorDebugFunction>) -> Vec<u32> {
    // 特別な値（NO_MAPPING、PROLOG、EPILOG）は 0xFFFFFFFD 以上。
    const SPECIAL: u32 = 0xFFFF_FFFD;
    let Ok(code) = take::<ICorDebugCode>("GetNativeCode", |out| function.GetNativeCode(out)) else { return Vec::new() };
    let mut count = 0u32;
    if !code.GetILToNativeMapping(0, &mut count, null_mut()).is_ok() || count == 0 {
        return Vec::new();
    }
    let mut map = vec![COR_DEBUG_IL_TO_NATIVE_MAP { ilOffset: 0, nativeStartOffset: 0, nativeEndOffset: 0 }; count as usize];
    let mut fetched = 0u32;
    if !code.GetILToNativeMapping(count, &mut fetched, map.as_mut_ptr()).is_ok() {
        return Vec::new();
    }
    let mut offsets: Vec<u32> = map[..fetched.min(count) as usize].iter().map(|m| m.ilOffset).filter(|&o| o < SPECIAL).collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

/// IL オフセット `call` の呼び出しから戻った直後の、戻り値がまだ残っている機械語の位置（値を返さない呼び出しは空）。
unsafe fn return_live_offsets(function: &ComPtr<ICorDebugFunction>, call: u32) -> Vec<u32> {
    let Ok(code) = take::<ICorDebugCode>("GetNativeCode", |out| function.GetNativeCode(out)) else { return Vec::new() };
    let Ok(code3) = code.cast::<ICorDebugCode3>() else { return Vec::new() };
    let mut offsets = [0u32; 8];
    let mut fetched = 0u32;
    if !code3.GetReturnValueLiveOffset(call, offsets.len() as u32, &mut fetched, offsets.as_mut_ptr()).is_ok() {
        return Vec::new();
    }
    offsets[..(fetched as usize).min(offsets.len())].to_vec()
}

/// 呼び出しから戻った直後の機械語の位置に、ブレークポイントを置く（ステップの戻り値の表示と同じ置き方。stepping.rs）。
unsafe fn break_after_call(function: &ComPtr<ICorDebugFunction>, call: u32) -> Vec<ComPtr<ICorDebugFunctionBreakpoint>> {
    let Ok(code) = take::<ICorDebugCode>("GetNativeCode", |out| function.GetNativeCode(out)) else { return Vec::new() };
    return_live_offsets(function, call)
        .into_iter()
        .filter_map(|offset| {
            let breakpoint = take::<ICorDebugFunctionBreakpoint>("CreateBreakpoint", |out| code.CreateBreakpoint(offset, out)).ok()?;
            breakpoint.Activate(1).is_ok().then_some(breakpoint)
        })
        .collect()
}

/// フレームのいまの IL の位置。
unsafe fn il_offset(frame: &ComPtr<ICorDebugILFrame>) -> Option<u32> {
    let (mut offset, mut mapping) = (0u32, 0);
    frame.GetIP(&mut offset, &mut mapping).ok().ok()?;
    Some(offset)
}

/// ブレークポイントを置いたメソッド。
unsafe fn function_of(breakpoint: &ComPtr<ICorDebugFunctionBreakpoint>) -> Option<ComPtr<ICorDebugFunction>> {
    take::<ICorDebugFunction>("GetFunction", |out| breakpoint.GetFunction(out)).ok()
}

/// プリミティブ型の値を、評価スタックの整数の形にする（IL の読み込みと同じく、32 ビット以下の符号つきは符号を広げ、
/// 符号なしの 8 / 16 ビットと bool、char は 0 で広げる）。
fn integer(element: u32, bytes: &[u8]) -> Option<i64> {
    let mut buffer = [0u8; 8];
    let length = bytes.len().min(8);
    buffer[..length].copy_from_slice(&bytes[..length]);
    let raw = u64::from_le_bytes(buffer);
    Some(match (element, length) {
        // BOOLEAN、CHAR、U1、U2
        (0x02 | 0x03 | 0x05 | 0x07, _) => raw as i64,
        (0x04, _) => raw as u8 as i8 as i64,
        (0x06, _) => raw as u16 as i16 as i64,
        // I4、U4（評価スタックでは int32）、I / U の 32 ビット
        (0x08 | 0x09, _) | (0x18 | 0x19, 4) => raw as u32 as i32 as i64,
        (0x0A | 0x0B | 0x18 | 0x19, _) => raw as i64,
        _ => return None,
    })
}

/// 既定の書き出す先: `<保存先>\<プログラム名>-<日時>.cobertura.xml`。保存先の既定は
/// `%LOCALAPPDATA%\netdbg\coverage`（アプリのフォルダー。Program Files のこともある。は汚さない）。
fn default_output(folder: Option<&Path>, program: Option<&str>) -> PathBuf {
    let base = folder.map_or_else(|| crate::folder::app_data("coverage"), Path::to_path_buf);
    let name = program.and_then(|p| Path::new(p).file_stem()).map_or_else(|| "coverage".to_string(), |s| s.to_string_lossy().into_owned());
    let mut time = [0u16; 8];
    unsafe { win32::GetLocalTime(&mut time) };
    let stamp = format!("{:04}{:02}{:02}-{:02}{:02}{:02}", time[0], time[1], time[3], time[4], time[5], time[6]);
    base.join(format!("{name}-{stamp}.cobertura.xml"))
}

/// カバレッジの保存先の引数（`folder`）。
fn coverage_folder(value: Option<&Value>) -> Result<Option<PathBuf>, String> {
    crate::folder::argument(value, &tr!("the coverage save folder (folder)"))
}

/// 行 1 つの状態（`netdbg.coverage { file }` の答え）。
fn line_json(line: u32, state: &coverage::LineState) -> Value {
    let (covered, valid, uncountable) = state.branches();
    let mut body = json!({ "line": line, "hit": state.hit });
    if !state.decisions.is_empty() {
        body["branchesCovered"] = json!(covered);
        body["branchesValid"] = json!(valid);
        if uncountable > 0 {
            body["uncountableBranches"] = json!(uncountable);
        }
        body["decisions"] = json!(state
            .decisions
            .iter()
            .map(|d| json!({
                "offset": d.offset,
                "kind": match d.kind { coverage::DecisionKind::Jump => "jump", coverage::DecisionKind::Switch => "switch" },
                "countable": d.countable(),
                "outcomes": d.outcomes.iter().map(|o| {
                    let mut outcome = json!({ "seen": o.seen });
                    if let Some(value) = o.value {
                        outcome["value"] = json!(value);
                    }
                    if d.kind == coverage::DecisionKind::Switch {
                        outcome["cases"] = json!(o.cases);
                    }
                    outcome
                }).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>());
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_widened_like_the_evaluation_stack() {
        assert_eq!(integer(0x02, &[1]), Some(1));
        assert_eq!(integer(0x04, &[0xFF]), Some(-1), "sbyte は符号を広げる");
        assert_eq!(integer(0x05, &[0xFF]), Some(255), "byte は 0 で広げる");
        assert_eq!(integer(0x09, &[0xFF, 0xFF, 0xFF, 0xFF]), Some(-1), "uint も評価スタックでは int32");
        assert_eq!(integer(0x0B, &[0xFF; 8]), Some(-1));
        assert_eq!(integer(0x18, &[0xFE, 0xFF, 0xFF, 0xFF]), Some(-2), "32 ビットの IntPtr");
        assert_eq!(integer(0x0C, &[0; 4]), None, "float は比べない");
    }
}
