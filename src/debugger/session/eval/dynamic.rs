//! `dynamic`（`((dynamic)x).Count`、`((dynamic)s).Substring(1, 2)`、`((dynamic)xs)[1]`）。
//!
//! C# のコンパイラと同じく、対象の中で Microsoft.CSharp のバインダ（`Binder.GetMember` / `InvokeMember` / `GetIndex`）を
//! 作り、`CallSite<Func<CallSite, object, …, object>>` に入れて、その `Target` を呼ぶ。名前の解決とオーバーロードの選択は、
//! バインダが実行時の型で行うので、プログラムの中で `dynamic` を使ったときと同じ結果になる。結果も `dynamic` として扱う。
//!
//! バインダを作る式は、C# の式の文字列として組み立てて、この評価器で評価する（対象の値は、式の中だけの変数に入れて渡す）。

use super::*;

/// .NET Framework の Microsoft.CSharp の、完全な名前。
const MICROSOFT_CSHARP_FRAMEWORK: &str = "Microsoft.CSharp, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b03f5f7f11d50a3a";
const BINDER: &str = "Microsoft.CSharp.RuntimeBinder";

/// バインダの種類。
enum Operation<'a> {
    GetMember(&'a str),
    InvokeMember(&'a str),
    GetIndex,
}

impl Session {
    /// 値が `dynamic` の式の値か（`(dynamic)x` と、その結果）。
    pub(super) fn is_dynamic(&self, value: &ComPtr<ICorDebugValue>) -> bool {
        self.dynamic_typed.iter().any(|known| known.as_raw() == value.as_raw())
    }

    pub(super) fn mark_dynamic(&mut self, value: &ComPtr<ICorDebugValue>) {
        self.dynamic_typed.push(value.clone());
    }

    /// `((dynamic)x).名前`
    pub(super) fn dynamic_member(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, name: &str) -> Result<ComPtr<ICorDebugValue>, String> {
        self.dynamic_call(thread, depth, Operation::GetMember(name), receiver, Vec::new())?.ok_or_else(|| tr!("The dynamic member '{name}' returned no value.", name = name))
    }

    /// `((dynamic)x).名前(引数, …)`。値を返さないメソッドなら `None`。
    pub(super) fn dynamic_invoke(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, name: &str, arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        self.dynamic_call(thread, depth, Operation::InvokeMember(name), receiver, arguments)
    }

    /// `((dynamic)x)[添字, …]`
    pub(super) fn dynamic_index(&mut self, thread: u32, depth: usize, receiver: &ComPtr<ICorDebugValue>, indices: Vec<ComPtr<ICorDebugValue>>) -> Result<ComPtr<ICorDebugValue>, String> {
        self.dynamic_call(thread, depth, Operation::GetIndex, receiver, indices)?.ok_or_else(|| tr!("The dynamic indexer returned no value."))
    }

    /// バインダを作って呼ぶ。値を返さないメソッドの呼び出しなら `None`。
    fn dynamic_call(&mut self, thread: u32, depth: usize, operation: Operation, receiver: &ComPtr<ICorDebugValue>, arguments: Vec<ComPtr<ICorDebugValue>>) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        self.system_type(thread, depth, &format!("{BINDER}.Binder"), "Microsoft.CSharp", MICROSOFT_CSHARP_FRAMEWORK).ok_or_else(|| tr!("Microsoft.CSharp, needed for dynamic, cannot be loaded into the target."))?;
        let context = self.binder_context(thread, depth);
        let count = arguments.len() + 1;
        let information = vec![format!("{BINDER}.CSharpArgumentInfo.Create({BINDER}.CSharpArgumentInfoFlags.None, null)"); count].join(", ");
        let information = format!("new {BINDER}.CSharpArgumentInfo[] {{ {information} }}");
        let mark = self.pattern_locals.len();
        self.pattern_locals.push(("__netdbg_dynamic_target".into(), receiver.clone()));
        for (index, argument) in arguments.iter().enumerate() {
            self.pattern_locals.push((format!("__netdbg_dynamic_argument{index}"), argument.clone()));
        }
        // 値を返すものとして呼び、値を返さないメソッドだったら（バインダが RuntimeBinderException を投げる）、結果を捨てる
        // 呼び出しとして呼び直す（C# のコンパイラも、文としての呼び出しには ResultDiscarded を付ける）。
        let mut result = self.dynamic_site_call(thread, depth, &operation, &context, &information, count, false);
        if let (Ok(Some(value)), Operation::InvokeMember(_)) = (&result, &operation) {
            if unsafe { value::type_names(value, &self.modules) }.iter().any(|name| name == "Microsoft.CSharp.RuntimeBinder.RuntimeBinderException") && self.returns_void_message(thread, value) {
                result = self.dynamic_site_call(thread, depth, &operation, &context, &information, count, true);
            }
        }
        self.pattern_locals.truncate(mark);
        let result = result?;
        if let Some(value) = &result {
            self.mark_dynamic(value);
        }
        Ok(result)
    }

    /// バインダの例外が「void を object に変換できない」か（値を返さないメソッドを、値を返すものとして呼んだ）。
    fn returns_void_message(&mut self, thread: u32, exception: &ComPtr<ICorDebugValue>) -> bool {
        self.member(thread, exception, "Message").ok().and_then(|message| unsafe { value::as_string(&message) }).is_some_and(|message| message.contains("void"))
    }

    /// `CallSite` を作り、その `Target` を呼ぶ。`discarded` は、結果を捨てる呼び出し（`Action<…>`）。
    #[allow(clippy::too_many_arguments)]
    fn dynamic_site_call(&mut self, thread: u32, depth: usize, operation: &Operation, context: &str, information: &str, count: usize, discarded: bool) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let flags = if discarded { format!("{BINDER}.CSharpBinderFlags.ResultDiscarded") } else { format!("{BINDER}.CSharpBinderFlags.None") };
        let binder = match operation {
            Operation::GetMember(name) => format!("{BINDER}.Binder.GetMember({flags}, \"{name}\", typeof({context}), {information})"),
            Operation::InvokeMember(name) => format!("{BINDER}.Binder.InvokeMember({flags}, \"{name}\", null, typeof({context}), {information})"),
            Operation::GetIndex => format!("{BINDER}.Binder.GetIndex({flags}, typeof({context}), {information})"),
        };
        let objects = vec!["object"; count].join(", ");
        let delegate = if discarded { format!("System.Action<System.Runtime.CompilerServices.CallSite, {objects}>") } else { format!("System.Func<System.Runtime.CompilerServices.CallSite, {objects}, object>") };
        let site = self.evaluate_text(thread, depth, &format!("System.Runtime.CompilerServices.CallSite<{delegate}>.Create({binder})"))?.ok_or_else(|| tr!("Cannot create the CallSite."))?;
        self.pattern_locals.push(("__netdbg_dynamic_site".into(), site));
        let arguments: String = (0..count - 1).map(|index| format!(", __netdbg_dynamic_argument{index}")).collect();
        let result = self.evaluate_text(thread, depth, &format!("__netdbg_dynamic_site.Target(__netdbg_dynamic_site, __netdbg_dynamic_target{arguments})"));
        self.pattern_locals.pop();
        let result = result?;
        if discarded {
            return Ok(None);
        }
        Ok(result.map(|value| self.unboxed(thread, value)))
    }

    /// 組み立てた式の文字列を、いまのフレームで評価する。式の中だけの変数（`__netdbg_dynamic_target` など。利用者の書く名前とは重ならない）は、`pattern_locals` から読む。
    pub(super) fn evaluate_text(&mut self, thread: u32, depth: usize, text: &str) -> Result<Option<ComPtr<ICorDebugValue>>, String> {
        let expr = syntax::parse(text)?;
        match self.evaluate_node(thread, depth, &expr)? {
            Current::Void => Ok(None),
            current => self.value_of(current).map(Some),
        }
    }

    /// バインダに渡す、呼び出す場所の型（private のメンバーが見えるかが、これで決まる）。いまのメソッドの型か、それを囲む型の
    /// うち、名前で書けるもの（コンパイラが作った型と、型引数を持つ型は書けない）。無ければ object。
    fn binder_context(&self, thread: u32, depth: usize) -> String {
        self.frame_types(thread, depth)
            .into_iter()
            .find(|(module, row)| {
                let name = self.plain_type_name(*module, *row);
                !name.contains('<') && self.modules[*module].metadata().is_some_and(|metadata| metadata.type_parameter_count(*row) == 0)
            })
            .map_or_else(|| "object".to_string(), |(module, row)| self.plain_type_name(module, row))
    }
}
