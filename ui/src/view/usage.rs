//! 使用量とメモリ: 動いている間、対象のプロセスの使用量（メモリ、CPU、GDI / USER、ハンドル、32bit のアドレス空間）を
//! 2 秒ごとに読み、直近の値の線を描く。止まっている間は、ヒープの統計（型ごとの数と大きさ）を読める。

use crate::actions;
use crate::state::store;
use crate::text;
use leptos::prelude::*;
use serde_json::Value;
use std::time::Duration;

/// 読む間隔。
const INTERVAL: Duration = Duration::from_secs(2);
/// ヒープの統計に出す型の数（大きいものから）。
const HEAP_ROWS: usize = 200;

fn megabytes(bytes: f64) -> String {
    format!("{:.1} MB", bytes / 1024.0 / 1024.0)
}

/// 直近の値の小さな線（SVG）。
fn sparkline(values: &[f64]) -> impl IntoView {
    let (width, height) = (240.0, 40.0);
    let max = values.iter().copied().fold(f64::MIN, f64::max);
    let min = values.iter().copied().fold(f64::MAX, f64::min);
    let span = (max - min).max(1.0);
    let step = if values.len() > 1 { width / (values.len() - 1) as f64 } else { 0.0 };
    let points: Vec<String> = values.iter().enumerate().map(|(i, v)| format!("{:.1},{:.1}", i as f64 * step, height - 2.0 - (v - min) / span * (height - 4.0))).collect();
    view! {
        <svg class="spark" viewBox=format!("0 0 {width} {height}") width=width height=height>
            <polyline points=points.join(" ") fill="none" stroke="currentColor" stroke-width="1.5" />
        </svg>
    }
}

#[component]
pub fn UsageView() -> impl IntoView {
    let store = store();
    // このタブを開いている間だけ読む。
    actions::sample_usage(store);
    if let Ok(handle) = set_interval_with_handle(move || actions::sample_usage(store), INTERVAL) {
        on_cleanup(move || handle.clear());
    }
    let latest = move || store.usage.with(|u| u.last().cloned());
    let series = move |key: &'static str| store.usage.with(|u| u.iter().filter_map(|s| s[key].as_f64()).collect::<Vec<_>>());
    let row = move |name: &'static str, key: &'static str, show: fn(&Value) -> String| {
        view! {
            <tr>
                <th>{name}</th>
                <td class="number">{move || latest().map(|l| show(&l)).unwrap_or_default()}</td>
                <td>{move || sparkline(&series(key))}</td>
            </tr>
        }
    };
    let heap_rows = move || {
        store.heap.with(|heap| {
            heap.as_ref()
                .and_then(|h| h["types"].as_array())
                .into_iter()
                .flatten()
                .take(HEAP_ROWS)
                .map(|t| view! { <tr><td>{t["name"].as_str().unwrap_or("").to_string()}</td><td class="number">{t["count"].as_u64().unwrap_or(0)}</td><td class="number">{megabytes(t["size"].as_f64().unwrap_or(0.0))}</td></tr> })
                .collect::<Vec<_>>()
        })
    };
    view! {
        <div class="usage">
            <Show when=move || store.usage.with(|u| !u.is_empty()) fallback=|| view! { <p class="empty">{text::usage_running_only()}</p> }>
                <table class="grid usage-table">
                    <tbody>
                        {row(text::usage_private(), "privateBytes", |l| megabytes(l["privateBytes"].as_f64().unwrap_or(0.0)))}
                        {row(text::usage_working_set(), "workingSet", |l| megabytes(l["workingSet"].as_f64().unwrap_or(0.0)))}
                        {row(text::usage_cpu(), "cpuMs", |l| format!("{:.1} s", l["cpuMs"].as_f64().unwrap_or(0.0) / 1000.0))}
                        {row(text::usage_gdi(), "gdiObjects", |l| format!("{} / {}", l["gdiObjects"], l["gdiQuota"]))}
                        {row(text::usage_user(), "userObjects", |l| format!("{} / {}", l["userObjects"], l["userQuota"]))}
                        {row(text::usage_handles(), "handles", |l| l["handles"].to_string())}
                        {row(text::usage_address(), "addressUsed", |l| if l["wow64"] == true { format!("{} / {}（{} {}）", megabytes(l["addressUsed"].as_f64().unwrap_or(0.0)), megabytes(l["addressLimit"].as_f64().unwrap_or(0.0)), text::usage_largest_free(), megabytes(l["largestFree"].as_f64().unwrap_or(0.0))) } else { "-".into() })}
                    </tbody>
                </table>
            </Show>
            <div class="bar">
                <button on:click=move |_| actions::load_heap(store) disabled=move || !store.session.with(|s| s.is_stopped() || s.dump)>{text::heap_stats()}</button>
                <span class="hint">{text::heap_stopped_only()}</span>
            </div>
            <Show when=move || store.heap.with(Option::is_some)>
                // COM オブジェクト（RCW）の数と、前の統計からの増分（解放されない COM の漏れ。D-092）。
                <p class="hint com-objects">{move || store.heap.with(|h| h.as_ref().map(|h| com_objects_text(&h["comObjects"])).unwrap_or_default())}</p>
                <table class="grid heap"><tbody>{heap_rows}</tbody></table>
            </Show>
        </div>
    }
}

/// COM オブジェクト（RCW）の行（`COM オブジェクト（RCW）: 12（前の統計から +10）`）。
fn com_objects_text(com: &serde_json::Value) -> String {
    let count = com["count"].as_u64().unwrap_or(0);
    match com["delta"].as_i64() {
        Some(delta) => crate::i18n::trf("COM objects (RCW): {count} ({delta} since the previous statistics)", &[("count", count.to_string()), ("delta", format!("{delta:+}"))]),
        None => crate::i18n::trf("COM objects (RCW): {count}", &[("count", count.to_string())]),
    }
}
