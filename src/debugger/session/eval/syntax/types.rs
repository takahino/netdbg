//! 型の名前: 名前.名前、型引数、タプル型、あとに付く ?、*、配列の次元。

use super::*;

impl Parser<'_> {
    /// 型の名前を読む: `名前.名前…`、型引数 `<…>`、タプル型 `(型, 型)`、あとに `?`、`*`、`[]` / `[,]`。
    /// 読めなければ、読む位置を戻して `None`。
    pub(super) fn type_syntax(&mut self) -> Option<TypeSyntax> {
        let saved = self.chars.clone();
        self.skip_spaces();
        let (name, arguments) = if self.chars.peek() == Some(&'(') { self.tuple_type().map(|elements| ("System.ValueTuple".to_string(), elements)).unwrap_or_default() } else { self.type_name() };
        if name.is_empty() {
            self.chars = saved;
            return None;
        }
        let nullable = self.question_suffix_follows();
        if nullable {
            self.chars.next();
        }
        // ポインターの `*`。掛け算と見分けるため、あとに `)`、`*`、`[`、`>`、`,` が続くときだけ。
        let mut pointers = 0;
        loop {
            let mut ahead = self.chars.clone();
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
            if ahead.next() != Some('*') {
                break;
            }
            let after = ahead.clone();
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
            if !matches!(ahead.peek(), Some(')' | '*' | '[' | '>' | ',')) {
                break;
            }
            self.chars = after;
            pointers += 1;
        }
        let shape = self.rank_specifiers();
        Some(TypeSyntax { name, nullable, pointers, arguments, ..TypeSyntax::default() }.with_shape(&shape))
    }

    /// 読む位置の `?` が、型の末尾の印（null 許容）か。`??` や `?.` や `?[`（添字）の始まりではないこと。
    /// ただし `?[]` と `?[,]` は、null 許容の要素の配列。
    pub(super) fn question_suffix_follows(&self) -> bool {
        let mut ahead = self.chars.clone();
        ahead.next() == Some('?')
            && match ahead.next() {
                Some('?' | '.') => false,
                Some('[') => matches!(ahead.next(), Some(']' | ',')),
                _ => true,
            }
    }

    /// 配列の次元の並び（`[]`、`[,]`。続けて書けば、配列の配列）。添字（`[0]`）は、中身があるので読まない。
    pub(super) fn rank_specifiers(&mut self) -> Vec<u32> {
        let mut shape = Vec::new();
        loop {
            let mut ahead = self.chars.clone();
            while ahead.next_if(|c| c.is_whitespace()).is_some() {}
            if ahead.next() != Some('[') {
                return shape;
            }
            let mut rank = 1;
            loop {
                while ahead.next_if(|c| c.is_whitespace()).is_some() {}
                match ahead.next() {
                    Some(',') => rank += 1,
                    Some(']') => break,
                    _ => return shape,
                }
            }
            self.chars = ahead;
            shape.push(rank);
            // 配列のあとの `?`（`int[]?`）は、null を許すという注釈で、型は変わらない。
            if self.question_suffix_follows() {
                self.chars.next();
            }
        }
    }

    /// 型の名前と型引数（`System.Collections.Generic.Dictionary<string, int>.Enumerator`）。読めなければ、名前が空。
    /// ジェネリック型の入れ子の型は、名前をつなぎ、型引数を並べて持つ（CLR での持ち方と同じ）。
    pub(super) fn type_name(&mut self) -> (String, Vec<TypeSyntax>) {
        let (mut name, mut arguments) = (String::new(), Vec::new());
        loop {
            let mut part = String::new();
            loop {
                // コンパイラが作った名前は、`<…>` で始まる（`<>f__AnonymousType0`）。
                if self.generated_names && self.chars.peek() == Some(&'<') {
                    let mut ahead = self.chars.clone();
                    let mut generated = String::new();
                    for c in ahead.by_ref() {
                        generated.push(c);
                        if c == '>' {
                            break;
                        }
                    }
                    if generated.ends_with('>') {
                        self.chars = ahead;
                        part.push_str(&generated);
                    }
                }
                match self.identifier() {
                    Some(word) if !word.starts_with(|c: char| c.is_ascii_digit() || c == '$') => part.push_str(&word),
                    _ => return (String::new(), Vec::new()),
                }
                // `.` のあとに名前が続くときだけ、型の名前の続きとして読む。
                let mut ahead = self.chars.clone();
                if ahead.next() == Some('.') && ahead.next().is_some_and(|c| c.is_alphabetic() || c == '_') {
                    self.chars.next();
                    part.push('.');
                } else {
                    break;
                }
            }
            if name.is_empty() {
                name = keyword_type(&part).map_or(part, str::to_string);
            } else {
                name = format!("{name}.{part}");
            }
            // 型引数。`<` のあとが型の並びで、`>` で閉じているときだけ（そうでなければ、比較の `<`）。
            let before_arguments = self.chars.clone();
            let mut own = Vec::new();
            if self.chars.next_if_eq(&'<').is_some() {
                loop {
                    match self.type_syntax() {
                        Some(argument) => own.push(argument),
                        // 型引数を書かない `<,>` は、開いたジェネリック型（typeof の中でだけ意味がある）。名前の空の型で表す。
                        None if matches!(self.peek_after_spaces(), Some(',' | '>')) && own.iter().all(|a: &TypeSyntax| a.name.is_empty()) => own.push(TypeSyntax::default()),
                        None => break,
                    }
                    self.skip_spaces();
                    if self.chars.next_if_eq(&',').is_none() {
                        break;
                    }
                }
                self.skip_spaces();
                if own.is_empty() || self.chars.next_if_eq(&'>').is_none() {
                    own.clear();
                    self.chars = before_arguments;
                }
            }
            let generic = !own.is_empty();
            arguments.extend(own);
            // `Outer<T>.Inner`: 型引数のあとの `.名前` は、入れ子の型。
            let mut ahead = self.chars.clone();
            if generic && ahead.next() == Some('.') && ahead.next().is_some_and(|c| c.is_alphabetic() || c == '_') {
                self.chars.next();
            } else {
                return (name, arguments);
            }
        }
    }

    /// タプル型（`(int, string)`、`(int From, int To)`）の要素の型。要素の名前は、型には入らないので読み捨てる。
    /// タプル型でなければ `None`（読む位置は、呼び出し側が戻す）。
    pub(super) fn tuple_type(&mut self) -> Option<Vec<TypeSyntax>> {
        self.chars.next();
        let mut elements = Vec::new();
        loop {
            elements.push(self.type_syntax()?);
            let _ = self.designation();
            self.skip_spaces();
            match self.chars.next() {
                Some(',') => {}
                Some(')') => break,
                _ => return None,
            }
        }
        (elements.len() >= 2).then_some(elements)
    }

    /// `typeof(型)` などの、括弧の中に型を一つ書く式。
    pub(super) fn type_argument(&mut self, keyword: &str) -> Result<TypeSyntax, String> {
        if !self.eat('(') {
            return Err(tr!("There is no ( after {word}.", word = keyword));
        }
        let ty = self.type_syntax().ok_or_else(|| tr!("Write a type name inside the parentheses of {word}.", word = keyword))?;
        if !self.eat(')') {
            return Err(tr!("The parentheses of {word} are not closed with ).", word = keyword));
        }
        Ok(ty)
    }
}

/// 型の名前（`System.Func<int, bool>`、`string`）を読む。全体が型の名前でなければ `None`。
pub(in crate::debugger::session) fn parse_type(text: &str) -> Option<TypeSyntax> {
    let mut parser = Parser { generated_names: true, ..Parser::new(text) };
    let ty = parser.type_syntax()?;
    parser.skip_spaces();
    parser.chars.peek().is_none().then_some(ty)
}

/// リフレクションの型の名前（`Type.FullName`）を、式に書く型の形にする。
/// ``System.Linq.IGrouping`2[[System.Int32, System.Private.CoreLib, …],[System.String, …]]`` は `System.Linq.IGrouping<int, string>`、
/// `Outer+Inner` は `Outer.Inner`、``System.Nullable`1[[System.Int32, …]]`` は `int?`、末尾の `[]` は配列。読めない形は `None`。
pub(in crate::debugger::session) fn parse_reflection_type(text: &str) -> Option<TypeSyntax> {
    let text = text.trim();
    let (text, array) = match text.strip_suffix("[]") {
        Some(rest) => (rest, true),
        None => (text, false),
    };
    let (head, arguments) = match text.find("[[") {
        Some(open) => (&text[..open], split_top_level(text[open + 1..].strip_suffix(']')?, ',')),
        None => (text, Vec::new()),
    };
    // 多次元の配列（`[,]`）や、配列の配列は、まだ扱わない。
    if head.contains('[') || head.contains(']') {
        return None;
    }
    // 型引数の数（`` `2 ``）を除き、入れ子の型の `+` を `.` にする。
    let mut name = String::new();
    let mut chars = head.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => while chars.next_if(char::is_ascii_digit).is_some() {},
            '+' => name.push('.'),
            c => name.push(c),
        }
    }
    let mut parsed = Vec::new();
    for argument in arguments {
        // `[型の名前, アセンブリ, Version=…]` の、型の名前の部分。
        let inner = argument.trim().strip_prefix('[')?.strip_suffix(']')?;
        let type_part = split_top_level(inner, ',').into_iter().next()?;
        parsed.push(parse_reflection_type(type_part)?);
    }
    if name == "System.Nullable" && parsed.len() == 1 && !parsed[0].array {
        return Some(TypeSyntax { nullable: true, array, ..parsed.remove(0) });
    }
    Some(TypeSyntax { name, array, arguments: parsed, ..TypeSyntax::default() })
}

/// 括弧の外にある `separator` で分ける。
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (index, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            c if c == separator && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}
