//! `new`: オブジェクトと配列の生成、初期化子。

use super::*;

impl Parser<'_> {
    /// `new` のあと。オブジェクトの生成か、配列の生成。
    pub(super) fn creation(&mut self) -> Result<Expr, String> {
        self.skip_spaces();
        // `new[] { … }`、`new[,] { { … }, { … } }`: 要素の型は、中身から決める。
        match self.rank_specifiers().as_slice() {
            [] => {}
            [1] => return Ok(Expr::NewArray { element: None, sizes: Vec::new(), items: Some(self.braced_items()?) }),
            [rank] => {
                let (dimensions, items) = self.nested_items(*rank as usize)?;
                return Ok(Expr::NewMatrix { element: None, dimensions, items });
            }
            _ => return Err(tr!("Only one [] can follow new (for an array of arrays, write the element type).")),
        }
        // `new { A = 1, x.B }`: 匿名型。`x.B` の形は、名前 B のメンバーになる。
        if self.chars.peek() == Some(&'{') {
            self.chars.next();
            let mut members = Vec::new();
            loop {
                if self.eat('}') {
                    break;
                }
                let start = self.chars.clone();
                let name = self.identifier();
                self.skip_spaces();
                let mut ahead = self.chars.clone();
                match (name, ahead.next(), ahead.next()) {
                    (Some(name), Some('='), next) if next != Some('=') => {
                        self.chars.next();
                        members.push((name, self.conditional()?));
                    }
                    _ => {
                        self.chars = start;
                        let expr = self.conditional()?;
                        let name = match &expr {
                            Expr::Name(name) | Expr::Member(_, name) => name.rsplit('.').next().unwrap_or(name).to_string(),
                            _ => return Err(tr!("Give anonymous type members names (name = expression).")),
                        };
                        members.push((name, expr));
                    }
                }
                if self.eat('}') {
                    break;
                }
                if !self.eat(',') {
                    return Err(tr!("The anonymous type is not closed with }."));
                }
            }
            return Ok(Expr::Anonymous(members));
        }
        // `new (string Label, int Value)[] { … }`: 要素がタプルの配列。型を省いた `new(…)` と見分けるため、
        // タプル型として読めて、すぐあとに `[` が続くときだけ。
        let tuple_array = self.chars.peek() == Some(&'(') && {
            let mut probe = Parser { chars: self.chars.clone(), wide: None, in_query: self.in_query, generated_names: self.generated_names };
            probe.tuple_type().is_some() && probe.peek_after_spaces() == Some('[')
        };
        // `new(…)`: 型を省いた生成。
        if self.chars.peek() == Some(&'(') && !tuple_array {
            self.chars.next();
            let arguments = self.arguments(')')?;
            self.skip_spaces();
            let initializer = if self.chars.peek() == Some(&'{') { self.initializer()? } else { Initializer::None };
            return Ok(Expr::TargetNew { arguments, initializer });
        }
        let mut ty = self.type_syntax().ok_or_else(|| tr!("Write a type name after new."))?;
        // `new Map <int, int> (…)`: 型の名前と `<` の間の空白。new のあとでは、比較の `<` と紛れない。
        if ty.arguments.is_empty() && !ty.array && !ty.nullable && self.peek_after_spaces() == Some('<') {
            let saved = self.chars.clone();
            self.eat('<');
            let mut arguments = Vec::new();
            while let Some(argument) = self.type_syntax() {
                arguments.push(argument);
                if !self.eat(',') {
                    break;
                }
            }
            if !arguments.is_empty() && self.eat('>') {
                ty.arguments = arguments;
                let shape = self.rank_specifiers();
                ty = ty.with_shape(&shape);
            } else {
                self.chars = saved;
            }
        }
        // `new int?[2]`: new のあとでは、`?[` は添字ではなく、null 許容の要素の印。
        if !ty.nullable && !ty.array && self.chars.clone().take(2).collect::<String>() == "?[" {
            self.chars.next();
            ty.nullable = true;
        }
        // 配列の型が書かれている: `new int[] { … }`、`new int[][] { … }`（要素は `int[]`）、`new int[,] { { … }, { … } }`。
        if let Some(&rank) = ty.shape().first() {
            let element = ty.element();
            if rank == 1 {
                return Ok(Expr::NewArray { element: Some(element), sizes: Vec::new(), items: Some(self.braced_items()?) });
            }
            let (dimensions, items) = self.nested_items(rank as usize)?;
            return Ok(Expr::NewMatrix { element: Some(element), dimensions, items });
        }
        // `new int[3]`、`new int[2, 3]`
        if self.eat('[') {
            let sizes = self.arguments(']')?;
            // `new string[n][]`: 要素が配列の、配列（中の配列は作らない。どれも null）。
            let element = ty.with_shape(&self.rank_specifiers());
            self.skip_spaces();
            // C# と同じく、配列の生成のすぐあとの `[…]` は、次元の指定としてしか読まない（添字にするには括弧が要る）。
            if self.chars.peek() == Some(&'[') {
                return Err(tr!("An index cannot follow an array creation directly. Wrap it in parentheses, like (new int[2])[0]."));
            }
            let items = if self.chars.peek() == Some(&'{') { Some(self.braced_items()?) } else { None };
            return Ok(Expr::NewArray { element: Some(element), sizes, items });
        }
        let arguments = if self.eat('(') { self.arguments(')')? } else { Vec::new() };
        self.skip_spaces();
        let initializer = if self.chars.peek() == Some(&'{') { self.initializer()? } else { Initializer::None };
        Ok(Expr::New { ty, arguments, initializer })
    }

    /// `{ 式, 式, … }`
    pub(super) fn braced_items(&mut self) -> Result<Vec<Expr>, String> {
        if !self.eat('{') {
            return Err(tr!("Write the array elements in { }."));
        }
        self.arguments('}')
    }

    /// 多次元の配列の初期化子（`{ { 1, 2 }, { 3, 4 } }`）。次元ごとの大きさと、行を優先した順に並べた要素。
    pub(super) fn nested_items(&mut self, rank: usize) -> Result<(Vec<usize>, Vec<Expr>), String> {
        if !self.eat('{') {
            return Err(tr!("Write the array elements in { }."));
        }
        if rank <= 1 {
            let items = self.arguments('}')?;
            return Ok((vec![items.len()], items));
        }
        let (mut inner, mut items, mut count) = (Vec::new(), Vec::new(), 0);
        while !self.eat('}') {
            let (dimensions, values) = self.nested_items(rank - 1)?;
            if count > 0 && dimensions != inner {
                return Err(tr!("The rows of the multidimensional array initializer are not the same length."));
            }
            inner = dimensions;
            items.extend(values);
            count += 1;
            if !self.eat(',') && self.peek_after_spaces() != Some('}') {
                return Err(tr!("The array initializer is not closed with }."));
            }
        }
        let mut dimensions = vec![count];
        dimensions.extend(if inner.is_empty() { vec![0; rank - 1] } else { inner });
        Ok((dimensions, items))
    }

    /// オブジェクトの生成のあとの `{ … }`。中身の形で、メンバーへの代入、要素の追加、インデクサーへの代入を見分ける。
    pub(super) fn initializer(&mut self) -> Result<Initializer, String> {
        self.chars.next();
        let (mut members, mut items, mut indexed) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            self.skip_spaces();
            if self.chars.next_if_eq(&'}').is_some() {
                break;
            }
            match self.chars.peek() {
                // `["k"] = 1`
                Some('[') => {
                    self.chars.next();
                    let indices = self.arguments(']')?;
                    if !self.eat('=') {
                        return Err(tr!("There is no = after [index] in the initializer."));
                    }
                    indexed.push((indices, self.initializer_value()?));
                }
                // `{ "k", 1 }`: 引数が複数の Add
                Some('{') => {
                    self.chars.next();
                    items.push(self.arguments('}')?);
                }
                _ => {
                    // `名前 = 式` なら、メンバーへの代入。そうでなければ、要素。
                    let start = self.chars.clone();
                    let name = self.identifier();
                    self.skip_spaces();
                    let mut ahead = self.chars.clone();
                    match (name, ahead.next(), ahead.next()) {
                        (Some(name), Some('='), next) if next != Some('=') && next != Some('>') => {
                            self.chars.next();
                            members.push((name, self.initializer_value()?));
                        }
                        _ => {
                            self.chars = start;
                            items.push(vec![self.expression()?]);
                        }
                    }
                }
            }
            self.skip_spaces();
            if self.chars.next_if_eq(&',').is_none() && self.chars.peek() != Some(&'}') {
                return Err(tr!("The initializer is not closed with }."));
            }
        }
        match (members.is_empty(), items.is_empty(), indexed.is_empty()) {
            (true, true, true) => Ok(Initializer::None),
            (false, true, true) => Ok(Initializer::Members(members)),
            (true, false, true) => Ok(Initializer::Items(items)),
            (true, true, false) => Ok(Initializer::Indexed(indexed)),
            _ => Err(tr!("The initializer mixes member assignments and elements.")),
        }
    }

    /// 初期化子の `名前 =` の右辺。式か、入れ子の初期化子（`Items = { 1, 2 }`、`Inner = { X = 1 }`）。
    pub(super) fn initializer_value(&mut self) -> Result<Expr, String> {
        if self.peek_after_spaces() == Some('{') {
            self.skip_spaces();
            return Ok(Expr::Nested(self.initializer()?));
        }
        self.expression()
    }
}
