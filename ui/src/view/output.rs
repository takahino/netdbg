//! 出力（対象の標準出力と標準エラー、エンジンの知らせ）と、式の評価の入力。モジュールの一覧も、ここ。

use crate::actions;
use crate::state::store;
use crate::text;
use leptos::html::Div;
use leptos::prelude::*;

/// 描く出力の行の数（それより古いものは、エンジンの記録と、MCP の `output` で読む）。
const SHOWN: usize = 1500;

#[component]
pub fn OutputView() -> impl IntoView {
    let store = store();
    let input = RwSignal::new(String::new());
    let container = NodeRef::<Div>::new();
    // 新しい行が来たら、いちばん下にいるときだけ下まで送る。
    Effect::new(move |_| {
        store.session.with(|s| s.output.len());
        if let Some(element) = container.get() {
            let near_bottom = element.scroll_height() - element.scroll_top() - element.client_height() < 60;
            if near_bottom {
                element.set_scroll_top(element.scroll_height());
            }
        }
    });
    // 行は番号で見分けて、増えた分だけを描き足す（出力の多い対象で、行ごとに全部を描き直さない）。
    let shown = Memo::new(move |_| {
        store.session.with(|s| {
            let skip = s.output.len().saturating_sub(SHOWN);
            s.output[skip..].to_vec()
        })
    });
    view! {
        <div class="output" node_ref=container>
            <For each=move || shown.get() key=|line| line.number let:line>
                <span class=format!("out {}", line.category)>{line.text}</span>
            </For>
        </div>
        <input
            class="expression"
            placeholder=text::evaluate()
            prop:value=move || input.get()
            on:input=move |e| input.set(event_target_value(&e))
            on:keydown=move |e| {
                if e.key() == "Enter" {
                    actions::evaluate(store, input.get_untracked());
                    input.set(String::new());
                }
            }
        />
    }
}

#[component]
pub fn ModulesView() -> impl IntoView {
    let store = store();
    // セッションのほかの変化（出力の行など）では描き直さない。
    let modules = Memo::new(move |_| store.session.with(|s| s.modules.clone()));
    let rows = move || {
        modules.get().into_iter().map(|m| view! { <tr><td>{m.name}</td><td class="symbols">{m.symbols}</td><td class="path">{m.path}</td></tr> }).collect::<Vec<_>>()
    };
    view! {
        <table class="modules">
            <tbody>{rows}</tbody>
        </table>
    }
}
