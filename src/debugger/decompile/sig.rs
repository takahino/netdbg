//! シグネチャ（ECMA-335 II.23.2）を、型の木に読む。
//!
//! `metadata.rs` の型の読み方は、式の評価のための大まかな形（名前にできない型は捨てる）。逆コンパイルでは、ポインター、
//! 多次元の配列、関数ポインター、修飾子まで、宣言どおりに書く必要があるので、ここで木として読む。

use crate::debugger::metadata::Reader;

/// 型。名前にするのは `names.rs`。
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Void,
    Bool,
    Char,
    I1,
    U1,
    I2,
    U2,
    I4,
    U4,
    I8,
    U8,
    R4,
    R8,
    String,
    Object,
    IntPtr,
    UIntPtr,
    TypedReference,
    /// クラスか値型。`token` は TypeDef（0x02）、TypeRef（0x01）、TypeSpec（0x1B）のトークン。
    Named { token: u32, value_type: bool },
    /// ジェネリック型のインスタンス（`List<int>`）。
    Generic { token: u32, value_type: bool, arguments: Vec<Ty> },
    /// 型の型引数（`!0`）。
    Var(u32),
    /// メソッドの型引数（`!!0`）。
    MVar(u32),
    /// 1 次元で 0 始まりの配列（`T[]`）。
    SzArray(Box<Ty>),
    /// 多次元の配列（`T[,]`）。
    Array { element: Box<Ty>, rank: u32 },
    Pointer(Box<Ty>),
    ByRef(Box<Ty>),
    FnPtr(Box<MethodSig>),
    /// `modreq` / `modopt`（`volatile`、`in` 引数など）。
    Modified { required: bool, modifier: u32, inner: Box<Ty> },
    /// ローカル変数の `pinned`（`fixed`）。
    Pinned(Box<Ty>),
}

impl Ty {
    /// 修飾子と `pinned` を外した型。
    pub fn unmodified(&self) -> &Ty {
        match self {
            Ty::Modified { inner, .. } | Ty::Pinned(inner) => inner.unmodified(),
            other => other,
        }
    }

    pub fn is_void(&self) -> bool {
        matches!(self.unmodified(), Ty::Void)
    }

    /// 型引数（`!0`、`!!0`）を、実際の型に置き換える。置き換えられない型引数が残れば `None`。
    pub fn substitute(&self, types: &[Ty], methods: &[Ty]) -> Option<Ty> {
        let each = |inner: &Ty| inner.substitute(types, methods).map(Box::new);
        Some(match self {
            Ty::Var(i) => types.get(*i as usize)?.clone(),
            Ty::MVar(i) => methods.get(*i as usize)?.clone(),
            Ty::Generic { token, value_type, arguments } => Ty::Generic { token: *token, value_type: *value_type, arguments: arguments.iter().map(|a| a.substitute(types, methods)).collect::<Option<_>>()? },
            Ty::SzArray(e) => Ty::SzArray(each(e)?),
            Ty::Array { element, rank } => Ty::Array { element: each(element)?, rank: *rank },
            Ty::Pointer(e) => Ty::Pointer(each(e)?),
            Ty::ByRef(e) => Ty::ByRef(each(e)?),
            Ty::Modified { required, modifier, inner } => Ty::Modified { required: *required, modifier: *modifier, inner: each(inner)? },
            Ty::Pinned(e) => Ty::Pinned(each(e)?),
            Ty::FnPtr(_) => return None,
            other => other.clone(),
        })
    }
}

/// メソッドのシグネチャ（MethodDef、MemberRef、StandAloneSig の calli、関数ポインター）。
#[derive(Debug, Clone, PartialEq)]
pub struct MethodSig {
    pub has_this: bool,
    /// `this` が引数の並びに書かれている（C# では出ない）。
    pub explicit_this: bool,
    /// 呼び出し規約の下位 4 ビット（0 = 既定、5 = vararg、そのほかはアンマネージド）。
    pub convention: u8,
    pub generic_count: u32,
    pub ret: Ty,
    pub params: Vec<Ty>,
}

impl MethodSig {
    /// 呼び出しで評価スタックから取り出す数（`this` を含む）。
    pub fn stack_arguments(&self) -> u32 {
        self.params.len() as u32 + (self.has_this && !self.explicit_this) as u32
    }
}

const HAS_THIS: u8 = 0x20;
const EXPLICIT_THIS: u8 = 0x40;
const GENERIC: u8 = 0x10;
/// 壊れたシグネチャで深く潜り続けないための上限。
const MAX_DEPTH: u32 = 64;

/// TypeDefOrRef の複合添字を、トークンにする。
pub fn type_def_or_ref_token(coded: u32) -> u32 {
    let table = match coded & 3 {
        0 => 0x02,
        1 => 0x01,
        _ => 0x1B,
    };
    table << 24 | coded >> 2
}

fn read(r: &mut Reader, depth: u32) -> Result<Ty, String> {
    if depth > MAX_DEPTH {
        return Err(tr!("The signature is nested too deeply"));
    }
    let next = |r: &mut Reader| read(r, depth + 1).map(Box::new);
    Ok(match r.u8()? {
        0x01 => Ty::Void,
        0x02 => Ty::Bool,
        0x03 => Ty::Char,
        0x04 => Ty::I1,
        0x05 => Ty::U1,
        0x06 => Ty::I2,
        0x07 => Ty::U2,
        0x08 => Ty::I4,
        0x09 => Ty::U4,
        0x0A => Ty::I8,
        0x0B => Ty::U8,
        0x0C => Ty::R4,
        0x0D => Ty::R8,
        0x0E => Ty::String,
        0x0F => Ty::Pointer(next(r)?),
        0x10 => Ty::ByRef(next(r)?),
        0x11 => Ty::Named { token: type_def_or_ref_token(r.unsigned()?), value_type: true },
        0x12 => Ty::Named { token: type_def_or_ref_token(r.unsigned()?), value_type: false },
        0x13 => Ty::Var(r.unsigned()?),
        0x14 => {
            let element = next(r)?;
            let rank = r.unsigned()?;
            // 次元ごとの大きさと下限（C# では書けないので読み飛ばす）。
            for _ in 0..r.unsigned()? {
                r.unsigned()?;
            }
            for _ in 0..r.unsigned()? {
                r.signed()?;
            }
            Ty::Array { element, rank }
        }
        0x15 => {
            let value_type = r.u8()? == 0x11;
            let token = type_def_or_ref_token(r.unsigned()?);
            let count = r.unsigned()?;
            let arguments = (0..count).map(|_| read(r, depth + 1)).collect::<Result<_, _>>()?;
            Ty::Generic { token, value_type, arguments }
        }
        0x16 => Ty::TypedReference,
        0x18 => Ty::IntPtr,
        0x19 => Ty::UIntPtr,
        0x1B => Ty::FnPtr(Box::new(method_from(r, depth + 1)?)),
        0x1C => Ty::Object,
        0x1D => Ty::SzArray(next(r)?),
        0x1E => Ty::MVar(r.unsigned()?),
        tag @ (0x1F | 0x20) => {
            let modifier = type_def_or_ref_token(r.unsigned()?);
            Ty::Modified { required: tag == 0x1F, modifier, inner: next(r)? }
        }
        0x45 => Ty::Pinned(next(r)?),
        other => return Err(tr!("The signature has an unknown type mark {mark}", mark = format!("0x{other:02X}"))),
    })
}

fn method_from(r: &mut Reader, depth: u32) -> Result<MethodSig, String> {
    let flags = r.u8()?;
    let generic_count = if flags & GENERIC != 0 { r.unsigned()? } else { 0 };
    let count = r.unsigned()?;
    let ret = read(r, depth)?;
    let mut params = Vec::with_capacity(count.min(256) as usize);
    for _ in 0..count {
        // vararg の呼び出し側の印（SENTINEL）。そのあとの引数も並べる。
        if r.data.get(r.pos) == Some(&0x41) {
            r.pos += 1;
        }
        params.push(read(r, depth)?);
    }
    Ok(MethodSig { has_this: flags & HAS_THIS != 0, explicit_this: flags & EXPLICIT_THIS != 0, convention: flags & 0x0F, generic_count, ret, params })
}

/// メソッドのシグネチャ（MethodDef、MemberRef、StandAloneSig の blob）。
pub fn method(blob: &[u8]) -> Result<MethodSig, String> {
    method_from(&mut Reader::new(blob), 0)
}

/// フィールドのシグネチャ: 0x06 と型。
pub fn field(blob: &[u8]) -> Result<Ty, String> {
    let mut r = Reader::new(blob);
    if r.u8()? != 0x06 {
        return Err(tr!("Not a field signature"));
    }
    read(&mut r, 0)
}

/// MemberRef のシグネチャが、フィールドのものか（メソッドのものか）。
pub fn is_field(blob: &[u8]) -> bool {
    blob.first() == Some(&0x06)
}

/// プロパティのシグネチャ: 0x08（インスタンスなら 0x28）、引数の数、型、引数の型。
pub fn property(blob: &[u8]) -> Result<(Ty, Vec<Ty>), String> {
    let mut r = Reader::new(blob);
    if r.u8()? & 0x0F != 0x08 {
        return Err(tr!("Not a property signature"));
    }
    let count = r.unsigned()?;
    let ty = read(&mut r, 0)?;
    let params = (0..count).map(|_| read(&mut r, 0)).collect::<Result<_, _>>()?;
    Ok((ty, params))
}

/// ローカル変数のシグネチャ: 0x07、数、型の並び。
pub fn locals(blob: &[u8]) -> Result<Vec<Ty>, String> {
    let mut r = Reader::new(blob);
    if r.u8()? != 0x07 {
        return Err(tr!("Not a local variable signature"));
    }
    let count = r.unsigned()?;
    (0..count).map(|_| read(&mut r, 0)).collect()
}

/// TypeSpec の blob: 型一つ。
pub fn type_spec(blob: &[u8]) -> Result<Ty, String> {
    read(&mut Reader::new(blob), 0)
}

/// MethodSpec の blob: 0x0A、数、型引数。
pub fn method_spec(blob: &[u8]) -> Result<Vec<Ty>, String> {
    let mut r = Reader::new(blob);
    if r.u8()? != 0x0A {
        return Err(tr!("Not a method type argument signature"));
    }
    let count = r.unsigned()?;
    (0..count).map(|_| read(&mut r, 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_signature_has_its_return_type_and_parameters() {
        // instance, 2 個の引数, 戻り値 int, string, int[]
        let sig = method(&[0x20, 0x02, 0x08, 0x0E, 0x1D, 0x08]).unwrap();
        assert!(sig.has_this);
        assert_eq!(sig.ret, Ty::I4);
        assert_eq!(sig.params, [Ty::String, Ty::SzArray(Box::new(Ty::I4))]);
        assert_eq!(sig.stack_arguments(), 3);
    }

    #[test]
    fn generic_instances_and_type_parameters_are_kept() {
        // GENERICINST CLASS TypeRef(行 3) 1 個: !!0
        let ty = type_spec(&[0x15, 0x12, 0x0D, 0x01, 0x1E, 0x00]).unwrap();
        assert_eq!(ty, Ty::Generic { token: 0x0100_0003, value_type: false, arguments: vec![Ty::MVar(0)] });
    }

    #[test]
    fn multidimensional_arrays_skip_their_bounds() {
        // ARRAY int rank 2, sizes 0, lobounds 0
        assert_eq!(type_spec(&[0x14, 0x08, 0x02, 0x00, 0x00]).unwrap(), Ty::Array { element: Box::new(Ty::I4), rank: 2 });
    }

    #[test]
    fn locals_keep_pinned_and_by_ref() {
        // LOCAL_SIG 2: pinned byref char, object
        let locals = locals(&[0x07, 0x02, 0x45, 0x10, 0x03, 0x1C]).unwrap();
        assert_eq!(locals[0], Ty::Pinned(Box::new(Ty::ByRef(Box::new(Ty::Char)))));
        assert_eq!(locals[0].unmodified(), &Ty::ByRef(Box::new(Ty::Char)));
        assert_eq!(locals[1], Ty::Object);
    }

    #[test]
    fn broken_signatures_are_errors_not_panics() {
        assert!(method(&[0x20]).is_err());
        assert!(method(&[0x00, 0x01, 0x08, 0x99]).is_err());
        let deep = vec![0x0F; 200];
        assert!(type_spec(&deep).is_err());
    }
}
