//! 式の本体: 代入から、演算子の強さの順に、単項演算子、後ろに続く部分（.名前、添字、呼び出し）、一次式まで。

use super::*;

impl Parser<'_> {
    /// `(` のあとから `)` までの、`,` で区切った式の並び。`close` は閉じる文字。
    pub(super) fn arguments(&mut self, close: char) -> Result<Vec<Expr>, String> {
        let mut arguments = Vec::new();
        if self.eat(close) {
            return Ok(arguments);
        }
        loop {
            // `out 変数`、`out var x`、`out int x`、`out _`、`ref 変数`、`in 変数`（読み取りだけの参照渡し。`ref` と同じに扱う）。
            if close == ')' && matches!(self.peek_word().as_str(), "out" | "ref" | "in") {
                let is_out = self.eat_word("out") || !(self.eat_word("ref") || self.eat_word("in"));
                let target = if is_out && self.eat_word("_") {
                    OutTarget::Discard
                } else if is_out && self.peek_word() == "var" {
                    self.eat_word("var");
                    OutTarget::Declare(None, self.designation().ok_or_else(|| tr!("Write a variable name after out var."))?)
                } else {
                    // `out int x` か、`out 既存の変数`。
                    let start = self.chars.clone();
                    match (is_out, self.type_syntax(), self.designation()) {
                        (true, Some(ty), Some(name)) => OutTarget::Declare(Some(ty), name),
                        _ => {
                            self.chars = start;
                            OutTarget::Existing(Box::new(self.unary()?))
                        }
                    }
                };
                arguments.push(Expr::Out(target));
                if self.eat(close) {
                    return Ok(arguments);
                }
                if !self.eat(',') {
                    return Err(tr!("It is not closed with '{close}'.", close = close));
                }
                continue;
            }
            // `名前: 式` は、名前つき引数。
            self.skip_spaces();
            let start = self.chars.clone();
            let name = self.identifier();
            self.skip_spaces();
            let mut ahead = self.chars.clone();
            match (name, ahead.next(), ahead.next()) {
                (Some(name), Some(':'), next) if next != Some(':') && close == ')' => {
                    self.chars.next();
                    arguments.push(Expr::Named(name, Box::new(self.expression()?)));
                }
                _ => {
                    self.chars = start;
                    arguments.push(self.expression()?);
                }
            }
            if self.eat(close) {
                return Ok(arguments);
            }
            if !self.eat(',') {
                // 読めない書き方が続いているなら、括弧の話ではなく、その書き方を伝える。
                return Err(self.unsupported_here().unwrap_or_else(|| tr!("It is not closed with '{close}'.", close = close)));
            }
            // 配列やコレクションの初期化子は、最後の要素のあとにも `,` を書ける（`{ 1, 2, }`）。
            if close != ')' && self.eat(close) {
                return Ok(arguments);
            }
        }
    }

    /// `(` から始まる所が `(型)式` のキャストなら、それを読む。そうでなければ、読む位置を戻して `None`。
    ///
    /// `(x)` のあとに式の始まり（名前、数、文字列、`(`、`!`、`~`）が続けばキャスト。`(a) - b` のような引き算と
    /// 区別するため、`-` が続くときは、型がキーワード（`(int)-1`）のときだけキャストとする（C# の規則と同じ考え方）。
    pub(super) fn cast(&mut self) -> Option<Result<Expr, String>> {
        let saved = self.chars.clone();
        self.chars.next();
        let Some(ty) = self.type_syntax() else {
            self.chars = saved;
            return None;
        };
        self.skip_spaces();
        let closed = self.chars.next() == Some(')');
        self.skip_spaces();
        let is_keyword = is_keyword_type(&ty.name);
        let operand_follows = match self.chars.peek() {
            Some(c) if c.is_alphanumeric() || matches!(c, '_' | '"' | '\'' | '(' | '!' | '~' | '$' | '@') => true,
            Some('-') => is_keyword,
            // `(uint*)&x`、`(int)*p`: ポインターの型へのキャストか、型がキーワードのとき。
            Some('&' | '*') => ty.pointers > 0 || is_keyword,
            // `(List<int>)[1, 2]`: コレクション式のキャスト。`(a)[0]` は添字なので、型の名前とはっきり分かる形
            // （型引数つき、配列、キーワード）のときだけ。
            Some('[') => !ty.arguments.is_empty() || ty.array || is_keyword,
            _ => false,
        };
        // `(a) is …` や `(a) as …` は、括弧でくくった式のあとの演算子。
        let mut ahead = self.chars.clone();
        let word: String = std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric())).collect();
        if !closed || !operand_follows || matches!(word.as_str(), "is" | "as") {
            self.chars = saved;
            return None;
        }
        Some(self.unary().map(|operand| Expr::Cast(ty, Box::new(operand))))
    }

    pub(super) fn primary(&mut self) -> Result<Expr, String> {
        self.skip_spaces();
        match self.chars.peek().copied() {
            Some('(') => {
                if let Some(cast) = self.cast() {
                    return cast;
                }
                self.chars.next();
                // タプルの要素の名前（`(x: 1, y: 2)`）は、コンパイル時だけのもので、値には残らない。読み捨てる。
                let mut ahead = self.chars.clone();
                while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                let label: String = std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric() || *c == '_')).collect();
                while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                if !label.is_empty() && !label.starts_with(|c: char| c.is_ascii_digit()) && ahead.next() == Some(':') && ahead.peek() != Some(&':') {
                    self.chars = ahead;
                }
                let inner = self.expression()?;
                // `(a, b)` はタプル。
                if self.eat(',') {
                    let mut items = vec![inner];
                    for item in self.arguments(')')? {
                        items.push(match item {
                            Expr::Named(_, value) => *value,
                            other => other,
                        });
                    }
                    return Ok(Expr::Tuple(items));
                }
                if !self.eat(')') {
                    return Err(tr!("It is not closed with ')'."));
                }
                Ok(inner)
            }
            // コレクション式。
            Some('[') => {
                self.chars.next();
                let mut arguments = Vec::new();
                let mut items = Vec::new();
                if self.eat(']') {
                    return Ok(Expr::Collection { arguments, items });
                }
                loop {
                    self.skip_spaces();
                    let mut ahead = self.chars.clone();
                    if (ahead.next(), ahead.next()) == (Some('.'), Some('.')) {
                        self.chars = ahead;
                        items.push((true, self.conditional()?));
                    } else if items.is_empty() && arguments.is_empty() && self.peek_word() == "with" && self.chars.clone().skip_while(|c| c.is_whitespace()).nth(4) == Some('(') {
                        // C# 15: 最初の要素の `with(…)` は、コレクションを作るときの引数。
                        self.eat_word("with");
                        self.eat('(');
                        arguments = self.arguments(')')?;
                    } else {
                        items.push((false, self.expression()?));
                    }
                    if self.eat(']') {
                        return Ok(Expr::Collection { arguments, items });
                    }
                    if !self.eat(',') {
                        return Err(tr!("The collection expression is not closed with ]."));
                    }
                    // 最後の要素のあとの `,`。
                    if self.eat(']') {
                        return Ok(Expr::Collection { arguments, items });
                    }
                }
            }
            Some('"' | '@' | '$') if self.string_start().is_some() => {
                let (verbatim, dollars, quotes) = self.string_start().unwrap_or_default();
                let interpolated = dollars > 0;
                // 開きの印（@、$）と引用符を読み飛ばす。
                while self.chars.next_if(|c| matches!(c, '@' | '$')).is_some() {}
                for _ in 0..quotes {
                    self.chars.next();
                }
                let pieces = self.string_body(verbatim, dollars, quotes)?;
                if !interpolated {
                    let text: String = pieces.into_iter().map(|piece| if let Piece::Text(text) = piece { text } else { String::new() }).collect();
                    // 組にならないサロゲートを含む文字列は、符号単位の並びで持つ。
                    if let Some(units) = self.wide.take() {
                        return Ok(Expr::Utf16(units));
                    }
                    // `"text"u8`: UTF-8 の文字列リテラル。
                    let mut ahead = self.chars.clone();
                    if (ahead.next(), ahead.next()) == (Some('u'), Some('8')) && !ahead.peek().is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                        self.chars = ahead;
                        return Ok(Expr::Utf8(text));
                    }
                    return Ok(Expr::Str(text));
                }
                Ok(Expr::Interpolated(pieces))
            }
            // `@名前`: キーワードと同じつづりの名前（`@new`、`@class`）。
            Some('@') => {
                self.chars.next();
                match self.identifier() {
                    Some(name) => Ok(Expr::Name(name)),
                    None => Err(UNSUPPORTED.into()),
                }
            }
            Some('\'') => {
                self.chars.next();
                let unit = match self.chars.next() {
                    Some('\\') => self.escape()?,
                    Some(c) if c != '\'' => c as u32,
                    _ => return Err(tr!("The character literal is empty.")),
                };
                if self.chars.next() != Some('\'') {
                    return Err(tr!("The character literal is not closed with ' (only one character can be written)."));
                }
                // char は UTF-16 の符号単位一つ。それに収まらない文字（絵文字など）は、char では書けない。
                u16::try_from(unit).map(Expr::Char).map_err(|_| tr!("This character does not fit in one char. Write it as a string."))
            }
            // 数。`.5` のように、整数の部分を省いた小数も。
            Some(c) if c.is_ascii_digit() || (c == '.' && self.chars.clone().nth(1).is_some_and(|next| next.is_ascii_digit())) => {
                // 0x… と 0b… は、16 進と 2 進。桁の間の _ は、読みやすさのための区切り。
                let mut ahead = self.chars.clone();
                let radix = match (ahead.next(), ahead.next().map(|c| c.to_ascii_lowercase())) {
                    (Some('0'), Some('x')) => 16,
                    (Some('0'), Some('b')) => 2,
                    _ => 10,
                };
                if radix != 10 {
                    self.chars.next();
                    self.chars.next();
                }
                let mut digits = String::new();
                while let Some(c) = self.chars.next_if(|c| c.is_digit(radix) || *c == '_') {
                    if c != '_' {
                        digits.push(c);
                    }
                }
                // `.5` は 0.5。
                if digits.is_empty() && radix == 10 {
                    digits.push('0');
                }
                // 小数点は、10 進で、あとに数字が続くときだけ（`5.ToString()` の `.` はメンバーの参照）。
                let mut ahead = self.chars.clone();
                let fraction = radix == 10 && ahead.next() == Some('.') && ahead.next().is_some_and(|c| c.is_ascii_digit());
                if fraction {
                    digits.push(self.chars.next().unwrap_or_default());
                    while let Some(c) = self.chars.next_if(|c| c.is_ascii_digit() || *c == '_') {
                        if c != '_' {
                            digits.push(c);
                        }
                    }
                }
                // 指数（`1e5`、`2.5E-3`）。あとに数字が続くときだけ。付いていれば、浮動小数点。
                let mut ahead = self.chars.clone();
                let exponent = radix == 10
                    && matches!(ahead.next(), Some('e' | 'E'))
                    && match ahead.next() {
                        Some('+' | '-') => ahead.next().is_some_and(|c| c.is_ascii_digit()),
                        next => next.is_some_and(|c| c.is_ascii_digit()),
                    };
                if exponent {
                    digits.push('e');
                    self.chars.next();
                    while let Some(c) = self.chars.next_if(|c| c.is_ascii_digit() || matches!(c, '+' | '-')) {
                        digits.push(c);
                    }
                }
                let fraction = fraction || exponent;
                let mut suffix = String::new();
                while let Some(c) = self.chars.next_if(|c| c.is_ascii_alphabetic()) {
                    suffix.push(c.to_ascii_lowercase());
                }
                let unreadable = || tr!("{digits} cannot be read as a number.", digits = digits);
                let integer = || u64::from_str_radix(&digits, radix).map_err(|_| unreadable());
                match (suffix.as_str(), fraction) {
                    ("", true) | ("d", _) => digits.parse().map(Expr::Double).map_err(|_| unreadable()),
                    ("f", _) if radix == 10 => digits.parse().map(Expr::Float).map_err(|_| unreadable()),
                    // 接尾辞の無い整数は、int、uint、long、ulong のうち、収まる最初の型（C# と同じ）。
                    ("", false) => integer().map(|n| match (i32::try_from(n), u32::try_from(n), i64::try_from(n)) {
                        (Ok(n), _, _) => Expr::Int(n),
                        (_, Ok(n), _) => Expr::UInt(n),
                        (_, _, Ok(n)) => Expr::Long(n),
                        _ => Expr::ULong(n),
                    }),
                    ("u", false) => integer().map(|n| u32::try_from(n).map_or(Expr::ULong(n), Expr::UInt)),
                    ("l", false) => integer().map(|n| i64::try_from(n).map_or(Expr::ULong(n), Expr::Long)),
                    ("ul" | "lu", false) => integer().map(Expr::ULong),
                    ("m", _) if radix == 10 => Ok(Expr::Decimal(digits.clone())),
                    (other, _) => Err(tr!("The number suffix {suffix} is not supported.", suffix = other)),
                }
            }
            _ => match self.identifier().as_deref() {
                Some("true") => Ok(Expr::Bool(true)),
                Some("false") => Ok(Expr::Bool(false)),
                Some("null") => Ok(Expr::Null),
                Some("new") => self.creation(),
                // `stackalloc int[4]`: 書き方は配列の生成と同じ。
                Some("stackalloc") => Ok(Expr::StackAlloc(Box::new(self.creation()?))),
                Some("delegate") if matches!(self.peek_after_spaces(), Some('(' | '{')) => self.anonymous_method(),
                Some("throw") => Err(tr!("A throw expression can be written only on the right of ??, in a branch of the conditional operator, or in a lambda body (as in C#).")),
                // `from x in …` は、クエリ式（from という名前の変数ではない）。
                Some("from") if self.query_follows() => self.query(),
                Some(word @ ("checked" | "unchecked")) if self.chars.clone().find(|c| !c.is_whitespace()) == Some('(') => {
                    self.eat('(');
                    let inner = self.expression()?;
                    if !self.eat(')') {
                        return Err(tr!("The parentheses of {word} are not closed with ).", word = word));
                    }
                    // unchecked は、実行時には既定と同じ（あふれは折り返す）。違うのは、定数どうしの計算があふれてもよいこと。
                    Ok(Expr::Checked(word == "checked", Box::new(inner)))
                }
                Some("typeof") => Ok(Expr::TypeOf(self.type_argument("typeof")?)),
                // `default(型)`。括弧の無い `default` は、型を周りから決める（`x == default`、`F(default)`）。
                Some("default") if self.peek_after_spaces() == Some('(') => Ok(Expr::Default(self.type_argument("default")?)),
                Some("default") => Ok(Expr::DefaultLiteral),
                Some("sizeof") => Ok(Expr::SizeOf(self.type_argument("sizeof")?)),
                // nameof(式) は、式のいちばん後ろの名前を文字列にする。式そのものは評価しない。
                Some("nameof") => {
                    if !self.eat('(') {
                        return Err(tr!("There is no ( after nameof."));
                    }
                    // `nameof(List<>)`、`nameof(Dictionary<int, string>)`: 型引数つきの型の名前。
                    let start = self.chars.clone();
                    if let Some(ty) = self.type_syntax().filter(|ty| !ty.arguments.is_empty()) {
                        if self.eat(')') {
                            return Ok(Expr::Str(ty.name.rsplit('.').next().unwrap_or(&ty.name).to_string()));
                        }
                    }
                    self.chars = start;
                    let inner = self.expression()?;
                    if !self.eat(')') {
                        return Err(tr!("The parentheses of nameof are not closed with )."));
                    }
                    match inner {
                        Expr::Name(name) | Expr::Member(_, name) => Ok(Expr::Str(name.rsplit('.').next().unwrap_or(&name).to_string())),
                        _ => Err(tr!("Write a name inside the parentheses of nameof.")),
                    }
                }
                // 型のキーワード（`int.MaxValue`、`string.Empty`）は、その型の名前として読む。
                Some(word) if keyword_type(word).is_some() => Ok(Expr::Name(keyword_type(word).unwrap_or_default().to_string())),
                Some(name) if !name.starts_with(|c: char| c.is_ascii_digit()) => Ok(Expr::Name(name.to_string())),
                _ => Err(self.unsupported_here().unwrap_or_else(|| UNSUPPORTED.into())),
            },
        }
    }

    /// 式を一つ読む。いちばん弱いのは代入で、右から結びつく（`a = b = c` は `a = (b = c)`）。
    pub(super) fn expression(&mut self) -> Result<Expr, String> {
        if let Some(lambda) = self.lambda() {
            return lambda;
        }
        if let Some(deconstruction) = self.deconstruction() {
            return deconstruction;
        }
        // 長い記号を先に試す。
        const ASSIGNMENTS: [(&str, Assignment); 13] = [
            (">>>=", Assignment::Compound(BinaryOp::ShiftRightUnsigned)),
            ("<<=", Assignment::Compound(BinaryOp::ShiftLeft)),
            (">>=", Assignment::Compound(BinaryOp::ShiftRight)),
            ("??=", Assignment::Coalesce),
            ("+=", Assignment::Compound(BinaryOp::Add)),
            ("-=", Assignment::Compound(BinaryOp::Subtract)),
            ("*=", Assignment::Compound(BinaryOp::Multiply)),
            ("/=", Assignment::Compound(BinaryOp::Divide)),
            ("%=", Assignment::Compound(BinaryOp::Remainder)),
            ("&=", Assignment::Compound(BinaryOp::BitAnd)),
            ("|=", Assignment::Compound(BinaryOp::BitOr)),
            ("^=", Assignment::Compound(BinaryOp::BitXor)),
            ("=", Assignment::Plain),
        ];
        let target = self.conditional()?;
        self.skip_spaces();
        let rest: String = self.chars.clone().take(4).collect();
        // `==` は比較、`=>` はラムダ。
        let Some(&(text, kind)) = ASSIGNMENTS.iter().find(|(text, _)| rest.starts_with(text) && !(*text == "=" && (rest.starts_with("==") || rest.starts_with("=>")))) else {
            return Ok(target);
        };
        // `a?.b = c`（C# 14 の null 条件の代入）も、左辺に書ける。
        let null_conditional = matches!(&target, Expr::NullConditional(_, rest) if matches!(**rest, Expr::Member(..) | Expr::Index(..)));
        // `*p = v`（ポインターの指す先）も書ける。
        if !matches!(target, Expr::Name(_) | Expr::Member(..) | Expr::Index(..) | Expr::Dereference(_)) && !null_conditional {
            return Err(tr!("Write a variable, field, property or indexer on the left side of the assignment."));
        }
        for _ in 0..text.len() {
            self.chars.next();
        }
        Ok(Expr::Assign(Box::new(target), kind, Box::new(self.expression()?)))
    }

    /// 条件演算子と、それより強い演算子。
    pub(super) fn conditional(&mut self) -> Result<Expr, String> {
        let condition = self.coalesce()?;
        // 条件演算子は、右から結びつく（`a ? b : c ? d : e` は `a ? b : (c ? d : e)`）。`??=` の `?` は、条件演算子ではない。
        self.skip_spaces();
        let mut ahead = self.chars.clone();
        if (ahead.next(), ahead.next()) == (Some('?'), Some('?')) || !self.eat('?') {
            return Ok(condition);
        }
        let when_true = self.throwable()?;
        if !self.eat(':') {
            return Err(tr!("The ':' of the conditional operator is missing (condition ? when true : when false)."));
        }
        Ok(Expr::Conditional(Box::new(condition), Box::new(when_true), Box::new(self.throwable()?)))
    }

    /// `throw 式` を書ける場所の式。`throw` でなければ、ふつうの式。
    pub(super) fn throwable(&mut self) -> Result<Expr, String> {
        if self.eat_word("throw") {
            return Ok(Expr::Throw(Box::new(self.coalesce()?)));
        }
        self.expression()
    }

    /// `??` は `||` より弱く、右から結びつく。
    pub(super) fn coalesce(&mut self) -> Result<Expr, String> {
        let left = self.binary(0)?;
        self.skip_spaces();
        let mut ahead = self.chars.clone();
        // `??=` は代入。ここでは読まない。
        if (ahead.next(), ahead.next(), ahead.next()) == (Some('?'), Some('?'), Some('=')) {
            return Ok(left);
        }
        let mut ahead = self.chars.clone();
        if (ahead.next(), ahead.next()) != (Some('?'), Some('?')) {
            return Ok(left);
        }
        self.chars.next();
        self.chars.next();
        if self.eat_word("throw") {
            return Ok(Expr::Coalesce(Box::new(left), Box::new(Expr::Throw(Box::new(self.coalesce()?)))));
        }
        Ok(Expr::Coalesce(Box::new(left), Box::new(self.coalesce()?)))
    }

    pub(super) fn binary(&mut self, level: usize) -> Result<Expr, String> {
        // 優先順位の低い順。同じ段の演算子は、左から結びつく。長い記号を先に試す（`<=` を `<` と読まないように）。
        const LEVELS: [&[(&str, BinaryOp)]; 10] = [
            &[("||", BinaryOp::Or)],
            &[("&&", BinaryOp::And)],
            &[("|", BinaryOp::BitOr)],
            &[("^", BinaryOp::BitXor)],
            &[("&", BinaryOp::BitAnd)],
            &[("==", BinaryOp::Equal), ("!=", BinaryOp::NotEqual)],
            &[("<=", BinaryOp::LessOrEqual), (">=", BinaryOp::GreaterOrEqual), ("<", BinaryOp::Less), (">", BinaryOp::Greater)],
            &[("<<", BinaryOp::ShiftLeft), (">>>", BinaryOp::ShiftRightUnsigned), (">>", BinaryOp::ShiftRight)],
            &[("+", BinaryOp::Add), ("-", BinaryOp::Subtract)],
            &[("*", BinaryOp::Multiply), ("/", BinaryOp::Divide), ("%", BinaryOp::Remainder)],
        ];
        /// `is` と `as` は、比較（`<` など）と同じ強さ。
        const RELATIONAL: usize = 6;
        let Some(operators) = LEVELS.get(level) else { return self.switch_level() };
        let mut left = self.binary(level + 1)?;
        loop {
            self.skip_spaces();
            if level == RELATIONAL {
                let mut ahead = self.chars.clone();
                let word: String = std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric() || *c == '_')).collect();
                match word.as_str() {
                    "is" => {
                        self.chars = ahead;
                        let pattern = self.pattern()?;
                        // `x is int?` は型の検査として書けるが、パターンの中（`not int?`、`int? or …`）には書けない。
                        if !matches!(pattern, Pattern::TypeOrConstant(..)) && nullable_type_inside(&pattern) {
                            return Err(tr!("A nullable type (int?, for example) cannot be written in a pattern. Write the underlying type (int) (as in C#)."));
                        }
                        left = Expr::Is(Box::new(left), pattern);
                        continue;
                    }
                    "as" => {
                        self.chars = ahead;
                        let ty = self.type_syntax().ok_or_else(|| tr!("Write a type name after as."))?;
                        left = Expr::As(Box::new(left), ty);
                        continue;
                    }
                    _ => {}
                }
            }
            let rest: String = self.chars.clone().take(3).collect();
            // 一文字の演算子は、同じ記号がもう一つ続くなら別の演算子（`&&`、`||`、`<<`、`>>`、`++`、`--`）なので、ここでは読まない。
            // 複合代入（`+=`、`<<=` など）も、二項演算子としては読まない（代入は、上の `expression` が読む）。
            let mut next = rest.chars();
            let doubled = next.next().is_some_and(|first| next.next() == Some(first));
            let Some(&(text, op)) = operators.iter().find(|(text, _)| {
                rest.starts_with(text) && !(text.len() == 1 && doubled) && !(matches!(*text, "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "<<" | ">>" | ">>>") && rest[text.len()..].starts_with('='))
            }) else {
                return Ok(left);
            };
            for _ in 0..text.len() {
                self.chars.next();
            }
            left = Expr::Binary(op, Box::new(left), Box::new(self.binary(level + 1)?));
        }
    }

    /// `switch` 式と `with` 式。掛け算より強く、範囲（`..`）より弱い。
    pub(super) fn switch_level(&mut self) -> Result<Expr, String> {
        let mut expr = self.range_level()?;
        loop {
            let word = self.peek_word();
            if !matches!(word.as_str(), "switch" | "with") || self.chars.clone().skip_while(|c| c.is_whitespace()).skip(word.len()).find(|c| !c.is_whitespace()) != Some('{') {
                return Ok(expr);
            }
            self.eat_word(&word);
            self.eat('{');
            if word == "with" {
                let mut members = Vec::new();
                loop {
                    if self.eat('}') {
                        break;
                    }
                    let name = self.identifier().ok_or_else(|| tr!("Write member names inside with."))?;
                    if !self.eat('=') {
                        return Err(tr!("There is no = after a name inside with."));
                    }
                    members.push((name, self.conditional()?));
                    if self.eat('}') {
                        break;
                    }
                    if !self.eat(',') {
                        return Err(tr!("with is not closed with }."));
                    }
                }
                expr = Expr::With(Box::new(expr), members);
                continue;
            }
            let mut arms = Vec::new();
            loop {
                if self.eat('}') {
                    break;
                }
                let pattern = self.pattern()?;
                // 条件は、ラムダ式としては読まない（あとに続く `=>` は、腕の矢印）。
                let guard = if self.eat_word("when") { Some(self.conditional()?) } else { None };
                self.skip_spaces();
                let mut ahead = self.chars.clone();
                if (ahead.next(), ahead.next()) != (Some('='), Some('>')) {
                    return Err(tr!("An arm of the switch expression has no => (pattern => result)."));
                }
                self.chars = ahead;
                // 腕の結果には、throw 式も書ける（`_ => throw new …`）。
                arms.push(Arm { pattern, guard, result: self.throwable()? });
                if self.eat('}') {
                    break;
                }
                if !self.eat(',') {
                    return Err(tr!("The switch expression is not closed with }."));
                }
            }
            expr = Expr::Switch(Box::new(expr), arms);
        }
    }

    /// 範囲（`a..b`、`..b`、`a..`、`..`）。単項演算子より弱い。
    pub(super) fn range_level(&mut self) -> Result<Expr, String> {
        self.skip_spaces();
        let starts_with_dots = |parser: &Self| {
            let mut ahead = parser.chars.clone();
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
            (ahead.next(), ahead.next()) == (Some('.'), Some('.'))
        };
        let start = if starts_with_dots(self) { None } else { Some(Box::new(self.unary()?)) };
        if !starts_with_dots(self) {
            return start.map(|start| *start).ok_or_else(|| UNSUPPORTED.into());
        }
        self.skip_spaces();
        self.chars.next();
        self.chars.next();
        let end = match self.peek_after_spaces() {
            None | Some(')' | ']' | ',' | '}') => None,
            _ => Some(Box::new(self.unary()?)),
        };
        Ok(Expr::Range(start, end))
    }

    /// 読む位置が `await 式` の `await` か。`await` という名前の変数（`await.Count`、`await + 1`）ではなく、
    /// あとに式の始まりが続くときだけ。
    pub(super) fn await_follows(&self) -> bool {
        if self.peek_word() != "await" {
            return false;
        }
        let mut ahead = self.chars.clone();
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        for _ in 0.."await".len() {
            ahead.next();
        }
        let spaced = ahead.peek().is_some_and(|c| c.is_whitespace());
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        let word: String = ahead.clone().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        match ahead.peek() {
            // `await is T`、`await as T` などは、変数の `await`。
            Some(c) if c.is_alphabetic() || *c == '_' => spaced && !matches!(word.as_str(), "is" | "as" | "switch" | "with" | "and" | "or" | "in" | "equals" | "select" | "where" | "orderby" | "group" | "into" | "on" | "by" | "ascending" | "descending"),
            Some('(' | '"' | '$' | '@' | '\'' | '!' | '~') => true,
            Some(c) => spaced && c.is_ascii_digit(),
            None => false,
        }
    }

    pub(super) fn unary(&mut self) -> Result<Expr, String> {
        self.skip_spaces();
        if self.await_follows() {
            self.eat_word("await");
            return Ok(Expr::Await(Box::new(self.unary()?)));
        }
        let mut ahead = self.chars.clone();
        match (ahead.next(), ahead.next()) {
            // `!=` は二項演算子。ここに来るのは `!式` だけ。
            (Some('!'), next) if next != Some('=') => {
                self.chars.next();
                Ok(Expr::Unary(UnaryOp::Not, Box::new(self.unary()?)))
            }
            // `++x` と `--x`。
            (Some(sign @ ('+' | '-')), Some(next)) if next == sign => {
                self.chars.next();
                self.chars.next();
                let target = self.unary()?;
                if !matches!(target, Expr::Name(_) | Expr::Member(..) | Expr::Index(..)) {
                    return Err(tr!("Put {operator} on a variable, field, property or indexer.", operator = format!("{sign}{sign}")));
                }
                Ok(Expr::Increment { target: Box::new(target), decrement: sign == '-', prefix: true })
            }
            (Some('-'), next) if next != Some('-') => {
                self.chars.next();
                // `-2147483648` と `-9223372036854775808` は、それぞれ int と long の最小値のリテラル（C# の規則。
                // 符号を付けない 2147483648 は uint で、その符号反転は long になってしまう）。
                let mut ahead = self.chars.clone();
                while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                let digits: String = std::iter::from_fn(|| ahead.next_if(char::is_ascii_digit)).collect();
                if !ahead.peek().is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '.') {
                    match digits.as_str() {
                        "2147483648" => {
                            self.chars = ahead;
                            return Ok(Expr::Int(i32::MIN));
                        }
                        "9223372036854775808" => {
                            self.chars = ahead;
                            return Ok(Expr::Long(i64::MIN));
                        }
                        _ => {}
                    }
                }
                Ok(Expr::Unary(UnaryOp::Negate, Box::new(self.unary()?)))
            }
            (Some('+'), next) if next != Some('+') => {
                self.chars.next();
                Ok(Expr::Unary(UnaryOp::Plus, Box::new(self.unary()?)))
            }
            (Some('~'), _) => {
                self.chars.next();
                Ok(Expr::Unary(UnaryOp::Complement, Box::new(self.unary()?)))
            }
            // `^n`: 終わりから数えた位置。
            (Some('^'), _) => {
                self.chars.next();
                Ok(Expr::FromEnd(Box::new(self.unary()?)))
            }
            // `&x`（アドレス）と `*p`（指す先）。二項の `&` と `*` は、ここへ来る前に読まれている。
            (Some('&'), next) if next != Some('&') => {
                self.chars.next();
                Ok(Expr::AddressOf(Box::new(self.unary()?)))
            }
            (Some('*'), _) => {
                self.chars.next();
                Ok(Expr::Dereference(Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    pub(super) fn postfix(&mut self) -> Result<Expr, String> {
        let start = self.primary()?;
        self.postfix_chain(start)
    }

    /// 式のあとに続く `.名前`、`[添字]`、`(引数)`、`?.`、`?[` を読む。
    pub(super) fn postfix_chain(&mut self, mut expr: Expr) -> Result<Expr, String> {
        loop {
            // `?.` と `?[` は、左が null ならそこから先を全部やめる。続きを一まとまりとして読む。
            self.skip_spaces();
            // `?` と `.` の間に空白や改行があってもよい（`a?` 改行 `.b`）。あとに名前が続くときだけ（条件演算子の
            // `a ? .5 : b` ではないこと）。
            let mut ahead = self.chars.clone();
            if ahead.next() == Some('?') && ahead.peek().is_some_and(|c| c.is_whitespace()) {
                while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                if ahead.next() == Some('.') {
                    while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                    if ahead.peek().is_some_and(|c| c.is_alphabetic() || *c == '_') {
                        self.chars = ahead;
                        let first = Expr::Member(Box::new(Expr::Receiver), self.identifier().ok_or(UNSUPPORTED)?);
                        return Ok(Expr::NullConditional(Box::new(expr), Box::new(self.postfix_chain(first)?)));
                    }
                }
            }
            let mut ahead = self.chars.clone();
            match (ahead.next(), ahead.next(), ahead.next()) {
                // `a ?.5 : b`（条件演算子と小数）ではないこと。
                (Some('?'), Some('.'), next) if !next.is_some_and(|c| c.is_ascii_digit()) => {
                    self.chars.next();
                    self.chars.next();
                    self.skip_spaces();
                    let first = Expr::Member(Box::new(Expr::Receiver), self.identifier().ok_or(UNSUPPORTED)?);
                    return Ok(Expr::NullConditional(Box::new(expr), Box::new(self.postfix_chain(first)?)));
                }
                (Some('?'), Some('['), _) => {
                    self.chars.next();
                    self.chars.next();
                    let first = Expr::Index(Box::new(Expr::Receiver), self.arguments(']')?);
                    return Ok(Expr::NullConditional(Box::new(expr), Box::new(self.postfix_chain(first)?)));
                }
                // `x++` と `x--`。
                (Some(sign @ ('+' | '-')), Some(next), _) if next == sign && matches!(expr, Expr::Name(_) | Expr::Member(..) | Expr::Index(..)) => {
                    self.chars.next();
                    self.chars.next();
                    return Ok(Expr::Increment { target: Box::new(expr), decrement: sign == '-', prefix: false });
                }
                // `x!`（null でないことの宣言）は、コンパイラへの注釈で、値は変わらない。読み飛ばす。`!=` は別の演算子。
                (Some('!'), next, _) if next != Some('=') => {
                    self.chars.next();
                    continue;
                }
                _ => {}
            }
            // `名前<型, …>(` は、型引数を付けた呼び出し。`<` のあとが型の並びで、`>` のすぐあとに `(` が続くときだけ。
            if matches!(expr, Expr::Name(_) | Expr::Member(..)) && self.chars.peek() == Some(&'<') {
                let saved = self.chars.clone();
                self.chars.next();
                let mut types = Vec::new();
                // `名前<>` / `名前<,>`: 型引数を書かないジェネリック型（`nameof(Map<,>.Count)` の中などに書ける）。
                let mut open = self.chars.clone();
                let commas = std::iter::from_fn(|| open.next_if(|c| *c == ',' || c.is_whitespace())).filter(|c| *c == ',').count();
                if open.peek() == Some(&'>') {
                    types = vec![TypeSyntax::default(); commas + 1];
                    self.chars = open;
                }
                while let Some(ty) = self.type_syntax() {
                    types.push(ty);
                    self.skip_spaces();
                    if self.chars.next_if_eq(&',').is_none() {
                        break;
                    }
                }
                self.skip_spaces();
                if !types.is_empty() && self.chars.next_if_eq(&'>').is_some() {
                    let mut ahead = self.chars.clone();
                    while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                    match (ahead.next(), ahead.next()) {
                        (Some('('), _) => {
                            self.eat('(');
                            expr = Expr::Call(Box::new(Expr::Generic(Box::new(expr), types)), self.arguments(')')?);
                            continue;
                        }
                        // `名前<型>.メンバー` は、ジェネリック型の静的メンバー（`Comparer<int>.Default`）。
                        (Some('.'), Some(c)) if c.is_alphabetic() || c == '_' || c.is_whitespace() => {
                            self.eat('.');
                            self.skip_spaces();
                            expr = Expr::Member(Box::new(Expr::Generic(Box::new(expr), types)), self.identifier().ok_or(UNSUPPORTED)?);
                            continue;
                        }
                        // 呼び出しでも、メンバーの参照でもない `名前<型>` は、メソッドの集まり（デリゲートの引数に渡すもの）。
                        // 比較（`a < b > c`）と見分けるため、あとが式の終わりのときだけ。
                        (None | Some(')' | ',' | ';' | ']' | '}' | ':'), _) => {
                            expr = Expr::Generic(Box::new(expr), types);
                            continue;
                        }
                        _ => {}
                    }
                }
                self.chars = saved;
            }
            // `..` は範囲で、メンバーの参照ではない。
            let mut ahead = self.chars.clone();
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
            let range_follows = (ahead.next(), ahead.next()) == (Some('.'), Some('.'));
            // `p->名前` は `(*p).名前`。
            let mut arrow = self.chars.clone();
            if (arrow.next(), arrow.next()) == (Some('-'), Some('>')) {
                self.chars = arrow;
                self.skip_spaces();
                expr = Expr::Member(Box::new(Expr::Dereference(Box::new(expr))), self.identifier().ok_or(UNSUPPORTED)?);
                continue;
            }
            if !range_follows && self.eat('.') {
                self.skip_spaces();
                expr = Expr::Member(Box::new(expr), self.identifier().ok_or(UNSUPPORTED)?);
            } else if self.eat('[') {
                expr = Expr::Index(Box::new(expr), self.arguments(']')?);
            } else if self.eat('(') {
                // 名前で指したメソッドのほか、デリゲートの値も呼べる（`f(1)(2)`、`((Func<int, int>)(x => x))(1)`）。
                expr = Expr::Call(Box::new(expr), self.arguments(')')?);
            } else {
                return Ok(expr);
            }
        }
    }
}
