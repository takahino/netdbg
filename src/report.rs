//! 成績書（B3、D-081）: テスト項目ごとに、exe の版と更新日時、操作、画面、変わったコントロール、SQL、メッセージ、
//! 例外、所要時間を束ね、Excel（1 項目が 1 行）と、1 ファイルで完結する HTML に書く。
//!
//! ここは Win32 も ICorDebug も知らない（材料はハブが集めて渡す。`interface::hub`）。
//! - `html.rs`: HTML（画像は data URI、CSS は style 要素。script と外部の参照は書かない）。
//! - `xlsx.rs`: Excel の .xlsx（ZIP と SpreadsheetML の最小の部品を自前で書く）。
//! - `operations.rs`: 人の手の入力（クリックとキー）を、操作の言葉にする。
//! - `tree.rs`: 項目の始めと終わりの画面の木の差分（変わったコントロール）。
//! - `paint.rs`: 画面の写し（画素）に、操作した箇所を塗る。

pub mod data;
pub mod html;
pub mod operations;
pub mod paint;
pub mod steps;
pub mod template;
pub mod tree;
pub mod xlsx;
pub mod xml;
pub mod zip;

use std::path::PathBuf;

/// 試験した exe。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exe {
    pub path: String,
    /// ファイルの版（版の情報が無ければ `None`）。
    pub version: Option<String>,
    /// 更新日時（地方時の `2026-11-21 09:41:44`）。
    pub modified: Option<String>,
}

/// 操作をした者。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// AI（MCP の ui_* のツール）。
    Ai,
    /// 人の手（マウスとキー）。
    Hand,
    /// 試験者が書いた手順。
    Note,
}

impl Actor {
    /// 言語によらない種類（画面の色分けに使う）。
    pub fn kind(self) -> &'static str {
        match self {
            Actor::Ai => "ai",
            Actor::Hand => "hand",
            Actor::Note => "note",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Actor::Ai => tr_str!("AI"),
            Actor::Hand => tr_str!("Hand"),
            Actor::Note => tr_str!("Step"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// 時刻（`09:41:44`）。
    pub time: String,
    pub actor: Actor,
    pub text: String,
    /// 操作の前の画面に、操作した箇所を塗ったもの（AI と手の操作。箇所が分からなければ `None`）。
    pub marked: Option<Picture>,
    /// 操作のあとの画面（AI と手の操作。対象の窓をまとめて 1 枚。写せなかったら `None` で、理由は項目の
    /// `picture_problems`）。
    pub picture: Option<Picture>,
    /// もう一度流すときの手順（D-086。流せない操作と手順の行は None）。
    pub step: Option<serde_json::Value>,
}

impl Operation {
    pub fn new(time: String, actor: Actor, text: String) -> Operation {
        Operation { time, actor, text, marked: None, picture: None, step: None }
    }
}

/// 操作した箇所（画面の座標。物理ピクセル。右と下は含まない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spot {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Spot {
    /// MCP の答えの `screen`（`{x, y, width, height}`、点なら `{x, y}`）から。点は、その回りの小さな四角にする。
    pub fn from_json(value: &serde_json::Value) -> Option<Spot> {
        let number = |key: &str| value.get(key).and_then(serde_json::Value::as_i64).map(|n| n as i32);
        let (x, y) = (number("x")?, number("y")?);
        match (number("width"), number("height")) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some(Spot { left: x, top: y, right: x + width, bottom: y + height }),
            _ => Some(Spot { left: x - POINT_SPOT, top: y - POINT_SPOT, right: x + POINT_SPOT, bottom: y + POINT_SPOT }),
        }
    }
}

/// 点（マウスで押した所）を塗るときの、四角の半分の大きさ（画素）。
const POINT_SPOT: i32 = 12;

/// 画面の写し 1 枚。
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// 窓の見出しと、いつ撮ったか（`操作 3 のあと`、`写す`、`終わり`）。
    pub caption: String,
    pub png: Vec<u8>,
}

/// テスト項目 1 つ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    /// `OK`、`NG`（終えるまでは `None`）。
    pub result: Option<String>,
    pub note: String,
    /// 始めた時刻（地方時の文字）と、UNIX 時刻のミリ秒。
    pub started: String,
    pub started_ms: u64,
    pub ended: Option<String>,
    pub duration_ms: Option<u64>,
    pub exe: Option<Exe>,
    pub operations: Vec<Operation>,
    pub pictures: Vec<Picture>,
    /// 撮れなかった写しの理由（画面がロックされている、など）。
    pub picture_problems: Vec<String>,
    pub changes: Vec<tree::Change>,
    /// 変わったコントロールを読めなかった理由（止まっていた、など）。
    pub change_problem: Option<String>,
    /// 項目の間に実行した SQL（値を埋めたもの）、出したメッセージ（ダイアログ）、投げられた例外。
    pub sql: Vec<String>,
    pub messages: Vec<String>,
    pub exceptions: Vec<String>,
}

impl Item {
    /// 所要時間の文字（`12.3 秒`）。
    pub fn duration_text(&self) -> String {
        self.duration_ms.map_or_else(String::new, |ms| tr!("{seconds} s", seconds = format!("{:.1}", ms as f64 / 1000.0)))
    }

    /// 画面の写しの枚数（操作ごとの分と、写す・終わりの分）。
    pub fn picture_count(&self) -> usize {
        self.pictures.len() + self.operations.iter().map(|o| usize::from(o.marked.is_some()) + usize::from(o.picture.is_some())).sum::<usize>()
    }
}

/// 成績書 1 冊（エンジンが動いている間の項目すべて）。
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub items: Vec<Item>,
    /// 書き出すフォルダー（最初に書くときに決める）。
    pub folder: Option<PathBuf>,
}

impl Report {
    /// 開いている（まだ終えていない）項目。
    pub fn open_item(&mut self) -> Option<&mut Item> {
        self.items.last_mut().filter(|item| item.result.is_none())
    }

    /// OK と NG の数。
    pub fn counts(&self) -> (usize, usize) {
        let count = |result: &str| self.items.iter().filter(|i| i.result.as_deref() == Some(result)).count();
        (count("OK"), count("NG"))
    }
}

/// HTML と表の文字に書けない文字を逃がす。
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // XML に書けない制御文字は落とす（タブと改行は残す）。
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}
