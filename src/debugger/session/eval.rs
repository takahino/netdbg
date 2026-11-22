//! 式の評価（エンジン側）。
//!
//! 構文木（`syntax.rs` が作る）を、止まっている対象の上で評価する。基準は「同じ式を、その場所に書いてコンパイルした
//! ときと、同じ値・同じ型」（docs/DECISIONS.md D-025）。確かめるのは適合試験（tests/apps/Console の Conformance）。
//!
//! 演算子はエンジンの側で計算し（`operators.rs`）、プロパティやメソッドは対象の中で実行する（`funceval.rs`）。
//!
//! 名前の解決は、C# の見え方に合わせて、近い方から:
//!   1. そのフレームの引数とローカル変数、`this`
//!   2. `this` のメンバー
//!   3. いまのメソッドが属する型の静的メンバー
//!   4. 型の名前（`Program`、`System.Environment`。いまの名前空間からの相対でもよい）
//!
//! このファイルは入口（`evaluate_expression`）と、式の種類ごとの振り分け（`evaluate_node`）。中身は、次のファイルにある。
//!
//! | ファイル | 中身 |
//! |---|---|
//! | `syntax.rs` | 構文解析（文字列 → 構文木） |
//! | `names.rs` | 名前、メンバー、静的メンバー、型と名前空間、添字 |
//! | `calls.rs` | メソッドの呼び出し、オーバーロードの選択、引数の受け渡し |
//! | `bind.rs` | 呼び出しの結び付け（省略できる引数、名前つき引数、`params`、型引数の推論、拡張メソッド） |
//! | `arithmetic.rs` | 単項・二項演算子、文字列の連結と補間、対象の値と `Operand` の行き来 |
//! | `userops.rs` | 利用者が定義した演算子と変換（`decimal` も） |
//! | `conversions.rs` | キャスト、`as`、`is`、`typeof`、`?:` と `??` の型の統一 |
//! | `types.rs` | 式の型（`Typed`）と、暗黙の変換の規則 |
//! | `patterns.rs` | パターン、`switch` 式、タプル、`with` |
//! | `assign.rs` | 代入、`++` / `--`、分解の代入 |
//! | `create.rs` | `new`（オブジェクト、配列、初期化子、匿名型） |
//! | `targeted.rs` | 型を渡す先から決める式（`new()`、コレクション式、`default`） |
//! | `lambda.rs` | ラムダ式（対象の中に式木を組み立てる）。本体の文は `lambda/statements.rs` |

use super::operators::{self, BinaryOp, EnumType, IntKind, Operand};
use super::*;
use crate::debugger::metadata::ParameterKind;
use crate::debugger::value::{ArgumentKind, Callable};

mod arithmetic;
mod assign;
mod bind;
mod calls;
mod conversions;
mod create;
mod dynamic;
mod lambda;
mod method_group;
mod names;
mod patterns;
mod pointers;
mod syntax;
mod targeted;
mod types;
mod userops;

use arithmetic::{constant_as, is_constant, primitive_operand};
pub(super) use lambda::LambdaFlow;
use syntax::{parse, Assignment, Expr, Pattern, Piece, TypeSyntax};
use types::{Kind, Typed};

const ELEMENT_TYPE_I4: u8 = 0x08;
const ELEMENT_TYPE_I8: u8 = 0x0A;
const ELEMENT_TYPE_R4: u8 = 0x0C;
const ELEMENT_TYPE_R8: u8 = 0x0D;

/// `System.Collections.Generic.Comparer` のような、名前とメンバーの並びを、点でつないだ名前にする。
fn dotted_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.clone()),
        Expr::Member(target, name) => Some(format!("{}.{name}", dotted_name(target)?)),
        _ => None,
    }
}

/// もう一度評価しても、同じ値になり、何も変えない式か（呼び出しを含まない）。型を知るためだけに評価してよいかの判断に使う。
/// プロパティの getter は実行されるが、値を読むだけのものとして扱う（変数の一覧を開いたときと同じ）。
fn repeatable(expr: &Expr) -> bool {
    match expr {
        // 呼び出しと代入は、実行すると何かが変わる。
        Expr::Call(..) | Expr::Assign(..) | Expr::Increment { .. } | Expr::New { .. } | Expr::NewArray { .. } | Expr::Named(..) | Expr::Generic(..) | Expr::Anonymous(..) | Expr::Out(..) | Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::Throw(_) | Expr::Block(_) | Expr::Await(_) | Expr::Async(_) => false,
        Expr::NewMatrix { .. } | Expr::Nested(_) | Expr::Deconstruct(..) | Expr::StackAlloc(_) => false,
        Expr::AddressOf(inner) | Expr::Dereference(inner) => repeatable(inner),
        Expr::Utf16(_) | Expr::Utf8(_) => true,
        Expr::Name(_) | Expr::Decimal(_) | Expr::Int(_) | Expr::Long(_) | Expr::UInt(_) | Expr::ULong(_) | Expr::Double(_) | Expr::Float(_) | Expr::Char(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Null | Expr::Receiver => true,
        Expr::TypeOf(_) | Expr::Default(_) | Expr::DefaultLiteral | Expr::SizeOf(_) => true,
        Expr::Checked(_, inner) | Expr::FromEnd(inner) => repeatable(inner),
        Expr::Range(start, end) => start.as_deref().is_none_or(repeatable) && end.as_deref().is_none_or(repeatable),
        // パターンの変数を作ったり、オブジェクトを作ったりする。
        Expr::Switch(..) | Expr::With(..) | Expr::Tuple(..) => false,
        Expr::Interpolated(pieces) => pieces.iter().all(|piece| match piece {
            Piece::Text(_) => true,
            Piece::Hole { expr, .. } => repeatable(expr),
        }),
        Expr::Member(target, _) | Expr::Unary(_, target) | Expr::Cast(_, target) | Expr::As(target, _) | Expr::Is(target, _) => repeatable(target),
        Expr::Index(target, indices) => repeatable(target) && indices.iter().all(repeatable),
        Expr::Binary(_, left, right) | Expr::Coalesce(left, right) | Expr::NullConditional(left, right) => repeatable(left) && repeatable(right),
        Expr::Conditional(condition, when_true, when_false) => repeatable(condition) && repeatable(when_true) && repeatable(when_false),
    }
}

/// 型の完全な名前から、名前空間を除いた名前（エラーの文に出す）。
fn csharp_type_name(full: &str) -> String {
    full.rsplit('.').next().unwrap_or(full).to_string()
}

/// プリミティブ型の値のメソッドを探すための、ランタイムでの型の名前。
fn primitive_type_name(element: u8) -> Option<&'static str> {
    crate::debugger::builtin::by_element(element).filter(|ty| ty.is_primitive()).map(|ty| ty.full_name)
}

/// 式の中の型の名前を、対象の型として解決したもの。
enum ResolvedType {
    /// プリミティブ型。中身は CorElementType。
    Primitive(u8),
    String,
    Object,
    /// メタデータにある型。
    Named { module: usize, row: u32, kind: TypeKind },
}

#[derive(Clone, Copy, PartialEq)]
enum TypeKind {
    /// クラス、インターフェイス、デリゲート。
    Class,
    Struct,
    Enum,
}

/// 完全な名前（`System.Int32`）がプリミティブ型なら、その CorElementType。
fn primitive_element(type_name: &str) -> Option<u8> {
    crate::debugger::builtin::by_full_name(type_name).filter(|ty| ty.is_primitive()).and_then(|ty| ty.element)
}

/// プリミティブ型の値の大きさ（バイト）。
fn primitive_size(element: u8) -> usize {
    match element {
        0x02 | 0x04 | 0x05 => 1,
        0x03 | 0x06 | 0x07 => 2,
        0x08 | 0x09 | 0x0C => 4,
        // nint / nuint は、対象（＝エンジン）のポインターの幅。
        0x18 | 0x19 => std::mem::size_of::<usize>(),
        _ => 8,
    }
}

/// 評価の途中の結果。型や名前空間の名前は、そのあとの `.名前` で値になる。
enum Current {
    Value(ComPtr<ICorDebugValue>),
    /// null 許容の値型（`int?`）の計算の結果。値は中身の型（`int`）で持ち、型だけが `int?`。
    Nullable(ComPtr<ICorDebugValue>),
    Type { module: usize, row: u32 },
    /// 型引数を付けたジェネリック型（`Comparer<int>`）。静的メンバーを読むのに使う。
    Generic { module: usize, row: u32, arguments: Vec<ComPtr<ICorDebugType>> },
    /// まだ型にも値にもなっていない名前の並び（`System.Collections`）。
    Namespace(String),
    /// 値を返さないメソッドを呼んだ結果。
    Void,
}

/// 式を評価した結果。
pub(super) enum Evaluated {
    Value(ComPtr<ICorDebugValue>),
    /// 型が null 許容の値型（`int?`）で、値は中身の型で持っているもの。型の表示に `?` を付ける。
    Nullable(ComPtr<ICorDebugValue>),
    /// 値を返さないメソッドの呼び出し。実行はできている。
    Void,
    /// ポインター（`p + 1`、`&x`）。表示は netcoredbg のポインターの変数と同じく、値の欄に指す先、型の欄にポインターの型。
    /// `target` は指す先（null と void* は `None`）。
    Pointer { type_name: String, target: Option<ComPtr<ICorDebugValue>>, address: u64 },
}

impl Session {
    /// フレームの文脈で式を評価する。
    ///
    /// 評価したコード（プロパティ、メソッド、インデクサー）が例外を投げたときは、その例外のオブジェクトが結果になる
    /// （netcoredbg と同じ。拡張は、結果の型が例外かどうかで失敗を見分けている）。
    /// `setVariable`（変数の欄のローカル変数と引数）: 変数に、文字列の式の値を代入する。戻り値は、代入した値。
    pub(super) fn set_local(&mut self, thread: u32, depth: usize, name: &str, value: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        self.assign_text(thread, depth, &Expr::Name(name.to_string()), None, value)
    }

    /// `setVariable`（開いたオブジェクトの中）: メンバー（`id`）か要素（`[0]`、`[1, 2]`）に代入する。
    pub(super) fn set_member(&mut self, thread: u32, owner: ComPtr<ICorDebugValue>, name: &str, value: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let place = Self::member_place(name)?;
        self.assign_text(thread, 0, &place, Some(owner), value)
    }

    /// `setVariable`（静的メンバーの節）: 型の静的フィールドかプロパティに代入する。
    pub(super) fn set_static(&mut self, thread: u32, type_name: &str, name: &str, value: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let place = Expr::Member(Box::new(Expr::Name(type_name.to_string())), name.to_string());
        self.assign_text(thread, 0, &place, None, value)
    }

    /// `setExpression`: 式が指す場所に代入する。
    pub(super) fn set_place(&mut self, thread: u32, depth: usize, expression: &str, value: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let place = Self::parse_place(expression)?;
        self.assign_text(thread, depth, &place, None, value)
    }

    /// 場所（式の形）に、文字列の式の値を代入する。`receiver` は、場所の式の中の `Expr::Receiver` が指すオブジェクト。
    fn assign_text(&mut self, thread: u32, depth: usize, target: &Expr, receiver: Option<ComPtr<ICorDebugValue>>, value: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        let value = parse(value)?;
        self.pattern_locals.clear();
        let pushed = receiver.is_some();
        if let Some(receiver) = receiver {
            self.receivers.push(receiver);
        }
        let result = self.assign(thread, depth, target, Assignment::Plain, &value);
        if pushed {
            self.receivers.pop();
        }
        self.pattern_locals.clear();
        self.value_of(result?)
    }

    /// 式の文字列を、代入の場所として読む（`setExpression` の expression）。
    fn parse_place(expression: &str) -> Result<Expr, String> {
        let place = parse(expression)?;
        if !matches!(place, Expr::Name(_) | Expr::Member(..) | Expr::Index(..)) {
            return Err(tr!("'{expression}' cannot be assigned to (only variables, fields, properties and indexers can be changed).", expression = expression));
        }
        Ok(place)
    }

    /// 変数の欄の名前（`id`、`[0]`、`[1, 2]`）を、開いたオブジェクトの中の場所の式にする。
    fn member_place(name: &str) -> Result<Expr, String> {
        match name.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            Some(indices) => {
                let indices = indices.split(',').map(|index| index.trim().parse::<i32>().map(Expr::Int)).collect::<Result<Vec<_>, _>>().map_err(|_| tr!("{name} cannot be read as an element position.", name = name))?;
                Ok(Expr::Index(Box::new(Expr::Receiver), indices))
            }
            None => Ok(Expr::Member(Box::new(Expr::Receiver), name.to_string())),
        }
    }

    /// `throw 式` の例外のオブジェクト。式は例外（`System.Exception` から派生した型）でなければならない。`throw null` は、
    /// 実行時と同じく `NullReferenceException`。
    fn thrown(&mut self, thread: u32, depth: usize, operand: &Expr) -> Result<ComPtr<ICorDebugValue>, String> {
        let current = self.evaluate_node(thread, depth, operand)?;
        let exception = self.value_of(current)?;
        if unsafe { value::argument_kind(&exception) } == ArgumentKind::Null {
            let ty = TypeSyntax::named("System.NullReferenceException");
            return self.create(thread, depth, &ty, &[], &syntax::Initializer::None);
        }
        if !unsafe { value::type_names(&exception, &self.modules) }.iter().any(|name| name == "System.Exception") {
            return Err(tr!("Only exceptions (types derived from System.Exception) can be thrown (this is {type}).", type = unsafe { value::describe(&exception, &self.modules) }.type_name));
        }
        Ok(exception)
    }

    /// `await` の結果。C# のコンパイラが作るコードと同じく、`GetAwaiter()` と `GetResult()` を呼ぶ（Task、ValueTask、
    /// `ConfigureAwait` の結果、利用者の型のどれでも同じ道）。値を返さないもの（`Task`）は `None`。
    fn await_result(&mut self, thread: u32, depth: usize, awaited: ComPtr<ICorDebugValue>) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        if unsafe { value::argument_kind(&awaited) } == ArgumentKind::Null {
            return Err(tr!("The value to await is null."));
        }
        let type_name = unsafe { value::describe(&awaited, &self.modules) }.type_name;
        let awaiter = self
            .call_method(thread, depth, Some(Current::Value(awaited)), "GetAwaiter", Vec::new())
            .map_err(|error| tr!("A value of {type} cannot be awaited (GetAwaiter cannot be called: {error}).", type = type_name, error = error))?
            .ok_or_else(|| tr!("GetAwaiter of {type} returns no value.", type = type_name))?;
        self.call_method(thread, depth, Some(Current::Value(awaiter)), "GetResult", Vec::new())
    }

    /// `DebuggerDisplay` の `{…}` の式を、表示しているオブジェクトを this として評価する。
    pub(super) fn evaluate_on(&mut self, thread: u32, this: &ComPtr<ICorDebugValue>, expression: &str) -> Result<Evaluated, String> {
        let saved = self.display_this.replace(this.clone());
        let result = self.evaluate_expression(thread, 0, expression);
        self.display_this = saved;
        result
    }

    pub(super) fn evaluate_expression(&mut self, thread: u32, depth: usize, expression: &str) -> Result<Evaluated, String> {
        let expr = parse(expression)?;
        // `null` だけの式は、型が決まらない（C# でも `var v = null;` はエラー）。
        if expr == Expr::Null {
            return Err(tr!("An expression of only null has no type (give it a type, like (string)null)."));
        }
        // パターンが作った変数は、式の中でだけ見える。前の式の分は捨てる。
        self.pattern_locals.clear();
        self.object_typed.clear();
        self.dynamic_typed.clear();
        self.eval_cache.clear();
        self.pointer_typed.clear();
        self.pointer_copies.clear();
        let result = self.evaluate_node(thread, depth, &expr).and_then(|current| match current {
            Current::Void => Ok(Evaluated::Void),
            Current::Nullable(value) => Ok(Evaluated::Nullable(value)),
            current => {
                let value = self.value_of(current)?;
                // 計算したポインターは、覚え書きを捨てる前に、表示の形にしておく。
                Ok(match self.pointer_result(thread, &value).filter(|_| unsafe { value::pointer_parts(&value) }.is_none()) {
                    Some((type_name, target, address)) => Evaluated::Pointer { type_name, target, address },
                    None => Evaluated::Value(value),
                })
            }
        });
        self.pattern_locals.clear();
        self.object_typed.clear();
        self.dynamic_typed.clear();
        self.eval_cache.clear();
        self.pointer_typed.clear();
        self.pointer_copies.clear();
        result
    }

    fn value_of(&self, current: Current) -> Result<ComPtr<ICorDebugValue>, String> {
        match current {
            Current::Value(value) | Current::Nullable(value) => Ok(value),
            Current::Type { module, row } | Current::Generic { module, row, .. } => Err(tr!("{type} is a type name. To see a value, name a member.", type = self.type_name(module, row))),
            Current::Namespace(name) => Err(tr!("No variable, member or type named '{name}' was found.", name = name)),
            Current::Void => Err(tr!("The result of a method that returns no value cannot be used in an expression.")),
        }
    }

    fn evaluate_node(&mut self, thread: u32, depth: usize, expr: &Expr) -> Result<Current, String> {
        Ok(match expr {
            Expr::Int(number) => Current::Value(self.new_int(thread, *number)?),
            Expr::Bool(flag) => Current::Value(self.new_bool(thread, *flag)?),
            Expr::Null => Current::Value(self.new_null(thread)?),
            Expr::Str(text) => Current::Value(self.new_string(thread, text).map_err(|failure| failure.describe(self))?),
            Expr::Long(number) => Current::Value(self.operand_value(thread, Operand::Int { value: *number as i128, kind: IntKind::I64 })?),
            Expr::UInt(number) => Current::Value(self.operand_value(thread, Operand::Int { value: *number as i128, kind: IntKind::U32 })?),
            Expr::ULong(number) => Current::Value(self.operand_value(thread, Operand::Int { value: *number as i128, kind: IntKind::U64 })?),
            Expr::Double(number) => Current::Value(self.operand_value(thread, Operand::Float { value: *number, single: false })?),
            Expr::Float(number) => Current::Value(self.operand_value(thread, Operand::Float { value: *number as f64, single: true })?),
            Expr::Char(unit) => Current::Value(self.new_primitive(thread, 0x03, &unit.to_le_bytes())?),
            Expr::Decimal(digits) => Current::Value(self.new_decimal(thread, depth, digits)?),
            Expr::Switch(subject, arms) => self.evaluate_switch(thread, depth, subject, arms)?,
            Expr::With(source, members) => Current::Value(self.evaluate_with(thread, depth, source, members)?),
            Expr::Tuple(items) => Current::Value(self.create_tuple(thread, depth, items)?),
            Expr::Anonymous(members) => Current::Value(self.create_anonymous(thread, depth, members)?),
            Expr::Out(_) => return Err(tr!("out / ref can be written only as call arguments.")),
            Expr::TargetNew { .. } | Expr::Collection { .. } => return Err(tr!("new(…) without a type and collection expressions ([…]) can be written only where the type is known from the argument, the assignment target or a cast (as in C#, they have no type of their own).")),
            Expr::DefaultLiteral => return Err(tr!("default without a type can be written only where the type is known from the other operand or the assignment target (elsewhere write default(type)).")),
            Expr::Lambda(..) => return Err(tr!("Write a lambda as an argument of a method that takes a delegate, or as a cast to a delegate type ((Func<int, int>)(x => x)).")),
            Expr::FromEnd(_) | Expr::Range(..) => return Err(tr!("^ and ranges (..) can be written only inside an index.")),
            Expr::Checked(checked, inner) => {
                let outer = self.checked.replace(*checked);
                let result = self.evaluate_node(thread, depth, inner);
                self.checked = outer;
                result?
            }
            Expr::Interpolated(pieces) => self.eval_interpolated(thread, depth, pieces)?,
            Expr::New { ty, arguments, initializer } => Current::Value(self.create(thread, depth, ty, arguments, initializer)?),
            Expr::NewArray { element, sizes, items } => Current::Value(self.create_array(thread, depth, element.as_ref(), sizes, items.as_deref())?),
            Expr::Assign(target, kind, value) if matches!(**target, Expr::NullConditional(..)) => self.eval_null_conditional_assign(thread, depth, target, kind, value)?,
            Expr::Assign(target, kind, value) => self.assign(thread, depth, target, *kind, value)?,
            Expr::Increment { target, decrement, prefix } => self.increment(thread, depth, target, *decrement, *prefix)?,
            // `throw 式`: 投げる例外のオブジェクトを、評価の結果にする（評価したメソッドが例外を投げたときと同じ見せ方）。
            Expr::Throw(operand) => Current::Value(self.thrown(thread, depth, operand)?),
            Expr::Block(_) | Expr::Async(_) => return Err(tr!("A block of statements { } and async can be written only in a lambda body.")),
            Expr::NewMatrix { element, dimensions, items } => Current::Value(self.create_matrix(thread, depth, element.as_ref(), dimensions, items)?),
            Expr::Nested(_) => return Err(tr!("An initializer of only { … } can be written only as \"name = { … }\" inside a new initializer.")),
            Expr::Deconstruct(targets, value) => Current::Value(self.deconstruct(thread, depth, targets, value)?),
            Expr::Utf16(units) => Current::Value(self.new_string_of_units(thread, units).map_err(|failure| failure.describe(self))?),
            Expr::Utf8(_) => return Err(tr!("A UTF-8 string literal (\"…\"u8) is a ReadOnlySpan<byte> and cannot be taken out as an evaluation result (the type can live only on the stack).")),
            Expr::Dereference(operand) => {
                let current = self.evaluate_node(thread, depth, operand)?;
                let pointer = self.value_of(current)?;
                Current::Value(self.dereference(thread, &pointer)?)
            }
            Expr::AddressOf(operand) => Current::Value(self.address_of(thread, depth, operand)?),
            Expr::StackAlloc(_) => return Err(tr!("stackalloc cannot be evaluated (space cannot be added later to the stack of a stopped method).")),
            Expr::Await(operand) => self.eval_await(thread, depth, operand)?,
            Expr::Conditional(condition, when_true, when_false) => self.eval_conditional(thread, depth, condition, when_true, when_false)?,
            Expr::Coalesce(left, right) => self.eval_coalesce(thread, depth, left, right)?,
            Expr::NullConditional(target, rest) => self.eval_null_conditional(thread, depth, target, rest)?,
            Expr::Receiver => Current::Value(self.receivers.last().cloned().ok_or_else(|| tr!("The position in the expression is not valid."))?),
            // ラムダ式をデリゲートの型にキャストする（`(Func<int, int>)(x => x * 3)`）。
            Expr::Cast(ty, operand) if matches!(**operand, Expr::Lambda(..)) => Current::Value(self.lambda_as(thread, depth, ty, operand)?),
            // `(List<int>)[1, 2]`、`(Point)new()`: キャストの型で作る。
            Expr::Cast(ty, operand) if matches!(**operand, Expr::TargetNew { .. } | Expr::Collection { .. }) => match self.build_targeted(thread, depth, operand, ty) {
                Some(result) => Current::Value(result?),
                None => return Err(tr!("The type cannot be determined.")),
            },
            // `(nint)null`: nint は、ポインター（void*）からの明示の変換を宣言していて、null のリテラルはポインターになれる。
            // （null の入った object の変数からは、取り出しになって実行時にエラー。）
            Expr::Cast(ty, operand) if **operand == Expr::Null && !ty.nullable && !ty.array && matches!(primitive_element(&ty.name), Some(0x18 | 0x19)) => {
                let element = primitive_element(&ty.name).unwrap_or(0x18);
                Current::Value(self.new_number(thread, element, &vec![0u8; primitive_size(element)])?)
            }
            Expr::Cast(ty, operand) => self.eval_cast(thread, depth, ty, operand)?,
            Expr::As(operand, ty) => self.eval_as(thread, depth, operand, ty)?,
            Expr::Is(operand, pattern) => self.eval_is(thread, depth, operand, pattern)?,
            Expr::TypeOf(ty) => self.eval_typeof(thread, depth, ty)?,
            Expr::Default(ty) => Current::Value(match self.resolve_type(thread, depth, ty)? {
                _ if ty.nullable || ty.array => self.new_null(thread)?,
                ResolvedType::Primitive(element) => self.new_primitive(thread, element as u32, &vec![0u8; primitive_size(element)])?,
                ResolvedType::String | ResolvedType::Object | ResolvedType::Named { kind: TypeKind::Class, .. } => self.new_null(thread)?,
                // 構造体と列挙型の既定値は、中身がすべて 0 の値。型引数つきの構造体（タプルなど）は、型引数を解決してから作る。
                ResolvedType::Named { .. } if !ty.arguments.is_empty() => {
                    let exact = self.runtime_type(thread, depth, ty)?;
                    self.new_object_of_type(thread, &exact).map_err(|failure| failure.describe(self))?
                }
                ResolvedType::Named { module, row, .. } => self.zero_value(thread, module, row)?,
            }),
            Expr::SizeOf(ty) => match self.resolve_type(thread, depth, ty)? {
                ResolvedType::Primitive(element) => Current::Value(self.new_int(thread, primitive_size(element) as i32)?),
                ResolvedType::Named { module, row, .. } if self.plain_type_name(module, row) == "System.Decimal" => Current::Value(self.new_int(thread, 16)?),
                _ => return Err(tr!("sizeof works only on built-in numeric types for now ({type} is not supported).", type = ty.name)),
            },
            Expr::Unary(op, operand) => self.eval_unary(thread, depth, op, operand)?,
            Expr::Binary(op, left, right) if matches!(**left, Expr::DefaultLiteral) || matches!(**right, Expr::DefaultLiteral) => self.eval_binary_with_default(thread, depth, op, left, right)?,
            Expr::Binary(op, left, right) => self.eval_binary(thread, depth, op, left, right)?,
            Expr::Name(name) => match self.resolve_name(thread, depth, name)? {
                Some(value) => Current::Value(value),
                None => self.type_or_namespace(thread, depth, name.clone()),
            },
            Expr::Member(target, name) => match self.evaluate_node(thread, depth, target)? {
                // `dynamic` の値のメンバーは、バインダで実行時に解決する。
                Current::Value(value) if self.is_dynamic(&value) => Current::Value(self.dynamic_member(thread, depth, &value, name)?),
                Current::Value(value) | Current::Nullable(value) => match self.member(thread, &value, name) {
                    Ok(member) => Current::Value(member),
                    Err(error) => match self.extension_property(thread, depth, &value, name) {
                        // C# 14 の拡張プロパティ（`words.IsEmptyNow`）は、`get_名前` の静的メソッド。
                        Some(found) => Current::Value(found),
                        // タプルの要素名（`pair.y`）は、値ではなく変数の側（PDB）に記録されている。
                        None => match &**target {
                            Expr::Name(variable) => match self.tuple_item_name(thread, depth, variable, name) {
                                Some(item) => Current::Value(self.member(thread, &value, &item)?),
                                None => return Err(error),
                            },
                            _ => return Err(error),
                        },
                    },
                },
                Current::Type { module, row } => match self.static_member(thread, depth, module, row, name)? {
                    Some(value) => Current::Value(value),
                    // 入れ子の型かもしれない。
                    None => match self.type_or_namespace(thread, depth, format!("{}.{name}", self.plain_type_name(module, row))) {
                        Current::Namespace(_) => match self.static_extension_property(thread, depth, &self.plain_type_name(module, row), name)? {
                            Some(value) => Current::Value(value),
                            None => return Err(tr!("The type {type} has no static member '{name}'.", type = self.type_name(module, row), name = name)),
                        },
                        found => found,
                    },
                },
                Current::Namespace(namespace) => self.type_or_namespace(thread, depth, format!("{namespace}.{name}")),
                Current::Generic { module, row, arguments } => match self.generic_static_member(thread, depth, module, row, &arguments, name)? {
                    Some(value) => Current::Value(value),
                    None => {
                        let shown = self.generic_display(module, row, &arguments);
                        match self.static_extension_property(thread, depth, &shown, name)? {
                            Some(value) => Current::Value(value),
                            None => return Err(tr!("The type {type} has no static member '{name}'.", type = self.type_name(module, row), name = name)),
                        }
                    }
                },
                Current::Void => return Err(tr!("The result of a method that returns no value cannot be used in an expression.")),
            },
            Expr::Index(target, indices) => self.eval_index(thread, depth, target, indices)?,
            Expr::Call(method, arguments) => self.eval_call(thread, depth, method, arguments)?,
            Expr::Named(..) => return Err(tr!("Named arguments (name: value) can be written only as call arguments.")),
            // `Comparer<int>`（このあとに .メンバー が続く）。
            Expr::Generic(inner, types) => {
                let name = dotted_name(inner).ok_or_else(|| tr!("Type arguments can be given only to a type name."))?;
                let (module, row) = self.find_type_with_arity(thread, depth, &name, types.len()).ok_or_else(|| tr!("No generic type named '{name}' was found.", name = name))?;
                let mut arguments = Vec::new();
                for ty in types {
                    arguments.push(self.runtime_type(thread, depth, ty)?);
                }
                Current::Generic { module, row, arguments }
            }
        })
    }

    /// `a?.b = c`: a が null なら、右辺を評価せずに null。そうでなければ a.b に代入する。
    fn eval_null_conditional_assign(&mut self, thread: u32, depth: usize, target: &Expr, kind: &Assignment, value: &Expr) -> Result<Current, String> {
        Ok({
            let Expr::NullConditional(base, rest) = target else { unreachable!() };
            let current = self.evaluate_node(thread, depth, base)?;
            let receiver = self.value_of(current)?;
            let receiver = match unsafe { (value::argument_kind(&receiver), value::nullable(&receiver, &self.modules)) } {
                (ArgumentKind::Null, _) | (_, Some(None)) => return Ok(Current::Value(self.new_null(thread)?)),
                (_, Some(Some(inner))) => inner,
                _ => receiver,
            };
            self.receivers.push(receiver);
            let result = self.assign(thread, depth, rest, *kind, value);
            self.receivers.pop();
            // 式の型は、値型なら、その null 許容の型（`holder?.Count = 9` は int?）。
            match result? {
                Current::Value(value) if unsafe { matches!(value::argument_kind(&value), ArgumentKind::Primitive(_) | ArgumentKind::ValueType) } => Current::Nullable(value),
                other => other,
            }
        })
    }

    /// `await 式`: 式の GetAwaiter().GetResult() を実行して、終わるまで待つ（D-034）。待っている相手は、ほかのスレッドで
    /// 進むので、この評価の間は、ほかのスレッドを止めない。
    fn eval_await(&mut self, thread: u32, depth: usize, operand: &Expr) -> Result<Current, String> {
        Ok({
            let current = self.evaluate_node(thread, depth, operand)?;
            let awaited = self.value_of(current)?;
            self.awaiting += 1;
            let result = self.await_result(thread, depth, awaited);
            self.awaiting -= 1;
            match result? {
                Some(value) => Current::Value(value),
                None => Current::Void,
            }
        })
    }

    /// `左?.続き`、`左?[添字]続き`。
    fn eval_null_conditional(&mut self, thread: u32, depth: usize, target: &Expr, rest: &Expr) -> Result<Current, String> {
        Ok({
            let current = self.evaluate_node(thread, depth, target)?;
            let value = self.value_of(current)?;
            let receiver = match unsafe { (value::argument_kind(&value), value::nullable(&value, &self.modules)) } {
                (ArgumentKind::Null, _) | (_, Some(None)) => return Ok(Current::Value(self.new_null(thread)?)),
                (_, Some(Some(inner))) => inner,
                _ => value,
            };
            self.receivers.push(receiver);
            let result = self.evaluate_node(thread, depth, rest);
            self.receivers.pop();
            // 続きの結果が値型なら、式の型は、その null 許容の型（`s?.Length` は int?）。
            match result? {
                Current::Value(value) if unsafe { matches!(value::argument_kind(&value), ArgumentKind::Primitive(_) | ArgumentKind::ValueType) } => Current::Nullable(value),
                other => other,
            }
        })
    }

    /// ラムダ式を、型が決まったデリゲート（`Func<int, int>` など）にする。
    pub(super) fn lambda_as(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, lambda: &Expr) -> Result<ComPtr<ICorDebugValue>, String> {
        let Expr::Lambda(parameters, body) = lambda else { return Err(tr!("It is not a lambda.")) };
        // 対象の中の型の、完全な名前と型引数に直してから、デリゲートの引数と戻り値の型を読む（`Func` とだけ書かれていても
        // `System.Func` として見分けられるように）。
        let runtime = self.runtime_type(thread, depth, ty)?;
        let display = unsafe { value::type_name(&runtime, &self.modules) };
        let parsed = syntax::parse_type(&display).ok_or_else(|| tr!("Cannot read the type name {name}.", name = display))?;
        let shape = crate::debugger::metadata::TypeShape { name: parsed.name.clone(), arguments: parsed.arguments.iter().map(lambda::type_syntax_text).collect() };
        let (inputs, _) = self.delegate_signature_of(thread, depth, &shape).ok_or_else(|| tr!("{type} is not a delegate type a lambda can be converted to.", type = ty.name))?;
        self.compile_lambda(thread, depth, parameters, body, &inputs, Some(&shape), false).map(|(compiled, _)| compiled)
    }

}

