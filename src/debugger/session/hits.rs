//! 行ブレークポイントに当たったときの判定（段階 11）: 条件、ヒットカウント（`hitCondition`）、印（条件の欄の `#once`、
//! `#after`、`#changed`、`#caller`）、ログポイント（`logMessage`。`#timer-start:` / `#timer-end:` / `#track:` も）。
//! 拡張の中継（DapProxy）が行っていたものを、同じ意味と同じ出力でエンジンが行う。判定の順は中継と同じで、条件、
//! ヒットカウント、印、ログ。当たったブレークポイントのどれかが止まるなら止まり、どれも止まらなければ、何も無かった
//! ことにして続ける（進行中のステップもそのまま）。

use super::*;
use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// `#caller` が見る呼び出し元の深さ（UI のイベントの連なりには足り、深い再帰では止める）。
const CALLER_SEARCH_DEPTH: usize = 100;
pub(super) const TRACK_PREFIX: &str = "#track:";
pub(super) const TIMER_START_PREFIX: &str = "#timer-start:";
pub(super) const TIMER_END_PREFIX: &str = "#timer-end:";

/// ヒットカウントの条件（Visual Studio と同じ）: `N` か `==N`（N 回目）、`>=N`、`>N`（N 回目から）、`%N`（N 回ごと）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HitCondition {
    Equal(u64),
    AtLeast(u64),
    MoreThan(u64),
    Every(u64),
}

impl HitCondition {
    pub(super) fn parse(text: &str) -> Option<HitCondition> {
        let text = text.trim();
        let (op, number) = ["==", ">=", ">", "%"].iter().find_map(|op| text.strip_prefix(op).map(|rest| (*op, rest))).unwrap_or(("", text));
        let n: u64 = number.trim().parse().ok()?;
        Some(match op {
            ">=" => HitCondition::AtLeast(n),
            ">" => HitCondition::MoreThan(n),
            "%" if n > 0 => HitCondition::Every(n),
            "%" => return None,
            _ => HitCondition::Equal(n),
        })
    }

    fn holds(self, hits: u64) -> bool {
        match self {
            HitCondition::Equal(n) => hits == n,
            HitCondition::AtLeast(n) => hits >= n,
            HitCondition::MoreThan(n) => hits > n,
            HitCondition::Every(n) => hits.is_multiple_of(n),
        }
    }
}

/// 条件の欄に書く印（Visual Studio 2022 のブレークポイントの設定と、`#caller`）。C# の式としては評価しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Marker {
    /// 一時的: 最初に止まったら外す（外すのは、ブレークポイントの一覧を持つ拡張。エンジンはそれまで止まらない）。
    Once,
    /// 依存: 別のブレークポイント（`File.cs:12`）で止まったあとだけ効く。
    After(String),
    /// 式の値が、前に通ったときから変わったときだけ止まる（初めて通ったときは止まらない）。
    Changed(String),
    /// これらのメソッドが呼び出し元にあるときだけ（`!` なら、無いときだけ）止まる。
    Caller { methods: Vec<String>, negate: bool },
}

impl Marker {
    pub(super) fn parse(condition: &str) -> Option<Marker> {
        let text = condition.trim();
        if text == "#once" {
            return Some(Marker::Once);
        }
        if let Some(target) = text.strip_prefix("#after").filter(|r| r.starts_with(char::is_whitespace)).map(str::trim) {
            // `ファイル:行`
            let (_, line) = target.rsplit_once(':')?;
            return line.parse::<u32>().is_ok().then(|| Marker::After(target.to_string()));
        }
        if let Some(rest) = text.strip_prefix("#caller").filter(|r| r.starts_with(char::is_whitespace)) {
            let rest = rest.trim_start();
            let (negate, list) = match rest.strip_prefix('!') {
                Some(list) => (true, list),
                None => (false, rest),
            };
            let methods: Vec<String> = list.split(|c: char| c == ',' || c.is_whitespace()).filter(|m| !m.is_empty()).map(str::to_string).collect();
            return (!methods.is_empty()).then_some(Marker::Caller { methods, negate });
        }
        text.strip_prefix("#changed").filter(|r| r.starts_with(char::is_whitespace)).map(str::trim).filter(|e| !e.is_empty()).map(|e| Marker::Changed(e.to_string()))
    }
}

/// フレームの名前（`MyLib.Orders.Save()`）が、`method` の名前のメソッドか。点で区切った末尾の部分で、大文字と小文字を
/// 区別せずに比べる（`Save`、`Orders.Save`、`MyLib.Orders.Save` はどれも合い、`SaveAll` は合わない）。拡張と同じ。
pub(super) fn frame_is_method(frame: &str, method: &str) -> bool {
    let name = frame.split('(').next().unwrap_or(frame).to_lowercase();
    let wanted = method.split('(').next().unwrap_or(method).to_lowercase();
    !wanted.is_empty() && (name == wanted || name.ends_with(&format!(".{wanted}")))
}

/// ログの文の部品: 文字のままの部分と、`{式}`。
#[derive(Debug, PartialEq)]
enum Part<'a> {
    Text(&'a str),
    Expression(&'a str),
}

/// `i={i} total={total}` を、文字と式に分ける。閉じない `{` は文字のまま。
fn template_parts(template: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open + 1..].find(['{', '}']).map(|i| open + 1 + i) else { break };
        if rest.as_bytes()[close] == b'{' || close == open + 1 {
            // `{{` や `{}` は式ではない: 開きの文字までを文字として進める。
            parts.push(Part::Text(&rest[..=open]));
            rest = &rest[open + 1..];
            continue;
        }
        if open > 0 {
            parts.push(Part::Text(&rest[..open]));
        }
        parts.push(Part::Expression(rest[open + 1..close].trim()));
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest));
    }
    parts
}

/// ブレークポイントの場所の鍵（クライアントが送ったソースのパスを小文字にしたものと、行）。
type Location = (String, u32);

/// 当たったときの判定に使う、セッションの間の覚え。`setBreakpoints` で置き直しても消さない（拡張と同じ）。
#[derive(Default)]
pub(super) struct HitState {
    hits: HashMap<Location, u64>,
    fired_once: HashSet<Location>,
    changed_values: HashMap<Location, String>,
    /// 止まったブレークポイントの場所（`#after` が見る）。
    reached: HashSet<Location>,
    /// `#timer-start:` を通った時刻（名前とスレッドごと）。
    timers: HashMap<(String, u32), Instant>,
}

impl HitState {
    /// 新しい対象: 数と覚えを捨てる。
    pub(super) fn new_target(&mut self) {
        *self = HitState::default();
    }
}

fn location(source: &str, line: u32) -> Location {
    (source.to_lowercase().replace('/', "\\"), line)
}

impl Session {
    /// 当たったブレークポイント（番号）のどれかが止まるか。止まるなら `stopped` と、止めた番号。止まらないなら、送る出来事
    /// （ログポイントの出力など）だけを残して続ける。呼ぶ前に、対象は止めた形にしてある（式を評価するため）。
    pub(super) fn judge_hits(&mut self, thread: u32, ids: &[i64], out: &mut Vec<Event>) -> Vec<i64> {
        let mut stopping = Vec::new();
        for &id in ids {
            if self.breakpoint_stops(thread, id, out) {
                stopping.push(id);
            }
        }
        // 止まるブレークポイントの場所を覚える（`#after` のため）。
        for &id in &stopping {
            if let Some((source, breakpoint)) = self.line_breakpoint(id) {
                let line = breakpoint.verified_line.unwrap_or(breakpoint.line);
                self.hit_state.reached.insert(location(&source, line));
            }
        }
        stopping
    }

    /// 番号の行ブレークポイントと、そのソース（送られてきたまま）。
    fn line_breakpoint(&self, id: i64) -> Option<(String, &Breakpoint)> {
        self.breakpoints.iter().find_map(|((_, source), list)| list.iter().find(|b| b.id == id).map(|b| (source.clone(), b)))
    }

    /// 1 つのブレークポイントで止まるか。条件とログの式は利用者が書いたものなので、利用者の評価として行う（観察専用では、
    /// その中の関数の評価を断る。D-089）。
    fn breakpoint_stops(&mut self, thread: u32, id: i64, out: &mut Vec<Event>) -> bool {
        self.as_user_evaluation(|session| session.judge_breakpoint(thread, id, out))
    }

    /// 1 つのブレークポイントで止まるか（条件、ヒットカウント、印、ログの順に見る）。
    fn judge_breakpoint(&mut self, thread: u32, id: i64, out: &mut Vec<Event>) -> bool {
        // 関数ブレークポイントは、条件だけ。
        let Some((source, breakpoint)) = self.line_breakpoint(id) else {
            let condition = self.function_breakpoints.iter().find(|b| b.id == id).and_then(|b| b.condition.clone());
            return condition.is_none_or(|c| self.condition_or_stop(thread, &c, out));
        };
        let line = breakpoint.verified_line.unwrap_or(breakpoint.line);
        let (condition, hit_condition, marker, log_message) = (breakpoint.condition.clone(), breakpoint.hit_condition, breakpoint.marker.clone(), breakpoint.log_message.clone());
        let key = location(&source, line);
        if let Some(condition) = &condition {
            if !self.condition_or_stop(thread, condition, out) {
                return false;
            }
        }
        if let Some(hit_condition) = hit_condition {
            let count = self.hit_state.hits.entry(key.clone()).or_insert(0);
            *count += 1;
            if !hit_condition.holds(*count) {
                return false;
            }
        }
        let place = self.top_frame_place(thread);
        if let Some(marker) = &marker {
            if !self.marker_allows_stop(thread, marker, &key, &place, out) {
                return false;
            }
            if *marker == Marker::Once {
                if !self.hit_state.fired_once.insert(key.clone()) {
                    // 拡張が外す前に、もう一度当たった。
                    return false;
                }
                // ブレークポイントの一覧は拡張（VS Code）が持っているので、外すのは拡張に頼む。
                out.push(("netdbg.removeBreakpoint", json!({ "source": place.source, "line": place.line })));
            }
        }
        let Some(message) = log_message else { return true };
        if let Some(name) = message.strip_prefix(TIMER_START_PREFIX) {
            self.hit_state.timers.insert((timer_name(name), thread), Instant::now());
        } else if let Some(name) = message.strip_prefix(TIMER_END_PREFIX) {
            let name = timer_name(name);
            // 始まりを通らずに終わりに来た（始まりが別の分かれ道にある）ときは、何も出さない。
            if let Some(started) = self.hit_state.timers.remove(&(name.clone(), thread)) {
                let ms = started.elapsed().as_millis();
                // デバッガーが止めて続ける時間も含むので「≤」。
                out.push(("netdbg.trackedValues", json!({ "source": place.source, "line": place.line, "time": now_ms(), "values": [{ "expression": format!("⏱ {name} (ms)"), "value": ms.to_string(), "type": "int" }] })));
                out.push(place.output(format!("⏱ {name}: ≤ {ms} ms\n")));
            }
        } else if let Some(list) = message.strip_prefix(TRACK_PREFIX) {
            let mut values = Vec::new();
            for expression in list.split(';').map(str::trim).filter(|e| !e.is_empty()) {
                values.push(match self.evaluate_shown(thread, expression) {
                    Ok((value, type_name)) => json!({ "expression": expression, "value": value, "type": type_name }),
                    Err(e) => json!({ "expression": expression, "value": "", "error": e }),
                });
            }
            out.push(("netdbg.trackedValues", json!({ "source": place.source, "line": place.line, "time": now_ms(), "values": values })));
        } else {
            let text = self.format_log_message(thread, &message, &place.function);
            out.push(place.output(format!("{text}\n")));
        }
        // ログポイントは止まらない。
        false
    }

    /// 条件（C# の式）が成り立つか。評価できなければ、理由を出して止まる（黙って通り過ぎると、止まらない理由が分からない）。
    fn condition_or_stop(&mut self, thread: u32, condition: &str, out: &mut Vec<Event>) -> bool {
        match self.condition_holds(thread, condition) {
            Ok(holds) => holds,
            Err(reason) => {
                out.push(("output", json!({ "category": "console", "output": tr!("The breakpoint condition '{condition}' cannot be evaluated, so it stops: {reason}\n", condition = condition, reason = reason) })));
                true
            }
        }
    }

    /// `#after`: 指定のブレークポイントで止まったあと。`#changed`: 前に通ったときから値が変わった（初めては止まらない）。
    /// `#caller`: メソッドが呼び出し元にある（止まったメソッド自身は数えない）。`#once` は、ここでは止める。
    fn marker_allows_stop(&mut self, thread: u32, marker: &Marker, key: &Location, place: &Place, out: &mut Vec<Event>) -> bool {
        match marker {
            Marker::Once => true,
            Marker::After(target) => {
                let Some((file, line)) = target.rsplit_once(':') else { return false };
                let Ok(line) = line.parse::<u32>() else { return false };
                let file = file.to_lowercase().replace('/', "\\");
                self.hit_state.reached.iter().any(|(path, reached)| *reached == line && (*path == file || path.ends_with(&format!("\\{file}"))))
            }
            Marker::Changed(expression) => {
                let value = match self.evaluate_shown(thread, expression) {
                    Ok((value, _)) => value,
                    Err(e) => format!("<error: {e}>"),
                };
                let previous = self.hit_state.changed_values.insert(key.clone(), value.clone());
                match previous {
                    Some(previous) if previous != value => {
                        out.push(place.output(format!("{expression}: {previous} → {value}\n")));
                        true
                    }
                    _ => false,
                }
            }
            Marker::Caller { methods, negate } => {
                let frames = self.walk_frames(thread).unwrap_or_default();
                let called = frames.iter().skip(1).take(CALLER_SEARCH_DEPTH).any(|frame| {
                    let name = unsafe { self.describe_frame(0, frame) }["name"].as_str().unwrap_or("").to_string();
                    methods.iter().any(|m| frame_is_method(&name, m))
                });
                called != *negate
            }
        }
    }

    /// 止まったフレームの名前、ソース、行（出力に付ける）。
    fn top_frame_place(&mut self, thread: u32) -> Place {
        let described = self.walk_frames(thread).ok().and_then(|frames| frames.first().map(|frame| unsafe { self.describe_frame(0, frame) }));
        let described = described.unwrap_or(Value::Null);
        Place { function: described["name"].as_str().unwrap_or("").to_string(), source: described.get("source").cloned().unwrap_or(Value::Null), line: described["line"].clone() }
    }

    /// `evaluate` と同じ書き方の値と型（文字列は C# のリテラル、オブジェクトは DebuggerDisplay など）。
    pub(super) fn evaluate_shown(&mut self, thread: u32, expression: &str) -> Result<(String, String), String> {
        let frame_id = self.frame_id(thread, 0);
        let body = self.evaluate(&json!({ "expression": expression, "frameId": frame_id, "context": "watch" }))?;
        Ok((body["result"].as_str().unwrap_or("").to_string(), body["type"].as_str().unwrap_or("").to_string()))
    }

    /// ログの文の `{式}` を止めたフレームで評価し、`$FUNCTION` と `$TID` を置き換える（Visual Studio のトレースポイントと同じ）。
    fn format_log_message(&mut self, thread: u32, template: &str, function: &str) -> String {
        let mut text = String::new();
        for part in template_parts(template) {
            match part {
                Part::Expression(expression) => match self.evaluate_shown(thread, expression) {
                    Ok((value, _)) => text.push_str(&value),
                    Err(e) => text.push_str(&format!("<error: {e}>")),
                },
                Part::Text(plain) => text.push_str(&plain.replace("$FUNCTION", function).replace("$TID", &thread.to_string())),
            }
        }
        text
    }
}

/// 止まった場所（出力の `source` と `line`）。
struct Place {
    function: String,
    source: Value,
    line: Value,
}

impl Place {
    fn output(&self, text: String) -> Event {
        let mut body = json!({ "category": "console", "output": text });
        if !self.source.is_null() {
            body["source"] = self.source.clone();
            body["line"] = self.line.clone();
        }
        ("output", body)
    }
}

fn timer_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() { "timer".to_string() } else { name.to_string() }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_conditions_are_read_as_in_visual_studio() {
        assert_eq!(HitCondition::parse("3"), Some(HitCondition::Equal(3)));
        assert_eq!(HitCondition::parse(" == 3 "), Some(HitCondition::Equal(3)));
        assert_eq!(HitCondition::parse(">=2"), Some(HitCondition::AtLeast(2)));
        assert_eq!(HitCondition::parse(">2"), Some(HitCondition::MoreThan(2)));
        assert_eq!(HitCondition::parse("%2"), Some(HitCondition::Every(2)));
        assert_eq!(HitCondition::parse("%0"), None);
        assert_eq!(HitCondition::parse("x"), None);
        assert!(HitCondition::Every(2).holds(4) && !HitCondition::Every(2).holds(3));
        assert!(HitCondition::MoreThan(2).holds(3) && !HitCondition::MoreThan(2).holds(2));
    }

    #[test]
    fn markers_are_read_from_the_condition() {
        assert_eq!(Marker::parse("#once"), Some(Marker::Once));
        assert_eq!(Marker::parse("#after Program.cs:12"), Some(Marker::After("Program.cs:12".into())));
        assert_eq!(Marker::parse("#after Program.cs"), None);
        assert_eq!(Marker::parse("#changed total"), Some(Marker::Changed("total".into())));
        assert_eq!(Marker::parse("#caller A, B"), Some(Marker::Caller { methods: vec!["A".into(), "B".into()], negate: false }));
        assert_eq!(Marker::parse("#caller !A"), Some(Marker::Caller { methods: vec!["A".into()], negate: true }));
        assert_eq!(Marker::parse("i > 3"), None);
        assert_eq!(Marker::parse("#onceMore"), None);
    }

    #[test]
    fn frame_names_match_methods_on_whole_parts_from_the_end() {
        assert!(frame_is_method("MyLib.Orders.Save()", "Save"));
        assert!(frame_is_method("MyLib.Orders.Save()", "orders.save"));
        assert!(frame_is_method("MyLib.Orders.Save()", "MyLib.Orders.Save"));
        assert!(!frame_is_method("MyLib.Orders.SaveAll()", "Save"));
    }

    #[test]
    fn log_messages_are_split_into_text_and_expressions() {
        assert_eq!(template_parts("i={i} total={ total }"), vec![Part::Text("i="), Part::Expression("i"), Part::Text(" total="), Part::Expression("total")]);
        assert_eq!(template_parts("no braces"), vec![Part::Text("no braces")]);
        assert_eq!(template_parts("open { only"), vec![Part::Text("open { only")]);
        assert_eq!(template_parts("{}x"), vec![Part::Text("{"), Part::Text("}x")]);
    }
}
