//! 画面の部品（Leptos）。配置は `App`: 上にツールバー、左にソース、右に変数・ウォッチ・スタック・スレッドのタブ、
//! 下に出力・AI の操作・モジュール・画面のタブ。

mod agent;
mod diff;
mod exceptions;
mod format;
pub mod open;
mod output;
mod records;
mod report;
mod source;
mod sources;
mod stack;
mod toolbar;
mod usage;
pub mod variables;
mod windows;

use crate::state::{store, Tab};
use crate::text;
use leptos::prelude::*;

/// タブの見出しの並び。
#[component]
fn Tabs(signal: RwSignal<Tab>, tabs: Vec<(Tab, &'static str)>) -> impl IntoView {
    view! {
        <div class="tabs">
            {tabs.into_iter().map(|(tab, label)| view! {
                <button class:active=move || signal.get() == tab on:click=move |_| signal.set(tab)>{label}</button>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn Layout() -> impl IntoView {
    let store = store();
    let side = move || match store.side.get() {
        Tab::Watch => view! { <variables::WatchView/> }.into_any(),
        Tab::Stack => view! { <stack::StackView/> }.into_any(),
        Tab::Threads => view! { <stack::ThreadsView/> }.into_any(),
        Tab::Sources => view! { <sources::SourcesView/> }.into_any(),
        _ => view! { <variables::VariablesView/> }.into_any(),
    };
    let bottom = move || match store.bottom.get() {
        Tab::Agent => view! { <agent::AgentView/> }.into_any(),
        Tab::Records => view! { <records::RecordsView/> }.into_any(),
        Tab::Usage => view! { <usage::UsageView/> }.into_any(),
        Tab::Diff => view! { <diff::DiffView/> }.into_any(),
        Tab::Report => view! { <report::ReportView/> }.into_any(),
        Tab::Modules => view! { <output::ModulesView/> }.into_any(),
        Tab::Windows => view! { <windows::WindowsView/> }.into_any(),
        _ => view! { <output::OutputView/> }.into_any(),
    };
    view! {
        <toolbar::Toolbar/>
        <main class="layout">
            <source::SourceView/>
            <section class="side">
                <Tabs signal=store.side tabs=vec![(Tab::Sources, text::sources()), (Tab::Variables, text::variables()), (Tab::Watch, text::watch()), (Tab::Stack, text::stack()), (Tab::Threads, text::threads())]/>
                <div class="pane">{side}</div>
            </section>
            <section class="bottom">
                <Tabs signal=store.bottom tabs=vec![(Tab::Output, text::output()), (Tab::Records, text::records()), (Tab::Usage, text::usage()), (Tab::Diff, text::diff()), (Tab::Report, text::report()),(Tab::Agent, text::agent()), (Tab::Modules, text::modules()), (Tab::Windows, text::windows())]/>
                <div class="pane">{bottom}</div>
            </section>
        </main>
        <open::OpenDialog/>
        <exceptions::ExceptionsDialog/>
        <variables::RetentionPanel/>
    }
}
