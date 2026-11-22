//! 式の構文。C# の式の文字列を、構文木（`Expr`）にする。意味（名前が何を指すか、どのオーバーロードか）は見ない。
//!
//! 対応していない書き方は、できるだけその書き方の名前を入れたエラーにする（`unsupported_here`）。

//!
//! | ファイル | 中身 |
//! |---|---|
//! | `ast.rs` | 構文木の型（`Expr`、`Statement`、`Pattern`、`TypeSyntax` など） |
//! | `expressions.rs` | 代入から一次式まで（演算子の強さの順） |
//! | `types.rs` | 型の名前 |
//! | `strings.rs` | 文字列のリテラル |
//! | `creation.rs` | `new` と初期化子 |
//! | `patterns.rs` | パターン |
//! | `lambdas.rs` | ラムダ式、本体の文、分解の代入 |
//! | `query.rs` | クエリ式 |
//! | `walk.rs` | 構文木をたどる（式の中の名前の参照を探す） |
//!
//! 読み取りは、文字を 1 つずつ見ていく再帰下降（字句に分ける段は無い）。読めなかったときに読む位置を戻せるように、
//! 文字の並びの写し（`self.chars.clone()`）を取ってから先を覗く。

use super::super::operators::{BinaryOp, UnaryOp};

mod ast;
mod creation;
mod expressions;
mod lambdas;
mod patterns;
mod query;
mod strings;
#[cfg(test)]
mod tests;
mod types;
mod walk;

pub(in crate::debugger::session) use ast::*;
pub(in crate::debugger::session) use types::{parse_reflection_type, parse_type};

use patterns::nullable_type_inside;

pub(super) const UNSUPPORTED: &str = "この書き方には対応していません。評価できるのは、名前をたどる式、メソッドの呼び出し、リテラル、演算子（+ - * / % == != < <= > >= && || !）です。キャスト、条件演算子（?:）、ラムダなどにはまだ対応していません。";

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    /// 直前に読んだ文字列リテラルが、組にならないサロゲートを含んでいたときの、UTF-16 の符号単位の並び。
    wide: Option<Vec<u16>>,
    /// いまクエリ式の中にいる深さ。0 でなければ、`select` や `where` などを変数の名前として読まない。
    in_query: u32,
    /// コンパイラが作った型の名前（`<>f__AnonymousType0`）を、型の名前として読むか。ランタイムから得た型の名前を読むとき
    /// だけ（利用者の書いた式に、こういう名前は書けない）。
    generated_names: bool,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Parser<'a> {
        Parser { chars: text.chars().peekable(), wide: None, in_query: 0, generated_names: false }
    }
}

impl Parser<'_> {
    fn skip_spaces(&mut self) {
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.chars.next();
        }
    }

    fn eat(&mut self, wanted: char) -> bool {
        self.skip_spaces();
        self.chars.next_if_eq(&wanted).is_some()
    }

    fn identifier(&mut self) -> Option<String> {
        // `global::名前` は、名前空間の始まりからの名前。この評価では、名前は初めから全体で探すので、印を外すだけでよい。
        let mut ahead = self.chars.clone();
        let prefix: String = ahead.by_ref().take(8).collect();
        if prefix == "global::" {
            self.chars = ahead;
        }
        // `@class` のような、キーワードを名前として使う書き方。`@` は名前に含まない。
        let mut ahead = self.chars.clone();
        if ahead.next() == Some('@') && ahead.next().is_some_and(|c| c.is_alphabetic() || c == '_') {
            self.chars.next();
        }
        let mut name = String::new();
        // `$exception` のような、デバッガーが用意する名前は `$` で始まる。
        while let Some(c) = self.chars.next_if(|&c| c.is_alphanumeric() || c == '_' || (c == '$' && name.is_empty())) {
            name.push(c);
        }
        // `別名::名前`（名前空間や外部アセンブリの別名）は、`global::` と同じく、印を外して、あとの名前だけにする。
        let mut ahead = self.chars.clone();
        if !name.is_empty() && (ahead.next(), ahead.next()) == (Some(':'), Some(':')) && ahead.peek().is_some_and(|c| c.is_alphabetic() || *c == '_') {
            self.chars = ahead;
            return self.identifier();
        }
        (!name.is_empty()).then_some(name)
    }

    /// 読む位置の単語が `word` なら、読み進めて true。
    fn eat_word(&mut self, word: &str) -> bool {
        self.skip_spaces();
        let mut ahead = self.chars.clone();
        let found: String = std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric() || *c == '_')).collect();
        if found == word {
            self.chars = ahead;
            true
        } else {
            false
        }
    }

    /// 読む位置の単語（読み進めない）。
    fn peek_word(&self) -> String {
        let mut ahead = self.chars.clone();
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        std::iter::from_fn(|| ahead.next_if(|c| c.is_alphanumeric() || *c == '_')).collect()
    }

    /// パターンのあとの変数の名前（`int k` の `k`）。無ければ `None`。
    fn designation(&mut self) -> Option<String> {
        // `@class` のような、キーワードと同じつづりの名前。
        let mut ahead = self.chars.clone();
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        if ahead.next() == Some('@') && ahead.peek().is_some_and(|c| c.is_alphabetic() || *c == '_') {
            self.chars = ahead;
            return Some(std::iter::from_fn(|| self.chars.next_if(|c| c.is_alphanumeric() || *c == '_')).collect());
        }
        let word = self.peek_word();
        let reserved = ["and", "or", "not", "when", "is", "as", "switch", "with", "in"];
        // クエリ式の中では、句の始まりの言葉も変数の名前ではない（`where x is T select x` の `select`）。
        let clauses = ["select", "where", "orderby", "group", "by", "into", "join", "on", "equals", "let", "from", "ascending", "descending"];
        if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) || reserved.contains(&word.as_str()) || (self.in_query > 0 && clauses.contains(&word.as_str())) {
            return None;
        }
        self.eat_word(&word);
        Some(word)
    }

    fn peek_after_spaces(&self) -> Option<char> {
        self.chars.clone().find(|c| !c.is_whitespace())
    }

    /// 読む位置にあるのが、式の続きとしては読めない書き方なら、その名前を入れたエラー文。
    fn unsupported_here(&mut self) -> Option<String> {
        self.skip_spaces();
        let rest: String = self.chars.clone().take(2).collect();
        let what = if rest.starts_with("=>") {
            "ラムダ式（=>）"
        } else if rest.starts_with('{') {
            "{ } を使う書き方（匿名型など）"
        } else {
            return None;
        };
        Some(format!("{what}には、まだ対応していません。"))
    }

}

/// 式を読む。扱えない書き方は、何が扱えないかを伝える。
pub(in crate::debugger::session) fn parse(expression: &str) -> Result<Expr, String> {
    let mut parser = Parser::new(expression);
    let expr = parser.expression()?;
    parser.skip_spaces();
    match parser.chars.peek() {
        None => Ok(expr),
        Some(_) => Err(parser.unsupported_here().unwrap_or_else(|| UNSUPPORTED.into())),
    }
}

