//! プログラムにまだ無いメンバーの並びの匿名型（`new { Zzz = 1 }`）を入れたアセンブリを、ファイルとして書き出す。
//!
//! 式の評価は Roslyn を持たない（docs/DECISIONS.md D-025）ので、匿名型は、C# のコンパイラがプログラムの中に作ったもの
//! しか使えなかった。そこで、コンパイラが作るのと同じ形の型を 1 つだけ入れた小さなアセンブリをここで作り、対象に
//! `Assembly.LoadFrom` で読み込ませる（D-038）。型の形は、C# のコンパイラの匿名型と同じ:
//! - 名前は `<>f__AnonymousType0`、メンバーごとに型引数（`<名前>j__TPar`）を持つジェネリック型（`<>f__AnonymousType0<int>`）。
//! - 読み取り専用のフィールド（`<名前>i__Field`）と、その getter だけのプロパティ。全部を受け取るコンストラクター。
//! - `ToString`（`{ A = 1, B = x }`）、`Equals`（型が同じで、メンバーが `EqualityComparer<T>.Default` で等しい）、
//!   `GetHashCode`（`EqualityComparer<T>.Default.GetHashCode` を `-1521134295` で混ぜる）。
//! - `[DebuggerDisplay(@"\{ A = {A} }", Type = "<Anonymous Type>")]`。
//!
//! 参照するアセンブリは mscorlib だけ。.NET では、mscorlib の転送で System.Private.CoreLib の型になる。
//! ファイルは IL だけの PE32（AnyCPU）で、x86 と x64 のどちらの対象にも読み込める。

/// メタデータの表の番号。
const MODULE: usize = 0x00;
const TYPE_REF: usize = 0x01;
const TYPE_DEF: usize = 0x02;
const FIELD: usize = 0x04;
const METHOD_DEF: usize = 0x06;
const MEMBER_REF: usize = 0x0A;
const CUSTOM_ATTRIBUTE: usize = 0x0C;
const PROPERTY_MAP: usize = 0x15;
const PROPERTY: usize = 0x17;
const METHOD_SEMANTICS: usize = 0x18;
const TYPE_SPEC: usize = 0x1B;
const ASSEMBLY: usize = 0x20;
const ASSEMBLY_REF: usize = 0x23;
const GENERIC_PARAM: usize = 0x2A;

/// 書き出したアセンブリを置くフォルダー（`%TEMP%\netdbg-anonymous`）。
pub fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join("netdbg-anonymous")
}

/// このエンジンのプロセスが書き出したアセンブリのファイル名の頭。
pub fn file_prefix() -> String {
    format!("NetdbgAnonymous_{}_", std::process::id())
}

/// このエンジンが書き出したアセンブリを消す（対象が終わったとき）。対象がまだ読み込んでいるもの（切り離したとき）は
/// 消せないので、そのまま残す。
pub fn remove_written() {
    let Ok(entries) = std::fs::read_dir(directory()) else { return };
    let prefix = file_prefix();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 匿名型の型の名前（アセンブリの中で 1 つだけなので、番号は 0）。メタデータの名前には、型引数の数が付く（C# のコンパイラと同じ）。
const TYPE_NAME: &str = "<>f__AnonymousType0";

/// 文字列、Blob、ユーザー文字列のヒープと、表の行。
#[derive(Default)]
struct Builder {
    strings: Vec<u8>,
    blobs: Vec<u8>,
    user_strings: Vec<u8>,
    /// 表ごとの行（列は、2 バイトの値と 4 バイトの値の並び）。
    rows: Vec<Vec<Vec<Cell>>>,
}

#[derive(Clone, Copy)]
enum Cell {
    U16(u16),
    U32(u32),
}

impl Builder {
    fn new() -> Builder {
        Builder { strings: vec![0], blobs: vec![0], user_strings: vec![0], rows: vec![Vec::new(); 64] }
    }

    fn string(&mut self, text: &str) -> Cell {
        if text.is_empty() {
            return Cell::U16(0);
        }
        let at = self.strings.len() as u16;
        self.strings.extend(text.as_bytes());
        self.strings.push(0);
        Cell::U16(at)
    }

    fn blob(&mut self, bytes: &[u8]) -> Cell {
        if bytes.is_empty() {
            return Cell::U16(0);
        }
        let at = self.blobs.len() as u16;
        compressed(&mut self.blobs, bytes.len() as u32);
        self.blobs.extend(bytes);
        Cell::U16(at)
    }

    /// ユーザー文字列（`ldstr` の文字列）。戻り値はトークン。
    fn user_string(&mut self, text: &str) -> u32 {
        let at = self.user_strings.len() as u32;
        let units: Vec<u16> = text.encode_utf16().collect();
        compressed(&mut self.user_strings, units.len() as u32 * 2 + 1);
        for unit in &units {
            self.user_strings.extend(unit.to_le_bytes());
        }
        // 終わりの 1 バイト: 0x80 以上の文字などがあれば 1（ここでは使わない文字だけなので 0）。
        self.user_strings.push(u8::from(units.iter().any(|&unit| unit >= 0x80)));
        0x7000_0000 | at
    }

    /// 行を足し、その行の番号（1 から）を返す。
    fn row(&mut self, table: usize, cells: Vec<Cell>) -> u32 {
        self.rows[table].push(cells);
        self.rows[table].len() as u32
    }

    /// `#~` ストリーム。
    fn tables(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(0u32.to_le_bytes());
        out.extend([2u8, 0, 0, 1]);
        let valid: u64 = (0..64).filter(|&table| !self.rows[table].is_empty()).fold(0, |mask, table| mask | 1 << table);
        out.extend(valid.to_le_bytes());
        // 並べ替えが要る表（CustomAttribute、GenericParam、MethodSemantics、PropertyMap）は、作る順で並んでいる。
        let sorted: u64 = 1 << CUSTOM_ATTRIBUTE | 1 << GENERIC_PARAM | 1 << METHOD_SEMANTICS | 1 << PROPERTY_MAP;
        out.extend(sorted.to_le_bytes());
        for table in 0..64 {
            if !self.rows[table].is_empty() {
                out.extend((self.rows[table].len() as u32).to_le_bytes());
            }
        }
        for table in 0..64 {
            for row in &self.rows[table] {
                for cell in row {
                    match cell {
                        Cell::U16(value) => out.extend(value.to_le_bytes()),
                        Cell::U32(value) => out.extend(value.to_le_bytes()),
                    }
                }
            }
        }
        pad(&mut out, 4);
        out
    }
}

/// ECMA-335 の圧縮した符号なし整数。
fn compressed(out: &mut Vec<u8>, value: u32) {
    match value {
        0..=0x7F => out.push(value as u8),
        0x80..=0x3FFF => out.extend([0x80 | (value >> 8) as u8, value as u8]),
        _ => out.extend([0xC0 | (value >> 24) as u8, (value >> 16) as u8, (value >> 8) as u8, value as u8]),
    }
}

fn pad(out: &mut Vec<u8>, alignment: usize) {
    while !out.len().is_multiple_of(alignment) {
        out.push(0);
    }
}

/// 型の符号（TypeDefOrRefOrSpecEncoded）。`tag` は 0 = TypeDef、1 = TypeRef、2 = TypeSpec。
fn type_encoded(out: &mut Vec<u8>, row: u32, tag: u32) {
    compressed(out, row << 2 | tag);
}

/// IL の命令の並び。分岐の飛び先は、あとで埋める。
#[derive(Default)]
struct Il {
    code: Vec<u8>,
    max_stack: u16,
}

impl Il {
    fn op(&mut self, bytes: &[u8]) -> &mut Il {
        self.code.extend(bytes);
        self
    }
    fn token(&mut self, op: u8, token: u32) -> &mut Il {
        self.code.push(op);
        self.code.extend(token.to_le_bytes());
        self
    }
    fn int(&mut self, value: i32) -> &mut Il {
        self.code.push(0x20);
        self.code.extend(value.to_le_bytes());
        self
    }
    fn load_argument(&mut self, index: u16) -> &mut Il {
        match index {
            0..=3 => self.op(&[0x02 + index as u8]),
            _ => self.op(&[0x0E, index as u8]),
        }
    }
    /// `brfalse`（長い形）。戻り値は、飛び先を書く場所。
    fn branch_false(&mut self) -> usize {
        self.code.push(0x39);
        self.code.extend(0i32.to_le_bytes());
        self.code.len() - 4
    }
    fn land(&mut self, at: usize) {
        let offset = self.code.len() as i32 - (at as i32 + 4);
        self.code[at..at + 4].copy_from_slice(&offset.to_le_bytes());
    }
    /// 太い形の頭を付けた、メソッドの本体。
    fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // Flags: CorILMethod_FatFormat（3）、頭の大きさ 3 dword。
        out.extend(0x3003u16.to_le_bytes());
        out.extend(self.max_stack.max(8).to_le_bytes());
        out.extend((self.code.len() as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(&self.code);
        out
    }
}

/// メンバーの名前の並び（書かれた順）の匿名型を入れたアセンブリのファイルの中身。`assembly` はアセンブリの名前
/// （読み込むたびに違う名前にする）。`stamp` は、モジュールの MVID に使う値。
pub fn assembly(assembly: &str, members: &[&str], stamp: u128) -> Vec<u8> {
    let count = members.len();
    let mut b = Builder::new();

    // 参照する型とメンバー。
    let module_name = b.string(&format!("{assembly}.dll"));
    b.row(MODULE, vec![Cell::U16(0), module_name, Cell::U16(1), Cell::U16(0), Cell::U16(0)]);
    let mscorlib_token = b.blob(&[0xB7, 0x7A, 0x5C, 0x56, 0x19, 0x34, 0xE0, 0x89]);
    let mscorlib_name = b.string("mscorlib");
    let mscorlib = b.row(ASSEMBLY_REF, vec![Cell::U16(4), Cell::U16(0), Cell::U16(0), Cell::U16(0), Cell::U32(0), mscorlib_token, mscorlib_name, Cell::U16(0), Cell::U16(0)]);
    let scope = Cell::U16((mscorlib << 2 | 2) as u16);
    let type_ref = |b: &mut Builder, namespace: &str, name: &str| {
        let (name, namespace) = (b.string(name), b.string(namespace));
        b.row(TYPE_REF, vec![scope, name, namespace])
    };
    let object = type_ref(&mut b, "System", "Object");
    let string = type_ref(&mut b, "System", "String");
    let format_provider = type_ref(&mut b, "System", "IFormatProvider");
    let comparer = type_ref(&mut b, "System.Collections.Generic", "EqualityComparer`1");
    let display = type_ref(&mut b, "System.Diagnostics", "DebuggerDisplayAttribute");

    // 型: <Module> と、匿名型。フィールドとメソッドは、このあとの順に並ぶ（1 から）。
    let empty = b.string("");
    let module_type = b.string("<Module>");
    b.row(TYPE_DEF, vec![Cell::U32(0), module_type, empty, Cell::U16(0), Cell::U16(1), Cell::U16(1)]);
    let type_name = b.string(&format!("{TYPE_NAME}`{count}"));
    let anonymous = b.row(TYPE_DEF, vec![Cell::U32(0x0010_0100), type_name, empty, Cell::U16((object << 2 | 1) as u16), Cell::U16(1), Cell::U16(1)]);

    // 型引数とフィールド。
    let mut field_names = Vec::new();
    for (index, member) in members.iter().enumerate() {
        let name = b.string(&format!("<{member}>j__TPar"));
        b.row(GENERIC_PARAM, vec![Cell::U16(index as u16), Cell::U16(0), Cell::U16((anonymous << 1) as u16), name]);
        let field_name = format!("<{member}>i__Field");
        let name = b.string(&field_name);
        let signature = b.blob(&[0x06, 0x13, index as u8]);
        b.row(FIELD, vec![Cell::U16(0x0021), name, signature]);
        field_names.push(field_name);
    }

    // 自分の型の、型引数を自分の型引数にしたもの（`<>f__AnonymousType0<!0, !1>`）。フィールドは、これを通して読み書きする。
    let mut self_spec = vec![0x15, 0x12];
    type_encoded(&mut self_spec, anonymous, 0);
    compressed(&mut self_spec, count as u32);
    for index in 0..count {
        self_spec.extend([0x13, index as u8]);
    }
    let self_spec = b.blob(&self_spec);
    let self_type = b.row(TYPE_SPEC, vec![self_spec]);
    let member_ref = |b: &mut Builder, parent: u32, tag: u32, name: &str, signature: &[u8]| {
        let (name, signature) = (b.string(name), b.blob(signature));
        0x0A00_0000 | b.row(MEMBER_REF, vec![Cell::U16((parent << 3 | tag) as u16), name, signature])
    };
    let mut field_refs = Vec::new();
    for (index, field) in field_names.iter().enumerate() {
        field_refs.push(member_ref(&mut b, self_type, 4, field, &[0x06, 0x13, index as u8]));
    }
    let object_constructor = member_ref(&mut b, object, 1, ".ctor", &[0x20, 0x00, 0x01]);
    let mut format_signature = vec![0x00, 0x03, 0x0E, 0x12];
    type_encoded(&mut format_signature, format_provider, 1);
    format_signature.extend([0x0E, 0x1D, 0x1C]);
    let format = member_ref(&mut b, string, 1, "Format", &format_signature);
    // メンバーごとの `EqualityComparer<!i>` と、その Default、Equals、GetHashCode。`box !i` に使う `!i`。
    let mut comparers = Vec::new();
    for index in 0..count {
        let mut spec = vec![0x15, 0x12];
        type_encoded(&mut spec, comparer, 1);
        spec.extend([0x01, 0x13, index as u8]);
        let spec = b.blob(&spec);
        let comparer_type = b.row(TYPE_SPEC, vec![spec]);
        let mut default_signature = vec![0x00, 0x00, 0x15, 0x12];
        type_encoded(&mut default_signature, comparer, 1);
        default_signature.extend([0x01, 0x13, 0x00]);
        let default = member_ref(&mut b, comparer_type, 4, "get_Default", &default_signature);
        let equals = member_ref(&mut b, comparer_type, 4, "Equals", &[0x20, 0x02, 0x02, 0x13, 0x00, 0x13, 0x00]);
        let hash = member_ref(&mut b, comparer_type, 4, "GetHashCode", &[0x20, 0x01, 0x08, 0x13, 0x00]);
        let variable = b.blob(&[0x13, index as u8]);
        let variable = 0x1B00_0000 | b.row(TYPE_SPEC, vec![variable]);
        comparers.push((default, equals, hash, variable));
    }
    let display_constructor = member_ref(&mut b, display, 1, ".ctor", &[0x20, 0x01, 0x01, 0x0E]);

    // メソッドの本体。
    let self_token = 0x1B00_0000 | self_type;
    let mut bodies: Vec<(String, u16, Vec<u8>, Il)> = Vec::new();
    // .ctor(!0, !1, …)
    let mut il = Il::default();
    il.op(&[0x02]).token(0x28, object_constructor);
    for (index, field) in field_refs.iter().enumerate() {
        il.op(&[0x02]).load_argument(index as u16 + 1).token(0x7D, *field);
    }
    il.op(&[0x2A]);
    let mut signature = vec![0x20];
    compressed(&mut signature, count as u32);
    signature.push(0x01);
    for index in 0..count {
        signature.extend([0x13, index as u8]);
    }
    bodies.push((".ctor".into(), 0x1886, signature, il));
    // get_名前()
    for (index, member) in members.iter().enumerate() {
        let mut il = Il::default();
        il.op(&[0x02]).token(0x7B, field_refs[index]).op(&[0x2A]);
        bodies.push((format!("get_{member}"), 0x0886, vec![0x20, 0x00, 0x13, index as u8], il));
    }
    // ToString(): string.Format(null, "{{ A = {0}, B = {1} }}", new object[] { A, B })
    let pattern = format!("{{{{ {} }}}}", members.iter().enumerate().map(|(index, member)| format!("{member} = {{{index}}}")).collect::<Vec<_>>().join(", "));
    let pattern = b.user_string(&pattern);
    let mut il = Il { max_stack: 8, ..Il::default() };
    il.op(&[0x14]).token(0x72, pattern).int(count as i32).token(0x8D, 0x0100_0000 | object);
    for (index, field) in field_refs.iter().enumerate() {
        il.op(&[0x25]).int(index as i32).op(&[0x02]).token(0x7B, *field).token(0x8C, comparers[index].3).op(&[0xA2]);
    }
    il.token(0x28, format).op(&[0x2A]);
    bodies.push(("ToString".into(), 0x00C6, vec![0x20, 0x00, 0x0E], il));
    // Equals(object): 同じ型で、メンバーがすべて EqualityComparer<T>.Default で等しい。
    let mut il = Il::default();
    il.op(&[0x03]).token(0x75, self_token);
    let mut to_false = vec![il.branch_false()];
    for (index, field) in field_refs.iter().enumerate() {
        il.token(0x28, comparers[index].0).op(&[0x02]).token(0x7B, *field).op(&[0x03]).token(0x74, self_token).token(0x7B, *field).token(0x6F, comparers[index].1);
        to_false.push(il.branch_false());
    }
    il.op(&[0x17, 0x2A]);
    for at in to_false {
        il.land(at);
    }
    il.op(&[0x16, 0x2A]);
    bodies.push(("Equals".into(), 0x00C6, vec![0x20, 0x01, 0x02, 0x1C], il));
    // GetHashCode(): C# のコンパイラと同じく、メンバーの値の EqualityComparer<T>.Default.GetHashCode を -1521134295 で混ぜる。
    let mut il = Il::default();
    il.int(seed(members));
    for (index, field) in field_refs.iter().enumerate() {
        il.int(-1521134295).op(&[0x5A]).token(0x28, comparers[index].0).op(&[0x02]).token(0x7B, *field).token(0x6F, comparers[index].2).op(&[0x58]);
    }
    il.op(&[0x2A]);
    bodies.push(("GetHashCode".into(), 0x00C6, vec![0x20, 0x00, 0x08], il));

    // 本体を .text に並べる位置（RVA）は、PE を組むときに決まる。ここでは、本体の並びの中の位置だけを覚える。
    let mut code = Vec::new();
    let mut offsets = Vec::new();
    for (_, _, _, il) in &bodies {
        pad(&mut code, 4);
        offsets.push(code.len() as u32);
        code.extend(il.body());
    }
    pad(&mut code, 4);

    // .text の並び: IAT（8）、CLI ヘッダー（72）、メソッドの本体、メタデータ、インポート。
    const TEXT_RVA: u32 = 0x2000;
    let code_rva = TEXT_RVA + 8 + 72;
    for (index, (name, flags, signature, _)) in bodies.iter().enumerate() {
        let (name, signature) = (b.string(name), b.blob(signature));
        b.row(METHOD_DEF, vec![Cell::U32(code_rva + offsets[index]), Cell::U16(0), Cell::U16(*flags), name, signature, Cell::U16(1)]);
    }
    // プロパティ（getter は 2 番目からのメソッド）。
    b.row(PROPERTY_MAP, vec![Cell::U16(anonymous as u16), Cell::U16(1)]);
    for (index, member) in members.iter().enumerate() {
        let name = b.string(member);
        let signature = b.blob(&[0x28, 0x00, 0x13, index as u8]);
        let property = b.row(PROPERTY, vec![Cell::U16(0), name, signature]);
        b.row(METHOD_SEMANTICS, vec![Cell::U16(0x0002), Cell::U16(index as u16 + 2), Cell::U16((property << 1 | 1) as u16)]);
    }
    // [DebuggerDisplay(@"\{ A = {A} }", Type = "<Anonymous Type>")]
    let shown = format!("\\{{ {} }}", members.iter().map(|member| format!("{member} = {{{member}}}")).collect::<Vec<_>>().join(", "));
    let mut value = vec![0x01, 0x00];
    serialized_string(&mut value, &shown);
    value.extend([0x01, 0x00, 0x54, 0x0E]);
    serialized_string(&mut value, "Type");
    serialized_string(&mut value, "<Anonymous Type>");
    let value = b.blob(&value);
    b.row(CUSTOM_ATTRIBUTE, vec![Cell::U16((anonymous << 5 | 3) as u16), Cell::U16(((display_constructor & 0x00FF_FFFF) << 3 | 3) as u16), value]);
    let name = b.string(assembly);
    b.row(ASSEMBLY, vec![Cell::U32(0x8004), Cell::U16(0), Cell::U16(0), Cell::U16(0), Cell::U16(0), Cell::U32(0), Cell::U16(0), name, Cell::U16(0)]);
    let metadata = metadata_root(&b, stamp);
    pe(&code, &metadata)
}

/// GetHashCode の最初の値（メンバーの名前の FNV-1a）。C# のコンパイラも名前から作るが、同じ値になるとは限らない
/// （ハッシュの値は、型やプロセスをまたいで比べるものではない）。
fn seed(members: &[&str]) -> i32 {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in members.iter().flat_map(|member| member.bytes()) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash as i32
}

/// カスタム属性の値の中の文字列（長さの圧縮整数と UTF-8）。
fn serialized_string(out: &mut Vec<u8>, text: &str) {
    compressed(out, text.len() as u32);
    out.extend(text.as_bytes());
}

/// `BSJB` で始まるメタデータ。
fn metadata_root(b: &Builder, stamp: u128) -> Vec<u8> {
    let tables = b.tables();
    let mut strings = b.strings.clone();
    pad(&mut strings, 4);
    let mut user_strings = b.user_strings.clone();
    pad(&mut user_strings, 4);
    let mut blobs = b.blobs.clone();
    pad(&mut blobs, 4);
    let guid = stamp.to_le_bytes().to_vec();
    let streams: [(&str, &Vec<u8>); 5] = [("#~", &tables), ("#Strings", &strings), ("#US", &user_strings), ("#GUID", &guid), ("#Blob", &blobs)];
    let version = b"v4.0.30319\0\0";
    let mut header = Vec::new();
    header.extend(0x424A_5342u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(0u32.to_le_bytes());
    header.extend((version.len() as u32).to_le_bytes());
    header.extend(version);
    header.extend(0u16.to_le_bytes());
    header.extend((streams.len() as u16).to_le_bytes());
    // ストリームの頭の大きさを先に数えて、中身の位置を決める。
    let headers_size: usize = streams.iter().map(|(name, _)| 8 + (name.len() + 1).div_ceil(4) * 4).sum();
    let mut offset = header.len() + headers_size;
    for (name, data) in &streams {
        header.extend((offset as u32).to_le_bytes());
        header.extend((data.len() as u32).to_le_bytes());
        header.extend(name.as_bytes());
        header.push(0);
        pad(&mut header, 4);
        offset += data.len();
    }
    for (_, data) in &streams {
        header.extend(data.iter());
    }
    header
}

/// IL だけの PE32 の DLL（.text と .reloc）。`code` はメソッドの本体（.text の 80 バイト目から）、`metadata` はメタデータ。
fn pe(code: &[u8], metadata: &[u8]) -> Vec<u8> {
    const FILE_ALIGNMENT: u32 = 0x200;
    const SECTION_ALIGNMENT: u32 = 0x2000;
    const IMAGE_BASE: u32 = 0x1000_0000;
    const TEXT_RVA: u32 = 0x2000;
    let align = |value: u32, to: u32| value.div_ceil(to) * to;

    // .text の中身。
    let mut text = vec![0u8; 8 + 72];
    text.extend(code);
    pad(&mut text, 4);
    let metadata_rva = TEXT_RVA + text.len() as u32;
    text.extend(metadata);
    pad(&mut text, 4);
    // インポート: mscoree.dll の _CorDllMain（.NET Framework の読み込みが、これを見る）。
    let import_rva = TEXT_RVA + text.len() as u32;
    let lookup_rva = import_rva + 40;
    let hint_rva = lookup_rva + 8;
    let name_rva = hint_rva + 2 + 12;
    let mut import = Vec::new();
    for value in [lookup_rva, 0, 0, name_rva, TEXT_RVA] {
        import.extend(value.to_le_bytes());
    }
    import.extend([0u8; 20]);
    import.extend(hint_rva.to_le_bytes());
    import.extend(0u32.to_le_bytes());
    import.extend(0u16.to_le_bytes());
    import.extend(b"_CorDllMain\0");
    import.extend(b"mscoree.dll\0");
    text.extend(&import);
    // 入り口の命令（jmp [_CorDllMain]）。番地を、.reloc で直してもらう。
    pad(&mut text, 4);
    text.extend([0, 0]);
    let entry_rva = TEXT_RVA + text.len() as u32;
    text.extend([0xFF, 0x25]);
    text.extend((IMAGE_BASE + TEXT_RVA).to_le_bytes());
    // IAT と CLI ヘッダー。
    text[0..4].copy_from_slice(&hint_rva.to_le_bytes());
    let mut cli = Vec::new();
    cli.extend(72u32.to_le_bytes());
    cli.extend(2u16.to_le_bytes());
    cli.extend(5u16.to_le_bytes());
    cli.extend(metadata_rva.to_le_bytes());
    cli.extend((metadata.len() as u32).to_le_bytes());
    cli.extend(1u32.to_le_bytes()); // COMIMAGE_FLAGS_ILONLY
    cli.resize(72, 0);
    text[8..80].copy_from_slice(&cli);

    let text_raw = align(text.len() as u32, FILE_ALIGNMENT);
    let reloc_rva = align(TEXT_RVA + text.len() as u32, SECTION_ALIGNMENT);
    let fixup = entry_rva + 2;
    let page = fixup & !0xFFF;
    let mut reloc = Vec::new();
    reloc.extend(page.to_le_bytes());
    reloc.extend(12u32.to_le_bytes());
    reloc.extend((0x3000u16 | (fixup - page) as u16).to_le_bytes());
    reloc.extend(0u16.to_le_bytes());
    let reloc_raw = align(reloc.len() as u32, FILE_ALIGNMENT);
    let image_size = align(reloc_rva + reloc.len() as u32, SECTION_ALIGNMENT);

    let mut out = Vec::new();
    // DOS ヘッダー（e_lfanew = 0x80）。
    let mut dos = vec![0u8; 0x80];
    dos[0..2].copy_from_slice(b"MZ");
    dos[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    out.extend(dos);
    out.extend(b"PE\0\0");
    // COFF ヘッダー。
    out.extend(0x014Cu16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend([0u8; 12]);
    out.extend(0xE0u16.to_le_bytes());
    out.extend(0x2102u16.to_le_bytes());
    // 任意ヘッダー（PE32）。
    out.extend(0x010Bu16.to_le_bytes());
    out.extend([8u8, 0]);
    for value in [text_raw, reloc_raw, 0, entry_rva, TEXT_RVA, reloc_rva, IMAGE_BASE, SECTION_ALIGNMENT, FILE_ALIGNMENT] {
        out.extend(value.to_le_bytes());
    }
    for value in [4u16, 0, 0, 0, 4, 0] {
        out.extend(value.to_le_bytes());
    }
    for value in [0u32, image_size, FILE_ALIGNMENT, 0] {
        out.extend(value.to_le_bytes());
    }
    out.extend(3u16.to_le_bytes());
    out.extend(0x8540u16.to_le_bytes());
    for value in [0x10_0000u32, 0x1000, 0x10_0000, 0x1000, 0, 16] {
        out.extend(value.to_le_bytes());
    }
    let mut directories = [(0u32, 0u32); 16];
    directories[1] = (import_rva, 40);
    directories[5] = (reloc_rva, reloc.len() as u32);
    directories[12] = (TEXT_RVA, 8);
    directories[14] = (TEXT_RVA + 8, 72);
    for (rva, size) in directories {
        out.extend(rva.to_le_bytes());
        out.extend(size.to_le_bytes());
    }
    // 節の表。
    let section = |out: &mut Vec<u8>, name: &[u8; 8], size: u32, rva: u32, raw: u32, at: u32, flags: u32| {
        out.extend(name);
        for value in [size, rva, raw, at, 0, 0] {
            out.extend(value.to_le_bytes());
        }
        out.extend([0u8; 4]);
        out.extend(flags.to_le_bytes());
    };
    section(&mut out, b".text\0\0\0", text.len() as u32, TEXT_RVA, text_raw, FILE_ALIGNMENT, 0x6000_0020);
    section(&mut out, b".reloc\0\0", reloc.len() as u32, reloc_rva, reloc_raw, FILE_ALIGNMENT + text_raw, 0x4200_0040);
    out.resize(FILE_ALIGNMENT as usize, 0);
    out.extend(&text);
    out.resize((FILE_ALIGNMENT + text_raw) as usize, 0);
    out.extend(&reloc);
    out.resize((FILE_ALIGNMENT + text_raw + reloc_raw) as usize, 0);
    out
}
