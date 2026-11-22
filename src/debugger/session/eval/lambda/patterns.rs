//! ラムダ式の中のパターン（`x is …`、`switch` 式の腕、`switch` 文の `case`）を、bool の節にする。
//!
//! パターンは、節の型（コンパイル時の型）を見て、C# のコンパイラが作るのと同じ検査に直す。
//! - 型の検査は `TypeIs`。変数を作るパターン（`int k`）は、検査のあとに、その型へ変換した値を変数へ入れる。
//! - 定数と比較は、値型なら演算子。null 許容の値型なら「値があり、かつ中身が合う」。参照型（`object` に入った数など）
//!   なら「定数と同じ型の値で、かつ値が合う」。
//! - パターンが作った変数と、値を一度だけ実行するための置き場所は、ラムダ式の本体の外側のブロックで宣言する
//!   （`LambdaFlow::locals`）。

use super::*;
use crate::debugger::session::eval::syntax::{Arm, Pattern};

type Node = ComPtr<ICorDebugValue>;

/// 節の型が、null になれるか。
#[derive(PartialEq)]
enum Nullness {
    /// 参照型。
    Reference,
    /// null 許容の値型（`int?`）。
    NullableValue,
    /// null にならない値型。
    Value,
}

/// switch 式の腕の結果。
enum ArmResult {
    /// 値の節。型を書かない `default` は `None`。
    Value(Option<Node>),
    /// `throw 式` の、例外の節。式としての型は、ほかの腕から決まる。
    Thrown(Node),
}

impl Session {
    fn nullness(&mut self, thread: u32, node: &Node) -> Result<Nullness, String> {
        let name = self.node_type(thread, node)?;
        if name.starts_with("System.Nullable`1[") {
            return Ok(Nullness::NullableValue);
        }
        if let Some(builtin) = crate::debugger::builtin::by_full_name(&name) {
            let is_value = builtin.is_primitive() || builtin.keyword == "decimal";
            return Ok(if is_value { Nullness::Value } else { Nullness::Reference });
        }
        let ty = self.member(thread, node, "Type")?;
        let is_value = self.member(thread, &ty, "IsValueType")?;
        Ok(if self.operand(&is_value) == Operand::Bool(true) { Nullness::Value } else { Nullness::Reference })
    }

    /// 何度読んでも、何も実行されない節（変数、定数）か。
    pub(super) fn rereadable(&self, node: &Node) -> bool {
        unsafe { value::type_names(node, &self.modules) }.iter().any(|name| name == PARAMETER || name == "System.Linq.Expressions.ConstantExpression")
    }

    /// パターンが、値を一度しか読まないか。
    fn reads_once(&self, thread: u32, depth: usize, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Null | Pattern::Discard | Pattern::Var(_) => true,
            Pattern::Not(inner) => self.reads_once(thread, depth, inner),
            Pattern::TypeOrConstant(Some(ty), _) => self.resolve_type(thread, depth, ty).is_ok(),
            _ => false,
        }
    }

    /// 式の途中で使う変数を作る。宣言は、ラムダ式の本体の外側のブロックがする。名前を渡せば、あとの式から名前で読める。
    pub(super) fn lambda_local(&mut self, thread: u32, depth: usize, ty: Node, name: Option<&str>) -> Result<Node, String> {
        let variable = self.variable(thread, depth, ty, name.unwrap_or("held"))?;
        let flow = self.flow()?;
        flow.locals.push(variable.clone());
        if let Some(name) = name {
            flow.named.push((name.to_string(), variable.clone()));
        }
        Ok(variable)
    }

    /// 値を新しい変数に入れる。戻り値は、代入の節と、変数。
    pub(super) fn stored(&mut self, thread: u32, depth: usize, value: Node, name: Option<&str>) -> Result<(Node, Node), String> {
        let ty = self.member(thread, &value, "Type")?;
        let variable = self.lambda_local(thread, depth, ty, name)?;
        Ok((self.factory(thread, depth, "Assign", vec![variable.clone(), value])?, variable))
    }

    /// 値を新しい変数に入れる、bool の節（いつも true。検査の並びに `&&` でつなげる）と、その変数。
    fn held(&mut self, thread: u32, depth: usize, value: Node, name: Option<&str>) -> Result<(Node, Node), String> {
        let (assign, variable) = self.stored(thread, depth, value, name)?;
        let done = self.constant_node(thread, depth, &Expr::Bool(true))?;
        Ok((self.block_of(thread, depth, &[], &[assign, done])?, variable))
    }

    /// 検査の並びを `&&` でつなぐ。一つも無ければ、true。
    fn all_of(&mut self, thread: u32, depth: usize, tests: Vec<Node>) -> Result<Node, String> {
        let mut tests = tests.into_iter();
        let Some(mut joined) = tests.next() else { return self.constant_node(thread, depth, &Expr::Bool(true)) };
        for test in tests {
            joined = self.factory(thread, depth, "AndAlso", vec![joined, test])?;
        }
        Ok(joined)
    }

    /// `値 is パターン` の節。値を二度以上読むパターンでは、先に値を変数へ入れる（値の式を、一度だけ実行するように）。
    pub(super) fn pattern_on(&mut self, thread: u32, depth: usize, value: Node, pattern: &Pattern) -> Result<Node, String> {
        if self.rereadable(&value) || self.reads_once(thread, depth, pattern) {
            return self.pattern_node(thread, depth, &value, pattern);
        }
        let (hold, input) = self.held(thread, depth, value, None)?;
        let test = self.pattern_node(thread, depth, &input, pattern)?;
        self.factory(thread, depth, "AndAlso", vec![hold, test])
    }

    /// パターンを、`input` の値を確かめる bool の節にする。`input` は、何度読んでもよい節。
    pub(super) fn pattern_node(&mut self, thread: u32, depth: usize, input: &Node, pattern: &Pattern) -> Result<Node, String> {
        match pattern {
            Pattern::Discard => self.constant_node(thread, depth, &Expr::Bool(true)),
            Pattern::Null => self.null_test(thread, depth, input),
            Pattern::Not(inner) => {
                let inner = self.pattern_node(thread, depth, input, inner)?;
                self.factory(thread, depth, "Not", vec![inner])
            }
            Pattern::And(left, right) | Pattern::Or(left, right) => {
                let (left, right) = (self.pattern_node(thread, depth, input, left)?, self.pattern_node(thread, depth, input, right)?);
                let name = if matches!(pattern, Pattern::And(..)) { "AndAlso" } else { "OrElse" };
                self.factory(thread, depth, name, vec![left, right])
            }
            Pattern::Var(name) => Ok(self.held(thread, depth, input.clone(), Some(name))?.0),
            Pattern::Declaration(ty, name) => {
                let target = self.type_object(thread, depth, &type_syntax_text(ty))?;
                Ok(self.typed_binding(thread, depth, input, target, Some(name))?.0)
            }
            Pattern::TypeOrConstant(ty, constant) => {
                // 型の名前として解決できれば、型の検査。できなければ、定数（`x is Shade.Dark`）。
                if let Some(ty) = ty.as_ref().filter(|ty| self.resolve_type(thread, depth, ty).is_ok()) {
                    let target = self.type_object(thread, depth, &type_syntax_text(ty))?;
                    return self.factory(thread, depth, "TypeIs", vec![input.clone(), target]);
                }
                self.constant_test(thread, depth, input, BinaryOp::Equal, constant)
            }
            Pattern::Relational(op, bound) => self.constant_test(thread, depth, input, *op, bound),
            Pattern::Recursive { ty, positional, members, designation } => self.recursive_pattern(thread, depth, input, ty.as_ref(), positional.as_deref(), members, designation.as_deref()),
            Pattern::List { items, slice, designation } => self.list_pattern(thread, depth, input, items, *slice, designation.as_deref()),
        }
    }

    /// `is null` の節。
    fn null_test(&mut self, thread: u32, depth: usize, input: &Node) -> Result<Node, String> {
        match self.nullness(thread, input)? {
            Nullness::Reference => {
                let nothing = self.constant_node(thread, depth, &Expr::Null)?;
                self.factory(thread, depth, "ReferenceEqual", vec![input.clone(), nothing])
            }
            Nullness::NullableValue => {
                let has_value = self.member_node(thread, depth, input, "HasValue")?;
                self.factory(thread, depth, "Not", vec![has_value])
            }
            Nullness::Value => Err(tr!("A pattern comparing a value of a non-nullable type with null cannot be written.")),
        }
    }

    /// 型の検査と、合ったときにその型へ変換した値を変数へ入れる節。戻り値は、検査の節と、変数。
    fn typed_binding(&mut self, thread: u32, depth: usize, input: &Node, target: Node, name: Option<&str>) -> Result<(Node, Node), String> {
        let fits = self.factory(thread, depth, "TypeIs", vec![input.clone(), target.clone()])?;
        let converted = self.factory(thread, depth, "Convert", vec![input.clone(), target])?;
        let (hold, variable) = self.held(thread, depth, converted, name)?;
        Ok((self.factory(thread, depth, "AndAlso", vec![fits, hold])?, variable))
    }

    /// 定数のパターン（`is 5`。`op` は `==`）と、比較のパターン（`is > 5`）。
    fn constant_test(&mut self, thread: u32, depth: usize, input: &Node, op: BinaryOp, constant: &Expr) -> Result<Node, String> {
        match self.nullness(thread, input)? {
            Nullness::Value => {
                let constant = self.constant_node(thread, depth, constant)?;
                self.binary_of(thread, depth, op, input.clone(), constant)
            }
            // 値があり、かつ中身が合う。
            Nullness::NullableValue => {
                let has_value = self.member_node(thread, depth, input, "HasValue")?;
                let inner = self.member_node(thread, depth, input, "Value")?;
                let test = self.constant_test(thread, depth, &inner, op, constant)?;
                self.factory(thread, depth, "AndAlso", vec![has_value, test])
            }
            // `object` に入った値は、実行時の型が定数の型と同じときだけ合う（`o is 42` は、long の 42 には合わない）。
            Nullness::Reference => {
                let constant = self.constant_node(thread, depth, constant)?;
                if op == BinaryOp::Equal {
                    // 型も値も同じなら true になる object.Equals(object, object) に任せる（null も扱う）。
                    let (left, right) = (self.convert_node(thread, depth, input.clone(), "object")?, self.convert_node(thread, depth, constant, "object")?);
                    let object = self.type_object(thread, depth, "object")?;
                    return self.call_node(thread, depth, None, Some(object), "Equals", vec![left, right]);
                }
                let ty = self.member(thread, &constant, "Type")?;
                let fits = self.factory(thread, depth, "TypeIs", vec![input.clone(), ty.clone()])?;
                let unboxed = self.factory(thread, depth, "Convert", vec![input.clone(), ty])?;
                let test = self.binary_of(thread, depth, op, unboxed, constant)?;
                self.factory(thread, depth, "AndAlso", vec![fits, test])
            }
        }
    }

    /// null でないことを確かめる節を `tests` に足し、中身（null 許容の値型なら、その値）の節を返す。
    fn present(&mut self, thread: u32, depth: usize, value: &Node, tests: &mut Vec<Node>) -> Result<Node, String> {
        match self.nullness(thread, value)? {
            Nullness::Value => Ok(value.clone()),
            Nullness::Reference => {
                let nothing = self.constant_node(thread, depth, &Expr::Null)?;
                tests.push(self.factory(thread, depth, "ReferenceNotEqual", vec![value.clone(), nothing])?);
                Ok(value.clone())
            }
            Nullness::NullableValue => {
                tests.push(self.member_node(thread, depth, value, "HasValue")?);
                self.member_node(thread, depth, value, "Value")
            }
        }
    }

    /// `型 (位置のパターン) { メンバー: パターン } 変数`。
    #[allow(clippy::too_many_arguments)]
    fn recursive_pattern(&mut self, thread: u32, depth: usize, input: &Node, ty: Option<&TypeSyntax>, positional: Option<&[Pattern]>, members: &[(Vec<String>, Pattern)], designation: Option<&str>) -> Result<Node, String> {
        let mut tests = Vec::new();
        // 型が書かれていれば、その型の値として中を見る。無ければ、null でないことだけを確かめる。
        let subject = match ty {
            Some(ty) => {
                let target = self.type_object(thread, depth, &type_syntax_text(ty))?;
                let (test, variable) = self.typed_binding(thread, depth, input, target, designation)?;
                tests.push(test);
                variable
            }
            None => {
                let subject = self.present(thread, depth, input, &mut tests)?;
                match designation {
                    Some(name) => {
                        let (hold, variable) = self.held(thread, depth, subject, Some(name))?;
                        tests.push(hold);
                        variable
                    }
                    None => subject,
                }
            }
        };
        // 位置のパターン: タプルは ItemN、そのほかの型は Deconstruct(out …) の結果（呼び出しは、検査の並びに true として足す）。
        if let Some(items) = positional {
            let mut steps = Vec::new();
            let parts = self.deconstructed_parts(thread, depth, &subject, items.len(), &mut steps)?;
            if !steps.is_empty() {
                steps.push(self.constant_node(thread, depth, &Expr::Bool(true))?);
                tests.push(self.block_of(thread, depth, &[], &steps)?);
            }
            for (part, item) in parts.into_iter().zip(items) {
                tests.push(self.pattern_on(thread, depth, part, item)?);
            }
        }
        for (path, sub) in members {
            let mut current = subject.clone();
            for (step, name) in path.iter().enumerate() {
                // 入れ子のメンバー（`A.B`）は、途中が null なら合わない。
                if step > 0 {
                    let (hold, variable) = self.held(thread, depth, current, None)?;
                    tests.push(hold);
                    current = self.present(thread, depth, &variable, &mut tests)?;
                }
                current = self.member_node(thread, depth, &current, name)?;
            }
            tests.push(self.pattern_on(thread, depth, current, sub)?);
        }
        self.all_of(thread, depth, tests)
    }

    /// `[1, .., 3]`。`slice` は、`..` の位置。
    fn list_pattern(&mut self, thread: u32, depth: usize, input: &Node, items: &[Pattern], slice: Option<usize>, designation: Option<&str>) -> Result<Node, String> {
        let mut tests = Vec::new();
        let subject = self.present(thread, depth, input, &mut tests)?;
        let count = self.count_node(thread, depth, &subject)?;
        let (hold, count) = self.held(thread, depth, count, None)?;
        tests.push(hold);
        let fixed = self.constant_node(thread, depth, &Expr::Int(items.len() as i32))?;
        let comparison = if slice.is_some() { "GreaterThanOrEqual" } else { "Equal" };
        tests.push(self.factory(thread, depth, comparison, vec![count.clone(), fixed])?);
        for (index, item) in items.iter().enumerate() {
            // `..` よりあとの要素は、終わりから数える。
            let position = match slice {
                Some(at) if index >= at => {
                    let back = self.constant_node(thread, depth, &Expr::Int((items.len() - index) as i32))?;
                    self.factory(thread, depth, "Subtract", vec![count.clone(), back])?
                }
                _ => self.constant_node(thread, depth, &Expr::Int(index as i32))?,
            };
            let element = self.element_node(thread, depth, subject.clone(), vec![position])?;
            tests.push(self.pattern_on(thread, depth, element, item)?);
        }
        if let Some(name) = designation {
            tests.push(self.held(thread, depth, subject, Some(name))?.0);
        }
        self.all_of(thread, depth, tests)
    }

    /// 配列やコレクションの要素の数の節（配列は長さ、ほかは Length か Count のプロパティ）。
    pub(super) fn count_node(&mut self, thread: u32, depth: usize, target: &Node) -> Result<Node, String> {
        if self.node_type(thread, target)?.ends_with("[]") {
            return self.factory(thread, depth, "ArrayLength", vec![target.clone()]);
        }
        match self.member_node(thread, depth, target, "Length") {
            Ok(length) => Ok(length),
            Err(_) => self.member_node(thread, depth, target, "Count").map_err(|_| tr!("This value has no element count (Length or Count).")),
        }
    }

    /// `値 switch { パターン when 条件 => 結果, … }`。上の腕から順に確かめる、条件演算子の入れ子にする。
    pub(super) fn switch_expression_node(&mut self, thread: u32, depth: usize, subject: &Expr, arms: &[Arm], scope: &[(String, Node)]) -> Result<Node, String> {
        let value = self.tree(thread, depth, subject, scope)?;
        let mut steps = Vec::new();
        let input = if self.rereadable(&value) {
            value
        } else {
            let (assign, variable) = self.stored(thread, depth, value, None)?;
            steps.push(assign);
            variable
        };
        // 腕ごとに、検査と結果を、書かれた順に組み立てる（パターンの変数は、その腕の条件と結果から見える）。
        let (mut tests, mut results) = (Vec::new(), Vec::new());
        for arm in arms {
            let mut test = self.pattern_node(thread, depth, &input, &arm.pattern)?;
            if let Some(guard) = &arm.guard {
                let guard = self.tree(thread, depth, guard, scope)?;
                test = self.factory(thread, depth, "AndAlso", vec![test, guard])?;
            }
            tests.push(test);
            results.push(match &arm.result {
                Expr::Throw(operand) => ArmResult::Thrown(self.tree(thread, depth, operand, scope)?),
                result => ArmResult::Value(self.branch_node(thread, depth, result, scope)?),
            });
        }
        // 結果の型は、値を返す腕のどれもが受け取れる型。`throw` の腕は、その型の式になる。
        let sources: Vec<&Expr> = arms.iter().map(|arm| &arm.result).filter(|result| !matches!(result, Expr::Throw(_))).collect();
        let values: Vec<Option<Node>> = results.iter().filter_map(|result| if let ArmResult::Value(node) = result { Some(node.clone()) } else { None }).collect();
        let mut values = self.unify_branches(thread, depth, values, &sources)?.into_iter();
        let Some(first) = values.next() else { return Err(tr!("Every arm of the switch expression is throw (the result type cannot be determined).")) };
        let ty = self.member(thread, &first, "Type")?;
        let mut values = std::iter::once(first).chain(values);
        let mut built = Vec::new();
        for result in results {
            built.push(match result {
                ArmResult::Value(_) => values.next().ok_or_else(|| tr!("Cannot build the arms of the switch expression."))?,
                ArmResult::Thrown(exception) => self.factory(thread, depth, "Throw", vec![exception, ty.clone()])?,
            });
        }
        // 最後の腕が `_` なら、それが残り全部の結果。そうでなければ、どの腕にも合わなかったときに、C# と同じ例外を投げる。
        let closed = arms.last().is_some_and(|arm| arm.pattern == Pattern::Discard && arm.guard.is_none());
        let mut chain = if closed { built.pop().ok_or_else(|| tr!("The switch expression has no arms."))? } else { self.unmatched_node(thread, depth, ty)? };
        if closed {
            tests.pop();
        }
        for (test, result) in tests.into_iter().zip(built).rev() {
            chain = self.factory(thread, depth, "Condition", vec![test, result, chain])?;
        }
        if steps.is_empty() {
            return Ok(chain);
        }
        steps.push(chain);
        self.block_of(thread, depth, &[], &steps)
    }

    /// switch 式のどの腕にも合わなかったときの `throw`。`ty` は、switch 式の型。
    fn unmatched_node(&mut self, thread: u32, depth: usize, ty: Node) -> Result<Node, String> {
        // .NET Core 3.0 以降は SwitchExpressionException。無いランタイム（.NET Framework）では、C# のコンパイラも
        // InvalidOperationException を投げるコードを作る。
        let exception = match self.type_object(thread, depth, "System.Runtime.CompilerServices.SwitchExpressionException") {
            Ok(found) => found,
            Err(_) => self.type_object(thread, depth, "System.InvalidOperationException")?,
        };
        let created = self.factory(thread, depth, "New", vec![exception])?;
        self.factory(thread, depth, "Throw", vec![created, ty])
    }
}
