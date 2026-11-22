//! 画面の状態の置き場（Leptos のシグナル）。どのパネルも `store()` で同じものを使う。

use crate::model::breakpoints::Breakpoints;
use crate::model::coverage::FileCoverage;
use std::collections::HashMap;
use crate::model::csharp::Token;
use crate::model::exceptions::ExceptionSettings;
use crate::model::records::Kind;
use crate::model::session::Session;
use std::collections::BTreeSet;
use crate::model::sources::Sources;
use crate::model::stack::{Frame, Thread};
use crate::model::variables::Node;
use crate::model::windows::Tree;
use leptos::prelude::*;
use serde_json::Value;
use std::sync::Arc;

/// 開いているソース。
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub path: String,
    /// 行ごとの字句（色付けしたもの）。大きいので、写さずに共有する。
    pub lines: Arc<Vec<Vec<Token>>>,
}

/// ウォッチの式 1 つと、最後に評価した結果。
#[derive(Clone, Debug, PartialEq)]
pub struct Watch {
    pub expression: String,
    pub result: Option<Result<String, String>>,
}

/// マウスを載せた名前の値の窓（データヒント）。座標は画面の左上から。
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    pub x: i32,
    pub y: i32,
}

/// 下と右のパネルのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Sources,
    Variables,
    Watch,
    Stack,
    Threads,
    Output,
    Records,
    Usage,
    Diff,
    Report,
    Agent,
    Modules,
    Windows,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub session: RwSignal<Session>,
    /// SSE のこれまでの分を受け終えたか（それまでは、出来事に応じて動かない）。
    pub replayed: RwSignal<bool>,
    pub threads: RwSignal<Vec<Thread>>,
    /// いま見ているスレッド。
    pub thread: RwSignal<Option<i64>>,
    pub frames: RwSignal<Vec<Frame>>,
    /// いま見ているフレーム。
    pub frame: RwSignal<Option<i64>>,
    pub source: RwSignal<Option<Source>>,
    /// 実行が止まっている場所（いま見ているフレームのファイルと行）。
    pub current: RwSignal<Option<(String, u32)>>,
    /// ソースを、この行が見える所まで動かす（動かしたら消す）。
    pub reveal: RwSignal<Option<u32>>,
    pub breakpoints: RwSignal<Breakpoints>,
    /// 読み込んだソースの一覧。
    pub sources: RwSignal<Sources>,
    /// 一覧を読んでいる途中か、その間にまた頼まれたか（モジュールが続けて読み込まれる間、何度も読まない）。
    pub sources_loading: RwSignal<(bool, bool)>,
    pub variables: RwSignal<Vec<Node>>,
    /// マウスを載せた名前の値の窓と、その値の木（根が 1 つ）。
    pub hover: RwSignal<Option<Hover>>,
    pub hover_tree: RwSignal<Vec<Node>>,
    /// 載せた / 外したごとに増やす（遅れて届いた評価や、外したあとの表示を捨てる）。
    pub hover_generation: RwSignal<u64>,
    /// マウスが値の窓の中にある（窓を開いたまま、中の木を開ける）。
    pub hover_inside: RwSignal<bool>,
    pub watches: RwSignal<Vec<Watch>>,
    pub dialog: RwSignal<bool>,
    /// 開く画面のフォルダー。
    pub folder: RwSignal<String>,
    pub error: RwSignal<Option<String>>,
    /// ビット数が合わずに起動できなかったとき、隣のエンジンで開き直す要求（`/relaunch` の本文）。
    pub relaunch: RwSignal<Option<Value>>,
    /// 最後に始めたもの（もう一度始めるため）。
    pub last_start: RwSignal<Option<Value>>,
    pub tree: RwSignal<Option<Tree>>,
    /// 画面の木で選んだコントロール（印の番号）。
    pub control: RwSignal<Option<u64>>,
    /// 写しを撮り直すたびに増やす（`<img>` の URL を変える）。
    pub shot: RwSignal<u64>,
    /// 記録のタブ: 見せる種類、文字の絞り込み、選んだ記録（`Record::key`）。
    pub record_kinds: RwSignal<BTreeSet<Kind>>,
    pub record_filter: RwSignal<String>,
    pub record: RwSignal<Option<u64>>,
    /// SQL と例外を記録しているか（起動の引数と、記録のタブの切り替え）。
    pub sql_log: RwSignal<bool>,
    pub exception_log: RwSignal<bool>,
    /// 「SQL で止める」「ダイアログで止める」の条件（含む文字）。
    pub sql_breaks: RwSignal<Vec<String>>,
    pub dialog_breaks: RwSignal<Vec<String>>,
    /// 例外の設定（どの例外で止まるか。ブラウザに覚える）と、その窓を開いているか。
    pub exception_settings: RwSignal<ExceptionSettings>,
    pub exception_dialog: RwSignal<bool>,
    /// 使用量の直近の値（`netdbg.processUsage` の答え。古い順）。
    pub usage: RwSignal<Vec<Value>>,
    /// ヒープの統計（止まっている間に読んだもの）。
    pub heap: RwSignal<Option<Value>>,
    /// 保持している経路の答え（窓に出す）。
    pub retention: RwSignal<Option<Value>>,
    /// 書き込みを見張っているフィールドの説明（D-093。セッションの間は止まり直しても残る）。
    pub data_watch: RwSignal<Option<String>>,
    /// カバレッジを取るか（起動の前に選ぶ。ブラウザに覚える）、開いているファイルの行ごとの状態、ファイルごとの率
    /// （キーは比べる形のパス）。
    pub coverage_on: RwSignal<bool>,
    pub coverage_file: RwSignal<Option<FileCoverage>>,
    pub coverage_rates: RwSignal<HashMap<String, (u64, u64)>>,
    /// 差分のタブ: 前の exe（フォルダー。ブラウザに覚える）、答え、再試験の範囲だけを見せるか。
    pub diff_old: RwSignal<String>,
    pub diff: RwSignal<Option<Value>>,
    pub diff_retest_only: RwSignal<bool>,
    /// 成績書のタブ: 次の項目の番号と項目名、備考、最後に書いたファイル。
    pub report_id: RwSignal<String>,
    pub report_title: RwSignal<String>,
    pub report_note: RwSignal<String>,
    pub report_written: RwSignal<Option<String>>,
    /// 流す手順のファイル（成績書を書くと、そのとき書いた `.steps.json`。ブラウザに覚える）と、流す項目（空ならすべて）。
    pub replay_file: RwSignal<String>,
    pub replay_item: RwSignal<String>,
    /// 成績書とカバレッジの XML の保存先（フォルダー。空なら既定の場所。ブラウザに覚える）。
    pub output_folder: RwSignal<String>,
    /// 成績書の現場のテンプレートのフォルダー（空なら既定のテンプレート。ブラウザに覚える。D-083）。
    pub report_template: RwSignal<String>,
    pub side: RwSignal<Tab>,
    pub bottom: RwSignal<Tab>,
}

impl Store {
    pub fn new() -> Store {
        Store {
            session: RwSignal::new(Session::default()),
            replayed: RwSignal::new(false),
            threads: RwSignal::new(Vec::new()),
            thread: RwSignal::new(None),
            frames: RwSignal::new(Vec::new()),
            frame: RwSignal::new(None),
            source: RwSignal::new(None),
            current: RwSignal::new(None),
            reveal: RwSignal::new(None),
            breakpoints: RwSignal::new(Breakpoints::default()),
            sources: RwSignal::new(Sources::default()),
            sources_loading: RwSignal::new((false, false)),
            variables: RwSignal::new(Vec::new()),
            hover: RwSignal::new(None),
            hover_tree: RwSignal::new(Vec::new()),
            hover_generation: RwSignal::new(0),
            hover_inside: RwSignal::new(false),
            watches: RwSignal::new(Vec::new()),
            dialog: RwSignal::new(false),
            folder: RwSignal::new(String::new()),
            error: RwSignal::new(None),
            relaunch: RwSignal::new(None),
            last_start: RwSignal::new(None),
            tree: RwSignal::new(None),
            control: RwSignal::new(None),
            shot: RwSignal::new(0),
            record_kinds: RwSignal::new(Kind::ALL.into_iter().collect()),
            record_filter: RwSignal::new(String::new()),
            record: RwSignal::new(None),
            sql_log: RwSignal::new(crate::api::remembered("sqlLog", true)),
            exception_log: RwSignal::new(crate::api::remembered("exceptionLog", true)),
            sql_breaks: RwSignal::new(Vec::new()),
            dialog_breaks: RwSignal::new(Vec::new()),
            exception_settings: RwSignal::new(crate::api::remembered_text("exceptionSettings").map(|t| ExceptionSettings::from_json(&t)).unwrap_or_default()),
            exception_dialog: RwSignal::new(false),
            usage: RwSignal::new(Vec::new()),
            heap: RwSignal::new(None),
            retention: RwSignal::new(None),
            data_watch: RwSignal::new(None),
            coverage_on: RwSignal::new(crate::api::remembered("coverage", false)),
            coverage_file: RwSignal::new(None),
            coverage_rates: RwSignal::new(HashMap::new()),
            diff_old: RwSignal::new(crate::api::remembered_text("diffOld").unwrap_or_default()),
            diff: RwSignal::new(None),
            diff_retest_only: RwSignal::new(false),
            report_id: RwSignal::new(String::new()),
            report_title: RwSignal::new(String::new()),
            report_note: RwSignal::new(String::new()),
            report_written: RwSignal::new(None),
            replay_file: RwSignal::new(crate::api::remembered_text("replayFile").unwrap_or_default()),
            replay_item: RwSignal::new(String::new()),
            output_folder: RwSignal::new(crate::api::remembered_text("outputFolder").unwrap_or_default()),
            report_template: RwSignal::new(crate::api::remembered_text("reportTemplate").unwrap_or_default()),
            side: RwSignal::new(Tab::Sources),
            bottom: RwSignal::new(Tab::Output),
        }
    }

    /// 保存先（入れてあれば）。エンジンに渡す `folder`。
    pub fn output_folder(&self) -> Option<String> {
        Some(self.output_folder.get_untracked().trim().to_string()).filter(|f| !f.is_empty())
    }

    /// 成績書のテンプレートのフォルダー（入れてあれば）。エンジンに渡す `template`。
    pub fn report_template(&self) -> Option<String> {
        Some(self.report_template.get_untracked().trim().to_string()).filter(|f| !f.is_empty())
    }

    /// 動き出した、または終わった: 止まっている間だけの表示を消す。
    pub fn clear_stop(&self) {
        self.frames.set(Vec::new());
        self.frame.set(None);
        self.current.set(None);
        self.variables.set(Vec::new());
        self.hover.set(None);
        self.heap.set(None);
        self.retention.set(None);
        self.watches.update(|list| list.iter_mut().for_each(|w| w.result = None));
    }

    /// 利用者に見せる失敗（上の帯と、出力）。
    pub fn fail(&self, message: impl Into<String>) {
        let message = message.into();
        self.session.update(|s| s.push_output("important", &format!("{message}\n")));
        self.error.set(Some(message));
    }
}

pub fn store() -> Store {
    use_context::<Store>().expect("Store")
}
