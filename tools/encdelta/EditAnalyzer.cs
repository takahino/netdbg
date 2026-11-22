// 書き換えたソースを前のソースと比べ、Roslyn に渡す編集 (SemanticEdit) を作る。
//
// Roslyn の Edit and Continue の判定 (Microsoft.CodeAnalysis.Features) は公開の API ではないので、ここで単純な形で行う:
// - メンバー (メソッド、コンストラクター、アクセサー、演算子、フィールド、プロパティ、イベント、型) を、型の名前、種類、
//   名前、引数の型の文字で対応づける。
// - 本体だけが変わったメソッドは Update。足したメンバーは Insert (ランタイムの能力にあるものだけ)。
// - 署名の変更、消したメンバー、フィールドの初期値、型の宣言の変更は「再起動が要る」として、場所と理由を返す。
// - 中身は同じで行だけが動いたメンバーは、行のずれ (lineUpdates) として返す (エンジンが PDB の行をずらす)。
// - Update するメソッドには、新旧の変えていない行の対応 (lineMaps) を付ける。止まっている関数の乗り換えに使う。
using System.Collections.Immutable;
using System.Text.Json.Nodes;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Emit;

namespace Netdbg.EncDelta;

sealed class EditAnalysis
{
    public List<SemanticEdit> Edits { get; } = [];
    public HashSet<ISymbol> Inserted { get; } = new(SymbolEqualityComparer.Default);
    public JsonArray RudeEdits { get; } = [];
    public JsonArray LineUpdates { get; } = [];
    public JsonArray LineMaps { get; } = [];
}

/// メンバーの宣言 1 つ。`Key` で新旧を対応づける。
sealed record Member(string Key, SyntaxNode Node, string Kind);

static class EditAnalyzer
{
    /// `documents` の Old が null なのは、ソース生成器が新しく出力したソース (メンバーはすべて足したもの)。
    public static EditAnalysis Analyze(Compilation oldCompilation, Compilation newCompilation, IReadOnlyList<(SyntaxTree? Old, SyntaxTree New)> documents, ISet<string> capabilities)
    {
        var analysis = new EditAnalysis();
        foreach (var (oldTree, newTree) in documents)
        {
            // 足したソースには古いメンバーが無いので、古い意味モデルは使わない。
            var oldModel = oldTree is null ? null! : oldCompilation.GetSemanticModel(oldTree);
            var newModel = newCompilation.GetSemanticModel(newTree);
            var oldMembers = oldTree is null ? [] : Members(oldTree.GetCompilationUnitRoot()).ToDictionary(m => m.Key);
            var newMembers = Members(newTree.GetCompilationUnitRoot()).ToDictionary(m => m.Key);
            foreach (var (key, old) in oldMembers)
            {
                if (!newMembers.ContainsKey(key) && !IsInsideRemovedType(key, oldMembers, newMembers))
                    Rude(analysis, old.Node, $"{Describe(old)} を消した、または署名を変えたので、適用できません。デバッグを再起動してください。");
            }
            foreach (var (key, added) in newMembers)
            {
                if (oldMembers.ContainsKey(key)) continue;
                if (IsInsideAddedType(key, oldMembers)) continue;
                Insert(analysis, added, newModel, capabilities);
            }
            foreach (var (key, old) in oldMembers)
            {
                if (!newMembers.TryGetValue(key, out var updated)) continue;
                Compare(analysis, old, updated, oldModel, newModel, oldCompilation, newCompilation, capabilities);
            }
        }
        return analysis;
    }

    // ---- メンバーを並べる ----

    static IEnumerable<Member> Members(CompilationUnitSyntax root)
    {
        if (root.Members.OfType<GlobalStatementSyntax>().Any())
            yield return new Member("<top-level>", root, "top-level");
        foreach (var member in MembersOf(root.Members, ""))
            yield return member;
    }

    static IEnumerable<Member> MembersOf(SyntaxList<MemberDeclarationSyntax> members, string container)
    {
        foreach (var member in members)
        {
            switch (member)
            {
                case BaseNamespaceDeclarationSyntax ns:
                    foreach (var inner in MembersOf(ns.Members, Join(container, ns.Name.ToString()))) yield return inner;
                    break;
                case BaseTypeDeclarationSyntax type:
                    var arity = type is TypeDeclarationSyntax { TypeParameterList: { } list } ? $"`{list.Parameters.Count}" : "";
                    var name = Join(container, type.Identifier.ValueText + arity);
                    yield return new Member("T:" + name, type, "type");
                    if (type is TypeDeclarationSyntax withMembers)
                        foreach (var inner in MembersOf(withMembers.Members, name)) yield return inner;
                    break;
                case DelegateDeclarationSyntax d:
                    yield return new Member("T:" + Join(container, d.Identifier.ValueText), d, "type");
                    break;
                case MethodDeclarationSyntax m:
                    yield return new Member($"M:{container}.{m.Identifier.ValueText}`{m.TypeParameterList?.Parameters.Count ?? 0}({Parameters(m.ParameterList)})", m, "method");
                    break;
                case ConstructorDeclarationSyntax c:
                    var ctor = c.Modifiers.Any(SyntaxKind.StaticKeyword) ? ".cctor" : ".ctor";
                    yield return new Member($"M:{container}.{ctor}({Parameters(c.ParameterList)})", c, "method");
                    break;
                case DestructorDeclarationSyntax f:
                    yield return new Member($"M:{container}.Finalize()", f, "method");
                    break;
                case OperatorDeclarationSyntax o:
                    yield return new Member($"M:{container}.op_{o.OperatorToken.ValueText}({Parameters(o.ParameterList)})", o, "method");
                    break;
                case ConversionOperatorDeclarationSyntax co:
                    yield return new Member($"M:{container}.op_{co.ImplicitOrExplicitKeyword.ValueText}_{co.Type}({Parameters(co.ParameterList)})", co, "method");
                    break;
                case PropertyDeclarationSyntax p:
                    yield return new Member($"P:{container}.{p.Identifier.ValueText}", p, "property");
                    break;
                case IndexerDeclarationSyntax i:
                    yield return new Member($"P:{container}.this[{Parameters(i.ParameterList)}]", i, "property");
                    break;
                case EventDeclarationSyntax e:
                    yield return new Member($"E:{container}.{e.Identifier.ValueText}", e, "event");
                    break;
                case BaseFieldDeclarationSyntax field:
                    foreach (var variable in field.Declaration.Variables)
                        yield return new Member($"F:{container}.{variable.Identifier.ValueText}", variable, field is EventFieldDeclarationSyntax ? "event field" : "field");
                    break;
            }
        }
    }

    static string Join(string container, string name) => container.Length == 0 ? name : container + "." + name;

    static string Parameters(BaseParameterListSyntax list) =>
        string.Join(",", list.Parameters.Select(p => string.Join(" ", p.Modifiers.Select(m => m.ValueText)) + (p.Type?.ToString() ?? "")));

    static bool IsInsideAddedType(string key, Dictionary<string, Member> oldMembers) =>
        ContainingTypes(key).Any(type => !oldMembers.ContainsKey("T:" + type));

    static bool IsInsideRemovedType(string key, Dictionary<string, Member> oldMembers, Dictionary<string, Member> newMembers) =>
        key[0] != 'T' && ContainingTypes(key).Any(type => !newMembers.ContainsKey("T:" + type));

    /// メンバーのキーから、それを入れている型の名前 (外側から)。
    static IEnumerable<string> ContainingTypes(string key)
    {
        if (key == "<top-level>") yield break;
        var body = key[2..];
        var paren = body.IndexOfAny(['(', '[']);
        if (paren >= 0) body = body[..paren];
        var parts = body.Split('.');
        // 最後の部分はメンバー自身 (型なら型自身なので、外側だけ)。
        for (var i = 1; i < parts.Length; i++)
            yield return string.Join(".", parts.Take(i));
    }

    static string Describe(Member member) => member.Kind switch
    {
        "type" => $"型 {Name(member)}",
        "field" or "event field" => $"フィールド {Name(member)}",
        "property" => $"プロパティ {Name(member)}",
        "event" => $"イベント {Name(member)}",
        "top-level" => "トップレベルのステートメント",
        _ => $"メソッド {Name(member)}",
    };

    static string Name(Member member)
    {
        var key = member.Key[2..];
        var paren = key.IndexOfAny(['(', '[', '`']);
        return paren >= 0 ? key[..paren] : key;
    }

    static void Rude(EditAnalysis analysis, SyntaxNode node, string message)
    {
        var span = node.GetLocation().GetLineSpan();
        analysis.RudeEdits.Add(Reply.Problem(span.Path, span.StartLinePosition.Line + 1, span.StartLinePosition.Character + 1, message));
    }

    // ---- 足したメンバー ----

    static void Insert(EditAnalysis analysis, Member added, SemanticModel model, ISet<string> capabilities)
    {
        var symbol = model.GetDeclaredSymbol(added.Node);
        if (symbol is null)
        {
            Rude(analysis, added.Node, $"{Describe(added)} を足せません。デバッグを再起動してください。");
            return;
        }
        var generic = symbol.ContainingType is { IsGenericType: true } || symbol is IMethodSymbol { IsGenericMethod: true };
        string? needed = added.Kind switch
        {
            "type" => "NewTypeDefinition",
            "field" or "event field" => symbol.IsStatic ? "AddStaticFieldToExistingType" : "AddInstanceFieldToExistingType",
            _ => generic ? "GenericAddMethodToExistingType" : "AddMethodToExistingType",
        };
        if (added.Kind == "top-level" || !capabilities.Contains(needed))
        {
            Rude(analysis, added.Node, $"{Describe(added)} を足すことは、このランタイムではできません。デバッグを再起動してください。");
            return;
        }
        // 既にある型の初期化 (静的コンストラクター) はもう終わっているので、足した静的フィールドの初期値は実行されず、
        // 既定値のままになる。黙って違う値で動かさないよう断る (定数は値が埋め込まれるので構わない)。
        var staticInitializer = symbol.IsStatic && symbol is not IFieldSymbol { IsConst: true } && added.Node switch
        {
            VariableDeclaratorSyntax variable => variable.Initializer is not null,
            PropertyDeclarationSyntax declaration => declaration.Initializer is not null,
            _ => false,
        };
        if (staticInitializer)
        {
            Rude(analysis, added.Node, $"{Describe(added)} は初期値つきの静的なメンバーです。既にある型に足しても、初期値は実行されません (型の初期化は終わっています)。デバッグを再起動してください。");
            return;
        }
        if (added.Kind == "property" && symbol is IPropertySymbol { GetMethod: { } } property && IsAutoProperty(added.Node) &&
            !capabilities.Contains(property.IsStatic ? "AddStaticFieldToExistingType" : "AddInstanceFieldToExistingType"))
        {
            Rude(analysis, added.Node, $"{Describe(added)} (自動プロパティ) を足すことは、このランタイムではできません。デバッグを再起動してください。");
            return;
        }
        analysis.Edits.Add(new SemanticEdit(SemanticEditKind.Insert, null, symbol));
        analysis.Inserted.Add(symbol);
        if (symbol is INamedTypeSymbol type)
        {
            foreach (var member in type.GetMembers()) analysis.Inserted.Add(member);
        }
        if (symbol is IPropertySymbol p)
        {
            if (p.GetMethod is { } getter) analysis.Inserted.Add(getter);
            if (p.SetMethod is { } setter) analysis.Inserted.Add(setter);
        }
    }

    static bool IsAutoProperty(SyntaxNode node) =>
        node is PropertyDeclarationSyntax { AccessorList: { } accessors, ExpressionBody: null } && accessors.Accessors.All(a => a.Body is null && a.ExpressionBody is null);

    // ---- 新旧のあるメンバー ----

    static void Compare(EditAnalysis analysis, Member old, Member updated, SemanticModel oldModel, SemanticModel newModel, Compilation oldCompilation, Compilation newCompilation, ISet<string> capabilities)
    {
        var sameText = old.Node.ToFullString() == updated.Node.ToFullString();
        var oldLine = old.Node.GetLocation().GetLineSpan().StartLinePosition.Line;
        var newLine = updated.Node.GetLocation().GetLineSpan().StartLinePosition.Line;
        if (sameText)
        {
            if (oldLine != newLine && old.Kind != "type")
                LineUpdate(analysis, old.Node, newLine - oldLine);
            return;
        }
        switch (old.Kind)
        {
            case "type":
                // 型の宣言 (修飾子、基底、型引数、属性) が同じか。メンバーは、それぞれのメンバーとして比べる。
                if (!SyntaxFactory.AreEquivalent(TypeHeader((BaseTypeDeclarationSyntax)old.Node), TypeHeader((BaseTypeDeclarationSyntax)updated.Node)))
                    Rude(analysis, updated.Node, $"{Describe(updated)} の宣言 (修飾子、基底の型、型引数、属性) を変えたので、適用できません。デバッグを再起動してください。");
                return;
            case "field":
            case "event field":
                var oldField = (VariableDeclaratorSyntax)old.Node;
                var newField = (VariableDeclaratorSyntax)updated.Node;
                if (!SyntaxFactory.AreEquivalent(oldField.Parent!.Parent!.WithoutTrivia(), newField.Parent!.Parent!.WithoutTrivia()))
                {
                    if (SyntaxFactory.AreEquivalent(oldField.Initializer, newField.Initializer) && SyntaxFactory.AreEquivalent(FieldHeader(oldField), FieldHeader(newField)))
                    {
                        if (oldLine != newLine) LineUpdate(analysis, old.Node, newLine - oldLine);
                        return;
                    }
                    Rude(analysis, updated.Node, $"{Describe(updated)} の型、修飾子、初期値を変えたので、適用できません。デバッグを再起動してください。");
                }
                else if (oldLine != newLine)
                {
                    LineUpdate(analysis, old.Node, newLine - oldLine);
                }
                return;
            case "property":
            case "event":
                // アクセサーごとに、メソッドとして比べる。宣言 (型、修飾子、アクセサーの有無) は同じであること。
                if (!SyntaxFactory.AreEquivalent(PropertyHeader(old.Node), PropertyHeader(updated.Node)))
                {
                    Rude(analysis, updated.Node, $"{Describe(updated)} の型、修飾子、アクセサーを変えたので、適用できません。デバッグを再起動してください。");
                    return;
                }
                foreach (var (oldBody, newBody) in Accessors(old.Node).Zip(Accessors(updated.Node)))
                    CompareBody(analysis, updated, oldBody, newBody, oldModel, newModel, capabilities);
                return;
            case "top-level":
                var oldEntry = oldCompilation.GetEntryPoint(default);
                var newEntry = newCompilation.GetEntryPoint(default);
                if (oldEntry is null || newEntry is null)
                {
                    Rude(analysis, updated.Node, "トップレベルのステートメントを書き換えられません。デバッグを再起動してください。");
                    return;
                }
                Update(analysis, updated, oldEntry, newEntry, old.Node, updated.Node, capabilities);
                return;
            default:
                if (!SyntaxFactory.AreEquivalent(MethodHeader(old.Node), MethodHeader(updated.Node)))
                {
                    Rude(analysis, updated.Node, $"{Describe(updated)} の署名 (修飾子、戻り値の型、引数、属性) を変えたので、適用できません。デバッグを再起動してください。");
                    return;
                }
                CompareBody(analysis, updated, old.Node, updated.Node, oldModel, newModel, capabilities);
                return;
        }
    }

    /// メソッド (かアクセサー) の本体を比べる。同じ文字で行だけが動いたなら行のずれ、違えば Update。
    static void CompareBody(EditAnalysis analysis, Member member, SyntaxNode oldNode, SyntaxNode newNode, SemanticModel oldModel, SemanticModel newModel, ISet<string> capabilities)
    {
        var oldLine = oldNode.GetLocation().GetLineSpan().StartLinePosition.Line;
        var newLine = newNode.GetLocation().GetLineSpan().StartLinePosition.Line;
        if (oldNode.ToFullString() == newNode.ToFullString())
        {
            if (oldLine != newLine) LineUpdate(analysis, oldNode, newLine - oldLine);
            return;
        }
        if (MethodOf(oldModel, oldNode) is not { } oldMethod || MethodOf(newModel, newNode) is not { } newMethod)
        {
            Rude(analysis, newNode, $"{Describe(member)} を書き換えられません。デバッグを再起動してください。");
            return;
        }
        Update(analysis, member, oldMethod, newMethod, oldNode, newNode, capabilities);
    }

    static void Update(EditAnalysis analysis, Member member, IMethodSymbol oldMethod, IMethodSymbol newMethod, SyntaxNode oldNode, SyntaxNode newNode, ISet<string> capabilities)
    {
        if ((newMethod.IsGenericMethod || newMethod.ContainingType.IsGenericType) && !capabilities.Contains("GenericUpdateMethod"))
        {
            Rude(analysis, newNode, $"{Describe(member)} はジェネリックなので、このランタイムでは書き換えられません。デバッグを再起動してください。");
            return;
        }
        // ラムダ、ローカル関数、async、イテレーターは、コンパイラが別のメソッドや型を作る。書き換えで、それらが足される
        // ことがあるので、ランタイムにその能力が要る。
        if ((HasClosuresOrStateMachine(oldNode) || HasClosuresOrStateMachine(newNode)) && !(capabilities.Contains("AddMethodToExistingType") && capabilities.Contains("NewTypeDefinition")))
        {
            Rude(analysis, newNode, $"{Describe(member)} はラムダ、ローカル関数、async、イテレーターを含むので、このランタイムでは書き換えられません。デバッグを再起動してください。");
            return;
        }
        analysis.Edits.Add(new SemanticEdit(SemanticEditKind.Update, oldMethod, newMethod, SyntaxMap(oldNode, newNode)));
        analysis.LineMaps.Add(LineMap(oldNode, newNode));
    }

    /// 宣言の節のメソッド。式の本体のプロパティ (`=> …`) は、その get アクセサー。
    static IMethodSymbol? MethodOf(SemanticModel model, SyntaxNode node) => node switch
    {
        ArrowExpressionClauseSyntax { Parent: BasePropertyDeclarationSyntax property } => (model.GetDeclaredSymbol(property) as IPropertySymbol)?.GetMethod,
        _ => model.GetDeclaredSymbol(node) as IMethodSymbol,
    };

    static bool HasClosuresOrStateMachine(SyntaxNode node) =>
        node.DescendantNodes().Any(n => n is AnonymousFunctionExpressionSyntax or LocalFunctionStatementSyntax or QueryExpressionSyntax or AwaitExpressionSyntax or YieldStatementSyntax);

    static void LineUpdate(EditAnalysis analysis, SyntaxNode oldNode, int delta)
    {
        var span = oldNode.GetLocation().GetLineSpan();
        analysis.LineUpdates.Add(new JsonObject
        {
            ["file"] = span.Path,
            ["startLine"] = span.StartLinePosition.Line + 1,
            ["endLine"] = span.EndLinePosition.Line + 1,
            ["delta"] = delta,
        });
    }

    // ---- 宣言の比べ方 (本体を除いた形) ----

    static SyntaxNode TypeHeader(BaseTypeDeclarationSyntax type) => type switch
    {
        TypeDeclarationSyntax t => t.WithMembers(default).WithOpenBraceToken(default).WithCloseBraceToken(default).WithSemicolonToken(default),
        EnumDeclarationSyntax e => e,
        _ => type,
    };

    static SyntaxNode FieldHeader(VariableDeclaratorSyntax variable) =>
        ((BaseFieldDeclarationSyntax)variable.Parent!.Parent!).WithDeclaration(((VariableDeclarationSyntax)variable.Parent).WithVariables(default));

    static SyntaxNode MethodHeader(SyntaxNode node) => node switch
    {
        BaseMethodDeclarationSyntax m => m.WithBody(null).WithExpressionBody(null).WithSemicolonToken(default),
        _ => node,
    };

    static SyntaxNode PropertyHeader(SyntaxNode node) => node switch
    {
        BasePropertyDeclarationSyntax p => p.WithAccessorList(p.AccessorList?.WithAccessors(SyntaxFactory.List(p.AccessorList.Accessors.Select(a => a.WithBody(null).WithExpressionBody(null).WithSemicolonToken(default))))) switch
        {
            PropertyDeclarationSyntax prop => prop.WithExpressionBody(null).WithInitializer(prop.Initializer).WithSemicolonToken(default),
            IndexerDeclarationSyntax indexer => indexer.WithExpressionBody(null).WithSemicolonToken(default),
            var other => other,
        },
        _ => node,
    };

    /// アクセサー (式の本体のプロパティは、get として 1 つ)。
    static IEnumerable<SyntaxNode> Accessors(SyntaxNode node) => node switch
    {
        PropertyDeclarationSyntax { ExpressionBody: { } } p => [p.ExpressionBody!],
        IndexerDeclarationSyntax { ExpressionBody: { } } i => [i.ExpressionBody!],
        BasePropertyDeclarationSyntax { AccessorList: { } list } => list.Accessors,
        _ => [],
    };

    // ---- 構文の対応 (syntaxMap) と行の対応 ----

    /// 新しい本体の節 → 古い本体の節。ローカル変数の宣言は名前と順番、ラムダや文などは種類と順番で対応づける。
    /// Roslyn は、これでローカル変数のスロットと、ラムダ・クロージャーの対応を保つ (乗り換えたフレームのローカル変数は、
    /// スロットの番号で引き継がれる)。
    static Func<SyntaxNode, SyntaxNode?> SyntaxMap(SyntaxNode oldBody, SyntaxNode newBody)
    {
        static string Key(SyntaxNode node) => node.RawKind + ":" + node switch
        {
            VariableDeclaratorSyntax v => v.Identifier.ValueText,
            SingleVariableDesignationSyntax s => s.Identifier.ValueText,
            ForEachStatementSyntax f => f.Identifier.ValueText,
            CatchDeclarationSyntax c => c.Identifier.ValueText,
            LocalFunctionStatementSyntax l => l.Identifier.ValueText,
            ParameterSyntax p => p.Identifier.ValueText,
            FromClauseSyntax q => q.Identifier.ValueText,
            LetClauseSyntax q => q.Identifier.ValueText,
            JoinClauseSyntax q => q.Identifier.ValueText,
            _ => "",
        };
        var olds = oldBody.DescendantNodesAndSelf().GroupBy(Key).ToDictionary(g => g.Key, g => g.ToList());
        var news = newBody.DescendantNodesAndSelf().GroupBy(Key).ToDictionary(g => g.Key, g => g.ToList());
        return node =>
        {
            var key = Key(node);
            if (!news.TryGetValue(key, out var list) || !olds.TryGetValue(key, out var oldList)) return null;
            var index = list.IndexOf(node);
            return index >= 0 && index < oldList.Count ? oldList[index] : null;
        };
    }

    /// 書き換えたメソッドの、新旧で同じ文字の行の対応 (最長共通部分列)。行は 1 始まり。try / catch / finally や using など、
    /// 例外の範囲を作る文の並びが変わったら、乗り換えは危ないので、対応を空にする。
    static JsonObject LineMap(SyntaxNode oldNode, SyntaxNode newNode)
    {
        var oldSpan = oldNode.GetLocation().GetLineSpan();
        var newSpan = newNode.GetLocation().GetLineSpan();
        var pairs = new JsonArray();
        if (RegionShape(oldNode) == RegionShape(newNode))
        {
            var oldLines = oldNode.SyntaxTree.GetText().Lines;
            var newLines = newNode.SyntaxTree.GetText().Lines;
            var a = Enumerable.Range(oldSpan.StartLinePosition.Line, oldSpan.EndLinePosition.Line - oldSpan.StartLinePosition.Line + 1).Select(l => oldLines[l].ToString().Trim()).ToArray();
            var b = Enumerable.Range(newSpan.StartLinePosition.Line, newSpan.EndLinePosition.Line - newSpan.StartLinePosition.Line + 1).Select(l => newLines[l].ToString().Trim()).ToArray();
            foreach (var (i, j) in LongestCommonSubsequence(a, b))
                pairs.Add(new JsonArray(oldSpan.StartLinePosition.Line + i + 1, newSpan.StartLinePosition.Line + j + 1));
        }
        return new JsonObject { ["file"] = newSpan.Path, ["oldStartLine"] = oldSpan.StartLinePosition.Line + 1, ["oldEndLine"] = oldSpan.EndLinePosition.Line + 1, ["lines"] = pairs };
    }

    static string RegionShape(SyntaxNode node) =>
        string.Join(",", node.DescendantNodes().Where(n => n is TryStatementSyntax or CatchClauseSyntax or FinallyClauseSyntax or UsingStatementSyntax or LockStatementSyntax or CommonForEachStatementSyntax or FixedStatementSyntax || n is LocalDeclarationStatementSyntax { UsingKeyword.RawKind: not 0 }).Select(n => n.Kind()));

    static List<(int, int)> LongestCommonSubsequence(string[] a, string[] b)
    {
        var table = new int[a.Length + 1, b.Length + 1];
        for (var i = a.Length - 1; i >= 0; i--)
            for (var j = b.Length - 1; j >= 0; j--)
                table[i, j] = a[i] == b[j] ? table[i + 1, j + 1] + 1 : Math.Max(table[i + 1, j], table[i, j + 1]);
        var pairs = new List<(int, int)>();
        for (int i = 0, j = 0; i < a.Length && j < b.Length;)
        {
            if (a[i] == b[j]) { pairs.Add((i, j)); i++; j++; }
            else if (table[i + 1, j] >= table[i, j + 1]) i++;
            else j++;
        }
        return pairs;
    }
}
