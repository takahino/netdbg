//! idl/cordebug.idl を読み、ICorDebug の Rust バインディング（src/cordebug.rs）を書き出す。
//!
//!     cargo run -p idlgen -- idl/cordebug.idl src/cordebug.rs
//!     cargo run -p idlgen -- idl/metahost.idl src/metahost.rs
//!
//! 手書きを避ける理由: x86 の COM は stdcall で、引数の個数や大きさを一つ間違えるとスタックが壊れる。
//! ここで扱うのは cordebug.idl に出てくる書き方だけで、MIDL の全文法ではない。知らない書き方に出会ったら
//! 黙って飛ばさず、行番号つきのエラーで止まる。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::process::ExitCode;

// ---------------------------------------------------------------- 字句

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 識別子と数値（`ICorDebug`、`0x10`、`3d6f5f61`）。uuid の区切りを単純にするため区別しない。
    Word(String),
    Str(String),
    Punct(char),
}

struct Lexer {
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

fn lex(source: &str) -> Result<Vec<(Tok, usize)>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut toks = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c == '#' {
            // #pragma、#if、#endif。条件は常に真として扱う（唯一の #if は型の別名を囲んでいるだけ）。
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            let start = i + 1;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            toks.push((Tok::Str(chars[start..i.min(chars.len())].iter().collect()), line));
            i += 1;
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            toks.push((Tok::Word(chars[start..i].iter().collect()), line));
        } else if "[](){};:,*=|+-<>~.&".contains(c) {
            toks.push((Tok::Punct(c), line));
            i += 1;
        } else {
            return Err(format!("line {line}: 読めない文字 {c:?}"));
        }
    }
    Ok(toks)
}

impl Lexer {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }
    fn line(&self) -> usize {
        self.toks.get(self.pos.min(self.toks.len().saturating_sub(1))).map_or(0, |t| t.1)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.0.clone());
        self.pos += 1;
        t
    }
    fn err<T>(&self, message: impl AsRef<str>) -> Result<T, String> {
        Err(format!("line {}: {} (次のトークン: {:?})", self.line(), message.as_ref(), self.peek()))
    }
    fn is_punct(&self, c: char) -> bool {
        self.peek() == Some(&Tok::Punct(c))
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if x == w)
    }
    fn eat_punct(&mut self, c: char) -> bool {
        let hit = self.is_punct(c);
        if hit {
            self.pos += 1;
        }
        hit
    }
    fn expect_punct(&mut self, c: char) -> Result<(), String> {
        if self.eat_punct(c) {
            Ok(())
        } else {
            self.err(format!("{c:?} が必要です"))
        }
    }
    fn word(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Word(w)) => Ok(w),
            _ => {
                self.pos -= 1;
                self.err("識別子が必要です")
            }
        }
    }
    /// `[` の直後から、対応する `]` までのトークンを取り出す。
    fn bracketed(&mut self) -> Result<Vec<Tok>, String> {
        let mut depth = 1;
        let mut inner = Vec::new();
        loop {
            let Some(t) = self.next() else { return self.err("] が閉じていません") };
            match t {
                Tok::Punct('[') => depth += 1,
                Tok::Punct(']') => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(inner);
                    }
                }
                _ => {}
            }
            inner.push(t);
        }
    }
}

// ---------------------------------------------------------------- 構文

#[derive(Debug, Clone)]
struct CType {
    /// `unsigned long` のように空白でつないだ型名。
    base: String,
    stars: usize,
}

#[derive(Debug, Clone)]
struct Decl {
    ty: CType,
    name: String,
    /// `x[]` は `Some(None)`、`x[16]` は `Some(Some(16))`。
    array: Option<Option<u64>>,
}

struct Method {
    name: String,
    ret: CType,
    params: Vec<Decl>,
}

struct Interface {
    name: String,
    base: String,
    uuid: String,
    methods: Vec<Method>,
}

enum Item {
    Alias { name: String, ty: CType },
    Enum { name: String, values: Vec<(String, u32)> },
    Struct { name: String, fields: Vec<Decl> },
    /// `module` の中の `const long 名前 = 値;`。
    Const { name: String, ty: String, value: i64 },
}

#[derive(Default)]
struct Idl {
    items: Vec<Item>,
    interfaces: Vec<Interface>,
    /// 列挙の定数。あとの列挙の式から参照される。
    constants: HashMap<String, i64>,
}

const QUALIFIERS: &[&str] = &["const", "far", "struct", "enum", "interface"];

/// `[属性] const 型 ** 名前 [N]` を読む。終端（`,` `)` `;`）は消費しない。
fn parse_decl(lx: &mut Lexer, constants: &HashMap<String, i64>) -> Result<Option<Decl>, String> {
    while lx.eat_punct('[') {
        lx.bracketed()?;
    }
    let mut words: Vec<String> = Vec::new();
    let mut stars = 0;
    loop {
        match lx.peek() {
            // `SAFEARRAY(int)` は、C では `SAFEARRAY *`（要素の型は中身を読むときにだけ要る）。
            Some(Tok::Word(w)) if w == "SAFEARRAY" && matches!(lx.toks.get(lx.pos + 1), Some((Tok::Punct('('), _))) => {
                lx.pos += 2;
                while !lx.eat_punct(')') {
                    if lx.next().is_none() {
                        return lx.err("SAFEARRAY( が閉じていません");
                    }
                }
                words.push("SAFEARRAY".into());
                stars += 1;
            }
            Some(Tok::Word(w)) => {
                if !QUALIFIERS.contains(&w.as_str()) {
                    words.push(w.clone());
                }
                lx.pos += 1;
            }
            Some(Tok::Punct('*')) => {
                stars += 1;
                lx.pos += 1;
            }
            _ => break,
        }
    }
    if words.is_empty() {
        return lx.err("型が必要です");
    }
    if words.len() == 1 {
        if words[0] == "void" && stars == 0 {
            return Ok(None); // f(void)
        }
        return lx.err(format!("名前の無い宣言です: {}", words[0]));
    }
    let name = words.pop().unwrap();
    let mut array = None;
    if lx.eat_punct('[') {
        let inner = lx.bracketed()?;
        array = Some(match inner.as_slice() {
            [] => None,
            _ => Some(eval(&inner, constants).map_err(|e| format!("line {}: 配列の長さ: {e}", lx.line()))? as u64),
        });
    }
    Ok(Some(Decl { ty: CType { base: words.join(" "), stars }, name, array }))
}

/// 列挙の `{ A = 1, B, ... }`（`{` の直前まで読んだところから）。値は定数にも覚える。
fn parse_enum_body(lx: &mut Lexer, idl: &mut Idl) -> Result<Vec<(String, u32)>, String> {
    lx.expect_punct('{')?;
    let mut values = Vec::new();
    let mut next: i64 = 0;
    while !lx.eat_punct('}') {
        let name = lx.word()?;
        if lx.eat_punct('=') {
            let mut expr = Vec::new();
            while !lx.is_punct(',') && !lx.is_punct('}') {
                expr.push(lx.next().ok_or("列挙の途中で終わりました")?);
            }
            next = eval(&expr, &idl.constants).map_err(|e| format!("line {}: {name}: {e}", lx.line()))?;
        }
        idl.constants.insert(name.clone(), next);
        values.push((name, next as u32)); // C の列挙は 32bit。負の値は折り返して同じビット列になる。
        next += 1;
        lx.eat_punct(',');
    }
    Ok(values)
}

/// `module 名前 { const long 名前 = 値; ... };`（関数の無い、定数だけのモジュール）。"module" は消費済み。
fn parse_module(lx: &mut Lexer, idl: &mut Idl) -> Result<(), String> {
    lx.word()?;
    lx.expect_punct('{')?;
    while !lx.eat_punct('}') {
        if !lx.is_word("const") {
            return lx.err("module の中は const の定数だけに対応しています");
        }
        lx.pos += 1;
        let ty = lx.word()?;
        let name = lx.word()?;
        lx.expect_punct('=')?;
        let mut expr = Vec::new();
        while !lx.is_punct(';') {
            expr.push(lx.next().ok_or("定数の途中で終わりました")?);
        }
        lx.expect_punct(';')?;
        let value = eval(&expr, &idl.constants).map_err(|e| format!("line {}: {name}: {e}", lx.line()))?;
        idl.constants.insert(name.clone(), value);
        idl.items.push(Item::Const { name, ty, value });
    }
    lx.eat_punct(';');
    Ok(())
}

fn parse_typedef(lx: &mut Lexer, idl: &mut Idl) -> Result<(), String> {
    // "typedef" は消費済み。
    if lx.eat_punct('[') {
        lx.bracketed()?; // [wire_marshal(...)] など
    }
    if lx.is_word("enum") {
        lx.pos += 1;
        if !lx.is_punct('{') {
            lx.word()?; // タグ名
        }
        let values = parse_enum_body(lx, idl)?;
        let name = lx.word()?;
        lx.expect_punct(';')?;
        idl.items.push(Item::Enum { name, values });
    } else if lx.is_word("struct") {
        lx.pos += 1;
        if !lx.is_punct('{') {
            lx.word()?;
        }
        lx.expect_punct('{')?;
        let mut fields = Vec::new();
        while !lx.eat_punct('}') {
            let Some(first) = parse_decl(lx, &idl.constants)? else { return lx.err("構造体のフィールドが読めません") };
            // `ULONG32 startOffset, endOffset;` のように、一つの型で複数のフィールドを宣言できる。
            let ty = first.ty.clone();
            fields.push(first);
            while lx.eat_punct(',') {
                if ty.stars != 0 {
                    return lx.err("ポインター型の複数宣言には対応していません");
                }
                fields.push(Decl { ty: ty.clone(), name: lx.word()?, array: None });
            }
            lx.expect_punct(';')?;
        }
        let name = lx.word()?;
        lx.expect_punct(';')?;
        idl.items.push(Item::Struct { name, fields });
    } else if lx.toks[lx.pos..].iter().take_while(|(t, _)| *t != Tok::Punct(';')).any(|(t, _)| *t == Tok::Punct('(')) {
        // 関数ポインター: typedef HRESULT (__stdcall *Name)(引数);
        // 呼び出すことは無く、引数として受け渡すだけなので、中身を問わないポインターとして扱う。
        while !lx.eat_punct('(') {
            lx.pos += 1;
        }
        while !lx.eat_punct('*') {
            lx.word()?; // 呼び出し規約
        }
        let name = lx.word()?;
        lx.expect_punct(')')?;
        lx.expect_punct('(')?;
        let mut depth = 1;
        while depth > 0 {
            match lx.next() {
                Some(Tok::Punct('(')) => depth += 1,
                Some(Tok::Punct(')')) => depth -= 1,
                Some(_) => {}
                None => return lx.err("関数ポインターの typedef の途中で終わりました"),
            }
        }
        lx.expect_punct(';')?;
        idl.items.push(Item::Alias { name, ty: CType { base: "void".into(), stars: 1 } });
    } else {
        let Some(d) = parse_decl(lx, &idl.constants)? else { return lx.err("typedef が読めません") };
        lx.expect_punct(';')?;
        if d.array.is_some() {
            return lx.err("配列の typedef には対応していません");
        }
        idl.items.push(Item::Alias { name: d.name, ty: d.ty });
    }
    Ok(())
}

fn parse(source: &str) -> Result<Idl, String> {
    let mut lx = Lexer { toks: lex(source)?, pos: 0 };
    let mut idl = Idl::default();
    let mut uuid = String::new();
    while lx.peek().is_some() {
        if lx.eat_punct('[') {
            let attrs = lx.bracketed()?;
            // uuid(xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx): 括弧の中のトークンをつなぎ直す。
            uuid.clear();
            if let Some(at) = attrs.iter().position(|t| *t == Tok::Word("uuid".into())) {
                for t in attrs[at + 2..].iter() {
                    match t {
                        Tok::Punct(')') => break,
                        Tok::Word(w) => uuid.push_str(w),
                        Tok::Punct('-') => uuid.push('-'),
                        other => return lx.err(format!("uuid の中に {other:?}")),
                    }
                }
            }
        } else if lx.is_word("interface") {
            lx.pos += 1;
            let name = lx.word()?;
            if lx.eat_punct(';') {
                continue; // 前方宣言
            }
            lx.expect_punct(':')?;
            let base = lx.word()?;
            lx.expect_punct('{')?;
            let mut methods = Vec::new();
            while !lx.eat_punct('}') {
                if lx.is_word("typedef") {
                    lx.pos += 1;
                    parse_typedef(&mut lx, &mut idl)?;
                } else if lx.is_word("cpp_quote") {
                    skip_cpp_quote(&mut lx)?;
                } else if lx.is_word("const") && matches!(lx.toks.get(lx.pos + 3), Some((Tok::Punct('='), _))) {
                    // const ULONG32 kLastStubFrame = 0xffffffff; のような定数は使わないので読み飛ばす。
                    while !lx.eat_punct(';') {
                        lx.pos += 1;
                    }
                } else {
                    methods.push(parse_method(&mut lx, &idl.constants)?);
                }
            }
            lx.eat_punct(';');
            if uuid.is_empty() {
                return lx.err(format!("interface {name} に uuid がありません"));
            }
            idl.interfaces.push(Interface { name, base, uuid: std::mem::take(&mut uuid), methods });
        } else if lx.is_word("typedef") {
            lx.pos += 1;
            parse_typedef(&mut lx, &mut idl)?;
        } else if lx.is_word("enum") {
            // typedef の無い `enum 名前 { ... };`（引数では `enum 名前` と書かれる）。
            lx.pos += 1;
            let name = lx.word()?;
            let values = parse_enum_body(&mut lx, &mut idl)?;
            lx.expect_punct(';')?;
            idl.items.push(Item::Enum { name, values });
        } else if lx.is_word("module") {
            lx.pos += 1;
            parse_module(&mut lx, &mut idl)?;
        } else if lx.is_word("cpp_quote") {
            skip_cpp_quote(&mut lx)?;
        } else if lx.is_word("import") {
            while !lx.eat_punct(';') {
                lx.pos += 1;
            }
        } else if lx.is_word("library") {
            break; // タイプライブラリの定義（coclass）は使わない
        } else {
            return lx.err("知らない書き方です");
        }
    }
    Ok(idl)
}

fn skip_cpp_quote(lx: &mut Lexer) -> Result<(), String> {
    lx.pos += 1;
    lx.expect_punct('(')?;
    match lx.next() {
        Some(Tok::Str(_)) => {}
        _ => return lx.err("cpp_quote の文字列が必要です"),
    }
    lx.expect_punct(')')?;
    lx.eat_punct(';');
    Ok(())
}

fn parse_method(lx: &mut Lexer, constants: &HashMap<String, i64>) -> Result<Method, String> {
    // [propget] / [propput] のプロパティは、C のヘッダーと同じく get_ / put_ を付けた名前にする（同じ名前が 2 つ並ぶ）。
    let mut prefix = "";
    while lx.eat_punct('[') {
        for attribute in lx.bracketed()? {
            match attribute {
                Tok::Word(w) if w == "propget" => prefix = "get_",
                Tok::Word(w) if w == "propput" => prefix = "put_",
                Tok::Word(w) if w == "propputref" => prefix = "putref_",
                _ => {}
            }
        }
    }
    // 戻り値の型とメソッド名。`(` の直前の語が名前。
    let mut words = Vec::new();
    let mut stars = 0;
    while !lx.is_punct('(') {
        match lx.next() {
            Some(Tok::Word(w)) => words.push(w),
            Some(Tok::Punct('*')) => stars += 1,
            _ => {
                lx.pos -= 1;
                return lx.err("メソッドの宣言が読めません");
            }
        }
    }
    if words.len() < 2 {
        return lx.err("メソッドの戻り値か名前がありません");
    }
    let name = format!("{prefix}{}", words.pop().unwrap());
    lx.expect_punct('(')?;
    let mut params = Vec::new();
    while !lx.eat_punct(')') {
        if let Some(d) = parse_decl(lx, constants)? {
            params.push(d);
        }
        lx.eat_punct(',');
    }
    lx.expect_punct(';')?;
    Ok(Method { name, ret: CType { base: words.join(" "), stars }, params })
}

// ---------------------------------------------------------------- 定数式

/// 列挙の値と配列の長さに出てくる式。整数、既出の定数、`| + - << ~ ( )`。
fn eval(toks: &[Tok], constants: &HashMap<String, i64>) -> Result<i64, String> {
    let mut pos = 0;
    let value = eval_or(toks, &mut pos, constants)?;
    if pos != toks.len() {
        return Err(format!("式が読めません: {:?}", &toks[pos..]));
    }
    Ok(value)
}

fn eval_or(t: &[Tok], p: &mut usize, c: &HashMap<String, i64>) -> Result<i64, String> {
    let mut v = eval_shift(t, p, c)?;
    while t.get(*p) == Some(&Tok::Punct('|')) {
        *p += 1;
        v |= eval_shift(t, p, c)?;
    }
    Ok(v)
}

fn eval_shift(t: &[Tok], p: &mut usize, c: &HashMap<String, i64>) -> Result<i64, String> {
    let mut v = eval_add(t, p, c)?;
    while t.get(*p) == Some(&Tok::Punct('<')) && t.get(*p + 1) == Some(&Tok::Punct('<')) {
        *p += 2;
        v <<= eval_add(t, p, c)?;
    }
    Ok(v)
}

fn eval_add(t: &[Tok], p: &mut usize, c: &HashMap<String, i64>) -> Result<i64, String> {
    let mut v = eval_unary(t, p, c)?;
    loop {
        match t.get(*p) {
            Some(Tok::Punct('+')) => {
                *p += 1;
                v += eval_unary(t, p, c)?;
            }
            Some(Tok::Punct('-')) => {
                *p += 1;
                v -= eval_unary(t, p, c)?;
            }
            _ => return Ok(v),
        }
    }
}

fn eval_unary(t: &[Tok], p: &mut usize, c: &HashMap<String, i64>) -> Result<i64, String> {
    match t.get(*p) {
        Some(Tok::Punct('-')) => {
            *p += 1;
            Ok(-eval_unary(t, p, c)?)
        }
        Some(Tok::Punct('~')) => {
            *p += 1;
            Ok(!eval_unary(t, p, c)?)
        }
        Some(Tok::Punct('(')) => {
            *p += 1;
            let v = eval_or(t, p, c)?;
            if t.get(*p) != Some(&Tok::Punct(')')) {
                return Err(") が必要です".into());
            }
            *p += 1;
            Ok(v)
        }
        Some(Tok::Word(w)) => {
            *p += 1;
            if let Some(v) = c.get(w) {
                return Ok(*v);
            }
            let digits = w.trim_end_matches(['u', 'U', 'l', 'L']);
            let parsed = match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
                Some(hex) => i64::from_str_radix(hex, 16),
                None => digits.parse(),
            };
            parsed.map_err(|_| format!("数値でも既出の定数でもありません: {w}"))
        }
        other => Err(format!("式の途中に {other:?}")),
    }
}

// ---------------------------------------------------------------- 出力

/// IDL の外（Windows のヘッダー）で定義されている型。値渡しで大きさが分かっているものだけ。
fn builtin(base: &str) -> Option<&'static str> {
    Some(match base {
        "HRESULT" => "HRESULT",
        "BOOL" | "LONG" | "INT32" | "INT" | "int" | "long" | "LONG32" => "i32",
        "DWORD" | "ULONG" | "ULONG32" | "UINT32" | "UINT" | "unsigned long" | "unsigned int" | "unsigned" => "u32",
        "BYTE" | "unsigned char" | "UCHAR" | "boolean" => "u8",
        "CHAR" | "char" => "i8",
        "WCHAR" | "WORD" | "USHORT" | "unsigned short" => "u16",
        "SHORT" | "short" => "i16",
        "ULONG64" | "UINT64" | "DWORD64" | "ULONGLONG" => "u64",
        "LONG64" | "INT64" | "LONGLONG" => "i64",
        "SIZE_T" | "UINT_PTR" | "ULONG_PTR" | "DWORD_PTR" => "usize",
        "LONG_PTR" | "INT_PTR" => "isize",
        "FLOAT" | "float" => "f32",
        "DOUBLE" | "double" => "f64",
        "GUID" | "IID" | "CLSID" => "GUID",
        "REFIID" | "REFGUID" | "REFCLSID" => "*const GUID",
        "LPCWSTR" => "*const u16",
        "LPWSTR" => "*mut u16",
        "LPCSTR" => "*const u8",
        "LPSTR" => "*mut u8",
        "HANDLE" | "PVOID" | "LPVOID" | "HMODULE" | "LPSECURITY_ATTRIBUTES" => "*mut c_void",
        _ => return None,
    })
}

const RUST_KEYWORDS: &[&str] = &["type", "ref", "in", "self", "mod", "fn", "match", "loop", "move", "box", "use", "impl", "where", "as", "dyn"];

fn ident(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

struct Gen<'a> {
    idl: &'a Idl,
    /// IDL が定義した型（別名、列挙、構造体）。
    defined: BTreeSet<&'a str>,
    interfaces: BTreeMap<&'a str, &'a Interface>,
    /// ポインターとしてだけ現れ、中身を知らない型。`*mut c_void` にした。
    opaque: BTreeSet<String>,
}

impl<'a> Gen<'a> {
    /// C の型を Rust の型にする。`extra_pointer` は `x[]` の引数（ポインターとして渡る）。
    fn rust_type(&mut self, ty: &CType, extra_pointer: bool, context: &str) -> Result<String, String> {
        let stars = ty.stars + extra_pointer as usize;
        let pointers = |n: usize, inner: &str| format!("{}{}", "*mut ".repeat(n), inner);
        if ty.base == "void" {
            if stars == 0 {
                return Err(format!("{context}: void の値渡し"));
            }
            return Ok(pointers(stars, "c_void"));
        }
        if self.interfaces.contains_key(ty.base.as_str()) || ty.base == "IUnknown" {
            if stars == 0 {
                return Err(format!("{context}: インターフェイス {} の値渡し", ty.base));
            }
            // インターフェイスへのポインターは型を付けずに渡す。呼び出し側が ComPtr に包む。
            return Ok(pointers(stars, "c_void"));
        }
        if let Some(rust) = builtin(&ty.base) {
            return Ok(pointers(stars, rust));
        }
        if self.defined.contains(ty.base.as_str()) {
            return Ok(pointers(stars, &ty.base));
        }
        if stars == 0 {
            return Err(format!("{context}: 大きさの分からない型 {} の値渡し", ty.base));
        }
        self.opaque.insert(ty.base.clone());
        Ok(pointers(stars, "c_void"))
    }

    /// 基底から順に並べた継承の列（IUnknown は含めない）。
    fn chain(&self, name: &str) -> Result<Vec<&'a Interface>, String> {
        let mut chain = Vec::new();
        let mut current = name;
        while current != "IUnknown" {
            let i = self.interfaces.get(current).ok_or_else(|| format!("基底のインターフェイス {current} が見つかりません"))?;
            chain.push(*i);
            current = &i.base;
        }
        chain.reverse();
        Ok(chain)
    }
}

fn guid_literal(uuid: &str) -> Result<String, String> {
    let parts: Vec<&str> = uuid.split('-').collect();
    if parts.len() != 5 || parts.iter().map(|p| p.len()).collect::<Vec<_>>() != [8, 4, 4, 4, 12] {
        return Err(format!("uuid の形が違います: {uuid}"));
    }
    let tail = format!("{}{}", parts[3], parts[4]);
    let bytes: Vec<String> = (0..8).map(|i| format!("0x{}", &tail[i * 2..i * 2 + 2])).collect();
    Ok(format!("GUID {{ data1: 0x{}, data2: 0x{}, data3: 0x{}, data4: [{}] }}", parts[0], parts[1], parts[2], bytes.join(", ")))
}

fn generate(idl: &Idl, source_name: &str) -> Result<String, String> {
    let mut g = Gen { idl, defined: BTreeSet::new(), interfaces: BTreeMap::new(), opaque: BTreeSet::new() };
    for item in &idl.items {
        g.defined.insert(match item {
            Item::Alias { name, .. } | Item::Enum { name, .. } | Item::Struct { name, .. } => name,
            Item::Const { .. } => continue,
        });
    }
    for i in &idl.interfaces {
        g.interfaces.insert(&i.name, i);
    }

    let mut out = String::new();
    let mut body = String::new();

    for item in &g.idl.items {
        match item {
            Item::Alias { name, ty } => {
                let rust = g.rust_type(ty, false, name)?;
                writeln!(body, "pub type {name} = {rust};").unwrap();
            }
            Item::Enum { name, values } => {
                writeln!(body, "\npub type {name} = u32;").unwrap();
                for (value_name, value) in values {
                    writeln!(body, "pub const {value_name}: {name} = 0x{value:x};").unwrap();
                }
            }
            Item::Struct { name, fields } => {
                writeln!(body, "\n#[repr(C)]\n#[derive(Clone, Copy)]\npub struct {name} {{").unwrap();
                for f in fields {
                    let rust = g.rust_type(&f.ty, false, &format!("{name}.{}", f.name))?;
                    match f.array {
                        None => writeln!(body, "    pub {}: {rust},", ident(&f.name)).unwrap(),
                        Some(Some(n)) => writeln!(body, "    pub {}: [{rust}; {n}],", ident(&f.name)).unwrap(),
                        Some(None) => return Err(format!("{name}.{}: 長さの無い配列のフィールド", f.name)),
                    }
                }
                writeln!(body, "}}").unwrap();
            }
            Item::Const { name, ty, value } => {
                let rust = builtin(ty).filter(|r| r.starts_with('i') || r.starts_with('u')).ok_or_else(|| format!("{name}: 定数の型 {ty} は整数ではありません"))?;
                writeln!(body, "pub const {name}: {rust} = {value};").unwrap();
            }
        }
    }

    for i in &g.idl.interfaces {
        let chain = g.chain(&i.name)?;
        let base_vtbl = if i.base == "IUnknown" { "IUnknownVtbl".to_string() } else { format!("{}Vtbl", i.base) };

        // vtable。フィールドの順序が呼び出し先を決める。
        writeln!(body, "\n// ---- {} : {}", i.name, i.base).unwrap();
        writeln!(body, "#[repr(C)]\npub struct {}Vtbl {{\n    pub base: {base_vtbl},", i.name).unwrap();
        for m in &i.methods {
            let (params, ret) = signature(&mut g, i, m)?;
            let list: Vec<String> = params.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            let all = std::iter::once("this: *mut c_void".to_string()).chain(list).collect::<Vec<_>>().join(", ");
            writeln!(body, "    pub {}: unsafe extern \"system\" fn({all}){ret},", m.name).unwrap();
        }
        writeln!(body, "}}").unwrap();

        writeln!(body, "pub enum {} {{}}", i.name).unwrap();
        writeln!(body, "unsafe impl Interface for {} {{\n    const IID: GUID = {};\n    type Vtbl = {}Vtbl;\n}}", i.name, guid_literal(&i.uuid)?, i.name).unwrap();

        // 呼び出し用のメソッド。継承したメソッドも並べる。同じ名前は派生側を優先する。
        writeln!(body, "impl ComPtr<{}> {{", i.name).unwrap();
        let mut seen = BTreeSet::new();
        for (depth, level) in chain.iter().rev().enumerate() {
            for m in &level.methods {
                if !seen.insert(m.name.as_str()) {
                    continue;
                }
                let (params, ret) = signature(&mut g, level, m)?;
                let decl: String = params.iter().map(|(n, t)| format!(", {n}: {t}")).collect();
                let pass: String = params.iter().map(|(n, _)| format!(", {n}")).collect();
                let path = ".base".repeat(depth);
                writeln!(body, "    #[inline]\n    pub unsafe fn {}(&self{decl}){ret} {{\n        (self.vtbl(){path}.{})(self.as_raw(){pass})\n    }}", m.name, m.name).unwrap();
            }
        }
        writeln!(body, "}}").unwrap();
    }

    writeln!(out, "// このファイルは tools/idlgen が {source_name} から生成した。手で編集しない。").unwrap();
    // 出力先は、IDL と同じ名前の src/<名前>.rs と決めてある。
    let stem = std::path::Path::new(source_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    writeln!(out, "//     cargo run -p idlgen -- {source_name} src/{stem}.rs").unwrap();
    writeln!(out, "//").unwrap();
    writeln!(out, "// メソッドは `extern \"system\"`: x86 では stdcall、x64 では通常の規約になる。").unwrap();
    writeln!(out, "// インターフェイスへのポインターは `*mut c_void` で受け渡しし、呼び出し側が `ComPtr` に包む。").unwrap();
    if !g.opaque.is_empty() {
        writeln!(out, "// 中身を知らずポインターとしてだけ扱っている型: {}", g.opaque.iter().cloned().collect::<Vec<_>>().join(", ")).unwrap();
    }
    writeln!(out, "#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]\n").unwrap();
    writeln!(out, "use crate::com::{{ComPtr, IUnknownVtbl, Interface, GUID, HRESULT}};\nuse std::ffi::c_void;\n").unwrap();
    out.push_str(&body);
    Ok(out)
}

/// メソッドの引数（名前と Rust の型）と、戻り値（` -> T` か空）。
fn signature(g: &mut Gen, interface: &Interface, m: &Method) -> Result<(Vec<(String, String)>, String), String> {
    let context = format!("{}::{}", interface.name, m.name);
    let mut params = Vec::new();
    for p in &m.params {
        let rust = g.rust_type(&p.ty, p.array.is_some(), &format!("{context}({})", p.name))?;
        params.push((ident(&p.name), rust));
    }
    let ret = if m.ret.base == "void" && m.ret.stars == 0 { String::new() } else { format!(" -> {}", g.rust_type(&m.ret, false, &context)?) };
    Ok((params, ret))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: idlgen <cordebug.idl> <出力する .rs>");
        return ExitCode::from(2);
    }
    let source = match std::fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} を読めません: {e}", args[1]);
            return ExitCode::FAILURE;
        }
    };
    let result = parse(&source).and_then(|idl| {
        let text = generate(&idl, &args[1].replace('\\', "/"))?;
        Ok((text, idl.interfaces.len(), idl.items.len()))
    });
    match result {
        Ok((text, interfaces, items)) => {
            if let Err(e) = std::fs::write(&args[2], text) {
                eprintln!("{} に書けません: {e}", args[2]);
                return ExitCode::FAILURE;
            }
            println!("{}: インターフェイス {interfaces} 個、型 {items} 個", args[2]);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}: {e}", args[1]);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
        import "unknwn.idl";
        cpp_quote("#if 0")
        typedef UINT32 mdToken;
        typedef ULONG64 CORDB_ADDRESS;
        typedef enum Color { RED = 0x1, GREEN, BOTH = RED | GREEN, NEG = -1 } Color;
        typedef struct Pair { CORDB_ADDRESS a; BYTE b[4]; } Pair;
        interface IBase;
        [ object, local, uuid(3d6f5f62-7538-11d3-8d5b-00104b35e7ef), pointer_default(unique) ]
        interface IBase : IUnknown {
            HRESULT Stop([in] DWORD dwTimeout);
            HRESULT Continue([in] BOOL fIsOutOfBand);
        };
        [ object, local, uuid(3d6f5f64-7538-11d3-8d5b-00104b35e7ef) ]
        interface IDerived : IBase {
            typedef enum Inner { ONE = 1 } Inner;
            /* comment */
            HRESULT GetThing([in] mdToken tok, [out] IBase **ppBase, [in, size_is(n)] IBase *args[], [in] Pair byValue, [out] WCHAR szName[]);
            HRESULT Continue([in] Color type);
        };
        library X { coclass Y { interface IBase; }; };
    "##;

    #[test]
    fn parses_types_enums_and_interfaces() {
        let idl = parse(SAMPLE).unwrap();
        assert_eq!(idl.interfaces.len(), 2);
        assert_eq!(idl.interfaces[1].methods.len(), 2);
        assert_eq!(idl.constants["GREEN"], 2);
        assert_eq!(idl.constants["BOTH"], 3);
        assert_eq!(idl.constants["NEG"], -1);
    }

    #[test]
    fn generates_vtables_in_declaration_order_with_the_base_first() {
        let text = generate(&parse(SAMPLE).unwrap(), "sample.idl").unwrap();
        let vtbl = text.split("pub struct IDerivedVtbl").nth(1).unwrap();
        let (base, get, cont) = (vtbl.find("pub base: IBaseVtbl").unwrap(), vtbl.find("pub GetThing").unwrap(), vtbl.find("pub Continue").unwrap());
        assert!(base < get && get < cont);
    }

    #[test]
    fn maps_parameter_types() {
        let text = generate(&parse(SAMPLE).unwrap(), "sample.idl").unwrap();
        assert!(text.contains("pub GetThing: unsafe extern \"system\" fn(this: *mut c_void, tok: mdToken, ppBase: *mut *mut c_void, args: *mut *mut c_void, byValue: Pair, szName: *mut u16) -> HRESULT,"), "{text}");
        assert!(text.contains("pub const NEG: Color = 0xffffffff;"));
        assert!(text.contains("pub b: [u8; 4],"));
        assert!(text.contains("data1: 0x3d6f5f64, data2: 0x7538, data3: 0x11d3, data4: [0x8d, 0x5b, 0x00, 0x10, 0x4b, 0x35, 0xe7, 0xef]"));
    }

    #[test]
    fn flattens_inherited_methods_and_lets_the_derived_name_win() {
        let text = generate(&parse(SAMPLE).unwrap(), "sample.idl").unwrap();
        let derived = text.split("impl ComPtr<IDerived>").nth(1).unwrap();
        // Stop は基底から、Continue は派生側（引数が Color）。
        assert!(derived.contains("(self.vtbl().base.Stop)(self.as_raw(), dwTimeout)"));
        assert!(derived.contains("pub unsafe fn Continue(&self, type_: Color)"));
        assert_eq!(derived.matches("pub unsafe fn Continue").count(), 1);
    }

    #[test]
    fn a_function_pointer_typedef_becomes_an_opaque_pointer() {
        let source = "typedef HRESULT (__stdcall *DoneFn)(int code, void (*inner)(void));\n[uuid(3d6f5f62-7538-11d3-8d5b-00104b35e7ef)] interface I : IUnknown { HRESULT F([in] DoneFn done); };";
        let text = generate(&parse(source).unwrap(), "x.idl").unwrap();
        assert!(text.contains("pub type DoneFn = *mut c_void;"), "{text}");
        assert!(text.contains("pub F: unsafe extern \"system\" fn(this: *mut c_void, done: DoneFn) -> HRESULT,"), "{text}");
    }

    #[test]
    fn reads_the_ui_automation_forms() {
        // UIAutomationClient.idl の書き方: typedef の無い enum、定数だけの module、SAFEARRAY(int)、[propget] / [propput]。
        let source = r#"
            enum TreeScope { TreeScope_Element = 0x1, TreeScope_Children = 0x2, TreeScope_Subtree = TreeScope_Element | TreeScope_Children };
            [dllname("<no entry points>")] module UIA_PatternIds { const long UIA_InvokePatternId = 10000; };
            [object, uuid(3d6f5f62-7538-11d3-8d5b-00104b35e7ef)] interface I : IUnknown {
                HRESULT GetRuntimeId([out, retval] SAFEARRAY(int) * runtimeId);
                HRESULT Find([in] enum TreeScope scope, [in] SAFEARRAY(int) ids);
                [propget] HRESULT Timeout([out, retval] DWORD * value);
                [propput] HRESULT Timeout([in] DWORD value);
            };"#;
        let text = generate(&parse(source).unwrap(), "x.idl").unwrap();
        assert!(text.contains("pub const TreeScope_Subtree: TreeScope = 0x3;"), "{text}");
        assert!(text.contains("pub const UIA_InvokePatternId: i32 = 10000;"), "{text}");
        assert!(text.contains("pub GetRuntimeId: unsafe extern \"system\" fn(this: *mut c_void, runtimeId: *mut *mut c_void) -> HRESULT,"), "{text}");
        assert!(text.contains("pub Find: unsafe extern \"system\" fn(this: *mut c_void, scope: TreeScope, ids: *mut c_void) -> HRESULT,"), "{text}");
        assert!(text.contains("pub get_Timeout: unsafe extern \"system\" fn(this: *mut c_void, value: *mut u32) -> HRESULT,"), "{text}");
        assert!(text.contains("pub put_Timeout: unsafe extern \"system\" fn(this: *mut c_void, value: u32) -> HRESULT,"), "{text}");
    }

    #[test]
    fn refuses_an_unknown_type_passed_by_value() {
        let source = "[uuid(3d6f5f62-7538-11d3-8d5b-00104b35e7ef)] interface I : IUnknown { HRESULT F([in] MYSTERY m); };";
        let error = generate(&parse(source).unwrap(), "x.idl").unwrap_err();
        assert!(error.contains("MYSTERY"), "{error}");
    }

    #[test]
    fn reports_the_line_of_a_construct_it_does_not_know() {
        let error = parse("\n\nunion U { int a; };").err().unwrap();
        assert!(error.starts_with("line 3:"), "{error}");
    }
}
