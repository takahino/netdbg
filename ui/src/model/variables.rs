//! 変数の木（DAP の `scopes` と `variables`）。開いた枝の子は、開いたときに読む（`variablesReference`）。
//!
//! 節は、根からの番号の並び（`Path`）で指す。画面は `rows` で、開いている分だけを平らに並べて描く。

use serde_json::Value;

pub type Path = Vec<usize>;

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub value: String,
    pub type_name: String,
    /// 子を読むための番号（0 なら子は無い）。
    pub reference: i64,
    /// 読んだ子（まだなら `None`）。
    pub children: Option<Vec<Node>>,
    pub expanded: bool,
}

impl Node {
    fn new(name: &str, value: &str, type_name: &str, reference: i64) -> Node {
        Node { name: name.to_string(), value: value.to_string(), type_name: type_name.to_string(), reference, children: None, expanded: false }
    }
}

/// 描く 1 行。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: Path,
    pub depth: usize,
    pub name: String,
    pub value: String,
    pub type_name: String,
    pub expandable: bool,
    pub expanded: bool,
    /// 子を読むための番号（保持している経路を読むときにも使う）。
    pub reference: i64,
    /// 親の値の番号と、親が根（スコープ）か。オブジェクトのフィールドは、親の番号と名前で書き込みの見張りを置ける（D-093）。
    pub parent: i64,
    pub parent_is_root: bool,
}

/// `scopes` の答えから、根（スコープ）を作る。最初のスコープ（ローカル変数）は開いておく（重いものは開かない）。
pub fn from_scopes(body: &Value) -> Vec<Node> {
    let scopes = body["scopes"].as_array().cloned().unwrap_or_default();
    scopes
        .iter()
        .enumerate()
        .map(|(index, scope)| {
            let mut node = Node::new(scope["name"].as_str().unwrap_or(""), "", "", scope["variablesReference"].as_i64().unwrap_or(0));
            node.expanded = index == 0 && !scope["expensive"].as_bool().unwrap_or(false);
            node
        })
        .collect()
}

/// `variables` の答えから、子の並び。
pub fn from_variables(body: &Value) -> Vec<Node> {
    body["variables"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|v| Node::new(v["name"].as_str().unwrap_or(""), v["value"].as_str().unwrap_or(""), v["type"].as_str().unwrap_or(""), v["variablesReference"].as_i64().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

fn node_mut<'a>(roots: &'a mut [Node], path: &[usize]) -> Option<&'a mut Node> {
    let (first, rest) = path.split_first()?;
    let mut node = roots.get_mut(*first)?;
    for index in rest {
        node = node.children.as_mut()?.get_mut(*index)?;
    }
    Some(node)
}

/// 枝を開くか閉じる。開いて、子をまだ読んでいなければ、読むための番号を返す。
pub fn toggle(roots: &mut [Node], path: &[usize]) -> Option<i64> {
    let node = node_mut(roots, path)?;
    if node.reference == 0 {
        return None;
    }
    node.expanded = !node.expanded;
    (node.expanded && node.children.is_none()).then_some(node.reference)
}

/// 読んだ子を入れる。
pub fn set_children(roots: &mut [Node], path: &[usize], children: Vec<Node>) {
    if let Some(node) = node_mut(roots, path) {
        node.children = Some(children);
    }
}

/// 開いているのに子をまだ読んでいない節（根を作り直した直後に、開いておく分を読む）。
pub fn pending(roots: &[Node]) -> Vec<(Path, i64)> {
    let mut out = Vec::new();
    fn walk(nodes: &[Node], path: &mut Path, out: &mut Vec<(Path, i64)>) {
        for (index, node) in nodes.iter().enumerate() {
            path.push(index);
            if node.expanded {
                match &node.children {
                    None if node.reference != 0 => out.push((path.clone(), node.reference)),
                    Some(children) => walk(children, path, out),
                    None => {}
                }
            }
            path.pop();
        }
    }
    walk(roots, &mut Vec::new(), &mut out);
    out
}

/// 開いている分を、上から順に平らに並べる。
pub fn rows(roots: &[Node]) -> Vec<Row> {
    let mut out = Vec::new();
    fn walk(nodes: &[Node], parent: i64, path: &mut Path, out: &mut Vec<Row>) {
        for (index, node) in nodes.iter().enumerate() {
            path.push(index);
            out.push(Row {
                path: path.clone(),
                depth: path.len() - 1,
                name: node.name.clone(),
                value: node.value.clone(),
                type_name: node.type_name.clone(),
                expandable: node.reference != 0,
                expanded: node.expanded,
                reference: node.reference,
                parent,
                parent_is_root: path.len() <= 2,
            });
            if node.expanded {
                if let Some(children) = &node.children {
                    walk(children, node.reference, path, out);
                }
            }
            path.pop();
        }
    }
    walk(roots, 0, &mut Vec::new(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_scope_opens_and_children_are_read_once() {
        let mut roots = from_scopes(&json!({ "scopes": [{ "name": "Locals", "variablesReference": 1 }, { "name": "Statics", "variablesReference": 2 }] }));
        assert_eq!(pending(&roots), vec![(vec![0], 1)]);
        set_children(&mut roots, &[0], from_variables(&json!({ "variables": [{ "name": "a", "value": "1", "type": "int", "variablesReference": 0 }, { "name": "o", "value": "{X}", "type": "X", "variablesReference": 9 }] })));
        assert!(pending(&roots).is_empty());
        assert_eq!(toggle(&mut roots, &[0, 1]), Some(9));
        set_children(&mut roots, &[0, 1], vec![]);
        assert_eq!(toggle(&mut roots, &[0, 1]), None, "closing reads nothing");
        assert_eq!(toggle(&mut roots, &[0, 1]), None, "children already read");
        assert_eq!(toggle(&mut roots, &[0, 0]), None, "a leaf does not open");
    }

    #[test]
    fn rows_follow_the_open_branches() {
        let mut roots = from_scopes(&json!({ "scopes": [{ "name": "Locals", "variablesReference": 1 }] }));
        set_children(&mut roots, &[0], from_variables(&json!({ "variables": [{ "name": "o", "value": "{X}", "variablesReference": 9 }] })));
        set_children(&mut roots, &[0, 0], from_variables(&json!({ "variables": [{ "name": "f", "value": "2" }] })));
        assert_eq!(rows(&roots).iter().map(|r| (r.name.as_str(), r.depth)).collect::<Vec<_>>(), [("Locals", 0), ("o", 1)]);
        toggle(&mut roots, &[0, 0]);
        let names: Vec<String> = rows(&roots).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Locals", "o", "f"]);
    }
}
