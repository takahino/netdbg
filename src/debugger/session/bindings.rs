//! データバインディングの失敗（段階 12）。WinForms の `Binding` は、値の書き出し（Format: データソース → コントロール。
//! `PushData`）と書き戻し（Parse: コントロール → データソース。`PullData`）で投げられた例外を自分で捕まえ、利用者には
//! 見せない（値が戻らない、入力欄から出られない、だけになる）。止めた瞬間の一覧は拡張が持つので、エンジンは走っている
//! 間の列を取る。
//!
//! 捕まえ方: 例外の「捕まえる所が見つかった」（CATCH_HANDLER_FOUND）の通知で、捕まえるフレームが `Binding` の
//! メソッドなら、止めずに読む（プローブの記録と同じ道。probes.rs の `run_record`）。読むのは、そのフレームの `this`
//! （コントロール、プロパティ、データソースとそのメンバー）と、例外の型と文言、利用者のコードの呼び出し元。
//! .NET Framework と .NET の WinForms で、捕まえるメソッドの名前は同じ（`PushData` / `PullData`）。

use super::*;

/// 捕まえるフレームの型（この型のメソッドが例外を捕まえたら、バインディングの失敗）。
const BINDING_TYPE: &str = "System.Windows.Forms.Binding.";
/// 呼び出し元として探すフレームの数。
const CALLER_SEARCH: usize = 60;
/// 捕まえる所を自分で求めるとき、`Binding` のフレームを探す深さ。
const BINDING_SEARCH: usize = 40;

/// 例外の節から見た、フレームのいまの位置。
struct HandlerPlace {
    /// メソッド（モジュールの基底のアドレス、トークン）。
    method: (u64, u32),
    /// catch / filter の節の try の中。
    in_try: bool,
    /// どれかの節のハンドラー（またはフィルターの条件）の中。
    in_handler: bool,
    /// catch / filter の節を持つのに、いまの位置が IL に確かには対応しない（エピローグなど）。捕まえるかは分からない。
    unsure: bool,
}

impl Session {
    /// 捕まえるフレームが WinForms のバインディングなら、そのメソッドの名前（`System.Windows.Forms.Binding.PullData()`）。
    pub(super) fn binding_catch(&self, frame: &ComPtr<ICorDebugFrame>) -> Option<String> {
        let name = unsafe { self.describe_frame(0, frame) }["name"].as_str()?.to_string();
        name.starts_with(BINDING_TYPE).then_some(name)
    }

    /// 通知に捕まえるフレームが無いとき（.NET 9 以降の例外処理は、`catch when` のフィルターで捕まえると、フレームを
    /// 渡してこない。`Binding.PullData` がそう）: 捕まえる所を自分で求め、それが `Binding` のメソッドなら、その名前。
    /// 捕まえる所は、上から見て、いまの IL の位置を try に含む catch / filter の節を持つ最初のフレーム（型つきの catch の
    /// 型は比べない。`Binding` のものは、どれもすべての例外を受ける）。調べるのは、スタックに `Binding` のフレームが
    /// あるときだけ。
    ///
    /// x86 の例外処理は、catch のハンドラーを投げた所のフレームの上で動かすので、ハンドラーの中から投げると、抜けた
    /// はずのフレームと、同じメソッドのもとのフレーム（位置は try の中のまま）がスタックの下に残って見える。ハンドラーの
    /// 中にいるフレームの下に同じメソッドのフレームがあれば、そこまでを飛ばす（x64 はハンドラーを別の関数として持つので、
    /// 同じメソッドは二度出ない）。
    pub(super) fn binding_catch_on_stack(&self, thread: u32) -> Option<String> {
        let frames = self.walk_frames(thread).ok()?;
        let names: Vec<String> = frames.iter().take(BINDING_SEARCH).map(|f| unsafe { self.describe_frame(0, f) }["name"].as_str().unwrap_or("").to_string()).collect();
        // 調べるのは、探す深さの中の最後の `Binding` のフレームまで（`Binding.ParseObject` の下に `PullData` がある）。
        let binding_at = names.iter().rposition(|n| n.starts_with(BINDING_TYPE))?;
        let places: Vec<Option<HandlerPlace>> = frames[..=binding_at].iter().enumerate().map(|(depth, f)| unsafe { self.handler_place(f, depth > 0) }).collect();

        let mut depth = 0;
        while depth <= binding_at {
            if let Some(place) = &places[depth] {
                // 捕まえるかもしれないフレームの位置が確かでない: 分からないので、バインディングの失敗とは言わない。
                if place.unsure {
                    return None;
                }
                if place.in_try {
                    return names[depth].starts_with(BINDING_TYPE).then(|| names[depth].clone());
                }
                if place.in_handler {
                    if let Some(original) = (depth + 1..=binding_at).find(|&d| places[d].as_ref().is_some_and(|p| p.method == place.method)) {
                        depth = original + 1;
                        continue;
                    }
                }
            }
            depth += 1;
        }
        None
    }

    /// フレームの、いまの IL の位置が、catch / filter の節の try の中か、例外の節のハンドラーの中か。メソッドの本体
    /// （例外の節）は、対象のメモリーから読む（読み込んだモジュールは、像の並びで置かれている）。
    ///
    /// 呼び出し元のフレーム（`caller`）の位置は、呼び出しの「あと」を指す。try の最後の命令が呼び出しなら、位置は try の
    /// 終わりと同じになる（エピローグに当たって、位置が try の終わりになることもある）ので、終わりも try に含める。
    unsafe fn handler_place(&self, frame: &ComPtr<ICorDebugFrame>, caller: bool) -> Option<HandlerPlace> {
        use crate::debugger::decompile::body::{self, ClauseKind};
        let il_frame = frame.cast::<ICorDebugILFrame>().ok()?;
        let (mut token, mut offset, mut mapping) = (0u32, 0u32, 0);
        frame.GetFunctionToken(&mut token);
        il_frame.GetIP(&mut offset, &mut mapping);
        let module = self.frame_module(frame)?;
        let rva = module.metadata()?.cell(crate::debugger::metadata::METHOD_DEF, token & 0x00FF_FFFF, 0);
        let mut base = 0u64;
        module.module.GetBaseAddress(&mut base).ok().ok()?;
        let bytes = self.read_method_body(base.checked_add(u64::from(rva)).filter(|_| rva != 0)?)?;
        let clauses = body::parse(&bytes).ok()?.clauses;
        // CorDebugMappingResult: MAPPING_EXACT、MAPPING_APPROXIMATE なら、IL の位置は確か（プロローグ、エピローグ、対応なしは違う）。
        const MAPPING_EXACT: u32 = 0x10;
        const MAPPING_APPROXIMATE: u32 = 0x20;
        let sure = mapping & (MAPPING_EXACT | MAPPING_APPROXIMATE) != 0;
        let catches = clauses.iter().any(|c| matches!(c.kind, ClauseKind::Catch | ClauseKind::Filter));
        Some(HandlerPlace {
            unsure: catches && !sure,
            method: (base, token),
            in_try: clauses.iter().any(|c| matches!(c.kind, ClauseKind::Catch | ClauseKind::Filter) && (c.try_range.contains(&offset) || (caller && offset == c.try_range.end))),
            in_handler: clauses.iter().any(|c| c.handler.contains(&offset) || (c.kind == ClauseKind::Filter && (c.filter_start..c.handler.start).contains(&offset))),
        })
    }
    /// 対象のメモリーの `address` から、メソッドの本体（ヘッダー、IL、最初の例外の節の表）を読む。
    fn read_method_body(&self, address: u64) -> Option<Vec<u8>> {
        const FAT: u8 = 0x3;
        const MORE_SECTIONS: u16 = 0x8;
        const FAT_SECTION: u8 = 0x40;
        let process = self.process.as_ref()?;
        let read = |at: u64, length: usize| -> Option<Vec<u8>> {
            let mut buffer = vec![0u8; length];
            let mut got = 0usize;
            (unsafe { process.ReadMemory(at, length as u32, buffer.as_mut_ptr(), &mut got) }.is_ok() && got == length).then_some(buffer)
        };
        let header = read(address, 12)?;
        let flags = u16::from_le_bytes([header[0], header[1]]);
        // 小さい形（tiny）と、節の無い本体には、例外の節が無い。
        if header[0] & 0x3 != FAT || flags & MORE_SECTIONS == 0 {
            return Some(header);
        }
        let header_size = usize::from((flags >> 12) & 0xF) * 4;
        let code_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let sections = (header_size + code_size + 3) & !3;
        let section = read(address + sections as u64, 4)?;
        let size = if section[0] & FAT_SECTION != 0 { usize::from(section[1]) | usize::from(section[2]) << 8 | usize::from(section[3]) << 16 } else { usize::from(section[1]) };
        read(address, sections + size.max(4))
    }

    /// バインディングの失敗を読み、出来事の列に積む（止めた形にしてある）。
    pub(super) fn record_binding(&mut self, thread: u32, method: &str) -> Vec<Event> {
        // 捕まえるフレームの深さと、利用者のコードの呼び出し元（評価の前に読む。評価で対象を動かすと、フレームは使えなくなる）。
        let described = self.describe_top_frames(thread, CALLER_SEARCH);
        let Some(depth) = described.iter().position(|f| f["name"].as_str() == Some(method)) else { return Vec::new() };
        let caller = stack::user_caller(&described, depth + 1);
        let direction = if method.contains("PullData") { "parse" } else { "format" };
        let mut read = |expression: &str| self.evaluate_text_at(thread, depth, expression);
        let control = read("this.Control.Name");
        let control_type = read("this.Control.GetType().Name");
        let property = read("this.PropertyName");
        let member = read("this.BindingMemberInfo.BindingMember");
        let data_source = read("this.DataSource.GetType().FullName");
        let formatting = read("this.FormattingEnabled");
        let (exception_type, message) = match self.current_exception(thread) {
            Some(exception) => {
                let type_name = unsafe { value::describe(&exception, &self.modules) }.type_name;
                (Some(type_name), self.string_property(thread, &exception, "get_Message"))
            }
            None => (None, None),
        };
        let mut body = json!({
            "direction": direction,
            "control": control,
            "controlType": control_type,
            "property": property,
            "member": member,
            "dataSource": data_source,
            "formattingEnabled": formatting.as_deref() == Some("true"),
            "exceptionType": exception_type,
            "message": message,
            "threadId": thread,
        });
        if let Some(caller) = caller {
            body["caller"] = caller;
        }
        let arrow = if direction == "parse" { "→" } else { "←" };
        let text = format!(
            "Data binding {direction} failed: {}.{} {arrow} {}: {}: {}\n",
            body["control"].as_str().unwrap_or("?"),
            body["property"].as_str().unwrap_or("?"),
            body["member"].as_str().unwrap_or("?"),
            body["exceptionType"].as_str().unwrap_or("?"),
            body["message"].as_str().unwrap_or("")
        );
        let record = self.probes.journal.push("binding", body).to_json();
        vec![("netdbg.bindingFailed", record), ("output", json!({ "category": "console", "output": text }))]
    }

    /// フレーム（深さ）の上で式を評価し、値の文字列を返す（文字列は引用符を外す）。評価できなければ `None`。
    fn evaluate_text_at(&mut self, thread: u32, depth: usize, expression: &str) -> Option<String> {
        let frame_id = self.frame_id(thread, depth);
        let body = self.evaluate(&json!({ "expression": expression, "frameId": frame_id, "context": "watch" })).ok()?;
        let result = body["result"].as_str()?;
        if result == "null" {
            return None;
        }
        Some(crate::debugger::sqllog::unquote(result))
    }
}
