//! ソース: 行の番号の欄でブレークポイントを置き、止まっている行を色で示す。大きなファイルでも軽いように、見えて
//! いる行（と前後の少し）だけを描く。

use crate::actions;
use crate::model::{inline, paths};
use crate::state::store;
use crate::text;
use crate::view::variables::VariableTree;
use leptos::html::Div;
use leptos::prelude::*;
use std::collections::HashMap;

/// 1 行の高さ（CSS の `.line` と同じ）。
const LINE_HEIGHT: f64 = 18.0;
/// 見えている範囲の前後に、余分に描く行。
const MARGIN: usize = 30;

#[component]
pub fn SourceView() -> impl IntoView {
    let store = store();
    let container = NodeRef::<Div>::new();
    let scroll = RwSignal::new(0.0);
    let height = RwSignal::new(800.0);
    // 止まった行が見える所まで動かす（行を真ん中あたりに）。
    Effect::new(move |_| {
        let Some(line) = store.reveal.get() else { return };
        let Some(element) = container.get() else { return };
        let view = f64::from(element.client_height());
        let top = (f64::from(line.saturating_sub(1)) * LINE_HEIGHT - view / 3.0).max(0.0);
        let current = f64::from(element.scroll_top());
        // すでに見えていれば動かさない。
        if !(top + view / 3.0 >= current && top + view / 3.0 + LINE_HEIGHT <= current + view) {
            element.set_scroll_top(top as i32);
        }
        scroll.set(f64::from(element.scroll_top()));
        height.set(view);
        store.reveal.set(None);
    });
    let on_scroll = move |_| {
        if let Some(element) = container.get_untracked() {
            scroll.set(f64::from(element.scroll_top()));
            height.set(f64::from(element.client_height()));
        }
    };
    // 止まったメソッドの最初の行から止まった行までの、行ごとの変数の値（`// a = 1`）。ローカル変数の値は、変数の欄で
    // 読んだもの（最初のスコープ）を使う（評価し直さない）。
    let inline_values = Memo::new(move |_| {
        let Some((path, line)) = store.current.get() else { return HashMap::new() };
        let Some(source) = store.source.get().filter(|s| s.path.eq_ignore_ascii_case(&path)) else { return HashMap::new() };
        let frame = store.frame.get();
        let first = store.frames.with(|frames| frames.iter().find(|f| Some(f.id) == frame).and_then(|f| f.method_lines)).map_or(line, |(first, _)| first);
        let values: HashMap<String, String> = store.variables.with(|roots| {
            roots.first().and_then(|scope| scope.children.as_ref()).into_iter().flatten().filter(|v| !v.name.contains(' ')).map(|v| (v.name.clone(), v.value.clone())).collect()
        });
        inline::inline_values(&source.lines, first, line, &values)
    });
    let title = move || store.source.with(|s| s.as_ref().map(|s| s.path.clone()).unwrap_or_default());
    let total = move || store.source.with(|s| s.as_ref().map_or(0, |s| s.lines.len()));
    let lines = move || {
        let Some(source) = store.source.get() else { return Vec::new() };
        let first = ((scroll.get() / LINE_HEIGHT) as usize).saturating_sub(MARGIN);
        let last = (((scroll.get() + height.get()) / LINE_HEIGHT) as usize + MARGIN).min(source.lines.len());
        let marks = store.breakpoints.with(|b| b.lines(&source.path));
        let current = store.current.with(|c| c.as_ref().filter(|(p, _)| p.eq_ignore_ascii_case(&source.path)).map(|(_, l)| *l));
        // カバレッジ（数えているときだけ）: 行の印と、載せたときの文言。
        let covered = |number: u32| store.coverage_file.with(|f| f.as_ref().filter(|f| f.path.eq_ignore_ascii_case(&source.path)).and_then(|f| f.lines.get(&number)).map(|l| (l.mark(), l.describe().join("\n"))));
        (first..last)
            .map(|index| {
                let number = index as u32 + 1;
                let mark = marks.iter().find(|m| m.line == number);
                let dot = match mark.map(|m| m.verified) {
                    Some(Some(false)) => "bp unverified",
                    Some(_) => "bp",
                    None => "nobp",
                };
                let tip = mark.and_then(|m| m.message.clone()).unwrap_or_default();
                let path = source.path.clone();
                // 名前には、マウスを載せたら値を出す口を付ける。
                let tokens: Vec<_> = inline::segments(&source.lines[index])
                    .into_iter()
                    .map(|segment| match segment.expression {
                        Some(expression) => view! {
                            <span class=format!("name {}", segment.kind.class())
                                on:mouseenter=move |e| actions::hover_enter(store, expression.clone(), e.client_x(), e.client_y())
                                on:mouseleave=move |_| actions::hover_leave(store)>{segment.text}</span>
                        }
                        .into_any(),
                        None => view! { <span class=segment.kind.class()>{segment.text}</span> }.into_any(),
                    })
                    .collect();
                let values = inline_values.with(|v| v.get(&number).cloned());
                let coverage = covered(number);
                let line_class = coverage.as_ref().map_or_else(|| "line".to_string(), |(mark, _)| format!("line {}", mark.class()));
                let coverage_mark = coverage.map(|(mark, describe)| view! { <span class=format!("cov {}", mark.class()) title=describe>{mark.symbol()}</span> });
                view! {
                    <div class=line_class class:current=current == Some(number) style:top=format!("{}px", index as f64 * LINE_HEIGHT)>
                        <span class="gutter" title=tip on:click=move |_| actions::toggle_breakpoint(store, path.clone(), number)>
                            <span class=dot></span>
                            {number}
                        </span>
                        {coverage_mark}
                        <code>{tokens}{values.map(|v| view! { <span class="inline">{format!("    // {v}")}</span> })}</code>
                    </div>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <section class="source-panel">
            <div class="source-title" title=title>
                {move || paths::file_name(&title()).to_string()}
                <span class="path">{title}</span>
            </div>
            <Show when=move || store.source.with(Option::is_some) fallback=|| view! { <p class="empty">{text::no_source()}</p> }>
                <div class="source" node_ref=container on:scroll=on_scroll>
                    <div class="lines" style:height=move || format!("{}px", total() as f64 * LINE_HEIGHT)>{lines}</div>
                </div>
            </Show>
            <Show when=move || store.hover.with(Option::is_some)>
                <div
                    class="hover"
                    style:left=move || format!("{}px", store.hover.with(|h| h.as_ref().map_or(0, |h| h.x)))
                    style:top=move || format!("{}px", store.hover.with(|h| h.as_ref().map_or(0, |h| h.y)) + 16)
                    on:mouseenter=move |_| store.hover_inside.set(true)
                    on:mouseleave=move |_| {
                        store.hover_inside.set(false);
                        actions::hover_leave(store);
                    }
                >
                    <VariableTree tree=store.hover_tree editable=false/>
                </div>
            </Show>
        </section>
    }
}
