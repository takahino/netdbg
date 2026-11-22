//! ラムダ式と匿名メソッド、その本体の文、分解の代入。

use super::*;

impl Parser<'_> {
    /// 読む位置がラムダ式なら、それを読む。そうでなければ、読む位置を戻して `None`。
    /// 前に付く `async` と `static` も読む（`static` は、外の変数を取り込まないというコンパイラへの約束で、動きは変わらない）。
    pub(super) fn lambda(&mut self) -> Option<Result<Expr, String>> {
        let saved = self.chars.clone();
        let (mut is_async, mut modified) = (false, false);
        // ラムダ式の前の属性（`[DebuggerHidden] static x => …`）は、評価に関係しないので読み飛ばす。
        // ラムダ式が続かなければ（`[1, 2]` はコレクション式）、読む位置は下で元に戻る。
        while self.peek_after_spaces() == Some('[') {
            self.skip_spaces();
            let (mut depth, mut quote) = (0usize, None::<char>);
            for c in self.chars.by_ref() {
                match (quote, c) {
                    (Some(open), c) if c == open => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(c),
                    (None, '[' | '(' | '{') => depth += 1,
                    (None, ']' | ')' | '}') => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
            }
            modified = true;
        }
        loop {
            if self.eat_word("async") {
                is_async = true;
            } else if !self.eat_word("static") {
                break;
            }
            modified = true;
        }
        // `async delegate (…) { … }`: 匿名メソッド。
        let found = if modified && self.peek_word() == "delegate" {
            self.eat_word("delegate");
            Some(self.anonymous_method())
        } else {
            self.lambda_after_modifiers()
        };
        match found {
            Some(Ok(Expr::Lambda(parameters, body))) if is_async => Some(Ok(Expr::Lambda(parameters, Box::new(Expr::Async(body))))),
            Some(result) => Some(result),
            // `async` や `static` という名前の変数だったのかもしれない（`async => async + 1`）。修飾なしで読み直す。
            None => {
                self.chars = saved;
                if modified { self.lambda_after_modifiers() } else { None }
            }
        }
    }

    pub(super) fn lambda_after_modifiers(&mut self) -> Option<Result<Expr, String>> {
        let saved = self.chars.clone();
        self.skip_spaces();
        let mut parameters = Vec::new();
        if self.chars.peek() == Some(&'(') {
            self.chars.next();
            if !self.eat(')') {
                loop {
                    // 引数の前の `ref` / `in` / `out` / `params` / `scoped` は、渡し方の指定。ラムダ式を読むだけなら要らない。
                    while ["ref", "in", "out", "params", "scoped"].iter().any(|modifier| self.eat_word(modifier)) {}
                    // `(x, y)` か `(int x, string y)`。
                    let start = self.chars.clone();
                    let typed = match (self.type_syntax(), self.designation()) {
                        (Some(ty), Some(name)) => Some((Some(ty), name)),
                        _ => {
                            self.chars = start;
                            None
                        }
                    };
                    match typed.or_else(|| self.designation().map(|name| (None, name))) {
                        Some(parameter) => parameters.push(parameter),
                        None => {
                            self.chars = saved;
                            return None;
                        }
                    }
                    if self.eat(')') {
                        break;
                    }
                    if !self.eat(',') {
                        self.chars = saved;
                        return None;
                    }
                }
            }
        } else {
            match self.designation() {
                Some(name) => parameters.push((None, name)),
                None => {
                    self.chars = saved;
                    return None;
                }
            }
        }
        self.skip_spaces();
        let mut ahead = self.chars.clone();
        if (ahead.next(), ahead.next()) != (Some('='), Some('>')) {
            self.chars = saved;
            return None;
        }
        self.chars = ahead;
        if self.peek_after_spaces() == Some('{') {
            return Some(self.block_body().map(|body| Expr::Lambda(parameters, Box::new(body))));
        }
        Some(self.throwable().map(|body| Expr::Lambda(parameters, Box::new(body))))
    }

    /// C# 2 の匿名メソッド（`delegate (int x) { return x + 1; }`、`delegate { }`）。ラムダ式として読む。
    pub(super) fn anonymous_method(&mut self) -> Result<Expr, String> {
        let mut parameters = Vec::new();
        if self.eat('(') && !self.eat(')') {
            loop {
                let ty = self.type_syntax().ok_or_else(|| tr!("Write a type and a name for each parameter of the anonymous method."))?;
                let name = self.designation().ok_or_else(|| tr!("Write a type and a name for each parameter of the anonymous method."))?;
                parameters.push((Some(ty), name));
                if self.eat(')') {
                    break;
                }
                if !self.eat(',') {
                    return Err(tr!("The parameters of the anonymous method are not closed with )."));
                }
            }
        }
        if self.peek_after_spaces() != Some('{') {
            return Err(tr!("Write the body of the anonymous method in { }."));
        }
        let body = self.block_body()?;
        Ok(Expr::Lambda(parameters, Box::new(body)))
    }

    /// 読む位置が分解の代入（`var (a, b) = 式`、`(int a, var b) = 式`、`(x, _) = 式`）なら、それを読む。
    /// そうでなければ、読む位置を戻して `None`（括弧でくくった式やタプルは、ふつうの式として読まれる）。
    pub(super) fn deconstruction(&mut self) -> Option<Result<Expr, String>> {
        // 先に、形だけを見る: （`var`）`( … )` のあとに `=` が続くこと。括弧でくくった式を、毎回 2 度読まないため。
        let mut ahead = self.chars.clone();
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        if ahead.clone().take(3).collect::<String>() == "var" {
            ahead.nth(2);
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        }
        if ahead.next() != Some('(') {
            return None;
        }
        let (mut depth, mut quote) = (1usize, None::<char>);
        for c in ahead.by_ref() {
            match (quote, c) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '(' | '[' | '{') => depth += 1,
                (None, ')' | ']' | '}') => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        if ahead.next() != Some('=') || matches!(ahead.next(), Some('=' | '>')) {
            return None;
        }
        let saved = self.chars.clone();
        let targets = self.deconstruction_targets(false);
        // 左辺のあとは、`=` 一つ（`==` と `=>` ではない）。
        self.skip_spaces();
        let mut ahead = self.chars.clone();
        let assigned = ahead.next() == Some('=') && !matches!(ahead.next(), Some('=' | '>'));
        match targets {
            Some(targets) if assigned && targets.len() >= 2 => {
                self.chars.next();
                Some(self.expression().map(|value| Expr::Deconstruct(targets, Box::new(value))))
            }
            _ => {
                self.chars = saved;
                None
            }
        }
    }

    /// 分解の代入の左辺（`var (a, b)` か `(要素, 要素, …)`）。要素は、`var 名前`、`型 名前`、`_`、代入できる式、入れ子の括弧。
    /// `declared` は、外側に `var` が付いていること（`var (a, (b, c))` の中の名前は、みな宣言）。
    pub(super) fn deconstruction_targets(&mut self, declared: bool) -> Option<Vec<OutTarget>> {
        self.skip_spaces();
        let all_declared = self.eat_word("var") || declared;
        if !self.eat('(') {
            return None;
        }
        let named = |name: String, ty: Option<TypeSyntax>| if name == "_" { OutTarget::Discard } else { OutTarget::Declare(ty, name) };
        let mut targets = Vec::new();
        loop {
            let nested = self.peek_after_spaces() == Some('(') || (self.peek_word() == "var" && self.chars.clone().skip_while(|c| c.is_whitespace()).skip("var".len()).find(|c| !c.is_whitespace()) == Some('('));
            let target = if nested {
                OutTarget::Tuple(self.deconstruction_targets(all_declared)?)
            } else if all_declared || self.eat_word("var") {
                named(self.designation()?, None)
            } else if self.eat_word("_") {
                OutTarget::Discard
            } else {
                // `型 名前` か、代入できる式。
                let start = self.chars.clone();
                match (self.type_syntax(), self.designation()) {
                    (Some(ty), Some(name)) if matches!(self.peek_after_spaces(), Some(',' | ')')) => named(name, Some(ty)),
                    _ => {
                        self.chars = start;
                        match self.unary() {
                            Ok(target @ (Expr::Name(_) | Expr::Member(..) | Expr::Index(..))) => OutTarget::Existing(Box::new(target)),
                            _ => return None,
                        }
                    }
                }
            };
            targets.push(target);
            if self.eat(')') {
                return Some(targets);
            }
            if !self.eat(',') {
                return None;
            }
        }
    }

    /// ラムダ式や匿名メソッドの `{ … }` の本体。文が 1 つだけの形（`{ return 式; }`、`{ 式; }`、`{ }`）を、式の本体に直す。
    /// 空の本体は、何もしない式（`default(object)` を捨てる。値を返さないデリゲートでだけ意味がある）。
    /// 文を 2 つ以上並べた本体は `Expr::Block`。書ける文は、ローカル変数の宣言、式の文、`throw`、最後の `return`。
    pub(super) fn block_body(&mut self) -> Result<Expr, String> {
        let statements = self.block()?;
        Ok(match statements.as_slice() {
            // 空の本体は、何もしない式（`default(object)` を捨てる。値を返さないデリゲートでだけ意味がある）。
            [] | [Statement::Return(None)] => Expr::Default(TypeSyntax::named("System.Object")),
            [Statement::Return(Some(body))] | [Statement::Expr(body)] => body.clone(),
            _ => Expr::Block(statements),
        })
    }

    /// `{ 文… }`。
    pub(super) fn block(&mut self) -> Result<Vec<Statement>, String> {
        if !self.eat('{') {
            return Err(tr!("The { of the statement block is missing."));
        }
        let mut statements = Vec::new();
        loop {
            if self.eat('}') {
                return Ok(statements);
            }
            if self.peek_after_spaces().is_none() {
                return Err(tr!("The lambda body is not closed with }."));
            }
            // `using var x = 式;`（`await using` も）: ブロックの残りが、その本体。
            let saved = self.chars.clone();
            self.eat_word("await");
            if self.eat_word("using") && self.peek_after_spaces() != Some('(') {
                let resource = match self.local_declaration()? {
                    Some(declaration) => declaration,
                    None => return Err(tr!("After using, write a variable declaration (using var x = expression;) or a resource in parentheses.")),
                };
                self.end_of_statement()?;
                let mut rest = Vec::new();
                while !self.eat('}') {
                    if self.peek_after_spaces().is_none() {
                        return Err(tr!("The lambda body is not closed with }."));
                    }
                    rest.push(self.statement()?);
                }
                statements.push(Statement::Using(Box::new(resource), rest));
                return Ok(statements);
            }
            self.chars = saved;
            statements.push(self.statement()?);
        }
    }

    /// 文を一つ読む。
    /// 読む位置が、switch 文の次の節の始まり（`case`、または `default:`）か。`default` のあとに `:` が無ければ、
    /// `default(T)` などの式の始まり。
    fn at_switch_label(&self) -> bool {
        match self.peek_word().as_str() {
            "case" => true,
            "default" => self.chars.clone().skip_while(|c| c.is_whitespace()).skip("default".len()).find(|c| !c.is_whitespace()) == Some(':'),
            _ => false,
        }
    }

    pub(super) fn statement(&mut self) -> Result<Statement, String> {
        self.skip_spaces();
        if self.chars.peek() == Some(&'{') {
            return Ok(Statement::Block(self.block()?));
        }
        if self.eat(';') {
            return Ok(Statement::Block(Vec::new()));
        }
        let word = self.peek_word();
        // `await foreach` と `await using` は、同期の形と同じに読む（待ちは、どれも同期。D-034）。
        if word == "await" {
            let saved = self.chars.clone();
            self.eat_word("await");
            if matches!(self.peek_word().as_str(), "foreach" | "using") {
                return self.statement();
            }
            self.chars = saved;
        }
        match word.as_str() {
            "return" => {
                self.eat_word("return");
                let value = if matches!(self.peek_after_spaces(), Some(';' | '}')) { None } else { Some(self.expression()?) };
                self.end_of_statement()?;
                Ok(Statement::Return(value))
            }
            "throw" => {
                self.eat_word("throw");
                if matches!(self.peek_after_spaces(), Some(';' | '}')) {
                    self.end_of_statement()?;
                    return Ok(Statement::Rethrow);
                }
                let thrown = self.expression()?;
                self.end_of_statement()?;
                Ok(Statement::Expr(Expr::Throw(Box::new(thrown))))
            }
            "if" => {
                self.eat_word("if");
                let condition = self.parenthesized("if")?;
                let then = Box::new(self.statement()?);
                let otherwise = if self.eat_word("else") { Some(Box::new(self.statement()?)) } else { None };
                Ok(Statement::If(condition, then, otherwise))
            }
            "while" => {
                self.eat_word("while");
                let condition = self.parenthesized("while")?;
                Ok(Statement::While(condition, Box::new(self.statement()?)))
            }
            "do" => {
                self.eat_word("do");
                let body = Box::new(self.statement()?);
                if !self.eat_word("while") {
                    return Err(tr!("There is no while after the body of the do statement."));
                }
                let condition = self.parenthesized("while")?;
                self.end_of_statement()?;
                Ok(Statement::Do(body, condition))
            }
            "for" => {
                self.eat_word("for");
                if !self.eat('(') {
                    return Err(tr!("There is no ( after for."));
                }
                let mut init = Vec::new();
                if !self.eat(';') {
                    match self.local_declaration()? {
                        Some(declaration) => init.push(declaration),
                        None => init.extend(self.expression_list()?.into_iter().map(Statement::Expr)),
                    }
                    if !self.eat(';') {
                        return Err(tr!("There is no ; after the initializer of for."));
                    }
                }
                let condition = if self.peek_after_spaces() == Some(';') { None } else { Some(self.expression()?) };
                if !self.eat(';') {
                    return Err(tr!("There is no ; after the condition of for."));
                }
                let step = if self.peek_after_spaces() == Some(')') { Vec::new() } else { self.expression_list()? };
                if !self.eat(')') {
                    return Err(tr!("The parentheses of for are not closed with )."));
                }
                Ok(Statement::For { init, condition, step, body: Box::new(self.statement()?) })
            }
            "foreach" => {
                self.eat_word("foreach");
                if !self.eat('(') {
                    return Err(tr!("There is no ( after foreach."));
                }
                let ty = if self.eat_word("var") { None } else { Some(self.type_syntax().ok_or_else(|| tr!("Write a type (or var) and a variable name inside the parentheses of foreach."))?) };
                // `foreach (var (key, value) in …)`: 要素を分解する。
                let mut names = Vec::new();
                if self.eat('(') {
                    loop {
                        self.eat_word("var");
                        names.push(self.designation().ok_or_else(|| tr!("List variable names in the foreach deconstruction."))?);
                        if self.eat(')') {
                            break;
                        }
                        if !self.eat(',') {
                            return Err(tr!("The foreach deconstruction is not closed with )."));
                        }
                    }
                } else {
                    names.push(self.designation().ok_or_else(|| tr!("Write a type (or var) and a variable name inside the parentheses of foreach."))?);
                }
                if !self.eat_word("in") {
                    return Err(tr!("There is no in after the foreach variable."));
                }
                let source = self.expression()?;
                if !self.eat(')') {
                    return Err(tr!("The parentheses of foreach are not closed with )."));
                }
                Ok(Statement::ForEach { ty, names, source, body: Box::new(self.statement()?) })
            }
            "switch" => {
                self.eat_word("switch");
                let subject = self.parenthesized("switch")?;
                if !self.eat('{') {
                    return Err(tr!("The { of the switch statement is missing."));
                }
                let mut sections = Vec::new();
                while !self.eat('}') {
                    let mut labels = Vec::new();
                    loop {
                        if self.eat_word("case") {
                            let pattern = self.pattern()?;
                            let guard = if self.eat_word("when") { Some(self.expression()?) } else { None };
                            labels.push(Some((pattern, guard)));
                        } else if self.peek_word() == "default" && self.chars.clone().skip_while(|c| c.is_whitespace()).skip("default".len()).find(|c| !c.is_whitespace()) == Some(':') {
                            self.eat_word("default");
                            labels.push(None);
                        } else {
                            break;
                        }
                        if !self.eat(':') {
                            return Err(tr!("There is no : after the label of the switch statement."));
                        }
                    }
                    if labels.is_empty() {
                        return Err(tr!("Write case or default inside the switch statement."));
                    }
                    let mut body = Vec::new();
                    while !self.at_switch_label() && self.peek_after_spaces() != Some('}') {
                        if self.peek_after_spaces().is_none() {
                            return Err(tr!("The switch statement is not closed with }."));
                        }
                        body.push(self.statement()?);
                    }
                    sections.push(SwitchSection { labels, body });
                }
                Ok(Statement::Switch(subject, sections))
            }
            "try" => {
                self.eat_word("try");
                let body = self.block()?;
                let mut catches = Vec::new();
                while self.eat_word("catch") {
                    let (mut ty, mut name) = (None, None);
                    if self.eat('(') {
                        ty = Some(self.type_syntax().ok_or_else(|| tr!("Write the exception type inside the parentheses of catch."))?);
                        name = self.designation();
                        if !self.eat(')') {
                            return Err(tr!("The parentheses of catch are not closed with )."));
                        }
                    }
                    let filter = if self.eat_word("when") { Some(self.parenthesized("when")?) } else { None };
                    catches.push(Catch { ty, name, filter, body: self.block()? });
                }
                let finally = if self.eat_word("finally") { Some(self.block()?) } else { None };
                if catches.is_empty() && finally.is_none() {
                    return Err(tr!("There is neither catch nor finally after try."));
                }
                Ok(Statement::Try { body, catches, finally })
            }
            "using" => {
                self.eat_word("using");
                if !self.eat('(') {
                    return Err(tr!("Wrap the resource of the using statement in ( ) (using var x = expression; can be written inside a block)."));
                }
                let resource = match self.local_declaration()? {
                    Some(declaration) => declaration,
                    None => Statement::Expr(self.expression()?),
                };
                if !self.eat(')') {
                    return Err(tr!("The parentheses of using are not closed with )."));
                }
                Ok(Statement::Using(Box::new(resource), vec![self.statement()?]))
            }
            "lock" => {
                self.eat_word("lock");
                let gate = self.parenthesized("lock")?;
                Ok(Statement::Lock(gate, Box::new(self.statement()?)))
            }
            "break" | "continue" => {
                self.eat_word(&word);
                self.end_of_statement()?;
                Ok(if word == "break" { Statement::Break } else { Statement::Continue })
            }
            "goto" | "yield" | "fixed" | "unsafe" => Err(tr!("The {word} statement in a lambda body is not supported.", word = word)),
            _ => {
                let statement = match self.local_declaration()? {
                    Some(declaration) => declaration,
                    None => Statement::Expr(self.expression()?),
                };
                self.end_of_statement()?;
                Ok(statement)
            }
        }
    }

    /// 文の終わりの `;`。ブロックの最後の文では省いてもよい（`x => { return x }` も、前と同じく読む）。
    pub(super) fn end_of_statement(&mut self) -> Result<(), String> {
        if self.eat(';') || self.peek_after_spaces() == Some('}') {
            return Ok(());
        }
        Err(tr!("A statement in the lambda body does not end with ;."))
    }

    /// `(式)`。`what` は、エラーに書く文の名前。
    pub(super) fn parenthesized(&mut self, what: &str) -> Result<Expr, String> {
        if !self.eat('(') {
            return Err(tr!("There is no ( after {word}.", word = what));
        }
        let inner = self.expression()?;
        if !self.eat(')') {
            return Err(tr!("The parentheses of {word} are not closed with ).", word = what));
        }
        Ok(inner)
    }

    /// `式, 式, …`（for の初期化と更新）。
    pub(super) fn expression_list(&mut self) -> Result<Vec<Expr>, String> {
        let mut items = vec![self.expression()?];
        while self.eat(',') {
            items.push(self.expression()?);
        }
        Ok(items)
    }

    /// 読む位置がローカル変数の宣言（`var x = 式`、`int x = 式`、`int x`、`int a = 1, b`、`const int k = 1`）なら、それを読む
    /// （終わりの `;` は読まない）。そうでなければ、読む位置を戻して `None`。
    pub(super) fn local_declaration(&mut self) -> Result<Option<Statement>, String> {
        // `await work;` は、`await` という型の変数 `work` の宣言ではない。
        if self.await_follows() {
            return Ok(None);
        }
        let saved = self.chars.clone();
        self.eat_word("const");
        let ty = if self.eat_word("var") { None } else {
            match self.type_syntax() {
                Some(ty) => Some(ty),
                None => {
                    self.chars = saved;
                    return Ok(None);
                }
            }
        };
        // 名前のあとは、`=`（`==` と `=>` ではない）か、`;` か、`,`。そうでなければ、宣言ではない（`a b` という式は無い）。
        let follows = |parser: &mut Self| {
            parser.skip_spaces();
            let mut ahead = parser.chars.clone();
            match ahead.next() {
                Some('=') => !matches!(ahead.next(), Some('=' | '>')),
                Some(';' | ',') => true,
                _ => false,
            }
        };
        let mut variables = Vec::new();
        loop {
            let Some(name) = self.designation().filter(|_| follows(self)) else {
                self.chars = saved;
                return Ok(None);
            };
            let initial = if self.eat('=') { Some(self.expression()?) } else { None };
            variables.push((name, initial));
            if !self.eat(',') {
                break;
            }
        }
        if ty.is_none() && variables.iter().any(|(_, initial)| initial.is_none()) {
            return Err(tr!("Give an initial value to a variable declared with var."));
        }
        Ok(Some(Statement::Local(ty, variables)))
    }
}
