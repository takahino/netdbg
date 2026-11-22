//! C# の組み込みの型の表: キーワード（`int`）、完全な名前（`System.Int32`）、シグネチャの中の番号（CorElementType）。
//!
//! 同じ対応を、構文解析（キーワード → 名前）、値の表示（名前 → キーワード）、式の評価（番号 ⇔ 名前）が使う。

/// 組み込みの型。
pub struct BuiltinType {
    /// C# のキーワード。
    pub keyword: &'static str,
    /// 完全な名前。
    pub full_name: &'static str,
    /// CorElementType。`decimal` は、ランタイムから見ればふつうの構造体なので、番号を持たない。
    pub element: Option<u8>,
}

impl BuiltinType {
    /// プリミティブ型（数、`bool`、`char`、`nint` / `nuint`）か。値を、そのままのバイト列で持つ型。
    pub fn is_primitive(&self) -> bool {
        matches!(self.element, Some(0x02..=0x0D | 0x18 | 0x19))
    }
}

const fn entry(keyword: &'static str, full_name: &'static str, element: Option<u8>) -> BuiltinType {
    BuiltinType { keyword, full_name, element }
}

pub const TYPES: [BuiltinType; 17] = [
    entry("bool", "System.Boolean", Some(0x02)),
    entry("char", "System.Char", Some(0x03)),
    entry("sbyte", "System.SByte", Some(0x04)),
    entry("byte", "System.Byte", Some(0x05)),
    entry("short", "System.Int16", Some(0x06)),
    entry("ushort", "System.UInt16", Some(0x07)),
    entry("int", "System.Int32", Some(0x08)),
    entry("uint", "System.UInt32", Some(0x09)),
    entry("long", "System.Int64", Some(0x0A)),
    entry("ulong", "System.UInt64", Some(0x0B)),
    entry("float", "System.Single", Some(0x0C)),
    entry("double", "System.Double", Some(0x0D)),
    entry("decimal", "System.Decimal", None),
    entry("string", "System.String", Some(0x0E)),
    entry("object", "System.Object", Some(0x1C)),
    entry("nint", "System.IntPtr", Some(0x18)),
    entry("nuint", "System.UIntPtr", Some(0x19)),
];

pub fn by_keyword(keyword: &str) -> Option<&'static BuiltinType> {
    TYPES.iter().find(|ty| ty.keyword == keyword)
}

pub fn by_full_name(full_name: &str) -> Option<&'static BuiltinType> {
    TYPES.iter().find(|ty| ty.full_name == full_name)
}

pub fn by_element(element: u8) -> Option<&'static BuiltinType> {
    TYPES.iter().find(|ty| ty.element == Some(element))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_names_of_a_type_point_at_each_other() {
        assert_eq!(by_keyword("int").map(|ty| ty.full_name), Some("System.Int32"));
        assert_eq!(by_full_name("System.Int32").map(|ty| ty.keyword), Some("int"));
        assert_eq!(by_element(0x08).map(|ty| ty.keyword), Some("int"));
        assert!(by_keyword("Int32").is_none());
        // decimal、string、object は、値をバイト列で持つ型ではない。
        assert!(by_keyword("nint").is_some_and(BuiltinType::is_primitive));
        assert!(!by_keyword("decimal").is_some_and(BuiltinType::is_primitive));
        assert!(!by_keyword("string").is_some_and(BuiltinType::is_primitive));
        assert!(!by_keyword("object").is_some_and(BuiltinType::is_primitive));
    }
}
