//! 型を周りから決める式: 型を省いた `new(…)` と、コレクション式（`[1, 2, ..rest]`）。型は、渡す先の引数の型、代入先の型、
//! キャストの型で決まる。それ以外の場所では、C# と同じくエラーにする（自分では型を持たない）。

use super::*;
use crate::debugger::session::eval::syntax::Initializer;

impl Session {
    /// 型の決まった場所で、`new(…)` かコレクション式を作る。型を周りから決める式でなければ `None`。
    pub(super) fn build_targeted(&mut self, thread: u32, depth: usize, expr: &Expr, target: &TypeSyntax) -> Option<Result<ComPtr<ICorDebugValue>, String>> {
        Some(match expr {
            Expr::TargetNew { arguments, initializer } => self.create(thread, depth, target, arguments, initializer),
            Expr::Collection { arguments, items } => self.create_collection(thread, depth, target, arguments, items),
            Expr::DefaultLiteral => {
                let ty = match self.runtime_type(thread, depth, target) {
                    Ok(ty) => ty,
                    Err(error) => return Some(Err(error)),
                };
                self.default_of_type(thread, &ty)
            }
            _ => return None,
        })
    }

    /// 対象の中の型を、式に書く型の形にする。
    pub(super) fn type_syntax_of(&self, ty: &ComPtr<ICorDebugType>) -> Option<TypeSyntax> {
        syntax::parse_type(&unsafe { value::type_name(ty, &self.modules) })
    }

    /// 型の名前が、止まっているメソッドか、その型の型引数（`T`）なら、その呼び出しでの実際の型（`int`）にする。
    /// 配列、`?`、ポインターの印は、書かれたまま付け直す（`T[]` は `int[]`）。型引数でなければ `None`。
    pub(super) fn type_parameter(&self, thread: u32, depth: usize, ty: &TypeSyntax) -> Option<TypeSyntax> {
        if ty.name.contains('.') || !ty.arguments.is_empty() {
            return None;
        }
        let frames = self.walk_frames(thread).ok()?;
        let frame = frames.get(depth)?;
        let mut token = 0u32;
        unsafe { frame.GetFunctionToken(&mut token) };
        let index = unsafe { self.frame_module(frame) }?.metadata()?.type_parameter_names(token & 0x00FF_FFFF).iter().position(|name| *name == ty.name)?;
        let frame2 = frame.cast::<ICorDebugILFrame2>().ok()?;
        let types = unsafe { take::<ICorDebugTypeEnum>("ICorDebugILFrame2::EnumerateTypeParameters", |out| frame2.EnumerateTypeParameters(out)) }.ok()?;
        let mut actual = None;
        for _ in 0..=index {
            let (mut raw, mut fetched) = (null_mut(), 0u32);
            if !unsafe { types.Next(1, &mut raw, &mut fetched) }.is_ok() || fetched == 0 {
                return None;
            }
            actual = unsafe { ComPtr::<ICorDebugType>::from_raw(raw) };
        }
        let actual = self.type_syntax_of(&actual?)?;
        // 型引数が配列なら、そのまま（`T` が `int[]`）。そうでなければ、書かれた印を付け直す。
        if !(ty.array || ty.nullable || ty.pointers > 0) {
            return Some(actual);
        }
        Some(TypeSyntax { name: actual.name, arguments: actual.arguments, ..ty.clone() })
    }

    /// 型の既定値（値型は中身がすべて 0、参照型は null）。
    fn default_of_type(&mut self, thread: u32, ty: &ComPtr<ICorDebugType>) -> Result<ComPtr<ICorDebugValue>, String> {
        let mut element = 0;
        unsafe { ty.GetType(&mut element) };
        const ELEMENT_TYPE_VALUETYPE: u32 = 0x11;
        match element {
            0x02..=0x0D | 0x18 | 0x19 => self.new_number(thread, element as u8, &vec![0u8; primitive_size(element as u8)]),
            ELEMENT_TYPE_VALUETYPE => self.new_object_of_type(thread, ty).map_err(|failure| failure.describe(self)),
            _ => self.new_null(thread),
        }
    }

    /// コレクション式を、決まった型の値として作る。
    ///   配列（`int[]`）: 要素を並べた配列。
    ///   `IEnumerable<T>` などの読み取り用のインターフェイス、`IList<T>` などの書き換えられるインターフェイス: `List<T>`
    ///   （C# のコンパイラは、読み取り用には専用の型を作ることがある。値は同じ）。
    ///   `Add` を持つ型（`List<T>`、`HashSet<T>` など）: その型を作って（`with(…)` があれば、その引数で）、要素を `Add` する。
    fn create_collection(&mut self, thread: u32, depth: usize, target: &TypeSyntax, arguments: &[Expr], items: &[(bool, Expr)]) -> Result<ComPtr<ICorDebugValue>, String> {
        // 要素を、`..` を広げながら集める。
        let mut values = Vec::new();
        for (spread, item) in items {
            let current = self.evaluate_node(thread, depth, item)?;
            let value = self.value_of(current)?;
            if *spread {
                let count = self.count_of(thread, &value)?;
                for position in 0..count {
                    values.push(self.element_at(thread, &value, position)?);
                }
            } else {
                values.push(value);
            }
        }
        if target.array {
            if !arguments.is_empty() {
                return Err(tr!("with(…) arguments cannot be written in an array collection expression."));
            }
            let element = target.element();
            let array = self.create_array_of(thread, depth, &element, values.len())?;
            for (index, value) in values.iter().enumerate() {
                let slot = unsafe { value::array_element(&array, &[index as u32]) }?.ok_or_else(|| tr!("Cannot read an element of the created array."))?;
                self.write(thread, &slot, value)?;
            }
            return Ok(array);
        }
        let interfaces = ["IEnumerable", "IReadOnlyCollection", "IReadOnlyList", "ICollection", "IList"];
        let simple = target.name.rsplit('.').next().unwrap_or(&target.name);
        let concrete = if interfaces.contains(&simple) && target.arguments.len() == 1 {
            TypeSyntax { name: "System.Collections.Generic.List".into(), ..target.clone() }
        } else {
            target.clone()
        };
        let collection = self.create(thread, depth, &concrete, arguments, &Initializer::None)?;
        for value in values {
            self.call_method(thread, depth, Some(Current::Value(collection.clone())), "Add", vec![value])?;
        }
        Ok(collection)
    }

    /// 決まった大きさの、一次元の配列。
    fn create_array_of(&mut self, thread: u32, depth: usize, element: &TypeSyntax, length: usize) -> Result<ComPtr<ICorDebugValue>, String> {
        let size = Expr::Int(length as i32);
        self.create_array(thread, depth, Some(element), std::slice::from_ref(&size), None)
    }
}
