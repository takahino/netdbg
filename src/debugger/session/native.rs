//! ネイティブ境界の表示（D-090）: 止まったスタックの、マネージドのフレームのあいだにあるネイティブのコードを、
//! 渡った先のモジュールの名前と番地（モジュールからの差）だけで 1 行ずつ見せる。混合モード（ネイティブのステップや
//! ブレークポイント）はしない。32bit のまま残る主因の COM / ActiveX、Win32 の呼び返しで、どこを通ったかを見るため。
//!
//! - 続けて同じモジュールにいるネイティブのフレームは 1 行にまとめる（いちばん上のフレームの番地）。
//! - ランタイム自身のモジュール（coreclr.dll、clr.dll など）は、呼び出しの橋渡しなので出さない。

use super::*;
use crate::win32::ModuleRange;

/// 区切りの行に使う番号（`stackTrace` の id）。論理的な呼び出し元（async_stack.rs）とは重ならない。
const NATIVE_FRAME_BASE: i64 = 1 << 41;

/// 出さないモジュール（ランタイムと JIT。呼び出しの橋渡し）。
const RUNTIME_MODULES: [&str; 6] = ["coreclr.dll", "clr.dll", "clrjit.dll", "mscorwks.dll", "mscoreei.dll", "mscoree.dll"];

/// ネイティブのフレーム（その下のマネージドのフレームの添字と、命令の番地）を、挟む行（添字と、見せる名前）にする。
/// `hidden` のモジュール（ランタイム、マネージドのアセンブリ）は出さない。どのモジュールにも入らない番地は、同じ位置に
/// モジュールの分かった行があれば出さない（ランタイムが作った橋渡しのコード）。
pub(super) fn native_lines(natives: &[(usize, usize)], modules: &[ModuleRange], hidden: &dyn Fn(&str) -> bool) -> Vec<(usize, String)> {
    let mut lines: Vec<(usize, String, String)> = Vec::new();
    for &(position, ip) in natives {
        let (module, label) = match modules.iter().find(|m| ip >= m.base && ip < m.base + m.size) {
            Some(m) => (m.name.to_ascii_lowercase(), format!("{}+0x{:X}", m.name, ip - m.base)),
            None => (String::new(), format!("0x{ip:X}")),
        };
        if !module.is_empty() && hidden(&module) {
            continue;
        }
        // 同じ位置で、続けて同じモジュール（関数の中のフレームの重なり）なら、上のものだけ。
        if lines.iter().any(|(p, m, _)| *p == position && *m == module && !module.is_empty()) {
            continue;
        }
        lines.push((position, module, label));
    }
    let resolved: std::collections::HashSet<usize> = lines.iter().filter(|(_, m, _)| !m.is_empty()).map(|(p, ..)| *p).collect();
    lines.retain(|(p, m, _)| !m.is_empty() || !resolved.contains(p));
    lines.sort_by_key(|(p, ..)| *p);
    lines.into_iter().map(|(position, _, label)| (position, tr!("[Native] {place}", place = label))).collect()
}

/// `code` の終わり（`code` は、戻る番地の直前の 8 バイト）が、call の命令か（E8 rel32、FF /2 のよくある形）。
pub(super) fn ends_with_call(code: &[u8; 8]) -> bool {
    let call_rm = |byte: u8| byte & 0x38 == 0x10;
    code[3] == 0xE8 // E8 rel32
        || (code[2] == 0xFF && call_rm(code[3])) // FF 15 / FF 90 disp32
        || (code[1] == 0xFF && call_rm(code[2])) // FF 94 SIB disp32
        || (code[5] == 0xFF && call_rm(code[6])) // FF 50 disp8 / FF 14 24
        || (code[4] == 0xFF && call_rm(code[5])) // FF 54 SIB disp8
        || (code[6] == 0xFF && call_rm(code[7])) // FF D0 (call reg)
}

impl Session {
    /// 説明したフレーム（`described`。async の呼び出し元を挟んだあと）に、ネイティブの行を挟む。`walked` はマネージドの
    /// フレーム、`natives` はスタックウォークが知らせたネイティブのフレーム。
    pub(super) fn with_native_boundaries(&mut self, thread: u32, walked: &[ComPtr<ICorDebugFrame>], described: Vec<Value>, natives: &[(usize, usize)]) -> Vec<Value> {
        let Some(process_id) = self.target_process_id() else { return described };
        let modules = crate::win32::process_module_ranges(process_id);
        let managed: std::collections::HashSet<String> = self.modules.iter().filter_map(|m| m.path.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase)).collect();
        let hidden = |name: &str| RUNTIME_MODULES.contains(&name) || name.ends_with(".ni.dll") || managed.contains(name);
        // .NET Framework などで、スタックウォークがネイティブのコードの中をたどれないときは、マネージドでないチェーンの
        // スタックから、呼び出しから戻る番地を拾って補う。
        let mut combined = natives.to_vec();
        combined.extend(unsafe { self.scan_native_chains(thread, walked, &modules, &hidden) });
        // マネージドのフレームのあいだだけに出す。いちばん上より上（止まった所。例外の処理の中など）に出すと、クライアントが
        // それを止まった場所として選んでしまう。いちばん下（Main など）より下は、スレッドの始まりのコード。
        let managed_count = walked.len();
        let lines: Vec<(usize, String)> = native_lines(&combined, &modules, &hidden).into_iter().filter(|(position, _)| *position > 0 && *position < managed_count).collect();
        if lines.is_empty() {
            return described;
        }
        let mut out = described;
        for (index, (position, label)) in lines.into_iter().enumerate().rev() {
            let line = json!({ "id": NATIVE_FRAME_BASE + index as i64, "name": label, "line": 0, "column": 0, "presentationHint": "label" });
            let id = self.frame_id(thread, position);
            let at = out.iter().position(|f| f["id"].as_i64() == Some(id)).unwrap_or(out.len());
            out.insert(at, line);
        }
        out
    }

    /// マネージドでないチェーンのスタックの範囲を読み、ネイティブのモジュールの中を指し、直前の命令が call の値（呼び出し
    /// から戻る番地）を拾う。添字は、そのチェーンより上（スタックの浅い所）にあるマネージドのフレームの数。
    unsafe fn scan_native_chains(&self, thread: u32, walked: &[ComPtr<ICorDebugFrame>], modules: &[ModuleRange], hidden: &dyn Fn(&str) -> bool) -> Vec<(usize, usize)> {
        /// 読むスタックの大きさの上限（壊れた範囲で長く読まない）。
        const MAX_SCAN: u64 = 256 * 1024;
        let mut found = Vec::new();
        let Some(process) = self.process.as_ref() else { return found };
        let Ok(thread) = take::<ICorDebugThread>("GetThread", |out| process.GetThread(thread, out)) else { return found };
        let Ok(chains) = take::<ICorDebugChainEnum>("EnumerateChains", |out| thread.EnumerateChains(out)) else { return found };
        let frame_starts: Vec<u64> = walked
            .iter()
            .map(|frame| {
                let (mut start, mut end) = (0, 0);
                frame.GetStackRange(&mut start, &mut end);
                start
            })
            .collect();
        let is_native_code = |value: usize| modules.iter().find(|m| value >= m.base && value < m.base + m.size).is_some_and(|m| !hidden(&m.name.to_ascii_lowercase()));
        for _ in 0..1000 {
            let (mut raw, mut fetched) = (null_mut(), 0u32);
            if !chains.Next(1, &mut raw, &mut fetched).is_ok() || fetched == 0 {
                break;
            }
            let Some(chain) = ComPtr::<ICorDebugChain>::from_raw(raw) else { break };
            let mut managed = 0;
            chain.IsManaged(&mut managed);
            let (mut start, mut end) = (0u64, 0u64);
            if managed != 0 || !chain.GetStackRange(&mut start, &mut end).is_ok() || end <= start || end - start > MAX_SCAN {
                continue;
            }
            let position = frame_starts.iter().filter(|&&s| s != 0 && s < start).count();
            let mut stack = vec![0u8; (end - start) as usize];
            let mut read = 0usize;
            process.ReadMemory(start, stack.len() as u32, stack.as_mut_ptr(), &mut read);
            let word = std::mem::size_of::<usize>();
            for chunk in stack[..read.min(stack.len())].chunks_exact(word) {
                let value = usize::from_le_bytes(chunk.try_into().unwrap_or_default());
                if value < 8 || !is_native_code(value) {
                    continue;
                }
                let mut code = [0u8; 8];
                let mut got = 0usize;
                process.ReadMemory((value - 8) as u64, 8, code.as_mut_ptr(), &mut got);
                if got == 8 && ends_with_call(&code) {
                    found.push((position, value));
                }
            }
        }
        found
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn module(name: &str, base: usize, size: usize) -> ModuleRange {
        ModuleRange { name: name.into(), base, size }
    }

    #[test]
    fn native_frames_are_named_by_module_and_collapsed() {
        let modules = [module("user32.dll", 0x7000_0000, 0x10_0000), module("coreclr.dll", 0x5000_0000, 0x10_0000)];
        // マネージドのコールバック（0）の下に、user32 の 2 フレームと、ランタイムの橋渡し。その下に呼んだ側（1）。
        let natives = [(1, 0x7000_1234), (1, 0x7000_5678), (1, 0x5000_0010), (2, 0x1234_5678)];
        let hidden = |name: &str| RUNTIME_MODULES.contains(&name);
        let lines = native_lines(&natives, &modules, &hidden);
        assert_eq!(lines, vec![(1, "[ネイティブ] user32.dll+0x1234".to_string()), (2, "[ネイティブ] 0x12345678".to_string())]);
    }

    #[test]
    fn the_same_module_at_another_position_is_shown_again() {
        let modules = [module("user32.dll", 0x1000, 0x1000)];
        let lines = native_lines(&[(1, 0x1010), (3, 0x1020)], &modules, &|_: &str| false);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn an_address_outside_every_module_is_dropped_when_the_place_has_a_named_one() {
        let modules = [module("user32.dll", 0x1000, 0x1000)];
        // ランタイムの橋渡し（モジュールの外）と、チェーンから拾った user32 の戻る番地。
        let lines = native_lines(&[(2, 0x9000), (2, 0x1010)], &modules, &|_: &str| false);
        assert_eq!(lines, vec![(2, "[ネイティブ] user32.dll+0x10".to_string())]);
    }

    #[test]
    fn return_addresses_follow_a_call_instruction() {
        assert!(ends_with_call(&[0, 0, 0, 0xE8, 1, 2, 3, 4]), "E8 rel32");
        assert!(ends_with_call(&[0, 0, 0xFF, 0x15, 1, 2, 3, 4]), "FF 15 disp32");
        assert!(ends_with_call(&[0, 0, 0, 0, 0, 0, 0xFF, 0xD0]), "FF D0 (call eax)");
        assert!(ends_with_call(&[0, 0, 0, 0, 0, 0xFF, 0x50, 0x08]), "FF 50 disp8");
        assert!(!ends_with_call(&[0x90; 8]), "nop");
        assert!(!ends_with_call(&[0, 0, 0, 0, 0, 0, 0xFF, 0xE0]), "FF E0 は jmp");
    }
}
