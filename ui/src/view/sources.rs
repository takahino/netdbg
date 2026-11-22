//! 読み込んだソースの一覧。止まる前にソースを開き、行の番号でブレークポイントを置ける。PDB の無いアプリの
//! モジュールは、ここで逆コンパイルするとソースになる。ファイルから開くこともできる（起動の前にも）。

use crate::actions;
use crate::state::store;
use crate::text;
use leptos::prelude::*;

#[component]
pub fn SourcesView() -> impl IntoView {
    let store = store();
    let filter = RwSignal::new(String::new());
    // タブを開いたときと、モジュールの数が変わったときに読み直す（出力の行など、ほかの変化では読まない）。
    let modules = Memo::new(move |_| store.session.with(|s| if s.is_live() { s.modules.len() } else { 0 }));
    Effect::new(move |_| {
        if modules.get() > 0 {
            actions::load_sources(store);
        }
    });
    let rows = move || {
        let wanted = filter.get();
        let current = store.source.with(|s| s.as_ref().map(|s| s.path.to_lowercase()));
        store
            .sources
            .with(|s| s.matching(&wanted))
            .into_iter()
            .map(|entry| {
                let selected = current.as_deref() == Some(entry.path.to_lowercase().as_str());
                let path = entry.path.clone();
                // カバレッジを数えていれば、ファイルの行の率。
                let rate = store.coverage_rates.with(|rates| rates.get(&entry.path.replace('/', "\\").to_lowercase()).map(|&(covered, valid)| crate::model::coverage::percent(covered, valid)));
                view! {
                    <li class:selected=selected title=entry.path.clone() on:click=move |_| actions::open_source(store, path.clone(), 1)>
                        <span class="name">{entry.name.clone()}</span>
                        {entry.decompiled.then(|| view! { <span class="origin">{text::decompiled()}</span> })}
                        {rate.map(|rate| view! { <span class="coverage-rate" title=text::coverage_rate_title()>{rate}</span> })}
                        <span class="path">{crate::model::paths::parent(&entry.path).to_string()}</span>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    let pending = move || {
        store
            .sources
            .with(|s| s.pending.clone())
            .into_iter()
            .map(|module| {
                let name = crate::model::paths::file_name(&module).to_string();
                view! {
                    <li class="pending">
                        <span class="name">{name}</span>
                        <button on:click=move |_| actions::decompile_module(store, module.clone())>{text::decompile()}</button>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };
    view! {
        <div class="bar">
            <input class="filter" placeholder=text::filter() prop:value=move || filter.get() on:input=move |e| filter.set(event_target_value(&e)) />
            <button title=text::open_file_title() on:click=move |_| store.dialog.set(true)>{text::open_file()}</button>
            <button on:click=move |_| actions::load_sources(store) disabled=move || !store.session.with(|s| s.is_live())>{text::reload()}</button>
        </div>
        <Show when=move || store.session.with(|s| !s.is_live())>
            <p class="hint">{text::sources_before_start()}</p>
        </Show>
        <ul class="list sources">{pending}{rows}</ul>
    }
}
