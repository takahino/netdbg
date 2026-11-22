//! `is` の右や `switch` の腕に書くパターン。

use super::*;

impl Parser<'_> {
    /// `is` のあとや `switch` の腕のパターン。いちばん弱いのは `or`、次に `and`、`not` の順。
    pub(super) fn pattern(&mut self) -> Result<Pattern, String> {
        let mut left = self.pattern_and()?;
        while self.eat_word("or") {
            left = Pattern::Or(Box::new(left), Box::new(self.pattern_and()?));
        }
        Ok(left)
    }

    pub(super) fn pattern_and(&mut self) -> Result<Pattern, String> {
        let mut left = self.pattern_not()?;
        while self.eat_word("and") {
            left = Pattern::And(Box::new(left), Box::new(self.pattern_not()?));
        }
        Ok(left)
    }

    pub(super) fn pattern_not(&mut self) -> Result<Pattern, String> {
        if self.eat_word("not") {
            return Ok(Pattern::Not(Box::new(self.pattern_not()?)));
        }
        self.pattern_primary()
    }

    pub(super) fn pattern_primary(&mut self) -> Result<Pattern, String> {
        /// 定数パターンと比較パターンの値は、シフトより強い演算子だけで書ける式。
        const OPERAND_LEVEL: usize = 7;
        self.skip_spaces();
        let word = self.peek_word();
        match word.as_str() {
            "null" => {
                self.eat_word("null");
                return Ok(Pattern::Null);
            }
            "var" => {
                self.eat_word("var");
                let name = self.designation().ok_or_else(|| tr!("Write a variable name after var."))?;
                return Ok(Pattern::Var(name));
            }
            "_" => {
                self.eat_word("_");
                return Ok(Pattern::Discard);
            }
            _ => {}
        }
        let rest: String = self.chars.clone().take(2).collect();
        for (text, op) in [("<=", BinaryOp::LessOrEqual), (">=", BinaryOp::GreaterOrEqual), ("<", BinaryOp::Less), (">", BinaryOp::Greater)] {
            if rest.starts_with(text) {
                for _ in 0..text.len() {
                    self.chars.next();
                }
                return Ok(Pattern::Relational(op, Box::new(self.binary(OPERAND_LEVEL)?)));
            }
        }
        match self.chars.peek() {
            // `( … )`: 括弧でくくったパターンか、位置のパターン（`(7, 3)`）。
            Some('(') => {
                self.chars.next();
                let (mut items, _) = self.pattern_items(')')?;
                // 中身が一つで、あとに `{` も変数の名前も続かなければ、括弧でくくっただけのパターン。
                return if items.len() == 1 && !matches!(self.peek_after_spaces(), Some('{')) && self.peek_word().is_empty() {
                    Ok(items.remove(0))
                } else {
                    self.finish_recursive(None, Some(items))
                };
            }
            Some('{') => return self.finish_recursive(None, None),
            Some('[') => {
                self.chars.next();
                let (items, slice) = self.pattern_items(']')?;
                let designation = self.designation();
                return Ok(Pattern::List { items, slice, designation });
            }
            _ => {}
        }
        // 型の名前としても、式としても読んでみる。同じ所まで読めたなら、型の名前の可能性がある。
        // 読み終えた位置は、あとの空白を飛ばしてから比べる（式の方は、空白まで読み進めることがある）。
        let start = self.chars.clone();
        let as_type = self.type_syntax().map(|ty| {
            self.skip_spaces();
            (ty, self.chars.clone().count())
        });
        let after_type = self.chars.clone();
        if let Some((ty, _)) = &as_type {
            // 型のあとに `{`、`(`、変数の名前が続けば、型のパターン。
            match self.chars.peek() {
                Some('{') => return self.finish_recursive(Some(ty.clone()), None),
                Some('(') => {
                    self.chars.next();
                    let (items, _) = self.pattern_items(')')?;
                    return self.finish_recursive(Some(ty.clone()), Some(items));
                }
                _ => {}
            }
            if let Some(name) = self.designation() {
                return Ok(Pattern::Declaration(ty.clone(), name));
            }
        }
        self.chars = start;
        // 型引数つきの型（`List<int>`）は、式として読むと比較になってしまう。型として読めていれば、型とする。
        if let Some((ty, _)) = as_type.as_ref().filter(|(ty, _)| !ty.arguments.is_empty()) {
            self.chars = after_type;
            return Ok(Pattern::TypeOrConstant(Some(ty.clone()), Box::new(Expr::Null)));
        }
        // `x is int?`: 型のあとの `?` は、続きが式の始まりでなければ（条件演算子でなければ）null 許容の印。
        if let Some((ty, _)) = as_type.as_ref().filter(|(ty, _)| ty.nullable) {
            if matches!(after_type.clone().next(), None | Some(')' | ']' | '}' | ',' | ';' | '&' | '|' | '^' | '=' | '!')) {
                self.chars = after_type;
                return Ok(Pattern::TypeOrConstant(Some(ty.clone()), Box::new(Expr::Null)));
            }
        }
        let expr = self.binary(OPERAND_LEVEL)?;
        self.skip_spaces();
        let ty = as_type.filter(|(_, left)| *left == self.chars.clone().count()).map(|(ty, _)| ty);
        Ok(Pattern::TypeOrConstant(ty, Box::new(expr)))
    }

    /// 位置のパターンかリストのパターンの中身を、閉じ括弧まで読む。リストの `..` は、2 番目に位置を返す。
    pub(super) fn pattern_items(&mut self, close: char) -> Result<(Vec<Pattern>, Option<usize>), String> {
        let (mut items, mut slice) = (Vec::new(), None);
        if self.eat(close) {
            return Ok((items, slice));
        }
        loop {
            self.skip_spaces();
            let mut ahead = self.chars.clone();
            if close == ']' && (ahead.next(), ahead.next()) == (Some('.'), Some('.')) {
                self.chars = ahead;
                if slice.is_some() {
                    return Err(tr!("Only one .. can be written in a list pattern."));
                }
                if !matches!(self.peek_after_spaces(), Some(',' | ']')) {
                    return Err(tr!("A pattern after .. in a list pattern (.. var rest, for example) is not supported yet."));
                }
                slice = Some(items.len());
            } else {
                // 位置のパターンの `名前: パターン` は、名前を読み飛ばす（位置で照合する）。
                if close == ')' {
                    let mut ahead = self.chars.clone();
                    let name: String = std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric() || *c == '_')).collect();
                    while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                    if !name.is_empty() && ahead.next() == Some(':') {
                        self.chars = ahead;
                    }
                }
                items.push(self.pattern()?);
            }
            if self.eat(close) {
                return Ok((items, slice));
            }
            if !self.eat(',') {
                return Err(tr!("The pattern is not closed with '{close}'.", close = close));
            }
        }
    }

    /// 型のあとの `{ メンバー: パターン, … }`（とその前の位置のパターン）と、最後の変数の名前を読む。
    pub(super) fn finish_recursive(&mut self, ty: Option<TypeSyntax>, positional: Option<Vec<Pattern>>) -> Result<Pattern, String> {
        let mut members = Vec::new();
        self.skip_spaces();
        if self.chars.peek() == Some(&'{') {
            self.chars.next();
            loop {
                self.skip_spaces();
                if self.eat('}') {
                    break;
                }
                // メンバーの名前（`A.B` のように入れ子でもよい）。
                let mut path = Vec::new();
                loop {
                    path.push(self.identifier().ok_or_else(|| tr!("Write member names in a property pattern."))?);
                    if !self.eat('.') {
                        break;
                    }
                }
                if !self.eat(':') {
                    return Err(tr!("There is no : after a name in the property pattern."));
                }
                members.push((path, self.pattern()?));
                if self.eat('}') {
                    break;
                }
                if !self.eat(',') {
                    return Err(tr!("The property pattern is not closed with }."));
                }
            }
        }
        let designation = self.designation();
        Ok(Pattern::Recursive { ty, positional, members, designation })
    }
}

/// `not` / `and` / `or` の中に、null 許容の型のパターン（`int?`）があるか。
pub(super) fn nullable_type_inside(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::TypeOrConstant(Some(ty), _) => ty.nullable,
        Pattern::Not(inner) => nullable_type_inside(inner),
        Pattern::And(a, b) | Pattern::Or(a, b) => nullable_type_inside(a) || nullable_type_inside(b),
        _ => false,
    }
}
