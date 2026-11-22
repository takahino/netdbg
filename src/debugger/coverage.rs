//! デバッグしているアプリの行と分岐のカバレッジ（B1、D-079）。ICorDebug を知らない部分。
//!
//! 数える単位:
//! - 行: PDB の見える系列点。系列点が始まる行は、その系列点を通ったときだけ通ったことにする。始まりより後の行は、
//!   その行から始まる系列点がどこにも無ければ、覆っている系列点を通れば通ったことにする（複数行の 1 文。coverlet と同じ
//!   範囲）。ラムダの本体は別のメソッドの系列点から始まるので、外側の `Click +=` を実行しただけでは通ったことにしない。
//!   回数は数えない（通ったか、通らなかったか）。
//! - 分かれ道: 見える系列点の範囲にある条件分岐（`brtrue`、`beq` など）と `switch`。向き（行き先）ごとに通ったかを見る
//!   （coverlet と同じ単位）。向きの見分け方は IL から決める（`decisions`）。見分けられない分かれ道は数えない。
//!
//! 集め方（どこにブレークポイントを置き、当たったら何を付けるか）はセッションの側（`session/coverage.rs`）。ここは、
//! 置く場所と役目（`Role`）を決め、当たった印を付け、要約と Cobertura XML を作る。

use crate::debugger::decompile::{self, names::Names};
use crate::debugger::il::{Flow, Instruction, Value as Operand};
use crate::debugger::metadata::{Metadata, FIELD, MEMBER_REF, TYPE_DEF};
use crate::debugger::pdb::{Pdb, SequencePoint};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// 条件を読む分かれ道で、新しい向きを見ないまま当たってよい回数。これを超えたら読むのをやめる（熱いループで止まり続けないため）。
pub const READ_LIMIT: u32 = 1000;

/// 見える系列点 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub il_offset: u32,
    /// PDB の Document の行番号（1 始まり）。
    pub document: u32,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    /// 2 つの向きの条件分岐。
    Jump,
    Switch,
}

/// 向きの見分け方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// まだ決めていない（メソッドがまだ呼ばれていない。JIT のあとで、ネイティブのコードの対応を見て決める）。
    Pending,
    /// 行き先に一度きりのブレークポイントを置く。
    AtTarget,
    /// 分岐の前（スタックが空の所）で、ローカル変数と引数を読んで向きを決める。
    ByReading,
    /// 見分けられない。この分かれ道は数えない。
    Unknown,
}

/// 分かれ道の向き 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// 行き先の IL オフセット。
    pub target: u32,
    /// 2 つの向きの分岐で、この向きに進むときの条件の値（`brtrue` の分岐する側なら真）。`switch` では `None`。
    pub value: Option<bool>,
    /// `switch` で、この向きに進む case の番号（空なら、どれにも当たらなかった側）。
    pub cases: Vec<u32>,
    /// 行き先に入る道がこの向きだけで、スタックが空（行き先に置けるかもしれない）。
    pub at_target: bool,
    pub how: How,
    pub seen: bool,
}

/// 分かれ道 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// 分岐の命令の IL オフセット。
    pub offset: u32,
    pub document: u32,
    /// 分岐が属する文の、始まりの行。
    pub line: u32,
    pub kind: DecisionKind,
    pub outcomes: Vec<Outcome>,
    /// 分岐の前で条件を読めるなら、その読み方。
    pub read: Option<Read>,
    /// 新しい向きを見ないまま、条件を読んだ回数。
    pub stale_reads: u32,
}

impl Decision {
    /// 数える分かれ道か（どの向きも見分けられる、または、まだ決めていないが見分けられる見込みがある）。
    pub fn countable(&self) -> bool {
        self.outcomes.iter().all(|o| match o.how {
            How::Pending => o.at_target || self.read.is_some(),
            How::AtTarget | How::ByReading => true,
            How::Unknown => false,
        })
    }

    /// 条件を読む向きで、まだ見ていないものがあるか。
    fn reading_needed(&self) -> bool {
        self.stale_reads < READ_LIMIT && self.outcomes.iter().any(|o| o.how == How::ByReading && !o.seen)
    }
}

/// 分岐の前の、副作用の無い読み取り（評価スタックに積むもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    Local(u16),
    Arg(u16),
    Int(i64),
    Null,
    /// `ceq` など。2 つ取り出して、比べた結果（0 か 1）を積む。
    Compare(Compare),
    /// いちばん上の値を、もう 1 つ積む（`?.` と `??` の `dup`）。
    Dup,
    /// その IL オフセットの呼び出しの戻り値（呼び出しから戻った所で読む）。
    Returned(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Gt { unsigned: bool },
    Lt { unsigned: bool },
}

/// 分岐の判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    True,
    False,
    Eq,
    NotEqual,
    Ge { unsigned: bool },
    Gt { unsigned: bool },
    Le { unsigned: bool },
    Lt { unsigned: bool },
    /// 分岐先（case の番号の順）。
    Switch(Vec<u32>),
}

/// 条件の読み方: `at`（スタックが空の所）で止め、`loads` を積んで、`test` で行き先を決める。
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub at: u32,
    pub loads: Vec<Load>,
    pub test: Test,
    /// 条件が成り立つときの行き先（`switch` では使わない）。
    pub taken: u32,
    /// 成り立たないときの行き先（次の命令）。
    pub next: u32,
}

/// 読んだ値。整数は符号を広げた 64 ビット、参照は番地（null は 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Val {
    Int(i64),
    Ref(u64),
}

impl Val {
    fn signed(self) -> i64 {
        match self {
            Val::Int(v) => v,
            Val::Ref(address) => address as i64,
        }
    }

    fn unsigned(self) -> u64 {
        match self {
            Val::Int(v) => v as u64,
            Val::Ref(address) => address,
        }
    }
}

impl Read {
    /// ローカル変数と引数を `read` で読み、行き先の IL オフセットを返す。読めない値があれば `None`。
    pub fn destination(&self, read: &mut dyn FnMut(Load) -> Option<Val>) -> Option<u32> {
        let mut stack: Vec<Val> = Vec::with_capacity(self.loads.len());
        for &load in &self.loads {
            let value = match load {
                Load::Int(v) => Val::Int(v),
                Load::Null => Val::Ref(0),
                Load::Local(_) | Load::Arg(_) | Load::Returned(_) => read(load)?,
                Load::Dup => *stack.last()?,
                Load::Compare(compare) => {
                    let (b, a) = (stack.pop()?, stack.pop()?);
                    let holds = match compare {
                        Compare::Eq => a.unsigned() == b.unsigned(),
                        Compare::Gt { unsigned: false } => a.signed() > b.signed(),
                        Compare::Gt { unsigned: true } => a.unsigned() > b.unsigned(),
                        Compare::Lt { unsigned: false } => a.signed() < b.signed(),
                        Compare::Lt { unsigned: true } => a.unsigned() < b.unsigned(),
                    };
                    Val::Int(holds as i64)
                }
            };
            stack.push(value);
        }
        let holds = match &self.test {
            Test::True => stack.pop()?.unsigned() != 0,
            Test::False => stack.pop()?.unsigned() == 0,
            Test::Switch(targets) => {
                let index = stack.pop()?.unsigned() as u32;
                return Some(targets.get(index as usize).copied().unwrap_or(self.next));
            }
            test => {
                let (b, a) = (stack.pop()?, stack.pop()?);
                match *test {
                    Test::Eq => a.unsigned() == b.unsigned(),
                    Test::NotEqual => a.unsigned() != b.unsigned(),
                    Test::Ge { unsigned: false } => a.signed() >= b.signed(),
                    Test::Ge { unsigned: true } => a.unsigned() >= b.unsigned(),
                    Test::Gt { unsigned: false } => a.signed() > b.signed(),
                    Test::Gt { unsigned: true } => a.unsigned() > b.unsigned(),
                    Test::Le { unsigned: false } => a.signed() <= b.signed(),
                    Test::Le { unsigned: true } => a.unsigned() <= b.unsigned(),
                    Test::Lt { unsigned: false } => a.signed() < b.signed(),
                    Test::Lt { unsigned: true } => a.unsigned() < b.unsigned(),
                    _ => return None,
                }
            }
        };
        Some(if holds { self.taken } else { self.next })
    }
}

/// ブレークポイントの役目（同じ場所に、いくつも重なる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// メソッドの入口（IL 0）。当たったら、残りを置く。
    Entry,
    /// 見える系列点（`points` の添字）。
    Point(usize),
    /// 分かれ道の向きの行き先（`decisions` と `outcomes` の添字）。
    Target { decision: usize, outcome: usize },
    /// 分かれ道の前で、条件を読む（`decisions` の添字）。
    Read(usize),
}

/// メソッド 1 つの記録。
#[derive(Debug, Clone)]
pub struct MethodCoverage {
    pub token: u32,
    /// 利用者の書いた型（コンパイラが作った入れ子の型は、外側の型に戻す）。
    pub class: String,
    /// 利用者の書いたメソッド（async の本体、ラムダ、ローカル関数は、それを書いたメソッド）。
    pub name: String,
    pub signature: String,
    pub points: Vec<Point>,
    pub hit: Vec<bool>,
    pub decisions: Vec<Decision>,
    /// 入口に当たった（残りを置いた）。
    pub entered: bool,
}

impl MethodCoverage {
    pub fn new(token: u32, class: String, name: String, signature: String, points: Vec<Point>, decisions: Vec<Decision>) -> MethodCoverage {
        let hit = vec![false; points.len()];
        MethodCoverage { token, class, name, signature, points, hit, decisions, entered: false }
    }

    /// 入口に当たったとき: 向きの見分け方を決め（`mapped` は、その IL オフセットがネイティブのコードに正確に対応するか）、
    /// 残りの置く場所と役目を返す。入口と同じ所（IL 0）の系列点は、ここで通ったことにする。
    pub fn enter(&mut self, mapped: &dyn Fn(u32) -> bool) -> Vec<(u32, Role)> {
        self.entered = true;
        let mut sites = Vec::new();
        for (index, point) in self.points.iter().enumerate() {
            if point.il_offset == 0 {
                self.hit[index] = true;
            } else if !self.hit[index] {
                sites.push((point.il_offset, Role::Point(index)));
            }
        }
        for (d, decision) in self.decisions.iter_mut().enumerate() {
            let readable = decision.read.as_ref().is_some_and(|read| mapped(read.at));
            for outcome in decision.outcomes.iter_mut().filter(|o| o.how == How::Pending) {
                outcome.how = if outcome.at_target && mapped(outcome.target) {
                    How::AtTarget
                } else if readable {
                    How::ByReading
                } else {
                    How::Unknown
                };
            }
            // 1 つでも見分けられない向きがあれば、その分かれ道は数えない（置いても使わない）。
            if !decision.countable() {
                continue;
            }
            for (o, outcome) in decision.outcomes.iter().enumerate() {
                if outcome.how == How::AtTarget && !outcome.seen {
                    sites.push((outcome.target, Role::Target { decision: d, outcome: o }));
                }
            }
            if decision.reading_needed() {
                sites.push((decision.read.as_ref().map_or(0, |r| r.at), Role::Read(d)));
            }
        }
        sites
    }

    /// 途中から数え始めたとき、いまこのメソッドが IL の `ip` にいる: そこを含む文（その位置以前で最後の系列点）は、
    /// これから実行するか実行中なので、通ったことにする（止まっている命令の上のブレークポイントは、続行しても当たらない）。
    pub fn mark_running(&mut self, ip: u32) {
        if let Some(index) = self.points.iter().rposition(|p| p.il_offset <= ip) {
            self.hit[index] = true;
        }
    }

    /// 役目 `role` のブレークポイントに当たった。`destination` は、条件を読んだときの行き先。役目を終えたら true
    /// （その役目のためには、もう止まらなくてよい）。
    pub fn reach(&mut self, role: Role, destination: Option<u32>) -> bool {
        match role {
            // 入口の残りは、呼び出し側が `enter` で置く。
            Role::Entry => true,
            Role::Point(index) => {
                if let Some(hit) = self.hit.get_mut(index) {
                    *hit = true;
                }
                true
            }
            Role::Target { decision, outcome } => {
                if let Some(outcome) = self.decisions.get_mut(decision).and_then(|d| d.outcomes.get_mut(outcome)) {
                    outcome.seen = true;
                }
                true
            }
            Role::Read(index) => {
                let Some(decision) = self.decisions.get_mut(index) else { return true };
                let fresh = destination.and_then(|target| decision.outcomes.iter_mut().find(|o| o.target == target)).filter(|o| !o.seen);
                match fresh {
                    Some(outcome) => {
                        outcome.seen = true;
                        decision.stale_reads = 0;
                    }
                    None => decision.stale_reads += 1,
                }
                !decision.reading_needed()
            }
        }
    }
}

/// モジュール 1 つの記録。
#[derive(Debug, Clone)]
pub struct ModuleCoverage {
    pub path: String,
    /// アセンブリの名前（Cobertura の package）。
    pub assembly: String,
    /// PDB の Document の表（添字は行番号 - 1）。
    pub documents: Vec<String>,
    pub methods: Vec<MethodCoverage>,
}

#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub modules: Vec<ModuleCoverage>,
}

/// モジュールの記録を作る。`image` は PE の像（ファイルの並び。分かれ道の解析に、メソッドの本体を読む）。
pub fn build(path: &str, md: &Metadata, image: &[u8], pdb: &Pdb) -> ModuleCoverage {
    let names = Names::new(md);
    // フィールドの名前がコンパイラの作ったもの（`<>9__0_0`、メソッド グループの `<0>__Name`）か。C# の名前は `<` で始まれない。
    let generated_field = |token: u32| {
        let row = token & 0x00FF_FFFF;
        let name = match (token >> 24) as usize {
            FIELD => md.string(md.cell(FIELD, row, 1)),
            MEMBER_REF => md.string(md.cell(MEMBER_REF, row, 1)),
            _ => "",
        };
        name.starts_with('<')
    };
    let returns_value = |token: u32| names.method_sig(token).is_some_and(|signature| !signature.ret.is_void());
    let lookup = Lookup { generated_field: &generated_field, returns_value: &returns_value };
    let assembly = std::path::Path::new(path).file_stem().map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned());
    let mut methods = Vec::new();
    for (row, all_points) in pdb.base_methods() {
        let points = visible_points(all_points);
        if points.is_empty() {
            continue;
        }
        let mut types = Vec::new();
        let mut ty = md.declaring_type(row);
        while let Some(row) = ty {
            types.push((md.string(md.cell(TYPE_DEF, row, 1)), md.type_name(row)));
            ty = md.enclosing_type(row);
        }
        let raw = md.method_simple_name(row).unwrap_or_default();
        let (class, name) = user_names(raw, &types);
        let flow = decompile::method_flow(&names, image, row);
        let decisions = flow.map(|f| decisions(&f.code, &f.depths, &f.handlers, all_points, &lookup)).unwrap_or_default();
        methods.push(MethodCoverage::new(0x0600_0000 | row, class, name, decompile::parameter_list(&names, row), points, decisions));
    }
    ModuleCoverage { path: path.to_string(), assembly, documents: pdb.documents.clone(), methods }
}

/// 分かれ道の解析が、メタデータから引くもの。
pub struct Lookup<'a> {
    /// フィールドのトークンがコンパイラの作ったもの（`<>9__0_0` などのラムダのキャッシュ）か。
    pub generated_field: &'a dyn Fn(u32) -> bool,
    /// 呼び出すメソッドのトークンが、値を返すか。
    pub returns_value: &'a dyn Fn(u32) -> bool,
}

/// 見える系列点（隠し行を除く）。
pub fn visible_points(points: &[SequencePoint]) -> Vec<Point> {
    points.iter().filter(|p| !p.is_hidden()).map(|p| Point { il_offset: p.il_offset, document: p.document, start_line: p.start_line, end_line: p.end_line.max(p.start_line) }).collect()
}

/// メソッドの分かれ道を IL から拾い、向きごとの見分け方の見込みを決める。`depths` は命令ごとのスタックの深さ、
/// `handlers` は例外のハンドラーとフィルターの頭、`points` はメソッドのすべてのシーケンスポイント（隠しも含む）。
pub fn decisions(code: &[Instruction], depths: &[Option<u32>], handlers: &[u32], points: &[SequencePoint], lookup: &Lookup) -> Vec<Decision> {
    let index_of = |offset: u32| code.binary_search_by_key(&offset, |i| i.offset).ok();
    // 命令ごとの、入ってくる道の数（前の命令から流れてくる道、分岐、leave、入口、ハンドラー）。
    let mut incoming = vec![0u32; code.len()];
    if !incoming.is_empty() {
        incoming[0] += 1;
    }
    for &handler in handlers {
        if let Some(index) = index_of(handler) {
            incoming[index] += 1;
        }
    }
    for (index, instruction) in code.iter().enumerate() {
        if depths.get(index).copied().flatten().is_none() {
            continue;
        }
        let falls_through = matches!(instruction.op.flow, Flow::Next | Flow::Prefix | Flow::Conditional | Flow::Switch);
        if falls_through && index + 1 < code.len() {
            incoming[index + 1] += 1;
        }
        let mut targets: Vec<u32> = instruction.targets().to_vec();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            if let Some(t) = index_of(target) {
                incoming[t] += 1;
            }
        }
    }

    let mut out = Vec::new();
    for (index, instruction) in code.iter().enumerate() {
        if !matches!(instruction.op.flow, Flow::Conditional | Flow::Switch) || depths.get(index).copied().flatten().is_none() {
            continue;
        }
        // 分岐が属する文（その位置以前で最後のシーケンスポイント）が見えるものだけ。隠し行はコンパイラが作ったもの
        // （async の `IsCompleted`、`using` の後始末など）。ただし Debug ビルドの `if` / `while` の条件は、値を一時変数に
        // 入れて隠し行で確かめる（`stloc V; ldloc V; brfalse`）。これは利用者の分かれ道なので、直前の見える行に付ける。
        let Some(last) = points.iter().rev().find(|p| p.il_offset <= instruction.offset) else { continue };
        let point = if !last.is_hidden() {
            last
        } else if tests_condition_temp(code, index) {
            let Some(visible) = points.iter().rev().find(|p| p.il_offset <= instruction.offset && !p.is_hidden()) else { continue };
            visible
        } else {
            continue;
        };
        if is_cache_check(code, index, lookup.generated_field) {
            continue;
        }
        let next = instruction.end();
        let Some(test) = test_of(instruction) else { continue };
        let mut outcomes: Vec<Outcome> = Vec::new();
        let mut add = |target: u32, value: Option<bool>, case: Option<u32>| {
            if let Some(existing) = outcomes.iter_mut().find(|o| o.target == target) {
                existing.cases.extend(case);
                return;
            }
            let at_target = index_of(target).is_some_and(|t| incoming[t] == 1 && depths[t] == Some(0) && !(t > 0 && code[t - 1].op.flow == Flow::Prefix));
            outcomes.push(Outcome { target, value, cases: case.into_iter().collect(), at_target, how: How::Pending, seen: false });
        };
        let kind = match &test {
            Test::Switch(targets) => {
                for (case, &target) in targets.iter().enumerate() {
                    add(target, None, Some(case as u32));
                }
                add(next, None, None);
                DecisionKind::Switch
            }
            test => {
                let taken = instruction.targets().first().copied().unwrap_or(next);
                // `brfalse` は、条件が偽のときに分岐する。
                let taken_value = *test != Test::False;
                add(taken, Some(taken_value), None);
                add(next, Some(!taken_value), None);
                DecisionKind::Jump
            }
        };
        // 行き先が 1 つしかない（次の命令への分岐）なら、分かれ道ではない。
        if outcomes.len() < 2 {
            continue;
        }
        let read = read_before(code, depths, &incoming, index, test, next, lookup);
        out.push(Decision { offset: instruction.offset, document: point.document, line: point.start_line, kind, outcomes, read, stale_reads: 0 });
    }
    out
}

/// 分岐が、直前で入れた一時変数を確かめるもの（`stloc V; ldloc V; brX`。Debug ビルドの `if` / `while` の条件）か。
fn tests_condition_temp(code: &[Instruction], branch: usize) -> bool {
    let (Some(store), Some(load)) = (branch.checked_sub(2).map(|i| &code[i]), branch.checked_sub(1).map(|i| &code[i])) else { return false };
    var_index(store, "stloc").is_some_and(|stored| var_index(load, "ldloc") == Some(stored))
}

/// コンパイラが作ったキャッシュの確かめ（`ldsfld <>9__0_0; dup; brtrue`。ラムダの委譲を一度だけ作る）か。利用者の書いた
/// 分かれ道ではないので数えない（coverlet と同じ）。
fn is_cache_check(code: &[Instruction], branch: usize, generated_field: &dyn Fn(u32) -> bool) -> bool {
    let (Some(dup), Some(load)) = (branch.checked_sub(1).map(|i| &code[i]), branch.checked_sub(2).map(|i| &code[i])) else { return false };
    dup.op.name == "dup" && matches!(load.op.name, "ldsfld" | "ldfld") && matches!(load.value, Operand::Token(token) if generated_field(token))
}

/// 分岐の命令の判定。
fn test_of(instruction: &Instruction) -> Option<Test> {
    let name = instruction.op.name.trim_end_matches(".s");
    Some(match name {
        "brtrue" => Test::True,
        "brfalse" => Test::False,
        "beq" => Test::Eq,
        "bne.un" => Test::NotEqual,
        "bge" => Test::Ge { unsigned: false },
        "bge.un" => Test::Ge { unsigned: true },
        "bgt" => Test::Gt { unsigned: false },
        "bgt.un" => Test::Gt { unsigned: true },
        "ble" => Test::Le { unsigned: false },
        "ble.un" => Test::Le { unsigned: true },
        "blt" => Test::Lt { unsigned: false },
        "blt.un" => Test::Lt { unsigned: true },
        "switch" => Test::Switch(instruction.targets().to_vec()),
        _ => return None,
    })
}

/// 副作用の無い読み取りの命令なら、その中身（`nop` は `Some(None)`）。
fn load_of(instruction: &Instruction) -> Option<Option<Load>> {
    if let Some(index) = var_index(instruction, "ldloc") {
        return Some(Some(Load::Local(index)));
    }
    if let Some(index) = var_index(instruction, "ldarg") {
        return Some(Some(Load::Arg(index)));
    }
    Some(Some(match instruction.op.name {
        "nop" => return Some(None),
        "ldnull" => Load::Null,
        "ldc.i4.m1" => Load::Int(-1),
        "ldc.i4.s" | "ldc.i4" | "ldc.i8" => match instruction.value {
            Operand::Int(v) => Load::Int(v),
            _ => return None,
        },
        "dup" => Load::Dup,
        "ceq" => Load::Compare(Compare::Eq),
        "cgt" => Load::Compare(Compare::Gt { unsigned: false }),
        "cgt.un" => Load::Compare(Compare::Gt { unsigned: true }),
        "clt" => Load::Compare(Compare::Lt { unsigned: false }),
        "clt.un" => Load::Compare(Compare::Lt { unsigned: true }),
        // ldc.i4.0 〜 ldc.i4.8
        name => Load::Int(name.strip_prefix("ldc.i4.")?.parse::<i64>().ok()?),
    }))
}

/// 読み取りの命令が、評価スタックから取り出す数と積む数。
fn load_effect(load: Load) -> (i32, i32) {
    match load {
        Load::Compare(_) => (2, 1),
        Load::Dup => (1, 2),
        _ => (0, 1),
    }
}

/// 分岐 `branch` の前で条件を読めるなら、その読み方。
///
/// 分岐の直前の、条件の値を作る読み取り（ローカル変数、引数、定数、比較、`dup`）を後ろから拾い、その前で最も近い
/// 止められる所（スタックが空の所か、呼び出しから戻った所。ネイティブのコードに対応がある）で止めて読む。
/// 呼び出しの引数の中の `a < b ? x : y` のように、スタックに別の値が積まれた所の分かれ道も、これで読める。
///
/// 読んだ向きが、実際に進む向きであること（偽の「通った」を出さない）を、次の条件で保つ: 止める所から分岐までは、
/// ほかから道の入らない一続きで、例外を出さない命令だけ（だから、読んだあと必ず分岐に着く）で、読むローカル変数と
/// 引数を書き換えない（呼び出しも無いので、ほかから書き換えられることもない）。
fn read_before(code: &[Instruction], depths: &[Option<u32>], incoming: &[u32], branch: usize, test: Test, next: u32, lookup: &Lookup) -> Option<Read> {
    let pops = match test {
        Test::True | Test::False | Test::Switch(_) => 1,
        _ => 2,
    };
    // 条件の値を作る読み取りの、始まり。
    let mut need = pops;
    let mut start = branch;
    while need > 0 {
        if incoming[start] != 1 {
            return None;
        }
        start = start.checked_sub(1)?;
        let Some(load) = load_of(&code[start]) else {
            // 最後に要る値が、呼び出しの戻り値（`if (x.IsEmpty())`、`c.Checked ? a : b`）: 呼び出しから戻った所で止めて、
            // その戻り値を読む。戻った所から分岐までは、上で拾った読み取りだけ（例外を出さない）。
            let returned = need == 1 && matches!(code[start].op.name, "call" | "callvirt") && matches!(code[start].value, Operand::Token(token) if (lookup.returns_value)(token));
            if !returned || incoming[start + 1] != 1 {
                return None;
            }
            let mut loads = vec![Load::Returned(code[start].offset)];
            loads.extend(code[start + 1..branch].iter().filter_map(|i| load_of(i).flatten()));
            let taken = code[branch].targets().first().copied().unwrap_or(next);
            return Some(Read { at: code[start + 1].offset, loads, test, taken, next });
        };
        // `nop` は何も積まない。
        let (pop, push) = load.map_or((0, 0), load_effect);
        need = need - push + pop;
        if need < 0 {
            return None;
        }
    }
    let loads: Vec<Load> = code[start..branch].iter().filter_map(|i| load_of(i).flatten()).collect();
    let read_vars: Vec<Load> = loads.iter().copied().filter(|l| matches!(l, Load::Local(_) | Load::Arg(_))).collect();
    // 止める所: 始まりから戻って、最も近い、スタックが空の命令か、呼び出しのすぐ後ろの命令。
    let mut at = start;
    while depths[at] != Some(0) && !(at > 0 && is_call(&code[at - 1])) {
        if incoming[at] != 1 {
            return None;
        }
        at = at.checked_sub(1)?;
        if !cannot_throw(&code[at]) || writes_any(&code[at], &read_vars) {
            return None;
        }
    }
    if at > 0 && code[at - 1].op.flow == Flow::Prefix {
        return None;
    }
    let taken = code[branch].targets().first().copied().unwrap_or(next);
    Some(Read { at: code[at].offset, loads, test, taken, next })
}

/// 命令の番号つきのオペランド（`ldloc.s 4` の 4、`stloc.2` の 2）。
fn var_index(instruction: &Instruction, prefix: &str) -> Option<u16> {
    let name = instruction.op.name;
    let rest = name.strip_prefix(prefix)?;
    match rest {
        "" | ".s" => match instruction.value {
            Operand::Var(index) => Some(index),
            _ => None,
        },
        _ => rest.strip_prefix('.')?.parse().ok(),
    }
}

/// 命令が、`vars` のローカル変数か引数を書き換えるか。
fn writes_any(instruction: &Instruction, vars: &[Load]) -> bool {
    let local = var_index(instruction, "stloc").map(Load::Local);
    let arg = var_index(instruction, "starg").map(Load::Arg);
    local.or(arg).is_some_and(|written| vars.contains(&written))
}

/// 呼び出しの命令か（そのすぐ後ろは、JIT がネイティブのコードとの対応を作る所）。
fn is_call(instruction: &Instruction) -> bool {
    matches!(instruction.op.name, "call" | "callvirt" | "newobj")
}

/// 例外を出さず、流れも変えない命令か（読み取り、定数、`ldstr`、`pop`、ローカル変数への書き込み）。
fn cannot_throw(instruction: &Instruction) -> bool {
    instruction.op.flow == Flow::Next
        && (load_of(instruction).is_some() || matches!(instruction.op.name, "ldstr" | "pop") || var_index(instruction, "stloc").is_some() || var_index(instruction, "starg").is_some())
}

/// コンパイラが作った名前（`<Foo>d__3`、`<Foo>b__0_0`、`<Foo>g__Local|0_0`）の、もとのメソッドの名前。
/// `<>c` や `<>c__DisplayClass0_0` のように、名前を持たないものは `None`。
fn generated_member(raw: &str) -> Option<&str> {
    let rest = raw.strip_prefix('<')?;
    let name = &rest[..rest.find('>')?];
    (!name.is_empty()).then_some(name)
}

/// 利用者の書いた型とメソッドの名前。`types` はメソッドを宣言している型から外側へ並べた「メタデータの名前、表示の名前」。
/// コンパイラが作った入れ子の型（名前が `<` で始まる）は外側へたどり、メソッドの名前はコンパイラの名前から戻す。
pub fn user_names(method: &str, types: &[(&str, String)]) -> (String, String) {
    let mut name = generated_member(method).unwrap_or(method).to_string();
    let named_by_method = generated_member(method).is_some();
    for (index, (raw, display)) in types.iter().enumerate() {
        if !raw.starts_with('<') || index + 1 == types.len() {
            return (display.clone(), name);
        }
        if !named_by_method {
            if let Some(member) = generated_member(raw) {
                name = member.to_string();
            }
        }
    }
    (String::new(), name)
}

/// 行 1 つの状態（同じファイルの同じ行にかかる系列点と分かれ道を合わせたもの）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineState {
    pub hit: bool,
    pub decisions: Vec<Decision>,
}

impl LineState {
    /// 数える分かれ道の向きの「通った数、全部の数」と、数えられない分かれ道の数。
    pub fn branches(&self) -> (usize, usize, usize) {
        let mut covered = 0;
        let mut valid = 0;
        let mut uncountable = 0;
        for decision in &self.decisions {
            if decision.countable() {
                valid += decision.outcomes.len();
                covered += decision.outcomes.iter().filter(|o| o.seen).count();
            } else {
                uncountable += 1;
            }
        }
        (covered, valid, uncountable)
    }
}

/// 数の合計。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub lines_covered: usize,
    pub lines_valid: usize,
    pub branches_covered: usize,
    pub branches_valid: usize,
    /// 数えられない分かれ道の数。
    pub uncountable: usize,
}

impl Totals {
    fn add_lines<'a>(&mut self, lines: impl Iterator<Item = &'a LineState>) {
        for line in lines {
            self.lines_valid += 1;
            self.lines_covered += line.hit as usize;
            let (covered, valid, uncountable) = line.branches();
            self.branches_covered += covered;
            self.branches_valid += valid;
            self.uncountable += uncountable;
        }
    }

    pub fn line_rate(&self) -> f64 {
        rate(self.lines_covered, self.lines_valid)
    }

    pub fn branch_rate(&self) -> f64 {
        rate(self.branches_covered, self.branches_valid)
    }
}

fn rate(covered: usize, valid: usize) -> f64 {
    if valid == 0 { 1.0 } else { covered as f64 / valid as f64 }
}

/// パスを比べる形（大文字と小文字、区切りの違いを除く）。
pub fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// まとめる途中の行。`owned` はその行から始まる系列点があること。
#[derive(Default)]
struct Acc {
    /// その行から始まる系列点を通った。
    start_hit: bool,
    owned: bool,
    /// より前の行から始まる系列点を通り、この行はその範囲の中。
    span_hit: bool,
    decisions: Vec<Decision>,
}

/// メソッドの系列点と分かれ道を、ドキュメントと行ごとに積む（通ったかどうかは `resolve` で決める）。
fn accumulate(method: &MethodCoverage, wanted: &dyn Fn(u32) -> bool, out: &mut BTreeMap<(u32, u32), Acc>) {
    for (point, &hit) in method.points.iter().zip(&method.hit) {
        if !wanted(point.document) {
            continue;
        }
        for line in point.start_line..=point.end_line {
            let state = out.entry((point.document, line)).or_default();
            if line == point.start_line {
                state.owned = true;
                state.start_hit |= hit;
            } else if hit {
                state.span_hit = true;
            }
        }
    }
    for decision in &method.decisions {
        if wanted(decision.document) {
            out.entry((decision.document, decision.line)).or_default().decisions.push(decision.clone());
        }
    }
}

fn merge_acc(into: &mut Acc, from: Acc) {
    into.start_hit |= from.start_hit;
    into.owned |= from.owned;
    into.span_hit |= from.span_hit;
    into.decisions.extend(from.decisions);
}

/// 行を通ったことにする。その行から始まる系列点があるなら、それを通ったときだけ。無ければ、覆っている系列点を通ればよい。
fn resolve(acc: BTreeMap<(u32, u32), Acc>) -> BTreeMap<(u32, u32), LineState> {
    acc.into_iter()
        .map(|(key, state)| {
            let hit = state.start_hit || (state.span_hit && !state.owned);
            (key, LineState { hit, decisions: state.decisions })
        })
        .collect()
}

impl ModuleCoverage {
    fn document(&self, row: u32) -> &str {
        row.checked_sub(1).and_then(|i| self.documents.get(i as usize)).map_or("", String::as_str)
    }
}

impl Coverage {
    /// ファイルごとの行の状態。キーは比べる形のパス（`normalize`）で、値は PDB に書かれたままのパスと行。同じファイルを
    /// 使うモジュールが複数あれば（リンクしたファイル）、合わせる。`wanted` は、比べる形のパスで選ぶ。
    fn collect(&self, wanted: &dyn Fn(&str) -> bool) -> BTreeMap<String, (String, BTreeMap<u32, LineState>)> {
        let mut out: BTreeMap<String, (String, BTreeMap<u32, Acc>)> = BTreeMap::new();
        for module in &self.modules {
            let keys: Vec<String> = module.documents.iter().map(|d| normalize(d)).collect();
            let chosen: Vec<bool> = keys.iter().map(|k| wanted(k)).collect();
            let mut lines = BTreeMap::new();
            for method in &module.methods {
                accumulate(method, &|document| document.checked_sub(1).and_then(|i| chosen.get(i as usize)).copied().unwrap_or(false), &mut lines);
            }
            for ((document, line), state) in lines {
                let index = document as usize - 1;
                let (_, file) = out.entry(keys[index].clone()).or_insert_with(|| (module.documents[index].clone(), BTreeMap::new()));
                merge_acc(file.entry(line).or_default(), state);
            }
        }
        out.into_iter()
            .map(|(key, (path, acc))| {
                let lines = acc
                    .into_iter()
                    .map(|(line, state)| {
                        let hit = state.start_hit || (state.span_hit && !state.owned);
                        (line, LineState { hit, decisions: state.decisions })
                    })
                    .collect();
                (key, (path, lines))
            })
            .collect()
    }

    /// ファイル `path` の行ごとの状態。
    pub fn file(&self, path: &str) -> BTreeMap<u32, LineState> {
        let wanted = normalize(path);
        self.collect(&|key| key == wanted).into_values().next().map(|(_, lines)| lines).unwrap_or_default()
    }

    /// ファイルごとの行の状態（パスは PDB に書かれたまま）。
    pub fn files(&self) -> Vec<(String, BTreeMap<u32, LineState>)> {
        self.collect(&|_| true).into_values().collect()
    }

    /// 合計。行は、ファイルと行の組で 1 つと数える。分かれ道は、メソッドごとに別のもの。
    pub fn totals(&self) -> Totals {
        let mut totals = Totals::default();
        for (_, lines) in self.files() {
            for line in lines.values() {
                totals.lines_valid += 1;
                totals.lines_covered += usize::from(line.hit);
            }
        }
        for module in &self.modules {
            for method in &module.methods {
                for decision in &method.decisions {
                    if decision.countable() {
                        totals.branches_valid += decision.outcomes.len();
                        totals.branches_covered += decision.outcomes.iter().filter(|o| o.seen).count();
                    } else {
                        totals.uncountable += 1;
                    }
                }
            }
        }
        totals
    }

    /// Cobertura XML（`coverage` / `packages` / `package` / `classes` / `class` / `methods` / `lines` / `line`）。
    /// `timestamp` は UNIX 時刻の秒。並びは名前の順（同じ実行なら、同じファイルになる）。
    pub fn to_cobertura(&self, timestamp: u64) -> String {
        let all_documents: Vec<&str> = self.modules.iter().flat_map(|m| m.methods.iter().flat_map(move |method| method.points.iter().map(move |p| m.document(p.document)))).collect();
        let root = common_root(&all_documents);
        let relative = |path: &str| match &root {
            Some(root) if path.len() > root.len() && normalize(&path[..root.len()]) == normalize(root) => path[root.len()..].to_string(),
            _ => path.to_string(),
        };
        let totals = self.totals();
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        let _ = writeln!(
            xml,
            "<coverage line-rate=\"{}\" branch-rate=\"{}\" version=\"1.9\" timestamp=\"{timestamp}\" lines-covered=\"{}\" lines-valid=\"{}\" branches-covered=\"{}\" branches-valid=\"{}\" complexity=\"0\">",
            format_rate(totals.line_rate()),
            format_rate(totals.branch_rate()),
            totals.lines_covered,
            totals.lines_valid,
            totals.branches_covered,
            totals.branches_valid
        );
        xml.push_str("  <sources>\n");
        if let Some(root) = &root {
            let _ = writeln!(xml, "    <source>{}</source>", escape(root));
        }
        xml.push_str("  </sources>\n  <packages>\n");
        let mut modules: Vec<&ModuleCoverage> = self.modules.iter().collect();
        modules.sort_by(|a, b| a.assembly.cmp(&b.assembly).then(a.path.cmp(&b.path)));
        for module in modules {
            write_package(&mut xml, module, &relative);
        }
        xml.push_str("  </packages>\n</coverage>\n");
        xml
    }
}

/// package（アセンブリ）1 つ。class は（型、ドキュメント）ごと（部分クラスは、ファイルごとに分ける。coverlet と同じ）。
fn write_package(xml: &mut String, module: &ModuleCoverage, relative: &dyn Fn(&str) -> String) {
    // （型、ドキュメント）→ そのクラスのメソッド。
    let mut classes: BTreeMap<(String, String), Vec<&MethodCoverage>> = BTreeMap::new();
    for method in &module.methods {
        let mut documents: Vec<u32> = method.points.iter().map(|p| p.document).collect();
        documents.sort_unstable();
        documents.dedup();
        for document in documents {
            classes.entry((method.class.clone(), module.document(document).to_string())).or_default().push(method);
        }
    }
    let mut package_totals = Totals::default();
    let mut body = String::new();
    for ((class, path), methods) in &classes {
        let wanted = normalize(path);
        let in_file = |document: u32| normalize(module.document(document)) == wanted;
        let mut class_acc = BTreeMap::new();
        for method in methods {
            accumulate(method, &in_file, &mut class_acc);
        }
        let class_lines = resolve(class_acc);
        let mut class_totals = Totals::default();
        class_totals.add_lines(class_lines.values());
        package_totals.add_lines(class_lines.values());
        let _ = writeln!(
            body,
            "        <class name=\"{}\" filename=\"{}\" line-rate=\"{}\" branch-rate=\"{}\" complexity=\"0\">",
            escape(class),
            escape(&relative(path)),
            format_rate(class_totals.line_rate()),
            format_rate(class_totals.branch_rate())
        );
        body.push_str("          <methods>\n");
        let mut sorted: Vec<&&MethodCoverage> = methods.iter().collect();
        sorted.sort_by(|a, b| (&a.name, &a.signature, a.token).cmp(&(&b.name, &b.signature, b.token)));
        for method in sorted {
            let mut acc = BTreeMap::new();
            accumulate(method, &in_file, &mut acc);
            let lines = resolve(acc);
            let mut totals = Totals::default();
            totals.add_lines(lines.values());
            let _ = writeln!(
                body,
                "            <method name=\"{}\" signature=\"{}\" line-rate=\"{}\" branch-rate=\"{}\" complexity=\"0\">",
                escape(&method.name),
                escape(&method.signature),
                format_rate(totals.line_rate()),
                format_rate(totals.branch_rate())
            );
            write_lines(&mut body, &lines, "              ");
            body.push_str("            </method>\n");
        }
        body.push_str("          </methods>\n");
        write_lines(&mut body, &class_lines, "          ");
        body.push_str("        </class>\n");
    }
    let _ = writeln!(
        xml,
        "    <package name=\"{}\" line-rate=\"{}\" branch-rate=\"{}\" complexity=\"0\">",
        escape(&module.assembly),
        format_rate(package_totals.line_rate()),
        format_rate(package_totals.branch_rate())
    );
    xml.push_str("      <classes>\n");
    xml.push_str(&body);
    xml.push_str("      </classes>\n    </package>\n");
}

/// `lines` の要素。分かれ道のある行は `branch="true"`、`condition-coverage`、`conditions`（数えるものだけ）。
fn write_lines(xml: &mut String, lines: &BTreeMap<(u32, u32), LineState>, indent: &str) {
    let _ = writeln!(xml, "{indent}<lines>");
    for ((_, number), state) in lines {
        let hits = state.hit as u32;
        let (covered, valid, _) = state.branches();
        if valid == 0 {
            let _ = writeln!(xml, "{indent}  <line number=\"{number}\" hits=\"{hits}\" branch=\"false\" />");
            continue;
        }
        let percent = covered * 100 / valid;
        let _ = writeln!(xml, "{indent}  <line number=\"{number}\" hits=\"{hits}\" branch=\"true\" condition-coverage=\"{percent}% ({covered}/{valid})\">");
        let _ = writeln!(xml, "{indent}    <conditions>");
        for decision in state.decisions.iter().filter(|d| d.countable()) {
            let seen = decision.outcomes.iter().filter(|o| o.seen).count();
            let kind = match decision.kind {
                DecisionKind::Jump => "jump",
                DecisionKind::Switch => "switch",
            };
            let _ = writeln!(xml, "{indent}      <condition number=\"{}\" type=\"{kind}\" coverage=\"{}%\" />", decision.offset, seen * 100 / decision.outcomes.len());
        }
        let _ = writeln!(xml, "{indent}    </conditions>");
        let _ = writeln!(xml, "{indent}  </line>");
    }
    let _ = writeln!(xml, "{indent}</lines>");
}

fn format_rate(rate: f64) -> String {
    let text = format!("{rate:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".into() } else { trimmed.to_string() }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// パスに共通のフォルダー（末尾の区切りまで）。共通のものが無い（ドライブが違う）か、パスが無ければ `None`。
fn common_root(paths: &[&str]) -> Option<String> {
    let first = paths.first()?;
    let mut end = first.rfind(['\\', '/'])? + 1;
    for path in &paths[1..] {
        let common = first.chars().zip(path.chars()).take_while(|(a, b)| a.eq_ignore_ascii_case(b) || (matches!(a, '\\' | '/') && matches!(b, '\\' | '/'))).map(|(a, _)| a.len_utf8()).sum::<usize>();
        let limit = common.min(end);
        end = first[..limit].rfind(['\\', '/']).map_or(0, |i| i + 1);
        if end == 0 {
            return None;
        }
    }
    Some(first[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::il::instructions;
    use crate::debugger::pdb::HIDDEN_LINE;

    fn sp(il_offset: u32, line: u32) -> SequencePoint {
        SequencePoint { il_offset, document: 1, start_line: line, start_column: 1, end_line: line, end_column: 2 }
    }

    fn never(_: u32) -> bool {
        false
    }

    fn always(_: u32) -> bool {
        true
    }

    /// メタデータを引かない（コンパイラのフィールドも、値を返す呼び出しも無い）。
    const NONE: Lookup<'static> = Lookup { generated_field: &never, returns_value: &never };

    #[test]
    fn a_branch_on_a_call_result_is_read_where_the_call_returns() {
        // if (x.IsEmpty()) の形: 0: ldarg.0  1: callvirt  6: brtrue.s 9  8: nop  9: ret
        let il = [0x02, 0x6F, 0x01, 0x00, 0x00, 0x0A, 0x2D, 0x01, 0x00, 0x2A];
        let (code, depths) = flow(&il);
        let lookup = Lookup { generated_field: &never, returns_value: &always };
        let found = decisions(&code, &depths, &[], &[sp(0, 1)], &lookup);
        let read = found[0].read.as_ref().expect("戻り値を読む");
        assert_eq!((read.at, read.loads.clone()), (6, vec![Load::Returned(1)]));
        assert!(decisions(&code, &depths, &[], &[sp(0, 1)], &NONE)[0].read.is_none(), "値を返さない呼び出しなら読まない");
    }

    /// スタックの深さを、決まった数の命令だけで数える（試験の IL は呼び出しを含まない）。
    fn flow(il: &[u8]) -> (Vec<Instruction>, Vec<Option<u32>>) {
        use crate::debugger::il::Stack;
        let code: Vec<Instruction> = instructions(il).collect();
        let effect = |i: &Instruction| {
            let n = |s: Stack| match s {
                Stack::Fixed(n) => n as u32,
                Stack::Call => 0,
            };
            (n(i.op.pop), n(i.op.push))
        };
        let depths = crate::debugger::decompile::flow::stack_depths(&code, &[], &effect);
        (code, depths)
    }

    /// Debug ビルドの `if (a || b) S; else E;`（引数 a、b。条件は一時変数 V_0 に入れる）。
    ///   0: ldarg.0  1: brtrue.s 6  3: ldarg.1  4: br.s 7  6: ldc.i4.1  7: stloc.0  8: ldloc.0  9: brfalse.s 14
    ///  11: nop  12: br.s 15  14: nop  15: ret
    const OR_DEBUG: [u8; 16] = [0x02, 0x2D, 0x03, 0x03, 0x2B, 0x01, 0x17, 0x0A, 0x06, 0x2C, 0x03, 0x00, 0x2B, 0x01, 0x00, 0x2A];

    #[test]
    fn the_branches_of_an_or_are_seen_at_their_targets_or_by_reading_the_condition() {
        let (code, depths) = flow(&OR_DEBUG);
        let points = [sp(0, 10), sp(11, 11), sp(14, 13), sp(15, 14)];
        let found = decisions(&code, &depths, &[], &points, &NONE);
        assert_eq!(found.len(), 2);
        // a の分岐: 真の側（6: ldc.i4.1）にも偽の側（3: ldarg.1）にも、ほかから道が入らない。
        let a = &found[0];
        assert_eq!((a.offset, a.line), (1, 10));
        assert_eq!(a.outcomes.iter().map(|o| (o.target, o.value, o.at_target)).collect::<Vec<_>>(), [(6, Some(true), true), (3, Some(false), true)]);
        assert_eq!(a.read.as_ref().map(|r| (r.at, r.loads.clone())), Some((0, vec![Load::Arg(0)])));
        // 条件全体の分岐: 偽の側（14）は else の頭で、道は 1 つ。真の側（11）も 1 つ。条件は V_0 を読めば分かる。
        let whole = &found[1];
        assert_eq!(whole.outcomes.iter().map(|o| (o.target, o.value, o.at_target)).collect::<Vec<_>>(), [(14, Some(false), true), (11, Some(true), true)]);
        assert_eq!(whole.read.as_ref().map(|r| (r.at, r.loads.clone(), r.test.clone())), Some((8, vec![Load::Local(0)], Test::False)));
    }

    #[test]
    fn the_condition_temp_tested_in_hidden_code_belongs_to_the_if_line() {
        // Roslyn は `stloc.0; ldloc.0; brfalse` を隠し行にする（7 から）。
        let (code, depths) = flow(&OR_DEBUG);
        let hidden = SequencePoint { start_line: HIDDEN_LINE, end_line: HIDDEN_LINE, ..sp(7, 0) };
        let found = decisions(&code, &depths, &[], &[sp(0, 10), hidden, sp(11, 11), sp(14, 13), sp(15, 14)], &NONE);
        assert_eq!(found.iter().map(|d| (d.offset, d.line)).collect::<Vec<_>>(), [(1, 10), (9, 10)]);
    }

    #[test]
    fn a_target_where_two_paths_meet_is_not_waited_at() {
        // if (c) S;（else なし）: 偽の側は、本体の終わりからも流れてくる。
        //   0: ldarg.0  1: brfalse.s 4  3: nop  4: ret
        let (code, depths) = flow(&[0x02, 0x2C, 0x01, 0x00, 0x2A]);
        let found = decisions(&code, &depths, &[], &[sp(0, 1), sp(3, 2), sp(4, 3)], &NONE);
        let outcomes: Vec<(u32, bool)> = found[0].outcomes.iter().map(|o| (o.target, o.at_target)).collect();
        assert_eq!(outcomes, [(4, false), (3, true)]);
        // 条件は読めるので、数える。
        assert!(found[0].countable());
        assert_eq!(found[0].read.as_ref().unwrap().at, 0);
    }

    #[test]
    fn branches_in_hidden_code_are_not_counted() {
        let (code, depths) = flow(&[0x02, 0x2C, 0x01, 0x00, 0x2A]);
        let hidden = SequencePoint { start_line: HIDDEN_LINE, end_line: HIDDEN_LINE, ..sp(0, 0) };
        assert!(decisions(&code, &depths, &[], &[hidden, sp(3, 2)], &NONE).is_empty());
    }

    #[test]
    fn a_condition_inside_call_arguments_is_read_at_the_statement_start() {
        // M("x", a <= b ? 1 : 2) の形: 0: ldstr  5: ldloc.0  6: ldloc.1  7: ble.s 12  9: ldc.i4.2  10: br.s 13
        // 12: ldc.i4.1  13: call  18: ret。分岐のときはスタックに "x" が残っていて、行き先には置けない。
        let il = [0x72, 1, 0, 0, 0x70, 0x06, 0x07, 0x31, 0x03, 0x18, 0x2B, 0x01, 0x17, 0x28, 1, 0, 0, 0x06, 0x2A];
        let (code, depths) = flow(&il);
        let found = decisions(&code, &depths, &[], &[sp(0, 1)], &NONE);
        let read = found[0].read.as_ref().expect("条件は読める");
        assert_eq!((read.at, read.loads.clone(), read.test.clone()), (0, vec![Load::Local(0), Load::Local(1)], Test::Le { unsigned: false }));
        assert!(found[0].countable());
    }

    #[test]
    fn a_condition_whose_variable_is_written_on_the_way_is_not_read() {
        // 0: ldstr  5: ldc.i4.1  6: stloc.0  7: ldloc.0  8: brtrue.s 11  10: nop  11: pop  12: ret
        let il = [0x72, 1, 0, 0, 0x70, 0x17, 0x0A, 0x06, 0x2D, 0x01, 0x00, 0x26, 0x2A];
        let (code, depths) = flow(&il);
        let found = decisions(&code, &depths, &[], &[sp(0, 1)], &NONE);
        assert!(found[0].read.is_none());
        assert!(!found[0].countable(), "行き先にも置けない（スタックが空でない）");
    }

    #[test]
    fn the_compilers_lambda_cache_check_is_not_a_branch() {
        // 0: ldsfld <>9__0_0  5: dup  6: brtrue.s 9  8: pop  9: pop  10: ret
        let il = [0x7E, 1, 0, 0, 0x04, 0x25, 0x2D, 0x01, 0x26, 0x26, 0x2A];
        let (code, depths) = flow(&il);
        assert!(decisions(&code, &depths, &[], &[sp(0, 1)], &Lookup { generated_field: &|token| token == 0x0400_0001, returns_value: &|_| false }).is_empty());
        assert_eq!(decisions(&code, &depths, &[], &[sp(0, 1)], &NONE).len(), 1, "利用者のフィールドの ?? は数える");
    }

    #[test]
    fn a_switch_has_one_outcome_per_target_and_the_default() {
        // 0: ldarg.0  1: switch (2 個。どちらも 15)  14: nop  15: ret
        // switch の分岐先は相対: 次の命令（14）から +1 → 15。
        let il = [0x02, 0x45, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2A];
        let (code, depths) = flow(&il);
        let found = decisions(&code, &depths, &[], &[sp(0, 1)], &NONE);
        let switch = &found[0];
        assert_eq!(switch.kind, DecisionKind::Switch);
        assert_eq!(switch.outcomes.iter().map(|o| (o.target, o.cases.clone())).collect::<Vec<_>>(), [(15, vec![0, 1]), (14, vec![])]);
    }

    #[test]
    fn a_read_condition_picks_the_destination() {
        let read = Read { at: 0, loads: vec![Load::Local(0), Load::Int(3)], test: Test::Lt { unsigned: false }, taken: 10, next: 5 };
        let mut values = |load: Load| match load {
            Load::Local(0) => Some(Val::Int(2)),
            _ => None,
        };
        assert_eq!(read.destination(&mut values), Some(10));
        let null_check = Read { at: 0, loads: vec![Load::Arg(1), Load::Null, Load::Compare(Compare::Gt { unsigned: true })], test: Test::True, taken: 10, next: 5 };
        assert_eq!(null_check.destination(&mut |_| Some(Val::Ref(0x1000))), Some(10));
        assert_eq!(null_check.destination(&mut |_| Some(Val::Ref(0))), Some(5));
        assert_eq!(null_check.destination(&mut |_| None), None, "読めない値があれば決めない");
        let switch = Read { at: 0, loads: vec![Load::Local(1)], test: Test::Switch(vec![20, 30]), taken: 0, next: 5 };
        assert_eq!(switch.destination(&mut |_| Some(Val::Int(1))), Some(30));
        assert_eq!(switch.destination(&mut |_| Some(Val::Int(-1))), Some(5), "範囲の外は既定の側");
    }

    fn method(points: Vec<Point>, decisions: Vec<Decision>) -> MethodCoverage {
        MethodCoverage::new(0x0600_0001, "App.Program".into(), "Main".into(), "(string[])".into(), points, decisions)
    }

    fn point(il_offset: u32, line: u32) -> Point {
        Point { il_offset, document: 1, start_line: line, end_line: line }
    }

    #[test]
    fn entering_a_method_places_the_rest_and_resolves_how_to_see_each_branch() {
        let (code, depths) = flow(&OR_DEBUG);
        let points = [sp(0, 10), sp(11, 11), sp(14, 13), sp(15, 14)];
        let mut m = method(visible_points(&points), decisions(&code, &depths, &[], &points, &NONE));
        // 6（`||` の真の側の ldc.i4.1）だけは、ネイティブのコードに対応が無いとする。
        let sites = m.enter(&|offset| offset != 6);
        assert!(m.hit[0], "入口と同じ所の系列点は、通ったことにする");
        assert_eq!(m.decisions[0].outcomes[0].how, How::ByReading);
        assert_eq!(m.decisions[0].outcomes[1].how, How::AtTarget);
        assert!(sites.contains(&(11, Role::Point(1))));
        assert!(sites.contains(&(3, Role::Target { decision: 0, outcome: 1 })));
        assert!(sites.contains(&(0, Role::Read(0))));
        // 条件を読んで、真の側を見た。読む役目は終わる（ほかに読む向きが無い）。
        assert!(m.reach(Role::Read(0), Some(6)));
        assert!(m.decisions[0].outcomes[0].seen);
    }

    #[test]
    fn reading_stops_after_too_many_reads_without_news() {
        let read = Read { at: 0, loads: vec![Load::Arg(0)], test: Test::True, taken: 6, next: 3 };
        let outcome = |target, value| Outcome { target, value: Some(value), cases: vec![], at_target: false, how: How::Pending, seen: false };
        let decision = Decision { offset: 1, document: 1, line: 1, kind: DecisionKind::Jump, outcomes: vec![outcome(6, true), outcome(3, false)], read: Some(read), stale_reads: 0 };
        let mut m = method(vec![point(0, 1)], vec![decision]);
        m.enter(&|_| true);
        assert!(!m.reach(Role::Read(0), Some(6)));
        for _ in 0..READ_LIMIT - 1 {
            assert!(!m.reach(Role::Read(0), Some(6)));
        }
        assert!(m.reach(Role::Read(0), Some(6)), "新しい向きを見ないまま上限に達した");
        assert!(!m.decisions[0].outcomes[1].seen);
    }

    #[test]
    fn compiler_generated_types_and_methods_are_named_after_the_users_method() {
        let types = |list: &[&'static str]| list.iter().map(|raw| (*raw, format!("App.{raw}"))).collect::<Vec<_>>();
        assert_eq!(user_names("MoveNext", &types(&["<LoadAsync>d__3", "MainForm"])), ("App.MainForm".into(), "LoadAsync".into()));
        assert_eq!(user_names("<Save>b__0_0", &types(&["<>c", "MainForm"])), ("App.MainForm".into(), "Save".into()));
        assert_eq!(user_names("<Run>b__0", &types(&["<>c__DisplayClass0_0", "Program"])), ("App.Program".into(), "Run".into()));
        assert_eq!(user_names("<Main>g__Local|0_0", &types(&["Program"])), ("App.Program".into(), "Main".into()));
        assert_eq!(user_names("Click", &types(&["MainForm"])), ("App.MainForm".into(), "Click".into()));
    }

    fn module(path: &str, methods: Vec<MethodCoverage>) -> ModuleCoverage {
        ModuleCoverage { path: format!("C:\\app\\{path}.dll"), assembly: path.into(), documents: vec!["C:\\src\\App\\Program.cs".into(), "C:\\src\\App\\Forms\\Main.cs".into()], methods }
    }

    #[test]
    fn a_line_owned_by_another_method_is_not_covered_by_an_outer_statement() {
        // Main の `Click += (s, e) => { … }` は 25 行目から 29 行目までを 1 つの系列点にする。本体の行は、ラムダのメソッドの
        // 系列点から始まる。外側の文を実行しただけでは、ボタンを押す前の行を通ったことにしない。
        let mut outer = method(vec![Point { il_offset: 0x27F, document: 1, start_line: 25, end_line: 29 }], vec![]);
        outer.hit[0] = true;
        let mut lambda = MethodCoverage::new(0x0600_0002, "App.Program".into(), "Main".into(), "()".into(), vec![point(0, 26), point(1, 27), point(0x55, 28), point(0x67, 29)], vec![]);
        let before = Coverage { modules: vec![module("App", vec![outer.clone(), lambda.clone()])] };
        let lines = before.file("c:/src/app/program.cs");
        assert!(lines[&25].hit);
        assert!(!lines[&26].hit && !lines[&27].hit && !lines[&28].hit && !lines[&29].hit);
        lambda.hit = vec![true, true, true, true];
        let after = Coverage { modules: vec![module("App", vec![outer, lambda])] };
        let lines = after.file("c:/src/app/program.cs");
        assert!(lines[&26].hit && lines[&27].hit && lines[&28].hit && lines[&29].hit);
    }

    #[test]
    fn a_point_over_several_lines_covers_them_all() {
        let mut m = method(vec![Point { il_offset: 0, document: 1, start_line: 3, end_line: 5 }, point(8, 7)], vec![]);
        m.hit[0] = true;
        let coverage = Coverage { modules: vec![module("App", vec![m])] };
        let lines = coverage.file("c:/src/app/program.cs");
        assert_eq!(lines.iter().map(|(n, s)| (*n, s.hit)).collect::<Vec<_>>(), [(3, true), (4, true), (5, true), (7, false)]);
        assert_eq!(coverage.totals(), Totals { lines_covered: 3, lines_valid: 4, ..Totals::default() });
    }

    #[test]
    fn cobertura_has_relative_file_names_rates_and_conditions() {
        let outcome = |target, value, seen| Outcome { target, value: Some(value), cases: vec![], at_target: true, how: How::AtTarget, seen };
        let decision = Decision { offset: 9, document: 1, line: 10, kind: DecisionKind::Jump, outcomes: vec![outcome(14, false, false), outcome(11, true, true)], read: None, stale_reads: 0 };
        let mut main = method(vec![point(0, 10), point(11, 11), point(14, 13)], vec![decision]);
        main.hit = vec![true, true, false];
        let mut click = MethodCoverage::new(0x0600_0002, "App.Main<T> & \"Form\"".into(), "Click".into(), "()".into(), vec![Point { il_offset: 0, document: 2, start_line: 4, end_line: 4 }], vec![]);
        click.hit = vec![true];
        let xml = Coverage { modules: vec![module("App", vec![main, click])] }.to_cobertura(1_700_000_000);
        assert!(xml.contains("<coverage line-rate=\"0.75\" branch-rate=\"0.5\" version=\"1.9\" timestamp=\"1700000000\" lines-covered=\"3\" lines-valid=\"4\" branches-covered=\"1\" branches-valid=\"2\""), "{xml}");
        assert!(xml.contains("<source>C:\\src\\App\\</source>"), "{xml}");
        assert!(xml.contains("<class name=\"App.Program\" filename=\"Program.cs\""), "{xml}");
        assert!(xml.contains("filename=\"Forms\\Main.cs\""), "{xml}");
        assert!(xml.contains("name=\"App.Main&lt;T&gt; &amp; &quot;Form&quot;\""), "{xml}");
        assert!(xml.contains("<line number=\"10\" hits=\"1\" branch=\"true\" condition-coverage=\"50% (1/2)\">"), "{xml}");
        assert!(xml.contains("<condition number=\"9\" type=\"jump\" coverage=\"50%\" />"), "{xml}");
        assert!(xml.contains("<line number=\"13\" hits=\"0\" branch=\"false\" />"), "{xml}");
        assert!(xml.contains("<method name=\"Main\" signature=\"(string[])\""), "{xml}");
    }

    #[test]
    fn uncountable_branches_are_left_out_of_the_rates() {
        let outcome = |how| Outcome { target: 1, value: Some(true), cases: vec![], at_target: false, how, seen: false };
        let decision = Decision { offset: 1, document: 1, line: 1, kind: DecisionKind::Jump, outcomes: vec![outcome(How::Unknown), outcome(How::AtTarget)], read: None, stale_reads: 0 };
        let m = method(vec![point(0, 1)], vec![decision]);
        let totals = Coverage { modules: vec![module("App", vec![m])] }.totals();
        assert_eq!((totals.branches_valid, totals.uncountable), (0, 1));
    }

    #[test]
    fn the_common_root_ends_at_a_separator() {
        assert_eq!(common_root(&["C:\\src\\App\\A.cs", "c:\\SRC\\App\\Forms\\B.cs"]).as_deref(), Some("C:\\src\\App\\"));
        assert_eq!(common_root(&["C:\\src\\Apple\\A.cs", "C:\\src\\App\\B.cs"]).as_deref(), Some("C:\\src\\"));
        assert_eq!(common_root(&["C:\\a\\A.cs", "D:\\a\\B.cs"]), None);
        assert_eq!(common_root(&[]), None);
    }

    #[test]
    fn rates_are_written_briefly() {
        assert_eq!(format_rate(1.0), "1");
        assert_eq!(format_rate(0.0), "0");
        assert_eq!(format_rate(2.0 / 3.0), "0.6667");
    }
}
