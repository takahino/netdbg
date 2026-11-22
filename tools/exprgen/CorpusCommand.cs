// exprgen corpus: 実際の C# のソースから、式を抜き出す（使い方は Program.cs の先頭）。

using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

static class CorpusCommand
{
    const int MaxLength = 300;

    public static int Run(string sourceDirectory, string outputPath)
    {
        var found = new Dictionary<string, (int Count, string Kinds)>();
        var files = 0;
        foreach (var path in Directory.EnumerateFiles(sourceDirectory, "*.cs", SearchOption.AllDirectories))
        {
            var separator = Path.DirectorySeparatorChar;
            if (path.Contains($"{separator}obj{separator}") || path.Contains($"{separator}bin{separator}") || path.EndsWith(".g.cs") || path.EndsWith(".Designer.cs")) continue;
            SyntaxNode root;
            try
            {
                root = CSharpSyntaxTree.ParseText(File.ReadAllText(path)).GetRoot();
            }
            catch (IOException e)
            {
                Console.Error.WriteLine($"読めないので飛ばします: {path}: {e.Message}");
                continue;
            }
            files++;
            foreach (var expression in root.DescendantNodes().OfType<ExpressionSyntax>().Where(e => IsWhole(e) && StandsAlone(e)))
            {
                var text = OneLine(expression);
                if (text == null) continue;
                var kinds = string.Join(',', expression.DescendantNodesAndSelf().Select(n => n.Kind().ToString()).Distinct().Order(StringComparer.Ordinal));
                found[text] = found.TryGetValue(text, out var existing) ? (existing.Count + 1, existing.Kinds) : (1, kinds);
            }
        }
        File.WriteAllLines(outputPath, found.OrderByDescending(x => x.Value.Count).ThenBy(x => x.Key, StringComparer.Ordinal).Select(x => $"{x.Value.Count}\t{x.Value.Kinds}\t{x.Key}"), new UTF8Encoding(false));
        Console.WriteLine($"exprgen: {files} ファイルから、{found.Values.Sum(x => x.Count)} 個（重複を除いて {found.Count} 種類）の式");
        return 0;
    }

    /// 文や宣言の中に、ひとまとまりで書かれた式か（ほかの式の一部ではない）。型の名前が書かれる場所は、これで除かれる。
    static bool IsWhole(ExpressionSyntax expression) => expression.Parent switch
    {
        EqualsValueClauseSyntax clause => clause.Parent is VariableDeclaratorSyntax or PropertyDeclarationSyntax,
        ArgumentSyntax or ReturnStatementSyntax or ExpressionStatementSyntax or ArrowExpressionClauseSyntax or ThrowStatementSyntax or YieldStatementSyntax or InterpolationSyntax => true,
        IfStatementSyntax statement => statement.Condition == expression,
        WhileStatementSyntax statement => statement.Condition == expression,
        DoStatementSyntax statement => statement.Condition == expression,
        ForStatementSyntax statement => statement.Condition == expression,
        SwitchStatementSyntax statement => statement.Expression == expression,
        ForEachStatementSyntax statement => statement.Expression == expression,
        LockStatementSyntax or UsingStatementSyntax => true,
        _ => false,
    };

    /// 単独で C# の式として成り立つか。次の 3 つは、書かれた場所でしか意味を持たないので、ひとまとまりの式としては数えない
    /// （ほかの式の中にあるものは、その式の一部として数える。docs/DECISIONS.md D-033）。
    ///   - `out` の引数の宣言だけ（`out int x` の `int x`）
    ///   - 配列やコレクションの初期化子だけ（`int[] a = { 1, 2 }` の `{ 1, 2 }`）
    ///   - `=> throw …` の throw だけ（単独の throw 式は、C# でも CS8115 のエラー）
    ///   - `return ref x` / `ref var y = ref x` の `ref x`（参照を返す所にだけ書ける）
    ///   - 分解の代入の左辺の一部（`(var a, (var b, var c)) = …` の `(var b, var c)`。宣言を含むタプルは、左辺にしか書けない）
    static bool StandsAlone(ExpressionSyntax expression) => expression switch
    {
        DeclarationExpressionSyntax or InitializerExpressionSyntax or ThrowExpressionSyntax or RefExpressionSyntax => false,
        TupleExpressionSyntax tuple => !tuple.Arguments.Any(a => a.Expression is DeclarationExpressionSyntax || !StandsAlone(a.Expression)),
        _ => true,
    };

    /// 式を 1 行にする。途中にコメントがある式、複数行の文字列を含む式、長すぎる式は対象にしない（null）。
    static string? OneLine(ExpressionSyntax expression)
    {
        if (expression.DescendantTrivia().Any(t => t.IsKind(SyntaxKind.SingleLineCommentTrivia) || t.IsKind(SyntaxKind.MultiLineCommentTrivia) || t.IsDirective)) return null;
        if (expression.DescendantTokens().Any(t => t.Text.Contains('\n') || t.Text.Contains('\t'))) return null;
        var text = new StringBuilder();
        foreach (var token in expression.DescendantTokens())
        {
            // 改行をまたぐ空白は、空白 1 つにする。行の中の空白は、書かれたとおり。
            if (text.Length > 0 && token.LeadingTrivia.Count > 0) text.Append(token.LeadingTrivia.ToString().Contains('\n') || text[^1] != ' ' ? " " : "");
            text.Append(token.Text);
            var trailing = token.TrailingTrivia.ToString();
            if (trailing.Length > 0) text.Append(' ');
        }
        var line = System.Text.RegularExpressions.Regex.Replace(text.ToString().Trim(), " {2,}", " ");
        return line.Length is 0 or > MaxLength ? null : line;
    }
}
