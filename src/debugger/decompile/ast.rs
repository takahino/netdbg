//! 段階 2 の逆コンパイルの、文と式の木。
//!
//! IL から作った直後は「ラベルと goto」の平らな形（`build.rs`）。`structure.rs` が if や while に組み直し、`csharp.rs` が
//! C# として書く。文は、系列点を打つ IL オフセット（評価スタックが空の位置）を持てる。

use super::sig::Ty;

/// 二項演算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::And => "&",
            BinOp::Or => "|",
            BinOp::Xor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }
}

/// 比べる演算（結果は bool）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }

    /// 否定（`!(a < b)` を `a >= b` にする）。浮動小数点数の NaN では成り立たないので、整数と参照の比較だけに使う。
    pub fn negate(self) -> CmpOp {
        match self {
            CmpOp::Eq => CmpOp::Ne,
            CmpOp::Ne => CmpOp::Eq,
            CmpOp::Lt => CmpOp::Ge,
            CmpOp::Le => CmpOp::Gt,
            CmpOp::Gt => CmpOp::Le,
            CmpOp::Ge => CmpOp::Lt,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// ローカル変数（番号）。
    Local(u16),
    /// 引数（`ldarg` の番号。インスタンスメソッドでは 0 が `this`）。
    Arg(u16),
    /// 一時変数（ブロックの境で残った評価スタックの値、`dup` の受け渡し）。
    Temp(u32),
    /// catch のハンドラーの入り口で積まれている例外。
    Exception,
    Int(i64),
    /// 64 ビットの整数（`ldc.i8`）。
    Long(i64),
    Float(f64),
    Double(f64),
    Str(String),
    Null,
    Bool(bool),
    Char(u16),
    /// 型を指定した既定値（`default(T)`）。
    Default(Ty),
    /// 代入先の型に合わせて直した定数（列挙型、符号なし整数）を、そのまま書く。
    Literal(String),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `checked` の演算（`add.ovf` など）。
    Checked(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    /// ビットの否定（`~`）。
    BitNot(Box<Expr>),
    /// 論理の否定（`!`）。
    Not(Box<Expr>),
    Compare(CmpOp, Box<Expr>, Box<Expr>),
    /// 数値の型の変換（`(int)x`）。`checked` なら桁あふれで例外。
    Convert { to: Ty, checked: bool, value: Box<Expr> },
    /// フィールド。`target` が無ければ静的フィールド。
    Field { target: Option<Box<Expr>>, token: u32 },
    /// プロパティ（`get_X()` の呼び出しを書き直したもの）。
    Property { target: Option<Box<Expr>>, method: u32, args: Vec<Expr> },
    /// アドレス（`ldloca`、`ldflda`、`ldelema`）。呼び出しの `this` や `ref` の引数になる。
    AddressOf(Box<Expr>),
    /// アドレスの指す先（`ldind`、`ldobj`）。
    Deref(Box<Expr>),
    Call { method: u32, target: Option<Box<Expr>>, args: Vec<Expr>, virtual_call: bool },
    New { method: u32, args: Vec<Expr> },
    /// デリゲートの作成（`newobj D::.ctor(object, IntPtr)` と `ldftn`）。
    NewDelegate { ty: u32, target: Option<Box<Expr>>, method: u32 },
    NewArray { element: u32, size: Box<Expr> },
    /// 配列の初期化子（`new int[] { 1, 2 }`。データは PE の像から）。
    ArrayInit { element: u32, values: Vec<Expr> },
    Element { array: Box<Expr>, index: Box<Expr> },
    Length(Box<Expr>),
    /// `(T)x`（`castclass`、`unbox.any`）。
    Cast { ty: u32, value: Box<Expr> },
    /// `x as T`（`isinst`）。
    As { ty: u32, value: Box<Expr> },
    /// `(object)x`（`box`）。
    Box { ty: u32, value: Box<Expr> },
    /// `typeof(T)`（`ldtoken` と `Type.GetTypeFromHandle`）。
    TypeOf(u32),
    /// `ldtoken` そのもの（型のハンドル）。
    Token(u32),
    SizeOf(u32),
    /// `ldftn`（デリゲートにしなかったもの）。
    FunctionPointer(u32),
    /// 代入の式（`dup` で値を使い回すもの、`x = y = 0`）。
    Assign(Box<Expr>, Box<Expr>),
    /// 三項演算子（`c ? a : b`）。
    Conditional(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `a ?? b`。
    Coalesce(Box<Expr>, Box<Expr>),
    /// 初期化子つきの `new`（`new List<int> { 1, 2 }`、`new Thread(…) { Name = "x" }`）。`items` は、コレクションなら
    /// `Add` の引数の並び、オブジェクトなら「メンバーの名前、値」。
    Initializer { new: Box<Expr>, collection: Vec<Vec<Expr>>, members: Vec<(String, Expr)> },
    /// `x?`（`x?.Method()` の受け手）。呼び出しの `target` に置く。
    NullConditional(Box<Expr>),
    /// 文字列の補間（`$"{name} has {count}"`）。
    Interpolated(Vec<Part>),
}

/// 文字列の補間の部分: 文字列そのものか、`{値,幅:書式}`。
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Hole { value: Expr, alignment: Option<i64>, format: Option<String> },
}

impl Expr {
    /// 副作用が無く、二度書いてもよい式（`dup` を一時変数にしなくてよいもの）。
    pub fn is_simple(&self) -> bool {
        match self {
            Expr::Local(_) | Expr::Arg(_) | Expr::Temp(_) | Expr::Int(_) | Expr::Long(_) | Expr::Float(_) | Expr::Double(_) | Expr::Str(_) | Expr::Null | Expr::Bool(_) | Expr::Char(_) | Expr::Literal(_) => true,
            Expr::AddressOf(inner) => inner.is_simple(),
            _ => false,
        }
    }

    /// 文として書ける式（呼び出し、代入、`new`）。ほかは `_ = …;` にする。
    pub fn is_statement(&self) -> bool {
        matches!(self, Expr::Call { .. } | Expr::New { .. } | Expr::Assign(..))
    }

    pub fn boxed(self) -> Box<Expr> {
        Box::new(self)
    }
}

/// ラベル（IL オフセット）。
pub type Label = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct Catch {
    /// 捕まえる型のトークン（`catch when` のフィルターでは 0 = すべて）。
    pub ty: u32,
    /// 例外を入れる変数の名前（使わなければ `None`）。
    pub variable: Option<String>,
    pub filter: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Expr(Expr),
    Assign(Expr, Expr),
    Return(Option<Expr>),
    /// `throw x;`。`None` は `throw;`（`rethrow`）。
    Throw(Option<Expr>),
    If { condition: Expr, then: Vec<Stmt>, otherwise: Vec<Stmt> },
    While { condition: Expr, body: Vec<Stmt> },
    DoWhile { body: Vec<Stmt>, condition: Expr },
    Break,
    Continue,
    Goto(Label),
    Label(Label),
    /// `switch (value) { case 0: goto L; … }`。`targets` の添字が case の値。`ty` は値の型（列挙型なら case をキャストで書く）。
    Switch { value: Expr, targets: Vec<Label>, ty: Option<Ty> },
    Try { body: Vec<Stmt>, catches: Vec<Catch>, finally: Option<Vec<Stmt>>, fault: Option<Vec<Stmt>> },
    /// 複合代入（`x += v`。イベントの `+=` / `-=` も）。
    Compound { target: Expr, op: BinOp, value: Expr },
    /// `x++` / `x--`。
    Increment { target: Expr, decrement: bool },
    /// `for (初期化; 条件; 増分)`。初期化と増分は文 1 つ（無ければ `None`）。
    For { init: Option<Box<Stmt>>, condition: Expr, step: Option<Box<Stmt>>, body: Vec<Stmt> },
    /// 宣言つきの代入（`int num = 0;`）。
    Declare { local: u16, value: Expr },
    /// `foreach (T v in collection)`。
    Foreach { local: u16, collection: Expr, body: Vec<Stmt> },
    /// `using (T r = resource)`。`local` が無ければ `using (resource)`。
    Using { local: Option<u16>, resource: Expr, body: Vec<Stmt> },
    /// `lock (target)`。
    Lock { target: Expr, body: Vec<Stmt> },
    /// case の中身のある `switch`。`cases` が空の節は `default`。
    SwitchBlock { value: Expr, sections: Vec<Section>, ty: Option<Ty> },
}

/// `switch` の節 1 つ: case の値（空なら `default`）と中身。
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub cases: Vec<i64>,
    pub default: bool,
    pub body: Vec<Stmt>,
}

/// 文 1 つ。`il` は系列点を打つ IL オフセット（その文の評価が始まる、スタックが空の位置）。`merged` は、この文にまとめた
/// 文（`foreach` にまとめた `GetEnumerator` や `MoveNext` など）の系列点。同じ行に打つので、ブレークポイントとステップの
/// 位置は失われない。
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub il: Option<u32>,
    pub merged: Vec<u32>,
}

impl Stmt {
    pub fn new(kind: StmtKind, il: Option<u32>) -> Stmt {
        Stmt { kind, il, merged: Vec::new() }
    }

    /// 別の文の系列点を、この文にまとめる（その文を消すとき）。
    pub fn absorb(&mut self, other: &Stmt) {
        for il in other.il.iter().chain(&other.merged) {
            if self.il.is_none() {
                self.il = Some(*il);
            } else if self.il != Some(*il) && !self.merged.contains(il) {
                self.merged.push(*il);
            }
        }
    }

    /// この文のあとに、次の文へ進まない（戻る、投げる、飛ぶ）。
    pub fn ends_flow(&self) -> bool {
        matches!(self.kind, StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Goto(_) | StmtKind::Break | StmtKind::Continue)
    }
}

impl Expr {
    /// 式の子を、読む順に `f` に渡す。木をたどるたびに一覧を作らないため（`children` は一覧を作る）。
    pub fn for_each_child<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        match self {
            Expr::Binary(_, a, b) | Expr::Checked(_, a, b) | Expr::Compare(_, a, b) | Expr::Assign(a, b) | Expr::Coalesce(a, b) | Expr::Element { array: a, index: b } => {
                f(a);
                f(b);
            }
            Expr::Conditional(c, a, b) => {
                f(c);
                f(a);
                f(b);
            }
            Expr::Neg(e) | Expr::BitNot(e) | Expr::Not(e) | Expr::AddressOf(e) | Expr::Deref(e) | Expr::Length(e) | Expr::NullConditional(e) => f(e),
            Expr::Convert { value: e, .. } | Expr::Cast { value: e, .. } | Expr::As { value: e, .. } | Expr::Box { value: e, .. } | Expr::NewArray { size: e, .. } => f(e),
            Expr::Field { target, .. } | Expr::NewDelegate { target, .. } => {
                if let Some(t) = target {
                    f(t);
                }
            }
            Expr::Property { target, args, .. } | Expr::Call { target, args, .. } => {
                if let Some(t) = target {
                    f(t);
                }
                args.iter().for_each(f);
            }
            Expr::New { args, .. } | Expr::ArrayInit { values: args, .. } => args.iter().for_each(f),
            Expr::Initializer { new, collection, members } => {
                f(new);
                collection.iter().flatten().for_each(&mut *f);
                members.iter().for_each(|(_, v)| f(v));
            }
            Expr::Interpolated(parts) => parts.iter().for_each(|part| {
                if let Part::Hole { value, .. } = part {
                    f(value);
                }
            }),
            _ => {}
        }
    }

    /// `for_each_child` の書き換えられる版。
    pub fn for_each_child_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        match self {
            Expr::Binary(_, a, b) | Expr::Checked(_, a, b) | Expr::Compare(_, a, b) | Expr::Assign(a, b) | Expr::Coalesce(a, b) | Expr::Element { array: a, index: b } => {
                f(a);
                f(b);
            }
            Expr::Conditional(c, a, b) => {
                f(c);
                f(a);
                f(b);
            }
            Expr::Neg(e) | Expr::BitNot(e) | Expr::Not(e) | Expr::AddressOf(e) | Expr::Deref(e) | Expr::Length(e) | Expr::NullConditional(e) => f(e),
            Expr::Convert { value: e, .. } | Expr::Cast { value: e, .. } | Expr::As { value: e, .. } | Expr::Box { value: e, .. } | Expr::NewArray { size: e, .. } => f(e),
            Expr::Field { target, .. } | Expr::NewDelegate { target, .. } => {
                if let Some(t) = target {
                    f(t);
                }
            }
            Expr::Property { target, args, .. } | Expr::Call { target, args, .. } => {
                if let Some(t) = target {
                    f(t);
                }
                args.iter_mut().for_each(f);
            }
            Expr::New { args, .. } | Expr::ArrayInit { values: args, .. } => args.iter_mut().for_each(f),
            Expr::Initializer { new, collection, members } => {
                f(new);
                collection.iter_mut().flatten().for_each(&mut *f);
                members.iter_mut().for_each(|(_, v)| f(v));
            }
            Expr::Interpolated(parts) => parts.iter_mut().for_each(|part| {
                if let Part::Hole { value, .. } = part {
                    f(value);
                }
            }),
            _ => {}
        }
    }

    /// 式の子（読む順）。
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Binary(_, a, b) | Expr::Checked(_, a, b) | Expr::Compare(_, a, b) | Expr::Assign(a, b) | Expr::Coalesce(a, b) => vec![a, b],
            Expr::Conditional(c, a, b) => vec![c, a, b],
            Expr::Element { array, index } => vec![array, index],
            Expr::Neg(e) | Expr::BitNot(e) | Expr::Not(e) | Expr::AddressOf(e) | Expr::Deref(e) | Expr::Length(e) | Expr::NullConditional(e) => vec![e],
            Expr::Convert { value, .. } | Expr::Cast { value, .. } | Expr::As { value, .. } | Expr::Box { value, .. } => vec![value],
            Expr::NewArray { size, .. } => vec![size],
            Expr::Field { target, .. } | Expr::NewDelegate { target, .. } => target.iter().map(|t| &**t).collect(),
            Expr::Property { target, args, .. } | Expr::Call { target, args, .. } => target.iter().map(|t| &**t).chain(args.iter()).collect(),
            Expr::New { args, .. } | Expr::ArrayInit { values: args, .. } => args.iter().collect(),
            Expr::Initializer { new, collection, members } => std::iter::once(&**new).chain(collection.iter().flatten()).chain(members.iter().map(|(_, v)| v)).collect(),
            Expr::Interpolated(parts) => parts.iter().filter_map(|part| if let Part::Hole { value, .. } = part { Some(value) } else { None }).collect(),
            _ => Vec::new(),
        }
    }

    pub fn children_mut(&mut self) -> Vec<&mut Expr> {
        match self {
            Expr::Binary(_, a, b) | Expr::Checked(_, a, b) | Expr::Compare(_, a, b) | Expr::Assign(a, b) | Expr::Coalesce(a, b) => vec![a, b],
            Expr::Conditional(c, a, b) => vec![c, a, b],
            Expr::Element { array, index } => vec![array, index],
            Expr::Neg(e) | Expr::BitNot(e) | Expr::Not(e) | Expr::AddressOf(e) | Expr::Deref(e) | Expr::Length(e) | Expr::NullConditional(e) => vec![e],
            Expr::Convert { value, .. } | Expr::Cast { value, .. } | Expr::As { value, .. } | Expr::Box { value, .. } => vec![value],
            Expr::NewArray { size, .. } => vec![size],
            Expr::Field { target, .. } | Expr::NewDelegate { target, .. } => target.iter_mut().map(|t| &mut **t).collect(),
            Expr::Property { target, args, .. } | Expr::Call { target, args, .. } => target.iter_mut().map(|t| &mut **t).chain(args.iter_mut()).collect(),
            Expr::New { args, .. } | Expr::ArrayInit { values: args, .. } => args.iter_mut().collect(),
            Expr::Initializer { new, collection, members } => std::iter::once(&mut **new).chain(collection.iter_mut().flatten()).chain(members.iter_mut().map(|(_, v)| v)).collect(),
            Expr::Interpolated(parts) => parts.iter_mut().filter_map(|part| if let Part::Hole { value, .. } = part { Some(value) } else { None }).collect(),
            _ => Vec::new(),
        }
    }

    /// この式と、その中の式すべてをたどる。
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        self.for_each_child(&mut |child| child.walk(f));
    }

    /// この式と、その中の式すべてを、書き換えながらたどる（親が先）。
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        f(self);
        self.for_each_child_mut(&mut |child| child.walk_mut(f));
    }

    /// 副作用のある式（呼び出し、`new`、プロパティ、代入）を含むか。
    pub fn has_side_effects(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| found |= matches!(e, Expr::Call { .. } | Expr::New { .. } | Expr::Property { .. } | Expr::Assign(..) | Expr::NewDelegate { .. }));
        found
    }
}

impl Stmt {
    /// 文の中のブロックを `f` に渡す（`blocks` と同じ順。一覧を作らない）。
    pub fn for_each_block<'a>(&'a self, f: &mut impl FnMut(&'a Vec<Stmt>)) {
        match &self.kind {
            StmtKind::If { then, otherwise, .. } => {
                f(then);
                f(otherwise);
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } | StmtKind::Foreach { body, .. } | StmtKind::Using { body, .. } | StmtKind::Lock { body, .. } | StmtKind::For { body, .. } => f(body),
            StmtKind::SwitchBlock { sections, .. } => sections.iter().for_each(|s| f(&s.body)),
            StmtKind::Try { body, catches, finally, fault } => {
                f(body);
                catches.iter().for_each(|c| f(&c.body));
                finally.iter().chain(fault.iter()).for_each(f);
            }
            _ => {}
        }
    }

    /// `for_each_block` の書き換えられる版。
    pub fn for_each_block_mut(&mut self, f: &mut impl FnMut(&mut Vec<Stmt>)) {
        match &mut self.kind {
            StmtKind::If { then, otherwise, .. } => {
                f(then);
                f(otherwise);
            }
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } | StmtKind::Foreach { body, .. } | StmtKind::Using { body, .. } | StmtKind::Lock { body, .. } | StmtKind::For { body, .. } => f(body),
            StmtKind::SwitchBlock { sections, .. } => sections.iter_mut().for_each(|s| f(&mut s.body)),
            StmtKind::Try { body, catches, finally, fault } => {
                f(body);
                catches.iter_mut().for_each(|c| f(&mut c.body));
                finally.iter_mut().chain(fault.iter_mut()).for_each(f);
            }
            _ => {}
        }
    }

    /// `for_each_expression` の書き換えられる版。
    pub fn for_each_expression_mut(&mut self, f: &mut impl FnMut(&mut Expr)) {
        match &mut self.kind {
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(Some(e)) | StmtKind::Declare { value: e, .. } | StmtKind::Increment { target: e, .. } => f(e),
            StmtKind::Assign(a, b) | StmtKind::Compound { target: a, value: b, .. } => {
                f(a);
                f(b);
            }
            StmtKind::If { condition: e, .. } | StmtKind::While { condition: e, .. } | StmtKind::DoWhile { condition: e, .. } => f(e),
            StmtKind::Switch { value: e, .. } | StmtKind::SwitchBlock { value: e, .. } => f(e),
            StmtKind::Foreach { collection: e, .. } | StmtKind::Using { resource: e, .. } | StmtKind::Lock { target: e, .. } => f(e),
            StmtKind::For { init, condition, step, .. } => {
                if let Some(init) = init {
                    init.for_each_expression_mut(f);
                }
                f(condition);
                if let Some(step) = step {
                    step.for_each_expression_mut(f);
                }
            }
            _ => {}
        }
    }

    /// 文の中のブロック（if の両側、ループの本体、try の各部分）。
    pub fn blocks(&self) -> Vec<&Vec<Stmt>> {
        match &self.kind {
            StmtKind::If { then, otherwise, .. } => vec![then, otherwise],
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } | StmtKind::Foreach { body, .. } | StmtKind::Using { body, .. } | StmtKind::Lock { body, .. } | StmtKind::For { body, .. } => vec![body],
            StmtKind::SwitchBlock { sections, .. } => sections.iter().map(|s| &s.body).collect(),
            StmtKind::Try { body, catches, finally, fault } => {
                let mut list = vec![body];
                list.extend(catches.iter().map(|c| &c.body));
                list.extend(finally.iter());
                list.extend(fault.iter());
                list
            }
            _ => Vec::new(),
        }
    }

    pub fn blocks_mut(&mut self) -> Vec<&mut Vec<Stmt>> {
        match &mut self.kind {
            StmtKind::If { then, otherwise, .. } => vec![then, otherwise],
            StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } | StmtKind::Foreach { body, .. } | StmtKind::Using { body, .. } | StmtKind::Lock { body, .. } | StmtKind::For { body, .. } => vec![body],
            StmtKind::SwitchBlock { sections, .. } => sections.iter_mut().map(|s| &mut s.body).collect(),
            StmtKind::Try { body, catches, finally, fault } => {
                let mut list = vec![body];
                list.extend(catches.iter_mut().map(|c| &mut c.body));
                list.extend(finally.iter_mut());
                list.extend(fault.iter_mut());
                list
            }
            _ => Vec::new(),
        }
    }

    /// 文そのものの式を `f` に渡す（`expressions` と同じ順。一覧を作らない）。
    pub fn for_each_expression<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        match &self.kind {
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(Some(e)) | StmtKind::Declare { value: e, .. } | StmtKind::Increment { target: e, .. } => f(e),
            StmtKind::Assign(a, b) | StmtKind::Compound { target: a, value: b, .. } => {
                f(a);
                f(b);
            }
            StmtKind::If { condition: e, .. } | StmtKind::While { condition: e, .. } | StmtKind::DoWhile { condition: e, .. } => f(e),
            StmtKind::Switch { value: e, .. } | StmtKind::SwitchBlock { value: e, .. } => f(e),
            StmtKind::Foreach { collection: e, .. } | StmtKind::Using { resource: e, .. } | StmtKind::Lock { target: e, .. } => f(e),
            StmtKind::For { init, condition, step, .. } => {
                if let Some(init) = init {
                    init.for_each_expression(f);
                }
                f(condition);
                if let Some(step) = step {
                    step.for_each_expression(f);
                }
            }
            _ => {}
        }
    }

    /// 文そのものの式（入れ子のブロックの中は含まない）。代入先も含む。
    pub fn expressions(&self) -> Vec<&Expr> {
        match &self.kind {
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(Some(e)) | StmtKind::Declare { value: e, .. } => vec![e],
            StmtKind::Assign(a, b) | StmtKind::Compound { target: a, value: b, .. } => vec![a, b],
            StmtKind::Increment { target, .. } => vec![target],
            StmtKind::If { condition, .. } | StmtKind::While { condition, .. } | StmtKind::DoWhile { condition, .. } => vec![condition],
            StmtKind::Switch { value, .. } | StmtKind::SwitchBlock { value, .. } => vec![value],
            StmtKind::Foreach { collection: e, .. } | StmtKind::Using { resource: e, .. } | StmtKind::Lock { target: e, .. } => vec![e],
            StmtKind::For { init, condition, step, .. } => init.iter().flat_map(|s| s.expressions()).chain(std::iter::once(condition)).chain(step.iter().flat_map(|s| s.expressions())).collect(),
            _ => Vec::new(),
        }
    }

    pub fn expressions_mut(&mut self) -> Vec<&mut Expr> {
        match &mut self.kind {
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(Some(e)) | StmtKind::Declare { value: e, .. } => vec![e],
            StmtKind::Assign(a, b) | StmtKind::Compound { target: a, value: b, .. } => vec![a, b],
            StmtKind::Increment { target, .. } => vec![target],
            StmtKind::If { condition, .. } | StmtKind::While { condition, .. } | StmtKind::DoWhile { condition, .. } => vec![condition],
            StmtKind::Switch { value, .. } | StmtKind::SwitchBlock { value, .. } => vec![value],
            StmtKind::Foreach { collection: e, .. } | StmtKind::Using { resource: e, .. } | StmtKind::Lock { target: e, .. } => vec![e],
            StmtKind::For { init, condition, step, .. } => {
                let mut list: Vec<&mut Expr> = Vec::new();
                if let Some(init) = init {
                    list.extend(init.expressions_mut());
                }
                list.push(condition);
                if let Some(step) = step {
                    list.extend(step.expressions_mut());
                }
                list
            }
            _ => Vec::new(),
        }
    }
}

/// 文と、その中の文すべてをたどる。
pub fn visit<'a>(body: &'a [Stmt], f: &mut impl FnMut(&'a Stmt)) {
    for stmt in body {
        f(stmt);
        stmt.for_each_block(&mut |inner| visit(inner, f));
    }
}

/// 文と、その中の文すべてを、書き換えながらたどる。
pub fn visit_mut(body: &mut [Stmt], f: &mut impl FnMut(&mut Stmt)) {
    for stmt in body {
        f(stmt);
        stmt.for_each_block_mut(&mut |inner| visit_mut(inner, f));
    }
}

/// 文の中の式すべて（入れ子の式も）を、書き換えながらたどる。
pub fn visit_expressions_mut(body: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
    visit_mut(body, &mut |s| s.for_each_expression_mut(&mut |e| e.walk_mut(f)));
}

/// 文の中の式すべて（入れ子の式も）をたどる。
pub fn visit_expressions<'a>(body: &'a [Stmt], f: &mut impl FnMut(&'a Expr)) {
    visit(body, &mut |s| s.for_each_expression(&mut |e| e.walk(f)));
}