//! クエリ式（`from x in … select …`）。メソッドの呼び出しに直して読む。

use super::*;

impl Parser<'_> {
    /// `from` のあとが「変数の名前 in」か「型 変数の名前 in」なら、クエリ式。
    pub(super) fn query_follows(&self) -> bool {
        let probe = || Parser { chars: self.chars.clone(), wide: None, in_query: 1, generated_names: self.generated_names };
        let mut plain = probe();
        if plain.designation().is_some() && plain.eat_word("in") {
            return true;
        }
        let mut typed = probe();
        typed.type_syntax().is_some() && typed.designation().is_some() && typed.eat_word("in")
    }

    /// クエリ式の `from 変数 in 式` の、変数と式（`from` は読んだあと）。型を書いた形（`from string s in 式`）は、
    /// C# と同じく `式.Cast<string>()` を元にする。
    pub(super) fn range(&mut self, clause: &str) -> Result<(String, Expr), String> {
        let start = self.chars.clone();
        let typed = match (self.type_syntax(), self.designation()) {
            (Some(ty), Some(name)) if self.peek_word() == "in" => Some((ty, name)),
            _ => {
                self.chars = start;
                None
            }
        };
        let (ty, variable) = match typed {
            Some((ty, name)) => (Some(ty), name),
            None => (None, self.designation().ok_or_else(|| tr!("Write a variable name after {clause}.", clause = clause))?),
        };
        if !self.eat_word("in") {
            return Err(tr!("There is no in after {clause} variable.", clause = clause));
        }
        let source = self.conditional()?;
        Ok((
            variable,
            match ty {
                Some(ty) => Expr::Call(Box::new(Expr::Generic(Box::new(Expr::Member(Box::new(source), "Cast".into())), vec![ty])), Vec::new()),
                None => source,
            },
        ))
    }

    /// クエリ式（`from x in 式 where 条件 orderby キー select 式`）を、メソッドの呼び出しに直して読む（C# の仕様の 12.20.3 と同じ直し方）。
    pub(super) fn query(&mut self) -> Result<Expr, String> {
        self.in_query += 1;
        let result = self.query_clauses();
        self.in_query -= 1;
        result
    }

    /// クエリ式の句を、順にメソッドの呼び出しにする。
    ///
    /// - `where` は `Where`、`orderby` は `OrderBy` / `ThenBy`（`descending` は `…Descending`）、`select` は `Select`、
    ///   `group 値 by キー` は `GroupBy`、2 つ目の `from` は `SelectMany`、`join` は `Join`（`into` があれば `GroupJoin`）。
    /// - `select …` / `group …` のあとの `into 変数` は、その結果を新しい範囲としてクエリを続ける。
    /// - C# のコンパイラと同じく、`where` などのあとの `select x`（そのままの値）は呼び出しにしない。
    /// - 範囲の変数が 2 つ以上になる所（`let`、2 つ目の `from` や `join` のあとに句が続く形）では、C# のコンパイラは
    ///   「見えない識別子」の匿名型に変数をまとめる。ここでは、匿名型の代わりにタプルにまとめる
    ///   （`(x, y)`、3 つ目からは `((x, y), z)`）。あとの句のラムダは、そのタプルを引数 `$t` で受け取り、本体の先頭で
    ///   もとの名前の変数に取り出す（`$t => { var x = $t.Item1; var y = $t.Item2; return 本体; }`）。
    pub(super) fn query_clauses(&mut self) -> Result<Expr, String> {
        let (first, mut source) = self.range("from")?;
        let mut variables = vec![first];
        let call = |target: Expr, name: &str, arguments: Vec<Expr>| Expr::Call(Box::new(Expr::Member(Box::new(target), name.into())), arguments);
        let single = |name: &str, body: Expr| Expr::Lambda(vec![(None, name.to_string())], Box::new(body));
        let mut clauses = 0;
        loop {
            let finished = match self.peek_word().as_str() {
                "where" => {
                    self.eat_word("where");
                    let condition = self.conditional()?;
                    source = call(source, "Where", vec![range_lambda(&variables, None, condition)]);
                    None
                }
                "let" => {
                    self.eat_word("let");
                    let name = self.designation().ok_or_else(|| tr!("Write a variable name after let."))?;
                    if !self.eat('=') {
                        return Err(tr!("There is no = after let variable."));
                    }
                    let value = self.conditional()?;
                    source = call(source, "Select", vec![range_lambda(&variables, None, Expr::Tuple(vec![range_value(&variables), value]))]);
                    variables.push(name);
                    None
                }
                "from" => {
                    self.eat_word("from");
                    let (second, inner) = self.range("from")?;
                    let collection = range_lambda(&variables, None, inner);
                    if self.eat_word("select") {
                        let selected = self.conditional()?;
                        Some(call(source.clone(), "SelectMany", vec![collection, range_lambda(&variables, Some(&second), selected)]))
                    } else {
                        let pair = Expr::Tuple(vec![range_value(&variables), Expr::Name(second.clone())]);
                        source = call(source, "SelectMany", vec![collection, range_lambda(&variables, Some(&second), pair)]);
                        variables.push(second);
                        None
                    }
                }
                "join" => {
                    self.eat_word("join");
                    let (second, inner) = self.range("join")?;
                    if !self.eat_word("on") {
                        return Err(tr!("There is no on after join variable in expression."));
                    }
                    let outer_key = self.conditional()?;
                    if !self.eat_word("equals") {
                        return Err(tr!("There is no equals after the on of join (on outer key equals inner key)."));
                    }
                    let inner_key = self.conditional()?;
                    let keys = vec![inner, range_lambda(&variables, None, outer_key), single(&second, inner_key)];
                    // `into 変数` があれば GroupJoin で、結果の式から見えるのは、その変数（内側の要素の集まり）。
                    let (method, joined) = if self.eat_word("into") {
                        ("GroupJoin", self.designation().ok_or_else(|| tr!("Write a variable name after join … into."))?)
                    } else {
                        ("Join", second)
                    };
                    if self.eat_word("select") {
                        let selected = self.conditional()?;
                        Some(call(source.clone(), method, [keys, vec![range_lambda(&variables, Some(&joined), selected)]].concat()))
                    } else {
                        let pair = Expr::Tuple(vec![range_value(&variables), Expr::Name(joined.clone())]);
                        source = call(source, method, [keys, vec![range_lambda(&variables, Some(&joined), pair)]].concat());
                        variables.push(joined);
                        None
                    }
                }
                "group" => {
                    self.eat_word("group");
                    let element = self.conditional()?;
                    if !self.eat_word("by") {
                        return Err(tr!("There is no by after group value (group value by key)."));
                    }
                    let key = range_lambda(&variables, None, self.conditional()?);
                    // 値が範囲の変数そのものなら、キーだけを渡す（C# と同じ）。
                    Some(if variables.len() == 1 && element == Expr::Name(variables[0].clone()) {
                        call(source.clone(), "GroupBy", vec![key])
                    } else {
                        call(source.clone(), "GroupBy", vec![key, range_lambda(&variables, None, element)])
                    })
                }
                "orderby" => {
                    self.eat_word("orderby");
                    let mut first = true;
                    loop {
                        let key = self.conditional()?;
                        let descending = self.eat_word("descending");
                        if !descending {
                            self.eat_word("ascending");
                        }
                        let name = match (first, descending) {
                            (true, false) => "OrderBy",
                            (true, true) => "OrderByDescending",
                            (false, false) => "ThenBy",
                            (false, true) => "ThenByDescending",
                        };
                        source = call(source, name, vec![range_lambda(&variables, None, key)]);
                        first = false;
                        if !self.eat(',') {
                            break;
                        }
                    }
                    None
                }
                "select" => {
                    self.eat_word("select");
                    let selected = self.conditional()?;
                    let identity = variables.len() == 1 && selected == Expr::Name(variables[0].clone());
                    Some(if identity && clauses > 0 { source.clone() } else { call(source.clone(), "Select", vec![range_lambda(&variables, None, selected)]) })
                }
                "" => return Err(tr!("The query expression does not end with select or group.")),
                other => return Err(tr!("{clause} in a query expression is not supported (from, let, where, orderby, join, group, select and into can be used).", clause = other)),
            };
            match finished {
                // `into 変数`: ここまでの結果を範囲として、続きを読む。
                Some(result) if self.eat_word("into") => {
                    variables = vec![self.designation().ok_or_else(|| tr!("Write a variable name after into."))?];
                    source = result;
                    clauses = 0;
                }
                Some(result) => return Ok(result),
                None => clauses += 1,
            }
        }
    }
}

/// クエリ式で、範囲の変数をまとめたタプルを受け取る、ラムダの引数の名前（C# の「見えない識別子」に当たる）。
const TRANSPARENT: &str = "$t";

/// いまの範囲の変数をまとめた値（1 つなら、その変数。2 つ以上なら、タプルを受け取った引数）。
fn range_value(variables: &[String]) -> Expr {
    match variables {
        [only] => Expr::Name(only.clone()),
        _ => Expr::Name(TRANSPARENT.into()),
    }
}

/// クエリ式の句の式を、範囲の変数を引数に取るラムダ式にする。`extra` は、2 つ目の引数（2 つ目の `from` や `join` の変数）。
/// 範囲の変数が 2 つ以上なら、タプル（`((x, y), z)` と左に入れ子）を受け取り、本体が使う変数を、先頭で取り出す。
fn range_lambda(variables: &[String], extra: Option<&str>, body: Expr) -> Expr {
    let mut parameters = vec![(None, if variables.len() == 1 { variables[0].clone() } else { TRANSPARENT.to_string() })];
    parameters.extend(extra.map(|name| (None, name.to_string())));
    let mut statements = Vec::new();
    if variables.len() > 1 {
        for (index, name) in variables.iter().enumerate() {
            if !body.mentions(std::slice::from_ref(name)) {
                continue;
            }
            // 最後に足した変数が Item2、それより前の変数は Item1 の中。
            let (mut value, mut size) = (Expr::Name(TRANSPARENT.into()), variables.len());
            while size > 1 {
                let last = index == size - 1;
                value = Expr::Member(Box::new(value), if last { "Item2" } else { "Item1" }.into());
                if last {
                    break;
                }
                size -= 1;
            }
            statements.push(Statement::Local(None, vec![(name.clone(), Some(value))]));
        }
    }
    if statements.is_empty() {
        return Expr::Lambda(parameters, Box::new(body));
    }
    statements.push(Statement::Return(Some(body)));
    Expr::Lambda(parameters, Box::new(Expr::Block(statements)))
}
