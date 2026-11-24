// exprgen calls: ラムダを渡す呼び出しの式をテンプレートから作り、C# のコンパイラに通す（使い方は Program.cs の先頭）。

using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

static class CallsCommand
{
    public static int Run(string programPath, string outputDirectory)
    {
        var program = File.ReadAllText(programPath);
        var candidates = Candidates();
        var types = new string[candidates.Count];
        var errors = Check(program, candidates, types);
        if (errors == null) return 1;

        Directory.CreateDirectory(outputDirectory);
        var accepted = candidates.Where((_, index) => errors[index] == null).ToList();
        var utf8 = new UTF8Encoding(false);
        File.WriteAllLines(Path.Combine(outputDirectory, "types.txt"), types.Where((_, index) => errors[index] == null), utf8);
        File.WriteAllText(Path.Combine(outputDirectory, "Generated.cs"), Generate(accepted), utf8);
        File.WriteAllLines(Path.Combine(outputDirectory, "texts.txt"), accepted, utf8);
        File.WriteAllLines(Path.Combine(outputDirectory, "rejected.txt"), candidates.Select((text, index) => (text, error: errors[index])).Where(x => x.error != null).Select(x => $"{x.text}\t{x.error}"), utf8);
        Console.WriteLine($"exprgen: 候補 {candidates.Count} 件、コンパイラが受け付けた式 {accepted.Count} 件、拒んだ式 {candidates.Count - accepted.Count} 件");
        return 0;
    }

    static List<string> Candidates()
    {
        var seen = new HashSet<string>();
        var list = new List<string>();
        void Add(string text)
        {
            if (seen.Add(text)) list.Add(text);
        }

        var aggregates = new[] { "Sum", "Average", "Max", "Min" };
        // 本体の型がそのまま合うオーバーロードになる式（int / long / float / double / decimal）と、null 許容。
        var numberBodies = new[] { "n => n", "n => (long)n", "n => n * 1.5f", "n => n * 0.5", "n => (decimal)n", "n => (int?)n" };
        var lineProps = new[] { "Qty", "Id", "Weight", "Factor", "Subtotal", "Tag" };
        var wordBodies = new[] { "w => w.Length", "w => w.Length * 0.5", "w => (decimal)w.Length", "w => (long)w.Length" };
        var maybeBodies = new[] { "n => n ?? 0", "n => (long)(n ?? 0)", "n => (n ?? 0) * 0.5", "n => (decimal)(n ?? 0)" };

        foreach (var method in aggregates)
        {
            foreach (var body in numberBodies) Add($"numbers.{method}({body})");
            foreach (var prop in lineProps) Add($"lines.{method}(l => l.{prop})");
            foreach (var body in wordBodies) Add($"words.{method}({body})");
            foreach (var body in maybeBodies) Add($"maybe.{method}({body})");
        }

        // 入れ子（トップレベルの結び付けと、ラムダの中の呼び出しの両方）。
        Add("lines.Where(l => l.Qty > 0).Sum(l => l.Subtotal)");
        Add("lines.Where(l => l.Qty > 0).Average(l => l.Factor)");
        Add("lines.Where(l => l.Tag > 0).Max(l => l.Weight)");
        Add("numbers.Select(n => lines.Sum(l => l.Subtotal)).First()");
        Add("numbers.Select(n => lines.Average(l => l.Factor)).First()");
        Add("words.Where(w => w.Length > 1).Sum(w => w.Length * 0.5)");
        Add("lines.Select(l => Pick.F(x => x.Subtotal)).First()");
        Add("numbers.Where(n => n > 0).Sum(n => (decimal)n)");

        // 利用者の型。Pick は int が先。OnlyInt は変換を挟む周（byte → int）も見る。
        foreach (var prop in lineProps) Add($"Pick.F(l => l.{prop})");
        Add("OnlyInt.F(l => l.Qty)");
        Add("OnlyInt.F(l => l.Tag)");

        // コンパイラが拒む式（エンジンも拒むこと）。
        Add("numbers.Sum(n => n.ToString())");
        Add("lines.Sum(l => \"x\")");
        Add("words.Average(w => w)");
        Add("Pick.F(l => \"x\")");
        Add("OnlyInt.F(l => l.Subtotal)");
        Add("OnlyInt.F(l => l.Factor)");
        Add("OnlyInt.F(l => l.Id)");
        Add("OnlyInt.F(l => l.Weight)");
        Add("numbers.Max(n => n.ToString())");
        Add("lines.Min(l => l.ToString())");

        return list;
    }

    /// 候補をまとめてコンパイルして、候補ごとに、最初のエラーの番号（無ければ null）を返す。
    static string?[]? Check(string program, List<string> candidates, string[] types)
    {
        var source = new StringBuilder("using System;\nusing System.Linq;\nstatic partial class Calls\n{\n    const int Count = 0;\n    static void RunAll() { }\n");
        const int HeaderLines = 6;
        for (var index = 0; index < candidates.Count; index++)
        {
            source.Append($"    static void C{index}() {{ var v = {candidates[index]}; }}\n");
        }
        source.Append("}\n");
        var references = ((string)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(Path.PathSeparator)
            .Where(path => Path.GetFileName(path) is var name && (name.StartsWith("System.") || name is "mscorlib.dll" or "netstandard.dll"))
            .Select(path => MetadataReference.CreateFromFile(path));
        var candidateTree = CSharpSyntaxTree.ParseText(source.ToString(), path: "Candidates.cs");
        var compilation = CSharpCompilation.Create(
            "CallsCheck",
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
        var source = new StringBuilder("// tools/exprgen が作ったファイル。手で直さない。\nusing System;\nusing System.Linq;\n\nstatic partial class Calls\n{\n");
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
