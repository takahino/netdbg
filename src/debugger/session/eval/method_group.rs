//! メソッドの集まり（`Doubled`、`string.IsNullOrEmpty`、`Echo<int>`）を、デリゲートを受け取る場所に書いたもの。
//!
//! 渡す先のデリゲートの引数の数だけ引数を取るラムダ式（`(a0, a1) => Doubled(a0, a1)`）に直して、ラムダ式と同じ道で
//! デリゲートにする。どのオーバーロードを呼ぶかは、ラムダ式の本体の呼び出しが、デリゲートの引数の型で選ぶので、C# が
//! メソッドの集まりを変換するときと同じものになる（`Array.ConvertAll(numbers, Convert.ToString)` は `ToString(int)`）。

use super::*;
use crate::debugger::session::eval::syntax::LambdaParameters;

/// メソッドの集まりを、`count` 個の引数をそのまま渡すラムダ式の、引数の並びと本体にする。
pub(super) fn as_lambda(method: &Expr, count: usize) -> (LambdaParameters, Expr) {
    let names: Vec<String> = (0..count).map(|index| format!("__group{index}")).collect();
    let parameters = names.iter().map(|name| (None, name.clone())).collect();
    (parameters, Expr::Call(Box::new(method.clone()), names.into_iter().map(Expr::Name).collect()))
}

/// メソッドの集まりを、`count` 個の引数を取るラムダ式にする。
pub(super) fn lambda_of(method: &Expr, count: usize) -> Expr {
    let (parameters, body) = as_lambda(method, count);
    Expr::Lambda(parameters, Box::new(body))
}

impl Session {
    /// 式が、値ではなく、メソッドの集まりを指すか（値として評価できなかった式について聞く）。
    pub(super) fn is_method_group(&mut self, thread: u32, depth: usize, expr: &Expr) -> bool {
        match expr {
            Expr::Generic(inner, _) => self.is_method_group(thread, depth, inner),
            // 名前だけ: いまのメソッドの型か、それを囲む型のメソッド。
            Expr::Name(name) => self.frame_types(thread, depth).into_iter().any(|(module, row)| self.declares_method(module, row, name)),
            Expr::Member(target, name) => match self.evaluate_node(thread, depth, target) {
                Ok(Current::Type { module, row } | Current::Generic { module, row, .. }) => self.declares_method(module, row, name),
                Ok(Current::Value(value) | Current::Nullable(value)) => self.class_row(&value).is_some_and(|(module, row)| self.declares_method(module, row, name)),
                _ => false,
            },
            _ => false,
        }
    }

    /// 型（か、同じモジュールの基底の型）が、その名前のメソッドを宣言しているか。
    fn declares_method(&self, module: usize, mut row: u32, name: &str) -> bool {
        let Some(metadata) = self.modules[module].metadata() else { return false };
        for _ in 0..16 {
            if !metadata.methods_named(row, name).is_empty() {
                return true;
            }
            match metadata.base_type(row) {
                Some(base) => row = base,
                None => return false,
            }
        }
        false
    }

    /// デリゲートの型 `ty`（表示の書き方）へ、メソッドの集まりを変換するラムダ式。デリゲートの型でなければ `None`。
    pub(super) fn group_lambda_for(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, method: &Expr) -> Option<Expr> {
        let runtime = self.runtime_type(thread, depth, ty).ok()?;
        let display = unsafe { value::type_name(&runtime, &self.modules) };
        let parsed = syntax::parse_type(&display)?;
        let shape = crate::debugger::metadata::TypeShape { name: parsed.name.clone(), arguments: parsed.arguments.iter().map(lambda::type_syntax_text).collect() };
        let (inputs, _) = self.delegate_signature_of(thread, depth, &shape)?;
        Some(lambda_of(method, inputs.len()))
    }
}
