//! 対象の値を読んで、表示する文字列と型の名前にする。
//!
//! 表記は netcoredbg に合わせる（拡張がそのまま解釈している。docs/plan.html「表記の互換」）:
//! 文字列は `"..."`、オブジェクトは `{型名}`、配列は `{int[3]}`、参照が空なら `null`、文字は `97 'a'`。
//! 対象のコードは実行しない。プロパティや `ToString()` の評価は段階 4（関数評価）で足す。

use crate::com::{ComPtr, Interface, HRESULT};
use crate::cordebug::*;
use crate::debugger::metadata::{Metadata, ParameterKind};
use std::ffi::c_void;
use std::ptr::null_mut;

/// 対象のモジュールからメタデータを引く。セッションが持っているモジュールの一覧が実装する。
pub trait Modules {
    fn metadata(&self, module: &ComPtr<ICorDebugModule>) -> Option<&Metadata>;
}

// CorElementType（corhdr.h）。cordebug.idl には定義が無い。
const ELEMENT_TYPE_BOOLEAN: u32 = 0x02;
const ELEMENT_TYPE_CHAR: u32 = 0x03;
const ELEMENT_TYPE_I1: u32 = 0x04;
const ELEMENT_TYPE_U1: u32 = 0x05;
const ELEMENT_TYPE_I2: u32 = 0x06;
const ELEMENT_TYPE_U2: u32 = 0x07;
const ELEMENT_TYPE_I4: u32 = 0x08;
const ELEMENT_TYPE_U4: u32 = 0x09;
const ELEMENT_TYPE_I8: u32 = 0x0A;
const ELEMENT_TYPE_U8: u32 = 0x0B;
const ELEMENT_TYPE_R4: u32 = 0x0C;
const ELEMENT_TYPE_R8: u32 = 0x0D;
const ELEMENT_TYPE_STRING: u32 = 0x0E;
const ELEMENT_TYPE_PTR: u32 = 0x0F;
const ELEMENT_TYPE_VOID: u32 = 0x01;
const ELEMENT_TYPE_BYREF: u32 = 0x10;
const ELEMENT_TYPE_VALUETYPE: u32 = 0x11;
const ELEMENT_TYPE_CLASS: u32 = 0x12;
const ELEMENT_TYPE_ARRAY: u32 = 0x14;
const ELEMENT_TYPE_I: u32 = 0x18;
const ELEMENT_TYPE_U: u32 = 0x19;
const ELEMENT_TYPE_SZARRAY: u32 = 0x1D;

/// 読めなかった値の表示（netcoredbg と同じ）。
const UNREADABLE: &str = "<error>";
/// 型の入れ子や参照をたどる深さの上限。壊れたデータで止まらなくなるのを防ぐ。
const MAX_DEPTH: u32 = 16;

/// 出力引数でインターフェイスを受け取る呼び出しを包む。失敗と null は `None`。
unsafe fn get<I: Interface>(call: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Option<ComPtr<I>> {
    let mut raw = null_mut();
    if !call(&mut raw).is_ok() {
        return None;
    }
    ComPtr::from_raw(raw)
}

enum Resolved {
    Null,
    Value(ComPtr<ICorDebugValue>),
    Unreadable,
}

/// ポインター（`int*`）の値なら、その番地と、ポインターの型。ポインターも ICorDebug では参照の値だが、指す先を
/// 中身として見せはしない（`resolve` はたどらない）。
pub unsafe fn pointer_parts(value: &ComPtr<ICorDebugValue>) -> Option<(u64, ComPtr<ICorDebugType>)> {
    let ty = exact_type(value)?;
    let mut element = 0;
    ty.GetType(&mut element);
    if element != ELEMENT_TYPE_PTR {
        return None;
    }
    let reference = value.cast::<ICorDebugReferenceValue>().ok()?;
    let mut address = 0u64;
    reference.GetValue(&mut address).ok().ok()?;
    Some((address, ty))
}

/// ポインターの指す先の値（対象のメモリの上の値）。null のポインターや、ポインターでない値は `None`。
pub unsafe fn pointee(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
    let (address, _) = pointer_parts(value)?;
    if address == 0 {
        return None;
    }
    let reference = value.cast::<ICorDebugReferenceValue>().ok()?;
    get(|out| reference.Dereference(out))
}

/// 参照とボックスをたどって、中身の値にする。ポインターは、たどらずにそのまま返す。
unsafe fn resolve(value: &ComPtr<ICorDebugValue>) -> Resolved {
    let mut current = value.clone();
    for _ in 0..MAX_DEPTH {
        if pointer_parts(&current).is_some() {
            return Resolved::Value(current);
        }
        if let Ok(reference) = current.cast::<ICorDebugReferenceValue>() {
            let mut is_null = 0;
            if !reference.IsNull(&mut is_null).is_ok() {
                return Resolved::Unreadable;
            }
            if is_null != 0 {
                return Resolved::Null;
            }
            match get(|out| reference.Dereference(out)) {
                Some(target) => current = target,
                None => return Resolved::Unreadable,
            }
        } else if let Ok(boxed) = current.cast::<ICorDebugBoxValue>() {
            match get::<ICorDebugObjectValue>(|out| boxed.GetObject(out)).and_then(|object| object.cast().ok()) {
                Some(inner) => current = inner,
                None => return Resolved::Unreadable,
            }
        } else {
            return Resolved::Value(current);
        }
    }
    Resolved::Unreadable
}

fn keyword(element: u32) -> Option<&'static str> {
    let ty = crate::debugger::builtin::by_element(u8::try_from(element).ok()?)?;
    // nint / nuint は、netcoredbg と同じく、型の名前で見せる。
    Some(if matches!(ty.keyword, "nint" | "nuint") { ty.full_name } else { ty.keyword })
}

/// メタデータの名前を C# の書き方に直す。
fn csharp_alias(name: String) -> String {
    if name == "System.Void" {
        return "void".into();
    }
    // ボックス化された値は、プリミティブ型でもクラスとして見える。nint / nuint は、型の名前のまま見せる。
    if let Some(ty) = crate::debugger::builtin::by_full_name(&name).filter(|ty| !matches!(ty.keyword, "nint" | "nuint")) {
        return ty.keyword.into();
    }
    match name.strip_prefix("System.Nullable<").and_then(|rest| rest.strip_suffix('>')) {
        Some(inner) => format!("{inner}?"),
        None => name,
    }
}

/// 型の名前。型引数は実際の型で書く（`System.Collections.Generic.List<string>`）。
pub unsafe fn type_name(ty: &ComPtr<ICorDebugType>, modules: &dyn Modules) -> String {
    type_name_within(ty, modules, 0)
}

unsafe fn type_name_within(ty: &ComPtr<ICorDebugType>, modules: &dyn Modules, depth: u32) -> String {
    let mut element = 0;
    ty.GetType(&mut element);
    if let Some(keyword) = keyword(element) {
        return keyword.into();
    }
    if depth >= MAX_DEPTH {
        return "?".into();
    }
    let first_argument = || get::<ICorDebugType>(|out| ty.GetFirstTypeParameter(out)).map_or("?".to_string(), |inner| type_name_within(&inner, modules, depth + 1));
    match element {
        ELEMENT_TYPE_SZARRAY | ELEMENT_TYPE_ARRAY => {
            let mut rank = 1;
            ty.GetRank(&mut rank);
            array_type_name(&first_argument(), &",".repeat(rank.saturating_sub(1) as usize))
        }
        ELEMENT_TYPE_PTR => first_argument() + "*",
        ELEMENT_TYPE_VOID => "void".into(),
        ELEMENT_TYPE_BYREF => first_argument(),
        ELEMENT_TYPE_CLASS | ELEMENT_TYPE_VALUETYPE => class_name(ty, modules, depth).map_or("?".into(), csharp_alias),
        _ => "?".into(),
    }
}

/// 配列の型の名前。要素も配列なら、C# の書き方どおり外側の次元を先に書く（`int[2][]`）。
fn array_type_name(element: &str, dimensions: &str) -> String {
    match element.find('[') {
        Some(at) => format!("{}[{dimensions}]{}", &element[..at], &element[at..]),
        None => format!("{element}[{dimensions}]"),
    }
}

unsafe fn class_name(ty: &ComPtr<ICorDebugType>, modules: &dyn Modules, depth: u32) -> Option<String> {
    let class = get::<ICorDebugClass>(|out| ty.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let module = get::<ICorDebugModule>(|out| class.GetModule(out))?;
    let Some(metadata) = modules.metadata(&module) else { return Some(format!("[0x{token:08X}]")) };

    let mut arguments = Vec::new();
    if let Some(parameters) = get::<ICorDebugTypeEnum>(|out| ty.EnumerateTypeParameters(out)) {
        loop {
            let (mut raw, mut fetched) = (null_mut(), 0u32);
            if !parameters.Next(1, &mut raw, &mut fetched).is_ok() || fetched == 0 {
                break;
            }
            match ComPtr::<ICorDebugType>::from_raw(raw) {
                Some(argument) => arguments.push(type_name_within(&argument, modules, depth + 1)),
                None => break,
            }
        }
    }
    Some(metadata.type_name_with(token & 0x00FF_FFFF, &arguments))
}

unsafe fn exact_type(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugType>> {
    let value2 = value.cast::<ICorDebugValue2>().ok()?;
    get(|out| value2.GetExactType(out))
}

unsafe fn type_name_of(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> String {
    match exact_type(value) {
        Some(ty) => type_name(&ty, modules),
        None => {
            let mut element = 0;
            value.GetType(&mut element);
            keyword(element).unwrap_or("?").into()
        }
    }
}

/// 文字列を C# のリテラルの形にする。
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\0' => quoted.push_str("\\0"),
            '\x07' => quoted.push_str("\\a"),
            '\x08' => quoted.push_str("\\b"),
            '\x0C' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\x0B' => quoted.push_str("\\v"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// プリミティブ型の値のバイト列（リトルエンディアン）を文字列にする。
fn format_primitive(element: u32, bytes: &[u8]) -> Option<String> {
    fn array<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
        bytes.get(..N)?.try_into().ok()
    }
    Some(match element {
        ELEMENT_TYPE_BOOLEAN => if *bytes.first()? != 0 { "true" } else { "false" }.into(),
        ELEMENT_TYPE_CHAR => {
            let code = u16::from_le_bytes(array(bytes)?);
            // サロゲートの片割れは文字にできないので、番号だけを見せる。
            match char::from_u32(code as u32) {
                Some(c) => format!("{code} '{c}'"),
                None => code.to_string(),
            }
        }
        ELEMENT_TYPE_I1 => (*bytes.first()? as i8).to_string(),
        ELEMENT_TYPE_U1 => bytes.first()?.to_string(),
        ELEMENT_TYPE_I2 => i16::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_U2 => u16::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_I4 => i32::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_U4 => u32::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_I8 => i64::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_U8 => u64::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_R4 => f32::from_le_bytes(array(bytes)?).to_string(),
        ELEMENT_TYPE_R8 => f64::from_le_bytes(array(bytes)?).to_string(),
        // ポインターの幅は対象（＝エンジン）のビット数で変わるので、バイト列の長さで決める。
        ELEMENT_TYPE_I | ELEMENT_TYPE_U | ELEMENT_TYPE_PTR => match bytes.len() {
            4 => format!("0x{:08x}", u32::from_le_bytes(array(bytes)?)),
            8 => format!("0x{:016x}", u64::from_le_bytes(array(bytes)?)),
            _ => return None,
        },
        _ => return None,
    })
}

unsafe fn read_primitive(value: &ComPtr<ICorDebugValue>, element: u32) -> Option<String> {
    let generic = value.cast::<ICorDebugGenericValue>().ok()?;
    let mut size = 0u32;
    value.GetSize(&mut size);
    let mut buffer = [0u8; 16];
    if size as usize > buffer.len() || !generic.GetValue(buffer.as_mut_ptr() as *mut c_void).is_ok() {
        return None;
    }
    format_primitive(element, &buffer[..size as usize])
}

/// ボックスから取り出したプリミティブ型の値（`object o = 5` の中身）。ICorDebug はこれを `System.Int32` のオブジェクトとして
/// 見せるので、ただ一つのフィールド（`m_value`）を読む。プリミティブ型でなければ `None`。
unsafe fn read_boxed_primitive(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let object = value.cast::<ICorDebugObjectValue>().ok()?;
    let class = get::<ICorDebugClass>(|out| object.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let metadata = modules.metadata(&get::<ICorDebugModule>(|out| class.GetModule(out))?)?;
    let row = token & 0x00FF_FFFF;
    // nint / nuint も含む。ボックスの中でも、じかに持っているときと同じ表示にする。
    if !crate::debugger::builtin::by_full_name(&metadata.type_name(row)).is_some_and(|ty| ty.is_primitive()) {
        return None;
    }
    let field = metadata.fields(row).into_iter().find(|f| !f.is_static)?;
    let inner = get::<ICorDebugValue>(|out| object.GetFieldValue(class.as_raw(), field.token, out))?;
    let mut element = 0;
    inner.GetType(&mut element);
    read_primitive(&inner, element)
}

/// フィールドの値を、符号なしの整数として読む（プリミティブ型の整数のフィールドだけ）。
unsafe fn field_bits(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, name: &str) -> Option<u64> {
    let (_, bytes) = primitive_bytes(&field(value, modules, name)?)?;
    let mut wide = [0u8; 8];
    wide[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
    Some(u64::from_le_bytes(wide))
}

/// `decimal` の値。96 ビットの整数と、小数点の位置、符号でできている。フィールドの名前はランタイムで違う
/// （.NET は `_flags` / `_hi32` / `_lo64`、.NET Framework は `flags` / `hi` / `lo` / `mid`）。
unsafe fn read_decimal(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let (flags, mantissa) = match field_bits(value, modules, "_flags") {
        Some(flags) => (flags, (field_bits(value, modules, "_hi32")? as u128) << 64 | field_bits(value, modules, "_lo64")? as u128),
        None => (
            field_bits(value, modules, "flags")?,
            (field_bits(value, modules, "hi")? as u128) << 64 | (field_bits(value, modules, "mid")? as u128) << 32 | field_bits(value, modules, "lo")? as u128,
        ),
    };
    Some(format_decimal(mantissa, (flags >> 16 & 0xFF) as usize, flags & 0x8000_0000 != 0))
}

/// 整数 `mantissa` を、後ろから `scale` 桁目に小数点を置いて書く。末尾の 0 は落とさない（`12.50` は `12.50`）。
fn format_decimal(mantissa: u128, scale: usize, negative: bool) -> String {
    let mut digits = mantissa.to_string();
    if scale > 0 {
        // 整数部が 1 桁は残るように、足りなければ 0 を補う。
        if digits.len() <= scale {
            digits = format!("{}{digits}", "0".repeat(scale + 1 - digits.len()));
        }
        digits.insert(digits.len() - scale, '.');
    }
    if negative { format!("-{digits}") } else { digits }
}

/// 列挙型の値を、名前で書く。ちょうどの名前が無ければ、ビットの組み合わせを `A | B` と書く。それもできなければ数。
/// 列挙型でなければ `None`。
unsafe fn read_enum(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let ty = exact_type(value)?;
    let base = get::<ICorDebugType>(|out| ty.GetBase(out))?;
    if class_name(&base, modules, 0).as_deref() != Some("System.Enum") {
        return None;
    }
    let (element, bytes) = primitive_bytes(&field(value, modules, "value__")?)?;
    let mut wide = [0u8; 8];
    wide[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
    let bits = u64::from_le_bytes(wide);
    // 名前に当たらないときの数は、もとの整数の型の値として書く（`(Tone)(-6)` は -6。C# の ToString() と同じ）。
    // netcoredbg は 64 ビットの符号なしに広げて 18446744073709551610 と書くが、合わせない（D-032）。
    let class = get::<ICorDebugClass>(|out| ty.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let metadata = modules.metadata(&get::<ICorDebugModule>(|out| class.GetModule(out))?)?;
    // 列挙型の名前は、定数のフィールドとして並んでいる。
    let names: Vec<(&str, u64)> = metadata
        .fields(token & 0x00FF_FFFF)
        .into_iter()
        .filter(|f| f.is_literal)
        .filter_map(|f| {
            let (_, bytes) = metadata.constant(f.token)?;
            let mut wide = [0u8; 8];
            wide[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
            Some((f.name, u64::from_le_bytes(wide)))
        })
        .collect();
    Some(enum_text(bits, &names).unwrap_or_else(|| enum_number(element, bits)))
}

/// 列挙型の値の数（`bits` は、もとの整数の型の幅で 0 を補って広げたもの）を、符号つきの型なら符号つきで書く。
fn enum_number(element: u32, bits: u64) -> String {
    match element {
        0x04 => (bits as u8 as i8).to_string(),
        0x06 => (bits as u16 as i16).to_string(),
        0x08 => (bits as u32 as i32).to_string(),
        0x0A => (bits as i64).to_string(),
        _ => bits.to_string(),
    }
}

/// 名前（か、名前のビットの組み合わせ）。どちらでも書けなければ `None`（数で書く）。
fn enum_text(bits: u64, names: &[(&str, u64)]) -> Option<String> {
    if let Some((name, _)) = names.iter().find(|(_, value)| *value == bits) {
        return Some(name.to_string());
    }
    let (mut left, mut parts) = (bits, Vec::new());
    for &(name, value) in names {
        if value != 0 && left & value == value {
            parts.push(name);
            left &= !value;
        }
    }
    (left == 0 && !parts.is_empty()).then(|| parts.join(" | "))
}

unsafe fn read_string(value: &ComPtr<ICorDebugValue>) -> Option<String> {
    let string = value.cast::<ICorDebugStringValue>().ok()?;
    let mut length = 0u32;
    string.GetLength(&mut length).ok().ok()?;
    let mut buffer = vec![0u16; length as usize + 1];
    let mut fetched = 0u32;
    string.GetString(buffer.len() as u32, &mut fetched, buffer.as_mut_ptr()).ok().ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

unsafe fn array_dimensions(array: &ComPtr<ICorDebugArrayValue>) -> Vec<u32> {
    let mut rank = 0u32;
    array.GetRank(&mut rank);
    let mut dimensions = vec![0u32; rank as usize];
    if rank > 0 {
        array.GetDimensions(rank, dimensions.as_mut_ptr());
    }
    dimensions
}

pub struct Described {
    pub value: String,
    pub type_name: String,
    /// 中身（フィールド、要素）を持ち、`variables` で開ける。
    pub expandable: bool,
}

/// 値を表示用にする。
pub unsafe fn describe(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Described {
    let resolved = match resolve(value) {
        Resolved::Null => return Described { value: "null".into(), type_name: type_name_of(value, modules), expandable: false },
        Resolved::Unreadable => return Described { value: UNREADABLE.into(), type_name: type_name_of(value, modules), expandable: false },
        Resolved::Value(resolved) => resolved,
    };
    // 型は、変数の宣言の型ではなく、入っている実体の型を見せる。
    let type_name = type_name_of(&resolved, modules);
    let mut element = 0;
    resolved.GetType(&mut element);

    // ポインター: 値の欄には指す先の値を、型の欄にはポインターの型（`int*`）を書き、開くと指す先が 1 つ見える（netcoredbg と同じ）。
    if let Some((address, _)) = pointer_parts(&resolved) {
        let Some(target) = pointee(&resolved).filter(|_| address != 0) else { return Described { value: "null".into(), type_name, expandable: false } };
        return Described { value: describe(&target, modules).value, type_name, expandable: true };
    }

    if element == ELEMENT_TYPE_STRING {
        let value = read_string(&resolved).map_or(UNREADABLE.into(), |text| quote(&text));
        return Described { value, type_name, expandable: false };
    }
    if let Ok(array) = resolved.cast::<ICorDebugArrayValue>() {
        let dimensions = array_dimensions(&array);
        let sizes = dimensions.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        // 型名の最初の `[]`（いちばん外側の次元）に、要素数を書き入れる。
        let value = match type_name.find('[').zip(type_name.find(']')) {
            Some((open, close)) if open < close => format!("{{{}[{sizes}]{}}}", &type_name[..open], &type_name[close + 1..]),
            _ => format!("{{{type_name}}}"),
        };
        return Described { value, type_name, expandable: dimensions.iter().all(|&d| d > 0) && !dimensions.is_empty() };
    }
    if let Some(value) = read_primitive(&resolved, element) {
        return Described { value, type_name, expandable: false };
    }
    if let Some(value) = read_boxed_primitive(&resolved, modules) {
        return Described { value, type_name, expandable: false };
    }
    if element == ELEMENT_TYPE_VALUETYPE {
        // 中身をそのまま値として見せる型。フィールドを並べても、利用者の知りたいことは分からない。
        if type_name == "decimal" {
            if let Some(value) = read_decimal(&resolved, modules) {
                return Described { value, type_name, expandable: false };
            }
        }
        if type_name.ends_with('?') {
            match field(&resolved, modules, "hasValue").map(|flag| describe(&flag, modules).value).as_deref() {
                Some("false") => return Described { value: "null".into(), type_name, expandable: false },
                Some("true") => {
                    if let Some(inner) = field(&resolved, modules, "value") {
                        let inner = describe(&inner, modules);
                        return Described { value: inner.value, type_name, expandable: inner.expandable };
                    }
                }
                _ => {}
            }
        }
        if let Some(value) = read_enum(&resolved, modules) {
            return Described { value, type_name, expandable: true };
        }
    }
    let expandable = resolved.cast::<ICorDebugObjectValue>().is_ok();
    Described { value: format!("{{{type_name}}}"), type_name, expandable }
}

pub enum Member {
    Value { name: String, value: ComPtr<ICorDebugValue> },
    /// 静的メンバーをまとめた節。開くと `static_members` の中身になる。
    Statics(ComPtr<ICorDebugType>),
    /// 値の代わりに、読めなかった理由を見せるメンバー（時間切れになったプロパティなど）。
    Note { name: String, text: String },
    /// まだ getter を実行していないプロパティ（暗黙の評価をしない設定のとき）。`this` は、静的プロパティなら `None`。
    Lazy { name: String, getter: Callable, this: Option<ComPtr<ICorDebugValue>> },
}

impl Member {
    /// 変数の欄の名前（静的メンバーの節は `None`）。
    pub fn name(&self) -> Option<&str> {
        match self {
            Member::Value { name, .. } | Member::Note { name, .. } | Member::Lazy { name, .. } => Some(name),
            Member::Statics(_) => None,
        }
    }
}

/// 自動実装プロパティの裏のフィールド（`<Name>k__BackingField`）は、プロパティの名前で見せる。
pub fn member_name(field: &str) -> &str {
    field.strip_prefix('<').and_then(|rest| rest.strip_suffix(">k__BackingField")).unwrap_or(field)
}

/// 型と、その基底の型を順にたどり、それぞれのクラスとメタデータを `visit` に渡す。
unsafe fn for_each_class(ty: ComPtr<ICorDebugType>, modules: &dyn Modules, mut visit: impl FnMut(&ComPtr<ICorDebugType>, &ComPtr<ICorDebugClass>, &Metadata, u32)) {
    let mut current = Some(ty);
    for _ in 0..64 {
        let Some(ty) = current else { return };
        if let Some(class) = get::<ICorDebugClass>(|out| ty.GetClass(out)) {
            let mut token = 0;
            class.GetToken(&mut token);
            if let Some(metadata) = get::<ICorDebugModule>(|out| class.GetModule(out)).and_then(|module| modules.metadata(&module)) {
                visit(&ty, &class, metadata, token & 0x00FF_FFFF);
            }
        }
        current = get(|out| ty.GetBase(out));
    }
}

/// 値の中身。配列なら要素（`start` から `count` 個）、オブジェクトならフィールド（派生した型の分が先）と、静的メンバーの節。
pub unsafe fn children(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, start: usize, count: usize) -> Vec<Member> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    let mut members = Vec::new();

    // ポインターの中身は、指す先の値 1 つ。名前は空（netcoredbg と同じ）。
    if pointer_parts(&resolved).is_some() {
        members.extend(pointee(&resolved).map(|value| Member::Value { name: String::new(), value }));
        return members;
    }
    if let Ok(array) = resolved.cast::<ICorDebugArrayValue>() {
        let dimensions = array_dimensions(&array);
        let mut total = 0u32;
        array.GetCount(&mut total);
        for position in (start..total as usize).take(count) {
            // 多次元の配列は、後ろの次元から順に進む並び（行優先）で入っている。
            let mut indices = vec![0usize; dimensions.len()];
            let mut rest = position;
            for (index, &size) in indices.iter_mut().zip(&dimensions).rev() {
                *index = rest % (size.max(1) as usize);
                rest /= size.max(1) as usize;
            }
            let name = format!("[{}]", indices.iter().map(usize::to_string).collect::<Vec<_>>().join(", "));
            if let Some(value) = get(|out| array.GetElementAtPosition(position as u32, out)) {
                members.push(Member::Value { name, value });
            }
        }
        return members;
    }

    let (Ok(object), Some(ty)) = (resolved.cast::<ICorDebugObjectValue>(), exact_type(&resolved)) else { return members };
    // 静的メンバーの節は、型か基底の型のどれかが静的メンバー（定数と静的プロパティも含む）を持てば出す（netcoredbg と同じ）。
    // System.Object / ValueType / Enum の分は数えない（静的メンバーの節にも出さない）。
    let mut has_statics = false;
    let mut own_type = true;
    for_each_class(ty.clone(), modules, |_, class, metadata, type_row| {
        let fields = metadata.fields(type_row);
        let system_base = !own_type && matches!(metadata.type_name(type_row).as_str(), "System.Object" | "System.ValueType" | "System.Enum");
        own_type = false;
        has_statics |= !system_base && (fields.iter().any(|f| f.is_static) || !metadata.static_properties(type_row).is_empty());
        for field in fields {
            if field.is_static {
            } else if let Some(value) = get(|out| object.GetFieldValue(class.as_raw(), field.token, out)) {
                members.push(Member::Value { name: member_name(field.name).to_string(), value });
            }
        }
    });
    if has_statics {
        members.push(Member::Statics(ty));
    }
    members
}

/// 値の型と基底の型に付いた `[DebuggerBrowsable]` の、隠すメンバーと、中身をその場に並べるメンバーの名前。
pub unsafe fn browsable_rules(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> (Vec<String>, Vec<String>) {
    let (mut never, mut root_hidden) = (Vec::new(), Vec::new());
    let Resolved::Value(resolved) = resolve(value) else { return (never, root_hidden) };
    if resolved.cast::<ICorDebugArrayValue>().is_ok() {
        return (never, root_hidden);
    }
    if let Some(ty) = exact_type(&resolved) {
        for_each_class(ty, modules, |_, _, metadata, type_row| {
            let (hidden, flattened) = metadata.browsable_rules(type_row);
            never.extend(hidden);
            root_hidden.extend(flattened);
        });
    }
    (never, root_hidden)
}

/// 型のクラスと、そのクラスがあるモジュール、TypeDef の行番号。
pub unsafe fn class_of(ty: &ComPtr<ICorDebugType>) -> Option<(ComPtr<ICorDebugClass>, ComPtr<ICorDebugModule>, u32)> {
    let class = get::<ICorDebugClass>(|out| ty.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let module = get::<ICorDebugModule>(|out| class.GetModule(out))?;
    Some((class, module, token & 0x00FF_FFFF))
}

/// 型の静的フィールドの値。まだ初期化されていない型のフィールドは読めない（`None`）。
/// スレッドごとの静的フィールド（`[ThreadStatic]`）は、そのスレッドのフレーム（`frame`）が無いと読めない。
pub unsafe fn static_field(ty: &ComPtr<ICorDebugType>, field: u32, frame: Option<&ComPtr<ICorDebugFrame>>) -> Option<ComPtr<ICorDebugValue>> {
    get(|out| ty.GetStaticFieldValue(field, frame.map_or(null_mut(), |f| f.as_raw()), out))
}

/// オブジェクトの文字列型のフィールドの中身。`names` のどれかの名前のフィールドを、基底の型も含めて探す
/// （ランタイムの版でフィールド名が違うものがあるので、候補を並べて渡す）。無い、または null なら `None`。
pub unsafe fn string_field(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, names: &[&str]) -> Option<String> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let object = resolved.cast::<ICorDebugObjectValue>().ok()?;
    let mut found = None;
    for_each_class(exact_type(&resolved)?, modules, |_, class, metadata, type_row| {
        for field in metadata.fields(type_row).into_iter().filter(|f| !f.is_static && names.contains(&f.name)) {
            if found.is_none() {
                found = get::<ICorDebugValue>(|out| object.GetFieldValue(class.as_raw(), field.token, out));
            }
        }
    });
    match resolve(&found?) {
        Resolved::Value(text) => read_string(&text),
        _ => None,
    }
}

/// 呼び出せるメソッド。ジェネリック型のメソッドは、その型の型引数を一緒に渡さないと呼べない。
#[derive(Clone)]
pub struct Callable {
    pub function: ComPtr<ICorDebugFunction>,
    pub type_arguments: Vec<ComPtr<ICorDebugType>>,
}

/// 値の型（と基底の型）から、引数の無いインスタンスメソッドを探す。派生した型の定義が先に見つかる。
pub unsafe fn find_parameterless_method(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, name: &str) -> Option<Callable> {
    find_instance_method(value, modules, name, 0)
}

/// 値の型（と基底の型）から、名前と引数の数が合うインスタンスメソッドを探す。派生した型の定義が先に見つかる。
/// 引数の型は見ていない（同じ数の引数を取るオーバーロードがあると、宣言の早い方になる）。
pub unsafe fn find_instance_method(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, name: &str, parameters: u32) -> Option<Callable> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let mut found = None;
    for_each_class(exact_type(&resolved)?, modules, |ty, class, metadata, type_row| {
        if found.is_some() {
            return;
        }
        let Some(token) = metadata.find_method(type_row, name, false, parameters) else { return };
        let Some(function) = get::<ICorDebugModule>(|out| class.GetModule(out)).and_then(|module| get::<ICorDebugFunction>(|out| module.GetFunctionFromToken(token, out))) else { return };
        let mut type_arguments = Vec::new();
        if let Some(parameters) = get::<ICorDebugTypeEnum>(|out| ty.EnumerateTypeParameters(out)) {
            loop {
                let (mut raw, mut fetched) = (null_mut(), 0u32);
                if !parameters.Next(1, &mut raw, &mut fetched).is_ok() || fetched == 0 {
                    break;
                }
                match ComPtr::<ICorDebugType>::from_raw(raw) {
                    Some(argument) => type_arguments.push(argument),
                    None => break,
                }
            }
        }
        found = Some(Callable { function, type_arguments });
    });
    found
}

/// プリミティブ型の値の中身（CorElementType と、リトルエンディアンのバイト列）。プリミティブ型でなければ `None`。
pub unsafe fn primitive_bytes(value: &ComPtr<ICorDebugValue>) -> Option<(u32, Vec<u8>)> {
    let generic = value.cast::<ICorDebugGenericValue>().ok()?;
    let (mut element, mut size) = (0, 0u32);
    value.GetType(&mut element);
    value.GetSize(&mut size);
    keyword(element)?;
    let mut buffer = [0u8; 16];
    if size as usize > buffer.len() || !generic.GetValue(buffer.as_mut_ptr() as *mut c_void).is_ok() {
        return None;
    }
    Some((element, buffer[..size as usize].to_vec()))
}

/// ボックス（`object` に入った値型）の中の、ただ一つのフィールドにバイト列を書き込む。
/// 作ったばかりの空のボックスに、プリミティブ型の値を入れるのに使う。
pub unsafe fn fill_box(boxed: &ComPtr<ICorDebugValue>, class: &ComPtr<ICorDebugClass>, field: u32, bytes: &[u8]) -> Result<(), String> {
    let reference = boxed.cast::<ICorDebugReferenceValue>().map_err(|hr| tr!("Cannot get the reference to the box ({code})", code = hr))?;
    let target = get::<ICorDebugValue>(|out| reference.Dereference(out)).ok_or_else(|| tr!("Cannot follow the box's contents"))?;
    let object = match target.cast::<ICorDebugBoxValue>() {
        Ok(boxed) => get::<ICorDebugObjectValue>(|out| boxed.GetObject(out)).ok_or_else(|| tr!("Cannot get the value in the box"))?,
        Err(_) => target.cast::<ICorDebugObjectValue>().map_err(|hr| tr!("Cannot get the value in the box ({code})", code = hr))?,
    };
    let slot = get::<ICorDebugValue>(|out| object.GetFieldValue(class.as_raw(), field, out)).ok_or_else(|| tr!("Cannot get the field in the box"))?;
    let generic = slot.cast::<ICorDebugGenericValue>().map_err(|hr| tr!("Cannot write the field in the box ({code})", code = hr))?;
    let mut buffer = [0u8; 16];
    buffer[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
    generic.SetValue(buffer.as_mut_ptr() as *mut c_void).ok().map_err(|hr| tr!("Cannot write the field in the box ({code})", code = hr))
}

/// 参照が指している場所（対象の中のアドレス）。同じオブジェクトかどうかを比べるのに使う。参照でなければ `None`。
pub unsafe fn reference_address(value: &ComPtr<ICorDebugValue>) -> Option<u64> {
    let reference = value.cast::<ICorDebugReferenceValue>().ok()?;
    let mut address = 0u64;
    reference.GetValue(&mut address).ok().ok()?;
    Some(address)
}

/// `Nullable<T>` の値なら、その中身（値が無ければ `Some(None)`）。`Nullable<T>` でなければ `None`。
pub unsafe fn nullable(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<Option<ComPtr<ICorDebugValue>>> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let mut element = 0;
    resolved.GetType(&mut element);
    if element != ELEMENT_TYPE_VALUETYPE || !type_name_of(&resolved, modules).ends_with('?') {
        return None;
    }
    match describe(&field(&resolved, modules, "hasValue")?, modules).value.as_str() {
        "true" => Some(Some(field(&resolved, modules, "value")?)),
        _ => Some(None),
    }
}

/// 参照が指しているのがボックス化されたプリミティブ型の値なら、その中身（CorElementType とバイト列）。
pub unsafe fn unboxed_primitive(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<(u32, Vec<u8>)> {
    value.cast::<ICorDebugReferenceValue>().ok()?;
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let object = resolved.cast::<ICorDebugObjectValue>().ok()?;
    let class = get::<ICorDebugClass>(|out| object.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let metadata = modules.metadata(&get::<ICorDebugModule>(|out| class.GetModule(out))?)?;
    let row = token & 0x00FF_FFFF;
    // System.Int32 などのプリミティブ型は、ただ一つのフィールド（m_value）に自分と同じ型の値を持つ。
    // decimal も別名は小文字だが、中身は複数のフィールドで、プリミティブ型ではない。
    let alias = csharp_alias(metadata.type_name(row));
    let pointer_sized = match alias.as_str() {
        "System.IntPtr" => Some(ELEMENT_TYPE_I),
        "System.UIntPtr" => Some(ELEMENT_TYPE_U),
        _ => None,
    };
    if pointer_sized.is_none() && (alias == "decimal" || !alias.chars().all(|c| c.is_ascii_lowercase())) {
        return None;
    }
    let field = metadata.fields(row).into_iter().find(|f| !f.is_static)?;
    let inner = get::<ICorDebugValue>(|out| object.GetFieldValue(class.as_raw(), field.token, out))?;
    // IntPtr の中身のフィールドは、ランタイムの版によって nint だったりポインターだったりする。どちらも、幅の分のバイト列として読む。
    if let Some(element) = pointer_sized {
        let generic = inner.cast::<ICorDebugGenericValue>().ok()?;
        let mut buffer = [0u8; 8];
        let mut size = 0u32;
        inner.GetSize(&mut size);
        if size as usize > buffer.len() || !generic.GetValue(buffer.as_mut_ptr() as *mut c_void).is_ok() {
            return None;
        }
        return Some((element, buffer[..size as usize].to_vec()));
    }
    primitive_bytes(&inner)
}

/// 値の実体の型。null は `None`。
pub unsafe fn exact_type_of(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugType>> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    exact_type(&resolved)
}

/// 値の実体の型の型引数（配列は、要素の型 1 つ）。
pub unsafe fn element_or_argument_types(value: &ComPtr<ICorDebugValue>) -> Vec<ComPtr<ICorDebugType>> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    let Some(ty) = exact_type(&resolved) else { return Vec::new() };
    if resolved.cast::<ICorDebugArrayValue>().is_ok() {
        return get::<ICorDebugType>(|out| ty.GetFirstTypeParameter(out)).into_iter().collect();
    }
    type_arguments(&ty)
}

/// 型のインスタンス化に使われている型引数。
pub unsafe fn type_arguments_of(ty: &ComPtr<ICorDebugType>) -> Vec<ComPtr<ICorDebugType>> {
    type_arguments(ty)
}

/// ボックス（`object` に入った値型）を指している参照なら、その中の値。そうでなければ `None`。
/// 値型の引数には、ボックスではなく中の値を渡す（x64 では、ボックスのまま渡すと別の値として読まれる）。
pub unsafe fn inside_box(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
    let reference = value.cast::<ICorDebugReferenceValue>().ok()?;
    let target = get::<ICorDebugValue>(|out| reference.Dereference(out))?;
    let boxed = target.cast::<ICorDebugBoxValue>().ok()?;
    get::<ICorDebugObjectValue>(|out| boxed.GetObject(out))?.cast::<ICorDebugValue>().ok()
}

/// 値の実体の型（クラスか構造体）の、モジュールと TypeDef のトークン。配列、プリミティブ型、null は `None`。
pub unsafe fn class_of_value(value: &ComPtr<ICorDebugValue>) -> Option<(ComPtr<ICorDebugModule>, u32)> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    if resolved.cast::<ICorDebugArrayValue>().is_ok() {
        return None;
    }
    let ty = exact_type(&resolved)?;
    let class = get::<ICorDebugClass>(|out| ty.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    Some((get::<ICorDebugModule>(|out| class.GetModule(out))?, token))
}

/// 列挙型の値なら、その型（モジュールと TypeDef のトークン）と、中の数（CorElementType とバイト列）。
pub unsafe fn enum_parts(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<(ComPtr<ICorDebugModule>, u32, u32, Vec<u8>)> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let ty = exact_type(&resolved)?;
    let base = get::<ICorDebugType>(|out| ty.GetBase(out))?;
    if class_name(&base, modules, 0).as_deref() != Some("System.Enum") {
        return None;
    }
    let class = get::<ICorDebugClass>(|out| ty.GetClass(out))?;
    let mut token = 0;
    class.GetToken(&mut token);
    let (element, bytes) = primitive_bytes(&field(&resolved, modules, "value__")?)?;
    Some((get::<ICorDebugModule>(|out| class.GetModule(out))?, token, element, bytes))
}

/// 値の実体の型と、その基底の型、実装しているインターフェイスの、完全な名前（型引数は付けない）。
/// `is` / `as` / キャストで「この型として扱えるか」を調べるのに使う。null は空。
pub unsafe fn type_names(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Vec<String> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    let mut names = Vec::new();
    if resolved.cast::<ICorDebugArrayValue>().is_ok() {
        names.extend(["System.Array", "System.Object", "System.Collections.IEnumerable", "System.Collections.IList", "System.Collections.ICollection"].map(String::from));
        // 一次元の配列は、要素の型のコレクションのインターフェイスも実装している（型引数は、要素の型）。
        names.extend(["IEnumerable", "ICollection", "IList", "IReadOnlyCollection", "IReadOnlyList"].map(|name| format!("System.Collections.Generic.{name}")));
        return names;
    }
    if let Some(ty) = exact_type(&resolved) {
        for_each_class(ty, modules, |_, _, metadata, type_row| {
            names.push(metadata.plain_name(type_row));
            names.extend(metadata.interface_names(type_row));
        });
    }
    names
}

/// 値の実体の型の型引数（`List<string>` の `string`。配列は、要素の型）。表示と同じ書き方。
pub unsafe fn exact_type_arguments(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Vec<String> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    let Some(ty) = exact_type(&resolved) else { return Vec::new() };
    if resolved.cast::<ICorDebugArrayValue>().is_ok() {
        return get::<ICorDebugType>(|out| ty.GetFirstTypeParameter(out)).map(|element| type_name(&element, modules)).into_iter().collect();
    }
    type_arguments(&ty).iter().map(|argument| type_name(argument, modules)).collect()
}

/// 配列の要素の総数。配列でなければ `None`。
pub unsafe fn array_length(value: &ComPtr<ICorDebugValue>) -> Option<u32> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let array = resolved.cast::<ICorDebugArrayValue>().ok()?;
    let mut count = 0u32;
    array.GetCount(&mut count).ok().ok()?;
    Some(count)
}

/// 引数として渡す値の、型の大まかな種類。オーバーロードを選ぶのに使う。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgumentKind {
    /// プリミティブ型。中身は CorElementType。
    Primitive(u8),
    String,
    Null,
    /// 文字列以外の参照（オブジェクト、配列、ボックス）。
    Reference,
    /// 構造体、列挙型。
    ValueType,
    /// `out` / `ref` で渡す引数（値からは作られない。呼び出しの側が印を付ける）。
    ByRef,
}

pub unsafe fn argument_kind(value: &ComPtr<ICorDebugValue>) -> ArgumentKind {
    let mut element = 0;
    if let Ok(reference) = value.cast::<ICorDebugReferenceValue>() {
        let mut is_null = 0;
        reference.IsNull(&mut is_null);
        if is_null != 0 {
            return ArgumentKind::Null;
        }
        return match get::<ICorDebugValue>(|out| reference.Dereference(out)) {
            Some(target) if target.GetType(&mut element).is_ok() && element == ELEMENT_TYPE_STRING => ArgumentKind::String,
            _ => ArgumentKind::Reference,
        };
    }
    value.GetType(&mut element);
    match element {
        ELEMENT_TYPE_BOOLEAN..=ELEMENT_TYPE_R8 | ELEMENT_TYPE_I | ELEMENT_TYPE_U => ArgumentKind::Primitive(element as u8),
        ELEMENT_TYPE_STRING => ArgumentKind::String,
        ELEMENT_TYPE_VALUETYPE => ArgumentKind::ValueType,
        _ => ArgumentKind::Reference,
    }
}

/// 値の型（と基底の型）にある、名前と引数の数が合うインスタンスメソッドすべてと、それぞれの引数の型の種類。
/// 派生した型の分が先。
pub unsafe fn instance_method_candidates(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, name: &str, parameters: u32) -> Vec<(Callable, Vec<ParameterKind>)> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    let Some(ty) = exact_type(&resolved) else { return Vec::new() };
    let mut candidates = Vec::new();
    for_each_class(ty, modules, |ty, class, metadata, type_row| {
        let Some(module) = get::<ICorDebugModule>(|out| class.GetModule(out)) else { return };
        for token in metadata.find_methods_shaped(type_row, name, false, parameters) {
            let (Some(kinds), Some(function)) = (metadata.parameter_kinds(token), get::<ICorDebugFunction>(|out| module.GetFunctionFromToken(token, out))) else { continue };
            candidates.push((Callable { function, type_arguments: type_arguments(ty) }, kinds));
        }
    });
    candidates
}

/// 型のインスタンス化に使われている型引数（`List<string>` の `string`）。
unsafe fn type_arguments(ty: &ComPtr<ICorDebugType>) -> Vec<ComPtr<ICorDebugType>> {
    let mut arguments = Vec::new();
    if let Some(parameters) = get::<ICorDebugTypeEnum>(|out| ty.EnumerateTypeParameters(out)) {
        loop {
            let (mut raw, mut fetched) = (null_mut(), 0u32);
            if !parameters.Next(1, &mut raw, &mut fetched).is_ok() || fetched == 0 {
                break;
            }
            match ComPtr::<ICorDebugType>::from_raw(raw) {
                Some(argument) => arguments.push(argument),
                None => break,
            }
        }
    }
    arguments
}

/// オブジェクトの、読めるインスタンスプロパティ（名前と getter）。派生した型の分が先。配列や文字列、null は空。
pub unsafe fn property_getters(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Vec<(String, Callable)> {
    let Resolved::Value(resolved) = resolve(value) else { return Vec::new() };
    if resolved.cast::<ICorDebugObjectValue>().is_err() {
        return Vec::new();
    }
    let Some(ty) = exact_type(&resolved) else { return Vec::new() };
    let mut getters = Vec::new();
    for_each_class(ty, modules, |ty, class, metadata, type_row| {
        let Some(module) = get::<ICorDebugModule>(|out| class.GetModule(out)) else { return };
        for (name, getter) in metadata.instance_properties(type_row) {
            if let Some(function) = get::<ICorDebugFunction>(|out| module.GetFunctionFromToken(getter, out)) {
                getters.push((name.to_string(), Callable { function, type_arguments: type_arguments(ty) }));
            }
        }
    });
    getters
}

/// オブジェクトの型（か基底の型）に付いた `DebuggerDisplay` の書式。派生した型のものが優先。
pub unsafe fn debugger_display(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    resolved.cast::<ICorDebugObjectValue>().ok()?;
    let mut format = None;
    for_each_class(exact_type(&resolved)?, modules, |_, _, metadata, type_row| {
        if format.is_none() {
            format = metadata.debugger_display(type_row);
        }
    });
    format
}

/// オブジェクトの型（か基底の型）に付いた `DebuggerDisplay` の `Name`（変数の欄の名前の書式）。
pub unsafe fn debugger_display_name(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    resolved.cast::<ICorDebugObjectValue>().ok()?;
    let mut format = None;
    for_each_class(exact_type(&resolved)?, modules, |_, _, metadata, type_row| {
        if format.is_none() {
            format = metadata.debugger_display_name(type_row);
        }
    });
    format
}

/// `DebuggerDisplay` の書式を、文字のままの部分と `{名前}` の部分に分ける。
/// 名前の部分は「メンバーの名前、`,nq`（引用符を付けない）が付いているか」。メンバーの名前だけでない式
/// （`{Items.Count}`、`{Describe()}`）は、評価の本体が入るまで扱えないので `None`。
pub fn parse_display_format(format: &str) -> Option<Vec<DisplayPart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // `\{` は、波括弧そのもの。
            '\\' if matches!(chars.peek(), Some('{' | '}')) => text.extend(chars.next()),
            '{' => {
                if !text.is_empty() {
                    parts.push(DisplayPart::Text(std::mem::take(&mut text)));
                }
                // `}` までを、式として取り出す。式の中の括弧、文字列、文字の中の `}` や `,` は数えない。
                let (mut inside, mut depth, mut quote) = (String::new(), 0usize, None::<char>);
                let mut format_at = None;
                loop {
                    let c = chars.next()?;
                    match (quote, c) {
                        (Some(_), '\\') => {
                            inside.push(c);
                            inside.extend(chars.next());
                            continue;
                        }
                        (Some(open), c) if c == open => quote = None,
                        (Some(_), _) => {}
                        (None, '"' | '\'') => quote = Some(c),
                        (None, '(' | '[' | '{') => depth += 1,
                        (None, ')' | ']') => depth = depth.saturating_sub(1),
                        (None, '}') if depth == 0 => break,
                        (None, '}') => depth -= 1,
                        (None, ',') if depth == 0 => format_at = Some(inside.len()),
                        _ => {}
                    }
                    inside.push(c);
                }
                // 最後の `,` のあとは、書式の指定（`nq` など）。
                let (expression, options) = match format_at {
                    Some(at) => (inside[..at].to_string(), inside[at + 1..].to_string()),
                    None => (inside, String::new()),
                };
                if expression.trim().is_empty() {
                    return None;
                }
                parts.push(DisplayPart::Member { name: expression.trim().to_string(), no_quotes: options.split(',').any(|o| o.trim() == "nq") });
            }
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(DisplayPart::Text(text));
    }
    Some(parts)
}
#[derive(Debug, PartialEq)]
pub enum DisplayPart {
    Text(String),
    Member { name: String, no_quotes: bool },
}

/// オブジェクトのフィールド（自動実装プロパティは、その名前で）。無ければ `None`。
pub unsafe fn field(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules, name: &str) -> Option<ComPtr<ICorDebugValue>> {
    children(value, modules, 0, usize::MAX).into_iter().find_map(|member| match member {
        Member::Value { name: existing, value } if existing == name => Some(value),
        _ => None,
    })
}

/// 配列の要素。`Ok(None)` は「配列ではない」。添字が範囲の外、次元の数が違う、などは `Err`。
pub unsafe fn array_element(value: &ComPtr<ICorDebugValue>, indices: &[u32]) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
    let resolved = match resolve(value) {
        Resolved::Value(resolved) => resolved,
        Resolved::Null => return Err(tr!("null cannot be indexed.")),
        Resolved::Unreadable => return Err(tr!("Cannot read the value.")),
    };
    let Ok(array) = resolved.cast::<ICorDebugArrayValue>() else { return Ok(None) };
    let dimensions = array_dimensions(&array);
    if dimensions.len() != indices.len() {
        return Err(tr!("This array has {rank} dimensions, but there are {count} indexes.", rank = dimensions.len(), count = indices.len()));
    }
    if let Some((index, size)) = indices.iter().zip(&dimensions).find(|(index, size)| index >= size) {
        return Err(tr!("The index {index} is out of range (there are {size} elements).", index = index, size = size));
    }
    let mut indices = indices.to_vec();
    get::<ICorDebugValue>(|out| array.GetElement(indices.len() as u32, indices.as_mut_ptr(), out)).map(Some).ok_or_else(|| tr!("Cannot read the array element."))
}

/// 値（参照なら指す先）の実際の型の名前。対象のコード（DebuggerDisplay、ToString）は実行しない。null と読めない値は `None`。
pub unsafe fn runtime_type_name(value: &ComPtr<ICorDebugValue>, modules: &dyn Modules) -> Option<String> {
    let Resolved::Value(resolved) = resolve(value) else { return None };
    let value2 = resolved.cast::<ICorDebugValue2>().ok()?;
    let ty = get::<ICorDebugType>(|out| value2.GetExactType(out))?;
    Some(type_name(&ty, modules))
}

/// 値が文字列なら、その中身。
pub unsafe fn as_string(value: &ComPtr<ICorDebugValue>) -> Option<String> {
    match resolve(value) {
        Resolved::Value(resolved) => read_string(&resolved),
        _ => None,
    }
}

/// 値がヒープのオブジェクトを指していれば、それをつかんだままにするハンドルを作る。
///
/// ふつうの値のオブジェクトは、対象を続行すると使えなくなる。ステップの途中で拾った戻り値を、止まったあとで
/// 開けるようにするのに使う。使い終わったら `dispose_handle` で放す。値型と null には作れない（`None`）。
pub unsafe fn strong_handle(value: &ComPtr<ICorDebugValue>) -> Option<ComPtr<ICorDebugValue>> {
    let reference = value.cast::<ICorDebugReferenceValue>().ok()?;
    let mut is_null = 1;
    if !reference.IsNull(&mut is_null).is_ok() || is_null != 0 {
        return None;
    }
    let target = get::<ICorDebugValue>(|out| reference.Dereference(out))?;
    let heap = target.cast::<ICorDebugHeapValue2>().ok()?;
    get::<ICorDebugHandleValue>(|out| heap.CreateHandle(HANDLE_STRONG, out))?.cast().ok()
}

/// `strong_handle` で作ったハンドルを放す（放さないと、対象のオブジェクトが回収されない）。
pub unsafe fn dispose_handle(handle: &ComPtr<ICorDebugValue>) {
    if let Ok(handle) = handle.cast::<ICorDebugHandleValue>() {
        handle.Dispose();
    }
}

/// コンパイラが作った状態機械（`async` メソッドやイテレーターの本体）の型か。
/// 名前は、C# が `<メソッド名>d__番号`、VB が `VB$StateMachine_番号_メソッド名`。
pub fn is_state_machine(type_name: &str) -> bool {
    let last = type_name.rsplit('.').next().unwrap_or(type_name);
    (last.starts_with('<') && last.contains(">d__")) || last.starts_with("VB$StateMachine_")
}

/// コンパイラが作ったクロージャ（ラムダに取り込まれた変数の入れ物）の型か。
/// 名前は、C# が `<>c__DisplayClass番号`、VB が `_Closure$__番号`。
pub fn is_closure(type_name: &str) -> bool {
    let last = type_name.rsplit('.').next().unwrap_or(type_name);
    last.starts_with("<>c__DisplayClass") || last.starts_with("_Closure$__")
}

/// クロージャを指している変数やフィールドの名前か（中の変数を、もとの名前で見せるために開く）。
///   C#: ローカル変数 `CS$<>8__locals0`、状態機械のフィールド `<>8__1`
///   VB: ローカル変数 `$VB$Closure_0`、フィールド `$VB$ResumableLocal_$VB$Closure_0$1`、`$VB$NonLocal_$VB$Closure_2`
pub fn holds_closure(name: &str) -> bool {
    name.starts_with("CS$<>8__locals") || name.starts_with("<>8__") || name.contains("$VB$Closure_")
}

/// 状態機械のフィールドを、もとのメソッドでの名前にする。利用者の書いた変数でなければ `None`。
///   `<total>5__1` → `total`（await をまたぐローカル変数）、`<>4__this` → `this`、`args` → `args`（引数の写し）。
///   `<>1__state`、`<>t__builder`、`<>u__1` などは、コンパイラの都合のフィールド。
///
/// VB では `$VB$Local_total`（引数と、クロージャに取り込まれた変数）、`$VB$ResumableLocal_total$1`（await をまたぐ
/// ローカル変数）、`$VB$Me` が利用者の変数で、`$State`、`$Builder`、`$A0` などはコンパイラの都合。
pub fn state_machine_variable(field: &str) -> Option<&str> {
    if field == "<>4__this" || field == "$VB$Me" {
        return Some("this");
    }
    if let Some(name) = field.strip_prefix("$VB$Local_") {
        return Some(name);
    }
    if let Some(rest) = field.strip_prefix("$VB$ResumableLocal_") {
        return rest.rsplit_once('$').map(|(name, _)| name).filter(|name| !name.is_empty());
    }
    if field.starts_with('$') {
        return None;
    }
    match field.strip_prefix('<') {
        None => Some(field),
        Some(rest) => rest.split_once(">5__").map(|(name, _)| name).filter(|name| !name.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_decimal_and_signed_by_their_type() {
        assert_eq!(format_primitive(ELEMENT_TYPE_I4, &42i32.to_le_bytes()).as_deref(), Some("42"));
        assert_eq!(format_primitive(ELEMENT_TYPE_I4, &(-1i32).to_le_bytes()).as_deref(), Some("-1"));
        assert_eq!(format_primitive(ELEMENT_TYPE_U4, &(-1i32).to_le_bytes()).as_deref(), Some("4294967295"));
        assert_eq!(format_primitive(ELEMENT_TYPE_I1, &[0xFF]).as_deref(), Some("-1"));
        assert_eq!(format_primitive(ELEMENT_TYPE_U8, &u64::MAX.to_le_bytes()).as_deref(), Some("18446744073709551615"));
    }

    #[test]
    fn a_bool_is_a_keyword() {
        assert_eq!(format_primitive(ELEMENT_TYPE_BOOLEAN, &[1]).as_deref(), Some("true"));
        assert_eq!(format_primitive(ELEMENT_TYPE_BOOLEAN, &[0]).as_deref(), Some("false"));
    }

    #[test]
    fn a_char_shows_its_code_and_the_character() {
        assert_eq!(format_primitive(ELEMENT_TYPE_CHAR, &[97, 0]).as_deref(), Some("97 'a'"));
        assert_eq!(format_primitive(ELEMENT_TYPE_CHAR, &0xD800u16.to_le_bytes()).as_deref(), Some("55296"));
    }

    #[test]
    fn a_whole_floating_point_number_has_no_fraction() {
        assert_eq!(format_primitive(ELEMENT_TYPE_R8, &1.0f64.to_le_bytes()).as_deref(), Some("1"));
        assert_eq!(format_primitive(ELEMENT_TYPE_R8, &2.75f64.to_le_bytes()).as_deref(), Some("2.75"));
        assert_eq!(format_primitive(ELEMENT_TYPE_R4, &0.5f32.to_le_bytes()).as_deref(), Some("0.5"));
    }

    #[test]
    fn a_pointer_is_as_wide_as_its_bytes() {
        assert_eq!(format_primitive(ELEMENT_TYPE_I, &0x1234u32.to_le_bytes()).as_deref(), Some("0x00001234"));
        assert_eq!(format_primitive(ELEMENT_TYPE_I, &0x1234u64.to_le_bytes()).as_deref(), Some("0x0000000000001234"));
    }

    #[test]
    fn a_value_shorter_than_its_type_is_not_formatted() {
        assert_eq!(format_primitive(ELEMENT_TYPE_I4, &[1, 2]), None);
        assert_eq!(format_primitive(ELEMENT_TYPE_CLASS, &[0; 8]), None);
    }

    #[test]
    fn a_string_is_quoted_with_csharp_escapes() {
        assert_eq!(quote("a\"b\\c\n\t"), r#""a\"b\\c\n\t""#);
        assert_eq!(quote("日本語"), "\"日本語\"");
    }

    #[test]
    fn a_jagged_array_names_the_outer_dimension_first() {
        assert_eq!(array_type_name("int", ""), "int[]");
        assert_eq!(array_type_name("int", ","), "int[,]");
        assert_eq!(array_type_name("int[]", ""), "int[][]");
        assert_eq!(array_type_name("int[,]", ""), "int[][,]");
    }

    #[test]
    fn well_known_types_use_their_csharp_names() {
        assert_eq!(csharp_alias("System.Object".into()), "object");
        assert_eq!(csharp_alias("System.Nullable<int>".into()), "int?");
        assert_eq!(csharp_alias("App.Order".into()), "App.Order");
    }

    #[test]
    fn a_state_machine_type_is_recognized_by_its_name() {
        assert!(is_state_machine("Basic.Program.<Main>d__0"));
        assert!(is_state_machine("<Run>d__12"));
        assert!(!is_state_machine("Basic.Program"));
        assert!(!is_state_machine("Basic.Program.<>c__DisplayClass0_0"));
    }

    #[test]
    fn state_machine_fields_get_their_source_names() {
        assert_eq!(state_machine_variable("<total>5__1"), Some("total"));
        assert_eq!(state_machine_variable("<>4__this"), Some("this"));
        assert_eq!(state_machine_variable("args"), Some("args"));
        assert_eq!(state_machine_variable("<>1__state"), None);
        assert_eq!(state_machine_variable("<>t__builder"), None);
        assert_eq!(state_machine_variable("<>u__1"), None);
        // VB
        assert!(is_state_machine("Program.VB$StateMachine_3_WaitAndAdd"));
        assert_eq!(state_machine_variable("$VB$Local_a"), Some("a"));
        assert_eq!(state_machine_variable("$VB$ResumableLocal_before$0"), Some("before"));
        assert_eq!(state_machine_variable("$VB$Me"), Some("this"));
        assert_eq!(state_machine_variable("$State"), None);
        assert_eq!(state_machine_variable("$Builder"), None);
        assert_eq!(state_machine_variable("$A0"), None);
        // クロージャ
        assert!(is_closure("Basic.Program.<>c__DisplayClass0_0"));
        assert!(is_closure("Program._Closure$__4-0"));
        assert!(!is_closure("Basic.Program.<>c"));
        assert!(holds_closure("CS$<>8__locals0") && holds_closure("<>8__1") && holds_closure("$VB$Closure_0") && holds_closure("$VB$ResumableLocal_$VB$Closure_0$1"));
        assert!(!holds_closure("total"));
    }

    #[test]
    fn a_display_format_is_split_into_text_and_members() {
        let member = |name: &str, no_quotes| DisplayPart::Member { name: name.into(), no_quotes };
        assert_eq!(parse_display_format("Count = {Count}"), Some(vec![DisplayPart::Text("Count = ".into()), member("Count", false)]));
        assert_eq!(parse_display_format("{Name,nq} ({_id})"), Some(vec![member("Name", true), DisplayPart::Text(" (".into()), member("_id", false), DisplayPart::Text(")".into())]));
        assert_eq!(parse_display_format("plain"), Some(vec![DisplayPart::Text("plain".into())]));
    }

    #[test]
    fn a_display_format_can_hold_expressions() {
        let member = |name: &str, no_quotes| DisplayPart::Member { name: name.into(), no_quotes };
        assert_eq!(parse_display_format("{Items.Count} items"), Some(vec![member("Items.Count", false), DisplayPart::Text(" items".into())]));
        assert_eq!(parse_display_format("{Describe()}"), Some(vec![member("Describe()", false)]));
        assert_eq!(parse_display_format("{F(a, b),nq}"), Some(vec![member("F(a, b)", true)]));
        assert_eq!(parse_display_format("{s == \"}\" ? 1 : 2}"), Some(vec![member("s == \"}\" ? 1 : 2", false)]));
        assert_eq!(parse_display_format("\\{literal\\}"), Some(vec![DisplayPart::Text("{literal}".into())]));
        assert_eq!(parse_display_format("{unclosed"), None);
        assert_eq!(parse_display_format("{}"), None);
    }

    #[test]
    fn a_decimal_keeps_its_scale() {
        assert_eq!(format_decimal(1250, 2, false), "12.50");
        assert_eq!(format_decimal(1250, 0, false), "1250");
        assert_eq!(format_decimal(5, 3, true), "-0.005");
        assert_eq!(format_decimal(0, 0, false), "0");
        assert_eq!(format_decimal(79_228_162_514_264_337_593_543_950_335, 0, false), "79228162514264337593543950335");
    }

    #[test]
    fn an_enum_value_is_written_by_name() {
        let names = [("None", 0), ("Alt", 1), ("Shift", 2), ("Control", 4)];
        assert_eq!(enum_text(2, &names).as_deref(), Some("Shift"));
        assert_eq!(enum_text(0, &names).as_deref(), Some("None"));
        assert_eq!(enum_text(3, &names).as_deref(), Some("Alt | Shift"));
        assert_eq!(enum_text(7, &names).as_deref(), Some("Alt | Shift | Control"));
    }

    #[test]
    fn an_enum_value_without_names_is_written_as_a_number() {
        let names = [("Alt", 1), ("Shift", 2)];
        assert_eq!(enum_text(8, &names), None);
        assert_eq!(enum_text(9, &names), None, "名前の無いビットが残るなら、全体を数で");
        assert_eq!(enum_text(0, &names), None);
        assert_eq!(enum_number(0x08, 0xFFFF_FFFA), "-6", "int の列挙型は符号つき");
        assert_eq!(enum_number(0x08, 0x8000_0000), "-2147483648");
        assert_eq!(enum_number(0x05, 250), "250", "byte の列挙型は符号なし");
        assert_eq!(enum_number(0x0A, u64::MAX), "-1");
    }

    #[test]
    fn a_backing_field_is_shown_as_its_property() {
        assert_eq!(member_name("<Name>k__BackingField"), "Name");
        assert_eq!(member_name("_count"), "_count");
        assert_eq!(member_name("<>4__this"), "<>4__this");
    }
}
