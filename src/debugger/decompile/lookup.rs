//! 逆コンパイラが書いたファイルの中を引く: 型の名前からファイル、メンバーの名前から行の範囲。DAP の `netdbg.decompile` と
//! MCP の `decompile` が使う。逆コンパイラ自身が書いた形（ファイルの置き方と字下げ）だけを前提にするので、構文解析は要らない。

/// 型の名前（`Namespace.Type`、`Type`、入れ子は `Outer.Inner` か `Outer+Inner`）から、その型を書いたファイル。入れ子の型は
/// 外の型のファイルにある。名前空間を省いた名前が 2 つ以上の型に当たるなら、候補を挙げて断る。
pub fn type_document<'a>(files: &'a [String], wanted: &str) -> Result<&'a str, String> {
    let wanted = wanted.replace('+', ".");
    let wanted = wanted.split('<').next().unwrap_or(&wanted).trim();
    let named: Vec<(String, &str)> = files.iter().map(|f| (dotted_type_name(f), f.as_str())).collect();
    let tests: [&dyn Fn(&str) -> bool; 4] = [
        &|name| name == wanted,
        &|name| wanted.starts_with(&format!("{name}.")),
        &|name| name.ends_with(&format!(".{wanted}")),
        &|name| wanted.split('.').any(|part| name == part || name.ends_with(&format!(".{part}"))) && wanted.split('.').next().is_some_and(|outer| name == outer || name.ends_with(&format!(".{outer}"))),
    ];
    for test in tests {
        let found: Vec<&(String, &str)> = named.iter().filter(|(name, _)| test(name)).collect();
        match found.as_slice() {
            [] => continue,
            [(_, file)] => return Ok(file),
            many => return Err(tr!("{count} types match {wanted}: {types}. Specify it with its namespace.", wanted = wanted, count = many.len(), types = many.iter().take(10).map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))),
        }
    }
    Err(tr!("There is no type {wanted} in the decompiled source.", wanted = wanted))
}

/// 逆コンパイルしたファイルのパスから、型の名前（`<キャッシュ>\Ns\Sub\Type.cs` → `Ns.Sub.Type`。ジェネリックの `` `2 `` は外す）。
fn dotted_type_name(path: &str) -> String {
    let relative = path.split("\\decompiled\\").nth(1).and_then(|r| r.split_once('\\')).map_or(path, |(_, r)| r);
    let stem = relative.strip_suffix(".cs").unwrap_or(relative);
    stem.split('\\').map(|part| part.split('`').next().unwrap_or(part)).collect::<Vec<_>>().join(".")
}

/// メンバー（メソッド、コンストラクター、プロパティ、イベント）の宣言の行の範囲（1 から、両端を含む）。逆コンパイラは
/// 入れ子の深さで字下げするので、宣言の行と同じ字下げの `}` までが、そのメンバー。
pub fn member_ranges(lines: &[&str], name: &str) -> Vec<(usize, usize)> {
    const STATEMENTS: [&str; 13] = ["if ", "if(", "while ", "for ", "foreach ", "switch ", "using ", "lock ", "catch", "else", "return ", "do", "try"];
    let mut ranges = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        // 型の宣言（コンストラクターと同じ名前）も、メンバーではない。
        let type_declaration = [" class ", " struct ", " interface ", " enum "].iter().any(|k| format!(" {trimmed}").contains(k));
        if trimmed.starts_with("//") || type_declaration || STATEMENTS.iter().any(|s| trimmed.starts_with(s)) {
            continue;
        }
        let declares = |at: usize| {
            let before = &trimmed[..at];
            let after = &trimmed[at + name.len()..];
            (at == 0 || before.ends_with(' ') || before.ends_with('.')) && (after.is_empty() || after.starts_with('(') || after.starts_with('<') || after.starts_with(" {") || after.starts_with(" =>"))
        };
        if !trimmed.match_indices(name).any(|(at, _)| declares(at)) {
            continue;
        }
        // 宣言の行: 次の行が `{` か初期化子 `:`、または 1 行で閉じる（`{ get; set; }`、`;` で終わる抽象メンバー）。
        let next = lines.get(index + 1).map(|l| l.trim()).unwrap_or("");
        let one_line = trimmed.ends_with('}') || (trimmed.ends_with(';') && !trimmed.contains(" = "));
        if !(next == "{" || next.starts_with(": ") || one_line) {
            continue;
        }
        if one_line {
            ranges.push((index + 1, index + 1));
            continue;
        }
        let indent = &line[..line.len() - trimmed.len()];
        let close = format!("{indent}}}");
        if let Some(end) = lines[index + 1..].iter().position(|l| *l == close) {
            ranges.push((index + 1, index + 2 + end));
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<String> {
        [r"C:\cache\decompiled\ABC-v3\Samples\Program.cs", r"C:\cache\decompiled\ABC-v3\Samples\Data\Holder`1.cs", r"C:\cache\decompiled\ABC-v3\Other\Program.cs"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_type_is_found_by_its_full_name_short_name_or_nested_name() {
        let files = files();
        assert_eq!(type_document(&files, "Samples.Program").unwrap(), files[0]);
        assert_eq!(type_document(&files, "Holder").unwrap(), files[1]);
        assert_eq!(type_document(&files, "Samples.Data.Holder<T>").unwrap(), files[1]);
        assert_eq!(type_document(&files, "Samples.Program+Inner").unwrap(), files[0]);
        assert!(type_document(&files, "Program").unwrap_err().contains("2"));
        assert!(type_document(&files, "Missing").is_err());
    }

    #[test]
    fn member_ranges_follow_the_indentation_of_the_declaration() {
        let text = [
            "    public class Account",
            "    {",
            "        public string Owner { get; set; }",
            "",
            "        public Account(string owner)",
            "            : base(owner)",
            "        {",
            "            Owner = owner;",
            "        }",
            "",
            "        public void Deposit(decimal amount)",
            "        {",
            "            if (Deposit(amount))",
            "            {",
            "            }",
            "        }",
            "    }",
        ];
        assert_eq!(member_ranges(&text, "Owner"), vec![(3, 3)]);
        assert_eq!(member_ranges(&text, "Account"), vec![(5, 9)]);
        assert_eq!(member_ranges(&text, "Deposit"), vec![(11, 16)]);
        assert!(member_ranges(&text, "Missing").is_empty());
    }
}
