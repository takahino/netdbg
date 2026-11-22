//! SQL ログ: ADO.NET のコマンド（`OracleCommand`、`SqlCommand` など）が実行しようとしている文を、止めた対象から読み、
//! パラメーターの値を埋めた SQL にする。拡張の `sqlLogCore.ts` と同じ結果にする（段階 11）。
//!
//! エンジンは、プロバイダーの `Execute*` の入口に止めずに読むブレークポイント（`session/probes.rs`）を置き、ここで
//! コマンドを読んで続行する。呼び出し元ではなくプロバイダーを捕まえるので、自作のデータアクセスの部品が間にあっても、
//! どの文もここを通る。ここは対象にも COM にも触れない（読むのは `Reader` を通す）ので、単体で試験できる。

use serde_json::{json, Value};

/// 既定で捕まえるメソッド（`Namespace.Type.Method`。同じ名前の多重定義はすべて）。
pub const DEFAULT_METHODS: &[&str] = &[
    "Oracle.ManagedDataAccess.Client.OracleCommand.ExecuteReader",
    "Oracle.ManagedDataAccess.Client.OracleCommand.ExecuteNonQuery",
    "Oracle.ManagedDataAccess.Client.OracleCommand.ExecuteScalar",
    "Oracle.DataAccess.Client.OracleCommand.ExecuteReader",
    "Oracle.DataAccess.Client.OracleCommand.ExecuteNonQuery",
    "Oracle.DataAccess.Client.OracleCommand.ExecuteScalar",
    "System.Data.SqlClient.SqlCommand.ExecuteReader",
    "System.Data.SqlClient.SqlCommand.ExecuteNonQuery",
    "System.Data.SqlClient.SqlCommand.ExecuteScalar",
    "Microsoft.Data.SqlClient.SqlCommand.ExecuteReader",
    "Microsoft.Data.SqlClient.SqlCommand.ExecuteNonQuery",
    "Microsoft.Data.SqlClient.SqlCommand.ExecuteScalar",
    "System.Data.OleDb.OleDbCommand.ExecuteReader",
    "System.Data.OleDb.OleDbCommand.ExecuteNonQuery",
    "System.Data.OleDb.OleDbCommand.ExecuteScalar",
    "System.Data.Odbc.OdbcCommand.ExecuteReader",
    "System.Data.Odbc.OdbcCommand.ExecuteNonQuery",
    "System.Data.Odbc.OdbcCommand.ExecuteScalar",
];

/// これより多いパラメーターは読まない（1 つにつき数回の評価がかかり、その間対象は止まっている）。
pub const MAX_PARAMETERS: usize = 100;
/// バイナリのパラメーターで読むバイト数。
const MAX_BINARY_BYTES: usize = 32;
/// 同じ文（空白の違いを除く）がこの回数を超えたら、繰り返しの印を付ける（N+1 の疑い）。拡張と同じ。
pub const REPEAT_WARNING: usize = 5;

/// 評価した値の見せ方（DAP の `evaluate` の `result` と `type` と同じ書き方）。
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub value: String,
    pub type_name: String,
}

/// 止めたフレーム（コマンドのメソッドの中。`this` がコマンド）で式を評価する口。評価できなければ `None`。
pub trait Reader {
    fn evaluate(&mut self, expression: &str) -> Option<Shown>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Null,
    String,
    Number,
    DateTime,
    Binary,
    Other,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Null => "null",
            Kind::String => "string",
            Kind::Number => "number",
            Kind::DateTime => "datetime",
            Kind::Binary => "binary",
            Kind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// 頭の印（`:`、`@`）を除いた名前。
    pub name: String,
    pub kind: Kind,
    /// string: 文字列、number / other: `ToString()`、datetime: `yyyy-MM-dd HH:mm:ss.fff`、binary: 十六進、null: 空。
    pub value: String,
    /// `Input`、`Output`、`InputOutput`、`ReturnValue`。
    pub direction: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Command {
    pub command_text: String,
    /// `Text`、`StoredProcedure`、`TableDirect`。
    pub command_type: String,
    pub parameters: Vec<Parameter>,
    /// Oracle だけ: false（既定）なら、名前に関係なく位置で結び付ける。
    pub bind_by_name: Option<bool>,
    pub data_source: Option<String>,
    /// 読んだより多くのパラメーターがある。
    pub truncated: bool,
}

impl Command {
    /// DAP のイベント（拡張の `netdbg.sqlExecuted`）と同じ形の中身。
    pub fn to_json(&self) -> Value {
        let mut body = json!({
            "commandText": self.command_text,
            "commandType": self.command_type,
            "parameters": self.parameters.iter().map(|p| json!({ "name": p.name, "kind": p.kind.as_str(), "value": p.value, "direction": p.direction })).collect::<Vec<_>>(),
        });
        if let Some(by_name) = self.bind_by_name {
            body["bindByName"] = json!(by_name);
        }
        if let Some(source) = &self.data_source {
            body["dataSource"] = json!(source);
        }
        if self.truncated {
            body["truncated"] = json!(true);
        }
        body
    }
}

/// デバッガーが C# のリテラルで見せる文字列の中身（`"a\"b"` → `a"b`）。リテラルでなければ、そのまま。
pub fn unquote(text: &str) -> String {
    let Some(inner) = text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).filter(|_| text.len() >= 2) else { return text.to_string() };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = chars.next() else { break };
        match e {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            'u' | 'x' => {
                // `\u` はちょうど 4 桁、`\x` は 1〜4 桁の十六進。
                let required = if e == 'u' { 4 } else { 1 };
                let mut digits = String::new();
                while digits.len() < 4 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    digits.extend(chars.next());
                }
                match u32::from_str_radix(&digits, 16).ok().and_then(char::from_u32) {
                    Some(decoded) if digits.len() >= required => out.push(decoded),
                    _ => {
                        out.push(e);
                        out.push_str(&digits);
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// 止めたフレームの `this`（コマンド）を読む。`this` がコマンドでなければ `None`。
///
/// 先に、プロパティの裏のフィールド（`System.Data` と `Microsoft.Data.SqlClient` の `_commandText`、`_parameters` など）
/// を読む。フィールドは対象のコードを動かさずに読めるので、入口がネイティブにコンパイル済み（NGEN）で関数の評価が
/// できない所に止まっても読める（CI の x86 の .NET Framework で、プロパティでは読めなかった）。フィールドが無い
/// プロバイダー（Oracle など）は、プロパティで読む。
pub fn capture(reader: &mut dyn Reader, type_name: &str) -> Option<Command> {
    let command_text = first_text(reader, &["this._commandText", "this.CommandText"])?;
    let mut command = Command {
        command_text: if command_text == "null" { String::new() } else { command_text },
        command_type: enum_name(reader, &["this._commandType", "this.CommandType.ToString()"]).unwrap_or_else(|| "Text".into()),
        ..Default::default()
    };
    if type_name.rsplit('.').next().is_some_and(|t| t.starts_with("Oracle")) {
        command.bind_by_name = Some(reader.evaluate("this.BindByName").is_some_and(|s| s.value == "true"));
    }
    command.data_source = text(reader, "this.Connection.DataSource");
    // パラメーターの集まりは、中の List<T> の配列と数をフィールドで読む。無ければ（または集まりがまだ作られて
    // いなければ）プロパティで。
    let (count, base) = match reader.evaluate("this._parameters._items._size").and_then(|s| s.value.parse::<usize>().ok()) {
        Some(count) => (count, "this._parameters._items._items"),
        None => (reader.evaluate("this.Parameters.Count").and_then(|s| s.value.parse::<usize>().ok()).unwrap_or(0), "this.Parameters"),
    };
    command.truncated = count > MAX_PARAMETERS;
    for index in 0..count.min(MAX_PARAMETERS) {
        command.parameters.push(read_parameter(reader, &format!("{base}[{index}]"), index));
    }
    Some(command)
}

/// `p` のパラメーター（`index` は、名前が読めないときの番号）。
fn read_parameter(reader: &mut dyn Reader, p: &str, index: usize) -> Parameter {
    let name = first_text(reader, &[&format!("{p}._parameterName"), &format!("{p}.ParameterName")]).unwrap_or_else(|| format!("#{}", index + 1));
    let name = name.strip_prefix([':', '@', '?']).unwrap_or(&name).to_string();
    // 向きのフィールドは、既定の Input を 0 のまま持つ（プロパティが 0 を Input にして返す）。
    let direction = match reader.evaluate(&format!("{p}._direction")) {
        Some(shown) if shown.value == "0" => "Input".to_string(),
        _ => enum_name(reader, &[&format!("{p}._direction"), &format!("{p}.Direction.ToString()")]).unwrap_or_else(|| "Input".into()),
    };
    let parameter = |kind: Kind, value: String| Parameter { name: name.clone(), kind, value, direction: direction.clone() };
    let Some((v, value)) = first(reader, &[&format!("{p}._value"), &format!("{p}.Value")]) else { return parameter(Kind::Other, "?".into()) };
    match value.type_name.as_str() {
        _ if value.value == "null" || value.type_name == "System.DBNull" => parameter(Kind::Null, String::new()),
        "string" => parameter(Kind::String, unquote(&value.value)),
        "System.DateTime" => {
            let formatted = text(reader, &format!("{v}.ToString(\"yyyy-MM-dd HH:mm:ss.fff\")")).or_else(|| date_time_from_fields(reader, &v));
            parameter(Kind::DateTime, formatted.unwrap_or(value.value))
        }
        "byte[]" => parameter(Kind::Binary, read_bytes(reader, &v)),
        type_name => {
            let numeric = matches!(type_name, "int" | "long" | "short" | "byte" | "sbyte" | "uint" | "ulong" | "ushort" | "double" | "float" | "decimal");
            let shown = text(reader, &format!("{v}.ToString()")).unwrap_or(value.value);
            parameter(if numeric { Kind::Number } else { Kind::Other }, shown)
        }
    }
}

/// DateTime を、中のフィールド（刻みと種類を詰めた `dateData`。.NET では `_dateData`）から `yyyy-MM-dd HH:mm:ss.fff` に
/// する。ToString が使えない（関数の評価ができない）所に止まったとき。
fn date_time_from_fields(reader: &mut dyn Reader, v: &str) -> Option<String> {
    let data = first(reader, &[&format!("{v}.dateData"), &format!("{v}._dateData")])?.1.value.parse::<u64>().ok()?;
    Some(format_ticks(data & 0x3FFF_FFFF_FFFF_FFFF))
}

/// 0001-01-01 からの 100 ナノ秒の刻みを `yyyy-MM-dd HH:mm:ss.fff` に。
fn format_ticks(ticks: u64) -> String {
    let millis = ticks / 10_000;
    let (days, rest) = ((millis / 86_400_000) as i64, millis % 86_400_000);
    // 0001-01-01 から 1970-01-01 までの日数で、Unix の日数にしてから暦にする（Howard Hinnant の方法）。
    let z = days - 719_162 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// `byte[]` の値の十六進（先頭の `MAX_BINARY_BYTES` バイトまで。それより長ければ `…` を付ける）。
fn read_bytes(reader: &mut dyn Reader, v: &str) -> String {
    let array = format!("((byte[]){v})");
    let Some(length) = reader.evaluate(&format!("{array}.Length")).and_then(|s| s.value.parse::<usize>().ok()) else { return String::new() };
    let read = length.min(MAX_BINARY_BYTES);
    if read == 0 {
        return String::new();
    }
    // 1 回の評価で読む（要素ごとに評価すると、その数だけ止まる時間が延びる）。`01-AB-…` の形で返る。関数の評価が
    // できない所では、要素を 1 つずつ読む（要素の読み取りは、対象のコードを動かさない）。
    let hex = match reader.evaluate(&format!("System.BitConverter.ToString({array}, 0, {read})")) {
        Some(shown) => unquote(&shown.value).chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase(),
        None => {
            let mut hex = String::new();
            for index in 0..read {
                let Some(byte) = reader.evaluate(&format!("{array}[{index}]")).and_then(|s| s.value.parse::<u8>().ok()) else { return String::new() };
                hex.push_str(&format!("{byte:02x}"));
            }
            hex
        }
    };
    if length > read {
        format!("{hex}…")
    } else {
        hex
    }
}

/// 式を評価して、文字列ならその中身（リテラルの引用符を外したもの）。
fn text(reader: &mut dyn Reader, expression: &str) -> Option<String> {
    reader.evaluate(expression).map(|s| unquote(&s.value))
}

/// 式を順に試して、最初に読めた式と値。
fn first(reader: &mut dyn Reader, expressions: &[&str]) -> Option<(String, Shown)> {
    expressions.iter().find_map(|expression| reader.evaluate(expression).map(|shown| (expression.to_string(), shown)))
}

/// 式を順に試して、最初に読めた文字列の中身。
fn first_text(reader: &mut dyn Reader, expressions: &[&str]) -> Option<String> {
    first(reader, expressions).map(|(_, shown)| unquote(&shown.value))
}

/// 列挙型の名前（フィールドの値は名前で見える。名前の無い数なら、次の式を試す）。
fn enum_name(reader: &mut dyn Reader, expressions: &[&str]) -> Option<String> {
    expressions.iter().find_map(|expression| {
        let name = text(reader, expression)?;
        (!name.starts_with(|c: char| c.is_ascii_digit() || c == '-')).then_some(name)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Oracle,
    SqlServer,
    Generic,
}

/// プロバイダーのコマンドの型から、SQL の方言（リテラルの書き方）。
pub fn dialect_of(type_name: &str) -> Dialect {
    let last = type_name.rsplit('.').next().unwrap_or(type_name);
    if (last.starts_with("Oracle") && last.ends_with("Command")) || type_name.starts_with("Oracle") {
        return Dialect::Oracle;
    }
    if type_name.ends_with(".SqlCommand") {
        Dialect::SqlServer
    } else {
        Dialect::Generic
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn is_number(text: &str) -> bool {
    // -?\d+(\.\d+)?([eE][+-]?\d+)?
    let b = text.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if !digits(&mut i) {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == b.len()
}

fn literal(p: &Parameter, dialect: Dialect) -> String {
    match p.kind {
        Kind::Null => "NULL".into(),
        Kind::Number if is_number(&p.value) => p.value.clone(),
        Kind::DateTime => match dialect {
            Dialect::Oracle => format!("TO_TIMESTAMP({}, 'YYYY-MM-DD HH24:MI:SS.FF3')", quote(&p.value)),
            Dialect::SqlServer => quote(&p.value.replacen(' ', "T", 1)),
            Dialect::Generic => quote(&p.value),
        },
        Kind::Binary => {
            let hex = p.value.replace('…', "");
            match dialect {
                Dialect::Oracle => format!("HEXTORAW('{hex}')"),
                Dialect::SqlServer => format!("0x{hex}"),
                Dialect::Generic => format!("X'{hex}'"),
            }
        }
        Kind::Other if p.value == "True" => "1".into(),
        Kind::Other if p.value == "False" => "0".into(),
        _ => quote(&p.value),
    }
}

/// 文の中のパラメーターの印（`:name`、`@name`、`?`）を値のリテラルに置き換えた文（SQL のツールにそのまま貼れる）。
/// 文字列のリテラル、引用符で囲んだ識別子、注釈の中の印はそのまま。ストアド プロシージャは、それを呼ぶ文にする。
pub fn substitute(command: &Command, dialect: Dialect) -> String {
    if command.command_type == "StoredProcedure" {
        let arguments: Vec<&Parameter> = command.parameters.iter().filter(|p| p.direction != "ReturnValue").collect();
        if dialect == Dialect::Oracle {
            let list: Vec<String> = arguments.iter().map(|p| format!("{} => {}", p.name, literal(p, dialect))).collect();
            return format!("BEGIN {}({}); END;", command.command_text, list.join(", "));
        }
        let list: Vec<String> = arguments.iter().map(|p| format!("@{} = {}", p.name, literal(p, dialect))).collect();
        return format!("EXEC {}{}{}", command.command_text, if list.is_empty() { "" } else { " " }, list.join(", "));
    }
    let sql: Vec<char> = command.command_text.chars().collect();
    // Oracle は BindByName が無ければ位置で結び付ける: n 番目の別々の印に、n 番目のパラメーター。
    let by_position = dialect == Dialect::Oracle && command.bind_by_name == Some(false);
    let by_name = |name: &str| command.parameters.iter().find(|p| p.name.to_lowercase() == name);
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '#');
    let mut seen: Vec<String> = Vec::new();
    let mut positional = 0;
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < sql.len() {
        let c = sql[i];
        let next = sql.get(i + 1).copied().unwrap_or('\0');
        let previous = if i > 0 { sql[i - 1] } else { '\0' };
        if c == '\'' || c == '"' {
            // 文字列のリテラルか、引用符で囲んだ識別子。中で二つ重ねた引用符は、引用符そのもの。
            let mut j = i + 1;
            while j < sql.len() && !(sql[j] == c && sql.get(j + 1) != Some(&c)) {
                j += if sql[j] == c { 2 } else { 1 };
            }
            let end = (j + 1).min(sql.len());
            out.extend(&sql[i..end]);
            i = end;
        } else if c == '-' && next == '-' {
            let j = (i..sql.len()).find(|&k| sql[k] == '\n').unwrap_or(sql.len());
            out.extend(&sql[i..j]);
            i = j;
        } else if c == '/' && next == '*' {
            let j = (i + 2..sql.len().saturating_sub(1)).find(|&k| sql[k] == '*' && sql[k + 1] == '/').map_or(sql.len(), |k| k + 2);
            out.extend(&sql[i..j]);
            i = j;
        } else if (c == ':' || c == '@') && (next.is_ascii_alphanumeric() || next == '_') && !word(previous) && previous != c && previous != ':' {
            let mut j = i + 1;
            while j < sql.len() && word(sql[j]) {
                j += 1;
            }
            let name: String = sql[i + 1..j].iter().collect::<String>().to_lowercase();
            let parameter = if by_position {
                if !seen.contains(&name) {
                    seen.push(name.clone());
                }
                seen.iter().position(|s| *s == name).and_then(|k| command.parameters.get(k))
            } else {
                by_name(&name)
            };
            match parameter {
                Some(p) => out.push_str(&literal(p, dialect)),
                None => out.extend(&sql[i..j]),
            }
            i = j;
        } else if c == '?' && dialect == Dialect::Generic {
            match command.parameters.get(positional) {
                Some(p) => out.push_str(&literal(p, dialect)),
                None => out.push(c),
            }
            positional += 1;
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// 空白の違いだけの文は、同じ文（繰り返しを数えるため）。
pub fn normalize(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// SQL で止める条件（段階 12）。書いた欄のすべてに合う文で止まる。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlCondition {
    /// 文に含まれる文字。大文字と小文字、空白の違いは見ない。元の文でも、値を埋めた文でもよい。
    pub text: Option<String>,
    /// パラメーターの名前（頭の `@`、`:` はあってもなくてもよい。大文字と小文字は見ない）。
    pub parameter: Option<String>,
    /// パラメーターの値（記録と同じ書き方: 文字列は引用符なし、日時は `yyyy-MM-dd HH:mm:ss.fff`、DBNull は `null`）。
    /// `parameter` が無ければ、どれかのパラメーターがこの値なら合う。
    pub value: Option<String>,
}

impl SqlCondition {
    /// `{ text?, parameter?, value? }`。どの欄も空なら `None`（何にでも合う条件は、作らない）。
    pub fn from_json(value: &Value) -> Option<SqlCondition> {
        let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let condition = SqlCondition { text: field("text"), parameter: field("parameter").map(|p| p.trim_start_matches(['@', ':', '?']).to_string()), value: field("value") };
        (condition != SqlCondition::default()).then_some(condition)
    }

    pub fn to_json(&self) -> Value {
        let mut out = serde_json::json!({});
        for (name, field) in [("text", &self.text), ("parameter", &self.parameter), ("value", &self.value)] {
            if let Some(field) = field {
                out[name] = Value::String(field.clone());
            }
        }
        out
    }

    /// 文（`command`、値を埋めた `sql`）が条件に合うか。
    pub fn matches(&self, command: &Command, sql: &str) -> bool {
        let folded = |s: &str| normalize(s).to_lowercase();
        if let Some(text) = &self.text {
            let text = folded(text);
            if !folded(&command.command_text).contains(&text) && !folded(sql).contains(&text) {
                return false;
            }
        }
        let shown = |p: &Parameter| if p.kind == Kind::Null { "null".to_string() } else { p.value.clone() };
        match (&self.parameter, &self.value) {
            (None, None) => true,
            (Some(name), value) => command.parameters.iter().any(|p| p.name.eq_ignore_ascii_case(name) && value.as_ref().is_none_or(|v| shown(p) == *v)),
            (None, Some(value)) => command.parameters.iter().any(|p| shown(p) == *value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn param(name: &str, kind: Kind, value: &str) -> Parameter {
        Parameter { name: name.into(), kind, value: value.into(), direction: "Input".into() }
    }

    #[test]
    fn a_sql_condition_matches_the_text_and_the_parameters_it_names() {
        let command = Command { command_text: "DELETE FROM  Audit\nWHERE Id = @id".into(), command_type: "Text".into(), parameters: vec![param("id", Kind::Number, "7"), param("note", Kind::Null, "")], ..Default::default() };
        let sql = "DELETE FROM Audit WHERE Id = 7";
        let condition = |text: serde_json::Value| SqlCondition::from_json(&text).unwrap();
        assert!(condition(serde_json::json!({ "text": "delete from audit" })).matches(&command, sql));
        assert!(condition(serde_json::json!({ "text": "Id = 7" })).matches(&command, sql), "the filled-in statement counts too");
        assert!(condition(serde_json::json!({ "text": "audit", "parameter": "@ID", "value": "7" })).matches(&command, sql));
        assert!(condition(serde_json::json!({ "value": "null" })).matches(&command, sql), "DBNull is written null");
        assert!(!condition(serde_json::json!({ "parameter": "id", "value": "8" })).matches(&command, sql));
        assert!(!condition(serde_json::json!({ "text": "orders" })).matches(&command, sql));
        assert!(!condition(serde_json::json!({ "parameter": "missing" })).matches(&command, sql));
        assert_eq!(SqlCondition::from_json(&serde_json::json!({ "text": " " })), None, "an empty condition would stop at every statement");
    }

    fn command(text: &str, parameters: Vec<Parameter>) -> Command {
        Command { command_text: text.into(), command_type: "Text".into(), parameters, ..Default::default() }
    }

    fn by_name(mut c: Command) -> Command {
        c.bind_by_name = Some(true);
        c
    }

    /// 決まった答えを返す読み手。聞かれた式を覚える。
    struct Fake {
        answers: HashMap<String, Shown>,
        asked: Vec<String>,
    }

    impl Fake {
        fn new(answers: &[(&str, &str, &str)]) -> Fake {
            Fake { answers: answers.iter().map(|(e, v, t)| (e.to_string(), Shown { value: v.to_string(), type_name: t.to_string() })).collect(), asked: Vec::new() }
        }
    }

    impl Reader for Fake {
        fn evaluate(&mut self, expression: &str) -> Option<Shown> {
            self.asked.push(expression.to_string());
            self.answers.get(expression).cloned()
        }
    }

    #[test]
    fn reads_the_csharp_literal_the_debugger_shows_for_a_string() {
        assert_eq!(unquote("\"O'Brien\""), "O'Brien");
        assert_eq!(unquote(r#""a\"b\\c""#), "a\"b\\c");
        assert_eq!(unquote(r#""line1\r\nline2\tx""#), "line1\r\nline2\tx");
        assert_eq!(unquote(r#""あ\x41""#), "あA");
        assert_eq!(unquote("42"), "42");
        assert_eq!(unquote("{System.DateTime}"), "{System.DateTime}");
        assert_eq!(unquote("null"), "null");
    }

    #[test]
    fn reads_the_text_the_type_the_data_source_and_each_parameter_by_its_type() {
        let mut reader = Fake::new(&[
            ("this.CommandText", r#""SELECT * FROM T WHERE A = :a""#, "string"),
            ("this.CommandType.ToString()", "\"Text\"", "string"),
            ("this.BindByName", "true", "bool"),
            ("this.Connection.DataSource", "\"ORCL\"", "string"),
            ("this.Parameters.Count", "4", "int"),
            ("this.Parameters[0].ParameterName", "\":a\"", "string"),
            ("this.Parameters[0].Direction.ToString()", "\"Input\"", "string"),
            ("this.Parameters[0].Value", "\"x\"", "string"),
            ("this.Parameters[1].ParameterName", "\"n\"", "string"),
            ("this.Parameters[1].Value", "12", "int"),
            ("this.Parameters[1].Value.ToString()", "\"12\"", "string"),
            ("this.Parameters[2].ParameterName", "\"d\"", "string"),
            ("this.Parameters[2].Value", "{4/1/2024 1:05:09 PM}", "System.DateTime"),
            ("this.Parameters[2].Value.ToString(\"yyyy-MM-dd HH:mm:ss.fff\")", "\"2024-04-01 13:05:09.000\"", "string"),
            ("this.Parameters[3].ParameterName", "\"b\"", "string"),
            ("this.Parameters[3].Value", "{byte[3]}", "byte[]"),
            ("((byte[])this.Parameters[3].Value).Length", "3", "int"),
            ("System.BitConverter.ToString(((byte[])this.Parameters[3].Value), 0, 3)", "\"00-01-AB\"", "string"),
        ]);
        let got = capture(&mut reader, "Oracle.ManagedDataAccess.Client.OracleCommand").unwrap();
        assert_eq!(got.command_text, "SELECT * FROM T WHERE A = :a");
        assert_eq!(got.bind_by_name, Some(true));
        assert_eq!(got.data_source.as_deref(), Some("ORCL"));
        assert_eq!(
            got.parameters,
            vec![param("a", Kind::String, "x"), param("n", Kind::Number, "12"), param("d", Kind::DateTime, "2024-04-01 13:05:09.000"), param("b", Kind::Binary, "0001ab")]
        );
    }

    #[test]
    fn reads_the_backing_fields_first_without_running_the_target_s_code() {
        // CI の x86 の .NET Framework: 入口がネイティブにコンパイル済みで、関数の評価（プロパティ、ToString、
        // BitConverter）はどれもできない。フィールドと配列の要素だけが読める。
        let items = "this._parameters._items._items";
        let mut reader = Fake::new(&[
            ("this._commandText", r#""SELECT * FROM Orders WHERE Id = @id""#, "string"),
            ("this._commandType", "Text", "System.Data.CommandType"),
            ("this._parameters._items._size", "3", "int"),
            (&format!("{items}[0]._parameterName"), "\"@id\"", "string"),
            (&format!("{items}[0]._direction"), "0", "System.Data.ParameterDirection"),
            (&format!("{items}[0]._value"), "42", "int"),
            (&format!("{items}[1]._parameterName"), "\"@when\"", "string"),
            (&format!("{items}[1]._direction"), "Output", "System.Data.ParameterDirection"),
            (&format!("{items}[1]._value"), "{System.DateTime}", "System.DateTime"),
            (&format!("{items}[1]._value.dateData"), &(638_475_735_091_230_000u64 | (1u64 << 62)).to_string(), "ulong"),
            (&format!("{items}[2]._parameterName"), "\"@bin\"", "string"),
            (&format!("{items}[2]._value"), "{byte[2]}", "byte[]"),
            (&format!("((byte[]){items}[2]._value).Length"), "2", "int"),
            (&format!("((byte[]){items}[2]._value)[0]"), "1", "byte"),
            (&format!("((byte[]){items}[2]._value)[1]"), "171", "byte"),
        ]);
        let got = capture(&mut reader, "System.Data.SqlClient.SqlCommand").unwrap();
        assert_eq!(got.command_text, "SELECT * FROM Orders WHERE Id = @id");
        assert_eq!(got.command_type, "Text");
        let output = Parameter { direction: "Output".into(), ..param("when", Kind::DateTime, "2024-04-01 13:05:09.123") };
        assert_eq!(got.parameters, vec![param("id", Kind::Number, "42"), output, param("bin", Kind::Binary, "01ab")]);
        assert!(!reader.asked.iter().any(|a| a.contains(".Parameters")), "フィールドで読めたら、プロパティは評価しない");
    }

    #[test]
    fn formats_ticks_as_the_calendar_date_and_time() {
        assert_eq!(format_ticks(0), "0001-01-01 00:00:00.000");
        assert_eq!(format_ticks(621_355_968_000_000_000), "1970-01-01 00:00:00.000");
        assert_eq!(format_ticks(638_475_735_091_230_000), "2024-04-01 13:05:09.123");
        assert_eq!(format_ticks(630_822_816_000_000_000), "2000-01-01 00:00:00.000");
    }

    #[test]
    fn asks_bind_by_name_of_oracle_commands_only() {
        let mut sql = Fake::new(&[("this.CommandText", "\"x\"", "string")]);
        capture(&mut sql, "System.Data.SqlClient.SqlCommand");
        assert!(!sql.asked.iter().any(|a| a == "this.BindByName"));
        let mut oracle = Fake::new(&[("this.CommandText", "\"x\"", "string")]);
        let got = capture(&mut oracle, "Oracle.DataAccess.Client.OracleCommand").unwrap();
        assert!(oracle.asked.iter().any(|a| a == "this.BindByName"));
        // 読めなければ既定（位置で結び付ける）。
        assert_eq!(got.bind_by_name, Some(false));
    }

    #[test]
    fn returns_nothing_when_the_frame_is_not_a_command_and_keeps_what_it_can() {
        assert_eq!(capture(&mut Fake::new(&[]), "X.Command"), None);
        let mut reader = Fake::new(&[("this.CommandText", "null", "string"), ("this.Parameters.Count", "1", "int")]);
        let got = capture(&mut reader, "X.Command").unwrap();
        assert_eq!(got.command_text, "");
        assert_eq!(got.command_type, "Text");
        assert_eq!(got.parameters, vec![param("#1", Kind::Other, "?")]);
    }

    #[test]
    fn reads_at_most_max_parameters_and_cuts_long_binary_values() {
        let mut reader = Fake::new(&[("this.CommandText", "\"x\"", "string"), ("this.Parameters.Count", "150", "int")]);
        let got = capture(&mut reader, "X.Command").unwrap();
        assert_eq!(got.parameters.len(), MAX_PARAMETERS);
        assert!(got.truncated);

        let mut reader = Fake::new(&[
            ("this.Parameters[0].Value", "{byte[40]}", "byte[]"),
            ("((byte[])this.Parameters[0].Value).Length", "40", "int"),
            ("System.BitConverter.ToString(((byte[])this.Parameters[0].Value), 0, 32)", &format!("\"{}\"", (0..32).map(|i| format!("{i:02X}")).collect::<Vec<_>>().join("-")), "string"),
        ]);
        let value = read_parameter(&mut reader, "this.Parameters[0]", 0).value;
        assert_eq!(value.chars().count(), 32 * 2 + 1);
        assert!(value.starts_with("000102") && value.ends_with('…'));
    }

    #[test]
    fn tells_the_sql_flavor_from_the_command_type() {
        assert_eq!(dialect_of("Oracle.ManagedDataAccess.Client.OracleCommand"), Dialect::Oracle);
        assert_eq!(dialect_of("Oracle.DataAccess.Client.OracleCommand"), Dialect::Oracle);
        assert_eq!(dialect_of("System.Data.SqlClient.SqlCommand"), Dialect::SqlServer);
        assert_eq!(dialect_of("Microsoft.Data.SqlClient.SqlCommand"), Dialect::SqlServer);
        assert_eq!(dialect_of("System.Data.OleDb.OleDbCommand"), Dialect::Generic);
        assert_eq!(dialect_of("Npgsql.NpgsqlCommand"), Dialect::Generic);
        for method in DEFAULT_METHODS {
            assert!(method.contains("Command.Execute"), "{method}");
        }
    }

    #[test]
    fn fills_in_oracle_named_parameters_by_name_ignoring_case() {
        let c = by_name(command("SELECT * FROM T WHERE A = :Name AND B >= :since AND C = :name", vec![param("name", Kind::String, "O'Brien"), param("since", Kind::DateTime, "2024-04-01 00:00:00.000")]));
        assert_eq!(substitute(&c, Dialect::Oracle), "SELECT * FROM T WHERE A = 'O''Brien' AND B >= TO_TIMESTAMP('2024-04-01 00:00:00.000', 'YYYY-MM-DD HH24:MI:SS.FF3') AND C = 'O''Brien'");
    }

    #[test]
    fn binds_oracle_placeholders_by_position_when_bind_by_name_is_off() {
        let mut c = command("UPDATE T SET X = :x WHERE Y = :y OR Z = :x", vec![param("p1", Kind::Number, "1"), param("p2", Kind::Number, "2")]);
        c.bind_by_name = Some(false);
        assert_eq!(substitute(&c, Dialect::Oracle), "UPDATE T SET X = 1 WHERE Y = 2 OR Z = 1");
    }

    #[test]
    fn fills_in_sql_server_parameters_and_leaves_globals_alone() {
        let c = command(
            "INSERT INTO T VALUES (@id, @when, @bin, @flag); SELECT @@IDENTITY",
            vec![param("id", Kind::Number, "7"), param("when", Kind::DateTime, "2024-04-01 13:05:09.123"), param("bin", Kind::Binary, "01ab"), param("flag", Kind::Other, "True")],
        );
        assert_eq!(substitute(&c, Dialect::SqlServer), "INSERT INTO T VALUES (7, '2024-04-01T13:05:09.123', 0x01ab, 1); SELECT @@IDENTITY");
    }

    #[test]
    fn fills_in_question_marks_in_order() {
        let c = command("SELECT * FROM T WHERE A = ? AND B = ? AND C = ?", vec![param("a", Kind::String, "x"), param("b", Kind::Null, "")]);
        assert_eq!(substitute(&c, Dialect::Generic), "SELECT * FROM T WHERE A = 'x' AND B = NULL AND C = ?");
    }

    #[test]
    fn leaves_placeholders_in_literals_identifiers_and_comments_alone() {
        let c = by_name(command("SELECT ':a' AS \"col:a\", -- :a here\n  /* :a there */ X FROM T WHERE Y = :a AND Z = 'it'':a'", vec![param("a", Kind::Number, "5")]));
        assert_eq!(substitute(&c, Dialect::Oracle), "SELECT ':a' AS \"col:a\", -- :a here\n  /* :a there */ X FROM T WHERE Y = 5 AND Z = 'it'':a'");
    }

    #[test]
    fn does_not_take_assignments_casts_links_or_other_names_for_placeholders() {
        let c = by_name(command("BEGIN x := :a; END; SELECT y::int FROM T@remote WHERE a:b = 1", vec![param("a", Kind::Number, "1")]));
        assert_eq!(substitute(&c, Dialect::Oracle), "BEGIN x := 1; END; SELECT y::int FROM T@remote WHERE a:b = 1");
        assert_eq!(substitute(&by_name(command("SELECT :missing FROM DUAL", vec![])), Dialect::Oracle), "SELECT :missing FROM DUAL");
    }

    #[test]
    fn writes_numbers_as_numbers_only_when_they_are_numbers_and_binary_per_dialect() {
        let c = by_name(command("SELECT :a, :b, :c", vec![param("a", Kind::Number, "-12.5"), param("b", Kind::Number, "1E+21"), param("c", Kind::Number, "NaN")]));
        assert_eq!(substitute(&c, Dialect::Oracle), "SELECT -12.5, 1E+21, 'NaN'");
        let c = by_name(command("SELECT :b", vec![param("b", Kind::Binary, "0102…")]));
        assert_eq!(substitute(&c, Dialect::Oracle), "SELECT HEXTORAW('0102')");
        assert_eq!(substitute(&Command { command_text: "SELECT @b".into(), ..c.clone() }, Dialect::SqlServer), "SELECT 0x0102");
        assert_eq!(substitute(&Command { command_text: "SELECT ?".into(), ..c }, Dialect::Generic), "SELECT X'0102'");
    }

    #[test]
    fn turns_a_stored_procedure_into_the_call_that_runs_it() {
        let mut ret = param("ret", Kind::Number, "0");
        ret.direction = "ReturnValue".into();
        let mut proc = command("PKG.SAVE_ORDER", vec![param("id", Kind::Number, "3"), param("note", Kind::String, "a'b"), ret]);
        proc.command_type = "StoredProcedure".into();
        assert_eq!(substitute(&proc, Dialect::Oracle), "BEGIN PKG.SAVE_ORDER(id => 3, note => 'a''b'); END;");
        assert_eq!(substitute(&proc, Dialect::SqlServer), "EXEC PKG.SAVE_ORDER @id = 3, @note = 'a''b'");
        let mut ping = command("dbo.Ping", vec![]);
        ping.command_type = "StoredProcedure".into();
        assert_eq!(substitute(&ping, Dialect::SqlServer), "EXEC dbo.Ping");
    }

    #[test]
    fn statements_that_differ_only_in_whitespace_are_the_same() {
        assert_eq!(normalize("SELECT  *\nFROM T "), normalize("SELECT * FROM T"));
        assert_ne!(normalize("SELECT * FROM T"), normalize("select * from t"));
    }
}
