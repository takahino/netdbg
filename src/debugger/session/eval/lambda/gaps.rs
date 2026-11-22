//! ラムダ式の本体のうち、式木に組み立てられない書き方を、構文木だけを見て数える。実際のコードの式の測定
//! （`tests/corpus/measure.ps1` → 単体試験の `corpus_report`）が、読めた式のうち、評価で断る書き方を見つけるのに使う。
//!
//! 組み立ての `tree` と同じ判断をする（対象が無いので、型で決まる所は見ない）。`tree` の腕を足したら、ここも合わせる。

use super::*;
use crate::debugger::session::eval::syntax::{Initializer, OutTarget, Piece};

/// 式の中のラムダ式の本体にある、組み立てられない書き方の名前（同じものは 1 つにまとめる）。
pub(in crate::debugger::session) fn lambda_gaps(expr: &Expr) -> Vec<&'static str> {
    let mut gaps = Vec::new();
    expr.any(&mut |inner| {
        if let Expr::Lambda(parameters, body) = inner {
            let names: Vec<String> = parameters.iter().map(|(_, name)| name.clone()).collect();
            let body = match &**body {
                Expr::Async(inner) => &**inner,
                other => other,
            };
            check_targeted(body, &names, &mut gaps);
        }
        false
    });
    gaps.sort_unstable();
    gaps.dedup();
    gaps
}

fn check_all<'a>(exprs: impl IntoIterator<Item = &'a Expr>, names: &[String], gaps: &mut Vec<&'static str>) {
    for expr in exprs {
        check(expr, names, gaps);
    }
}

/// 呼び出しと new の引数（ラムダ式、名前つき引数、`out` / `ref`、型を渡す先から決める式を書ける）。
fn check_arguments(arguments: &[Expr], names: &[String], gaps: &mut Vec<&'static str>) {
    for argument in arguments {
        match argument {
            Expr::Named(_, value) => check_targeted(value, names, gaps),
            Expr::Out(OutTarget::Existing(place)) => assignable(place, names, gaps),
            Expr::Out(_) => {}
            other => check_targeted(other, names, gaps),
        }
    }
}

/// 型を渡す先（仮引数、代入先、宣言した変数）から決められる場所の式: ラムダ式、`new()`、`[…]`、`default`。
fn check_targeted(expr: &Expr, names: &[String], gaps: &mut Vec<&'static str>) {
    match expr {
        Expr::DefaultLiteral => {}
        Expr::TargetNew { arguments, initializer } => {
            check_arguments(arguments, names, gaps);
            check_initializer(initializer, names, gaps);
        }
        Expr::Collection { arguments, items } => {
            check_arguments(arguments, names, gaps);
            check_all(items.iter().map(|(_, item)| item), names, gaps);
        }
        // 内側のラムダ式の本体は、外側の引数も使える。
        Expr::Lambda(parameters, body) => {
            let mut inner = names.to_vec();
            inner.extend(parameters.iter().map(|(_, name)| name.clone()));
            let body = match &**body {
                Expr::Async(body) => &**body,
                other => other,
            };
            // 本体の `new(…)` / `[…]` / `default` とラムダ式は、戻り値の型で作る。
            check_targeted(body, &inner, gaps);
        }
        other => check(other, names, gaps),
    }
}

/// 条件演算子と switch 式の枝（型を書かない default と throw を書ける）。
fn check_branch(expr: &Expr, names: &[String], gaps: &mut Vec<&'static str>) {
    match expr {
        Expr::DefaultLiteral => {}
        Expr::Throw(operand) => check(operand, names, gaps),
        other => check(other, names, gaps),
    }
}

fn check(expr: &Expr, names: &[String], gaps: &mut Vec<&'static str>) {
    let mentioned = expr.mentions(names) || expr.any(&mut |inner| *inner == Expr::Receiver);
    if !mentioned && repeatable(expr) && !expr.declares() {
        return;
    }
    match expr {
        Expr::Lambda(..) => check_targeted(expr, names, gaps),
        Expr::Block(statements) => {
            let mut names = names.to_vec();
            declared_names(statements, &mut names);
            for statement in statements {
                check_statement(statement, &names, gaps);
            }
        }
        Expr::Throw(operand) | Expr::Await(operand) => check(operand, names, gaps),
        Expr::Tuple(items) => check_all(items, names, gaps),
        Expr::Assign(target, _, value) => {
            assignable(target, names, gaps);
            if !matches!(**value, Expr::Generic(..)) {
                check_targeted(value, names, gaps);
            }
        }
        Expr::Checked(_, inner) => check(inner, names, gaps),
        Expr::With(source, members) => {
            check(source, names, gaps);
            check_all(members.iter().map(|(_, value)| value), names, gaps);
        }
        Expr::Deconstruct(targets, value) => {
            check_all(targets.iter().filter_map(|target| if let OutTarget::Existing(place) = target { Some(&**place) } else { None }), names, gaps);
            check(value, names, gaps);
        }
        Expr::Increment { target, .. } => assignable(target, names, gaps),
        Expr::Name(_) | Expr::Receiver => {}
        Expr::Member(target, _) => check(target, names, gaps),
        Expr::Index(target, indices) => {
            check(target, names, gaps);
            for index in indices {
                match index {
                    Expr::FromEnd(inner) => check(inner, names, gaps),
                    Expr::Range(start, end) => check_all(start.iter().chain(end.iter()).map(|bound| match &**bound {
                        Expr::FromEnd(inner) => &**inner,
                        other => other,
                    }), names, gaps),
                    other => check(other, names, gaps),
                }
            }
        }
        Expr::Call(method, arguments) => {
            let method = match &**method {
                Expr::Generic(inner, _) => &**inner,
                other => other,
            };
            let invoked = match method {
                Expr::Member(target, _) => {
                    check(target, names, gaps);
                    false
                }
                Expr::Name(name) => names.contains(name),
                other if other.mentions(names) => {
                    check(other, names, gaps);
                    true
                }
                _ => false,
            };
            if invoked && arguments.iter().any(|argument| matches!(argument, Expr::Lambda(..))) {
                gaps.push(tr_str!("lambdas passed to a call of a delegate value"));
            } else {
                check_arguments(arguments, names, gaps);
            }
        }
        Expr::Unary(_, operand) | Expr::Cast(_, operand) => check(operand, names, gaps),
        // 型を書かない `default` は、もう一方の辺の型（`n == default`）。
        Expr::Binary(_, left, right) => {
            check_branch(left, names, gaps);
            check_branch(right, names, gaps);
        }
        Expr::Conditional(condition, when_true, when_false) => {
            check(condition, names, gaps);
            check_branch(when_true, names, gaps);
            check_branch(when_false, names, gaps);
        }
        // ここから下は、ラムダの引数を使うときだけ組み立てる（使わなければ、いま評価した値の定数）。
        _ if !mentioned && !expr.declares() => {}
        Expr::Is(operand, _) | Expr::As(operand, _) => check(operand, names, gaps),
        Expr::Switch(subject, arms) => {
            check(subject, names, gaps);
            for arm in arms {
                check_all(&arm.guard, names, gaps);
                check_branch(&arm.result, names, gaps);
            }
        }
        Expr::Coalesce(left, right) => {
            check(left, names, gaps);
            check_branch(right, names, gaps);
        }
        Expr::NullConditional(target, rest) => {
            check(target, names, gaps);
            check(rest, names, gaps);
        }
        Expr::Interpolated(pieces) => {
            for piece in pieces {
                if let Piece::Hole { expr, .. } = piece {
                    check(expr, names, gaps);
                }
            }
        }
        Expr::NewArray { sizes, items, .. } => {
            check_all(sizes, names, gaps);
            check_all(items.iter().flatten(), names, gaps);
        }
        Expr::NewMatrix { items, .. } => check_all(items, names, gaps),
        Expr::Anonymous(members) => check_all(members.iter().map(|(_, value)| value), names, gaps),
        Expr::New { arguments, initializer, .. } => {
            check_arguments(arguments, names, gaps);
            check_initializer(initializer, names, gaps);
        }
        Expr::DefaultLiteral => gaps.push(tr_str!("default without a type (where the type is unknown)")),
        Expr::TargetNew { .. } | Expr::Collection { .. } => gaps.push(tr_str!("new without a type and collection expressions (where the type is unknown)")),
        Expr::Generic(..) => gaps.push(tr_str!("method groups (where the type is unknown)")),
        Expr::FromEnd(_) | Expr::Range(..) => gaps.push(tr_str!("^ and ranges (outside an index)")),
        Expr::StackAlloc(_) | Expr::AddressOf(_) | Expr::Dereference(_) => gaps.push(tr_str!(" pointers")),
        _ => gaps.push(tr_str!("other constructs")),
    }
}

/// 代入の左辺。
fn assignable(target: &Expr, names: &[String], gaps: &mut Vec<&'static str>) {
    match target {
        Expr::Name(_) => {}
        Expr::Member(inner, _) => check(inner, names, gaps),
        Expr::Index(inner, indices) => {
            check(inner, names, gaps);
            check_all(indices, names, gaps);
        }
        _ => gaps.push(tr_str!("other assignment targets")),
    }
}

fn check_initializer(initializer: &Initializer, names: &[String], gaps: &mut Vec<&'static str>) {
    match initializer {
        Initializer::None => {}
        Initializer::Members(members) => {
            for (_, value) in members {
                match value {
                    Expr::Nested(inner) => check_initializer(inner, names, gaps),
                    other => check_targeted(other, names, gaps),
                }
            }
        }
        Initializer::Items(items) => {
            for item in items {
                check_arguments(item, names, gaps);
            }
        }
        Initializer::Indexed(entries) => {
            for (indices, value) in entries {
                check_all(indices, names, gaps);
                check(value, names, gaps);
            }
        }
    }
}

/// ブロックの中で宣言される変数の名前（入れ子の文の中も。使っているかの判断にだけ使うので、範囲は見ない）。
fn declared_names(statements: &[Statement], names: &mut Vec<String>) {
    for statement in statements {
        match statement {
            Statement::Local(_, declared) => names.extend(declared.iter().map(|(name, _)| name.clone())),
            Statement::ForEach { names: items, body, .. } => {
                names.extend(items.iter().cloned());
                declared_names(std::slice::from_ref(body), names);
            }
            Statement::For { init, body, .. } => {
                declared_names(init, names);
                declared_names(std::slice::from_ref(body), names);
            }
            Statement::Block(inner) => declared_names(inner, names),
            Statement::If(_, then, otherwise) => {
                declared_names(std::slice::from_ref(then), names);
                if let Some(otherwise) = otherwise {
                    declared_names(std::slice::from_ref(otherwise), names);
                }
            }
            Statement::While(_, body) | Statement::Do(body, _) | Statement::Lock(_, body) => declared_names(std::slice::from_ref(body), names),
            Statement::Switch(_, sections) => sections.iter().for_each(|section| declared_names(&section.body, names)),
            Statement::Try { body, catches, finally } => {
                declared_names(body, names);
                for handler in catches {
                    names.extend(handler.name.iter().cloned());
                    declared_names(&handler.body, names);
                }
                if let Some(finally) = finally {
                    declared_names(finally, names);
                }
            }
            Statement::Using(resource, body) => {
                declared_names(std::slice::from_ref(resource), names);
                declared_names(body, names);
            }
            Statement::Expr(_) | Statement::Return(_) | Statement::Break | Statement::Continue | Statement::Rethrow => {}
        }
    }
}

fn check_statement(statement: &Statement, names: &[String], gaps: &mut Vec<&'static str>) {
    let each = |statements: &[Statement], gaps: &mut Vec<&'static str>| statements.iter().for_each(|inner| check_statement(inner, names, gaps));
    match statement {
        Statement::Local(_, declared) => declared.iter().filter_map(|(_, initial)| initial.as_ref()).for_each(|initial| check_targeted(initial, names, gaps)),
        Statement::Expr(expr) => check(expr, names, gaps),
        Statement::Return(value) => value.iter().for_each(|value| check_targeted(value, names, gaps)),
        Statement::Block(inner) => each(inner, gaps),
        Statement::If(condition, then, otherwise) => {
            check(condition, names, gaps);
            each(std::slice::from_ref(then), gaps);
            if let Some(otherwise) = otherwise {
                each(std::slice::from_ref(otherwise), gaps);
            }
        }
        Statement::While(condition, body) | Statement::Do(body, condition) | Statement::Lock(condition, body) => {
            check(condition, names, gaps);
            each(std::slice::from_ref(body), gaps);
        }
        Statement::For { init, condition, step, body } => {
            each(init, gaps);
            check_all(condition, names, gaps);
            check_all(step, names, gaps);
            each(std::slice::from_ref(body), gaps);
        }
        Statement::ForEach { source, body, .. } => {
            check(source, names, gaps);
            each(std::slice::from_ref(body), gaps);
        }
        Statement::Switch(subject, sections) => {
            check(subject, names, gaps);
            for section in sections {
                check_all(section.labels.iter().flatten().filter_map(|(_, guard)| guard.as_ref()), names, gaps);
                each(&section.body, gaps);
            }
        }
        Statement::Try { body, catches, finally } => {
            each(body, gaps);
            for handler in catches {
                check_all(&handler.filter, names, gaps);
                each(&handler.body, gaps);
            }
            if let Some(finally) = finally {
                each(finally, gaps);
            }
        }
        Statement::Using(resource, body) => {
            each(std::slice::from_ref(resource), gaps);
            each(body, gaps);
        }
        Statement::Break | Statement::Continue | Statement::Rethrow => {}
    }
}
