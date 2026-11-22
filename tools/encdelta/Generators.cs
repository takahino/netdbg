// ソース生成器を走らせ直す。書き換えたソースから生成器の出力が変わる (System.Text.Json のソース生成で、シリアル化する型に
// プロパティを足したなど) とき、PDB に埋め込まれた古い出力のままだと、当てたコードが静かに食い違う。
//
// PDB には、どの生成器を使ったかは記録されていない。生成されたソースのパスが `<出力の基準>\<生成器のアセンブリ名>\<生成器の
// 型>\<ヒント名>` なので、アセンブリ名から生成器を探す (SDK の参照パックの analyzers、project.assets.json の NuGet の
// パッケージ、プロジェクトのファイルにある生成器のプロジェクトの参照、モジュールのフォルダー)。見つけた生成器は、ビルドしたときのコンパイルで走らせ、埋め込まれた出力と同じになることを
// 確かめてから使う (版の違う生成器や、渡せない入力 (追加のファイルなど) を使う生成器を、取り違えないため)。
using System.Collections.Immutable;
using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;
using System.Xml.Linq;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Diagnostics;
using Microsoft.CodeAnalysis.Text;

namespace Netdbg.EncDelta;

/// 生成器のアセンブリ 1 つ分の、生成されたソースのまとまり。
sealed record GeneratedGroup(string BaseDirectory, string AssemblyName, ImmutableArray<SourceDocument> Documents)
{
    public string Folder => Path.Combine(BaseDirectory, AssemblyName);

    public bool Contains(string path) => path.StartsWith(Folder + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
}

/// 確かめられた生成器のまとまり 1 つ。ドライバーを持ち続け、インクリメンタル生成器の途中の結果を使い回す。
sealed class VerifiedGenerator(GeneratedGroup group, GeneratorDriver driver)
{
    public GeneratedGroup Group { get; } = group;
    public GeneratorDriver Driver { get; set; } = driver;
}

sealed class GeneratorHost
{
    readonly List<VerifiedGenerator> verified;
    readonly CSharpParseOptions parseOptions;

    /// 走らせ直せない生成器 (見つからない、または同じ出力にならない)。生成されたコードは古いままになる。
    public ImmutableArray<string> Unverified { get; }

    GeneratorHost(CSharpParseOptions parseOptions, ImmutableArray<string> unverified, List<VerifiedGenerator> verified)
    {
        this.parseOptions = parseOptions;
        Unverified = unverified;
        this.verified = verified;
    }

    /// 生成されたソースの、走らせ直す生成器の出力か (ソースの比べ直しから外す)。
    public bool Owns(string path) => verified.Any(v => v.Group.Contains(path));

    /// `documents` のうち生成されたソースを、生成器のアセンブリごとにまとめ、生成器を探して確かめる。
    /// `compilation` はビルドしたときのコンパイル (生成されたソースを含む)。
    public static GeneratorHost Create(CSharpCompilation compilation, CSharpParseOptions parseOptions, ImmutableArray<SourceDocument> documents, IEnumerable<string> referencePaths, string moduleDirectory)
    {
        var groups = Group(documents);
        var unverified = ImmutableArray.CreateBuilder<string>();
        var verified = new List<VerifiedGenerator>();
        var locator = new GeneratorLocator(referencePaths, moduleDirectory);
        foreach (var group in groups)
        {
            var onDisk = group.Documents.All(d => File.Exists(d.Path));
            var candidates = locator.Find(group).ToList();
            if (candidates.Count == 0)
            {
                // ディスクにあるソース (EmbedAllSources で埋め込んだ利用者のソース) は、たまたま 3 段のフォルダーにあるだけ。
                if (!onDisk) unverified.Add(group.AssemblyName);
                continue;
            }
            var options = ConfigOptions.Load(group.BaseDirectory);
            var withoutGenerated = compilation.RemoveSyntaxTrees(compilation.SyntaxTrees.Where(t => group.Contains(t.FilePath)));
            var found = false;
            foreach (var candidate in candidates)
            {
                var generators = Load(candidate);
                if (generators.IsDefaultOrEmpty) continue;
                var driver = CSharpGeneratorDriver.Create(generators, additionalTexts: [], parseOptions, options, new GeneratorDriverOptions(IncrementalGeneratorOutputKind.None, trackIncrementalGeneratorSteps: false, baseDirectory: group.BaseDirectory));
                var (ran, outputs) = Run(driver, withoutGenerated);
                if (outputs is { } trees && SameOutputs(trees, group.Documents))
                {
                    verified.Add(new VerifiedGenerator(group, ran));
                    found = true;
                    break;
                }
            }
            if (!found) unverified.Add(group.AssemblyName);
        }
        return new GeneratorHost(parseOptions, unverified.ToImmutable(), verified);
    }

    /// `compilation` (利用者のソースを書き換えたもの) で生成器を走らせ直し、生成されたソースを入れ替えたコンパイルと、
    /// 変わったソースの組 (足したものは Old が null、無くなったものは New が空のソース) を返す。
    public (CSharpCompilation Compilation, List<(SyntaxTree? Old, SyntaxTree New)> Changed) Regenerate(CSharpCompilation compilation)
    {
        var changed = new List<(SyntaxTree? Old, SyntaxTree New)>();
        foreach (var generator in verified)
        {
            var group = generator.Group;
            var current = compilation.SyntaxTrees.Where(t => group.Contains(t.FilePath)).ToList();
            var (ran, result) = Run(generator.Driver, compilation.RemoveSyntaxTrees(current));
            generator.Driver = ran;
            var outputs = result ?? throw new InvalidOperationException($"ソース生成器 {group.AssemblyName} が、書き換えたソースで失敗しました。");
            foreach (var old in current)
            {
                var output = outputs.FirstOrDefault(o => string.Equals(o.FilePath, old.FilePath, StringComparison.OrdinalIgnoreCase));
                if (output is not null && output.GetText().ContentEquals(old.GetText())) continue;
                // 無くなった出力は、空のソースにしておく (メンバーが消えたことは、比べると分かる)。
                var replacement = output ?? CSharpSyntaxTree.ParseText(SourceText.From(""), parseOptions, old.FilePath);
                compilation = compilation.ReplaceSyntaxTree(old, replacement);
                changed.Add((old, replacement));
            }
            foreach (var output in outputs.Where(o => !current.Any(t => string.Equals(t.FilePath, o.FilePath, StringComparison.OrdinalIgnoreCase))))
            {
                compilation = compilation.AddSyntaxTrees(output);
                changed.Add((null, output));
            }
        }
        return (compilation, changed);
    }

    /// 生成器を走らせ、次のドライバーと出力のソースを返す。生成器が例外を投げたら、出力は null。
    static (GeneratorDriver Driver, ImmutableArray<SyntaxTree>? Outputs) Run(GeneratorDriver driver, CSharpCompilation compilation)
    {
        driver = driver.RunGenerators(compilation);
        var result = driver.GetRunResult();
        return (driver, result.Results.Any(r => r.Exception is not null) ? null : result.GeneratedTrees);
    }

    static bool SameOutputs(ImmutableArray<SyntaxTree> outputs, ImmutableArray<SourceDocument> expected) =>
        outputs.Length == expected.Length
        && expected.All(e => outputs.Any(o => string.Equals(o.FilePath, e.Path, StringComparison.OrdinalIgnoreCase) && o.GetText().ContentEquals(e.Text)));

    /// 埋め込まれたソースを、パスの 3 段上 (`<基準>\<アセンブリ名>\<型>\<ヒント名>`) でまとめる。
    static List<GeneratedGroup> Group(ImmutableArray<SourceDocument> documents) =>
        documents
            .Where(d => d.Embedded)
            .Select(d => (Document: d, Type: Path.GetDirectoryName(d.Path)))
            .Select(x => (x.Document, Assembly: x.Type is null ? null : Path.GetDirectoryName(x.Type)))
            .Where(x => x.Assembly is not null && Path.GetDirectoryName(x.Assembly) is not null)
            .GroupBy(x => x.Assembly!, StringComparer.OrdinalIgnoreCase)
            .Select(g => new GeneratedGroup(Path.GetDirectoryName(g.Key)!, Path.GetFileName(g.Key), g.Select(x => x.Document).ToImmutableArray()))
            .ToList();

    /// 生成器のアセンブリを、自分の読み込みの文脈に読み込む。Roslyn と .NET の部品は、補助と同じものを使う。
    static ImmutableArray<ISourceGenerator> Load(string path)
    {
        try
        {
            var reference = new AnalyzerFileReference(path, new GeneratorLoader());
            return reference.GetGenerators(LanguageNames.CSharp);
        }
        catch (Exception e) when (e is IOException or BadImageFormatException or ReflectionTypeLoadException or TypeLoadException)
        {
            return default;
        }
    }
}

/// 生成器のアセンブリと、同じフォルダーにある依存を読む。Roslyn と .NET の部品は既定の文脈 (補助と同じもの) に任せる。
sealed class GeneratorLoader : IAnalyzerAssemblyLoader
{
    static readonly HashSet<string> Platform = ((string?)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES") ?? "")
        .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
        .Select(Path.GetFileNameWithoutExtension)
        .OfType<string>()
        .ToHashSet(StringComparer.OrdinalIgnoreCase);

    readonly Dictionary<string, AssemblyLoadContext> contexts = new(StringComparer.OrdinalIgnoreCase);

    public void AddDependencyLocation(string fullPath) { }

    public Assembly LoadFromPath(string fullPath)
    {
        var folder = Path.GetDirectoryName(fullPath)!;
        if (!contexts.TryGetValue(folder, out var context))
            contexts[folder] = context = new FolderContext(folder);
        return context.LoadFromAssemblyPath(fullPath);
    }

    sealed class FolderContext(string folder) : AssemblyLoadContext($"generator: {folder}")
    {
        // 言語のフォルダー (analyzers\dotnet\cs) の生成器は、言語を問わない部品 (analyzers\dotnet) にも依存する
        // (WinForms の System.Windows.Forms.Analyzers など)。MSBuild は両方をコンパイラに渡している。
        readonly string[] folders = Path.GetFileName(folder) is "cs" && Path.GetDirectoryName(folder) is { } parent ? [folder, parent] : [folder];

        protected override Assembly? Load(AssemblyName name)
        {
            if (name.Name is null || name.Name.StartsWith("Microsoft.CodeAnalysis", StringComparison.OrdinalIgnoreCase) || Platform.Contains(name.Name)) return null;
            var file = folders.Select(f => Path.Combine(f, name.Name + ".dll")).FirstOrDefault(File.Exists);
            return file is null ? null : LoadFromAssemblyPath(file);
        }
    }
}

/// 生成器のアセンブリのファイルを探す。
sealed class GeneratorLocator(IEnumerable<string> referencePaths, string moduleDirectory)
{
    // 参照を解決したパック (dotnet\packs\<名前>\<版>) を先に。ビルドに使った版の生成器がそこにある。
    readonly List<string> usedPacks = referencePaths
        .Select(PackFolder)
        .OfType<string>()
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .ToList();

    public IEnumerable<string> Find(GeneratedGroup group)
    {
        var file = group.AssemblyName + ".dll";
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        // モジュールのフォルダー: `dotnet build -o` でまとめて出力すると、参照した生成器のプロジェクトの出力もそこに入る。
        var beside = new[] { Path.Combine(moduleDirectory, file) }.Where(File.Exists);
        foreach (var candidate in FromPacks(file).Concat(FromAssets(group.BaseDirectory, file)).Concat(beside))
        {
            if (seen.Add(candidate)) yield return candidate;
        }
    }

    /// dotnet\packs\*\<版>\analyzers の下。ビルドに使ったパックを先に、ほかは新しい版から。
    IEnumerable<string> FromPacks(string file)
    {
        var packs = usedPacks.Concat(
            ReferenceLocator.DotnetRoots()
                .SelectMany(root => Directories(Path.Combine(root, "packs")))
                .SelectMany(Directories)
                .OrderByDescending(p => Version.TryParse(Path.GetFileName(p).Split('-')[0], out var v) ? v : new Version()));
        foreach (var pack in packs)
        {
            foreach (var found in Analyzers(Path.Combine(pack, "analyzers"), file)) yield return found;
        }
    }

    /// project.assets.json (obj の下) に載っている NuGet のパッケージの analyzers と、参照しているプロジェクトのビルドの出力。
    static IEnumerable<string> FromAssets(string baseDirectory, string file)
    {
        var assets = Ancestors(baseDirectory).Take(5).Select(d => Path.Combine(d, "project.assets.json")).FirstOrDefault(File.Exists);
        if (assets is null) yield break;
        JsonDocument json;
        try { json = JsonDocument.Parse(File.ReadAllBytes(assets)); }
        catch (Exception e) when (e is IOException or JsonException) { yield break; }
        using (json)
        {
            var folders = json.RootElement.TryGetProperty("packageFolders", out var pf) ? pf.EnumerateObject().Select(p => p.Name).ToList() : [];
            if (!json.RootElement.TryGetProperty("libraries", out var libraries)) yield break;
            foreach (var library in libraries.EnumerateObject())
            {
                var type = library.Value.TryGetProperty("type", out var t) ? t.GetString() : null;
                var path = library.Value.TryGetProperty("path", out var p) ? p.GetString() : null;
                if (type != "package" || path is null) continue;
                foreach (var folder in folders)
                {
                    foreach (var found in Analyzers(Path.Combine(folder, path, "analyzers"), file)) yield return found;
                }
            }
            // <ProjectReference OutputItemType="Analyzer" ReferenceOutputAssembly="false"> は復元の対象にならず、assets に
            // 載らない。プロジェクトのファイルから参照の先を読み、その bin の下を新しいものから探す。
            var projectPath = json.RootElement.TryGetProperty("project", out var project) && project.TryGetProperty("restore", out var restore) && restore.TryGetProperty("projectPath", out var pp)
                ? pp.GetString()
                : null;
            foreach (var referenced in AnalyzerProjects(projectPath))
            {
                foreach (var found in Files(Path.Combine(referenced, "bin"), file).OrderByDescending(File.GetLastWriteTimeUtc)) yield return found;
            }
        }
    }

    /// プロジェクトのファイルにある、生成器 (OutputItemType が Analyzer) のプロジェクトの参照の、フォルダー。
    static IEnumerable<string> AnalyzerProjects(string? projectPath)
    {
        if (projectPath is null || !File.Exists(projectPath)) return [];
        XDocument project;
        try { project = XDocument.Load(projectPath); }
        catch (Exception e) when (e is IOException or System.Xml.XmlException) { return []; }
        var directory = Path.GetDirectoryName(projectPath)!;
        return project.Descendants()
            .Where(e => e.Name.LocalName == "ProjectReference")
            .Where(e => ((string?)e.Attribute("OutputItemType") ?? e.Elements().FirstOrDefault(c => c.Name.LocalName == "OutputItemType")?.Value ?? "").Contains("Analyzer", StringComparison.OrdinalIgnoreCase))
            .Select(e => (string?)e.Attribute("Include"))
            .OfType<string>()
            .Select(include => Path.GetDirectoryName(Path.GetFullPath(Path.Combine(directory, include))))
            .OfType<string>()
            .ToList();
    }

    /// analyzers の下の C# の生成器 (`dotnet\cs`、`dotnet`、`dotnet\roslyn4.x\cs`)。Roslyn の版つきのフォルダーは新しい版から。
    static IEnumerable<string> Analyzers(string root, string file) =>
        Files(root, file)
            .Where(f => !f.Contains($"{Path.DirectorySeparatorChar}vb{Path.DirectorySeparatorChar}", StringComparison.OrdinalIgnoreCase))
            .OrderByDescending(f => RoslynVersion(f));

    static Version RoslynVersion(string path)
    {
        foreach (var part in path.Split(Path.DirectorySeparatorChar))
        {
            if (part.StartsWith("roslyn", StringComparison.OrdinalIgnoreCase) && Version.TryParse(part[6..], out var version)) return version;
        }
        return new Version();
    }

    /// 参照のパスから、パックのフォルダー (dotnet\packs\<名前>\<版>)。
    static string? PackFolder(string path)
    {
        var parts = path.Split(Path.DirectorySeparatorChar);
        var index = Array.FindLastIndex(parts, p => string.Equals(p, "packs", StringComparison.OrdinalIgnoreCase));
        return index >= 0 && index + 2 < parts.Length ? string.Join(Path.DirectorySeparatorChar, parts[..(index + 3)]) : null;
    }

    static IEnumerable<string> Ancestors(string directory)
    {
        for (var d = directory; d is not null; d = Path.GetDirectoryName(d)) yield return d;
    }

    static IEnumerable<string> Directories(string root)
    {
        try { return Directory.Exists(root) ? Directory.GetDirectories(root) : []; }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return []; }
    }

    static IEnumerable<string> Files(string root, string file)
    {
        try { return Directory.Exists(root) ? Directory.GetFiles(root, file, SearchOption.AllDirectories) : []; }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return []; }
    }
}

/// 生成器に渡す MSBuild の値 (`build_property.*`)。ビルドが obj に書いた `*.GeneratedMSBuildEditorConfig.editorconfig` を読む。
sealed class ConfigOptions(AnalyzerConfigSet set) : AnalyzerConfigOptionsProvider
{
    public static AnalyzerConfigOptionsProvider Load(string baseDirectory)
    {
        var configs = new[] { baseDirectory, Path.GetDirectoryName(baseDirectory) }
            .OfType<string>()
            .Where(Directory.Exists)
            .SelectMany(d => Directory.GetFiles(d, "*.GeneratedMSBuildEditorConfig.editorconfig"))
            .Take(1)
            .Select(f => AnalyzerConfig.Parse(SourceText.From(File.ReadAllText(f)), f))
            .ToList();
        return new ConfigOptions(AnalyzerConfigSet.Create(configs));
    }

    public override AnalyzerConfigOptions GlobalOptions => new Values(set.GlobalConfigOptions.AnalyzerOptions);

    public override AnalyzerConfigOptions GetOptions(SyntaxTree tree) => new Values(set.GetOptionsForSourcePath(tree.FilePath).AnalyzerOptions);

    public override AnalyzerConfigOptions GetOptions(AdditionalText textFile) => new Values(set.GetOptionsForSourcePath(textFile.Path).AnalyzerOptions);

    sealed class Values(ImmutableDictionary<string, string> values) : AnalyzerConfigOptions
    {
        public override bool TryGetValue(string key, out string value) => values.TryGetValue(key, out value!);

        public override IEnumerable<string> Keys => values.Keys;
    }
}
