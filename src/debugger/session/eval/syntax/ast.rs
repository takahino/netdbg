//! 式の構文木。意味（名前が何を指すか、どのオーバーロードか）は持たない。

use crate::debugger::session::operators::{BinaryOp, UnaryOp};

/// ラムダ式の引数の並び（書いた型と名前。型を書かなければ `None`）。
pub(in crate::debugger::session) type LambdaParameters = Vec<(Option<TypeSyntax>, String)>;

#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum Expr {
    /// 先頭の名前（変数、`this`、`$exception`、型や名前空間の最初の部分）。
    Name(String),
    /// `式.名前`
    Member(Box<Expr>, String),
    /// `式[引数, …]`
    Index(Box<Expr>, Vec<Expr>),
    /// `式(引数, …)`。式は、メソッドを指す `名前` か `式.名前`。
    Call(Box<Expr>, Vec<Expr>),
    Int(i32),
    /// `10L`、または `int` と `uint` に収まらない整数。
    Long(i64),
    /// `10u`、または `int` に収まらず `uint` に収まる整数（`3000000000`、`0xFFFFFFFF`）。
    UInt(u32),
    /// `10ul`、または `long` に収まらない整数。
    ULong(u64),
    /// `1.5`、`2d`。
    Double(f64),
    /// `0.5f`。
    Float(f32),
    /// `decimal` のリテラル（`1.50m`）。書かれた数字のまま持つ（末尾の 0 も値の一部）。
    Decimal(String),
    /// `'x'`。中身は UTF-16 の符号単位。
    Char(u16),
    Bool(bool),
    Str(String),
    Null,
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `条件 ? 真のとき : 偽のとき`
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `左 ?? 右`: 左が null でなければ左、null なら右。
    Coalesce(Box<Expr>, Box<Expr>),
    /// `左?.続き`、`左?[添字]続き`: 左が null なら全体が null。そうでなければ、続き（中の `Receiver` が左の値）を評価する。
    NullConditional(Box<Expr>, Box<Expr>),
    /// `NullConditional` の続きの中で、左辺の値を表す。
    Receiver,
    /// `式 switch { 腕, … }`
    Switch(Box<Expr>, Vec<Arm>),
    /// `式 with { 名前 = 値, … }`
    With(Box<Expr>, Vec<(String, Expr)>),
    /// タプル（`(1, "a")`）。
    Tuple(Vec<Expr>),
    /// `^n`: 終わりから数えた位置。添字の中でだけ使える。
    FromEnd(Box<Expr>),
    /// `a..b`。どちらの端も省ける。添字の中でだけ使える。
    Range(Option<Box<Expr>>, Option<Box<Expr>>),
    /// 型を省いた `new(引数) { 初期化子 }`。型は、渡す先や代入先から決める。
    TargetNew { arguments: Vec<Expr>, initializer: Initializer },
    /// コレクション式（`[1, 2, ..rest]`、`[with(capacity: 4), 1]`）。型は、渡す先や代入先から決める。
    /// 要素の `true` は、`..` で中身を広げるもの。
    Collection { arguments: Vec<Expr>, items: Vec<(bool, Expr)> },
    /// ラムダ式（`x => x + 1`、`(a, b) => a + b`、`(int x) => x`）。
    Lambda(LambdaParameters, Box<Expr>),
    /// 匿名型（`new { A = 1, B = "x" }`）。
    Anonymous(Vec<(String, Expr)>),
    /// `out` / `ref` の引数。引数の並びの中にだけ現れる。
    Out(OutTarget),
    /// 名前つき引数（`times: 2`）。引数の並びの中にだけ現れる。
    Named(String, Box<Expr>),
    /// 型引数を付けた名前（`Echo<int>`）。呼び出しの対象か、ジェネリック型の静的メンバーの左か、メソッドの集まり
    /// （`registry.GetPipeline<string>` を、デリゲートを受け取る引数に渡す）。
    Generic(Box<Expr>, Vec<TypeSyntax>),
    /// 型を書かない `default`。型は、比べる相手や代入先から決める。
    DefaultLiteral,
    /// 補間文字列（`$"{a}-{b:F2}"`）。
    Interpolated(Vec<Piece>),
    /// `checked(式)`（true）: 中の整数の計算があふれたら、失敗にする。`unchecked(式)`（false）: 定数どうしでも折り返す。
    Checked(bool, Box<Expr>),
    /// `new 型(引数) { 初期化子 }`。
    New { ty: TypeSyntax, arguments: Vec<Expr>, initializer: Initializer },
    /// 配列の生成。`new int[3]`、`new int[2, 3]`、`new int[] { 1, 2 }`、`new[] { 1, 2 }`（要素の型は中身から決める）。
    NewArray { element: Option<TypeSyntax>, sizes: Vec<Expr>, items: Option<Vec<Expr>> },
    /// 代入（`a = b`、`a += b`、`a ??= b`）。
    Assign(Box<Expr>, Assignment, Box<Expr>),
    /// `++x`、`x++`、`--x`、`x--`。
    Increment { target: Box<Expr>, decrement: bool, prefix: bool },
    /// `(型)式`
    Cast(TypeSyntax, Box<Expr>),
    /// `式 is パターン`
    Is(Box<Expr>, Pattern),
    /// `式 as 型`
    As(Box<Expr>, TypeSyntax),
    /// `typeof(型)`
    TypeOf(TypeSyntax),
    /// `default(型)`
    Default(TypeSyntax),
    /// `sizeof(型)`
    SizeOf(TypeSyntax),
    /// `throw 式`。C# と同じく、`??` の右辺、条件演算子の枝、ラムダ式の本体にだけ書ける。
    Throw(Box<Expr>),
    /// ラムダ式の `{ … }` の本体のうち、文を 2 つ以上並べたもの。ラムダ式の中にだけ現れる。
    Block(Vec<Statement>),
    /// 多次元の配列の、中身を書いた生成（`new int[,] { { 1, 2 }, { 3, 4 } }`、`new[,] { … }`）。`dimensions` は次元ごとの
    /// 大きさ、`items` は行を優先した順の要素。
    NewMatrix { element: Option<TypeSyntax>, dimensions: Vec<usize>, items: Vec<Expr> },
    /// 初期化子の中の `名前 = { … }` の右辺（メンバーを作り直さずに、いまの値の中へ足す・代入する）。そこにだけ現れる。
    Nested(Initializer),
    /// 分解の代入（`var (a, b) = 式`、`(int a, string b) = 式`、`(x, _) = 式`）。
    Deconstruct(Vec<OutTarget>, Box<Expr>),
    /// 文字列のうち、組になっていないサロゲート（`"\ud800"`）を含むもの。UTF-16 の符号単位の並びで持つ。
    Utf16(Vec<u16>),
    /// UTF-8 の文字列リテラル（`"text"u8`）。型は `ReadOnlySpan<byte>`。
    Utf8(String),
    /// `stackalloc 型[大きさ]`。
    StackAlloc(Box<Expr>),
    /// `&式`（変数のアドレス）。
    AddressOf(Box<Expr>),
    /// `*式`（ポインターの指す先）。
    Dereference(Box<Expr>),
    /// `await 式`。式の `GetAwaiter().GetResult()` を実行して、終わるまで待つ（docs/DECISIONS.md D-034）。
    Await(Box<Expr>),
    /// `async` を付けたラムダ式の本体。本体は同期で実行し、結果を完了済みの Task に包む。`Lambda` の本体にだけ現れる。
    Async(Box<Expr>),
}

/// ラムダ式の `{ … }` の本体の文。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum Statement {
    /// `var x = 式;`、`int x = 式;`、`int x;`、`int a = 1, b;`。型の `None` は `var`。
    Local(Option<TypeSyntax>, Vec<(String, Option<Expr>)>),
    /// `式;`（呼び出し、代入、`throw 式;` など）。
    Expr(Expr),
    /// `return 式;`、`return;`
    Return(Option<Expr>),
    /// `{ 文… }`。空の文（`;`）は、空のブロック。
    Block(Vec<Statement>),
    /// `if (条件) 文 else 文`
    If(Expr, Box<Statement>, Option<Box<Statement>>),
    /// `while (条件) 文`
    While(Expr, Box<Statement>),
    /// `do 文 while (条件);`
    Do(Box<Statement>, Expr),
    /// `for (初期化; 条件; 更新) 文`。初期化は、変数の宣言か、式の並び。
    For { init: Vec<Statement>, condition: Option<Expr>, step: Vec<Expr>, body: Box<Statement> },
    /// `foreach (型 名前 in 式) 文`。型の `None` は `var`。名前が 2 つ以上なら、分解（`foreach (var (k, v) in 式)`）。
    ForEach { ty: Option<TypeSyntax>, names: Vec<String>, source: Expr, body: Box<Statement> },
    /// `switch (式) { case …: 文… default: 文… }`
    Switch(Expr, Vec<SwitchSection>),
    /// `try { … } catch (型 名前) when (条件) { … } finally { … }`
    Try { body: Vec<Statement>, catches: Vec<Catch>, finally: Option<Vec<Statement>> },
    /// `using (資源) 文`、`using var x = 式;`（あとの文が本体）。資源は、変数の宣言か式。
    Using(Box<Statement>, Vec<Statement>),
    /// `lock (式) 文`
    Lock(Expr, Box<Statement>),
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `throw;`（catch の中で、受け取った例外を投げ直す）
    Rethrow,
}

/// `switch` 文の、ラベルの並びと、その文の並び。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) struct SwitchSection {
    /// `case パターン when 条件:`。`None` は `default:`。
    pub labels: Vec<Option<(Pattern, Option<Expr>)>>,
    pub body: Vec<Statement>,
}

/// `catch (型 名前) when (条件) { … }`。どの部分も省ける。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) struct Catch {
    pub ty: Option<TypeSyntax>,
    pub name: Option<String>,
    pub filter: Option<Expr>,
    pub body: Vec<Statement>,
}

/// 補間文字列の部分。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum Piece {
    Text(String),
    /// `{式,幅:書式}`
    Hole { expr: Expr, alignment: Option<String>, format: Option<String> },
}

/// `new` のあとの `{ … }`。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum Initializer {
    None,
    /// `{ X = 1, Y = 2 }`: フィールドやプロパティへの代入。
    Members(Vec<(String, Expr)>),
    /// `{ 1, 2 }`、`{ { "k", 1 } }`: 要素ごとに `Add(…)` を呼ぶ。
    Items(Vec<Vec<Expr>>),
    /// `{ ["k"] = 1 }`: 要素ごとにインデクサーへ代入する。
    Indexed(Vec<(Vec<Expr>, Expr)>),
}

/// `out` の引数の受け先。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum OutTarget {
    /// `out _`
    Discard,
    /// `out var x`、`out int x`: 式の中だけの変数を作る。
    Declare(Option<TypeSyntax>, String),
    /// `out x`、`ref x`: いまある変数やフィールド。
    Existing(Box<Expr>),
    /// 分解の代入の左辺の、入れ子（`(var a, (var b, var c)) = 式` の `(var b, var c)`）。分解の代入にだけ現れる。
    Tuple(Vec<OutTarget>),
}

/// 代入の種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::debugger::session) enum Assignment {
    /// `=`
    Plain,
    /// `+=` など。中身は、先に行う演算。
    Compound(BinaryOp),
    /// `??=`: 左辺が null のときだけ代入する。
    Coalesce,
}

/// 式の中に書かれた型の名前。
#[derive(Debug, Clone, PartialEq, Default)]
pub(in crate::debugger::session) struct TypeSyntax {
    /// 完全な名前。`int` などのキーワードは `System.Int32` などに直してある。タプル型（`(int, string)`）は `System.ValueTuple`。
    pub name: String,
    /// 末尾の `?`（`int?`）。配列のあとの `?`（`int[]?`）は、null を許すという注釈で、型は変わらないので持たない。
    pub nullable: bool,
    /// 配列か（`int[]`、`int[,]`、`int[][]`）。
    pub array: bool,
    /// 配列の形が `[]` 一つではないときの、次元の数の並び（書かれた順。`int[,]` は `[2]`、`int[][]` は `[1, 1]`、
    /// `int[][,]` は `[1, 2]` で、「2 次元の配列」を要素にした配列）。`[]` 一つのときは空。
    pub ranks: Vec<u32>,
    /// ポインターの `*` の数（`uint*` は 1）。
    pub pointers: u8,
    /// 型引数（`List<int>` の `int`）。
    pub arguments: Vec<TypeSyntax>,
}

impl TypeSyntax {
    /// 型引数も配列の印も無い型。
    pub fn named(name: &str) -> TypeSyntax {
        TypeSyntax { name: name.to_string(), ..TypeSyntax::default() }
    }

    /// 配列の形（次元の数の並び。配列でなければ空）。
    pub fn shape(&self) -> Vec<u32> {
        match (self.array, self.ranks.is_empty()) {
            (false, _) => Vec::new(),
            (true, true) => vec![1],
            (true, false) => self.ranks.clone(),
        }
    }

    /// 配列の形を付けた型。
    pub fn with_shape(self, shape: &[u32]) -> TypeSyntax {
        TypeSyntax { array: !shape.is_empty(), ranks: if shape == [1] { Vec::new() } else { shape.to_vec() }, ..self }
    }

    /// 配列の要素の型（`int[][,]` の要素は `int[,]`）。
    pub fn element(&self) -> TypeSyntax {
        let shape = self.shape();
        self.clone().with_shape(shape.get(1..).unwrap_or_default())
    }
}

/// `is` の右に書くパターン。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) enum Pattern {
    /// `not パターン`
    Not(Box<Pattern>),
    /// `null`
    Null,
    /// `> 5` など。
    Relational(BinaryOp, Box<Expr>),
    /// 型の名前か、定数。どちらかは、名前を解決するまで分からない（`x is Shade` と `x is Shade.Dark`）。
    /// 型の名前として読めたときだけ、1 番目が入る。2 番目は、同じ部分を式として読んだもの。
    TypeOrConstant(Option<TypeSyntax>, Box<Expr>),
    /// `パターン and パターン`
    And(Box<Pattern>, Box<Pattern>),
    /// `パターン or パターン`
    Or(Box<Pattern>, Box<Pattern>),
    /// `int k`: 型が合えば、値を変数に入れる。
    Declaration(TypeSyntax, String),
    /// `var v`: いつも合い、値を変数に入れる。
    Var(String),
    /// `_`: いつも合う。
    Discard,
    /// `型 (位置のパターン) { メンバー: パターン } 変数`。どの部分も省ける。
    Recursive { ty: Option<TypeSyntax>, positional: Option<Vec<Pattern>>, members: Vec<(Vec<String>, Pattern)>, designation: Option<String> },
    /// `[1, .., 3]`。`slice` は、`..` の位置。
    List { items: Vec<Pattern>, slice: Option<usize>, designation: Option<String> },
}

/// `switch` 式の腕: `パターン when 条件 => 結果`。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::debugger::session) struct Arm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub result: Expr,
}

/// キーワード（`int`）の指す型の、完全な名前（`System.Int32`）。
pub(in crate::debugger::session) fn keyword_type(word: &str) -> Option<&'static str> {
    crate::debugger::builtin::by_keyword(word).map(|ty| ty.full_name)
}

/// 完全な名前（`System.Int32`）が、キーワードのある型か。
pub(in crate::debugger::session) fn is_keyword_type(full_name: &str) -> bool {
    crate::debugger::builtin::by_full_name(full_name).is_some()
}
