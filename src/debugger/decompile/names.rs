//! 型とメンバーの、C# の書き方での名前。
//!
//! 型の名前は名前空間を付けずに書き、使った名前空間を覚えておく（ファイルの頭の `using` にする）。組み込みの型は
//! キーワード（`int`、`string`）、`Nullable<T>` は `T?` にする。

use super::sig::{self, MethodSig, Ty};
use crate::debugger::metadata::{Metadata, FIELD, MEMBER_REF, METHOD_DEF, METHOD_SPEC, STAND_ALONE_SIG, TYPE_DEF, TYPE_REF, TYPE_SPEC};
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeSet, HashMap};

/// 型引数の名前（`!0` と `!!0` を名前にする）。
#[derive(Debug, Clone, Default)]
pub struct Generics {
    pub types: Vec<String>,
    pub methods: Vec<String>,
}

/// 呼び出し先のメソッド。
#[derive(Debug, Clone)]
pub struct MethodRef {
    /// 宣言している型の名前（型引数を付けた形）。
    pub declaring: String,
    pub name: String,
    pub sig: MethodSig,
    /// シグネチャの中の `!0` と `!!0` の名前（宣言している型とメソッドの型引数）。
    pub generics: Generics,
    /// MethodSpec の型引数（`Echo<int>` の `int`）。
    pub method_arguments: Vec<String>,
}

pub struct Names<'a> {
    pub md: &'a Metadata,
    used: RefCell<BTreeSet<String>>,
    /// いま書いているファイルの型（入れ子と、このモジュールの基底の型を含む）のメンバーの名前。同じ名前の型は、
    /// 名前空間つきで書く（メンバーの方に結び付くので）。
    members: RefCell<std::collections::HashSet<String>>,
    /// 型引数の文脈の無い（ジェネリックでない型とメソッドの中の）`method_ref` の答え。同じファイルの中だけで使う
    /// （名前を引くと `using` と名前の重なりが決まるので、ファイルが変わったら捨てる）。
    method_cache: RefCell<HashMap<u32, Option<MethodRef>>>,
    lookups: OnceCell<Lookups>,
}

/// 子から親を引く表（メタデータの表は親から子の向きにしか並んでいないので、毎回頭から探すと二乗になる）。
/// モジュールを書く間、一度だけ作る。
#[derive(Default)]
pub struct Lookups {
    /// FieldRVA: フィールドの行 → データの RVA。
    pub field_rva: HashMap<u32, u32>,
    /// MethodImpl: 本体のメソッドの行 → 実装する宣言（MethodDefOrRef）。
    pub method_impl: HashMap<u32, u32>,
    /// PropertyMap: 型の行 → PropertyMap の行。
    pub property_map: HashMap<u32, u32>,
    /// EventMap: 型の行 → EventMap の行。
    pub event_map: HashMap<u32, u32>,
    /// 型の名前（型引数の数の印を含む）ごとの、その名前の型がある名前空間（TypeDef と TypeRef の、入れ子でないもの）。
    /// 2 つ以上の名前空間にある名前は、名前空間つきで書く（`using` だけでは、どちらか決まらない）。
    pub namespaces_of: HashMap<String, std::collections::HashSet<String>>,

    /// イベントの add / remove のメソッドの行（MethodSemantics から）。
    pub event_accessors: std::collections::HashSet<u32>,
}

/// C# のキーワード。識別子に使うときは `@` を付ける。
const KEYWORDS: &[&str] = &[
    "abstract", "as", "base", "bool", "break", "byte", "case", "catch", "char", "checked", "class", "const", "continue", "decimal", "default", "delegate", "do",
    "double", "else", "enum", "event", "explicit", "extern", "false", "finally", "fixed", "float", "for", "foreach", "goto", "if", "implicit", "in", "int",
    "interface", "internal", "is", "lock", "long", "namespace", "new", "null", "object", "operator", "out", "override", "params", "private", "protected", "public",
    "readonly", "ref", "return", "sbyte", "sealed", "short", "sizeof", "stackalloc", "static", "string", "struct", "switch", "this", "throw", "true", "try",
    "typeof", "uint", "ulong", "unchecked", "unsafe", "ushort", "using", "virtual", "void", "volatile", "while",
];

/// C# のキーワードか。
pub fn is_keyword(name: &str) -> bool {
    KEYWORDS.contains(&name)
}

/// 識別子。キーワードなら `@` を付ける。コンパイラが作った名前（`<Main>b__0_0`、`<PrivateImplementationDetails>`）は、
/// C# で書けない文字を `_` にする（宣言と参照で同じ規則なので、戻した C# はそのままコンパイルできる）。
pub fn ident(name: &str) -> String {
    if is_keyword(name) {
        return format!("@{name}");
    }
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) {
        return name.to_string();
    }
    let mangled: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if mangled.starts_with(|c: char| c.is_ascii_digit()) { format!("_{mangled}") } else { mangled }
}

/// フィールドの名前。自動プロパティのバッキング フィールド（`<X>k__BackingField`）は、プロパティの名前で書く
/// （宣言は `{ get; set; }` にしたので、フィールドの名前は書けない。コンストラクターの中の代入は、プロパティへの代入で同じ）。
pub fn member_name(name: &str) -> String {
    match name.strip_prefix('<').and_then(|rest| rest.strip_suffix(">k__BackingField")) {
        Some(property) => ident(property),
        None => ident(name),
    }
}

/// 型の名前の末尾の、型引数の数の印（`` `1 ``）を外す。
pub(super) fn without_arity(name: &str) -> &str {
    name.rsplit_once('`').map_or(name, |(base, _)| base)
}

/// 型の名前の末尾の、型引数の数。
pub(super) fn arity(name: &str) -> usize {
    name.rsplit_once('`').and_then(|(_, n)| n.parse().ok()).unwrap_or(0)
}

/// `System` 名前空間の組み込みの型の、キーワード。
fn keyword(namespace: &str, name: &str) -> Option<&'static str> {
    if namespace != "System" {
        return None;
    }
    Some(match name {
        "Boolean" => "bool",
        "Char" => "char",
        "SByte" => "sbyte",
        "Byte" => "byte",
        "Int16" => "short",
        "UInt16" => "ushort",
        "Int32" => "int",
        "UInt32" => "uint",
        "Int64" => "long",
        "UInt64" => "ulong",
        "Single" => "float",
        "Double" => "double",
        "Decimal" => "decimal",
        "String" => "string",
        "Object" => "object",
        "Void" => "void",
        _ => return None,
    })
}

/// C# の文字列リテラル。
pub fn string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl<'a> Names<'a> {
    pub fn new(md: &'a Metadata) -> Names<'a> {
        Names { md, used: RefCell::new(BTreeSet::new()), members: RefCell::default(), method_cache: RefCell::default(), lookups: OnceCell::new() }
    }

    /// 子から親を引く表（初めて使うときに作る）。
    pub fn lookups(&self) -> &Lookups {
        const FIELD_RVA: usize = 0x1D;
        const METHOD_IMPL: usize = 0x19;
        const PROPERTY_MAP: usize = 0x15;
        const EVENT_MAP: usize = 0x12;
        const METHOD_SEMANTICS: usize = 0x18;
        self.lookups.get_or_init(|| {
            let md = self.md;
            let mut lookups = Lookups::default();
            for row in 1..=md.rows(FIELD_RVA) {
                lookups.field_rva.insert(md.cell(FIELD_RVA, row, 1), md.cell(FIELD_RVA, row, 0));
            }
            // MethodBody は MethodDefOrRef（下位 1 ビットが 0 なら MethodDef）。
            for row in 1..=md.rows(METHOD_IMPL) {
                let body = md.cell(METHOD_IMPL, row, 1);
                if body & 1 == 0 {
                    lookups.method_impl.insert(body >> 1, md.cell(METHOD_IMPL, row, 2));
                }
            }
            for row in 1..=md.rows(PROPERTY_MAP) {
                lookups.property_map.insert(md.cell(PROPERTY_MAP, row, 0), row);
            }
            for row in 1..=md.rows(TYPE_DEF) {
                if md.enclosing_type(row).is_none() {
                    lookups.namespaces_of.entry(md.string(md.cell(TYPE_DEF, row, 1)).to_string()).or_default().insert(md.string(md.cell(TYPE_DEF, row, 2)).to_string());
                }
            }
            // TypeRef の ResolutionScope の下位 2 ビットが 3 なら入れ子（外側の型が scope）。
            for row in 1..=md.rows(TYPE_REF) {
                if md.cell(TYPE_REF, row, 0) & 3 != 3 {
                    lookups.namespaces_of.entry(md.string(md.cell(TYPE_REF, row, 1)).to_string()).or_default().insert(md.string(md.cell(TYPE_REF, row, 2)).to_string());
                }
            }

            for row in 1..=md.rows(EVENT_MAP) {
                lookups.event_map.insert(md.cell(EVENT_MAP, row, 0), row);
            }
            // MethodSemantics: AddOn（0x8）、RemoveOn（0x10）で、相手がイベント（HasSemantics の下位 1 ビットが 0）のもの。
            for row in 1..=md.rows(METHOD_SEMANTICS) {
                if md.cell(METHOD_SEMANTICS, row, 0) & 0x18 != 0 && md.cell(METHOD_SEMANTICS, row, 2) & 1 == 0 {
                    lookups.event_accessors.insert(md.cell(METHOD_SEMANTICS, row, 1));
                }
            }
            lookups
        })
    }

    /// これまでに書いた型の名前空間（`using` にする）を取り出して、覚えを空にする。
    pub fn take_namespaces(&self) -> BTreeSet<String> {
        self.method_cache.borrow_mut().clear();
        std::mem::take(&mut self.used.borrow_mut())
    }

    /// 1 つのファイルに書く型（入れ子でない型 `row`）のメンバーの名前を集める。入れ子の型と、このモジュールにある基底の
    /// 型の分も入れる。プロパティとイベントは、アクセサーの名前（`get_X`、`add_X`）から。
    pub fn enter_type(&self, row: u32) {
        self.method_cache.borrow_mut().clear();
        let mut members = self.members.borrow_mut();
        members.clear();
        let md = self.md;
        let mut types = vec![row];
        let mut index = 0;
        while index < types.len() {
            let current = types[index];
            index += 1;
            types.extend((1..=md.rows(TYPE_DEF)).filter(|&r| md.enclosing_type(r) == Some(current)));
            if let Some(base) = md.base_type(current).filter(|b| !types.contains(b)) {
                types.push(base);
            }
        }
        for &t in &types {
            for method in md.child_rows(TYPE_DEF, t, 5, METHOD_DEF) {
                let name = md.string(md.cell(METHOD_DEF, method, 3));
                let accessor = ["get_", "set_", "add_", "remove_"].iter().find_map(|p| name.strip_prefix(p));
                members.insert(accessor.unwrap_or(name).to_string());
            }
            for field in md.child_rows(TYPE_DEF, t, 4, FIELD) {
                members.insert(md.string(md.cell(FIELD, field, 1)).to_string());
            }
        }
    }

    fn use_namespace(&self, namespace: &str) {
        if !namespace.is_empty() {
            self.used.borrow_mut().insert(namespace.to_string());
        }
    }

    /// 入れ子でない型の名前の前に書くもの。ふつうは空で、名前空間を `using` に回す。同じ名前の型がほかの名前空間にも
    /// あれば、名前空間つきで書く（`Newtonsoft.Json.Serialization.ErrorEventArgs`）。
    fn namespace_prefix(&self, namespace: &str, raw: &str) -> String {
        if namespace.is_empty() {
            return String::new();
        }
        // メンバーの名前と同じ型の名前（`Convert` という名前のメソッドを持つ型の中の `Convert.ToInt32`）も、名前空間つき。
        if self.lookups().namespaces_of.get(raw).is_some_and(|set| set.len() > 1) || self.members.borrow().contains(raw) {
            return format!("{namespace}.");
        }
        self.use_namespace(namespace);
        String::new()
    }

    /// 型の型引数の名前（外側の型の分を含む、宣言の順）。
    pub fn type_generics(&self, type_row: u32) -> Vec<String> {
        self.md.generic_params(type_row << 1).into_iter().map(ident).collect()
    }

    /// メソッドの型引数の名前。
    pub fn method_generics(&self, method_row: u32) -> Vec<String> {
        self.md.generic_params(method_row << 1 | 1).into_iter().map(ident).collect()
    }

    /// 型の名前。
    pub fn ty(&self, ty: &Ty, g: &Generics) -> String {
        match ty {
            Ty::Void => "void".into(),
            Ty::Bool => "bool".into(),
            Ty::Char => "char".into(),
            Ty::I1 => "sbyte".into(),
            Ty::U1 => "byte".into(),
            Ty::I2 => "short".into(),
            Ty::U2 => "ushort".into(),
            Ty::I4 => "int".into(),
            Ty::U4 => "uint".into(),
            Ty::I8 => "long".into(),
            Ty::U8 => "ulong".into(),
            Ty::R4 => "float".into(),
            Ty::R8 => "double".into(),
            Ty::String => "string".into(),
            Ty::Object => "object".into(),
            Ty::IntPtr => "IntPtr".into(),
            Ty::UIntPtr => "UIntPtr".into(),
            Ty::TypedReference => "TypedReference".into(),
            Ty::Named { token, .. } => self.type_token(*token, &[], g),
            Ty::Generic { token, arguments, .. } => {
                let arguments: Vec<String> = arguments.iter().map(|a| self.ty(a, g)).collect();
                if self.is_nullable_type(*token) && arguments.len() == 1 {
                    return format!("{}?", arguments[0]);
                }
                self.type_token(*token, &arguments, g)
            }
            Ty::Var(i) => g.types.get(*i as usize).cloned().unwrap_or_else(|| format!("!{i}")),
            Ty::MVar(i) => g.methods.get(*i as usize).cloned().unwrap_or_else(|| format!("!!{i}")),
            Ty::SzArray(element) => format!("{}[]", self.ty(element, g)),
            Ty::Array { element, rank } => format!("{}[{}]", self.ty(element, g), ",".repeat(rank.saturating_sub(1) as usize)),
            Ty::Pointer(element) => format!("{}*", self.ty(element, g)),
            Ty::ByRef(element) => format!("ref {}", self.ty(element, g)),
            Ty::FnPtr(sig) => {
                let mut parts: Vec<String> = sig.params.iter().map(|p| self.ty(p, g)).collect();
                parts.push(self.ty(&sig.ret, g));
                let unmanaged = if sig.convention != 0 && sig.convention != 5 { " unmanaged" } else { "" };
                format!("delegate*{unmanaged}<{}>", parts.join(", "))
            }
            Ty::Modified { inner, .. } | Ty::Pinned(inner) => self.ty(inner, g),
        }
    }

    /// `System.Nullable<T>` か。構築された `Nullable<int>`（TypeSpec）も含む。
    pub fn is_nullable_type(&self, token: u32) -> bool {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            TYPE_REF => self.md.string(self.md.cell(TYPE_REF, row, 2)) == "System" && self.md.string(self.md.cell(TYPE_REF, row, 1)) == "Nullable`1",
            TYPE_DEF => self.md.string(self.md.cell(TYPE_DEF, row, 2)) == "System" && self.md.string(self.md.cell(TYPE_DEF, row, 1)) == "Nullable`1",
            TYPE_SPEC => match self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).and_then(sig::type_spec) {
                Ok(Ty::Generic { token, .. }) => self.is_nullable_type(token),
                _ => false,
            },
            _ => false,
        }
    }

    /// 型のトークン（TypeDef、TypeRef、TypeSpec）の名前。`arguments` は型引数（外側の型の分から）。
    pub fn type_token(&self, token: u32, arguments: &[String], g: &Generics) -> String {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            TYPE_DEF => self.type_def(row, arguments, 0),
            TYPE_REF => self.type_ref(row, arguments, 0).0,
            TYPE_SPEC => match self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).and_then(sig::type_spec) {
                Ok(ty) => self.ty(&ty, g),
                Err(_) => format!("[0x{token:08X}]"),
            },
            _ => format!("[0x{token:08X}]"),
        }
    }

    /// TypeDef の名前。入れ子の型は `Outer<T>.Inner`。型引数が無ければ、宣言の名前（`T`）を書く。
    pub fn type_def(&self, row: u32, arguments: &[String], depth: u32) -> String {
        let raw = self.md.string(self.md.cell(TYPE_DEF, row, 1));
        let namespace = self.md.string(self.md.cell(TYPE_DEF, row, 2));
        if let Some(keyword) = keyword(namespace, raw) {
            return keyword.into();
        }
        let declared = self.type_generics(row);
        let all: Vec<String> = (0..declared.len()).map(|i| arguments.get(i).cloned().unwrap_or_else(|| declared[i].clone())).collect();
        let own = arity(raw).min(all.len());
        let mut name = match self.md.enclosing_type(row).filter(|_| depth < 32) {
            Some(outer) => self.type_def(outer, &all[..all.len() - own], depth + 1) + ".",
            None => self.namespace_prefix(namespace, raw),
        };
        name.push_str(&ident(without_arity(raw)));
        push_arguments(&mut name, &all[all.len() - own..]);
        name
    }

    /// TypeRef の名前と、使った型引数の数。入れ子の型の TypeRef は、ResolutionScope が外側の TypeRef。
    fn type_ref(&self, row: u32, arguments: &[String], depth: u32) -> (String, usize) {
        let raw = self.md.string(self.md.cell(TYPE_REF, row, 1));
        let namespace = self.md.string(self.md.cell(TYPE_REF, row, 2));
        if let Some(keyword) = keyword(namespace, raw) {
            return (keyword.into(), 0);
        }
        let scope = self.md.cell(TYPE_REF, row, 0);
        let own = arity(raw);
        let (mut name, outer_count) = if scope & 3 == 3 && depth < 32 {
            // 外側の型は、型引数の前の方を使う。外側の型引数の数は、名前の印からしか分からない。
            let outer_total = arguments.len().saturating_sub(own);
            let (outer, used) = self.type_ref(scope >> 2, &arguments[..outer_total], depth + 1);
            (outer + ".", used)
        } else {
            (self.namespace_prefix(namespace, raw), 0)
        };
        name.push_str(&ident(without_arity(raw)));
        let own_arguments: Vec<String> = arguments.iter().skip(outer_count).take(own).cloned().collect();
        if own_arguments.len() == own {
            push_arguments(&mut name, &own_arguments);
        } else if own > 0 {
            // 開いたジェネリック型（`typeof(List<>)`）。
            name.push('<');
            name.push_str(&",".repeat(own - 1));
            name.push('>');
        }
        (name, outer_count + own)
    }

    /// メソッドのトークン（MethodDef、MemberRef、MethodSpec）が指すメソッド。`g` は、いま書いているメソッドの型引数。
    pub fn method_ref(&self, token: u32, g: &Generics) -> Option<MethodRef> {
        if !g.types.is_empty() || !g.methods.is_empty() {
            return self.resolve_method(token, g);
        }
        if let Some(found) = self.method_cache.borrow().get(&token) {
            return found.clone();
        }
        let found = self.resolve_method(token, g);
        self.method_cache.borrow_mut().insert(token, found.clone());
        found
    }

    fn resolve_method(&self, token: u32, g: &Generics) -> Option<MethodRef> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => {
                let owner = self.md.declaring_type(row)?;
                let sig = self.method_def_sig(row)?;
                let generics = Generics { types: self.type_generics(owner), methods: self.method_generics(row) };
                Some(MethodRef { declaring: self.type_def(owner, &[], 0), name: self.md.string(self.md.cell(METHOD_DEF, row, 3)).to_string(), sig, generics, method_arguments: Vec::new() })
            }
            MEMBER_REF => {
                let blob = self.md.blob(self.md.cell(MEMBER_REF, row, 2)).ok()?;
                let sig = sig::method(blob).ok()?;
                let (declaring, type_arguments) = self.member_parent(self.md.cell(MEMBER_REF, row, 0), g);
                let generics = Generics { types: type_arguments, methods: (0..sig.generic_count).map(|i| format!("!!{i}")).collect() };
                Some(MethodRef { declaring, name: self.md.string(self.md.cell(MEMBER_REF, row, 1)).to_string(), sig, generics, method_arguments: Vec::new() })
            }
            METHOD_SPEC => {
                let method = self.md.cell(METHOD_SPEC, row, 0);
                let inner = if method & 1 == 0 { (METHOD_DEF as u32) << 24 | method >> 1 } else { (MEMBER_REF as u32) << 24 | method >> 1 };
                let mut found = self.method_ref(inner, g)?;
                let arguments = sig::method_spec(self.md.blob(self.md.cell(METHOD_SPEC, row, 1)).ok()?).ok()?;
                found.method_arguments = arguments.iter().map(|a| self.ty(a, g)).collect();
                found.generics.methods = found.method_arguments.clone();
                Some(found)
            }
            _ => None,
        }
    }

    /// 呼び出し先（`Invoke`）を宣言している型が、デリゲートか。このモジュールの型は基底の型で、外の型は名前で見分ける。
    pub fn is_delegate(&self, method: u32) -> bool {
        let Some(owner) = self.method_owner(method) else { return false };
        let row = owner & 0x00FF_FFFF;
        let name = match (owner >> 24) as usize {
            TYPE_DEF => return self.md.base_type_name(row).is_some_and(|b| b.ends_with("MulticastDelegate")),
            TYPE_REF => self.md.string(self.md.cell(TYPE_REF, row, 1)).to_string(),
            TYPE_SPEC => match sig::type_spec(self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).ok().unwrap_or_default()) {
                Ok(Ty::Generic { token, .. }) if (token >> 24) as usize == TYPE_REF => self.md.string(self.md.cell(TYPE_REF, token & 0x00FF_FFFF, 1)).to_string(),
                Ok(Ty::Generic { token, .. }) if (token >> 24) as usize == TYPE_DEF => return self.md.base_type_name(token & 0x00FF_FFFF).is_some_and(|b| b.ends_with("MulticastDelegate")),
                _ => return false,
            },
            _ => return false,
        };
        let name = name.split('`').next().unwrap_or(&name);
        matches!(name, "Func" | "Action" | "Predicate" | "Comparison" | "Converter") || name.ends_with("EventHandler") || name.ends_with("Callback")
    }

    /// `typeof` に書く型。ジェネリックな型の定義そのもの（TypeDef のトークン）は、型引数を空にする（`typeof(List<>)`）。
    pub fn open_type(&self, token: u32, generics: &Generics) -> String {
        if (token >> 24) as usize == TYPE_DEF {
            let row = token & 0x00FF_FFFF;
            let count = self.md.type_parameter_count(row);
            // 外側の型もジェネリックな入れ子の型（`Outer<>.Inner<>`）は、型引数のまま書く。
            let outer_generic = self.md.enclosing_type(row).is_some_and(|outer| self.md.type_parameter_count(outer) > 0);
            if count > 0 && !outer_generic {
                let name = self.type_token(token, &[], generics);
                let base = name.split('<').next().unwrap_or(&name);
                return format!("{base}<{}>", ",".repeat(count - 1));
            }
        }
        self.type_token(token, &[], generics)
    }

    /// メソッドを宣言している型が、このモジュールの型なら、その TypeDef の行（ジェネリックな型の `C<T>` も、`C` の行）。
    pub fn owner_type_row(&self, token: u32) -> Option<u32> {
        let owner = self.method_owner(token)?;
        match (owner >> 24) as usize {
            TYPE_DEF => Some(owner & 0x00FF_FFFF),
            TYPE_SPEC => match sig::type_spec(self.md.blob(self.md.cell(TYPE_SPEC, owner & 0x00FF_FFFF, 0)).ok()?).ok()? {
                Ty::Generic { token, .. } if (token >> 24) as usize == TYPE_DEF => Some(token & 0x00FF_FFFF),
                _ => None,
            },
            _ => None,
        }
    }

    /// 呼び出し先のトークンが、このモジュールのメソッドを指すなら、その MethodDef の行。ジェネリックな型の中で自分の
    /// メソッドを呼ぶ所は、MemberRef（親が `C<T>` の TypeSpec）になるので、名前で引く。
    pub fn method_definition(&self, token: u32) -> Option<u32> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => Some(row),
            MEMBER_REF => {
                let type_row = self.owner_type_row(token)?;
                let rows = self.md.method_rows_named(type_row, self.md.string(self.md.cell(MEMBER_REF, row, 1)));
                // 同じ名前が 2 つ以上（多重定義）なら、どれか決めない。
                if rows.len() == 1 { rows.first().copied() } else { None }
            }
            _ => None,
        }
    }

    /// 呼び出し先やフィールドのシグネチャの型引数（`!0`、`!!0`）に当たる、呼び出す側の文脈の型。
    /// MemberRef の親が `List<string>` なら `!0` は `string`。MethodSpec ならメソッドの型引数も。MethodDef と
    /// FieldDef（同じアセンブリの、ジェネリックでない呼び出し）は、宣言の型引数がそのまま呼び出す側のものなので
    /// `None`（置き換えない）。
    pub fn instantiation(&self, token: u32) -> Option<(Vec<Ty>, Vec<Ty>)> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            MEMBER_REF => {
                let parent = self.md.cell(MEMBER_REF, row, 0);
                let types = if parent & 7 == 4 {
                    match sig::type_spec(self.md.blob(self.md.cell(TYPE_SPEC, parent >> 3, 0)).ok()?).ok()? {
                        Ty::Generic { arguments, .. } => arguments,
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                Some((types, Vec::new()))
            }
            METHOD_SPEC => {
                let method = self.md.cell(METHOD_SPEC, row, 0);
                let methods = sig::method_spec(self.md.blob(self.md.cell(METHOD_SPEC, row, 1)).ok()?).ok()?;
                let inner = if method & 1 == 0 { (METHOD_DEF as u32) << 24 | method >> 1 } else { (MEMBER_REF as u32) << 24 | method >> 1 };
                let types = match self.instantiation(inner) {
                    Some((types, _)) => types,
                    // 同じ型のジェネリックメソッド（MethodDef）: 型の型引数は、呼び出す側のもの。
                    None => self.md.declaring_type(method >> 1).map(|t| (0..self.md.type_parameter_count(t) as u32).map(Ty::Var).collect()).unwrap_or_default(),
                };
                Some((types, methods))
            }
            _ => None,
        }
    }

    /// 呼び出し先が、イベントの add / remove か。同じアセンブリのメソッドは MethodSemantics で、ほかのアセンブリのもの
    /// （MemberRef）は印が読めないので、名前（`add_`、`remove_`）と、引数がデリゲート（クラス）であることで見分ける。
    pub fn is_event_accessor(&self, token: u32) -> bool {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => self.lookups().event_accessors.contains(&row),
            MEMBER_REF => {
                let name = self.md.string(self.md.cell(MEMBER_REF, row, 1));
                (name.starts_with("add_") || name.starts_with("remove_"))
                    && self.method_sig(token).is_some_and(|s| s.ret.is_void() && matches!(s.params.as_slice(), [Ty::Named { value_type: false, .. } | Ty::Generic { value_type: false, .. }]))
            }
            _ => false,
        }
    }

    /// 型（TypeDef の行）が、インターフェイス `interface`（TypeDef、TypeRef、TypeSpec のトークン）を、自分の宣言で実装しているか。
    pub fn implements_directly(&self, row: u32, interface: u32) -> bool {
        use crate::debugger::metadata::INTERFACE_IMPL;
        self.md.matching_rows(INTERFACE_IMPL, 0, row).into_iter().any(|r| super::sig::type_def_or_ref_token(self.md.cell(INTERFACE_IMPL, r, 1)) == interface)
    }

    /// 型（TypeDef、TypeRef、TypeSpec のトークン）が、コレクション初期化子を書ける型（`IEnumerable` を実装する）か。
    /// このモジュールの型は、実装しているインターフェイスと基底の型をたどる。ほかのアセンブリの型は中を読めないので、
    /// `System.Collections` の名前空間のものだけをそうとみなす（分からない型は、初期化子にしない）。
    pub fn is_collection_type(&self, token: u32) -> bool {
        self.is_collection_type_at(token, 0)
    }

    fn is_collection_type_at(&self, token: u32, depth: u32) -> bool {
        if depth > 16 {
            return false;
        }
        let row = token & 0x00FF_FFFF;
        let collections_namespace = |namespace: &str| namespace == "System.Collections" || namespace.starts_with("System.Collections.");
        match (token >> 24) as usize {
            TYPE_REF => collections_namespace(self.md.string(self.md.cell(TYPE_REF, row, 2))),
            TYPE_SPEC => match self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).ok().and_then(|blob| sig::type_spec(blob).ok()) {
                Some(Ty::Generic { token, .. } | Ty::Named { token, .. }) => self.is_collection_type_at(token, depth + 1),
                _ => false,
            },
            TYPE_DEF => {
                use crate::debugger::metadata::INTERFACE_IMPL;
                let interfaces = self.md.matching_rows(INTERFACE_IMPL, 0, row).into_iter().map(|r| sig::type_def_or_ref_token(self.md.cell(INTERFACE_IMPL, r, 1)));
                let base = sig::type_def_or_ref_token(self.md.cell(TYPE_DEF, row, 3));
                interfaces.chain((base & 0x00FF_FFFF != 0).then_some(base)).any(|t| self.is_collection_type_at(t, depth + 1))
            }
            _ => false,
        }
    }

    /// `using` と `foreach` に戻してよいか。このモジュールの型は、`IDisposable` か `IAsyncDisposable` を実装しているかを見る。
    /// ほかのアセンブリの型は実装が読めないので、変数の型そのものがそのインターフェイスであるときだけ戻す。
    pub fn is_disposable(&self, ty: &Ty) -> bool {
        match ty.unmodified() {
            Ty::Named { token, .. } | Ty::Generic { token, .. } => self.disposable_token(*token, 0),
            Ty::ByRef(inner) | Ty::Pointer(inner) => self.is_disposable(inner),
            _ => false,
        }
    }

    fn disposable_token(&self, token: u32, depth: u32) -> bool {
        if depth > 16 {
            return false;
        }
        if self.disposable_name(token) {
            return true;
        }
        let row = token & 0x00FF_FFFF;
        if (token >> 24) as usize != TYPE_DEF || row == 0 {
            return false;
        }
        use crate::debugger::metadata::INTERFACE_IMPL;
        let interfaces = self.md.matching_rows(INTERFACE_IMPL, 0, row).into_iter().map(|r| sig::type_def_or_ref_token(self.md.cell(INTERFACE_IMPL, r, 1)));
        let base = sig::type_def_or_ref_token(self.md.cell(TYPE_DEF, row, 3));
        interfaces.chain((base & 0x00FF_FFFF != 0).then_some(base)).any(|next| self.disposable_token(next, depth + 1))
    }

    fn disposable_name(&self, token: u32) -> bool {
        let row = token & 0x00FF_FFFF;
        let raw = match (token >> 24) as usize {
            TYPE_DEF => self.md.string(self.md.cell(TYPE_DEF, row, 1)),
            TYPE_REF => self.md.string(self.md.cell(TYPE_REF, row, 1)),
            TYPE_SPEC => {
                let Some(ty) = self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).ok().and_then(|blob| sig::type_spec(blob).ok()) else { return false };
                return match ty {
                    Ty::Named { token, .. } | Ty::Generic { token, .. } => self.disposable_name(token),
                    _ => false,
                };
            }
            _ => return false,
        };
        let name = raw.split('`').next().unwrap_or(raw);
        matches!(name, "IDisposable" | "IAsyncDisposable")
    }

    /// このモジュールのメソッドが、拡張メソッドか（`this` の印が付いている）。
    pub fn is_extension_method(&self, token: u32) -> bool {
        if (token >> 24) as usize != METHOD_DEF {
            return false;
        }
        self.md.method_shape(token & 0x00FF_FFFF).is_some_and(|shape| shape.is_extension)
    }

    /// メソッドを宣言している型のトークン（TypeDef、TypeRef、TypeSpec）。`new` の式の型に使う。
    pub fn method_owner(&self, token: u32) -> Option<u32> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => self.md.declaring_type(row).map(|r| (TYPE_DEF as u32) << 24 | r),
            MEMBER_REF => {
                let parent = self.md.cell(MEMBER_REF, row, 0);
                let table = match parent & 7 {
                    0 => TYPE_DEF,
                    1 => TYPE_REF,
                    4 => TYPE_SPEC,
                    _ => return None,
                };
                Some((table as u32) << 24 | parent >> 3)
            }
            _ => None,
        }
    }

    /// MemberRef の親（MemberRefParent）の名前と、その型引数（シグネチャの `!0` の名前）。
    fn member_parent(&self, coded: u32, g: &Generics) -> (String, Vec<String>) {
        let row = coded >> 3;
        match coded & 7 {
            0 => (self.type_def(row, &[], 0), self.type_generics(row)),
            1 => (self.type_ref(row, &[], 0).0, Vec::new()),
            // ModuleRef（グローバルな関数）と MethodDef（vararg）は、型の名前を書かない。
            2 | 3 => (String::new(), Vec::new()),
            _ => match self.md.blob(self.md.cell(TYPE_SPEC, row, 0)).and_then(sig::type_spec) {
                Ok(Ty::Generic { token, arguments, .. }) => {
                    let names: Vec<String> = arguments.iter().map(|a| self.ty(a, g)).collect();
                    (self.type_token(token, &names, g), names)
                }
                Ok(other) => (self.ty(&other, g), Vec::new()),
                Err(_) => (format!("[0x{:08X}]", 0x1B00_0000 | row), Vec::new()),
            },
        }
    }

    /// 呼び出し先の書き方: `int Program.Compute(int)`。
    pub fn call_text(&self, method: &MethodRef) -> String {
        let params: Vec<String> = method.sig.params.iter().map(|p| self.ty(p, &method.generics)).collect();
        let mut name = ident(&method.name);
        push_arguments(&mut name, &method.method_arguments);
        let owner = if method.declaring.is_empty() { String::new() } else { format!("{}.", method.declaring) };
        format!("{} {owner}{name}({})", self.ty(&method.sig.ret, &method.generics), params.join(", "))
    }

    /// フィールドのトークン（FieldDef、MemberRef）の書き方: `int Point.X`。
    pub fn field_text(&self, token: u32, g: &Generics) -> String {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            FIELD => {
                let Some((owner, field)) = self.md.field_declaration(row) else { return format!("[0x{token:08X}]") };
                let generics = Generics { types: self.type_generics(owner), methods: Vec::new() };
                let ty = self.field_def_type(row).map(|t| self.ty(&t, &generics)).unwrap_or_default();
                format!("{ty} {}.{}", self.type_def(owner, &[], 0), ident(field.name))
            }
            MEMBER_REF => {
                let (declaring, arguments) = self.member_parent(self.md.cell(MEMBER_REF, row, 0), g);
                let generics = Generics { types: arguments, methods: Vec::new() };
                let ty = self.md.blob(self.md.cell(MEMBER_REF, row, 2)).and_then(sig::field).map(|t| self.ty(&t, &generics)).unwrap_or_default();
                format!("{ty} {declaring}.{}", ident(self.md.string(self.md.cell(MEMBER_REF, row, 1))))
            }
            _ => format!("[0x{token:08X}]"),
        }
    }

    /// 表示用のクラスの、外の `this` を入れたフィールド（`<>4__this`）。ジェネリックな型では MemberRef。
    pub fn is_outer_this_field(&self, token: u32) -> bool {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            FIELD => self.md.field_declaration(row).is_some_and(|(_, field)| field.name == "<>4__this"),
            MEMBER_REF => self.md.string(self.md.cell(MEMBER_REF, row, 1)) == "<>4__this",
            _ => false,
        }
    }

    /// フィールドの「持ち主の型、名前」（`Point`、`X`）。
    pub fn field_parts(&self, token: u32, g: &Generics) -> (String, String) {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            FIELD => match self.md.field_declaration(row) {
                Some((owner, field)) => (self.type_def(owner, &[], 0), member_name(field.name)),
                None => (String::new(), format!("[0x{token:08X}]")),
            },
            MEMBER_REF => (self.member_parent(self.md.cell(MEMBER_REF, row, 0), g).0, member_name(self.md.string(self.md.cell(MEMBER_REF, row, 1)))),
            _ => (String::new(), format!("[0x{token:08X}]")),
        }
    }

    /// メソッドの `out` の引数の位置（0 始まり）。MethodDef は Param の印から。ほかのアセンブリのメソッド（MemberRef）は
    /// 印が読めないので、`TryXxx` の最後の ByRef の引数を `out` とみなす（`int.TryParse(s, out n)` など）。
    pub fn out_parameters(&self, token: u32) -> Vec<usize> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => match self.method_def_sig(row) {
                Some(sig) => self.parameters(row, &sig).iter().enumerate().filter(|(_, p)| p.out).map(|(i, _)| i).collect(),
                None => Vec::new(),
            },
            MEMBER_REF => {
                let name = self.md.string(self.md.cell(MEMBER_REF, row, 1));
                match self.method_sig(token) {
                    Some(sig) if name.starts_with("Try") && sig.params.last().is_some_and(|t| matches!(t.unmodified(), Ty::ByRef(_))) => vec![sig.params.len() - 1],
                    _ => Vec::new(),
                }
            }
            METHOD_SPEC => {
                let method = self.md.cell(METHOD_SPEC, row, 0);
                let table = if method & 1 == 0 { METHOD_DEF } else { MEMBER_REF };
                self.out_parameters((table as u32) << 24 | method >> 1)
            }
            _ => Vec::new(),
        }
    }

    /// `calli` のシグネチャ（StandAloneSig）。
    pub fn signature_text(&self, token: u32, g: &Generics) -> String {
        match self.md.blob(self.md.cell(STAND_ALONE_SIG, token & 0x00FF_FFFF, 0)).and_then(sig::method) {
            Ok(sig) => self.ty(&Ty::FnPtr(Box::new(sig)), g),
            Err(_) => format!("[0x{token:08X}]"),
        }
    }

    /// `calli` のシグネチャ（評価スタックの出入りを数えるため）。
    pub fn signature(&self, token: u32) -> Option<MethodSig> {
        sig::method(self.md.blob(self.md.cell(STAND_ALONE_SIG, token & 0x00FF_FFFF, 0)).ok()?).ok()
    }

    /// ローカル変数のシグネチャ（StandAloneSig のトークン。0 なら無し）の、変数ごとの型。
    pub fn local_types(&self, token: u32) -> Vec<Ty> {
        if token == 0 {
            return Vec::new();
        }
        self.md.blob(self.md.cell(STAND_ALONE_SIG, token & 0x00FF_FFFF, 0)).ok().and_then(|b| sig::locals(b).ok()).unwrap_or_default()
    }

    /// MethodDef の行のシグネチャ。
    pub fn method_def_sig(&self, row: u32) -> Option<MethodSig> {
        sig::method(self.md.blob(self.md.cell(METHOD_DEF, row, 4)).ok()?).ok()
    }

    /// FieldDef の行の型。
    pub fn field_def_type(&self, row: u32) -> Option<Ty> {
        sig::field(self.md.blob(self.md.cell(FIELD, row, 2)).ok()?).ok()
    }

    /// 呼び出し先（MethodDef、MemberRef、MethodSpec）のシグネチャだけ。名前は組み立てない（評価スタックの出入りを
    /// 数えるのは、すべての呼び出し命令でするので、`method_ref` より軽くする）。
    pub fn method_sig(&self, token: u32) -> Option<MethodSig> {
        let row = token & 0x00FF_FFFF;
        match (token >> 24) as usize {
            METHOD_DEF => self.method_def_sig(row),
            MEMBER_REF => sig::method(self.md.blob(self.md.cell(MEMBER_REF, row, 2)).ok()?).ok(),
            METHOD_SPEC => {
                let method = self.md.cell(METHOD_SPEC, row, 0);
                let table = if method & 1 == 0 { METHOD_DEF } else { MEMBER_REF };
                self.method_sig((table as u32) << 24 | method >> 1)
            }
            _ => None,
        }
    }
}

/// 型引数を `<…>` で付ける（無ければ何もしない）。
fn push_arguments(name: &mut String, arguments: &[String]) {
    if !arguments.is_empty() {
        name.push('<');
        name.push_str(&arguments.join(", "));
        name.push('>');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_escaped_and_compiler_names_are_kept() {
        assert_eq!(ident("class"), "@class");
        assert_eq!(ident("value"), "value");
        assert_eq!(ident("<Main>b__0_0"), "_Main_b__0_0");
        assert_eq!(ident("<>c"), "__c");
    }

    #[test]
    fn string_literals_escape_what_csharp_needs() {
        assert_eq!(string_literal("a\"b\\c\n\t"), r#""a\"b\\c\n\t""#);
        assert_eq!(string_literal("\u{1}"), r#""\u0001""#);
        assert_eq!(string_literal("日本"), "\"日本\"");
    }

    #[test]
    fn system_types_become_keywords() {
        assert_eq!(keyword("System", "Int32"), Some("int"));
        assert_eq!(keyword("System", "IntPtr"), None);
        assert_eq!(keyword("Other", "Int32"), None);
    }
}
