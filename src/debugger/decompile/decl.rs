//! 型とメンバーの宣言（本体の外側）の、C# の書き方。IL の表示（段階 1）と C# の印字（段階 2）の両方が使う。

use super::names::{arity, ident, string_literal, without_arity, Generics, Names};
use super::sig::{self, Ty};
use crate::debugger::metadata::{EVENT, FIELD, GENERIC_PARAM, INTERFACE_IMPL, METHOD_DEF, PARAM, PROPERTY, TYPE_DEF, TYPE_REF};

/// 型の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Struct,
    Interface,
    Enum,
    Delegate,
}

/// プロパティ 1 つ（PropertyMap、Property、MethodSemantics から）。
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub ty: Ty,
    /// インデクサーの引数の型（ふつうのプロパティは空）。
    pub params: Vec<Ty>,
    pub getter: Option<u32>,
    pub setter: Option<u32>,
}

/// イベント 1 つ（EventMap、Event、MethodSemantics から）。
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    /// イベントの型（デリゲート）のトークン。
    pub ty: u32,
    pub add: Option<u32>,
    pub remove: Option<u32>,
    /// イベントと同じ名前のフィールド（フィールドのようなイベントの、デリゲートの入れ物）。
    pub backing_field: Option<u32>,
}

/// 引数 1 つ。
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub ty: Ty,
    /// `out`（ByRef で Out の印）。
    pub out: bool,
    /// `in`（ByRef で In の印、`IsReadOnlyAttribute` の modreq）。
    pub is_in: bool,
}

/// TypeDef の Flags の、型の見え方（入れ子の型は別の番号）。
fn type_visibility(flags: u32) -> &'static str {
    match flags & 0x7 {
        1 | 2 => "public",
        3 => "private",
        4 => "protected",
        6 => "private protected",
        7 => "protected internal",
        _ => "internal",
    }
}

/// MethodDef と FieldDef の Flags の、メンバーの見え方。
fn member_visibility(flags: u32) -> &'static str {
    match flags & 0x7 {
        1 => "private",
        2 => "private protected",
        3 => "internal",
        4 => "protected",
        5 => "protected internal",
        6 => "public",
        // CompilerControlled（コンパイラだけが使う）。C# では書けないので private にする。
        _ => "private",
    }
}

impl Names<'_> {
    /// 基底の型の「名前空間、名前」。無ければ `None`。
    fn base_of(&self, row: u32) -> Option<(&str, &str)> {
        let extends = self.md.cell(TYPE_DEF, row, 3);
        let target = extends >> 2;
        match extends & 3 {
            0 if target != 0 => Some((self.md.string(self.md.cell(TYPE_DEF, target, 2)), self.md.string(self.md.cell(TYPE_DEF, target, 1)))),
            1 => Some((self.md.string(self.md.cell(TYPE_REF, target, 2)), self.md.string(self.md.cell(TYPE_REF, target, 1)))),
            _ => None,
        }
    }

    pub fn type_kind(&self, row: u32) -> TypeKind {
        const INTERFACE: u32 = 0x20;
        if self.md.cell(TYPE_DEF, row, 0) & INTERFACE != 0 {
            return TypeKind::Interface;
        }
        let own = (self.md.string(self.md.cell(TYPE_DEF, row, 2)), self.md.string(self.md.cell(TYPE_DEF, row, 1)));
        match self.base_of(row) {
            Some(("System", "Enum")) => TypeKind::Enum,
            Some(("System", "ValueType")) if own != ("System", "Enum") => TypeKind::Struct,
            Some(("System", "MulticastDelegate")) => TypeKind::Delegate,
            _ => TypeKind::Class,
        }
    }

    /// 型の宣言の頭: `public static class Program : Base, IFoo`。
    pub fn type_header(&self, row: u32) -> String {
        const ABSTRACT: u32 = 0x80;
        const SEALED: u32 = 0x100;
        let flags = self.md.cell(TYPE_DEF, row, 0);
        let kind = self.type_kind(row);
        let mut parts = vec![type_visibility(flags).to_string()];
        if kind == TypeKind::Class {
            match (flags & ABSTRACT != 0, flags & SEALED != 0) {
                (true, true) => parts.push("static".into()),
                (true, false) => parts.push("abstract".into()),
                (false, true) => parts.push("sealed".into()),
                _ => {}
            }
        }
        parts.push(match kind {
            TypeKind::Class => "class",
            TypeKind::Struct => "struct",
            TypeKind::Interface => "interface",
            TypeKind::Enum => "enum",
            TypeKind::Delegate => "delegate",
        }
        .into());
        let raw = self.md.string(self.md.cell(TYPE_DEF, row, 1));
        let own_names = self.own_type_generics(row);
        let mut name = ident(without_arity(raw));
        if !own_names.is_empty() {
            name = format!("{name}<{}>", own_names.join(", "));
        }
        let generics = Generics { types: self.type_generics(row), methods: Vec::new() };
        let mut bases = Vec::new();
        if kind == TypeKind::Enum {
            if let Some(underlying) = self.enum_underlying(row).filter(|t| *t != Ty::I4) {
                bases.push(self.ty(&underlying, &generics));
            }
        } else if kind == TypeKind::Class {
            let extends = self.md.cell(TYPE_DEF, row, 3);
            if extends != 0 && self.base_of(row) != Some(("System", "Object")) {
                bases.push(self.type_token(sig::type_def_or_ref_token(extends), &[], &generics));
            }
        }
        if kind != TypeKind::Enum && kind != TypeKind::Delegate {
            bases.extend(self.interfaces(row, &generics));
        }
        let mut header = parts.join(" ") + " " + &name;
        if !bases.is_empty() {
            header.push_str(" : ");
            header.push_str(&bases.join(", "));
        }
        let inherited = self.type_generics(row).len() - own_names.len();
        header.push_str(&self.constraints(row << 1, inherited, &generics));
        header
    }

    /// 型が自分で足した型引数の名前（入れ子の型では、外側の型の分を除く）。
    fn own_type_generics(&self, row: u32) -> Vec<String> {
        let raw = self.md.string(self.md.cell(TYPE_DEF, row, 1));
        let own = arity(raw);
        let all = self.type_generics(row);
        all[all.len().saturating_sub(own)..].to_vec()
    }

    /// 型が実装しているインターフェイス（InterfaceImpl。Class の順に並んでいる）。
    fn interfaces(&self, row: u32, generics: &Generics) -> Vec<String> {
        self.md.matching_rows(INTERFACE_IMPL, 0, row).into_iter().map(|r| self.type_token(sig::type_def_or_ref_token(self.md.cell(INTERFACE_IMPL, r, 1)), &[], generics)).collect()
    }

    /// 型のプロパティ（宣言の順）。
    pub fn properties(&self, type_row: u32) -> Vec<Property> {
        const PROPERTY_MAP: usize = 0x15;
        const METHOD_SEMANTICS: usize = 0x18;
        const SETTER: u32 = 0x1;
        const GETTER: u32 = 0x2;
        let Some(&map) = self.lookups().property_map.get(&type_row) else { return Vec::new() };
        self.md
            .child_rows(PROPERTY_MAP, map, 1, PROPERTY)
            .filter_map(|row| {
                let (ty, params) = sig::property(self.md.blob(self.md.cell(PROPERTY, row, 2)).ok()?).ok()?;
                let mut property = Property { name: self.md.string(self.md.cell(PROPERTY, row, 1)).to_string(), ty, params, getter: None, setter: None };
                // HasSemantics: 下位 1 ビットが 1 ならプロパティ。MethodSemantics は Association の順に並んでいる。
                for semantics in self.md.matching_rows(METHOD_SEMANTICS, 2, row << 1 | 1) {
                    let method = self.md.cell(METHOD_SEMANTICS, semantics, 1);
                    match self.md.cell(METHOD_SEMANTICS, semantics, 0) {
                        GETTER => property.getter = Some(method),
                        SETTER => property.setter = Some(method),
                        _ => {}
                    }
                }
                (property.getter.is_some() || property.setter.is_some()).then_some(property)
            })
            .collect()
    }

    /// 型のイベント（宣言の順）。
    pub fn events(&self, type_row: u32) -> Vec<Event> {
        const EVENT_MAP: usize = 0x12;
        const METHOD_SEMANTICS: usize = 0x18;
        const ADD: u32 = 0x8;
        const REMOVE: u32 = 0x10;
        let Some(&map) = self.lookups().event_map.get(&type_row) else { return Vec::new() };
        self.md
            .child_rows(EVENT_MAP, map, 1, EVENT)
            .map(|row| {
                let name = self.md.string(self.md.cell(EVENT, row, 1)).to_string();
                let mut event = Event { ty: sig::type_def_or_ref_token(self.md.cell(EVENT, row, 2)), add: None, remove: None, backing_field: None, name };
                // HasSemantics: 下位 1 ビットが 0 ならイベント。
                for semantics in self.md.matching_rows(METHOD_SEMANTICS, 2, row << 1) {
                    let method = self.md.cell(METHOD_SEMANTICS, semantics, 1);
                    match self.md.cell(METHOD_SEMANTICS, semantics, 0) {
                        ADD => event.add = Some(method),
                        REMOVE => event.remove = Some(method),
                        _ => {}
                    }
                }
                event.backing_field = self.md.child_rows(TYPE_DEF, type_row, 4, FIELD).find(|&f| self.md.string(self.md.cell(FIELD, f, 1)) == event.name);
                event
            })
            .collect()
    }

    /// イベントの宣言の頭: `public event EventHandler Changed`（明示的な実装なら `event EventHandler INotify.Changed`）。
    pub fn event_header(&self, event: &Event, kind: TypeKind, generics: &Generics) -> String {
        let main = event.add.or(event.remove).unwrap_or(0);
        let explicit = self.explicit_interface(main);
        let mut parts = if explicit.is_some() { Vec::new() } else { self.method_modifiers(main, kind) };
        let simple = event.name.rsplit('.').next().unwrap_or(&event.name);
        let owner = explicit.map(|i| format!("{i}.")).unwrap_or_default();
        parts.push(format!("event {} {owner}{}", self.type_token(event.ty, &[], generics), ident(simple)));
        parts.join(" ")
    }

    /// プロパティが自動プロパティなら、そのバッキング フィールド（`<Name>k__BackingField`）の行。
    pub fn backing_field(&self, type_row: u32, property: &Property) -> Option<u32> {
        let name = format!("<{}>k__BackingField", property.name);
        self.md.child_rows(TYPE_DEF, type_row, 4, FIELD).find(|&f| self.md.string(self.md.cell(FIELD, f, 1)) == name)
    }

    /// プロパティの宣言の頭: `public int Count`、`public string this[int index]`。修飾子は、主なアクセサー（getter、
    /// 無ければ setter）のもの。
    pub fn property_header(&self, property: &Property, kind: TypeKind, generics: &Generics) -> String {
        let main = property.getter.or(property.setter).unwrap_or(0);
        let ty = self.ty(&property.ty, generics);
        // 明示的なインターフェイスの実装（`object IEnumerator.Current`）。修飾子は書けない。
        let explicit = self.explicit_interface(main);
        let mut parts = if explicit.is_some() { Vec::new() } else { self.method_modifiers(main, kind) };
        let simple = property.name.rsplit('.').next().unwrap_or(&property.name);
        let owner = explicit.map(|i| format!("{i}.")).unwrap_or_default();
        if property.params.is_empty() {
            parts.push(format!("{ty} {owner}{}", ident(simple)));
        } else {
            // インデクサーの引数の名前は、getter（無ければ setter の、最後の value を除いた）引数から。
            let names: Vec<String> = self.method_def_sig(main).map(|sig| self.parameters(main, &sig).into_iter().map(|p| p.name).collect()).unwrap_or_default();
            let params: Vec<String> = property.params.iter().enumerate().map(|(i, t)| format!("{} {}", self.ty(t, generics), names.get(i).cloned().unwrap_or_else(|| format!("index{i}")))).collect();
            parts.push(format!("{ty} {owner}this[{}]", params.join(", ")));
        }
        parts.join(" ")
    }

    /// アクセサーの見え方が、プロパティ（主なアクセサー）より狭ければ、その印（`private`）。
    pub fn accessor_visibility(&self, property: &Property, accessor: u32) -> Option<&'static str> {
        let main = property.getter.or(property.setter)?;
        if self.explicit_interface(main).is_some() {
            return None;
        }
        let (a, b) = (self.md.cell(METHOD_DEF, accessor, 2) & 0x7, self.md.cell(METHOD_DEF, main, 2) & 0x7);
        (a != b).then(|| member_visibility(a))
    }

    /// フィールドが、PE の像にデータを持つもの（配列の初期化子のデータ。FieldRVA）か。
    pub fn has_data(&self, field_row: u32) -> bool {
        self.lookups().field_rva.contains_key(&field_row)
    }

    /// 型のトークンが、このアセンブリの列挙型なら、その「元の整数の型、型の名前」。
    pub fn enum_of(&self, token: u32) -> Option<(Ty, String)> {
        if (token >> 24) as usize != TYPE_DEF {
            return None;
        }
        let row = token & 0x00FF_FFFF;
        (self.type_kind(row) == TypeKind::Enum).then(|| Some((self.enum_underlying(row)?, self.type_def(row, &[], 0))))?
    }

    /// 列挙型の値の型（`value__` フィールドの型）。
    pub fn enum_underlying(&self, row: u32) -> Option<Ty> {
        const STATIC: u32 = 0x10;
        self.md
            .child_rows(TYPE_DEF, row, 4, FIELD)
            .find(|&f| self.md.cell(FIELD, f, 0) & STATIC == 0)
            .and_then(|f| self.field_def_type(f))
    }

    /// フィールドの宣言（`;` まで）。列挙型の値は `Name = 1,`。
    pub fn field_declaration(&self, field_row: u32, kind: TypeKind, generics: &Generics) -> Option<String> {
        const STATIC: u32 = 0x10;
        const INIT_ONLY: u32 = 0x20;
        const LITERAL: u32 = 0x40;
        let flags = self.md.cell(FIELD, field_row, 0);
        let name = ident(self.md.string(self.md.cell(FIELD, field_row, 1)));
        if kind == TypeKind::Enum {
            // 値の器（value__）は書かない。
            if flags & STATIC == 0 {
                return None;
            }
            let value = self.field_constant_text(field_row).unwrap_or_default();
            return Some(if value.is_empty() { format!("{name},") } else { format!("{name} = {value},") });
        }
        let ty = self.field_def_type(field_row).map(|t| self.ty(&t, generics)).unwrap_or_else(|| "object".into());
        let mut parts = vec![member_visibility(flags).to_string()];
        if flags & LITERAL != 0 {
            parts.push("const".into());
        } else {
            if flags & STATIC != 0 {
                parts.push("static".into());
            }
            if flags & INIT_ONLY != 0 {
                parts.push("readonly".into());
            }
        }
        let ty_name = ty.clone();
        parts.push(ty);
        parts.push(name);
        let mut text = parts.join(" ");
        if flags & LITERAL != 0 {
            if let Some(value) = self.field_constant_text(field_row) {
                text.push_str(" = ");
                // 列挙型の定数は、値を型にキャストする（定数表には元の整数の型で入っている）。
                if let Some(Ty::Named { value_type: true, .. }) = self.field_def_type(field_row) {
                    text.push_str(&format!("({ty_name})"));
                }
                text.push_str(&value);
            }
        }
        text.push(';');
        Some(text)
    }

    /// 定数のフィールド（const、列挙型の値）の値を、C# のリテラルにする。
    pub fn field_constant_text(&self, field_row: u32) -> Option<String> {
        let (kind, bytes) = self.md.constant(field_row)?;
        constant_literal(kind, bytes)
    }

    /// メソッドの引数（宣言の順）。名前は Param 表から（無ければ `A_1` の形）。
    pub fn parameters(&self, method_row: u32, sig: &sig::MethodSig) -> Vec<Parameter> {
        const IN: u32 = 0x1;
        const OUT: u32 = 0x2;
        let mut names = vec![None; sig.params.len()];
        let mut flags = vec![0u32; sig.params.len()];
        for param in self.md.child_rows(METHOD_DEF, method_row, 5, PARAM) {
            let sequence = self.md.cell(PARAM, param, 1) as usize;
            if (1..=sig.params.len()).contains(&sequence) {
                names[sequence - 1] = Some(self.md.string(self.md.cell(PARAM, param, 2)).to_string());
                flags[sequence - 1] = self.md.cell(PARAM, param, 0);
            }
        }
        sig.params
            .iter()
            .enumerate()
            .map(|(i, ty)| {
                let by_ref = matches!(ty.unmodified(), Ty::ByRef(_));
                let name = names[i].clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("A_{}", i + 1));
                Parameter { name: ident(&name), ty: ty.clone(), out: by_ref && flags[i] & OUT != 0 && flags[i] & IN == 0, is_in: by_ref && flags[i] & IN != 0 && flags[i] & OUT == 0 }
            })
            .collect()
    }

    /// 引数の書き方: `ref int x`、`out string s`。
    pub fn parameter_text(&self, parameter: &Parameter, generics: &Generics) -> String {
        let ty = match parameter.ty.unmodified() {
            Ty::ByRef(inner) => {
                let modifier = if parameter.out {
                    "out"
                } else if parameter.is_in {
                    "in"
                } else {
                    "ref"
                };
                format!("{modifier} {}", self.ty(inner, generics))
            }
            other => self.ty(other, generics),
        };
        format!("{ty} {}", parameter.name)
    }

    /// メソッドの宣言の頭（本体の前まで）: `public static int Compute(int x)`。
    /// 本体が無いメソッド（abstract、extern、インターフェイス）は、呼び出し側が `;` を付ける。
    pub fn method_header(&self, method_row: u32, kind: TypeKind, generics: &Generics) -> String {
        let raw_name = self.md.string(self.md.cell(METHOD_DEF, method_row, 3));
        let Some(sig) = self.method_def_sig(method_row) else {
            return format!("/* {} */ void {}()", tr_str!("Cannot read the signature"), ident(raw_name));
        };
        let parameters: Vec<String> = self.parameters(method_row, &sig).iter().map(|p| self.parameter_text(p, generics)).collect();
        // 明示的なインターフェイスの実装（`IEnumerator IEnumerable.GetEnumerator()`）。修飾子は書けない。
        if let Some(interface) = self.explicit_interface(method_row) {
            let simple = raw_name.rsplit('.').next().unwrap_or(raw_name);
            let mut name = format!("{interface}.{}", ident(simple));
            if !generics.methods.is_empty() {
                name = format!("{name}<{}>", generics.methods.join(", "));
            }
            return format!("{} {name}({})", self.ty(&sig.ret, generics), parameters.join(", "));
        }
        // 演算子（`op_Addition`、`op_Explicit`）。戻り値の型だけが違う変換演算子も、この書き方なら重ならない。
        if let Some(text) = self.operator_header(method_row, raw_name, &sig, &parameters, generics) {
            return text;
        }
        let mut parts = self.method_modifiers(method_row, kind);
        // 静的コンストラクターには、見え方を書けない。
        if raw_name == ".cctor" {
            parts.retain(|p| p == "static");
        }
        let type_name = self.md.declaring_type(method_row).map(|t| {
            let raw = self.md.string(self.md.cell(TYPE_DEF, t, 1));
            ident(without_arity(raw))
        });
        let signature = match (raw_name, type_name) {
            (".ctor", Some(type_name)) => format!("{type_name}({})", parameters.join(", ")),
            (".cctor", Some(type_name)) => format!("{type_name}()"),
            _ => {
                let mut name = ident(raw_name);
                if !generics.methods.is_empty() {
                    name = format!("{name}<{}>", generics.methods.join(", "));
                }
                // override のメソッドには、制約を書けない（基底のものを受け継ぐ）。
                let constraints = if parts.iter().any(|p| p.contains("override")) { String::new() } else { self.constraints(method_row << 1 | 1, 0, generics) };
                format!("{} {name}({}){constraints}", self.ty(&sig.ret, generics), parameters.join(", "))
            }
        };
        parts.push(signature);
        parts.join(" ")
    }

    /// メソッドが、インターフェイスのメソッドを明示的に実装していれば、そのインターフェイスの名前（MethodImpl から）。
    /// 名前に `.` を含むもの（`System.IDisposable.Dispose`）か、public でないもの（状態機械の `MoveNext`）。public で
    /// 同じ名前なら、ふつうの（暗黙の）実装なので `None`。
    pub fn explicit_interface(&self, method_row: u32) -> Option<String> {
        const PUBLIC: u32 = 6;
        let name = self.md.string(self.md.cell(METHOD_DEF, method_row, 3));
        if name.starts_with('.') || (!name.contains('.') && self.md.cell(METHOD_DEF, method_row, 2) & 0x7 == PUBLIC) {
            return None;
        }
        let declaration = *self.lookups().method_impl.get(&method_row)?;
        let token = if declaration & 1 == 0 { (METHOD_DEF as u32) << 24 | declaration >> 1 } else { 0x0A00_0000 | declaration >> 1 };
        // インターフェイスの型引数（`IEnumerable<T>` の `T`）は、宣言している型とメソッドの型引数の名前で書く。
        let owner = self.md.declaring_type(method_row)?;
        let generics = Generics { types: self.type_generics(owner), methods: self.method_generics(method_row) };
        Some(self.method_ref(token, &generics)?.declaring)
    }

    /// 演算子の宣言の頭。演算子でなければ `None`。
    fn operator_header(&self, method_row: u32, name: &str, sig: &sig::MethodSig, parameters: &[String], generics: &Generics) -> Option<String> {
        const SPECIAL_NAME: u32 = 0x800;
        if self.md.cell(METHOD_DEF, method_row, 2) & SPECIAL_NAME == 0 || sig.has_this {
            return None;
        }
        let ret = self.ty(&sig.ret, generics);
        let params = parameters.join(", ");
        let symbol = match name {
            "op_Implicit" => return Some(format!("public static implicit operator {ret}({params})")),
            "op_Explicit" => return Some(format!("public static explicit operator {ret}({params})")),
            "op_Addition" | "op_UnaryPlus" => "+",
            "op_Subtraction" | "op_UnaryNegation" => "-",
            "op_Multiply" => "*",
            "op_Division" => "/",
            "op_Modulus" => "%",
            "op_BitwiseAnd" => "&",
            "op_BitwiseOr" => "|",
            "op_ExclusiveOr" => "^",
            "op_LeftShift" => "<<",
            "op_RightShift" => ">>",
            "op_Equality" => "==",
            "op_Inequality" => "!=",
            "op_LessThan" => "<",
            "op_LessThanOrEqual" => "<=",
            "op_GreaterThan" => ">",
            "op_GreaterThanOrEqual" => ">=",
            "op_LogicalNot" => "!",
            "op_OnesComplement" => "~",
            "op_Increment" => "++",
            "op_Decrement" => "--",
            "op_True" => "true",
            "op_False" => "false",
            _ => return None,
        };
        Some(format!("public static {ret} operator {symbol}({params})"))
    }
    /// 型引数の制約（`where T : class, IFoo, new()`）。`owner` は TypeOrMethodDef の複合添字（型は `行 << 1`、メソッドは
    /// `行 << 1 | 1`）。`skip` は、外側の型から受け継いだ型引数の数（入れ子の型では、自分で足した分だけを書く）。
    pub fn constraints(&self, owner: u32, skip: usize, generics: &Generics) -> String {
        const GENERIC_PARAM_CONSTRAINT: usize = 0x2C;
        const CLASS: u32 = 0x4;
        const STRUCT: u32 = 0x8;
        const NEW: u32 = 0x10;
        let mut clauses = String::new();
        for row in self.md.matching_rows(GENERIC_PARAM, 2, owner).into_iter().skip(skip) {
            let flags = self.md.cell(GENERIC_PARAM, row, 1);
            let mut parts = Vec::new();
            if flags & CLASS != 0 {
                parts.push("class".to_string());
            }
            if flags & STRUCT != 0 {
                parts.push("struct".to_string());
            }
            for constraint in self.md.matching_rows(GENERIC_PARAM_CONSTRAINT, 0, row) {
                let name = self.type_token(sig::type_def_or_ref_token(self.md.cell(GENERIC_PARAM_CONSTRAINT, constraint, 1)), &[], generics);
                // struct の制約は、System.ValueType の制約として記録されている。
                if !(flags & STRUCT != 0 && name == "ValueType") {
                    parts.push(name);
                }
            }
            if flags & NEW != 0 && flags & STRUCT == 0 {
                parts.push("new()".to_string());
            }
            if !parts.is_empty() {
                clauses.push_str(&format!(" where {} : {}", ident(self.md.string(self.md.cell(GENERIC_PARAM, row, 3))), parts.join(", ")));
            }
        }
        clauses
    }

    /// メソッドの修飾子（見え方、static、extern、virtual / override / abstract）。
    pub fn method_modifiers(&self, method_row: u32, kind: TypeKind) -> Vec<String> {
        const STATIC: u32 = 0x10;
        const FINAL: u32 = 0x20;
        const VIRTUAL: u32 = 0x40;
        const NEW_SLOT: u32 = 0x100;
        const ABSTRACT: u32 = 0x400;
        const PINVOKE: u32 = 0x2000;
        let flags = self.md.cell(METHOD_DEF, method_row, 2);
        let mut parts = Vec::new();
        if kind != TypeKind::Interface {
            parts.push(member_visibility(flags).to_string());
        }
        if flags & STATIC != 0 {
            parts.push("static".into());
        }
        if kind != TypeKind::Interface {
            if flags & PINVOKE != 0 || (self.md.cell(METHOD_DEF, method_row, 0) == 0 && flags & ABSTRACT == 0) {
                parts.push("extern".into());
            }
            match (flags & VIRTUAL != 0, flags & ABSTRACT != 0, flags & NEW_SLOT != 0, flags & FINAL != 0) {
                (true, true, _, _) => parts.push("abstract".into()),
                (true, false, true, false) => parts.push("virtual".into()),
                (true, false, false, false) => parts.push("override".into()),
                (true, false, false, true) => parts.push("sealed override".into()),
                // virtual + newslot + final はインターフェイスの実装（C# では印を書かない）。
                _ => {}
            }
        }
        parts
    }

    /// デリゲートの宣言: `public delegate int Handler(string s);`（Invoke のシグネチャ）。
    pub fn delegate_declaration(&self, row: u32) -> String {
        let generics = Generics { types: self.type_generics(row), methods: Vec::new() };
        let raw = self.md.string(self.md.cell(TYPE_DEF, row, 1));
        let mut name = ident(without_arity(raw));
        let own = self.own_type_generics(row);
        if !own.is_empty() {
            name = format!("{name}<{}>", own.join(", "));
        }
        let visibility = type_visibility(self.md.cell(TYPE_DEF, row, 0));
        let invoke = self.md.child_rows(TYPE_DEF, row, 5, METHOD_DEF).find(|&m| self.md.string(self.md.cell(METHOD_DEF, m, 3)) == "Invoke");
        let (ret, parameters) = match invoke.and_then(|m| Some((m, self.method_def_sig(m)?))) {
            Some((invoke, sig)) => (self.ty(&sig.ret, &generics), self.parameters(invoke, &sig).iter().map(|p| self.parameter_text(p, &generics)).collect::<Vec<_>>()),
            None => ("void".to_string(), Vec::new()),
        };
        format!("{visibility} delegate {ret} {name}({});", parameters.join(", "))
    }
}

/// 定数の値（型の印とバイト列）を C# のリテラルにする。
pub fn constant_literal(kind: u8, bytes: &[u8]) -> Option<String> {
    fn take<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
        bytes.get(..N)?.try_into().ok()
    }
    Some(match kind {
        0x02 => (bytes.first()? != &0).to_string(),
        0x03 => char_literal(u16::from_le_bytes(take(bytes)?)),
        0x04 => (take::<1>(bytes)?[0] as i8).to_string(),
        0x05 => take::<1>(bytes)?[0].to_string(),
        0x06 => i16::from_le_bytes(take(bytes)?).to_string(),
        0x07 => u16::from_le_bytes(take(bytes)?).to_string(),
        0x08 => i32::from_le_bytes(take(bytes)?).to_string(),
        0x09 => format!("{}u", u32::from_le_bytes(take(bytes)?)),
        0x0A => format!("{}L", i64::from_le_bytes(take(bytes)?)),
        0x0B => format!("{}UL", u64::from_le_bytes(take(bytes)?)),
        0x0C => float_literal(f32::from_le_bytes(take(bytes)?) as f64, true),
        0x0D => float_literal(f64::from_le_bytes(take(bytes)?), false),
        0x0E => {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
            string_literal(&String::from_utf16_lossy(&units))
        }
        0x12 => "null".into(),
        _ => return None,
    })
}

/// 文字のリテラル（`'a'`、`'\n'`）。
pub fn char_literal(unit: u16) -> String {
    match char::from_u32(unit as u32) {
        Some('\'') => "'\\''".into(),
        Some('\\') => "'\\\\'".into(),
        Some('\n') => "'\\n'".into(),
        Some('\r') => "'\\r'".into(),
        Some('\t') => "'\\t'".into(),
        Some('\0') => "'\\0'".into(),
        // 行の区切りになる文字（U+2028、U+2029）は、C# では文字のリテラルに直接書けない。
        Some(c) if !c.is_control() && c != '\u{2028}' && c != '\u{2029}' => format!("'{c}'"),
        _ => format!("'\\u{unit:04X}'"),
    }
}

/// 浮動小数点数のリテラル。`single` なら `f` を付ける。
pub fn float_literal(value: f64, single: bool) -> String {
    let (type_name, suffix) = if single { ("float", "f") } else { ("double", "") };
    if value.is_nan() {
        return format!("{type_name}.NaN");
    }
    if value.is_infinite() {
        return format!("{type_name}.{}", if value > 0.0 { "PositiveInfinity" } else { "NegativeInfinity" });
    }
    let text = if single { format!("{:?}", value as f32) } else { format!("{value:?}") };
    format!("{text}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_become_csharp_literals() {
        assert_eq!(constant_literal(0x08, &(-5i32).to_le_bytes()).unwrap(), "-5");
        assert_eq!(constant_literal(0x0B, &7u64.to_le_bytes()).unwrap(), "7UL");
        assert_eq!(constant_literal(0x0C, &1.5f32.to_le_bytes()).unwrap(), "1.5f");
        assert_eq!(constant_literal(0x0D, &f64::NAN.to_le_bytes()).unwrap(), "double.NaN");
        assert_eq!(constant_literal(0x0E, &[0x41, 0x00, 0x22, 0x00]).unwrap(), "\"A\\\"\"");
        assert_eq!(constant_literal(0x12, &[0, 0, 0, 0]).unwrap(), "null");
    }

    #[test]
    fn chars_are_escaped() {
        assert_eq!(char_literal(b'a' as u16), "'a'");
        assert_eq!(char_literal(b'\'' as u16), "'\\''");
        assert_eq!(char_literal(1), "'\\u0001'");
    }
}
