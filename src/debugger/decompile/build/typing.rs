//! 式の型の推定と、型に合わせた定数の直し（`1` を `true` に、`0` を `null` に、など）。条件の形（`!= null`、`!= 0`）も。

use super::super::ast::{visit_expressions, visit_expressions_mut, CmpOp, Expr, Stmt, StmtKind};
use super::super::names::Names;
use super::super::sig::Ty;
use super::{negate, Builder};
use crate::debugger::metadata::{FIELD, MEMBER_REF};

impl Builder<'_> {
    /// 条件として使う値: bool はそのまま、参照は `!= null`、整数は `!= 0`。
    pub(super) fn truthy(&self, value: Expr) -> Expr {
        if is_boolean(&value) {
            return value;
        }
        match self.type_of(&value) {
            Some(Ty::Bool) => value,
            Some(ty) if is_reference(&ty) => Expr::Compare(CmpOp::Ne, value.boxed(), Expr::Null.boxed()),
            // 箱詰めした値（ジェネリックの T）と、`as` の結果は、null と比べる（`x is T` / `x != null`）。
            _ if matches!(value, Expr::As { .. } | Expr::Box { .. } | Expr::Null | Expr::Str(_)) => Expr::Compare(CmpOp::Ne, value.boxed(), Expr::Null.boxed()),
            _ => Expr::Compare(CmpOp::Ne, value.boxed(), Expr::Int(0).boxed()),
        }
    }

    /// 比べる 2 つの値の型をそろえる（char と整数の定数、bool と 0 / 1 など）。
    pub(super) fn matched(&self, a: Expr, b: Expr) -> (Expr, Expr) {
        match (self.type_of(&a), self.type_of(&b)) {
            (Some(ty), _) if is_literal(&b) => {
                let b = self.coerce(b, &ty);
                (a, b)
            }
            (_, Some(ty)) if is_literal(&a) => {
                let a = self.coerce(a, &ty);
                (a, b)
            }
            _ => (a, b),
        }
    }

    /// 型が、組み込みでない値型（列挙型、構造体）か。
    pub(super) fn is_value_type(&self, value: &Expr) -> bool {
        matches!(self.type_of(value), Some(Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. }))
    }

    pub(super) fn is_integer(&self, value: &Expr) -> bool {
        matches!(self.type_of(value), Some(Ty::I1 | Ty::U1 | Ty::I2 | Ty::U2 | Ty::I4 | Ty::U4 | Ty::I8 | Ty::U8 | Ty::Char | Ty::IntPtr | Ty::UIntPtr)) || matches!(value, Expr::Int(_) | Expr::Long(_))
    }

    /// 符号なしとして扱う（`div.un` など）: 符号つきの整数なら、符号なしの型に変換する。
    pub(super) fn unsigned(&self, value: Expr) -> Expr {
        let to = match self.type_of(&value) {
            Some(Ty::I1 | Ty::I2 | Ty::I4) => Ty::U4,
            Some(Ty::I8) => Ty::U8,
            Some(Ty::IntPtr) => Ty::UIntPtr,
            _ if matches!(value, Expr::Int(_)) => return value,
            _ => return value,
        };
        Expr::Convert { to, checked: false, value: value.boxed() }
    }

    /// 値を型 `ty` に合う書き方にする（定数の bool、char、列挙型、符号なし、null）。
    pub(super) fn coerce(&self, value: Expr, ty: &Ty) -> Expr {
        let ty = ty.unmodified();
        match (&value, ty) {
            (Expr::Int(n), Ty::Bool) => Expr::Bool(*n != 0),
            (Expr::Int(n), Ty::Char) => Expr::Char(*n as u16),
            (Expr::Int(n), Ty::U4) if *n < 0 => Expr::Literal(format!("{}u", *n as i32 as u32)),
            (Expr::Int(n), Ty::U1) if *n < 0 => Expr::Literal(format!("unchecked((byte)({n}))")),
            (Expr::Int(n), Ty::U2) if *n < 0 => Expr::Literal(format!("unchecked((ushort)({n}))")),
            (Expr::Int(n), Ty::I8) => Expr::Long(*n),
            (Expr::Int(n), Ty::U8) => Expr::Literal(format!("{}UL", *n as u64)),
            (Expr::Long(n), Ty::U8) if *n < 0 => Expr::Literal(format!("{}UL", *n as u64)),
            (Expr::Int(n), Ty::R4) => Expr::Float(*n as f64),
            (Expr::Int(n), Ty::R8) => Expr::Double(*n as f64),
            (Expr::Int(0), ty) if is_reference(ty) => Expr::Null,
            // 値型の定数（列挙型）: `(Color)1`。
            (Expr::Int(_) | Expr::Long(_), Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. }) => {
                Expr::Convert { to: ty.clone(), checked: false, value: value.boxed() }
            }
            // 列挙型の値を、整数の型の所へ渡す（`Write(sbyte)` に列挙型の値）: C# ではキャストが要る（IL では変換しない）。
            (_, Ty::I1 | Ty::U1 | Ty::I2 | Ty::U2 | Ty::I4 | Ty::U4 | Ty::I8 | Ty::U8) if !is_literal(&value) && self.is_value_type(&value) => {
                Expr::Convert { to: ty.clone(), checked: false, value: value.boxed() }
            }
            // 別の列挙型の値（IL では同じ整数）: キャストが要る。
            (_, Ty::Named { token, value_type: true }) if !is_literal(&value) && matches!(self.type_of(&value), Some(Ty::Named { token: other, value_type: true }) if other != *token) => {
                Expr::Convert { to: ty.clone(), checked: false, value: value.boxed() }
            }
            // 整数を列挙型の所へ（定数でない式）: キャストが要る。
            (_, Ty::Named { value_type: true, .. } | Ty::Generic { value_type: true, .. }) if !is_literal(&value) && self.type_of(&value).is_some_and(|t| is_numeric(&t)) => {
                Expr::Convert { to: ty.clone(), checked: false, value: value.boxed() }
            }
            // 数値の型どうしで、C# の暗黙の変換が無いもの（符号の違い、大きい型から小さい型）: IL は黙って切り詰めるので、
            // キャストを書く。
            (_, target) if is_numeric(target) && !is_literal(&value) && self.type_of(&value).is_some_and(|from| is_numeric(&from) && !implicitly_converts(&from, target)) => {
                Expr::Convert { to: ty.clone(), checked: false, value: value.boxed() }
            }
            // bool の式を整数として使う所（比べた結果を int に入れる）は、そのまま（C# では書けないが、まれ）。
            _ => value,
        }
    }

    /// 条件の否定。整数か参照どうしの大小は、逆の比較にする（浮動小数点数は NaN があるので `!` のまま）。
    pub(super) fn negate(&self, condition: Expr) -> Expr {
        match condition {
            Expr::Compare(op, a, b) if self.is_integer(&a) || self.is_integer(&b) => Expr::Compare(op.negate(), a, b),
            other => negate(other),
        }
    }

    /// 一時変数の型が決まったあとで、一時変数に入れた定数をその型に直す（`s = 0` を `s = false` に）。最初に入れたのが
    /// 定数だったために型が決まっていなければ、あとで入れた式の型にする。
    pub(super) fn coerce_temps(&mut self, body: &mut [Stmt]) {
        // 一時変数から一時変数へ写す所（`s13 = sd`）は、写し元の型が決まってから決まるので、変わらなくなるまで繰り返す。
        for _ in 0..4 {
            let mut assigned: Vec<(u32, Option<Ty>)> = Vec::new();
            visit_assignments(body, &mut |temp, value| assigned.push((temp, self.type_of(value).filter(|_| !is_literal(value)))));
            let mut changed = false;
            for (temp, ty) in assigned {
                if let (Some(entry), Some(ty)) = (self.temps.iter_mut().find(|(t, _)| *t == temp), ty) {
                    if (entry.1.is_none() || entry.1 == Some(Ty::I4)) && entry.1.as_ref() != Some(&ty) {
                        entry.1 = Some(ty);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        self.type_temps_by_use(body);
        let temps = self.temps.clone();
        rewrite_assignments(body, &mut |temp, value| {
            if let Some(Some(ty)) = temps.iter().find(|(t, _)| *t == temp).map(|(_, ty)| ty.clone()) {
                *value = self.coerce(value.clone(), &ty);
            }
        });
        // 型が決まる前に入れた定数（`t[0] = 35` の t が char[] と分かった）を、入れる先の型に直す。
        super::super::ast::visit_mut(body, &mut |s| {
            if let StmtKind::Assign(target, value) = &mut s.kind {
                if is_literal(value) && matches!(target, Expr::Element { array, .. } if matches!(**array, Expr::Temp(_))) {
                    if let Some(ty) = self.type_of(target) {
                        *value = self.coerce(value.clone(), &ty);
                    }
                }
            }
        });
        // 型が決まる前に作った条件（`t != 0`）は、bool の一時変数なら `t` / `!t`、参照なら `t != null`。
        let type_of_temp = |e: &Expr| match e {
            Expr::Temp(t) => temps.iter().find(|(temp, _)| temp == t).and_then(|(_, ty)| ty.clone()),
            _ => None,
        };
        visit_expressions_mut(body, &mut |e| {
            let replacement = match e {
                Expr::Compare(op @ (CmpOp::Eq | CmpOp::Ne), a, b) if **b == Expr::Int(0) => match type_of_temp(a) {
                    Some(Ty::Bool) => Some(if *op == CmpOp::Ne { (**a).clone() } else { Expr::Not(a.clone()) }),
                    Some(ty) if is_reference(&ty) => Some(Expr::Compare(*op, a.clone(), Expr::Null.boxed())),
                    _ => None,
                },
                _ => None,
            };
            if let Some(replacement) = replacement {
                *e = replacement;
            }
        });
    }

    /// 代入からは型が分からない一時変数（定数や `typeof` だけを入れたもの）に、使い道の型を付ける: 呼び出しの引数なら
    /// その引数の型、呼び出しの受け手なら、そのメソッドを宣言している型。
    fn type_temps_by_use(&mut self, body: &[Stmt]) {
        let unknown: Vec<u32> = self.temps.iter().filter(|(_, ty)| ty.is_none()).map(|(t, _)| *t).collect();
        if unknown.is_empty() {
            return;
        }
        let names = self.names();
        let mut found: Vec<(u32, Ty)> = Vec::new();
        visit_expressions(body, &mut |e| {
            let (method, target, args) = match e {
                Expr::Call { method, target, args, .. } | Expr::Property { method, target, args } => (*method, target.as_deref(), args.as_slice()),
                Expr::New { method, args } => (*method, None, args.as_slice()),
                _ => return,
            };
            if let Some(Expr::Temp(t)) = target {
                if let (true, Some(owner)) = (unknown.contains(t), names.method_owner(method)) {
                    found.push((*t, Ty::Named { token: owner, value_type: false }));
                }
            }
            let Some(sig) = names.method_sig(method) else { return };
            for (arg, ty) in args.iter().zip(sig.params) {
                if let (Expr::Temp(t), Some(ty)) = (arg, in_caller(names, method, ty)) {
                    if unknown.contains(t) && !matches!(ty, Ty::Object) {
                        found.push((*t, ty));
                    }
                }
            }
        });
        for (temp, ty) in found {
            if let Some(entry) = self.temps.iter_mut().find(|(t, ty)| *t == temp && ty.is_none()) {
                entry.1 = Some(ty);
            }
        }
    }

    /// 式の型（分かるものだけ）。
    pub fn type_of(&self, value: &Expr) -> Option<Ty> {
        let names = self.names();
        match value {
            Expr::Local(index) => self.method.locals.get(*index as usize).cloned(),
            Expr::Arg(index) => {
                let sig = self.method.signature;
                let index = *index as usize;
                if sig.has_this && !sig.explicit_this {
                    if index == 0 { self.method.this_type.clone() } else { sig.params.get(index - 1).cloned() }
                } else {
                    sig.params.get(index).cloned()
                }
            }
            Expr::Temp(temp) => self.temps.iter().find(|(t, _)| t == temp).and_then(|(_, ty)| ty.clone()),
            Expr::Int(_) => Some(Ty::I4),
            Expr::Long(_) => Some(Ty::I8),
            Expr::Float(_) => Some(Ty::R4),
            Expr::Double(_) => Some(Ty::R8),
            Expr::Str(_) => Some(Ty::String),
            Expr::Bool(_) | Expr::Compare(..) | Expr::Not(_) => Some(Ty::Bool),
            Expr::Char(_) => Some(Ty::Char),
            Expr::Convert { to, .. } => Some(to.clone()),
            Expr::Default(ty) => Some(ty.clone()),
            Expr::Field { token, .. } => field_type(names, *token).and_then(|t| in_caller(names, *token, t)),
            Expr::Property { method, .. } | Expr::Call { method, .. } => names.method_sig(*method).and_then(|s| in_caller(names, *method, s.ret)),
            Expr::Element { array, .. } => match self.type_of(array)? {
                Ty::SzArray(element) => Some(*element),
                _ => None,
            },
            Expr::Length(_) => Some(Ty::I4),
            Expr::Deref(inner) => match self.type_of(inner)? {
                Ty::ByRef(inner) | Ty::Pointer(inner) => Some(*inner),
                _ => None,
            },
            Expr::AddressOf(inner) => self.type_of(inner).map(|t| Ty::ByRef(Box::new(t))),
            // C# では、int より小さい整数（と char）の演算の結果は int。
            Expr::Binary(_, a, b) | Expr::Checked(_, a, b) => self.type_of(a).filter(|t| !matches!(t, Ty::Bool)).or_else(|| self.type_of(b)).map(promote),
            Expr::Neg(inner) | Expr::BitNot(inner) => self.type_of(inner).map(promote),
            Expr::Assign(target, _) => self.type_of(target),
            Expr::New { method, .. } => names.method_owner(*method).map(|token| Ty::Named { token, value_type: false }),
            Expr::NewDelegate { ty, .. } => names.method_owner(*ty).map(|token| Ty::Named { token, value_type: false }),
            Expr::NewArray { element, .. } | Expr::ArrayInit { element, .. } => Some(Ty::SzArray(Box::new(Ty::Named { token: *element, value_type: false }))),
            Expr::Cast { ty, .. } | Expr::As { ty, .. } => Some(Ty::Named { token: *ty, value_type: false }),
            _ => None,
        }
    }
}

/// フィールドのトークン（FieldDef、MemberRef）の型。
pub(super) fn field_type(names: &Names, token: u32) -> Option<Ty> {
    let row = token & 0x00FF_FFFF;
    match (token >> 24) as usize {
        FIELD => names.field_def_type(row),
        MEMBER_REF => super::super::sig::field(names.md.blob(names.md.cell(MEMBER_REF, row, 2)).ok()?).ok(),
        _ => None,
    }
}

/// 呼び出し先やフィールドのシグネチャの型を、呼び出す側の文脈の型にする（型引数を置き換える）。置き換えられない
/// 型引数が残れば `None`（型は分からない）。
pub(super) fn in_caller(names: &Names, token: u32, ty: Ty) -> Option<Ty> {
    match names.instantiation(token) {
        Some((types, methods)) => ty.substitute(&types, &methods),
        None => Some(ty),
    }
}

/// 参照型か（null を入れられる）。
pub(super) fn is_reference(ty: &Ty) -> bool {
    matches!(ty.unmodified(), Ty::String | Ty::Object | Ty::SzArray(_) | Ty::Array { .. } | Ty::Named { value_type: false, .. } | Ty::Generic { value_type: false, .. })
}

/// 式そのものが bool（比べた結果など）。
pub(super) fn is_boolean(value: &Expr) -> bool {
    matches!(value, Expr::Bool(_) | Expr::Compare(..) | Expr::Not(_))
}

pub(super) fn is_literal(value: &Expr) -> bool {
    matches!(value, Expr::Int(_) | Expr::Long(_))
}

/// 一時変数への代入（入れ子の中も）を、たどる。
pub(super) fn visit_assignments(body: &[Stmt], f: &mut impl FnMut(u32, &Expr)) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Assign(Expr::Temp(temp), value) => f(*temp, value),
            StmtKind::If { then, otherwise, .. } => {
                visit_assignments(then, f);
                visit_assignments(otherwise, f);
            }
            StmtKind::Try { body, catches, finally, fault } => {
                visit_assignments(body, f);
                for catch in catches {
                    visit_assignments(&catch.body, f);
                }
                for inner in finally.iter().chain(fault.iter()) {
                    visit_assignments(inner, f);
                }
            }
            _ => {}
        }
    }
}

/// 一時変数への代入の値を、書き換える。
pub(super) fn rewrite_assignments(body: &mut [Stmt], f: &mut impl FnMut(u32, &mut Expr)) {
    for stmt in body {
        match &mut stmt.kind {
            StmtKind::Assign(Expr::Temp(temp), value) => f(*temp, value),
            StmtKind::If { then, otherwise, .. } => {
                rewrite_assignments(then, f);
                rewrite_assignments(otherwise, f);
            }
            StmtKind::Try { body, catches, finally, fault } => {
                rewrite_assignments(body, f);
                for catch in catches {
                    rewrite_assignments(&mut catch.body, f);
                }
                for inner in finally.iter_mut().chain(fault.iter_mut()) {
                    rewrite_assignments(inner, f);
                }
            }
            _ => {}
        }
    }
}

/// 組み込みの数値の型か（char を含む）。
pub(super) fn is_numeric(ty: &Ty) -> bool {
    matches!(ty.unmodified(), Ty::I1 | Ty::U1 | Ty::I2 | Ty::U2 | Ty::I4 | Ty::U4 | Ty::I8 | Ty::U8 | Ty::R4 | Ty::R8 | Ty::Char)
}

/// C# の暗黙の数値変換があるか（同じ型を含む）。IL の評価スタックでは 32 ビットより小さい型は int なので、ここで
/// 判定するのは「宣言の型」どうし。
pub(super) fn implicitly_converts(from: &Ty, to: &Ty) -> bool {
    use Ty::*;
    let (from, to) = (from.unmodified(), to.unmodified());
    from == to
        || matches!(
            (from, to),
            (I1, I2 | I4 | I8 | R4 | R8)
                | (U1, I2 | U2 | I4 | U4 | I8 | U8 | R4 | R8)
                | (I2, I4 | I8 | R4 | R8)
                | (U2, I4 | U4 | I8 | U8 | R4 | R8)
                | (I4, I8 | R4 | R8)
                | (U4, I8 | U8 | R4 | R8)
                | (I8 | U8, R4 | R8)
                | (Char, U2 | I4 | U4 | I8 | U8 | R4 | R8)
                | (R4, R8)
        )
}

/// 演算の結果の型: int より小さい整数と char は int になる（C# の数値の昇格）。
fn promote(ty: Ty) -> Ty {
    match ty {
        Ty::I1 | Ty::U1 | Ty::I2 | Ty::U2 | Ty::Char => Ty::I4,
        other => other,
    }
}