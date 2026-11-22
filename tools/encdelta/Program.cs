// Edit and Continue の差分を作る補助。エンジン (src/debugger/session/enc.rs) が起動し、標準入出力で 1 行 1 JSON の
// 要求に答える。
//
//   起動: dotnet EncDelta.dll --roslyn <SDK の Roslyn\bincore>
//   要求: {"id":0,"command":"open","module":"…\App.dll","pdb":"…\App.pdb"}  デバッグの始め (モジュールの読み込み) に、
//         そのときのソースを基準として覚える (pdb が無ければ、モジュールに埋め込まれた PDB)
//         {"id":1,"command":"update","module":"…\App.dll","files":["…\Program.cs"],"capabilities":"Baseline …"}
//         {"id":2,"command":"commit","module":"…"}   当てられたので、次の世代に進む
//         {"id":3,"command":"discard","module":"…"}  当てられなかったので、捨てる
//   応答: {"id":1,"status":"ready","delta":{"metadata":"<base64>","il":"…","pdb":"…"},"updatedMethods":[…],
//          "lineUpdates":[…],"lineMaps":[…],"rudeEdits":[…],"diagnostics":[…],"message":"…"}
//         status: ready (当てられる差分がある) / noChanges / rude (再起動が要る) / errors (コンパイルのエラー) / failed
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Netdbg.EncDelta;

static class Program
{
    static int Main(string[] args)
    {
        var roslyn = args.Length == 2 && args[0] == "--roslyn" ? args[1] : null;
        if (roslyn is null || !File.Exists(Path.Combine(roslyn, "Microsoft.CodeAnalysis.dll")))
        {
            Console.Error.WriteLine("使い方: EncDelta --roslyn <SDK の Roslyn\\bincore のフォルダー>");
            return 2;
        }
        // Roslyn を、利用者の SDK から読み込む (この補助は Roslyn を同梱しない)。言語ごとの資源 (エラーの文) は、
        // カルチャの名前のフォルダーにある。
        AssemblyLoadContext.Default.Resolving += (context, name) =>
        {
            var folder = string.IsNullOrEmpty(name.CultureName) ? roslyn : Path.Combine(roslyn, name.CultureName);
            var file = Path.Combine(folder, name.Name + ".dll");
            return File.Exists(file) ? context.LoadFromAssemblyPath(file) : null;
        };
        Console.InputEncoding = new UTF8Encoding(false);
        var output = new StreamWriter(Console.OpenStandardOutput(), new UTF8Encoding(false)) { AutoFlush = true };
        Serve(output);
        return 0;
    }

    // Roslyn の型に触れるのは、ここから先 (読み込みの解決を登録したあとで JIT されるように、別のメソッドにする)。
    [MethodImpl(MethodImplOptions.NoInlining)]
    static void Serve(StreamWriter output)
    {
        var sessions = new Dictionary<string, ModuleEntry>(StringComparer.OrdinalIgnoreCase);
        // 覚えられなかったモジュールと、その理由 (Windows PDB など)。update で、その理由をそのまま返す。
        var openFailures = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        string? line;
        while ((line = Console.In.ReadLine()) is not null)
        {
            if (line.Length == 0) continue;
            JsonObject reply;
            JsonNode? id = null;
            try
            {
                var request = JsonNode.Parse(line)!.AsObject();
                id = request["id"]?.DeepClone();
                reply = Handle(sessions, openFailures, request);
            }
            catch (Exception e)
            {
                reply = Reply.Failed(Describe(e));
            }
            reply["id"] = id;
            output.WriteLine(reply.ToJsonString());
        }
    }

    static JsonObject Handle(Dictionary<string, ModuleEntry> sessions, Dictionary<string, string> openFailures, JsonObject request)
    {
        var command = (string?)request["command"] ?? "";
        var module = (string?)request["module"] ?? throw new InvalidOperationException("module がありません。");
        switch (command)
        {
            case "open":
                // sources: デバッグを始めたときのソースの中身 (エンジンが、モジュールの読み込みの時点で読んだもの)。
                var sources = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
                foreach (var source in request["sources"]?.AsArray() ?? [])
                {
                    if ((string?)source?["path"] is { } path && (string?)source["bytes"] is { } bytes) sources[path] = Convert.FromBase64String(bytes);
                }
                try
                {
                    sessions[module] = new ModuleEntry(ModuleSession.Capture(module, (string?)request["pdb"], sources));
                    openFailures.Remove(module);
                }
                catch (Exception e)
                {
                    openFailures[module] = Describe(e);
                    throw;
                }
                return new JsonObject { ["status"] = "ok" };
            case "update":
                if (!sessions.TryGetValue(module, out var entry) && openFailures.TryGetValue(module, out var reason))
                    throw new InvalidOperationException(reason);
                if (entry is null)
                    throw new InvalidOperationException("デバッグを始めたときのソースを覚えていないので、変更を当てられません (差分を作る補助を起動できなかったか、補助が途中で終わりました)。デバッグを再起動してください。");
                var session = entry.Session ??= ModuleSession.Open(entry.Snapshot);
                var files = request["files"]?.AsArray().Select(f => (string)f!).ToList();
                var capabilities = ((string?)request["capabilities"] ?? "Baseline").Split(' ', StringSplitOptions.RemoveEmptyEntries).ToHashSet(StringComparer.Ordinal);
                return session.Update(files, capabilities);
            case "commit":
                if (sessions.TryGetValue(module, out var committed)) committed.Session?.Commit();
                return new JsonObject { ["status"] = "ok" };
            case "discard":
                if (sessions.TryGetValue(module, out var discarded)) discarded.Session?.Discard();
                return new JsonObject { ["status"] = "ok" };
            default:
                throw new InvalidOperationException($"知らない要求です: {command}");
        }
    }

    static string Describe(Exception e) => e switch
    {
        MissingMethodException or TypeLoadException or FileNotFoundException when e.Message.Contains("CodeAnalysis") =>
            $"この .NET SDK の Roslyn では差分を作れません。.NET SDK 9.0.200 以降を入れてください ({e.Message})",
        TargetInvocationException { InnerException: { } inner } => Describe(inner),
        _ => e.Message,
    };
}

/// モジュール 1 つ: デバッグを始めたときの写しと、最初の `update` で作るセッション。
sealed class ModuleEntry(ModuleSession.Snapshot snapshot)
{
    public ModuleSession.Snapshot Snapshot { get; } = snapshot;
    public ModuleSession? Session { get; set; }
}

/// 応答を組み立てる。
static class Reply
{
    public static JsonObject Failed(string message) => new() { ["status"] = "failed", ["message"] = message };

    /// 場所つきの知らせ (許されない書き換え、コンパイルのエラー)。行と列は 1 始まり。
    public static JsonObject Problem(string file, int line, int column, string message) =>
        new() { ["file"] = file, ["line"] = line, ["column"] = column, ["message"] = message };
}
