// exprgen sweep: 式を総当たりで組み合わせ、C# のコンパイラに通す（使い方は Program.cs の先頭）。

using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

static class SweepCommand
{
    static readonly string[] Binary = ["+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", ">>>", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "??"];
    static readonly string[] Unary = ["+", "-", "!", "~"];
    // 型の違うリテラル。0 は列挙型に暗黙に変換でき、2147483648 は uint、4294967296 は long になる。
    static readonly string[] Literals = ["0", "1", "-1", "2147483648", "4294967296", "1u", "1L", "1ul", "1.5", "1.5f", "1.5m", "'a'", "\"s\"", "true", "null"];
    static readonly string[] Types = ["sbyte", "byte", "short", "ushort", "int", "uint", "long", "ulong", "char", "float", "double", "decimal", "bool", "string", "object", "Tone", "Flags", "int?", "double?", "nint"];
    // シフトの数。型の幅を超える数と負の数は、幅で丸められる。
    static readonly string[] ShiftCounts = ["0", "1", "31", "32", "33", "63", "64", "65", "-1"];
    static readonly HashSet<string> Integral = ["sbyte", "byte", "short", "ushort", "int", "uint", "long", "ulong", "char", "nint"];

    public static int Run(string programPath, string outputDirectory)
    {
        var program = File.ReadAllText(programPath);
        var operands = ReadOperands(program);
        if (operands.Count == 0)
        {
            Console.Error.WriteLine($"{programPath} の Sweep クラスに、被演算子のフィールドが見つかりません。");
            return 1;
        }
        var candidates = Candidates(operands);
        var types = new string[candidates.Count];
        var errors = Check(program, candidates, types);
        if (errors == null) return 1;

        Directory.CreateDirectory(outputDirectory);
        var accepted = candidates.Where((_, index) => errors[index] == null).ToList();
        var utf8 = new UTF8Encoding(false);
        // 式の、コンパイル時の型。実行時の値からは分からない違い（int と int?）を比べるのに使う。
        File.WriteAllLines(Path.Combine(outputDirectory, "types.txt"), types.Where((_, index) => errors[index] == null), utf8);
        File.WriteAllText(Path.Combine(outputDirectory, "Generated.cs"), Generate(accepted), utf8);
        File.WriteAllLines(Path.Combine(outputDirectory, "texts.txt"), accepted, utf8);
        File.WriteAllLines(Path.Combine(outputDirectory, "rejected.txt"), candidates.Select((text, index) => (text, error: errors[index])).Where(x => x.error != null).Select(x => $"{x.text}\t{x.error}"), utf8);
        Console.WriteLine($"exprgen: 候補 {candidates.Count} 件、コンパイラが受け付けた式 {accepted.Count} 件、拒んだ式 {candidates.Count - accepted.Count} 件");
        return 0;
    }

    /// Sweep クラスの静的フィールド（初期値つき）を、型ごとに、書かれた順で。
    static List<(string Type, List<string> Names)> ReadOperands(string program)
    {
        var result = new List<(string Type, List<string> Names)>();
        var root = CSharpSyntaxTree.ParseText(program).GetRoot();
        var sweep = root.DescendantNodes().OfType<ClassDeclarationSyntax>().FirstOrDefault(c => c.Identifier.Text == "Sweep");
        foreach (var field in sweep?.Members.OfType<FieldDeclarationSyntax>() ?? [])
        {
            // public のフィールド（Values）は、結果の置き場。
            if (field.Modifiers.Any(SyntaxKind.PublicKeyword)) continue;
            var type = field.Declaration.Type.ToString();
            var names = field.Declaration.Variables.Select(v => v.Identifier.Text).ToList();
            var existing = result.FindIndex(x => x.Type == type);
            if (existing >= 0) result[existing].Names.AddRange(names);
            else result.Add((type, names));
        }
        return result;
    }

    static List<string> Candidates(List<(string Type, List<string> Names)> operands)
    {
        var all = operands.SelectMany(o => o.Names).Concat(Literals).ToList();
        var representatives = operands.Select(o => o.Names[0]).Concat(Literals).ToList();
        var seen = new HashSet<string>();
        var list = new List<string>();
        void Add(string text)
        {
            if (seen.Add(text)) list.Add(text);
        }
        // 単項演算子の直後に符号つきのリテラルを置くと、`--1` のように別の演算子になってしまう。
        static string Under(string operand) => operand.StartsWith('-') ? $"({operand})" : operand;

        // 値そのもの、単項演算子
        foreach (var x in all) Add(x);
        foreach (var op in Unary) foreach (var x in all) Add(op + Under(x));
        // 型の組み合わせ（代表どうし）
        foreach (var op in Binary) foreach (var x in representatives) foreach (var y in representatives) Add($"{x} {op} {y}");
        // 同じ型の中での境界値
        foreach (var (_, names) in operands) foreach (var op in Binary) foreach (var x in names) foreach (var y in names) Add($"{x} {op} {y}");
        // シフトの数
        foreach (var (type, names) in operands)
        {
            if (!Integral.Contains(type)) continue;
            foreach (var op in new[] { "<<", ">>", ">>>" }) foreach (var x in names) foreach (var count in ShiftCounts) Add($"{x} {op} {count}");
        }
        // キャスト
        foreach (var type in Types) foreach (var x in all) Add($"({type}){Under(x)}");
        // 型の検査
        foreach (var x in representatives)
        {
            foreach (var type in Types)
            {
                Add($"{x} is {type}");
                Add($"{x} as {type}");
                Add($"{x} is not {type}");
            }
            foreach (var pattern in new[] { "null", "not null", "> 0", "<= 1", "1", "not 1", "true", "\"s\"", "Tone.Dark", "'a'", "1.5" }) Add($"{x} is {pattern}");
        }
        // 条件演算子（両側の型の統一）
        foreach (var x in representatives) foreach (var y in representatives) Add($"yes ? {x} : {y}");
        foreach (var x in representatives) Add($"{x} ? 1 : 2");
        // 優先順位と結合
        foreach (var first in Binary)
        {
            foreach (var second in Binary)
            {
                Add($"i {first} j {second} 2");
                Add($"yes {first} no {second} yes");
                Add($"i {first} j {second} yes");
                Add($"yes {first} i {second} j");
                Add($"d {first} i {second} l");
            }
        }
        foreach (var unary in Unary)
        {
            foreach (var op in Binary)
            {
                Add($"{unary}i {op} j");
                Add($"i {op} {unary}j");
                Add($"{unary}yes {op} no");
                Add($"{unary}(i {op} j)");
            }
            foreach (var second in Unary) Add($"{unary}{(unary == second || second is "+" or "-" ? $"({second}i)" : $"{second}i")}");
        }
        return list;
    }

    /// 候補をまとめてコンパイルして、候補ごとに、最初のエラーの番号（無ければ null）を返す。
    static string?[]? Check(string program, List<string> candidates, string[] types)
    {
        var source = new StringBuilder("using System;\nstatic partial class Sweep\n{\n    const int Count = 0;\n    static void RunAll() { }\n");
        const int HeaderLines = 5;
        for (var index = 0; index < candidates.Count; index++)
        {
            // 評価の結果を受ける側の型が式の意味を変えないように（`yes ? 1 : null` は、受ける側が object だと通る）、var で受ける。
            source.Append($"    static void C{index}() {{ var v = {candidates[index]}; }}\n");
        }
        source.Append("}\n");
        var references = ((string)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(Path.PathSeparator)
            .Where(path => Path.GetFileName(path) is var name && (name.StartsWith("System.") || name is "mscorlib.dll" or "netstandard.dll"))
            .Select(path => MetadataReference.CreateFromFile(path));
        var candidateTree = CSharpSyntaxTree.ParseText(source.ToString(), path: "Candidates.cs");
        var compilation = CSharpCompilation.Create(
            "SweepCheck",
            [CSharpSyntaxTree.ParseText(program, path: "Program.cs"), candidateTree],
            references,
            new CSharpCompilationOptions(OutputKind.ConsoleApplication, nullableContextOptions: NullableContextOptions.Disable));
        var errors = new string?[candidates.Count];
        foreach (var diagnostic in compilation.GetDiagnostics().Where(d => d.Severity == DiagnosticSeverity.Error))
        {
            var span = diagnostic.Location.GetLineSpan();
            var index = span.StartLinePosition.Line - HeaderLines;
            if (span.Path != "Candidates.cs" || index < 0 || index >= candidates.Count)
            {
                Console.Error.WriteLine($"候補の式ではない所でコンパイルエラーになりました: {diagnostic}");
                return null;
            }
            errors[index] ??= diagnostic.Id;
        }
        var model = compilation.GetSemanticModel(candidateTree);
        foreach (var declarator in candidateTree.GetRoot().DescendantNodes().OfType<VariableDeclaratorSyntax>())
        {
            var index = declarator.GetLocation().GetLineSpan().StartLinePosition.Line - HeaderLines;
            if (index < 0 || index >= candidates.Count || declarator.Initializer == null) continue;
            types[index] = model.GetTypeInfo(declarator.Initializer.Value).Type?.ToDisplayString(SymbolDisplayFormat.MinimallyQualifiedFormat) ?? "";
        }
        return errors;
    }

    static string Generate(List<string> accepted)
    {
        const int PartSize = 250;
        var source = new StringBuilder("// tools/exprgen が作ったファイル。手で直さない。\nusing System;\n\nstatic partial class Sweep\n{\n");
        source.Append($"    public const int Count = {accepted.Count};\n\n    static void RunAll()\n    {{\n");
        for (var part = 0; part * PartSize < accepted.Count; part++) source.Append($"        Part{part}();\n");
        source.Append("    }\n");
        for (var index = 0; index < accepted.Count; index++)
        {
            if (index % PartSize == 0) source.Append($"\n    static void Part{index / PartSize}()\n    {{\n");
            source.Append($"        try {{ var v = {accepted[index]}; Values[{index}] = v; }} catch (Exception e) {{ Values[{index}] = new Thrown(e); }}\n");
            if (index % PartSize == PartSize - 1 || index == accepted.Count - 1) source.Append("    }\n");
        }
        source.Append("}\n");
        return source.ToString();
    }
}
