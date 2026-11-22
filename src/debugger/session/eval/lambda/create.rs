//! ラムダ式の中の `new`（配列、匿名型）を、式木の節にする。ラムダの引数を使わない `new` は、ここへ来る前に、
//! いま評価した値の定数になっている。

use super::*;

type Node = ComPtr<ICorDebugValue>;

impl Session {
    /// 節の型が `ty`（対象の中の `System.Type`）と違えば、その型への変換を挟む。`type_name` は、`ty` の `FullName`。
    pub(super) fn converted_to(&mut self, thread: u32, depth: usize, node: Node, ty: &Node, type_name: &str) -> Result<Node, String> {
        if self.node_type(thread, &node)? == type_name {
            return Ok(node);
        }
        self.factory(thread, depth, "Convert", vec![node, ty.clone()])
    }

    /// 配列の要素の節を、要素の型に揃える。型が書かれていればその型へ、無ければ（`new[] { … }`）、どの要素も受け取れる型へ。
    /// 戻り値は、要素の型（`System.Type`）と、揃えた節。
    fn array_items(&mut self, thread: u32, depth: usize, element: Option<&TypeSyntax>, items: &[Expr], scope: &[(String, Node)]) -> Result<(Node, Vec<Node>), String> {
        let mut nodes = Vec::new();
        for item in items {
            nodes.push(self.tree(thread, depth, item, scope)?);
        }
        match element {
            Some(element) => {
                let ty = self.type_object(thread, depth, &type_syntax_text(element))?;
                let name = self.member(thread, &ty, "FullName")?;
                let name = unsafe { value::as_string(&name) }.ok_or_else(|| tr!("Cannot read the name of the array's element type."))?;
                let mut converted = Vec::new();
                for node in nodes {
                    converted.push(self.converted_to(thread, depth, node, &ty, &name)?);
                }
                Ok((ty, converted))
            }
            None => {
                let sources: Vec<&Expr> = items.iter().collect();
                let nodes = self.unified(thread, depth, nodes, &sources)?;
                let first = nodes.first().ok_or_else(|| tr!("new[] { } is empty, so the element type cannot be determined."))?;
                Ok((self.member(thread, first, "Type")?, nodes))
            }
        }
    }

    /// 配列の生成（`new int[n]`、`new int[2, n]`、`new[] { a, b }`、`new long[] { a }`）。
    pub(super) fn new_array_node(&mut self, thread: u32, depth: usize, element: Option<&TypeSyntax>, sizes: &[Expr], items: Option<&[Expr]>, scope: &[(String, Node)]) -> Result<Node, String> {
        if let Some(items) = items {
            if sizes.len() > 1 {
                return Err(tr!("The array size and the number of elements do not match (elements can be written only for one-dimensional arrays)."));
            }
            let (ty, nodes) = self.array_items(thread, depth, element, items, scope)?;
            let nodes = self.node_array(thread, depth, EXPRESSION, &nodes)?;
            return self.factory(thread, depth, "NewArrayInit", vec![ty, nodes]);
        }
        let element = element.ok_or_else(|| tr!("Write the element type or the elements for an array."))?;
        let ty = self.type_object(thread, depth, &type_syntax_text(element))?;
        let mut bounds = Vec::new();
        for size in sizes {
            bounds.push(self.tree(thread, depth, size, scope)?);
        }
        let bounds = self.node_array(thread, depth, EXPRESSION, &bounds)?;
        self.factory(thread, depth, "NewArrayBounds", vec![ty, bounds])
    }

    /// 多次元の配列の、中身を書いた生成（`new[,] { { a, 1 }, { 2, b } }`）。式木には、中身を書いた多次元の配列の節が無いので、
    /// 大きさだけで作ってから、要素を一つずつ入れる。`items` は、行を優先した順の要素。
    pub(super) fn new_matrix_node(&mut self, thread: u32, depth: usize, element: Option<&TypeSyntax>, dimensions: &[usize], items: &[Expr], scope: &[(String, Node)]) -> Result<Node, String> {
        let (ty, nodes) = self.array_items(thread, depth, element, items, scope)?;
        let mut bounds = Vec::new();
        for size in dimensions {
            bounds.push(self.constant_node(thread, depth, &Expr::Int(*size as i32))?);
        }
        let bounds = self.node_array(thread, depth, EXPRESSION, &bounds)?;
        let created = self.factory(thread, depth, "NewArrayBounds", vec![ty, bounds])?;
        let (assign, array) = self.stored(thread, depth, created, None)?;
        let mut steps = vec![assign];
        for (position, node) in nodes.into_iter().enumerate() {
            // 行を優先した順の位置を、次元ごとの添字に直す。
            let (mut rest, mut indices) = (position, vec![0usize; dimensions.len()]);
            for (axis, size) in dimensions.iter().enumerate().rev() {
                indices[axis] = rest % (*size).max(1);
                rest /= (*size).max(1);
            }
            let mut index_nodes = Vec::new();
            for index in indices {
                index_nodes.push(self.constant_node(thread, depth, &Expr::Int(index as i32))?);
            }
            let index_nodes = self.node_array(thread, depth, EXPRESSION, &index_nodes)?;
            let slot = self.factory(thread, depth, "ArrayAccess", vec![array.clone(), index_nodes])?;
            steps.push(self.factory(thread, depth, "Assign", vec![slot, node])?);
        }
        steps.push(array);
        self.block_of(thread, depth, &[], &steps)
    }

    /// 匿名型（`new { A = x, B = "t" }`）。式を直に評価するときと同じく、C# のコンパイラが、同じメンバーの並びの匿名型を
    /// プログラムの中に作ってあるときだけ作れる。型引数（メンバーの型）は、メンバーの節の型。
    pub(super) fn anonymous_node(&mut self, thread: u32, depth: usize, members: &[(String, Expr)], scope: &[(String, Node)]) -> Result<Node, String> {
        let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
        let (module, row) = self.anonymous_shape(thread, depth, &names)?;
        let (mut nodes, mut types) = (Vec::new(), Vec::new());
        for (_, value) in members {
            let node = self.tree(thread, depth, value, scope)?;
            types.push(self.member(thread, &node, "Type")?);
            nodes.push(node);
        }
        // 型引数を入れる前の型（``<>f__AnonymousType0`2``）を名前で得て、メンバーの型を型引数にする。
        let arity = if members.is_empty() { String::new() } else { format!("`{}", members.len()) };
        // 書き出して読み込ませた匿名型（anonymous.rs）のアセンブリは、名前では探せない（`LoadFrom` で読み込んだため）。
        // どちらの匿名型も、モジュールのファイルからアセンブリを引いて、その中の型として得る。
        let path = self.modules[module].path.replace('"', "\"\"");
        let definition = self.evaluate_text(thread, depth, &format!("System.Reflection.Assembly.LoadFrom(@\"{path}\").GetType(\"{}{arity}\")", self.plain_type_name(module, row)))?;
        let definition = definition.filter(|found| unsafe { value::argument_kind(found) } != ArgumentKind::Null).ok_or_else(|| tr!("Cannot get the anonymous type."))?;
        let ty = if members.is_empty() {
            definition
        } else {
            let types = self.node_array(thread, depth, "System.Type", &types)?;
            self.call_method(thread, depth, Some(Current::Value(definition)), "MakeGenericType", vec![types])?.ok_or_else(|| tr!("Cannot create the anonymous type."))?
        };
        // 匿名型のコンストラクターは、メンバーを順に受け取るもの一つだけ。
        let constructors = self.call_method(thread, depth, Some(Current::Value(ty)), "GetConstructors", Vec::new())?.ok_or_else(|| tr!("The anonymous type's constructor is not found."))?;
        let constructor = unsafe { value::array_element(&constructors, &[0]) }?.ok_or_else(|| tr!("The anonymous type's constructor is not found."))?;
        let nodes = self.node_array(thread, depth, EXPRESSION, &nodes)?;
        self.factory(thread, depth, "New", vec![constructor, nodes])
    }
}
