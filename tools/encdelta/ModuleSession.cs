// モジュール 1 つ分の Edit and Continue の状態: いまのコンパイル (最後に当てたソース) と、Roslyn の基準 (EmitBaseline)。
// 差分を作ったら、エンジンが当てられたか (commit / discard) を待ってから、次の世代に進む。
using System.Collections.Immutable;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text.Json.Nodes;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Emit;
using Microsoft.CodeAnalysis.Text;

namespace Netdbg.EncDelta;

sealed class ModuleSession
{
    static readonly Guid SlotMapKind = new("755F52A8-91C5-45BE-B4B8-209571E552BD");
    static readonly Guid LambdaMapKind = new("A643004C-0240-496F-A783-30D64F4979DE");
    static readonly Guid StateMachineMapKind = new("8B78CD68-2EDE-420B-980B-E15884B8AAA3");

    // 基準の情報は、EmitDifference が必要になったときに読むので、セッションの間ずっと持っておく。
    readonly PEReader module;
    readonly MetadataReaderProvider pdbProvider;
    readonly SourceHashAlgorithm checksum;
    readonly CSharpParseOptions parseOptions;
    readonly ImmutableHashSet<string> staleDocuments;
    readonly GeneratorHost? generators;
    readonly string? generatorProblem;
    CSharpCompilation compilation;
    EmitBaseline baseline;
    (CSharpCompilation Compilation, EmitBaseline Baseline)? pending;

    ModuleSession(PEReader module, MetadataReaderProvider pdbProvider, RebuiltCompilation rebuilt, EmitBaseline baseline, string modulePath)
    {
        this.module = module;
        this.pdbProvider = pdbProvider;
        checksum = rebuilt.ChecksumAlgorithm;
        parseOptions = rebuilt.ParseOptions;
        staleDocuments = rebuilt.StaleDocuments;
        compilation = rebuilt.Compilation;
        this.baseline = baseline;
        try
        {
            var references = compilation.References.OfType<PortableExecutableReference>().Select(r => r.FilePath).OfType<string>();
            generators = GeneratorHost.Create(compilation, parseOptions, rebuilt.Documents, references, Path.GetDirectoryName(Path.GetFullPath(modulePath))!);
            if (generators.Unverified.Length > 0)
                generatorProblem = $"ソース生成器 {string.Join("、", generators.Unverified)} を走らせ直せないので、生成されたコードはデバッグを始めたときのままです。生成器の入力 (属性、生成の対象の型など) を変えたときは、デバッグを再起動してください。";
        }
        catch (Exception e)
        {
            // 生成器を探せなくても、生成器に関わらない書き換えは当てられる。
            generators = null;
            generatorProblem = $"ソース生成器を走らせ直せないので、生成されたコードはデバッグを始めたときのままです ({e.Message})。";
        }
    }

    /// デバッグを始めたときのモジュール、PDB、ソースのバイト列 (`open`)。コンパイルを作り直すのは、最初に差分を頼まれたとき
    /// (エディット コンティニュを使わないセッションに、コンパイルの重さをかけない)。
    public sealed record Snapshot(string ModulePath, byte[] Image, byte[]? Pdb, IReadOnlyDictionary<string, byte[]> Sources);

    /// `open`: ファイルを読むだけ (開いたままにしない。利用者がビルドし直せるように)。
    public static Snapshot Capture(string modulePath, string? pdbPath, IReadOnlyDictionary<string, byte[]> sources)
    {
        var pdb = pdbPath is null ? null : File.ReadAllBytes(pdbPath);
        if (pdb is not null && pdb.AsSpan().StartsWith("Microsoft C/C++ MSF"u8))
            throw new InvalidOperationException("Windows PDB では Edit and Continue を使えません。プロジェクトに <DebugType>portable</DebugType> を指定してビルドし直してください。");
        return new Snapshot(modulePath, File.ReadAllBytes(modulePath), pdb, sources);
    }

    public static ModuleSession Open(Snapshot snapshot)
    {
        var (modulePath, image, pdbBytes, sources) = (snapshot.ModulePath, snapshot.Image, snapshot.Pdb, snapshot.Sources);
        var module = new PEReader(image.ToImmutableArray());
        MetadataReaderProvider pdbProvider;
        if (pdbBytes is null)
        {
            var embedded = module.ReadDebugDirectory().FirstOrDefault(e => e.Type == DebugDirectoryEntryType.EmbeddedPortablePdb);
            if (embedded.DataSize == 0)
                throw new InvalidOperationException("このモジュールの PDB が見つかりません。");
            pdbProvider = module.ReadEmbeddedPortablePdbDebugDirectoryData(embedded);
        }
        else
        {
            pdbProvider = MetadataReaderProvider.FromPortablePdbImage(pdbBytes.ToImmutableArray());
        }
        var pdb = pdbProvider.GetMetadataReader();
        var rebuilt = CompilationFactory.Rebuild(module, pdb, modulePath, sources);
        var errors = rebuilt.Compilation.GetDiagnostics().Where(d => d.Severity == DiagnosticSeverity.Error).ToList();
        if (errors.Count > 0)
            throw new InvalidOperationException($"ビルドしたときのコンパイルを作り直せませんでした ({errors[0]})。ソースがビルドのあとで書き換えられていないか確かめ、ビルドし直してください。");
        var metadata = module.GetMetadataReader();

        EditAndContinueMethodDebugInformation DebugInformation(MethodDefinitionHandle method)
        {
            ImmutableArray<byte> slots = default, lambdas = default, states = default;
            foreach (var handle in pdb.GetCustomDebugInformation(method))
            {
                var information = pdb.GetCustomDebugInformation(handle);
                var kind = pdb.GetGuid(information.Kind);
                var blob = pdb.GetBlobContent(information.Value);
                if (kind == SlotMapKind) slots = blob;
                else if (kind == LambdaMapKind) lambdas = blob;
                else if (kind == StateMachineMapKind) states = blob;
            }
            return EditAndContinueMethodDebugInformation.Create(slots, lambdas, states);
        }

        StandaloneSignatureHandle LocalSignature(MethodDefinitionHandle method)
        {
            var rva = metadata.GetMethodDefinition(method).RelativeVirtualAddress;
            return rva == 0 ? default : module.GetMethodBody(rva).LocalSignature;
        }

        var baseline = EmitBaseline.CreateInitialBaseline(rebuilt.Compilation, ModuleMetadata.CreateFromImage(image), DebugInformation, LocalSignature, hasPortableDebugInformation: true);
        return new ModuleSession(module, pdbProvider, rebuilt, baseline, modulePath);
    }

    /// ソースを読み直し、前に当てたソースと比べて差分を作る。`files` が無ければ、すべてのソースを読み直す。
    public JsonObject Update(IReadOnlyList<string>? files, ISet<string> capabilities)
    {
        pending = null;
        var wanted = files?.Select(Path.GetFullPath).ToHashSet(StringComparer.OrdinalIgnoreCase);
        var changed = new List<(SyntaxTree? Old, SyntaxTree New)>();
        var next = compilation;
        foreach (var tree in compilation.SyntaxTrees)
        {
            var path = tree.FilePath;
            // PDB に埋め込まれたソース (ソース生成器の出力など) は読み直さない。生成器の出力は、下で走らせ直して作る。
            if (!File.Exists(path) || generators?.Owns(path) == true) continue;
            if (wanted is not null && !wanted.Contains(Path.GetFullPath(path))) continue;
            var text = CompilationFactory.ReadText(path, checksum);
            if (text.ContentEquals(tree.GetText())) continue;
            var updated = CSharpSyntaxTree.ParseText(text, parseOptions, path);
            next = next.ReplaceSyntaxTree(tree, updated);
            changed.Add((tree, updated));
        }
        if (changed.Count == 0) return new JsonObject { ["status"] = "noChanges" };
        var stale = changed.Select(c => c.Old!.FilePath).Where(staleDocuments.Contains).ToList();
        if (stale.Count > 0)
            return Reply.Failed($"{Path.GetFileName(stale[0])} は、ビルドのあとで (デバッグを始める前に) 書き換えられていたので、変更を当てられません。ビルドし直して、デバッグを再起動してください。");

        // 書き換えたソースで生成器を走らせ直す (コンパイルのエラーを見る前に。生成されるメンバーを使う書き換えがある)。
        if (generators is not null)
        {
            var (regenerated, generated) = generators.Regenerate(next);
            next = regenerated;
            changed.AddRange(generated);
        }

        var errors = next.GetDiagnostics().Where(d => d.Severity == DiagnosticSeverity.Error).Take(20).ToList();
        if (errors.Count > 0)
            return new JsonObject { ["status"] = "errors", ["diagnostics"] = Problems(errors) };

        var analysis = EditAnalyzer.Analyze(compilation, next, changed, capabilities);
        if (analysis.RudeEdits.Count > 0)
        {
            // 生成器の出力の中の場所は、利用者が書いたものではないので、そう添える。
            foreach (var problem in analysis.RudeEdits.OfType<JsonObject>())
            {
                if ((string?)problem["file"] is { } file && generators?.Owns(file) == true)
                    problem["message"] = $"{(string?)problem["message"]} (ソース生成器が、書き換えたソースから生成し直したコード {Path.GetFileName(file)} の中です)";
            }
            return new JsonObject { ["status"] = "rude", ["rudeEdits"] = analysis.RudeEdits };
        }

        var reply = new JsonObject { ["status"] = "ready", ["lineUpdates"] = analysis.LineUpdates, ["lineMaps"] = analysis.LineMaps, ["updatedMethods"] = new JsonArray() };
        if (generatorProblem is not null) reply["warnings"] = new JsonArray(generatorProblem);
        if (analysis.Edits.Count == 0)
        {
            // 行が動いただけ: ランタイムに当てるものは無い。
            pending = (next, baseline);
            return reply;
        }
        using var metadataStream = new MemoryStream();
        using var ilStream = new MemoryStream();
        using var pdbStream = new MemoryStream();
        var result = next.EmitDifference(baseline, analysis.Edits, analysis.Inserted.Contains, metadataStream, ilStream, pdbStream);
        if (!result.Success)
        {
            var problems = result.Diagnostics.Where(d => d.Severity == DiagnosticSeverity.Error).Take(20).ToList();
            return new JsonObject { ["status"] = "errors", ["diagnostics"] = Problems(problems) };
        }
        pending = (next, result.Baseline!);
        reply["delta"] = new JsonObject
        {
            ["metadata"] = Convert.ToBase64String(metadataStream.ToArray()),
            ["il"] = Convert.ToBase64String(ilStream.ToArray()),
            ["pdb"] = Convert.ToBase64String(pdbStream.ToArray()),
        };
        reply["updatedMethods"] = new JsonArray(result.UpdatedMethods.Select(h => (JsonNode)System.Reflection.Metadata.Ecma335.MetadataTokens.GetToken(h)).ToArray());
        return reply;
    }

    public void Commit()
    {
        if (pending is { } next)
        {
            compilation = next.Compilation;
            baseline = next.Baseline;
        }
        pending = null;
    }

    public void Discard() => pending = null;

    static JsonArray Problems(IEnumerable<Diagnostic> diagnostics) => new(diagnostics.Select(d =>
    {
        var span = d.Location.GetLineSpan();
        return (JsonNode)Reply.Problem(span.Path, span.StartLinePosition.Line + 1, span.StartLinePosition.Character + 1, $"{d.Id}: {d.GetMessage()}");
    }).ToArray());
}
