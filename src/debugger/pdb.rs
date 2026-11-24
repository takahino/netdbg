//! Portable PDB リーダ。
//!
//! 読むのは Document と MethodDebugInformation（シーケンスポイント）、LocalScope と LocalVariable（ローカル変数の名前）。
//! これで「ファイルと行 ⇔ メソッドと IL オフセット」と「IL オフセット → その場所で見えるローカル変数」が引ける。
//! PDB のファイルのほか、アセンブリに埋め込まれた PDB（Deflate で圧縮されている）も読む。どれを使うかは `symbols.rs`。
//!
//! 形式は ECMA-335 のメタデータと同じ器なので、表の読み方は `metadata.rs` に任せ、ここは PDB に固有の
//! blob（Document の名前、シーケンスポイント）の解釈だけを持つ。
//!
//! Edit and Continue: 差分（`EmitDifference` の PDB の差分）を重ねると、変えたメソッドに新しい版ができる。引く関数は
//! メソッドの版（`ICorDebugFunction2::GetVersionNumber`。古い版を実行中のフレームは古い版）を受け取り、その版の
//! シーケンスポイントとローカル変数を返す。行だけが動いたメソッド（中身は同じ）は、行のずれ（`shift_lines`）で直す。

use crate::debugger::metadata::{Metadata, Reader, CUSTOM_DEBUG_INFORMATION, DOCUMENT, ENC_MAP, LOCAL_SCOPE, LOCAL_VARIABLE, METHOD_DEBUG_INFORMATION};
use std::collections::BTreeMap;
use std::path::Path;

mod windows;

/// 「いちばん新しい版」を指す版の番号（ブレークポイントを置くとき）。
pub const LATEST: u32 = u32::MAX;
const IMPORT_SCOPE: usize = 0x35;
const STATE_MACHINE_METHOD: usize = 0x36;
/// タプルの要素名（CustomDebugInformation の TupleElementNames。親は LocalVariable）。
/// ED9FDF71-8879-4747-8ED3-FE5EDE3CE710
const TUPLE_NAMES: [u8; 16] = [0x71, 0xDF, 0x9F, 0xED, 0x79, 0x88, 0x47, 0x47, 0x8E, 0xD3, 0xFE, 0x5E, 0xDE, 0x3C, 0xE7, 0x10];
/// async メソッドのステップ用の情報（CustomDebugInformation の「AsyncMethodSteppingInformation」）。
/// 54FD2AC5-E925-401A-9C2A-F94F171072F8
const ASYNC_STEPPING: [u8; 16] = [0xC5, 0x2A, 0xFD, 0x54, 0x25, 0xE9, 0x1A, 0x40, 0x9C, 0x2A, 0xF9, 0x4F, 0x17, 0x10, 0x72, 0xF8];

/// 「隠し」シーケンスポイントの行番号。ステップでは止まらずに通過する。
pub const HIDDEN_LINE: u32 = 0x00FE_EFEE;

#[derive(Debug, Clone, PartialEq)]
pub struct SequencePoint {
    pub il_offset: u32,
    /// Document 表の行番号（1 始まり）。
    pub document: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl SequencePoint {
    pub fn is_hidden(&self) -> bool {
        self.start_line == HIDDEN_LINE
    }
}

#[derive(Debug, Default)]
pub struct Pdb {
    /// Document 表。添字は行番号 - 1。コンパイラが記録したパスそのまま。
    pub documents: Vec<String>,
    /// MethodDebugInformation 表。添字は MethodDef の行番号 - 1。IL オフセットの昇順。
    methods: Vec<Vec<SequencePoint>>,
    /// LocalScope 表。メソッドの行番号、開始オフセットの昇順（表の並びのまま）。
    scopes: Vec<LocalScope>,
    /// ImportScope の行ごとの、取り込まれた名前空間（添字は行番号。0 は空）。
    namespaces: Vec<Vec<String>>,
    /// PDB の ID。アセンブリが覚えている ID と同じなら、そのビルドの PDB。
    pub id: Option<[u8; 20]>,
    /// Windows PDB の GUID（exe の CodeView の GUID と照らす。Windows PDB には、Portable PDB の ID が無い）。
    pub windows_guid: Option<[u8; 16]>,
    /// async メソッドの本体（状態機械の MoveNext）ごとの `await` の位置。キーは MethodDef の行番号。
    awaits: BTreeMap<u32, Vec<Await>>,
    /// StateMachineMethod 表: async メソッド・イテレーターの、もとのメソッド（キックオフ）から本体（MoveNext）への対応。
    /// どちらも MethodDef の行番号。
    state_machines: BTreeMap<u32, u32>,
    /// Edit and Continue: メソッドの行ごとの、差分で作られた版（古い順）。
    revisions: BTreeMap<u32, Vec<Revision>>,
    /// Edit and Continue: ファイルの版のメソッドの、行のずれ（中身が同じまま、上の行が増えたり減ったりした分）。
    base_shifts: BTreeMap<u32, i32>,
}

/// 差分で作られたメソッドの版 1 つ。
#[derive(Debug)]
struct Revision {
    /// ランタイムの版の番号（`ICorDebugFunction2::GetVersionNumber`）。
    version: u32,
    points: Vec<SequencePoint>,
    scopes: Vec<LocalScope>,
    awaits: Vec<Await>,
    /// この版ができたあとの、行のずれ。
    shift: i32,
}

/// シーケンスポイントの行を、行のずれの分だけ動かす（隠し行はそのまま）。
fn shifted(point: &SequencePoint, shift: i32) -> SequencePoint {
    let mut point = point.clone();
    if !point.is_hidden() && shift != 0 {
        point.start_line = point.start_line.saturating_add_signed(shift);
        point.end_line = point.end_line.saturating_add_signed(shift);
    }
    point
}

/// `await` 一つ分。待つものがまだ終わっていなければ、メソッドは `yield_offset` でいったん戻り、あとで `resume_offset` から続く。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Await {
    pub yield_offset: u32,
    pub resume_offset: u32,
}

#[derive(Debug, Clone)]
struct LocalScope {
    /// MethodDef の行番号。
    method: u32,
    /// IL オフセットの範囲。
    range: std::ops::Range<u32>,
    variables: Vec<LocalVariable>,
    /// ImportScope の行番号（この範囲から見える using）。
    imports: u32,
}

/// ImportScope の Imports（Portable PDB の仕様「Imports Blob」）から、取り込まれた名前空間を取り出す。
/// 別名（`using X = …`）や型の取り込み（`using static`）は読み飛ばす。
fn imported_namespaces(metadata: &Metadata, data: &[u8]) -> Vec<String> {
    let mut r = Reader::new(data);
    let mut namespaces = Vec::new();
    let name = |index: u32| metadata.blob(index).ok().and_then(|bytes| std::str::from_utf8(bytes).ok()).map(str::to_string);
    while r.pos < r.data.len() {
        let Ok(kind) = r.unsigned() else { break };
        // 種類ごとの、続く値の数と、そのうち名前空間の位置（0 始まり）。
        let (count, namespace_at) = match kind {
            1 => (1, Some(0)),
            2 => (2, Some(1)),
            3 | 5 => (1, None),
            4 | 6 | 7 | 9 => (2, None),
            8 => (3, None),
            _ => break,
        };
        for position in 0..count {
            let Ok(value) = r.unsigned() else { return namespaces };
            if namespace_at == Some(position) {
                namespaces.extend(name(value));
            }
        }
    }
    namespaces
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalVariable {
    /// メソッドのローカル変数の番号（ICorDebugILFrame::GetLocalVariable に渡す）。
    pub index: u32,
    pub name: String,
    /// 型がタプルなら、要素の名前（`(int x, string y)` の x、y。名前の無い要素は空）。
    pub tuple_names: Vec<String>,
}

/// ブレークポイントを置く場所。
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// MethodDef トークン（0x06xxxxxx）。
    pub method: u32,
    pub il_offset: u32,
    /// 実際に止まる行。要求された行にコードが無ければ、その先の最初の行になる。
    pub line: u32,
    /// 結んだ文の列（1 始まり）。`end_column` は含まない。
    pub column: u32,
    pub end_column: u32,
}

fn located(method: u32, point: &SequencePoint) -> Location {
    Location { method, il_offset: point.il_offset, line: point.start_line, column: point.start_column, end_column: point.end_column }
}

/// Document の名前。区切り文字 1 バイトのあとに、各部分の #Blob の添字が並ぶ。
fn document_name(metadata: &Metadata, index: u32) -> Result<String, String> {
    let mut r = Reader::new(metadata.blob(index)?);
    let separator = r.u8()?;
    let mut name = String::new();
    let mut first = true;
    while !r.at_end() {
        if !first && separator != 0 {
            name.push(separator as char);
        }
        first = false;
        name.push_str(&String::from_utf8_lossy(metadata.blob(r.unsigned()?)?));
    }
    Ok(name)
}

/// メソッド一つ分のシーケンスポイントの blob を読む。
fn sequence_points(data: &[u8], mut document: u32) -> Result<Vec<SequencePoint>, String> {
    let mut s = Reader::new(data);
    s.unsigned()?; // ローカル変数のシグネチャ
    if document == 0 {
        document = s.unsigned()?; // メソッドが複数のファイルにまたがるとき、最初のファイルはここにある
    }
    let mut points = Vec::new();
    let (mut il, mut line, mut column) = (0u32, 0i64, 0i64);
    let (mut first, mut first_visible) = (true, true);
    while !s.at_end() {
        let delta_il = s.unsigned()?;
        if !first && delta_il == 0 {
            document = s.unsigned()?; // ファイルの切り替え
            continue;
        }
        il += delta_il;
        first = false;
        let delta_lines = s.unsigned()?;
        let delta_columns = if delta_lines == 0 { s.unsigned()? as i64 } else { s.signed()? as i64 };
        if delta_lines == 0 && delta_columns == 0 {
            points.push(SequencePoint { il_offset: il, document, start_line: HIDDEN_LINE, start_column: 0, end_line: HIDDEN_LINE, end_column: 0 });
            continue;
        }
        if first_visible {
            line = s.unsigned()? as i64;
            column = s.unsigned()? as i64;
            first_visible = false;
        } else {
            line += s.signed()? as i64;
            column += s.signed()? as i64;
        }
        points.push(SequencePoint {
            il_offset: il,
            document,
            start_line: line as u32,
            start_column: column as u32,
            end_line: (line + delta_lines as i64) as u32,
            end_column: (column + delta_columns) as u32,
        });
    }
    Ok(points)
}

/// async のステップ用の情報の blob: catch ハンドラーの位置（4 バイト）のあとに、
/// 「中断する位置（4 バイト）、再開する位置（4 バイト）、再開するメソッド（圧縮整数）」が `await` の数だけ並ぶ。
fn async_stepping(data: &[u8]) -> Result<Vec<Await>, String> {
    let mut r = Reader::new(data);
    r.u32()?;
    let mut awaits = Vec::new();
    while !r.at_end() {
        let (yield_offset, resume_offset) = (r.u32()?, r.u32()?);
        r.unsigned()?; // 再開するメソッド。C# では、いつも同じ MoveNext。
        awaits.push(Await { yield_offset, resume_offset });
    }
    Ok(awaits)
}

/// パスを比べる形にそろえる。Windows のパスは大文字小文字を区別せず、区切りは `\` と `/` の両方がある。
fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// MethodDebugInformation の行のシーケンスポイント。
fn method_points(metadata: &Metadata, row: u32) -> Result<Vec<SequencePoint>, String> {
    let document = metadata.cell(METHOD_DEBUG_INFORMATION, row, 0);
    let points = metadata.cell(METHOD_DEBUG_INFORMATION, row, 1);
    if points == 0 {
        return Ok(Vec::new());
    }
    sequence_points(metadata.blob(points)?, document)
}

/// LocalScope の行。`tuple_names` は LocalVariable の行ごとのタプルの要素名。
fn local_scope(metadata: &Metadata, row: u32, tuple_names: &BTreeMap<u32, Vec<String>>) -> LocalScope {
    const DEBUGGER_HIDDEN: u32 = 1;
    let start = metadata.cell(LOCAL_SCOPE, row, 4);
    let variables = metadata
        .child_rows(LOCAL_SCOPE, row, 2, LOCAL_VARIABLE)
        // コンパイラが作った一時変数は見せない。
        .filter(|&variable| metadata.cell(LOCAL_VARIABLE, variable, 0) & DEBUGGER_HIDDEN == 0)
        .map(|variable| LocalVariable {
            index: metadata.cell(LOCAL_VARIABLE, variable, 1),
            name: metadata.string(metadata.cell(LOCAL_VARIABLE, variable, 2)).to_string(),
            tuple_names: tuple_names.get(&variable).cloned().unwrap_or_default(),
        })
        .collect();
    LocalScope { method: metadata.cell(LOCAL_SCOPE, row, 0), range: start..start.saturating_add(metadata.cell(LOCAL_SCOPE, row, 5)), variables, imports: metadata.cell(LOCAL_SCOPE, row, 1) }
}

/// CustomDebugInformation の行が、種類 `kind` で、親の種類（HasCustomDebugInformation の下位 5 ビット）が `tag` なら、
/// 親の行番号と中身。
fn custom_information(metadata: &Metadata, row: u32, tag: u32, kind: [u8; 16]) -> Option<(u32, &[u8])> {
    let parent = metadata.cell(CUSTOM_DEBUG_INFORMATION, row, 0);
    if parent & 31 != tag || metadata.guid(metadata.cell(CUSTOM_DEBUG_INFORMATION, row, 1)) != Some(kind) {
        return None;
    }
    Some((parent >> 5, metadata.blob(metadata.cell(CUSTOM_DEBUG_INFORMATION, row, 2)).ok()?))
}

/// タプルの要素名の blob: 名前を、終わりの 0 で区切って並べたもの。
fn tuple_element_names(blob: &[u8]) -> Vec<String> {
    let names = blob.split(|&b| b == 0).map(|name| String::from_utf8_lossy(name).into_owned()).collect::<Vec<_>>();
    match names.split_last() {
        Some((last, rest)) if last.is_empty() => rest.to_vec(),
        _ => names,
    }
}

impl Pdb {
    /// 逆コンパイルしたソース（D-060）の PDB を、メモリーの上で組み立てる。`documents` はソースのファイルのパス
    /// （`decompiled.documents` と同じ順）。ローカル変数は、メソッド全体で見えるものとして扱う。
    pub fn synthetic(documents: Vec<String>, decompiled: &crate::debugger::decompile::Decompiled) -> Pdb {
        let rows = decompiled.methods.iter().map(|m| m.row).max().unwrap_or(0) as usize;
        let mut methods = vec![Vec::new(); rows];
        let mut scopes = Vec::new();
        let mut ordered: Vec<_> = decompiled.methods.iter().collect();
        // LocalScope はメソッドの行の順に並べる（`scopes` の引き方がそれを前提にしている）。
        ordered.sort_by_key(|m| m.row);
        for method in ordered {
            let document = method.document as u32 + 1;
            methods[method.row as usize - 1] = method
                .points
                .iter()
                .map(|p| SequencePoint { il_offset: p.il_offset, document, start_line: p.line, start_column: p.start_column, end_line: p.line, end_column: p.end_column })
                .collect();
            if !method.locals.is_empty() {
                let variables = method.locals.iter().map(|(index, name)| LocalVariable { index: *index, name: name.clone(), tuple_names: Vec::new() }).collect();
                scopes.push(LocalScope { method: method.row, range: 0..u32::MAX, variables, imports: 0 });
            }
        }
        Pdb { documents, methods, scopes, namespaces: vec![Vec::new()], ..Pdb::default() }
    }

    pub fn open(path: &Path) -> Result<Pdb, String> {
        let data = std::fs::read(path).map_err(|e| tr!("Cannot read {path}: {error}", path = path.display(), error = e))?;
        Pdb::parse(&data).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// アセンブリに埋め込まれた PDB（デバッグ ディレクトリの埋め込み PDB の項目の中身）を読む。
    /// 中身は "MPDB"、展開後の大きさ（4 バイト）、Deflate で圧縮した Portable PDB。
    pub fn from_embedded(entry: &[u8]) -> Result<Pdb, String> {
        // 大きさは信用できないファイルの値。そのまま確保すると、x86 ではメモリが尽きてエンジンが落ちる。
        // ふつうの PDB は大きくても数十 MB。
        const LIMIT: usize = 256 << 20;
        let (Some(b"MPDB"), Some(size)) = (entry.get(..4), entry.get(4..8)) else {
            return Err(tr!("The header of the embedded PDB is not valid"));
        };
        let size = u32::from_le_bytes(size.try_into().map_err(|_| tr_str!("The header of the embedded PDB is not valid"))?) as usize;
        if size > LIMIT {
            return Err(tr!("The embedded PDB is too large ({size} bytes)", size = size));
        }
        let data = crate::inflate::inflate(&entry[8..], size).map_err(|e| tr!("Cannot expand the embedded PDB: {error}", error = e))?;
        Pdb::parse(&data)
    }

    pub fn parse(data: &[u8]) -> Result<Pdb, String> {
        if data.starts_with(b"Microsoft C/C++ MSF") {
            // Windows PDB（古い形式のプロジェクトの既定）。読めない形（壊れている、ネイティブのコードだけ）なら、直し方を出す。
            return windows::parse(data).map(|(pdb, guid)| Pdb { windows_guid: Some(guid), ..pdb }).map_err(|e| {
                tr!("Cannot read the Windows PDB ({error}). Rebuilding the project with <DebugType>portable</DebugType> makes it readable for sure", error = e)
            });
        }
        let metadata = Metadata::parse(data.to_vec())?;
        let mut documents = Vec::with_capacity(metadata.rows(DOCUMENT) as usize);
        for row in 1..=metadata.rows(DOCUMENT) {
            documents.push(document_name(&metadata, metadata.cell(DOCUMENT, row, 0))?);
        }
        let mut methods = Vec::with_capacity(metadata.rows(METHOD_DEBUG_INFORMATION) as usize);
        for row in 1..=metadata.rows(METHOD_DEBUG_INFORMATION) {
            methods.push(method_points(&metadata, row)?);
        }
        const LOCAL_VARIABLE_TAG: u32 = 24;
        let tuple_names: BTreeMap<u32, Vec<String>> =
            (1..=metadata.rows(CUSTOM_DEBUG_INFORMATION)).filter_map(|row| custom_information(&metadata, row, LOCAL_VARIABLE_TAG, TUPLE_NAMES)).map(|(variable, blob)| (variable, tuple_element_names(blob))).collect();
        let scopes = (1..=metadata.rows(LOCAL_SCOPE)).map(|row| local_scope(&metadata, row, &tuple_names)).collect();
        let mut pdb = Pdb { documents, methods, scopes, id: metadata.pdb_id(), namespaces: vec![Vec::new()], ..Pdb::default() };
        pdb.read_namespaces(&metadata);
        // Parent は HasCustomDebugInformation: 下位 5 ビットが種類（0 = MethodDef）。
        for row in 1..=metadata.rows(CUSTOM_DEBUG_INFORMATION) {
            if let Some((method, blob)) = custom_information(&metadata, row, 0, ASYNC_STEPPING) {
                pdb.awaits.insert(method, async_stepping(blob)?);
            }
        }
        pdb.read_state_machines(&metadata, 1);
        Ok(pdb)
    }

    /// using で取り込まれた名前空間（ImportScope）のうち、まだ読んでいない行。親（ファイル、プロジェクト全体）の分も、子に含める。
    fn read_namespaces(&mut self, metadata: &Metadata) {
        for row in self.namespaces.len() as u32..=metadata.rows(IMPORT_SCOPE) {
            let mut found = metadata.blob(metadata.cell(IMPORT_SCOPE, row, 1)).ok().map(|blob| imported_namespaces(metadata, blob)).unwrap_or_default();
            // 親は、自分より前の行にある。
            let parent = metadata.cell(IMPORT_SCOPE, row, 0) as usize;
            if let Some(inherited) = self.namespaces.get(parent).filter(|_| parent < row as usize) {
                found.extend(inherited.iter().cloned());
            }
            self.namespaces.push(found);
        }
    }

    /// StateMachineMethod 表（0x36: MoveNextMethod、KickoffMethod）の `first` 行から。
    fn read_state_machines(&mut self, metadata: &Metadata, first: u32) {
        for row in first..=metadata.rows(STATE_MACHINE_METHOD) {
            self.state_machines.insert(metadata.cell(STATE_MACHINE_METHOD, row, 1), metadata.cell(STATE_MACHINE_METHOD, row, 0));
        }
    }

    /// Edit and Continue の PDB の差分を重ねる。差分で変わったメソッドと足されたメソッドに、新しい版を作る。
    /// `version_of` は、メソッドの（当てたあとの）ランタイムの版の番号。変えたメソッドの MethodDef トークンを返す。
    ///
    /// PDB の差分は、メタデータの差分と違い、ほぼ単体の PDB の形をしている（Roslyn の EmitDifference で確かめた）。
    /// ヒープ、Document、ImportScope、LocalScope、LocalVariable は差分の中だけの番号で、EncMap に載るのは
    /// MethodDebugInformation（行番号が MethodDef の行番号）だけ。LocalScope の Method と StateMachineMethod は、
    /// MethodDef の行番号そのもの。そこで差分を単体で読み、Document と ImportScope の番号をこちらの番号に付け替える。
    pub fn apply_delta(&mut self, bytes: Vec<u8>, version_of: &dyn Fn(u32) -> u32) -> Result<Vec<u32>, String> {
        let delta = Metadata::parse(bytes)?;
        if delta.rows(ENC_MAP) == 0 {
            return Err(tr!("Not an Edit and Continue PDB delta (there is no EncMap)"));
        }
        // Document: 差分の行 → こちらの行。同じパスがあればそれ、無ければ足す。
        let mut documents = vec![0u32];
        for row in 1..=delta.rows(DOCUMENT) {
            let name = document_name(&delta, delta.cell(DOCUMENT, row, 0))?;
            let ours = match self.documents.iter().position(|d| *d == name) {
                Some(index) => index as u32 + 1,
                None => {
                    self.documents.push(name);
                    self.documents.len() as u32
                }
            };
            documents.push(ours);
        }
        // ImportScope: 差分の行を、こちらの並びの後ろに足す（親は差分の中の、自分より前の行）。
        let first_import = self.namespaces.len() as u32;
        let mut imports: Vec<Vec<String>> = vec![Vec::new()];
        for row in 1..=delta.rows(IMPORT_SCOPE) {
            let mut found = delta.blob(delta.cell(IMPORT_SCOPE, row, 1)).ok().map(|blob| imported_namespaces(&delta, blob)).unwrap_or_default();
            let parent = delta.cell(IMPORT_SCOPE, row, 0) as usize;
            if let Some(inherited) = imports.get(parent).filter(|_| parent < row as usize) {
                found.extend(inherited.iter().cloned());
            }
            imports.push(found);
        }
        self.namespaces.extend(imports.into_iter().skip(1));
        const LOCAL_VARIABLE_TAG: u32 = 24;
        let tuple_names: BTreeMap<u32, Vec<String>> = (1..=delta.rows(CUSTOM_DEBUG_INFORMATION))
            .filter_map(|row| custom_information(&delta, row, LOCAL_VARIABLE_TAG, TUPLE_NAMES))
            .map(|(variable, blob)| (variable, tuple_element_names(blob)))
            .collect();
        let scopes: Vec<LocalScope> = (1..=delta.rows(LOCAL_SCOPE))
            .map(|row| {
                let mut scope = local_scope(&delta, row, &tuple_names);
                scope.imports = if scope.imports == 0 { 0 } else { first_import + scope.imports - 1 };
                scope
            })
            .collect();
        let mut awaits: BTreeMap<u32, Vec<Await>> = BTreeMap::new();
        for row in 1..=delta.rows(CUSTOM_DEBUG_INFORMATION) {
            if let Some((method, blob)) = custom_information(&delta, row, 0, ASYNC_STEPPING) {
                awaits.insert(method, async_stepping(blob)?);
            }
        }
        self.read_state_machines(&delta, 1);
        // MethodDebugInformation の k 行目は、EncMap の k 番目のトークンのメソッド。
        let rows: Vec<u32> = (1..=delta.rows(ENC_MAP)).map(|row| delta.cell(ENC_MAP, row, 0)).filter(|token| (token >> 24) as usize == METHOD_DEBUG_INFORMATION).map(|token| token & 0x00FF_FFFF).collect();
        let mut methods = Vec::with_capacity(rows.len());
        for (index, &row) in rows.iter().enumerate() {
            let mut points = method_points(&delta, index as u32 + 1)?;
            for point in &mut points {
                point.document = documents.get(point.document as usize).copied().unwrap_or(0);
            }
            let token = 0x0600_0000 | row;
            let revision = Revision {
                version: version_of(token),
                points,
                scopes: scopes.iter().filter(|s| s.method == row).cloned().collect(),
                awaits: awaits.remove(&row).unwrap_or_default(),
                shift: 0,
            };
            self.revisions.entry(row).or_default().push(revision);
            methods.push(token);
        }
        Ok(methods)
    }

    /// 中身は同じまま行だけが動いたメソッドの、すべての版の行を `delta` 行ずらす。
    pub fn shift_lines(&mut self, method: u32, delta: i32) {
        let row = method & 0x00FF_FFFF;
        *self.base_shifts.entry(row).or_default() += delta;
        for revision in self.revisions.get_mut(&row).into_iter().flatten() {
            revision.shift += delta;
        }
    }

    /// メソッドの版 `version` の差分の版（無ければファイルの版を使う）。
    fn revision(&self, row: u32, version: u32) -> Option<&Revision> {
        self.revisions.get(&row)?.iter().rev().find(|r| r.version <= version)
    }

    /// メソッドの版 `version` のシーケンスポイントと、その行のずれ。
    fn points(&self, method: u32, version: u32) -> Option<(&[SequencePoint], i32)> {
        let row = method & 0x00FF_FFFF;
        if let Some(revision) = self.revision(row, version) {
            return Some((&revision.points, revision.shift));
        }
        let points = self.methods.get(row.checked_sub(1)? as usize)?;
        Some((points, self.base_shifts.get(&row).copied().unwrap_or(0)))
    }

    /// メソッドの版 `version` のスコープ（ローカル変数）。
    fn scopes(&self, method: u32, version: u32) -> &[LocalScope] {
        let row = method & 0x00FF_FFFF;
        if let Some(revision) = self.revision(row, version) {
            return &revision.scopes;
        }
        let first = self.scopes.partition_point(|s| s.method < row);
        let count = self.scopes[first..].iter().take_while(|s| s.method == row).count();
        &self.scopes[first..first + count]
    }

    /// シーケンスポイントを持つメソッドの行（ファイルの分と、差分で足された分）。
    fn method_rows(&self) -> impl Iterator<Item = u32> + '_ {
        let base = self.methods.len() as u32;
        (1..=base).chain(self.revisions.keys().copied().filter(move |&row| row > base))
    }

    /// メソッドの、いちばん新しい版の最初の見える系列点（差分の一覧から、メソッドのソースを開くのに使う）。
    pub fn first_visible_point(&self, method: u32) -> Option<SequencePoint> {
        let (points, shift) = self.points(method, LATEST)?;
        points.iter().find(|p| !p.is_hidden()).map(|p| shifted(p, shift))
    }

    /// ファイルの版（差分を当てる前）のメソッドごとの「MethodDef の行番号、シーケンスポイント」。系列点の無いメソッドは除く。
    pub fn base_methods(&self) -> impl Iterator<Item = (u32, &[SequencePoint])> + '_ {
        self.methods.iter().enumerate().filter(|(_, points)| !points.is_empty()).map(|(index, points)| (index as u32 + 1, points.as_slice()))
    }

    /// async メソッド（かイテレーター）なら、本体（状態機械の MoveNext）の MethodDef トークンと、その最初の見える行の
    /// IL オフセット（メソッドの `{`）。そうでなければ `None`。本体は、いちばん新しい版で見る。
    pub fn state_machine_start(&self, kickoff: u32) -> Option<(u32, u32)> {
        let move_next = 0x0600_0000 | *self.state_machines.get(&(kickoff & 0x00FF_FFFF))?;
        let (points, _) = self.points(move_next, LATEST)?;
        let first = points.iter().find(|p| !p.is_hidden())?;
        Some((move_next, first.il_offset))
    }

    /// メソッドのコードから見える、`using` で取り込まれた名前空間（内側の using が先）。
    pub fn namespaces(&self, method: u32, version: u32) -> &[String] {
        self.scopes(method, version).first().and_then(|s| self.namespaces.get(s.imports as usize)).map_or(&[], Vec::as_slice)
    }

    /// `path` に当たる Document の行番号（1 始まり）。
    pub fn find_document(&self, path: &str) -> Option<u32> {
        let wanted = normalize(path);
        self.documents.iter().position(|d| normalize(d) == wanted).map(|i| i as u32 + 1)
    }

    /// 文書 `document` の `first` 行から `last` 行までで始まるメソッド（いちばん新しい版の、最初の見える行で見る）。
    /// Edit and Continue の補助が返す行の範囲（メンバーの宣言の範囲）を、メソッドに結びつけるのに使う。
    pub fn methods_between(&self, document: u32, first: u32, last: u32) -> Vec<u32> {
        self.method_rows()
            .filter(|&row| {
                let Some((points, shift)) = self.points(row, LATEST) else { return false };
                points.iter().map(|p| shifted(p, shift)).find(|p| !p.is_hidden() && p.document == document).is_some_and(|p| (first..=last).contains(&p.start_line))
            })
            .map(|row| 0x0600_0000 | row)
            .collect()
    }

    /// メソッドの版 `version` で `il_offset` を含む文（乗り換え元）。見えるシーケンスポイントと、その IL の終わり（次の
    /// シーケンスポイント。隠しも含む。最後の文は `None`）。隠し区間の中にいるときは `None`。
    pub fn statement_span(&self, method: u32, version: u32, il_offset: u32) -> Option<(SequencePoint, Option<u32>)> {
        let (points, shift) = self.points(method, version)?;
        let index = points.iter().rposition(|p| p.il_offset <= il_offset)?;
        let point = shifted(&points[index], shift);
        (!point.is_hidden()).then(|| (point, points.get(index + 1).map(|p| p.il_offset)))
    }

    /// メソッドの版 `version` で、`line` 行 `column` 列から始まる文（乗り換え先）。IL の始まりと終わり（`statement_span` と同じ）。
    /// 列まで見るのは、`for (int i = 0; i < n; i++)` のように 1 行に文がいくつもあるため。
    pub fn statement_at(&self, method: u32, version: u32, line: u32, column: u32) -> Option<(u32, Option<u32>)> {
        let (points, shift) = self.points(method, version)?;
        let index = points.iter().position(|p| {
            let p = shifted(p, shift);
            !p.is_hidden() && p.start_line == line && p.start_column == column
        })?;
        Some((points[index].il_offset, points.get(index + 1).map(|p| p.il_offset)))
    }

    /// ファイルの `line` 行目、または、その先でコードのある最初の行。各メソッドの、いちばん新しい版で探す。
    pub fn resolve_line(&self, document: u32, line: u32) -> Option<Location> {
        let mut best: Option<Location> = None;
        for row in self.method_rows() {
            let Some((points, shift)) = self.points(row, LATEST) else { continue };
            for p in points.iter().map(|p| shifted(p, shift)) {
                if p.document != document || p.is_hidden() || p.start_line < line {
                    continue;
                }
                // 行が近いものを選ぶ。同じ行なら IL の若い方（行の先頭の文）。
                let better = match &best {
                    None => true,
                    Some(b) => p.start_line < b.line || (p.start_line == b.line && row == b.method & 0x00FF_FFFF && p.il_offset < b.il_offset),
                };
                if better {
                    best = Some(located(0x0600_0000 | row, &p));
                }
            }
        }
        best
    }

    /// `resolve_line` と同じだが、メソッド `method` の版 `version` の中だけで探す（実行する場所を動かすとき。動かせるのは
    /// 同じメソッドの中だけ）。
    pub fn resolve_line_in(&self, document: u32, line: u32, method: u32, version: u32) -> Option<Location> {
        let (points, shift) = self.points(method, version)?;
        points
            .iter()
            .map(|p| shifted(p, shift))
            .filter(|p| p.document == document && !p.is_hidden() && p.start_line >= line)
            // 行が近いもの。同じ行なら IL の若い方（行の先頭の文）。
            .min_by_key(|p| (p.start_line, p.il_offset))
            .map(|p| located(method, &p))
    }

    /// `line` 行の、`column` 列を含む文。隙間の列は、その列以前で始まるいちばん遅い文。その行に文が無ければ
    /// `resolve_line`（先の最初の行の、最初の文）。列の無い要求は `resolve_line` のまま。
    pub fn resolve_column(&self, document: u32, line: u32, column: u32) -> Option<Location> {
        let mut on_line = Vec::new();
        for row in self.method_rows() {
            let Some((points, shift)) = self.points(row, LATEST) else { continue };
            for point in points.iter().map(|p| shifted(p, shift)) {
                if point.document == document && !point.is_hidden() && point.start_line == line {
                    on_line.push((0x0600_0000 | row, point));
                }
            }
        }
        if on_line.is_empty() {
            return self.resolve_line(document, line);
        }
        let contains = |point: &SequencePoint| {
            let end = point.end_column.max(point.start_column.saturating_add(1));
            point.start_column <= column && column < end
        };
        let chosen = on_line.iter().filter(|(_, point)| contains(point)).max_by_key(|(_, point)| point.start_column).or_else(|| {
            on_line.iter().filter(|(_, point)| point.start_column <= column).max_by_key(|(_, point)| point.start_column)
        }).or_else(|| on_line.iter().min_by_key(|(_, point)| (point.start_column, point.il_offset)));
        chosen.map(|(method, point)| located(*method, point))
    }

    /// メソッドの版 `version` の IL オフセットに当たるシーケンスポイント（そのオフセット以前で最後のもの）。
    pub fn find_point(&self, method: u32, il_offset: u32, version: u32) -> Option<SequencePoint> {
        let (points, shift) = self.points(method, version)?;
        points.iter().rev().find(|p| p.il_offset <= il_offset).map(|p| shifted(p, shift))
    }

    /// コールスタックに見せる位置: そのオフセット以前で最後の、見えるシーケンスポイント。隠し行（コンパイラが作った
    /// コード。async メソッドの await の続きなど）の上にいるときは、その前の行に見せる（netcoredbg と同じ）。
    pub fn find_visible_point(&self, method: u32, il_offset: u32, version: u32) -> Option<SequencePoint> {
        let (points, shift) = self.points(method, version)?;
        points.iter().rev().find(|p| p.il_offset <= il_offset && !p.is_hidden()).map(|p| shifted(p, shift))
    }

    /// メソッドの版 `version` の、文書 `document` の中の行の範囲（見えるシーケンスポイントの最初の行と最後の行）。
    /// 画面が、止まった行までに通った行へ変数の値を出す範囲に使う。
    pub fn method_lines(&self, method: u32, version: u32, document: u32) -> Option<(u32, u32)> {
        let (points, shift) = self.points(method, version)?;
        points.iter().filter(|p| !p.is_hidden() && p.document == document).map(|p| shifted(p, shift)).fold(None, |range, p| {
            Some(match range {
                None => (p.start_line, p.end_line),
                Some((first, last)) => (first.min(p.start_line), last.max(p.end_line)),
            })
        })
    }

    /// メソッドの版 `version` の `il_offset` の場所で見えるローカル変数。外側のスコープの変数が先。
    pub fn locals(&self, method: u32, il_offset: u32, version: u32) -> Vec<&LocalVariable> {
        self.scopes(method, version).iter().filter(|s| s.range.contains(&il_offset)).flat_map(|s| &s.variables).collect()
    }

    /// `il_offset` を含む文の IL の範囲（ステップで「この範囲を出るまで進む」のに使う）。
    ///
    /// 始まりは、その場所以前で最後の見えるシーケンスポイント。終わりは、その先の最初の見えるシーケンスポイント
    /// （無ければ `None` ＝ メソッドの終わりまで）。隠しシーケンスポイントの区間は、前の文の続きとして範囲に含める。
    /// 見える行より前（メソッドの先頭の隠し区間）にいるときは、いまの場所から始める。
    pub fn step_range(&self, method: u32, il_offset: u32, version: u32) -> Option<(u32, Option<u32>)> {
        let (points, _) = self.points(method, version)?;
        let mut visible = points.iter().filter(|p| !p.is_hidden()).peekable();
        visible.peek()?;
        let mut start = il_offset;
        for point in visible {
            if point.il_offset > il_offset {
                return Some((start, Some(point.il_offset)));
            }
            start = point.il_offset;
        }
        Some((start, None))
    }

    /// メソッドの版 `version` の中の `await`。async メソッドの本体でなければ空。
    pub fn awaits(&self, method: u32, version: u32) -> &[Await] {
        let row = method & 0x00FF_FFFF;
        if let Some(revision) = self.revision(row, version) {
            return &revision.awaits;
        }
        self.awaits.get(&row).map_or(&[], Vec::as_slice)
    }

    pub fn document(&self, row: u32) -> Option<&str> {
        self.documents.get(row.checked_sub(1)? as usize).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 小さな Portable PDB を手で組み立てる。Document 1 つ、メソッド 2 つ。
    fn compressed(value: u32) -> Vec<u8> {
        assert!(value < 0x80);
        vec![value as u8]
    }
    fn signed(value: i32) -> Vec<u8> {
        // 1 バイトで足りる範囲（-64..=63）だけ。
        assert!((-64..=63).contains(&value));
        let raw = ((value << 1) & 0x7F) as u8 | (value < 0) as u8;
        vec![raw]
    }

    fn sample() -> Vec<u8> {
        // #Blob: [0] 空、名前の部分、Document 名、シーケンスポイント 2 つ。
        let mut blobs = vec![0u8];
        let mut add = |bytes: &[u8]| {
            let at = blobs.len() as u8;
            blobs.push(bytes.len() as u8);
            blobs.extend_from_slice(bytes);
            at
        };
        let part_c = add(b"C:");
        let part_src = add(b"src");
        let part_file = add(b"Program.cs");
        let name = add(&[b'\\', part_c, part_src, part_file]);

        // メソッド 1: IL 0 → 10 行目、IL 1 → 隠し、IL 5 → 12 行目。
        let mut m1 = compressed(0); // local sig
        m1.extend([compressed(0), compressed(0), compressed(4), compressed(10), compressed(5)].concat()); // IL0 line10 col5..9
        m1.extend([compressed(1), compressed(0), compressed(0)].concat()); // hidden at IL1
        m1.extend([compressed(4), compressed(0), compressed(6), signed(2), signed(0)].concat()); // IL5 line12
        let points1 = add(&m1);
        // メソッド 2: IL 0 → 20 行目から 21 行目まで。
        let mut m2 = compressed(0);
        m2.extend([compressed(0), compressed(1), signed(3), compressed(20), compressed(9)].concat());
        let points2 = add(&m2);

        // #~: Document 1 行、MethodDebugInformation 2 行。添字はすべて 2 バイト。
        let mut tables = vec![0, 0, 0, 0, 2, 0, 0, 1];
        tables.extend(((1u64 << 0x30) | (1u64 << 0x31)).to_le_bytes());
        tables.extend(0u64.to_le_bytes());
        tables.extend(1u32.to_le_bytes());
        tables.extend(2u32.to_le_bytes());
        tables.extend([name, 0, 0, 0, 0, 0, 0, 0]); // Document: name, hashAlg, hash, language
        tables.extend([1, 0, points1, 0]);
        tables.extend([1, 0, points2, 0]);

        let version = b"PDB v1.0\0\0\0\0";
        let mut file = Vec::new();
        file.extend(0x424A_5342u32.to_le_bytes());
        file.extend([1, 0, 1, 0, 0, 0, 0, 0]);
        file.extend((version.len() as u32).to_le_bytes());
        file.extend(version);
        file.extend([0, 0, 2, 0]); // flags, 2 streams
        let header_len = file.len() + (8 + 4) + (8 + 8);
        file.extend((header_len as u32).to_le_bytes());
        file.extend((tables.len() as u32).to_le_bytes());
        file.extend(b"#~\0\0");
        file.extend(((header_len + tables.len()) as u32).to_le_bytes());
        file.extend((blobs.len() as u32).to_le_bytes());
        file.extend(b"#Blob\0\0\0");
        file.extend(tables);
        file.extend(blobs);
        file
    }

    #[test]
    fn reads_document_names_with_their_separator() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.documents, ["C:\\src\\Program.cs"]);
    }

    #[test]
    fn finds_a_document_regardless_of_case_and_slashes() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.find_document("c:/SRC/program.cs"), Some(1));
        assert_eq!(pdb.find_document("C:\\src\\Other.cs"), None);
    }

    #[test]
    fn resolves_a_line_to_its_method_and_il_offset() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.resolve_line(1, 10).map(|l| (l.method, l.il_offset, l.line)), Some((0x0600_0001, 0, 10)));
        assert_eq!(pdb.resolve_line(1, 12).map(|l| (l.method, l.il_offset, l.line)), Some((0x0600_0001, 5, 12)));
        assert_eq!(pdb.resolve_line(1, 20).map(|l| (l.method, l.il_offset, l.line)), Some((0x0600_0002, 0, 20)));
        let inside = pdb.resolve_column(1, 12, 7).unwrap();
        assert_eq!((inside.il_offset, inside.column, inside.end_column), (5, 5, 11));
        assert_eq!(pdb.resolve_column(1, 12, 1).unwrap().il_offset, 5, "the column is before every statement, so the first one");
        assert_eq!(pdb.resolve_column(1, 11, 3).unwrap().line, 12, "a line without code moves forward, and the column is not applied there");
    }

    #[test]
    fn a_method_spans_the_lines_of_its_visible_points() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.method_lines(0x0600_0001, LATEST, 1).map(|(first, _)| first), Some(10));
        assert!(pdb.method_lines(0x0600_0001, LATEST, 1).is_some_and(|(_, last)| (12..20).contains(&last)));
        assert_eq!(pdb.method_lines(0x0600_0002, LATEST, 1).map(|(first, _)| first), Some(20));
        assert_eq!(pdb.method_lines(0x0600_0001, LATEST, 2), None, "another document");
    }

    #[test]
    fn a_line_without_code_moves_to_the_next_line_that_has_some() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.resolve_line(1, 11).unwrap().line, 12);
        assert_eq!(pdb.resolve_line(1, 13).unwrap().line, 20);
        assert_eq!(pdb.resolve_line(1, 21), None);
    }

    #[test]
    fn maps_an_il_offset_back_to_its_line() {
        let pdb = Pdb::parse(&sample()).unwrap();
        let point = pdb.find_point(0x0600_0001, 7, LATEST).unwrap();
        assert_eq!((point.start_line, point.start_column, point.end_line, point.end_column), (12, 5, 12, 11));
        assert!(pdb.find_point(0x0600_0001, 2, LATEST).unwrap().is_hidden());
        let second = pdb.find_point(0x0600_0002, 0, LATEST).unwrap();
        assert_eq!((second.start_line, second.end_line, second.start_column, second.end_column), (20, 21, 9, 12));
        assert!(pdb.find_point(0x0600_0003, 0, LATEST).is_none());
    }

    /// メソッド 1: 全体（IL 0〜20）に `a` と隠し変数、内側（IL 5〜10）に `b`。メソッド 2: `c`。
    fn with_locals() -> Pdb {
        use crate::debugger::metadata::tests::Builder;
        let mut b = Builder::new();
        let (a, hidden, inner, c) = (b.string("a"), b.string("CS$tmp"), b.string("b"), b.string("c"));
        // LocalScope: Method, ImportScope, VariableList, ConstantList, StartOffset(4), Length(4)
        b.table(LOCAL_SCOPE, &[&[1, 0, 1, 1, 0, 0, 20, 0], &[1, 0, 3, 1, 5, 0, 5, 0], &[2, 0, 4, 1, 0, 0, 8, 0]]);
        // LocalVariable: Attributes, Index, Name
        b.table(LOCAL_VARIABLE, &[&[0, 0, a], &[1, 1, hidden], &[0, 2, inner], &[0, 0, c]]);
        Pdb::parse(&b.build()).unwrap()
    }

    fn names(locals: Vec<&LocalVariable>) -> Vec<(u32, &str)> {
        locals.into_iter().map(|l| (l.index, l.name.as_str())).collect()
    }

    #[test]
    fn locals_are_those_whose_scope_contains_the_il_offset() {
        let pdb = with_locals();
        assert_eq!(names(pdb.locals(0x0600_0001, 0, LATEST)), [(0, "a")]);
        assert_eq!(names(pdb.locals(0x0600_0001, 5, LATEST)), [(0, "a"), (2, "b")]);
        assert_eq!(names(pdb.locals(0x0600_0001, 10, LATEST)), [(0, "a")], "スコープの終わりは含まない");
        assert_eq!(names(pdb.locals(0x0600_0001, 20, LATEST)), []);
    }

    #[test]
    fn locals_of_another_method_are_not_mixed_in() {
        let pdb = with_locals();
        assert_eq!(names(pdb.locals(0x0600_0002, 5, LATEST)), [(0, "c")]);
        assert_eq!(names(pdb.locals(0x0600_0003, 0, LATEST)), []);
    }

    #[test]
    fn compiler_generated_locals_are_hidden() {
        assert!(with_locals().locals(0x0600_0001, 1, LATEST).iter().all(|l| l.name != "CS$tmp"));
    }

    #[test]
    fn a_line_is_resolved_only_inside_the_given_method() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.resolve_line_in(1, 12, 0x0600_0001, LATEST).map(|l| (l.method, l.il_offset, l.line)), Some((0x0600_0001, 5, 12)));
        assert_eq!(pdb.resolve_line_in(1, 11, 0x0600_0001, LATEST).map(|l| l.line), Some(12), "コードの無い行は次の行へ");
        assert_eq!(pdb.resolve_line_in(1, 20, 0x0600_0001, LATEST), None, "20 行目は別のメソッド");
        assert_eq!(pdb.resolve_line_in(1, 20, 0x0600_0002, LATEST).map(|l| l.il_offset), Some(0));
    }

    #[test]
    fn the_async_stepping_blob_lists_where_each_await_yields_and_resumes() {
        // catch ハンドラー 0x40、await 2 つ（再開するメソッドは行 3）。
        let blob = [0x40, 0, 0, 0, 0x10, 0, 0, 0, 0x30, 0, 0, 0, 3, 0x50, 0, 0, 0, 0x70, 0, 0, 0, 3];
        assert_eq!(async_stepping(&blob).unwrap(), [Await { yield_offset: 0x10, resume_offset: 0x30 }, Await { yield_offset: 0x50, resume_offset: 0x70 }]);
        assert!(async_stepping(&blob[..10]).is_err(), "途中で切れた blob");
        assert_eq!(async_stepping(&[0, 0, 0, 0]).unwrap(), []);
    }

    #[test]
    fn a_method_that_is_not_async_has_no_awaits() {
        assert!(Pdb::parse(&sample()).unwrap().awaits(0x0600_0001, LATEST).is_empty());
    }

    #[test]
    fn a_step_covers_the_statement_and_the_hidden_code_after_it() {
        let pdb = Pdb::parse(&sample()).unwrap();
        // メソッド 1: IL 0 が 10 行目、IL 1 から隠し、IL 5 が 12 行目。
        assert_eq!(pdb.step_range(0x0600_0001, 0, LATEST), Some((0, Some(5))));
        assert_eq!(pdb.step_range(0x0600_0001, 3, LATEST), Some((0, Some(5))), "隠し区間は前の文の続き");
        assert_eq!(pdb.step_range(0x0600_0001, 7, LATEST), Some((5, None)), "最後の文はメソッドの終わりまで");
    }

    #[test]
    fn a_method_without_visible_lines_has_no_step_range() {
        let pdb = Pdb::parse(&sample()).unwrap();
        assert_eq!(pdb.step_range(0x0600_0003, 0, LATEST), None);
    }

    #[test]
    fn says_how_to_fix_a_windows_pdb() {
        let error = Pdb::parse(b"Microsoft C/C++ MSF 7.00\r\n\x1aDS\0\0\0").unwrap_err();
        assert!(error.contains("portable"), "{error}");
    }

    #[test]
    fn a_truncated_file_is_an_error_not_a_panic() {
        let full = sample();
        for len in [0, 3, 20, full.len() / 2, full.len() - 1] {
            assert!(Pdb::parse(&full[..len]).is_err(), "len {len}");
        }
    }

    // Edit and Continue の差分（tests/fixtures/enc）: EncTarget.pdb に、Roslyn が書いた PDB の差分 EncTarget.1.pdb
    // （Compute と Main の更新、Helper の追加。ソースは Program.1.cs.txt）を、版 2 として重ねる。
    fn enc_fixture() -> (Pdb, Vec<u32>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enc");
        let mut pdb = Pdb::open(&dir.join("EncTarget.pdb")).unwrap();
        let methods = pdb.apply_delta(std::fs::read(dir.join("EncTarget.1.pdb")).unwrap(), &|_| 2).unwrap();
        (pdb, methods)
    }

    /// 行 `line` にあるメソッド（いちばん新しい版）。
    fn method_at(pdb: &Pdb, line: u32) -> u32 {
        let document = pdb.find_document(&pdb.documents.iter().find(|d| d.ends_with("Program.cs")).unwrap().clone()).unwrap();
        pdb.resolve_line(document, line).unwrap().method
    }

    #[test]
    fn a_delta_gives_the_changed_methods_a_new_version() {
        let (pdb, methods) = enc_fixture();
        let (compute, main) = (method_at(&pdb, 9), method_at(&pdb, 18));
        assert!(methods.contains(&compute) && methods.contains(&main), "{methods:x?}");
        // Main の版 1（古いコードを実行中のフレーム）と版 2 は、IL が違っても同じ行を指す。
        let old = pdb.find_visible_point(main, 0, 1).unwrap();
        let new = pdb.find_visible_point(main, 0, 2).unwrap();
        assert_eq!((old.start_line, new.start_line), (13, 13));
        let names = |version| pdb.locals(main, 0x10, version).iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        assert!(names(2).contains(&"total".to_string()), "{:?}", names(2));
    }

    #[test]
    fn a_method_added_by_a_delta_can_be_found_by_its_line() {
        let (pdb, methods) = enc_fixture();
        let helper = method_at(&pdb, 25);
        assert!(methods.contains(&helper));
        assert_ne!(helper, method_at(&pdb, 9));
        assert_eq!(pdb.find_visible_point(helper, 0, LATEST).unwrap().start_line, 24);
    }

    #[test]
    fn a_method_that_only_moved_is_shifted_in_every_version() {
        let (mut pdb, _) = enc_fixture();
        let compute = method_at(&pdb, 9);
        pdb.shift_lines(compute, 2);
        assert_eq!(pdb.find_point(compute, 0, 1).unwrap().start_line, 10, "ファイルの版（8 行目の {{ ）");
        assert_eq!(pdb.find_point(compute, 0, 2).unwrap().start_line, 10, "差分の版");
        assert_eq!(method_at(&pdb, 11), compute, "ずらした行でブレークポイントを置ける");
    }
}
