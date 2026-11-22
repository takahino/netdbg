//! 構文木をたどる。式の中に（文、パターン、初期化子の中も含めて）ある式を、順に見ていく。

use super::*;

impl Expr {
    /// この式か、中に書かれた式のどれかが、`test` に合うか。
    pub(in crate::debugger::session) fn any(&self, test: &mut dyn FnMut(&Expr) -> bool) -> bool {
        if test(self) {
            return true;
        }
        fn each(items: &[Expr], test: &mut dyn FnMut(&Expr) -> bool) -> bool {
            items.iter().any(|item| item.any(test))
        }
        match self {
            Expr::Name(_)
            | Expr::Int(_)
            | Expr::Long(_)
            | Expr::UInt(_)
            | Expr::ULong(_)
            | Expr::Double(_)
            | Expr::Float(_)
            | Expr::Decimal(_)
            | Expr::Char(_)
            | Expr::Bool(_)
            | Expr::Str(_)
            | Expr::Utf16(_)
            | Expr::Utf8(_)
            | Expr::Null
            | Expr::Receiver
            | Expr::DefaultLiteral
            | Expr::TypeOf(_)
            | Expr::Default(_)
            | Expr::SizeOf(_) => false,
            Expr::Member(inner, _)
            | Expr::Unary(_, inner)
            | Expr::FromEnd(inner)
            | Expr::Checked(_, inner)
            | Expr::Cast(_, inner)
            | Expr::As(inner, _)
            | Expr::Throw(inner)
            | Expr::Await(inner)
            | Expr::Async(inner)
            | Expr::StackAlloc(inner)
            | Expr::AddressOf(inner)
            | Expr::Dereference(inner)
            | Expr::Named(_, inner)
            | Expr::Generic(inner, _)
            | Expr::Lambda(_, inner)
            | Expr::Increment { target: inner, .. } => inner.any(test),
            Expr::Index(target, items) | Expr::Call(target, items) => target.any(test) || each(items, test),
            Expr::Binary(_, left, right) | Expr::Coalesce(left, right) | Expr::NullConditional(left, right) | Expr::Assign(left, _, right) => left.any(test) || right.any(test),
            Expr::Conditional(condition, when_true, when_false) => condition.any(test) || when_true.any(test) || when_false.any(test),
            Expr::Range(start, end) => start.as_deref().is_some_and(|start| start.any(test)) || end.as_deref().is_some_and(|end| end.any(test)),
            Expr::Switch(subject, arms) => subject.any(test) || arms.iter().any(|arm| arm.pattern.any(test) || arm.guard.as_ref().is_some_and(|guard| guard.any(test)) || arm.result.any(test)),
            Expr::With(source, members) => source.any(test) || members.iter().any(|(_, value)| value.any(test)),
            Expr::Tuple(items) | Expr::NewMatrix { items, .. } => each(items, test),
            Expr::Collection { arguments, items } => each(arguments, test) || items.iter().any(|(_, item)| item.any(test)),
            Expr::TargetNew { arguments, initializer } | Expr::New { arguments, initializer, .. } => each(arguments, test) || initializer.any(test),
            Expr::NewArray { sizes, items, .. } => each(sizes, test) || items.as_deref().is_some_and(|items| each(items, test)),
            Expr::Anonymous(members) => members.iter().any(|(_, value)| value.any(test)),
            Expr::Interpolated(pieces) => pieces.iter().any(|piece| matches!(piece, Piece::Hole { expr, .. } if expr.any(test))),
            Expr::Is(operand, pattern) => operand.any(test) || pattern.any(test),
            Expr::Out(target) => target.any(test),
            Expr::Block(statements) => statements.iter().any(|statement| statement.any(test)),
            Expr::Nested(initializer) => initializer.any(test),
            Expr::Deconstruct(targets, value) => targets.iter().any(|target| target.any(test)) || value.any(test),
        }
    }

    /// 式の中に、`names` のどれかの名前の参照があるか。
    pub(in crate::debugger::session) fn mentions(&self, names: &[String]) -> bool {
        self.any(&mut |expr| matches!(expr, Expr::Name(name) if names.contains(name)))
    }

    /// 式の中に、変数を作るパターン（`x is int k`、`x switch { var v => … }`）があるか。
    pub(in crate::debugger::session) fn declares(&self) -> bool {
        self.any(&mut |expr| match expr {
            Expr::Is(_, pattern) => pattern.declares(),
            Expr::Switch(_, arms) => arms.iter().any(|arm| arm.pattern.declares()),
            _ => false,
        })
    }
}

impl Pattern {
    /// パターンが変数を作るか（`int k`、`var v`、`{ } x`、`[..] x`）。
    pub(in crate::debugger::session) fn declares(&self) -> bool {
        match self {
            Pattern::Declaration(..) | Pattern::Var(_) => true,
            Pattern::Null | Pattern::Discard | Pattern::Relational(..) | Pattern::TypeOrConstant(..) => false,
            Pattern::Not(inner) => inner.declares(),
            Pattern::And(left, right) | Pattern::Or(left, right) => left.declares() || right.declares(),
            Pattern::Recursive { positional, members, designation, .. } => {
                designation.is_some() || positional.as_deref().is_some_and(|items| items.iter().any(Pattern::declares)) || members.iter().any(|(_, pattern)| pattern.declares())
            }
            Pattern::List { items, designation, .. } => designation.is_some() || items.iter().any(Pattern::declares),
        }
    }

    fn any(&self, test: &mut dyn FnMut(&Expr) -> bool) -> bool {
        match self {
            Pattern::Null | Pattern::Declaration(..) | Pattern::Var(_) | Pattern::Discard => false,
            Pattern::Not(inner) => inner.any(test),
            Pattern::Relational(_, value) | Pattern::TypeOrConstant(_, value) => value.any(test),
            Pattern::And(left, right) | Pattern::Or(left, right) => left.any(test) || right.any(test),
            Pattern::Recursive { positional, members, .. } => positional.as_deref().is_some_and(|items| items.iter().any(|item| item.any(test))) || members.iter().any(|(_, pattern)| pattern.any(test)),
            Pattern::List { items, .. } => items.iter().any(|item| item.any(test)),
        }
    }
}

impl Initializer {
    fn any(&self, test: &mut dyn FnMut(&Expr) -> bool) -> bool {
        match self {
            Initializer::None => false,
            Initializer::Members(members) => members.iter().any(|(_, value)| value.any(test)),
            Initializer::Items(items) => items.iter().flatten().any(|item| item.any(test)),
            Initializer::Indexed(entries) => entries.iter().any(|(indices, value)| indices.iter().any(|index| index.any(test)) || value.any(test)),
        }
    }
}

impl OutTarget {
    fn any(&self, test: &mut dyn FnMut(&Expr) -> bool) -> bool {
        match self {
            OutTarget::Discard | OutTarget::Declare(..) => false,
            OutTarget::Existing(place) => place.any(test),
            OutTarget::Tuple(targets) => targets.iter().any(|target| target.any(test)),
        }
    }
}

impl Statement {
    fn any(&self, test: &mut dyn FnMut(&Expr) -> bool) -> bool {
        fn all(statements: &[Statement], test: &mut dyn FnMut(&Expr) -> bool) -> bool {
            statements.iter().any(|statement| statement.any(test))
        }
        match self {
            Statement::Break | Statement::Continue | Statement::Rethrow => false,
            Statement::Local(_, declared) => declared.iter().any(|(_, initial)| initial.as_ref().is_some_and(|initial| initial.any(test))),
            Statement::Expr(expr) => expr.any(test),
            Statement::Return(value) => value.as_ref().is_some_and(|value| value.any(test)),
            Statement::Block(statements) => all(statements, test),
            Statement::If(condition, then, otherwise) => condition.any(test) || then.any(test) || otherwise.as_deref().is_some_and(|otherwise| otherwise.any(test)),
            Statement::While(condition, body) | Statement::Do(body, condition) | Statement::Lock(condition, body) => condition.any(test) || body.any(test),
            Statement::For { init, condition, step, body } => all(init, test) || condition.as_ref().is_some_and(|condition| condition.any(test)) || step.iter().any(|step| step.any(test)) || body.any(test),
            Statement::ForEach { source, body, .. } => source.any(test) || body.any(test),
            Statement::Switch(subject, sections) => {
                subject.any(test)
                    || sections.iter().any(|section| {
                        section.labels.iter().flatten().any(|(pattern, guard)| pattern.any(test) || guard.as_ref().is_some_and(|guard| guard.any(test))) || all(&section.body, test)
                    })
            }
            Statement::Try { body, catches, finally } => {
                all(body, test) || catches.iter().any(|handler| handler.filter.as_ref().is_some_and(|filter| filter.any(test)) || all(&handler.body, test)) || finally.as_deref().is_some_and(|finally| all(finally, test))
            }
            Statement::Using(resource, body) => resource.any(test) || all(body, test),
        }
    }
}
