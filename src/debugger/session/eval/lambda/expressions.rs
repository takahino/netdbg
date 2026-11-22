//! ラムダ式の中の `??`、`?.`、補間文字列、`new 型(…) { 初期化子 }` を、式木の節にする。どれも、C# のコンパイラが
//! 作るのと同じ値になる形に直す。
//! - `a ?? b` は `Coalesce`。`a ?? throw 式` は、右辺を a の（null を外した）型の `Throw` にする。
//! - `a?.続き` は、a を一度だけ読み、null でなければ続きを、null なら続きの型の null（値型なら、null 許容に持ち上げる）。
//! - 補間文字列は `string.Format(書式, object[])`（いまのカルチャで書式を当てるのは、C# と同じ）。
//! - `new` は `New`。初期化子は、作った値を変数に入れてから、メンバーへの代入、`Add`、インデクサーへの代入を順に実行する。

use super::*;
use crate::debugger::session::eval::syntax::{Initializer, Piece};

type Node = ComPtr<ICorDebugValue>;
type Scope = [(String, Node)];

impl Session {
    /// `左 ?? 右`。
    pub(super) fn coalesce_node(&mut self, thread: u32, depth: usize, left: &Expr, right: &Expr, scope: &Scope) -> Result<Node, String> {
        let left = self.tree(thread, depth, left, scope)?;
        if let Expr::Throw(operand) = right {
            // 結果の型は、左辺の null を外した型（`int?` なら int）。
            let ty = match self.member_node(thread, depth, &left, "Value") {
                Ok(value) if self.node_type(thread, &left)?.starts_with("System.Nullable`1[") => self.member(thread, &value, "Type")?,
                _ => self.member(thread, &left, "Type")?,
            };
            let thrown = self.throw_node(thread, depth, operand, scope, Some(ty))?;
            return self.factory(thread, depth, "Coalesce", vec![left, thrown]);
        }
        let right = self.tree(thread, depth, right, scope)?;
        if let Ok(node) = self.factory(thread, depth, "Coalesce", vec![left.clone(), right.clone()]) {
            return Ok(node);
        }
        // 右辺を左辺の型へ（`text ?? null`）、それが無理なら、参照型の左辺を右辺の型へ（`text ?? (object)1`）。
        let converted = self.converted_like(thread, depth, right.clone(), &left)?;
        if let Ok(node) = self.factory(thread, depth, "Coalesce", vec![left.clone(), converted]) {
            return Ok(node);
        }
        let widened = self.converted_like(thread, depth, left, &right)?;
        self.factory(thread, depth, "Coalesce", vec![widened, right])
    }

    /// `左?.続き`。`rest` の中の `Receiver` が、左の値。
    pub(super) fn null_conditional_node(&mut self, thread: u32, depth: usize, target: &Expr, rest: &Expr, scope: &Scope) -> Result<Node, String> {
        let value = self.tree(thread, depth, target, scope)?;
        let mut steps = Vec::new();
        let held = if self.rereadable(&value) {
            value
        } else {
            let (assign, variable) = self.stored(thread, depth, value, None)?;
            steps.push(assign);
            variable
        };
        // null 許容の値型の受け手は、中身の値（`m?.ToString()` は、int の ToString）。
        let (present, receiver) = if self.node_type(thread, &held)?.starts_with("System.Nullable`1[") {
            (self.member_node(thread, depth, &held, "HasValue")?, self.member_node(thread, depth, &held, "Value")?)
        } else {
            if self.member(thread, &held, "Type").and_then(|ty| self.member(thread, &ty, "IsValueType")).is_ok_and(|flag| self.operand(&flag) == Operand::Bool(true)) {
                return Err(tr!("The left side of ?. is a non-nullable type."));
            }
            let nothing = self.constant_node(thread, depth, &Expr::Null)?;
            (self.factory(thread, depth, "ReferenceNotEqual", vec![held.clone(), nothing])?, held)
        };
        self.flow()?.receivers.push(receiver);
        let built = self.tree(thread, depth, rest, scope);
        if let Some(flow) = self.lambda_flows.last_mut() {
            flow.receivers.pop();
        }
        let built = built?;
        let result_type = self.node_type(thread, &built)?;
        let node = if result_type == "System.Void" {
            self.factory(thread, depth, "IfThen", vec![present, built])?
        } else {
            // 値型の結果は、null 許容に持ち上げる（`w?.Length` は int?）。
            let ty = self.member(thread, &built, "Type")?;
            let is_value = self.member(thread, &ty, "IsValueType")?;
            let lifted = self.operand(&is_value) == Operand::Bool(true) && !result_type.starts_with("System.Nullable`1[");
            let (built, ty) = if lifted {
                let open = self.type_object(thread, depth, "System.Nullable<>")?;
                let arguments = self.node_array(thread, depth, "System.Type", &[ty])?;
                let nullable = self.call_method(thread, depth, Some(Current::Value(open)), "MakeGenericType", vec![arguments])?.ok_or_else(|| tr!("Cannot create the nullable type."))?;
                (self.factory(thread, depth, "Convert", vec![built, nullable.clone()])?, nullable)
            } else {
                (built, ty)
            };
            let nothing = self.factory(thread, depth, "Default", vec![ty])?;
            self.factory(thread, depth, "Condition", vec![present, built, nothing])?
        };
        if steps.is_empty() {
            return Ok(node);
        }
        steps.push(node);
        self.block_of(thread, depth, &[], &steps)
    }

    /// 補間文字列（`$"{x,5:F2}!"`）を、`string.Format(書式, object[])` にする。
    pub(super) fn interpolated_node(&mut self, thread: u32, depth: usize, pieces: &[Piece], scope: &Scope) -> Result<Node, String> {
        let (mut format, mut holes) = (String::new(), Vec::new());
        for piece in pieces {
            match piece {
                Piece::Text(text) => format.push_str(&text.replace('{', "{{").replace('}', "}}")),
                Piece::Hole { expr, alignment, format: spec } => {
                    format.push_str(&format!("{{{}", holes.len()));
                    if let Some(alignment) = alignment {
                        format.push_str(&format!(",{alignment}"));
                    }
                    if let Some(spec) = spec {
                        format.push_str(&format!(":{spec}"));
                    }
                    format.push('}');
                    let node = self.tree(thread, depth, expr, scope)?;
                    holes.push(self.convert_node(thread, depth, node, "object")?);
                }
            }
        }
        let object = self.type_object(thread, depth, "object")?;
        let holes = self.node_array(thread, depth, EXPRESSION, &holes)?;
        let arguments = self.factory(thread, depth, "NewArrayInit", vec![object, holes])?;
        let format = self.text(thread, &format)?;
        let format = self.factory(thread, depth, "Constant", vec![format])?;
        // 名前での呼び出しでは、Format(string, object) とも合うので決まらない。Format(string, object[]) を名指しする。
        let string_type = self.type_object(thread, depth, "string")?;
        let object_array = self.type_object(thread, depth, "object[]")?;
        let parameter_types = self.node_array(thread, depth, "System.Type", &[string_type.clone(), object_array])?;
        let name = self.text(thread, "Format")?;
        let method = self.call_method(thread, depth, Some(Current::Value(string_type)), "GetMethod", vec![name, parameter_types])?.ok_or_else(|| tr!("string.Format is not found."))?;
        self.factory(thread, depth, "Call", vec![method, format, arguments])
    }

    /// `new 型(引数) { 初期化子 }`。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new_object_node(&mut self, thread: u32, depth: usize, ty: &TypeSyntax, arguments: &[Expr], initializer: &Initializer, scope: &Scope) -> Result<Node, String> {
        let shown = type_syntax_text(ty);
        // デリゲートの生成（`new Func<int, int>(x => x + n)`、`new Func<int, int>(Doubled)`）。
        if let [single] = arguments {
            if matches!(single, Expr::Lambda(..)) || (matches!(single, Expr::Name(_) | Expr::Member(..) | Expr::Generic(..)) && self.tree(thread, depth, single, scope).is_err() && self.is_method_group(thread, depth, single)) {
                if let Ok(node) = self.lambda_for_type(thread, depth, &shown, single, scope) {
                    return Ok(node);
                }
            }
        }
        let type_object = self.type_object(thread, depth, &shown)?;
        // 引数にラムダ式、名前つき引数、型を渡す先から決める式があれば、コンストラクターをシグネチャから選ぶ。
        let created = if arguments.iter().any(|argument| matches!(argument, Expr::Named(..) | Expr::Lambda(..) | Expr::Out(_) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral)) {
            let owner = self.find_type_with_arity(thread, depth, &ty.name, ty.arguments.len()).ok_or_else(|| tr!("No type named '{name}' was found.", name = ty.name))?;
            let class_names = ty.arguments.iter().map(type_syntax_text).collect();
            self.call_by_signature_node(thread, depth, CallTarget::Constructor { type_object, owner, class_names }, &[], arguments, scope).map_err(|reason| tr!("Cannot create {type}: {reason}", type = shown, reason = reason))?
        } else {
            let mut nodes = Vec::new();
            for argument in arguments {
                nodes.push(self.tree(thread, depth, argument, scope)?);
            }
            self.constructor_node(thread, depth, &type_object, nodes).map_err(|reason| tr!("Cannot create {type}: {reason}", type = shown, reason = reason))?
        };
        if *initializer == Initializer::None {
            return Ok(created);
        }
        let (assign, variable) = self.stored(thread, depth, created, None)?;
        let mut steps = vec![assign];
        self.initializer_steps(thread, depth, &variable, initializer, scope, &mut steps)?;
        steps.push(variable);
        self.block_of(thread, depth, &[], &steps)
    }

    /// コンストラクターの呼び出しの節。引数の型にそのまま合うものが無ければ、引数の数が合うもののうち、引数をその仮引数の
    /// 型へ変換できる最初のもの。
    fn constructor_node(&mut self, thread: u32, depth: usize, type_object: &Node, arguments: Vec<Node>) -> Result<Node, String> {
        if arguments.is_empty() {
            if let Ok(node) = self.factory(thread, depth, "New", vec![type_object.clone()]) {
                return Ok(node);
            }
        }
        let mut types = Vec::new();
        for argument in &arguments {
            types.push(self.member(thread, argument, "Type")?);
        }
        let types = self.node_array(thread, depth, "System.Type", &types)?;
        let exact = self.call_method(thread, depth, Some(Current::Value(type_object.clone())), "GetConstructor", vec![types])?;
        if let Some(constructor) = exact.filter(|found| unsafe { value::argument_kind(found) } != ArgumentKind::Null) {
            let arguments = self.node_array(thread, depth, EXPRESSION, &arguments)?;
            return self.factory(thread, depth, "New", vec![constructor, arguments]);
        }
        let constructors = self.call_method(thread, depth, Some(Current::Value(type_object.clone())), "GetConstructors", Vec::new())?.ok_or_else(|| tr!("The constructor is not found."))?;
        for index in 0..unsafe { value::array_length(&constructors) }.unwrap_or(0) {
            let constructor = unsafe { value::array_element(&constructors, &[index]) }?.ok_or_else(|| tr!("Cannot read the constructor."))?;
            let Some(parameters) = self.call_method(thread, depth, Some(Current::Value(constructor.clone())), "GetParameters", Vec::new())? else { continue };
            if unsafe { value::array_length(&parameters) } != Some(arguments.len() as u32) {
                continue;
            }
            let mut converted = Vec::new();
            for (position, argument) in arguments.iter().enumerate() {
                let parameter = unsafe { value::array_element(&parameters, &[position as u32]) }?.ok_or_else(|| tr!("Cannot read the parameter."))?;
                let wanted = self.member(thread, &parameter, "ParameterType")?;
                match self.factory(thread, depth, "Convert", vec![argument.clone(), wanted]) {
                    Ok(node) => converted.push(node),
                    Err(_) => break,
                }
            }
            if converted.len() == arguments.len() {
                let converted = self.node_array(thread, depth, EXPRESSION, &converted)?;
                return self.factory(thread, depth, "New", vec![constructor, converted]);
            }
        }
        Err(tr!("There is no constructor that takes these arguments."))
    }

    /// 初期化子を、`target`（作った値の入った変数か、そのメンバー）に実行する節を `steps` に足す。
    fn initializer_steps(&mut self, thread: u32, depth: usize, target: &Node, initializer: &Initializer, scope: &Scope, steps: &mut Vec<Node>) -> Result<(), String> {
        match initializer {
            Initializer::None => {}
            Initializer::Members(members) => {
                for (name, expr) in members {
                    let member = self.member_node(thread, depth, target, name)?;
                    // `名前 = { … }`: メンバーを作り直さずに、いまの値の中へ。
                    if let Expr::Nested(inner) = expr {
                        self.initializer_steps(thread, depth, &member, inner, scope, steps)?;
                        continue;
                    }
                    // 型を代入先から決める値（ラムダ式、型を省いた new、コレクション式、default）は、メンバーの型で作る。
                    let value = if matches!(expr, Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral) {
                        let full = self.node_type(thread, &member)?;
                        let shown = display_of_reflection(&full).ok_or_else(|| tr!("Cannot read the type name {name}.", name = full))?;
                        match expr {
                            Expr::Lambda(..) => self.lambda_for_type(thread, depth, &shown, expr, scope)?,
                            _ => self.targeted_node(thread, depth, expr, &shown, scope)?,
                        }
                    } else {
                        self.tree(thread, depth, expr, scope)?
                    };
                    let value = self.converted_like(thread, depth, value, &member)?;
                    steps.push(self.factory(thread, depth, "Assign", vec![member, value])?);
                }
            }
            Initializer::Items(items) => {
                for item in items {
                    // 要素がラムダ式などなら、`Add` をシグネチャから選ぶ（渡す先のデリゲートの型で作る）。
                    if item.iter().any(|value| matches!(value, Expr::Lambda(..) | Expr::TargetNew { .. } | Expr::Collection { .. } | Expr::DefaultLiteral)) {
                        steps.push(self.call_by_signature_node(thread, depth, CallTarget::Instance(target.clone(), "Add".into()), &[], item, scope)?);
                        continue;
                    }
                    let mut nodes = Vec::new();
                    for value in item {
                        nodes.push(self.tree(thread, depth, value, scope)?);
                    }
                    steps.push(self.call_node(thread, depth, Some(target.clone()), None, "Add", nodes)?);
                }
            }
            Initializer::Indexed(entries) => {
                for (indices, expr) in entries {
                    let mut nodes = Vec::new();
                    for index in indices {
                        nodes.push(self.tree(thread, depth, index, scope)?);
                    }
                    let item = self.text(thread, "Item")?;
                    let indices = self.node_array(thread, depth, EXPRESSION, &nodes)?;
                    let slot = self.factory(thread, depth, "Property", vec![target.clone(), item, indices])?;
                    let value = self.tree(thread, depth, expr, scope)?;
                    let value = self.converted_like(thread, depth, value, &slot)?;
                    steps.push(self.factory(thread, depth, "Assign", vec![slot, value])?);
                }
            }
        }
        Ok(())
    }
}
