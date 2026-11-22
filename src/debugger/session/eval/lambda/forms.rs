//! ラムダ式の中の、そのほかの書き方: タプル（要素が 8 個以上も）、`with`、分解の代入、`^` と範囲の添字、`>>>`。
//! どれも、C# のコンパイラが作るのと同じ値になる節に直す。
//! - 要素が 8 個以上のタプルは、`ValueTuple<T1, …, T7, TRest>` の入れ子（C# と同じ形）。`Item8` は `Rest.Item1`。
//! - `with` は、レコード（クラス）は `<Clone>$` の写し、構造体は値の写しに、メンバーを代入したもの。
//! - 分解の代入は、タプルなら `ItemN`、そのほかは `Deconstruct(out …)` を呼ぶ。宣言した変数は、ラムダ式の本体の外側の
//!   ブロックで宣言する（あとの式と文から名前で読める）。
//! - `^n` は「要素の数 - n」、範囲は、文字列なら `Substring`、配列なら新しい配列への `Array.Copy`、ほかは `Slice`。
//! - `>>>` は、符号なしの型にしてから `>>` で右へずらし、もとの型に戻す。

use super::*;
use crate::debugger::session::eval::syntax::OutTarget;

type Node = ComPtr<ICorDebugValue>;
type Scope = [(String, Node)];

/// エラーの文に出す、書き方の名前。
pub(super) fn construct_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Lambda(..) => tr_str!(" a lambda whose target type is unknown"),
        Expr::Generic(..) => tr_str!(" a method group (whose target type is unknown)"),
        Expr::FromEnd(_) | Expr::Range(..) => tr_str!(" ^ and ranges outside an index"),
        Expr::StackAlloc(_) => " stackalloc",
        Expr::AddressOf(_) | Expr::Dereference(_) => tr_str!(" pointers"),
        Expr::Utf8(_) => tr_str!(" a UTF-8 string (ReadOnlySpan<byte>)"),
        Expr::TargetNew { .. } => tr_str!(" new without a type (whose target type is unknown)"),
        Expr::Collection { .. } => tr_str!(" a collection expression (whose target type is unknown)"),
        Expr::DefaultLiteral => tr_str!(" default without a type (whose type is unknown)"),
        Expr::Out(_) => tr_str!(" out / ref (outside call arguments)"),
        Expr::Named(..) => tr_str!(" a named argument (outside call arguments)"),
        Expr::Nested(_) => tr_str!(" a nested initializer (outside an initializer)"),
        Expr::Tuple(_) => tr_str!(" an empty tuple"),
        _ => tr_str!(" this construct"),
    }
}

impl Session {
    /// タプルの節。要素が 8 個以上なら、8 番目からを入れ子のタプルにする（`ValueTuple<T1, …, T7, TRest>`）。
    pub(super) fn tuple_node(&mut self, thread: u32, depth: usize, mut nodes: Vec<Node>) -> Result<Node, String> {
        if nodes.len() <= 7 {
            let mut types = Vec::new();
            for node in &nodes {
                types.push(self.member(thread, node, "Type")?);
            }
            let tuple = self.type_object(thread, depth, "System.ValueTuple")?;
            let create = self.text(thread, "Create")?;
            let types = self.node_array(thread, depth, "System.Type", &types)?;
            let nodes = self.node_array(thread, depth, EXPRESSION, &nodes)?;
            return self.factory(thread, depth, "Call", vec![tuple, create, types, nodes]);
        }
        let rest = nodes.split_off(7);
        nodes.push(self.tuple_node(thread, depth, rest)?);
        let mut types = Vec::new();
        for node in &nodes {
            types.push(self.member(thread, node, "Type")?);
        }
        let open = self.type_object(thread, depth, "System.ValueTuple<,,,,,,,>")?;
        let types = self.node_array(thread, depth, "System.Type", &types)?;
        let closed = self.call_method(thread, depth, Some(Current::Value(open)), "MakeGenericType", vec![types])?.ok_or_else(|| tr!("Cannot create the tuple type."))?;
        let constructors = self.call_method(thread, depth, Some(Current::Value(closed)), "GetConstructors", Vec::new())?.ok_or_else(|| tr!("The tuple's constructor is not found."))?;
        let constructor = unsafe { value::array_element(&constructors, &[0]) }?.ok_or_else(|| tr!("The tuple's constructor is not found."))?;
        let nodes = self.node_array(thread, depth, EXPRESSION, &nodes)?;
        self.factory(thread, depth, "New", vec![constructor, nodes])
    }

    /// タプルの `ItemN`。8 番目からは、入れ子のタプル（`Rest`）の中。
    pub(super) fn tuple_item_node(&mut self, thread: u32, depth: usize, tuple: &Node, number: usize) -> Result<Node, String> {
        if number <= 7 {
            return self.member_node(thread, depth, tuple, &format!("Item{number}"));
        }
        let rest = self.member_node(thread, depth, tuple, "Rest")?;
        self.tuple_item_node(thread, depth, &rest, number - 7)
    }

    /// 何度読んでもよい節にする。そうでなければ、変数に入れる代入を `steps` に足して、その変数を返す。
    fn rereadable_node(&mut self, thread: u32, depth: usize, node: Node, steps: &mut Vec<Node>) -> Result<Node, String> {
        if self.rereadable(&node) {
            return Ok(node);
        }
        let (assign, variable) = self.stored(thread, depth, node, None)?;
        steps.push(assign);
        Ok(variable)
    }

    /// `値 with { 名前 = 値, … }`。
    pub(super) fn with_node(&mut self, thread: u32, depth: usize, source: &Expr, members: &[(String, Expr)], scope: &Scope) -> Result<Node, String> {
        let source = self.tree(thread, depth, source, scope)?;
        let ty = self.member(thread, &source, "Type")?;
        let is_value = self.member(thread, &ty, "IsValueType")?;
        // 構造体は、変数に入れれば写しになる。レコードは `<Clone>$` で写しを作る。
        let copy = if self.operand(&is_value) == Operand::Bool(true) {
            source
        } else {
            self.parameterless_call_node(thread, depth, source, "<Clone>$").map_err(|_| tr!("with can be used only on records and structs."))?
        };
        let (assign, variable) = self.stored(thread, depth, copy, None)?;
        let mut steps = vec![assign];
        for (name, value) in members {
            let member = self.member_node(thread, depth, &variable, name)?;
            let value = self.tree(thread, depth, value, scope)?;
            let value = self.converted_like(thread, depth, value, &member)?;
            steps.push(self.factory(thread, depth, "Assign", vec![member, value])?);
        }
        steps.push(variable);
        self.block_of(thread, depth, &[], &steps)
    }

    /// 分解の代入（`var (a, b) = 式`、`(x, _) = 式`）。式の値は、右辺の値。
    pub(super) fn deconstruct_node(&mut self, thread: u32, depth: usize, targets: &[OutTarget], value: &Expr, scope: &Scope) -> Result<Node, String> {
        let source = self.tree(thread, depth, value, scope)?;
        let (assign, variable) = self.stored(thread, depth, source, None)?;
        let mut steps = vec![assign];
        self.deconstruct_node_into(thread, depth, &variable, targets, scope, &mut steps)?;
        steps.push(variable);
        self.block_of(thread, depth, &[], &steps)
    }

    fn deconstruct_node_into(&mut self, thread: u32, depth: usize, source: &Node, targets: &[OutTarget], scope: &Scope, steps: &mut Vec<Node>) -> Result<(), String> {
        let parts = self.deconstructed_parts(thread, depth, source, targets.len(), steps)?;
        for (target, part) in targets.iter().zip(parts) {
            match target {
                OutTarget::Discard => {}
                OutTarget::Declare(written, name) => {
                    let ty = match written {
                        Some(written) => self.type_object(thread, depth, &type_syntax_text(written))?,
                        None => self.member(thread, &part, "Type")?,
                    };
                    let variable = self.lambda_local(thread, depth, ty, Some(name))?;
                    let part = self.converted_like(thread, depth, part, &variable)?;
                    steps.push(self.factory(thread, depth, "Assign", vec![variable, part])?);
                }
                OutTarget::Existing(place) => {
                    let place = self.assignable(thread, depth, place, scope)?;
                    let part = self.converted_like(thread, depth, part, &place)?;
                    steps.push(self.factory(thread, depth, "Assign", vec![place, part])?);
                }
                OutTarget::Tuple(inner) => {
                    let (assign, variable) = self.stored(thread, depth, part, None)?;
                    steps.push(assign);
                    self.deconstruct_node_into(thread, depth, &variable, inner, scope, steps)?;
                }
            }
        }
        Ok(())
    }

    /// 値を `count` 個に分けた節。タプルは `ItemN`、そのほかは `Deconstruct(out …)` の呼び出し（`steps` に足す）の結果。
    pub(super) fn deconstructed_parts(&mut self, thread: u32, depth: usize, source: &Node, count: usize, steps: &mut Vec<Node>) -> Result<Vec<Node>, String> {
        let full = self.node_type(thread, source)?;
        if full.starts_with("System.ValueTuple`") || full.starts_with("System.Tuple`") {
            let mut parts = Vec::new();
            for number in 1..=count {
                parts.push(self.tuple_item_node(thread, depth, source, number)?);
            }
            return Ok(parts);
        }
        // 引数が `count` 個の Deconstruct を、リフレクションで探す。
        let ty = self.member(thread, source, "Type")?;
        let name = self.text(thread, "Deconstruct")?;
        let methods = self.call_method(thread, depth, Some(Current::Value(ty)), "GetMember", vec![name])?.ok_or_else(|| tr!("Deconstruct is not found."))?;
        for index in 0..unsafe { value::array_length(&methods) }.unwrap_or(0) {
            let method = unsafe { value::array_element(&methods, &[index]) }?.ok_or_else(|| tr!("Cannot read Deconstruct."))?;
            let Some(parameters) = self.call_method(thread, depth, Some(Current::Value(method.clone())), "GetParameters", Vec::new())? else { continue };
            if unsafe { value::array_length(&parameters) } != Some(count as u32) {
                continue;
            }
            let mut parts = Vec::new();
            for position in 0..count {
                let parameter = unsafe { value::array_element(&parameters, &[position as u32]) }?.ok_or_else(|| tr!("Cannot read the arguments of Deconstruct."))?;
                let by_reference = self.member(thread, &parameter, "ParameterType")?;
                let element = self.call_method(thread, depth, Some(Current::Value(by_reference)), "GetElementType", Vec::new())?.ok_or_else(|| tr!("Cannot read the argument types of Deconstruct."))?;
                parts.push(self.lambda_local(thread, depth, element, None)?);
            }
            let arguments = self.node_array(thread, depth, EXPRESSION, &parts)?;
            steps.push(self.factory(thread, depth, "Call", vec![source.clone(), method, arguments])?);
            return Ok(parts);
        }
        Err(tr!("{type} has no Deconstruct with {count} parts.", type = keyword_of(&full), count = count))
    }

    /// `対象[^n]` と `対象[a..b]`。
    pub(super) fn special_index_node(&mut self, thread: u32, depth: usize, target: Node, index: &Expr, scope: &Scope) -> Result<Node, String> {
        let mut steps = Vec::new();
        let target = self.rereadable_node(thread, depth, target, &mut steps)?;
        let count = self.count_node(thread, depth, &target)?;
        let count = self.rereadable_node(thread, depth, count, &mut steps)?;
        let result = match index {
            Expr::FromEnd(back) => {
                let position = self.index_from_end_node(thread, depth, &count, back, scope)?;
                self.element_node(thread, depth, target, vec![position])?
            }
            Expr::Range(start, end) => {
                let start = match start.as_deref() {
                    Some(Expr::FromEnd(back)) => self.index_from_end_node(thread, depth, &count, back, scope)?,
                    Some(start) => self.tree(thread, depth, start, scope)?,
                    None => self.constant_node(thread, depth, &Expr::Int(0))?,
                };
                let end = match end.as_deref() {
                    Some(Expr::FromEnd(back)) => self.index_from_end_node(thread, depth, &count, back, scope)?,
                    Some(end) => self.tree(thread, depth, end, scope)?,
                    None => count.clone(),
                };
                let start = self.rereadable_node(thread, depth, start, &mut steps)?;
                let length = self.factory(thread, depth, "Subtract", vec![end, start.clone()])?;
                let length = self.rereadable_node(thread, depth, length, &mut steps)?;
                let full = self.node_type(thread, &target)?;
                if full == "System.String" {
                    self.call_node(thread, depth, Some(target), None, "Substring", vec![start, length])?
                } else if is_array_type(&full) {
                    // 新しい配列に写す（C# の RuntimeHelpers.GetSubArray と同じ結果）。
                    let ty = self.member(thread, &target, "Type")?;
                    let element = self.call_method(thread, depth, Some(Current::Value(ty)), "GetElementType", Vec::new())?.ok_or_else(|| tr!("The array's element type is not known."))?;
                    let bounds = self.node_array(thread, depth, EXPRESSION, std::slice::from_ref(&length))?;
                    let part = self.factory(thread, depth, "NewArrayBounds", vec![element, bounds])?;
                    let (assign, part) = self.stored(thread, depth, part, None)?;
                    steps.push(assign);
                    let zero = self.constant_node(thread, depth, &Expr::Int(0))?;
                    let array = self.type_object(thread, depth, "System.Array")?;
                    steps.push(self.call_node(thread, depth, None, Some(array), "Copy", vec![target, start, part.clone(), zero, length])?);
                    part
                } else {
                    self.call_node(thread, depth, Some(target), None, "Slice", vec![start, length])?
                }
            }
            _ => return Err(tr!("The index is not ^ or a range.")),
        };
        if steps.is_empty() {
            return Ok(result);
        }
        steps.push(result);
        self.block_of(thread, depth, &[], &steps)
    }

    /// `^n` の位置（要素の数 - n）。
    fn index_from_end_node(&mut self, thread: u32, depth: usize, count: &Node, back: &Expr, scope: &Scope) -> Result<Node, String> {
        let back = self.tree(thread, depth, back, scope)?;
        self.factory(thread, depth, "Subtract", vec![count.clone(), back])
    }

    /// `左 >>> 右`。int より小さい型は int に広げ、符号なしの型（uint / ulong）にして右へずらしてから、もとの型に戻す。
    pub(super) fn unsigned_shift_node(&mut self, thread: u32, depth: usize, left: Node, right: Node) -> Result<Node, String> {
        let right = if keyword_of(&self.node_type(thread, &right)?) == "int" { right } else { self.convert_node(thread, depth, right, "int")? };
        let keyword = keyword_of(&self.node_type(thread, &left)?).to_string();
        let (left, signed, unsigned) = match keyword.as_str() {
            "uint" | "ulong" => return self.factory(thread, depth, "RightShift", vec![left, right]),
            "int" => (left, "int", "uint"),
            "sbyte" | "byte" | "short" | "ushort" | "char" => (self.convert_node(thread, depth, left, "int")?, "int", "uint"),
            "long" => (left, "long", "ulong"),
            other => return Err(tr!(">>> cannot be used on {type}.", type = other)),
        };
        let unsigned = self.convert_node(thread, depth, left, unsigned)?;
        let shifted = self.factory(thread, depth, "RightShift", vec![unsigned, right])?;
        self.convert_node(thread, depth, shifted, signed)
    }
}
