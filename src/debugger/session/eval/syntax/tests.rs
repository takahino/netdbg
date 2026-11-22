//! 構文解析の単体試験。

use super::*;

fn name(text: &str) -> Box<Expr> {
    Box::new(Expr::Name(text.into()))
}

fn member(target: Box<Expr>, text: &str) -> Box<Expr> {
    Box::new(Expr::Member(target, text.into()))
}

#[test]
fn a_name_alone_is_a_name() {
    assert_eq!(parse("sum"), Ok(Expr::Name("sum".into())));
    assert_eq!(parse("  this "), Ok(Expr::Name("this".into())));
    assert_eq!(parse("$exception"), Ok(Expr::Name("$exception".into())));
}

#[test]
fn members_and_indices_nest_from_the_left() {
    assert_eq!(parse("order.Customer.Name"), Ok(*member(member(name("order"), "Customer"), "Name")));
    assert_eq!(parse("Program.Counts[0]"), Ok(Expr::Index(member(name("Program"), "Counts"), vec![Expr::Int(0)])));
    assert_eq!(parse("grid[1, 2].Value"), Ok(*member(Box::new(Expr::Index(name("grid"), vec![Expr::Int(1), Expr::Int(2)])), "Value")));
    assert_eq!(parse("table[\"key\"]"), Ok(Expr::Index(name("table"), vec![Expr::Str("key".into())])));
    assert_eq!(parse("items[i]"), Ok(Expr::Index(name("items"), vec![Expr::Name("i".into())])));
}

#[test]
fn calls_take_expressions_as_arguments() {
    assert_eq!(parse("form.GetType().FullName"), Ok(*member(Box::new(Expr::Call(member(name("form"), "GetType"), vec![])), "FullName")));
    assert_eq!(
        parse("System.Object.ReferenceEquals(a, b.c)"),
        Ok(Expr::Call(member(member(name("System"), "Object"), "ReferenceEquals"), vec![Expr::Name("a".into()), *member(name("b"), "c")]))
    );
    assert_eq!(parse("Work(21)"), Ok(Expr::Call(name("Work"), vec![Expr::Int(21)])));
}

#[test]
fn literals_and_parentheses_are_read() {
    assert_eq!(parse("42"), Ok(Expr::Int(42)));
    assert_eq!(parse("-7"), Ok(Expr::Unary(UnaryOp::Negate, Box::new(Expr::Int(7)))));
    assert_eq!(parse("-2147483648"), Ok(Expr::Int(i32::MIN)), "int の最小値のリテラル");
    assert_eq!(parse("-9223372036854775808"), Ok(Expr::Long(i64::MIN)));
    assert_eq!(parse("a -2147483648"), Ok(Expr::Binary(BinaryOp::Subtract, Box::new(Expr::Name("a".into())), Box::new(Expr::UInt(2_147_483_648)))), "引き算の右辺は uint");
    assert_eq!(parse("10L"), Ok(Expr::Long(10)));
    assert_eq!(parse("3000000000"), Ok(Expr::UInt(3_000_000_000)), "int に収まらなければ uint、その次が long");
    assert_eq!(parse("5000000000"), Ok(Expr::Long(5_000_000_000)));
    assert_eq!(parse("0xFF"), Ok(Expr::Int(255)));
    assert_eq!(parse("0xFFFFFFFF"), Ok(Expr::UInt(u32::MAX)));
    assert_eq!(parse("0b101"), Ok(Expr::Int(5)));
    assert_eq!(parse("1_000"), Ok(Expr::Int(1000)));
    assert_eq!(parse("10u"), Ok(Expr::UInt(10)));
    assert_eq!(parse("10UL"), Ok(Expr::ULong(10)));
    assert_eq!(parse("1.5f"), Ok(Expr::Float(1.5)));
    assert_eq!(parse("'x'"), Ok(Expr::Char(120)));
    assert_eq!(parse(r"'\n'"), Ok(Expr::Char(10)));
    assert_eq!(parse(r"'é'"), Ok(Expr::Char(0xE9)));
    assert_eq!(parse("1.5"), Ok(Expr::Double(1.5)));
    assert_eq!(parse("2d"), Ok(Expr::Double(2.0)));
    assert_eq!(parse("5.ToString()"), Ok(Expr::Call(member(Box::new(Expr::Int(5)), "ToString"), vec![])), "数のあとの . はメンバーの参照");
    assert_eq!(parse("true"), Ok(Expr::Bool(true)));
    assert_eq!(parse("null"), Ok(Expr::Null));
    assert_eq!(parse(r#""a\"b\n""#), Ok(Expr::Str("a\"b\n".into())));
    assert_eq!(parse("(order).Name"), Ok(*member(name("order"), "Name")));
}

#[test]
fn syntax_outside_the_supported_set_is_refused_with_the_reason() {
    for expression in ["a ? b", "x is", "x as 5", "1 = 2", "x => { goto end; }", "(1) => x", "a + b = 1", "++1", "(a + b)++", "a.", ".a", "", "0x1m", "1.5x", "''", "'ab'"] {
        let error = parse(expression).unwrap_err();
        assert!(!error.is_empty(), "{expression}");
    }
}

#[test]
fn binary_operators_follow_csharp_precedence() {
    use BinaryOp::*;
    let binary = |op, left, right| Expr::Binary(op, Box::new(left), Box::new(right));
    let n = |text: &str| Expr::Name(text.into());
    assert_eq!(parse("a + b * c"), Ok(binary(Add, n("a"), binary(Multiply, n("b"), n("c")))));
    assert_eq!(parse("a - b - c"), Ok(binary(Subtract, binary(Subtract, n("a"), n("b")), n("c"))), "左から結びつく");
    assert_eq!(parse("(a + b) * c"), Ok(binary(Multiply, binary(Add, n("a"), n("b")), n("c"))));
    assert_eq!(parse("a < b == c >= d"), Ok(binary(Equal, binary(Less, n("a"), n("b")), binary(GreaterOrEqual, n("c"), n("d")))));
    assert_eq!(parse("a || b && c"), Ok(binary(Or, n("a"), binary(And, n("b"), n("c")))));
    assert_eq!(parse("i==3"), Ok(binary(Equal, n("i"), Expr::Int(3))), "空白は無くてもよい");
    assert_eq!(parse("a != b"), Ok(binary(NotEqual, n("a"), n("b"))));
    assert_eq!(parse("a <= 1"), Ok(binary(LessOrEqual, n("a"), Expr::Int(1))));
}

#[test]
fn bitwise_and_shift_operators_have_their_own_precedence() {
    use BinaryOp::*;
    let binary = |op, left, right| Expr::Binary(op, Box::new(left), Box::new(right));
    let n = |text: &str| Expr::Name(text.into());
    // & は == より弱く、| は & より弱い。&& は | より弱い。
    assert_eq!(parse("a | b & c"), Ok(binary(BitOr, n("a"), binary(BitAnd, n("b"), n("c")))));
    assert_eq!(parse("a & b == c"), Ok(binary(BitAnd, n("a"), binary(Equal, n("b"), n("c")))));
    assert_eq!(parse("a && b | c"), Ok(binary(And, n("a"), binary(BitOr, n("b"), n("c")))));
    assert_eq!(parse("a ^ b"), Ok(binary(BitXor, n("a"), n("b"))));
    // シフトは、足し算より弱く、比較より強い。
    assert_eq!(parse("a << b + 1"), Ok(binary(ShiftLeft, n("a"), binary(Add, n("b"), Expr::Int(1)))));
    assert_eq!(parse("a >> 1 < b"), Ok(binary(Less, binary(ShiftRight, n("a"), Expr::Int(1)), n("b"))));
    assert_eq!(parse("a >>> 1"), Ok(binary(ShiftRightUnsigned, n("a"), Expr::Int(1))));
    assert_eq!(parse("~a"), Ok(Expr::Unary(UnaryOp::Complement, Box::new(n("a")))));
}

#[test]
fn the_conditional_operator_is_the_weakest_and_binds_to_the_right() {
    let n = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("a ? b : c"), Ok(Expr::Conditional(n("a"), n("b"), n("c"))));
    assert_eq!(parse("a ? b : c ? d : e"), Ok(Expr::Conditional(n("a"), n("b"), Box::new(Expr::Conditional(n("c"), n("d"), n("e"))))));
    assert_eq!(
        parse("a > b ? 1 : 2"),
        Ok(Expr::Conditional(Box::new(Expr::Binary(BinaryOp::Greater, n("a"), n("b"))), Box::new(Expr::Int(1)), Box::new(Expr::Int(2))))
    );
    assert!(parse("a ? b").unwrap_err().contains("':'"));
}

#[test]
fn null_coalescing_binds_to_the_right_and_is_weaker_than_or() {
    let n = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("a ?? b ?? c"), Ok(Expr::Coalesce(n("a"), Box::new(Expr::Coalesce(n("b"), n("c"))))));
    assert_eq!(parse("a ?? b || c"), Ok(Expr::Coalesce(n("a"), Box::new(Expr::Binary(BinaryOp::Or, n("b"), n("c"))))));
}

#[test]
fn a_null_conditional_takes_the_whole_rest_of_the_chain() {
    let receiver = || Box::new(Expr::Receiver);
    // a?.b.c は、a が null なら b も c も評価しない。
    assert_eq!(
        parse("a?.b.c"),
        Ok(Expr::NullConditional(name("a"), Box::new(Expr::Member(Box::new(Expr::Member(receiver(), "b".into())), "c".into()))))
    );
    assert_eq!(parse("a?[0]"), Ok(Expr::NullConditional(name("a"), Box::new(Expr::Index(receiver(), vec![Expr::Int(0)])))));
    assert_eq!(
        parse("a?.b?.c"),
        Ok(Expr::NullConditional(name("a"), Box::new(Expr::NullConditional(Box::new(Expr::Member(receiver(), "b".into())), Box::new(Expr::Member(receiver(), "c".into()))))))
    );
    assert_eq!(parse("s?.ToUpper()"), Ok(Expr::NullConditional(name("s"), Box::new(Expr::Call(Box::new(Expr::Member(receiver(), "ToUpper".into())), vec![])))));
}

fn ty(name: &str) -> TypeSyntax {
    TypeSyntax::named(name)
}

#[test]
fn a_cast_is_told_apart_from_a_parenthesized_expression() {
    let n = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("(long)a"), Ok(Expr::Cast(ty("System.Int64"), n("a"))));
    assert_eq!(parse("(Shade)1"), Ok(Expr::Cast(ty("Shade"), Box::new(Expr::Int(1)))));
    assert_eq!(parse("(App.Model.Order)item"), Ok(Expr::Cast(ty("App.Model.Order"), n("item"))));
    assert_eq!(parse("(int)-1"), Ok(Expr::Cast(ty("System.Int32"), Box::new(Expr::Unary(UnaryOp::Negate, Box::new(Expr::Int(1)))))));
    // キャストは二項演算子より強い。
    assert_eq!(parse("(double)a / b"), Ok(Expr::Binary(BinaryOp::Divide, Box::new(Expr::Cast(ty("System.Double"), n("a"))), n("b"))));
    // 括弧でくくった式。
    assert_eq!(parse("(a) - b"), Ok(Expr::Binary(BinaryOp::Subtract, n("a"), n("b"))));
    assert_eq!(parse("(a).Name"), Ok(Expr::Member(n("a"), "Name".into())));
    assert_eq!(parse("(a + b) * c"), Ok(Expr::Binary(BinaryOp::Multiply, Box::new(Expr::Binary(BinaryOp::Add, n("a"), n("b"))), n("c"))));
    assert_eq!(parse("(a)"), Ok(Expr::Name("a".into())));
}

#[test]
fn is_and_as_take_a_type_or_a_pattern() {
    let n = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("x is string"), Ok(Expr::Is(n("x"), Pattern::TypeOrConstant(Some(ty("System.String")), n("System.String")))));
    assert_eq!(parse("x as Order"), Ok(Expr::As(n("x"), ty("Order"))));
    assert_eq!(parse("x is null"), Ok(Expr::Is(n("x"), Pattern::Null)));
    assert_eq!(parse("x is not null"), Ok(Expr::Is(n("x"), Pattern::Not(Box::new(Pattern::Null)))));
    assert_eq!(parse("a is > 5"), Ok(Expr::Is(n("a"), Pattern::Relational(BinaryOp::Greater, Box::new(Expr::Int(5))))));
    assert_eq!(parse("a is 5"), Ok(Expr::Is(n("a"), Pattern::TypeOrConstant(None, Box::new(Expr::Int(5))))));
    // `x is Shade.Dark` は、型かもしれないし、定数かもしれない。両方の読み方を持っておく。
    assert_eq!(parse("x is Shade.Dark"), Ok(Expr::Is(n("x"), Pattern::TypeOrConstant(Some(ty("Shade.Dark")), Box::new(Expr::Member(n("Shade"), "Dark".into()))))));
    // is は && より強い。
    assert_eq!(parse("x is string && ok"), Ok(Expr::Binary(BinaryOp::And, Box::new(Expr::Is(n("x"), Pattern::TypeOrConstant(Some(ty("System.String")), n("System.String")))), n("ok"))));
}

#[test]
fn type_expressions_take_a_type_in_parentheses() {
    assert_eq!(parse("typeof(int)"), Ok(Expr::TypeOf(ty("System.Int32"))));
    assert_eq!(parse("default(string)"), Ok(Expr::Default(ty("System.String"))));
    assert_eq!(parse("sizeof(long)"), Ok(Expr::SizeOf(ty("System.Int64"))));
    assert_eq!(parse("default(int?)"), Ok(Expr::Default(TypeSyntax { nullable: true, ..ty("System.Int32") })));
    assert_eq!(parse("typeof(int[])"), Ok(Expr::TypeOf(TypeSyntax { array: true, ..ty("System.Int32") })));
    assert_eq!(parse("nameof(order.Name)"), Ok(Expr::Str("Name".into())));
    assert_eq!(parse("nameof(a)"), Ok(Expr::Str("a".into())));
    assert_eq!(parse("int.MaxValue"), Ok(Expr::Member(Box::new(Expr::Name("System.Int32".into())), "MaxValue".into())));
    assert!(parse("typeof(1)").is_err());
    assert!(parse("nameof(1 + 2)").is_err());
}

#[test]
fn unary_operators_bind_tighter_than_binary_ones() {
    let n = |text: &str| Expr::Name(text.into());
    assert_eq!(parse("!ok"), Ok(Expr::Unary(UnaryOp::Not, Box::new(n("ok")))));
    assert_eq!(parse("!a && b"), Ok(Expr::Binary(BinaryOp::And, Box::new(Expr::Unary(UnaryOp::Not, Box::new(n("a")))), Box::new(n("b")))));
    assert_eq!(parse("a - -b"), Ok(Expr::Binary(BinaryOp::Subtract, Box::new(n("a")), Box::new(Expr::Unary(UnaryOp::Negate, Box::new(n("b")))))));
    assert_eq!(parse("!list.Contains(x)"), Ok(Expr::Unary(UnaryOp::Not, Box::new(Expr::Call(member(name("list"), "Contains"), vec![n("x")])))));
}

#[test]
fn an_unsupported_construct_is_named_in_the_error() {
    assert!(parse("x => { yield return 1; }").unwrap_err().contains("ラムダ"));
    let call = |target: Expr, name: &str, arguments: Vec<Expr>| Expr::Call(Box::new(Expr::Member(Box::new(target), name.into())), arguments);
    let one = |v: &str, body: Expr| Expr::Lambda(vec![(None, v.into())], Box::new(body));
    let two = |a: &str, b: &str, body: Expr| Expr::Lambda(vec![(None, a.into()), (None, b.into())], Box::new(body));
    let n = |v: &str| Expr::Name(v.into());
    // 範囲の変数が 2 つ以上になる所は、タプルにまとめ、あとの句のラムダの先頭で取り出す。
    let item = |path: &[&str]| path.iter().fold(n("$t"), |value, part| Expr::Member(Box::new(value), (*part).into()));
    let unpacked = |locals: Vec<(&str, Expr)>, body: Expr| {
        let mut statements: Vec<Statement> = locals.into_iter().map(|(name, value)| Statement::Local(None, vec![(name.into(), Some(value))])).collect();
        statements.push(Statement::Return(Some(body)));
        Expr::Lambda(vec![(None, "$t".into())], Box::new(Expr::Block(statements)))
    };
    assert_eq!(
        parse("from n in xs let m = n select m"),
        Ok(call(call(n("xs"), "Select", vec![one("n", Expr::Tuple(vec![n("n"), n("n")]))]), "Select", vec![unpacked(vec![("m", item(&["Item2"]))], n("m"))]))
    );
    assert_eq!(
        parse("from a in xs from b in ys where a > b select a"),
        Ok(call(
            call(
                call(n("xs"), "SelectMany", vec![one("a", n("ys")), two("a", "b", Expr::Tuple(vec![n("a"), n("b")]))]),
                "Where",
                vec![unpacked(vec![("a", item(&["Item1"])), ("b", item(&["Item2"]))], Expr::Binary(BinaryOp::Greater, Box::new(n("a")), Box::new(n("b"))))]
            ),
            "Select",
            vec![unpacked(vec![("a", item(&["Item1"]))], n("a"))]
        ))
    );
    // 3 つ目の変数は、タプルの入れ子（((a, b), c)）。
    assert!(matches!(parse("from a in xs let b = a let c = b select a + c"), Ok(Expr::Call(_, arguments)) if arguments[0] == unpacked(vec![("a", item(&["Item1", "Item1"])), ("c", item(&["Item2"]))], Expr::Binary(BinaryOp::Add, Box::new(n("a")), Box::new(n("c"))))));
    assert!(parse("from n in xs where n is T select n").is_ok(), "クエリの中では、select は変数の名前ではない");
    assert!(parse("from string s in xs select s").is_ok());
    assert_eq!(parse("from n in xs group n by k"), Ok(call(n("xs"), "GroupBy", vec![one("n", n("k"))])));
    assert_eq!(parse("from n in xs group v by k"), Ok(call(n("xs"), "GroupBy", vec![one("n", n("k")), one("n", n("v"))])));
    assert_eq!(parse("from n in xs group n by k into g select g"), Ok(call(call(n("xs"), "GroupBy", vec![one("n", n("k"))]), "Select", vec![one("g", n("g"))])));
    assert_eq!(parse("from a in xs from b in ys select c"), Ok(call(n("xs"), "SelectMany", vec![one("a", n("ys")), two("a", "b", n("c"))])));
    assert_eq!(parse("from a in xs join b in ys on p equals q select c"), Ok(call(n("xs"), "Join", vec![n("ys"), one("a", n("p")), one("b", n("q")), two("a", "b", n("c"))])));
    assert_eq!(parse("from a in xs join b in ys on p equals q into g select c"), Ok(call(n("xs"), "GroupJoin", vec![n("ys"), one("a", n("p")), one("b", n("q")), two("a", "g", n("c"))])));
    assert_eq!(parse("from a in xs select b into c where d select c"), Ok(call(call(n("xs"), "Select", vec![one("a", n("b"))]), "Where", vec![one("c", n("d"))])));
}

#[test]
fn unclosed_brackets_and_strings_say_what_is_missing() {
    assert!(parse("items[0").unwrap_err().contains("']'"));
    assert!(parse("Work(1, 2").unwrap_err().contains("')'"));
    assert!(parse("\"abc").unwrap_err().contains("閉じていません"));
    assert!(parse("(a").unwrap_err().contains("')'"));
}

#[test]
fn any_value_can_be_called_as_a_delegate() {
    assert_eq!(parse("items[0](1)"), Ok(Expr::Call(Box::new(Expr::Index(name("items"), vec![Expr::Int(0)])), vec![Expr::Int(1)])));
    assert_eq!(parse("f(1)(2)"), Ok(Expr::Call(Box::new(Expr::Call(name("f"), vec![Expr::Int(1)])), vec![Expr::Int(2)])));
}

/// 実際のコードから抜き出した式（`tools/exprgen corpus` の出力）を構文解析に通して、読めた割合と、読めなかった式に
/// 多い構文を報告する（docs/DECISIONS.md D-026 の 4）。入力のファイルは環境変数 `NETDBG_EXPR_CORPUS` で渡し、
/// 報告は、その隣の `.report.txt` に書く。ふだんの試験では動かさない。
#[test]
#[ignore = "NETDBG_EXPR_CORPUS に、exprgen corpus の出力を指定して動かす"]
fn corpus_report() {
    use std::collections::HashMap;
    use std::fmt::Write;
    let path = std::env::var("NETDBG_EXPR_CORPUS").expect("NETDBG_EXPR_CORPUS が無い");
    let input = std::fs::read_to_string(&path).expect("入力を読めない");
    // 種類ごとの「出てきた回数、読めなかった回数」。回数は、式がソースに出てきた回数で数える。
    let mut kinds: HashMap<&str, (u64, u64)> = HashMap::new();
    let mut errors: HashMap<String, (u64, String)> = HashMap::new();
    // 読めなかった式の全部（回数、エラーの先頭、式）。報告の例は 1 つずつなので、残りを追うときはこちらを見る。
    let mut unparsed = String::new();
    // 読めた式のうち、ラムダ式を含むものと、その本体に式木へ組み立てられない書き方があるもの（評価で断る）。
    let (mut with_lambda, mut lambda_gapped) = (0u64, 0u64);
    let mut gap_kinds: HashMap<&str, (u64, String)> = HashMap::new();
    let mut gapped = String::new();
    let (mut total, mut accepted, mut distinct, mut distinct_accepted) = (0u64, 0u64, 0u64, 0u64);
    for line in input.lines() {
        let mut columns = line.splitn(3, '\t');
        let (Some(count), Some(kind_list), Some(text)) = (columns.next().and_then(|c| c.parse::<u64>().ok()), columns.next(), columns.next()) else { continue };
        let result = parse(text);
        total += count;
        distinct += 1;
        if result.is_ok() {
            accepted += count;
            distinct_accepted += 1;
        }
        if let Ok(expr) = &result {
            if expr.any(&mut |inner| matches!(inner, Expr::Lambda(..))) {
                with_lambda += count;
                let gaps = crate::debugger::session::eval::lambda::gaps::lambda_gaps(expr);
                if !gaps.is_empty() {
                    lambda_gapped += count;
                    writeln!(gapped, "{count}\t{}\t{text}", gaps.join(",")).unwrap();
                }
                for gap in gaps {
                    let entry = gap_kinds.entry(gap).or_insert_with(|| (0, text.to_string()));
                    entry.0 += count;
                }
            }
        }
        for kind in kind_list.split(',') {
            let entry = kinds.entry(kind).or_default();
            entry.0 += count;
            entry.1 += if result.is_ok() { 0 } else { count };
        }
        if let Err(message) = result {
            let head: String = message.chars().take(60).collect();
            writeln!(unparsed, "{count}\t{head}\t{text}").unwrap();
            let entry = errors.entry(head).or_insert_with(|| (0, text.to_string()));
            entry.0 += count;
        }
    }
    let percent = |part: u64, whole: u64| part as f64 * 100.0 / whole.max(1) as f64;
    let mut report = String::new();
    writeln!(report, "式 {total} 個中 {accepted} 個を読めた（{:.1}%）。重複を除くと {distinct} 種類中 {distinct_accepted} 種類（{:.1}%）。", percent(accepted, total), percent(distinct_accepted, distinct)).unwrap();
    writeln!(report, "\n読めなかった式に含まれる構文（読めなかった回数の多い順。右は、その構文を含む式のうち読めなかった割合）").unwrap();
    let mut by_kind: Vec<_> = kinds.into_iter().filter(|(_, (_, rejected))| *rejected > 0).collect();
    by_kind.sort_by_key(|(kind, (_, rejected))| (std::cmp::Reverse(*rejected), *kind));
    for (kind, (seen, rejected)) in by_kind.iter().take(70) {
        writeln!(report, "{rejected:>8} / {seen:>8}  {:>5.1}%  {kind}", percent(*rejected, *seen)).unwrap();
    }
    writeln!(report, "\nエラーの文ごと（回数、文の先頭、例）").unwrap();
    let mut by_error: Vec<_> = errors.into_iter().collect();
    by_error.sort_by_key(|(message, (count, _))| (std::cmp::Reverse(*count), message.clone()));
    for (message, (count, example)) in by_error.iter().take(40) {
        writeln!(report, "{count:>8}  {message}  例: {example}").unwrap();
    }
    writeln!(report, "\nラムダ式を含む式 {with_lambda} 個のうち、本体に式木へ組み立てられない書き方があるもの {lambda_gapped} 個（{:.1}%）", percent(lambda_gapped, with_lambda)).unwrap();
    let mut by_gap: Vec<_> = gap_kinds.into_iter().collect();
    by_gap.sort_by_key(|(gap, (count, _))| (std::cmp::Reverse(*count), *gap));
    for (gap, (count, example)) in &by_gap {
        writeln!(report, "{count:>8}  {gap}  例: {example}").unwrap();
    }
    std::fs::write(format!("{path}.report.txt"), report).expect("報告を書けない");
    std::fs::write(format!("{path}.unparsed.txt"), unparsed).expect("読めなかった式の一覧を書けない");
    std::fs::write(format!("{path}.lambda-gaps.txt"), gapped).expect("組み立てられないラムダ式の一覧を書けない");
}

#[test]
fn escapes_exponents_and_the_null_forgiving_mark_are_read() {
    assert_eq!(parse(r#""Aé\x41\t\a""#), Ok(Expr::Str("A\u{e9}A\t\u{7}".into())));
    assert_eq!(parse(r#""\U0001F600""#), Ok(Expr::Str("\u{1F600}".into())));
    assert_eq!(parse(r"'\x000f'"), Ok(Expr::Char(0x0F)));
    assert_eq!(parse(r"'\v'"), Ok(Expr::Char(0x0B)));
    assert!(parse(r#""\q""#).is_err());
    assert_eq!(parse("1e3"), Ok(Expr::Double(1000.0)));
    assert_eq!(parse("2.5E-1"), Ok(Expr::Double(0.25)));
    assert_eq!(parse("1e2f"), Ok(Expr::Float(100.0)));
    assert_eq!(parse("5.78960446186581E+77d"), Ok(Expr::Double(5.78960446186581E+77)));
    // x! は x と同じ。!= は比較のまま。
    assert_eq!(parse("a!.b"), Ok(Expr::Member(name("a"), "b".into())));
    assert_eq!(parse("a!"), Ok(Expr::Name("a".into())));
    assert_eq!(parse("a != b"), Ok(Expr::Binary(BinaryOp::NotEqual, name("a"), name("b"))));
    assert_eq!(parse("+a"), Ok(Expr::Unary(UnaryOp::Plus, name("a"))));
    assert_eq!(parse("++a"), Ok(Expr::Increment { target: name("a"), decrement: false, prefix: true }));
}

#[test]
fn assignments_bind_weakest_and_to_the_right() {
    let n = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("a = 1"), Ok(Expr::Assign(n("a"), Assignment::Plain, Box::new(Expr::Int(1)))));
    assert_eq!(parse("a = b = 1"), Ok(Expr::Assign(n("a"), Assignment::Plain, Box::new(Expr::Assign(n("b"), Assignment::Plain, Box::new(Expr::Int(1)))))));
    assert_eq!(parse("a += b * 2"), Ok(Expr::Assign(n("a"), Assignment::Compound(BinaryOp::Add), Box::new(Expr::Binary(BinaryOp::Multiply, n("b"), Box::new(Expr::Int(2)))))));
    assert_eq!(parse("a <<= 1"), Ok(Expr::Assign(n("a"), Assignment::Compound(BinaryOp::ShiftLeft), Box::new(Expr::Int(1)))));
    assert_eq!(parse("a ??= b"), Ok(Expr::Assign(n("a"), Assignment::Coalesce, n("b"))));
    assert_eq!(parse("x.y[0] = 1"), Ok(Expr::Assign(Box::new(Expr::Index(member(name("x"), "y"), vec![Expr::Int(0)])), Assignment::Plain, Box::new(Expr::Int(1)))));
    // 比較とラムダは、代入ではない。
    assert_eq!(parse("a == 1"), Ok(Expr::Binary(BinaryOp::Equal, n("a"), Box::new(Expr::Int(1)))));
    assert_eq!(parse("a => 1"), Ok(Expr::Lambda(vec![(None, "a".into())], Box::new(Expr::Int(1)))));
    // 増減
    assert_eq!(parse("i++"), Ok(Expr::Increment { target: n("i"), decrement: false, prefix: false }));
    assert_eq!(parse("--i"), Ok(Expr::Increment { target: n("i"), decrement: true, prefix: true }));
    assert_eq!(parse("i++ + i"), Ok(Expr::Binary(BinaryOp::Add, Box::new(Expr::Increment { target: n("i"), decrement: false, prefix: false }), n("i"))));
    assert_eq!(parse("a - -b"), Ok(Expr::Binary(BinaryOp::Subtract, n("a"), Box::new(Expr::Unary(UnaryOp::Negate, n("b"))))));
    assert!(parse("(a + b) = 1").is_err());
}

#[test]
fn generic_types_and_creations_are_read() {
    let int = || ty("System.Int32");
    let list = TypeSyntax { arguments: vec![int()], ..ty("List") };
    assert_eq!(parse("typeof(List<int>)"), Ok(Expr::TypeOf(list.clone())));
    assert_eq!(parse("typeof(Dictionary<string, List<int>>)"), Ok(Expr::TypeOf(TypeSyntax { arguments: vec![ty("System.String"), list.clone()], ..ty("Dictionary") })));
    assert_eq!(parse("x is List<int>"), Ok(Expr::Is(name("x"), Pattern::TypeOrConstant(Some(list.clone()), Box::new(Expr::Null)))));
    // 比較の < は、型引数ではない。
    assert_eq!(parse("a < b"), Ok(Expr::Binary(BinaryOp::Less, name("a"), name("b"))));
    assert_eq!(parse("new Point()"), Ok(Expr::New { ty: ty("Point"), arguments: vec![], initializer: Initializer::None }));
    assert_eq!(parse("new System.Text.StringBuilder(\"ab\").Length"), Ok(Expr::Member(Box::new(Expr::New { ty: ty("System.Text.StringBuilder"), arguments: vec![Expr::Str("ab".into())], initializer: Initializer::None }), "Length".into())));
    assert_eq!(parse("new Point { X = 3, Y = 4 }"), Ok(Expr::New { ty: ty("Point"), arguments: vec![], initializer: Initializer::Members(vec![("X".into(), Expr::Int(3)), ("Y".into(), Expr::Int(4))]) }));
    assert_eq!(parse("new List<int> { 1, 2 }"), Ok(Expr::New { ty: list.clone(), arguments: vec![], initializer: Initializer::Items(vec![vec![Expr::Int(1)], vec![Expr::Int(2)]]) }));
    assert_eq!(parse("new D { [\"k\"] = 1 }"), Ok(Expr::New { ty: ty("D"), arguments: vec![], initializer: Initializer::Indexed(vec![(vec![Expr::Str("k".into())], Expr::Int(1))]) }));
    assert_eq!(parse("new int[3]"), Ok(Expr::NewArray { element: Some(int()), sizes: vec![Expr::Int(3)], items: None }));
    assert_eq!(parse("new int[2, 3]"), Ok(Expr::NewArray { element: Some(int()), sizes: vec![Expr::Int(2), Expr::Int(3)], items: None }));
    assert_eq!(parse("new int[] { 1, 2 }"), Ok(Expr::NewArray { element: Some(int()), sizes: vec![], items: Some(vec![Expr::Int(1), Expr::Int(2)]) }));
    assert_eq!(parse("new[] { 1, 2 }"), Ok(Expr::NewArray { element: None, sizes: vec![], items: Some(vec![Expr::Int(1), Expr::Int(2)]) }));
}

#[test]
fn verbatim_raw_and_interpolated_strings_are_read() {
    assert_eq!(parse(r#"@"c:\x""#), Ok(Expr::Str(r"c:\x".into())));
    assert_eq!(parse(r#"@"a""b""#), Ok(Expr::Str("a\"b".into())));
    assert_eq!(parse(r#""""raw "x" text""""#), Ok(Expr::Str("raw \"x\" text".into())));
    assert_eq!(parse(r#""""#), Ok(Expr::Str(String::new())));
    let hole = |text: &str, alignment: Option<&str>, format: Option<&str>| Piece::Hole { expr: Expr::Name(text.into()), alignment: alignment.map(String::from), format: format.map(String::from) };
    assert_eq!(parse(r#"$"{a}-{b}""#), Ok(Expr::Interpolated(vec![hole("a", None, None), Piece::Text("-".into()), hole("b", None, None)])));
    assert_eq!(parse(r#"$"{a,4}|{d:F2}""#), Ok(Expr::Interpolated(vec![hole("a", Some("4"), None), Piece::Text("|".into()), hole("d", None, Some("F2"))])));
    assert_eq!(parse(r#"$"{a,-4:X}{{x}}""#), Ok(Expr::Interpolated(vec![hole("a", Some("-4"), Some("X")), Piece::Text("{x}".into())])));
    assert_eq!(parse(r#"$@"{a}\{b}""#), Ok(Expr::Interpolated(vec![hole("a", None, None), Piece::Text("\\".into()), hole("b", None, None)])));
    // 式の中の文字列と、括弧の中の : は、式の一部。
    let Ok(Expr::Interpolated(pieces)) = parse(r#"$"{(a > b ? "y" : "n")}!""#) else { panic!("補間文字列として読めていない") };
    assert!(matches!(&pieces[0], Piece::Hole { expr: Expr::Conditional(..), alignment: None, format: None }));
    assert_eq!(pieces[1], Piece::Text("!".into()));
    assert!(parse(r#"$"{a""#).is_err());
    assert_eq!(parse("unchecked(a + 1)"), Ok(Expr::Checked(false, Box::new(Expr::Binary(BinaryOp::Add, name("a"), Box::new(Expr::Int(1)))))));
    assert_eq!(parse("checked(a + 1)"), Ok(Expr::Checked(true, Box::new(Expr::Binary(BinaryOp::Add, name("a"), Box::new(Expr::Int(1)))))));
    assert_eq!(parse("$exception"), Ok(Expr::Name("$exception".into())));
}

#[test]
fn named_arguments_and_generic_calls_are_read() {
    assert_eq!(parse("Join(\"x\", times: 2)"), Ok(Expr::Call(name("Join"), vec![Expr::Str("x".into()), Expr::Named("times".into(), Box::new(Expr::Int(2)))])));
    assert_eq!(parse("Echo<int>(a)"), Ok(Expr::Call(Box::new(Expr::Generic(name("Echo"), vec![ty("System.Int32")])), vec![Expr::Name("a".into())])));
    assert_eq!(parse("x.Cast<string>()"), Ok(Expr::Call(Box::new(Expr::Generic(member(name("x"), "Cast"), vec![ty("System.String")])), vec![])));
    // 比較は、型引数ではない。条件演算子の : も、名前つき引数ではない。
    assert_eq!(parse("a < b"), Ok(Expr::Binary(BinaryOp::Less, name("a"), name("b"))));
    assert_eq!(parse("F(a < b, c > d)"), Ok(Expr::Call(name("F"), vec![Expr::Binary(BinaryOp::Less, name("a"), name("b")), Expr::Binary(BinaryOp::Greater, name("c"), name("d"))])));
    // `>` のあとに `(` が続けば、C# の仕様（6.2.5）どおり、型引数つきの呼び出し（空白があっても）。
    assert_eq!(parse("F(a < b, c > (d))"), Ok(Expr::Call(name("F"), vec![Expr::Call(Box::new(Expr::Generic(name("a"), vec![ty("b"), ty("c")])), vec![Expr::Name("d".into())])])));
    // 呼び出しでない `名前<型>` は、メソッドの集まり。
    assert_eq!(parse("F(registry.Get<string>)"), Ok(Expr::Call(name("F"), vec![Expr::Generic(member(name("registry"), "Get"), vec![ty("System.String")])])));
    assert_eq!(parse("F(a ? b : c)"), Ok(Expr::Call(name("F"), vec![Expr::Conditional(name("a"), name("b"), name("c"))])));
}

#[test]
fn patterns_combine_and_nest() {
    let int = |n| Box::new(Expr::Int(n));
    let constant = |n| Pattern::TypeOrConstant(None, int(n));
    assert_eq!(parse("a is 7 or 8"), Ok(Expr::Is(name("a"), Pattern::Or(Box::new(constant(7)), Box::new(constant(8))))));
    assert_eq!(parse("a is > 5 and < 10"), Ok(Expr::Is(name("a"), Pattern::And(Box::new(Pattern::Relational(BinaryOp::Greater, int(5))), Box::new(Pattern::Relational(BinaryOp::Less, int(10)))))));
    // not は and より強い。
    assert_eq!(parse("a is not 1 and 2"), Ok(Expr::Is(name("a"), Pattern::And(Box::new(Pattern::Not(Box::new(constant(1)))), Box::new(constant(2))))));
    assert_eq!(parse("x is int k && k > 40"), Ok(Expr::Binary(BinaryOp::And, Box::new(Expr::Is(name("x"), Pattern::Declaration(ty("System.Int32"), "k".into()))), Box::new(Expr::Binary(BinaryOp::Greater, name("k"), int(40))))));
    assert_eq!(parse("x is var v"), Ok(Expr::Is(name("x"), Pattern::Var("v".into()))));
    let nullable_int = TypeSyntax { nullable: true, ..ty("System.Int32") };
    assert_eq!(parse("x is int?"), Ok(Expr::Is(name("x"), Pattern::TypeOrConstant(Some(nullable_int.clone()), Box::new(Expr::Null)))));
    assert_eq!(parse("(x is int?)"), Ok(Expr::Is(name("x"), Pattern::TypeOrConstant(Some(nullable_int), Box::new(Expr::Null)))));
    assert!(matches!(parse("x is Shade ? a : b"), Ok(Expr::Conditional(..))), "? のあとに式が続けば条件演算子");
    assert_eq!(parse("s is { Length: 5 }"), Ok(Expr::Is(name("s"), Pattern::Recursive { ty: None, positional: None, members: vec![(vec!["Length".into()], constant(5))], designation: None })));
    assert_eq!(parse("p is Point { X.Y: > 1 } q"), Ok(Expr::Is(name("p"), Pattern::Recursive { ty: Some(ty("Point")), positional: None, members: vec![(vec!["X".into(), "Y".into()], Pattern::Relational(BinaryOp::Greater, int(1)))], designation: Some("q".into()) })));
    assert_eq!(parse("t is (7, _)"), Ok(Expr::Is(name("t"), Pattern::Recursive { ty: None, positional: Some(vec![constant(7), Pattern::Discard]), members: vec![], designation: None })));
    assert_eq!(parse("a is (1 or 2)"), Ok(Expr::Is(name("a"), Pattern::Or(Box::new(constant(1)), Box::new(constant(2))))));
    assert_eq!(parse("xs is [1, .., 3]"), Ok(Expr::Is(name("xs"), Pattern::List { items: vec![constant(1), constant(3)], slice: Some(1), designation: None })));
    assert_eq!(parse("x is string or null"), Ok(Expr::Is(name("x"), Pattern::Or(Box::new(Pattern::TypeOrConstant(Some(ty("System.String")), name("System.String"))), Box::new(Pattern::Null)))));
}

#[test]
fn switch_with_tuples_and_ranges_are_read() {
    let arms = |text: &str| match parse(text) {
        Ok(Expr::Switch(_, arms)) => arms.len(),
        other => panic!("{other:?}"),
    };
    assert_eq!(arms("a switch { 7 => \"seven\", _ => \"other\" }"), 2);
    assert_eq!(arms("x switch { int m when m > 40 => 1, string => 2, _ => 3, }"), 3);
    // switch は掛け算より強い。
    assert!(matches!(parse("a switch { _ => 1 } * 2"), Ok(Expr::Binary(BinaryOp::Multiply, ..))));
    assert_eq!(parse("(1, \"a\")"), Ok(Expr::Tuple(vec![Expr::Int(1), Expr::Str("a".into())])));
    assert_eq!(parse("p with { A = 5 }"), Ok(Expr::With(name("p"), vec![("A".into(), Expr::Int(5))])));
    assert_eq!(parse("xs[^1]"), Ok(Expr::Index(name("xs"), vec![Expr::FromEnd(Box::new(Expr::Int(1)))])));
    assert_eq!(parse("xs[1..]"), Ok(Expr::Index(name("xs"), vec![Expr::Range(Some(Box::new(Expr::Int(1))), None)])));
    assert_eq!(parse("s[..^1]"), Ok(Expr::Index(name("s"), vec![Expr::Range(None, Some(Box::new(Expr::FromEnd(Box::new(Expr::Int(1))))))])));
    assert_eq!(parse("a ^ b"), Ok(Expr::Binary(BinaryOp::BitXor, name("a"), name("b"))));
}

#[test]
fn out_arguments_anonymous_types_and_open_generics_are_read() {
    assert_eq!(parse("F(8, out _)"), Ok(Expr::Call(name("F"), vec![Expr::Int(8), Expr::Out(OutTarget::Discard)])));
    assert_eq!(parse("F(out var x)"), Ok(Expr::Call(name("F"), vec![Expr::Out(OutTarget::Declare(None, "x".into()))])));
    assert_eq!(parse("F(out int x)"), Ok(Expr::Call(name("F"), vec![Expr::Out(OutTarget::Declare(Some(ty("System.Int32")), "x".into()))])));
    assert_eq!(parse("F(out y, ref z)"), Ok(Expr::Call(name("F"), vec![Expr::Out(OutTarget::Existing(name("y"))), Expr::Out(OutTarget::Existing(name("z")))])));
    assert_eq!(parse("new { A = 1, p.B }"), Ok(Expr::Anonymous(vec![("A".into(), Expr::Int(1)), ("B".into(), Expr::Member(name("p"), "B".into()))])));
    let open = TypeSyntax { arguments: vec![ty(""), ty("")], ..ty("Dictionary") };
    assert_eq!(parse("typeof(Dictionary<,>)"), Ok(Expr::TypeOf(open)));
}

#[test]
fn lambdas_and_queries_are_read() {
    let lambda = |names: &[&str], body: Expr| Expr::Lambda(names.iter().map(|n| (None, n.to_string())).collect(), Box::new(body));
    assert_eq!(parse("xs.Count(n => n > a)"), Ok(Expr::Call(member(name("xs"), "Count"), vec![lambda(&["n"], Expr::Binary(BinaryOp::Greater, name("n"), name("a")))])));
    assert_eq!(parse("F((w, i) => w + i)"), Ok(Expr::Call(name("F"), vec![lambda(&["w", "i"], Expr::Binary(BinaryOp::Add, name("w"), name("i")))])));
    assert_eq!(parse("F(() => 1)"), Ok(Expr::Call(name("F"), vec![lambda(&[], Expr::Int(1))])));
    assert_eq!(parse("F((int x) => x)"), Ok(Expr::Call(name("F"), vec![Expr::Lambda(vec![(Some(ty("System.Int32")), "x".into())], name("x"))])));
    // キャストと括弧の式は、ラムダではない。
    assert_eq!(parse("(int)x"), Ok(Expr::Cast(ty("System.Int32"), name("x"))));
    assert_eq!(parse("(a)"), Ok(Expr::Name("a".into())));
    // 任意の式の呼び出し
    assert!(matches!(parse("((Func<int, int>)(x => x * 3))(a)"), Ok(Expr::Call(callee, _)) if matches!(*callee, Expr::Cast(..))));
    // クエリ式
    assert_eq!(parse("from n in xs where n > 10 select n"), Ok(Expr::Call(member(name("xs"), "Where"), vec![lambda(&["n"], Expr::Binary(BinaryOp::Greater, name("n"), Box::new(Expr::Int(10))))])));
    assert_eq!(parse("from n in xs select n * 2"), Ok(Expr::Call(member(name("xs"), "Select"), vec![lambda(&["n"], Expr::Binary(BinaryOp::Multiply, name("n"), Box::new(Expr::Int(2))))])));
    assert!(parse("x => { foreach (var y in x) { } }").is_ok());
    assert_eq!(parse_type("System.Func<int, bool>"), Some(TypeSyntax { arguments: vec![ty("System.Int32"), ty("System.Boolean")], ..ty("System.Func") }));
}

#[test]
fn nullable_element_arrays_and_the_default_literal_are_read() {
    let object = TypeSyntax { nullable: true, ..ty("System.Object") };
    assert_eq!(parse("new object?[] { 1 }"), Ok(Expr::NewArray { element: Some(object), sizes: vec![], items: Some(vec![Expr::Int(1)]) }));
    assert_eq!(parse("a?[0]"), Ok(Expr::NullConditional(name("a"), Box::new(Expr::Index(Box::new(Expr::Receiver), vec![Expr::Int(0)])))));
    assert_eq!(parse("x == default"), Ok(Expr::Binary(BinaryOp::Equal, name("x"), Box::new(Expr::DefaultLiteral))));
    assert_eq!(parse("default(int)"), Ok(Expr::Default(ty("System.Int32"))));
}

#[test]
fn older_and_rarer_forms_are_read() {
    assert_eq!(parse("global::System.Math.PI"), Ok(Expr::Member(member(name("System"), "Math"), "PI".into())));
    assert_eq!(parse("delegate (int x) { return x + 1; }"), Ok(Expr::Lambda(vec![(Some(ty("System.Int32")), "x".into())], Box::new(Expr::Binary(BinaryOp::Add, name("x"), Box::new(Expr::Int(1)))))));
    assert!(matches!(parse("delegate { }"), Ok(Expr::Lambda(parameters, _)) if parameters.is_empty()));
    assert_eq!(parse("x => { return x; }"), Ok(Expr::Lambda(vec![(None, "x".into())], name("x"))));
assert_eq!(
    parse("x => { var y = x * 2; int z = y; z += 1; return z; }"),
    Ok(Expr::Lambda(vec![(None, "x".into())], Box::new(Expr::Block(vec![
        Statement::Local(None, vec![("y".into(), Some(Expr::Binary(BinaryOp::Multiply, name("x"), Box::new(Expr::Int(2)))))]),
        Statement::Local(Some(ty("System.Int32")), vec![("z".into(), Some(Expr::Name("y".into())))]),
        Statement::Expr(Expr::Assign(name("z"), Assignment::Compound(BinaryOp::Add), Box::new(Expr::Int(1)))),
        Statement::Return(Some(Expr::Name("z".into()))),
    ]))))
);
assert_eq!(parse("x => throw e"), Ok(Expr::Lambda(vec![(None, "x".into())], Box::new(Expr::Throw(name("e"))))));
}

#[test]
fn statements_in_a_lambda_body_are_read() {
    let name = |text: &str| Expr::Name(text.into());
    let body = |text: &str| match parse(text) {
        Ok(Expr::Lambda(_, body)) => match *body {
            Expr::Block(statements) => statements,
            Expr::Async(inner) if matches!(*inner, Expr::Block(_)) => match *inner {
                Expr::Block(statements) => statements,
                _ => unreachable!(),
            },
            other => panic!("ブロックではない: {other:?}"),
        },
        other => panic!("ラムダではない: {other:?}"),
    };
    let returned = |value: i32| Box::new(Statement::Return(Some(Expr::Int(value))));
    assert_eq!(body("x => { if (x) return 1; return 2; }"), vec![Statement::If(name("x"), returned(1), None), Statement::Return(Some(Expr::Int(2)))]);
    assert_eq!(body("x => { if (x) { return 1; } else return 2; F(); }")[0], Statement::If(name("x"), Box::new(Statement::Block(vec![*returned(1)])), Some(returned(2))));
    // 初期値の無い宣言と、複数の宣言。
    assert_eq!(body("() => { int a, b = 1; a = b; }")[0], Statement::Local(Some(TypeSyntax::named("System.Int32")), vec![("a".into(), None), ("b".into(), Some(Expr::Int(1)))]));
    assert!(matches!(&body("() => { while (ok) { if (a) break; continue; } F(); }")[0], Statement::While(_, inner) if matches!(&**inner, Statement::Block(inner) if inner[1] == Statement::Continue)));
    assert!(matches!(&body("() => { do { F(); } while (ok); G(); }")[0], Statement::Do(..)));
    assert!(matches!(&body("() => { for (int i = 0, j = 1; i < 3; i++, j--) F(i); G(); }")[0], Statement::For { init, step, .. } if init.len() == 1 && step.len() == 2));
    assert!(matches!(&body("() => { for (;;) { break; } G(); }")[0], Statement::For { init, condition: None, step, .. } if init.is_empty() && step.is_empty()));
    assert!(matches!(&body("() => { foreach (var item in items) F(item); G(); }")[0], Statement::ForEach { ty: None, names, .. } if names == &["item".to_string()]));
    assert!(matches!(&body("() => { foreach (var (k, v) in map) F(k); G(); }")[0], Statement::ForEach { names, .. } if names.len() == 2));
    assert!(matches!(&body("() => { await foreach (var item in items) F(item); G(); }")[0], Statement::ForEach { .. }));
    assert!(matches!(&body("() => { switch (x) { case 1: case 2: F(); break; default: G(); break; } H(); }")[0], Statement::Switch(_, sections) if sections.len() == 2 && sections[0].labels.len() == 2 && sections[1].labels == vec![None]));
    assert!(matches!(&body("() => { try { F(); } catch (IOException e) when (e.Message == null) { G(); } catch { throw; } finally { H(); } I(); }")[0],
        Statement::Try { catches, finally: Some(_), .. } if catches.len() == 2 && catches[0].name.as_deref() == Some("e") && catches[0].filter.is_some() && catches[1].body == vec![Statement::Rethrow]));
    assert!(matches!(&body("() => { using (var s = Open()) { F(s); } G(); }")[0], Statement::Using(resource, _) if matches!(&**resource, Statement::Local(..))));
    // using の宣言は、ブロックの残りが本体。
    assert!(matches!(&body("() => { F(); using var s = Open(); G(s); H(); }")[1], Statement::Using(_, rest) if rest.len() == 2));
    assert!(matches!(&body("() => { lock (gate) { F(); } G(); }")[0], Statement::Lock(..)));
    // await で始まる文は、宣言ではなく式。
    assert_eq!(body("async () => { await work; F(); }").len(), 2);
    assert!(parse("() => { goto end; }").is_err_and(|e| e.contains("goto")));
}

#[test]
fn reflection_type_names_are_read() {
    let ty = |name: &str, arguments: Vec<TypeSyntax>| TypeSyntax { arguments, ..TypeSyntax::named(name) };
    let asm = ", System.Private.CoreLib, Version=10.0.0.0, Culture=neutral, PublicKeyToken=7cec85d7bea7798e";
    assert_eq!(parse_reflection_type("System.Int32"), Some(ty("System.Int32", vec![])));
    assert_eq!(parse_reflection_type("System.Int32[]"), Some(TypeSyntax { array: true, ..ty("System.Int32", vec![]) }));
    assert_eq!(
        parse_reflection_type(&format!("System.Linq.IGrouping`2[[System.Int32{asm}],[System.String{asm}]]")),
        Some(ty("System.Linq.IGrouping", vec![ty("System.Int32", vec![]), ty("System.String", vec![])]))
    );
    assert_eq!(
        parse_reflection_type(&format!("System.Collections.Generic.List`1[[System.Collections.Generic.KeyValuePair`2[[System.Int32{asm}],[System.String{asm}]]{asm}]]")),
        Some(ty("System.Collections.Generic.List", vec![ty("System.Collections.Generic.KeyValuePair", vec![ty("System.Int32", vec![]), ty("System.String", vec![])])]))
    );
    assert_eq!(parse_reflection_type(&format!("System.Nullable`1[[System.Int32{asm}]]")), Some(TypeSyntax { nullable: true, ..ty("System.Int32", vec![]) }));
    assert_eq!(parse_reflection_type("Outer+Inner"), Some(ty("Outer.Inner", vec![])));
    assert_eq!(parse_reflection_type("System.Int32[,]"), None);
}

#[test]
fn await_and_lambda_modifiers_are_read() {
    let name = |text: &str| Box::new(Expr::Name(text.into()));
    let call = |target: Box<Expr>| Box::new(Expr::Call(target, vec![]));
    assert_eq!(parse("await work"), Ok(Expr::Await(name("work"))));
    assert_eq!(parse("await Run()"), Ok(Expr::Await(call(name("Run")))));
    assert_eq!(parse("(await Run()).Length"), Ok(Expr::Member(Box::new(Expr::Await(call(name("Run")))), "Length".into())));
    // await は単項演算子の強さ（`await a + b` は `(await a) + b`）。
    assert_eq!(parse("await a + b"), Ok(Expr::Binary(BinaryOp::Add, Box::new(Expr::Await(name("a"))), name("b"))));
    // await という名前の変数は、これまでどおり名前。
    assert_eq!(parse("await.Count"), Ok(Expr::Member(name("await"), "Count".into())));
    assert_eq!(parse("await + 1"), Ok(Expr::Binary(BinaryOp::Add, name("await"), Box::new(Expr::Int(1)))));
    assert_eq!(parse("await"), Ok(Expr::Name("await".into())));
    let lambda = |body: Expr| Expr::Lambda(vec![(None, "x".into())], Box::new(body));
    assert_eq!(parse("static x => x"), Ok(lambda(Expr::Name("x".into()))));
    assert_eq!(parse("async x => await x"), Ok(lambda(Expr::Async(Box::new(Expr::Await(name("x")))))));
    assert_eq!(parse("static async x => x"), Ok(lambda(Expr::Async(name("x")))));
    assert_eq!(parse("async () => { }").map(|e| matches!(e, Expr::Lambda(_, body) if matches!(*body, Expr::Async(_)))), Ok(true));
    assert_eq!(parse("async delegate (int x) { return x; }").map(|e| matches!(e, Expr::Lambda(_, body) if matches!(*body, Expr::Async(_)))), Ok(true));
    // async という名前の変数。
    assert_eq!(parse("async.Count"), Ok(Expr::Member(name("async"), "Count".into())));
}

#[test]
fn throw_expressions_are_read_only_where_csharp_allows_them() {
    let name = |text: &str| Box::new(Expr::Name(text.into()));
    assert_eq!(parse("a ?? throw e"), Ok(Expr::Coalesce(name("a"), Box::new(Expr::Throw(name("e"))))));
    assert_eq!(parse("c ? a : throw e"), Ok(Expr::Conditional(name("c"), name("a"), Box::new(Expr::Throw(name("e"))))));
    assert_eq!(parse("c ? throw e : a"), Ok(Expr::Conditional(name("c"), Box::new(Expr::Throw(name("e"))), name("a"))));
    assert!(parse("throw e").is_err_and(|e| e.contains("throw 式は")));
    assert!(parse("1 + throw e").is_err_and(|e| e.contains("throw 式は")));
        assert!(matches!(parse("() => { Run(); }"), Ok(Expr::Lambda(_, body)) if matches!(*body, Expr::Call(..))));
        assert!(parse("x => { var y = x; return y").is_err());
        assert_eq!(parse("@operator + @class"), Ok(Expr::Binary(BinaryOp::Add, name("operator"), name("class"))));
        assert_eq!(parse(r#""💬""#), Ok(Expr::Str("\u{1F4AC}".into())));
        assert_eq!(parse("Comparer<int>.Default"), Ok(Expr::Member(Box::new(Expr::Generic(name("Comparer"), vec![ty("System.Int32")])), "Default".into())));
        assert_eq!(parse("a < b.c"), Ok(Expr::Binary(BinaryOp::Less, name("a"), member(name("b"), "c"))));
    }

    #[test]
    fn csharp_14_forms_are_read() {
        assert_eq!(parse("nameof(List<>)"), Ok(Expr::Str("List".into())));
        assert_eq!(parse("nameof(Dictionary<,>)"), Ok(Expr::Str("Dictionary".into())));
        assert!(matches!(parse("holder?.Count = 9"), Ok(Expr::Assign(target, Assignment::Plain, _)) if matches!(*target, Expr::NullConditional(..))));
        assert!(matches!(parse("holder?.Count += 1"), Ok(Expr::Assign(_, Assignment::Compound(BinaryOp::Add), _))));
    }

    #[test]
    fn target_typed_new_and_collection_expressions_are_read() {
        assert_eq!(parse("new()"), Ok(Expr::TargetNew { arguments: vec![], initializer: Initializer::None }));
        assert_eq!(parse("new(1) { X = 2 }"), Ok(Expr::TargetNew { arguments: vec![Expr::Int(1)], initializer: Initializer::Members(vec![("X".into(), Expr::Int(2))]) }));
        assert_eq!(parse("[1, ..xs]"), Ok(Expr::Collection { arguments: vec![], items: vec![(false, Expr::Int(1)), (true, Expr::Name("xs".into()))] }));
        assert_eq!(parse("[with(capacity: 4), 1]"), Ok(Expr::Collection { arguments: vec![Expr::Named("capacity".into(), Box::new(Expr::Int(4)))], items: vec![(false, Expr::Int(1))] }));
        assert_eq!(parse("[]"), Ok(Expr::Collection { arguments: vec![], items: vec![] }));
        assert_eq!(parse("[with]"), Ok(Expr::Collection { arguments: vec![], items: vec![(false, Expr::Name("with".into()))] }));
    }

#[test]
fn names_are_found_wherever_they_are_written() {
    let uses = |text: &str, name: &str| parse(text).unwrap().mentions(&[name.to_string()]);
    assert!(uses("a + b", "b"));
    assert!(uses("f(x => x + n)", "n"), "ラムダ式の本体");
    assert!(uses("x => { if (flag) { return y; } return 0; }", "y"), "文の中");
    assert!(uses("x is > limit", "limit"), "パターンの中");
    assert!(uses("new Point { X = dx }", "dx"), "初期化子の中");
    assert!(uses("$\"{count} 個\"", "count"), "補間文字列の穴の中");
    assert!(uses("from n in xs let m = n select m + k", "k"), "クエリ式の中");
    assert!(!uses("a.b + \"b\"", "b"), "メンバーの名前と文字列は、名前の参照ではない");
    assert!(!uses("typeof(b)", "b"), "型の名前は、名前の参照ではない");
}
