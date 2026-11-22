//! 命令ごとの、評価スタックの深さ。
//!
//! PDB の無いモジュールでは、ランタイムは暗黙の区切り（評価スタックが空になる所、呼び出し）だけで IL とネイティブの
//! コードの対応を作る。系列点（止まれる所）は、その区切りに合わせて「スタックが空の命令」に打つ。

use super::body::{Clause, ClauseKind};
use crate::debugger::il::{Flow, Instruction};

/// 命令の、取り出す数と積む数。
pub type Effect = (u32, u32);

/// 命令ごとの、実行する前の評価スタックの深さ。たどり着かない命令は `None`。
/// `effect` は呼び出しなどシグネチャで決まる命令の出入りを返す（決まった数の命令は表のとおり）。
pub fn stack_depths(code: &[Instruction], clauses: &[Clause], effect: &dyn Fn(&Instruction) -> Effect) -> Vec<Option<u32>> {
    let mut depths = vec![None; code.len()];
    let index_of = |offset: u32| code.binary_search_by_key(&offset, |i| i.offset).ok();
    let mut work: Vec<(usize, u32)> = vec![(0, 0)];
    // 例外のハンドラーの頭: catch とフィルターは例外のオブジェクトが 1 つ積まれた状態で始まる。
    for clause in clauses {
        let entry = if matches!(clause.kind, ClauseKind::Catch | ClauseKind::Filter) { 1 } else { 0 };
        work.extend(index_of(clause.handler.start).map(|i| (i, entry)));
        if clause.kind == ClauseKind::Filter {
            work.extend(index_of(clause.filter_start).map(|i| (i, 1)));
        }
    }
    while let Some((index, depth)) = work.pop() {
        let Some(instruction) = code.get(index) else { continue };
        if depths[index].is_some() {
            continue;
        }
        depths[index] = Some(depth);
        let (pop, push) = effect(instruction);
        // 壊れた IL で負にならないよう、0 で止める。
        let after = depth.saturating_sub(pop) + push;
        let next = |work: &mut Vec<(usize, u32)>| {
            if index + 1 < code.len() {
                work.push((index + 1, after));
            }
        };
        match instruction.op.flow {
            Flow::Next | Flow::Prefix => next(&mut work),
            Flow::Conditional | Flow::Switch => {
                next(&mut work);
                work.extend(instruction.targets().iter().filter_map(|&t| index_of(t)).map(|i| (i, after)));
            }
            Flow::Branch => work.extend(instruction.targets().iter().filter_map(|&t| index_of(t)).map(|i| (i, after))),
            Flow::Leave => work.extend(instruction.targets().iter().filter_map(|&t| index_of(t)).map(|i| (i, 0))),
            Flow::Return | Flow::Throw | Flow::EndHandler => {}
        }
    }
    depths
}

/// 系列点を打つ命令の位置: スタックが空で、前置（`constrained.` など）の直後でない命令。
pub fn statement_starts(code: &[Instruction], depths: &[Option<u32>]) -> Vec<bool> {
    code.iter()
        .enumerate()
        .map(|(i, _)| depths[i] == Some(0) && !(i > 0 && code[i - 1].op.flow == Flow::Prefix))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::il::{instructions, Stack};

    fn fixed(instruction: &Instruction) -> Effect {
        let count = |stack: Stack| match stack {
            Stack::Fixed(n) => n as u32,
            Stack::Call => 0,
        };
        (count(instruction.op.pop), count(instruction.op.push))
    }

    #[test]
    fn statements_start_where_the_stack_is_empty() {
        // ldc.i4.1; ldc.i4.2; add; stloc.0; ldloc.0; brtrue.s +1; nop; ret
        let il = [0x17, 0x18, 0x58, 0x0A, 0x06, 0x2D, 0x01, 0x00, 0x2A];
        let code: Vec<_> = instructions(&il).collect();
        let depths = stack_depths(&code, &[], &fixed);
        assert_eq!(depths, [Some(0), Some(1), Some(2), Some(1), Some(0), Some(1), Some(0), Some(0)]);
        let starts = statement_starts(&code, &depths);
        assert_eq!(starts, [true, false, false, false, true, false, true, true]);
    }

    #[test]
    fn a_catch_handler_starts_with_the_exception_on_the_stack() {
        // try { nop; leave.s +2 } catch { pop; leave.s 0 } ret
        let il = [0x00, 0xDE, 0x03, 0x26, 0xDE, 0x00, 0x2A];
        let code: Vec<_> = instructions(&il).collect();
        let clause = Clause { kind: ClauseKind::Catch, try_range: 0..3, handler: 3..6, class_token: 0x0100_0001, filter_start: 0 };
        let depths = stack_depths(&code, &[clause], &fixed);
        assert_eq!(depths, [Some(0), Some(0), Some(1), Some(0), Some(0)]);
    }

    #[test]
    fn unreachable_code_has_no_depth() {
        // ret; nop
        let code: Vec<_> = instructions(&[0x2A, 0x00]).collect();
        assert_eq!(stack_depths(&code, &[], &fixed), [Some(0), None]);
    }
}
