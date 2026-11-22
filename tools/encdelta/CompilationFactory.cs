// 実行中のモジュールと、その Portable PDB から、ビルドしたときと同じコンパイルを作り直す。
//
// PDB には、コンパイラが「同じビルドをやり直すための情報」を書いている (Roslyn の BuildValidator と同じ材料):
// - CompilationOptions (モジュールの CDI): 言語、言語の版、define、出力の種類、platform、最適化、checked、unsafe、nullable。
// - CompilationMetadataReferences (モジュールの CDI): 参照ごとの、ファイルの名前と MVID。
// - Document と、埋め込まれたソース (ソース生成器の出力など)。
// プロジェクトも MSBuild も要らず、どの作り方でビルドしたアセンブリでも、ビルドしたときと同じ形になる。
using System.Collections.Immutable;
using System.IO.Compression;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Text;

namespace Netdbg.EncDelta;

/// コンパイルに使ったソース 1 つ。
sealed record SourceDocument(string Path, SourceText Text, bool Embedded);

sealed class RebuiltCompilation
{
    public required CSharpCompilation Compilation { get; init; }
    /// ビルドのあとで書き換えられていたソース (PDB のハッシュと合わない)。基準のソースが分からないので、変更を当てられない。
    public required ImmutableHashSet<string> StaleDocuments { get; init; }
    public required CSharpParseOptions ParseOptions { get; init; }
    public required ImmutableArray<SourceDocument> Documents { get; init; }
    public required SourceHashAlgorithm ChecksumAlgorithm { get; init; }
}

static class CompilationFactory
{
    static readonly Guid CompilationOptionsKind = new("B5FEEC05-8CD0-4A83-96DA-466284BB4BD8");
    static readonly Guid MetadataReferencesKind = new("7E4D4708-096E-4C5C-AEDA-CB10BA6A740D");
    static readonly Guid EmbeddedSourceKind = new("0E8A571B-6926-466E-B4AD-8AB04611F5FE");
    static readonly Guid CSharpLanguage = new("3F5162F8-07C6-11D3-9053-00C04FA302A1");
    static readonly Guid Sha1 = new("FF1816EC-AA5E-4D10-87F7-6F4963833460");

    public static RebuiltCompilation Rebuild(PEReader module, MetadataReader pdb, string modulePath, IReadOnlyDictionary<string, byte[]> sources)
    {
        var options = ReadOptions(pdb);
        if (options.GetValueOrDefault("language") is { } language && language != "C#")
            throw new InvalidOperationException($"{language} のモジュールは、Edit and Continue に対応していません (C# だけです)。");
        if (!options.ContainsKey("language-version"))
            throw new InvalidOperationException("PDB にコンパイルの情報がありません。新しい C# コンパイラ (.NET SDK) でビルドし直してください。");

        var parseOptions = new CSharpParseOptions(
            languageVersion: LanguageVersionFacts.TryParse(options["language-version"], out var version) ? version : LanguageVersion.Default,
            documentationMode: DocumentationMode.None,
            preprocessorSymbols: options.GetValueOrDefault("define")?.Split(',', StringSplitOptions.RemoveEmptyEntries));

        var metadata = module.GetMetadataReader();
        var outputKind = Enum.TryParse<OutputKind>(options.GetValueOrDefault("output-kind"), ignoreCase: true, out var kind) ? kind : OutputKind.DynamicallyLinkedLibrary;
        var compilationOptions = new CSharpCompilationOptions(outputKind)
            .WithModuleName(metadata.GetString(metadata.GetModuleDefinition().Name))
            .WithPlatform(Enum.TryParse<Platform>(options.GetValueOrDefault("platform"), ignoreCase: true, out var platform) ? platform : Platform.AnyCpu)
            .WithOptimizationLevel(options.GetValueOrDefault("optimization")?.StartsWith("release", StringComparison.OrdinalIgnoreCase) == true ? OptimizationLevel.Release : OptimizationLevel.Debug)
            .WithOverflowChecks(options.GetValueOrDefault("checked") == "True")
            .WithAllowUnsafe(options.GetValueOrDefault("unsafe") == "True")
            .WithNullableContextOptions(Enum.TryParse<NullableContextOptions>(options.GetValueOrDefault("nullable"), ignoreCase: true, out var nullable) ? nullable : NullableContextOptions.Disable)
            .WithDeterministic(true);

        var checksum = pdb.Documents.Select(h => pdb.GetGuid(pdb.GetDocument(h).HashAlgorithm)).FirstOrDefault() == Sha1 ? SourceHashAlgorithm.Sha1 : SourceHashAlgorithm.Sha256;
        var documents = ReadDocuments(pdb, checksum, sources, out var stale);
        var trees = documents.Select(d => CSharpSyntaxTree.ParseText(d.Text, parseOptions, d.Path)).ToList();
        var references = ResolveReferences(ReadReferences(pdb), Path.GetDirectoryName(Path.GetFullPath(modulePath))!);
        var assemblyName = metadata.IsAssembly ? metadata.GetString(metadata.GetAssemblyDefinition().Name) : null;
        var compilation = CSharpCompilation.Create(assemblyName, trees, references, compilationOptions);
        return new RebuiltCompilation { Compilation = compilation, ParseOptions = parseOptions, Documents = documents, ChecksumAlgorithm = checksum, StaleDocuments = stale };
    }


    /// モジュールの CDI のうち、種類が `kind` のものの中身。
    static BlobReader? ModuleInformation(MetadataReader pdb, Guid kind)
    {
        foreach (var handle in pdb.GetCustomDebugInformation(EntityHandle.ModuleDefinition))
        {
            var information = pdb.GetCustomDebugInformation(handle);
            if (pdb.GetGuid(information.Kind) == kind) return pdb.GetBlobReader(information.Value);
        }
        return null;
    }

    /// CompilationOptions: 名前と値を、終わりの 0 で区切って交互に並べたもの (UTF-8)。
    static Dictionary<string, string> ReadOptions(MetadataReader pdb)
    {
        var options = new Dictionary<string, string>(StringComparer.Ordinal);
        if (ModuleInformation(pdb, CompilationOptionsKind) is not { } reader) return options;
        var bytes = reader.ReadBytes(reader.RemainingBytes);
        var parts = Encoding.UTF8.GetString(bytes).Split('\0');
        for (var i = 0; i + 1 < parts.Length; i += 2) options[parts[i]] = parts[i + 1];
        return options;
    }

    /// 参照 1 つ (CompilationMetadataReferences の 1 件)。
    sealed record ReferenceInfo(string Name, ImmutableArray<string> Aliases, bool EmbedInteropTypes, bool IsAssembly, Guid Mvid);

    /// CompilationMetadataReferences: 名前 (0 終わり)、別名 (0 終わり、カンマ区切り)、種類 (1 バイト: 1 = アセンブリ、
    /// 2 = 相互運用の型を埋め込む)、日時印 (4)、大きさ (4)、MVID (16) の繰り返し。
    static List<ReferenceInfo> ReadReferences(MetadataReader pdb)
    {
        var references = new List<ReferenceInfo>();
        if (ModuleInformation(pdb, MetadataReferencesKind) is not { } blob) return references;
        var reader = blob;
        while (reader.RemainingBytes > 0)
        {
            var name = ReadTerminated(ref reader);
            var aliases = ReadTerminated(ref reader);
            var kind = reader.ReadByte();
            reader.ReadInt32();
            reader.ReadInt32();
            var mvid = reader.ReadGuid();
            references.Add(new ReferenceInfo(name, aliases.Length == 0 ? ImmutableArray<string>.Empty : aliases.Split(',').ToImmutableArray(), (kind & 2) != 0, (kind & 1) != 0, mvid));
        }
        return references;
    }

    static string ReadTerminated(ref BlobReader reader)
    {
        var bytes = new List<byte>();
        byte b;
        while ((b = reader.ReadByte()) != 0) bytes.Add(b);
        return Encoding.UTF8.GetString(bytes.ToArray());
    }

    /// 参照を、名前と MVID でファイルから探す。見つからない参照があれば、名前を挙げて断る。
    static List<MetadataReference> ResolveReferences(List<ReferenceInfo> references, string moduleDirectory)
    {
        var resolver = new ReferenceLocator(moduleDirectory);
        var resolved = new List<MetadataReference>();
        var missing = new List<string>();
        foreach (var reference in references)
        {
            var path = resolver.Find(reference.Name, reference.Mvid);
            if (path is null)
            {
                missing.Add(reference.Name);
                continue;
            }
            var properties = new MetadataReferenceProperties(reference.IsAssembly ? MetadataImageKind.Assembly : MetadataImageKind.Module, reference.Aliases, reference.EmbedInteropTypes);
            resolved.Add(MetadataReference.CreateFromImage(File.ReadAllBytes(path), properties, filePath: path));
        }
        if (missing.Count > 0)
            throw new InvalidOperationException($"ビルドに使った参照が見つかりません: {string.Join(", ", missing.Take(5))}{(missing.Count > 5 ? " ほか" : "")}。ビルドしたときの参照アセンブリ (SDK、NuGet のパッケージ) が、この PC にある必要があります。");
        return resolved;
    }

    /// PDB の Document のうち、C# のソース。ディスクにあるものはディスクから、PDB に埋め込まれたもの (ソース生成器の出力など)
    /// は PDB から読む。
    /// `sources` は、デバッグを始めたときに写し取ったソース (あれば、ディスクより先に使う)。
    static ImmutableArray<SourceDocument> ReadDocuments(MetadataReader pdb, SourceHashAlgorithm checksum, IReadOnlyDictionary<string, byte[]> sources, out ImmutableHashSet<string> stale)
    {
        var documents = ImmutableArray.CreateBuilder<SourceDocument>();
        var changed = ImmutableHashSet.CreateBuilder<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var handle in pdb.Documents)
        {
            var document = pdb.GetDocument(handle);
            if (pdb.GetGuid(document.Language) != CSharpLanguage) continue;
            var path = pdb.GetString(document.Name);
            var embedded = EmbeddedSource(pdb, handle);
            if (embedded is not null)
            {
                documents.Add(new SourceDocument(path, SourceText.From(embedded, embedded.Length, encoding: null, checksumAlgorithm: checksum, throwIfBinaryDetected: false, canBeEmbedded: true), true));
            }
            else if (sources.TryGetValue(path, out var snapshot) || File.Exists(path))
            {
                var text = snapshot is not null ? FromBytes(snapshot, checksum) : ReadText(path, checksum);
                var hash = pdb.GetBlobBytes(document.Hash);
                if (hash.Length > 0 && !text.GetChecksum().AsSpan().SequenceEqual(hash)) changed.Add(path);
                documents.Add(new SourceDocument(path, text, false));
            }
            else
            {
                throw new InvalidOperationException($"ビルドに使ったソースが見つかりません: {path}");
            }
        }
        stale = changed.ToImmutable();
        return documents.ToImmutable();
    }

    public static SourceText ReadText(string path, SourceHashAlgorithm checksum) => FromBytes(File.ReadAllBytes(path), checksum);

    static SourceText FromBytes(byte[] bytes, SourceHashAlgorithm checksum) =>
        SourceText.From(bytes, bytes.Length, encoding: null, checksumAlgorithm: checksum, throwIfBinaryDetected: false, canBeEmbedded: true);

    /// 埋め込まれたソース: 先頭の 4 バイトが 0 ならそのまま、正なら Deflate で圧縮した中身の、展開後の大きさ。
    static byte[]? EmbeddedSource(MetadataReader pdb, DocumentHandle document)
    {
        foreach (var handle in pdb.GetCustomDebugInformation(document))
        {
            var information = pdb.GetCustomDebugInformation(handle);
            if (pdb.GetGuid(information.Kind) != EmbeddedSourceKind) continue;
            var reader = pdb.GetBlobReader(information.Value);
            var format = reader.ReadInt32();
            var bytes = reader.ReadBytes(reader.RemainingBytes);
            if (format == 0) return bytes;
            using var inflate = new DeflateStream(new MemoryStream(bytes), CompressionMode.Decompress);
            var result = new byte[format];
            inflate.ReadExactly(result);
            return result;
        }
        return null;
    }
}

/// 参照アセンブリを、ファイルの名前と MVID で探す。探す場所: モジュールのフォルダー (プロジェクトの参照、NuGet の実装)、
/// .NET の参照パック (dotnet\packs)、.NET Framework の参照アセンブリ、NuGet のキャッシュ。
sealed class ReferenceLocator(string moduleDirectory)
{
    readonly Dictionary<string, Guid?> mvids = new(StringComparer.OrdinalIgnoreCase);
    List<string>? packFiles;

    public string? Find(string name, Guid mvid)
    {
        foreach (var candidate in Candidates(name))
        {
            if (Mvid(candidate) == mvid) return candidate;
        }
        return null;
    }

    IEnumerable<string> Candidates(string name)
    {
        yield return Path.Combine(moduleDirectory, name);
        foreach (var file in PackFiles().Where(f => string.Equals(Path.GetFileName(f), name, StringComparison.OrdinalIgnoreCase))) yield return file;
        var packages = Environment.GetEnvironmentVariable("NUGET_PACKAGES") ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".nuget", "packages");
        var id = Path.GetFileNameWithoutExtension(name).ToLowerInvariant();
        foreach (var directory in new[] { Path.Combine(packages, id) }.Concat(SafeDirectories(packages, "microsoft.netframework.referenceassemblies.*")))
        {
            if (!Directory.Exists(directory)) continue;
            foreach (var file in SafeFiles(directory, name)) yield return file;
        }
    }

    /// .NET の参照パックと .NET Framework の参照アセンブリにあるファイル (一度だけ並べる)。
    List<string> PackFiles()
    {
        if (packFiles is not null) return packFiles;
        var roots = new List<string>();
        foreach (var dotnet in DotnetRoots()) roots.AddRange(SafeDirectories(Path.Combine(dotnet, "packs"), "*.Ref"));
        var programFiles86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
        roots.Add(Path.Combine(programFiles86, "Reference Assemblies", "Microsoft", "Framework", ".NETFramework"));
        packFiles = roots.Where(Directory.Exists).SelectMany(root => SafeFiles(root, "*.dll")).ToList();
        return packFiles;
    }

    public static IEnumerable<string> DotnetRoots()
    {
        var runtime = Path.GetDirectoryName(typeof(object).Assembly.Location);
        if (runtime is not null) yield return Path.GetFullPath(Path.Combine(runtime, "..", "..", ".."));
        foreach (var variable in new[] { "DOTNET_ROOT", "DOTNET_ROOT(x86)" })
        {
            if (Environment.GetEnvironmentVariable(variable) is { Length: > 0 } root) yield return root;
        }
        yield return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "dotnet");
        yield return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86), "dotnet");
    }

    static IEnumerable<string> SafeDirectories(string root, string pattern)
    {
        try { return Directory.Exists(root) ? Directory.GetDirectories(root, pattern) : []; }
        catch (IOException) { return []; }
        catch (UnauthorizedAccessException) { return []; }
    }

    static IEnumerable<string> SafeFiles(string root, string pattern)
    {
        try { return Directory.GetFiles(root, pattern, SearchOption.AllDirectories); }
        catch (IOException) { return []; }
        catch (UnauthorizedAccessException) { return []; }
    }

    Guid? Mvid(string path)
    {
        if (mvids.TryGetValue(path, out var known)) return known;
        Guid? mvid = null;
        try
        {
            if (File.Exists(path))
            {
                using var reader = new PEReader(File.OpenRead(path));
                if (reader.HasMetadata)
                {
                    var metadata = reader.GetMetadataReader();
                    mvid = metadata.GetGuid(metadata.GetModuleDefinition().Mvid);
                }
            }
        }
        catch (BadImageFormatException) { }
        catch (IOException) { }
        mvids[path] = mvid;
        return mvid;
    }
}
