// 総当たり試験の実行。エンジンを起動して DAP で話し、作っておいた式を 1 つずつ評価させて、コンパイラの結果と比べる。
// PowerShell から 1 式ずつ送ると 1 往復に 10 ms ほどかかり、全部で 10 分を超えるので、ここで行う。

using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

/// エンジンとの DAP のやり取り（標準入出力、Content-Length の枠）。
sealed class DapClient : IDisposable
{
    readonly Process process;
    readonly Stream input;
    readonly Stream output;
    readonly Queue<JsonNode> events = new();
    int sequence;

    public DapClient(string engine)
    {
        process = Process.Start(new ProcessStartInfo(engine, "--interpreter=vscode")
        {
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            UseShellExecute = false,
        }) ?? throw new InvalidOperationException($"{engine} を起動できません。");
        input = process.StandardInput.BaseStream;
        output = process.StandardOutput.BaseStream;
    }

    /// 要求を送って、その応答を待つ。途中で来たイベントは、ためておく。
    public JsonNode Request(string command, JsonObject? arguments = null)
    {
        var seq = ++sequence;
        var body = Encoding.UTF8.GetBytes(new JsonObject { ["seq"] = seq, ["type"] = "request", ["command"] = command, ["arguments"] = arguments ?? new JsonObject() }.ToJsonString());
        input.Write(Encoding.ASCII.GetBytes($"Content-Length: {body.Length}\r\n\r\n"));
        input.Write(body);
        input.Flush();
        while (true)
        {
            var message = Read();
            if ((string?)message["type"] == "response" && (int?)message["request_seq"] == seq) return message;
            if ((string?)message["type"] == "event") events.Enqueue(message);
        }
    }

    /// 指定のイベントが来るまで待つ。
    public JsonNode WaitEvent(string name)
    {
        while (true)
        {
            while (events.TryDequeue(out var queued))
            {
                if ((string?)queued["event"] == name) return queued;
                if ((string?)queued["event"] is "terminated" or "exited") throw new InvalidOperationException($"{name} を待っている間に、対象が終わりました。");
            }
            var message = Read();
            if ((string?)message["type"] == "event") events.Enqueue(message);
        }
    }

    JsonNode Read()
    {
        var length = -1;
        while (true)
        {
            var line = new StringBuilder();
            while (true)
            {
                var value = output.ReadByte();
                if (value < 0) throw new EndOfStreamException("エンジンが終わりました。");
                if (value == '\n') break;
                if (value != '\r') line.Append((char)value);
            }
            if (line.Length == 0) break;
            if (line.ToString().StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase)) length = int.Parse(line.ToString()[15..].Trim());
        }
        if (length < 0) throw new InvalidDataException("Content-Length の無いメッセージです。");
        var buffer = new byte[length];
        output.ReadExactly(buffer);
        return JsonNode.Parse(buffer) ?? throw new InvalidDataException("空のメッセージです。");
    }

    public void Dispose()
    {
        try
        {
            if (!process.HasExited) process.Kill(entireProcessTree: true);
        }
        catch (InvalidOperationException)
        {
            // もう終わっている。
        }
        process.Dispose();
    }
}

static class RunCommand
{
    const string PendingHeader = "# 総当たり試験で、まだコンパイラと合わない式 (tests/conformance/sweep.ps1 -UpdatePending が作る)。合うようになったら消す。";
    const string RejectedPendingHeader = "# C# のコンパイラは拒むのに、エンジンが評価してしまう式 (tests/conformance/sweep.ps1 -Rejected -UpdatePending が作る)。拒むようになったら消す。";

    /// 戻り値は、失敗があれば 1。
    public static int Run(string engine, string app, string source, string sweep, string pendingDirectory, bool rejected, bool updatePending, string? match, string arch)
    {
        var utf8 = new UTF8Encoding(false);
        var texts = File.ReadAllLines(Path.Combine(sweep, "texts.txt"), utf8);
        var types = File.ReadAllLines(Path.Combine(sweep, "types.txt"), utf8);
        var pendingPath = Path.Combine(pendingDirectory, "sweep-pending.txt");
        var rejectedPendingPath = Path.Combine(pendingDirectory, "sweep-rejected-pending.txt");
        var pending = ReadList(pendingPath);
        var rejectedPending = ReadList(rejectedPendingPath);
        var filter = match == null ? null : new Regex(match);
        var failures = 0;

        using var dap = new DapClient(engine);
        dap.Request("initialize", new JsonObject { ["adapterID"] = "netdbg" });
        dap.Request("launch", new JsonObject { ["program"] = app });
        var line = Array.FindIndex(File.ReadAllLines(source), l => Regex.IsMatch(l, @"@sweep(?![\w-])")) + 1;
        dap.Request("setBreakpoints", new JsonObject { ["source"] = new JsonObject { ["path"] = source }, ["breakpoints"] = new JsonArray(new JsonObject { ["line"] = line }) });
        dap.Request("configurationDone");
        var thread = (int)dap.WaitEvent("stopped")["body"]!["threadId"]!;
        var frame = (int)dap.Request("stackTrace", new JsonObject { ["threadId"] = thread, ["levels"] = 1 })["body"]!["stackFrames"]![0]!["id"]!;
        JsonNode Evaluate(string expression) => dap.Request("evaluate", new JsonObject { ["expression"] = expression, ["frameId"] = frame });

        var count = int.Parse((string)Evaluate("Count")["body"]!["result"]!);
        if (count != texts.Length)
        {
            Console.Error.WriteLine($"対象アプリの式の数 ({count}) が texts.txt ({texts.Length}) と違います。./tests/apps/build.ps1 をやり直してください。");
            return 1;
        }

        // --- コンパイラが受け付けた式: 値と型が同じこと ---
        var wrong = new List<string>();
        var report = new List<string>();
        var nowMatching = new List<string>();
        var matched = 0;
        for (var index = 0; index < count; index++)
        {
            var text = texts[index];
            if (filter != null && !filter.IsMatch(text)) continue;
            var expected = Evaluate($"Values[{index}]")["body"]!;
            var actual = Evaluate(text);
            var succeeded = (bool)actual["success"]!;
            var (result, type) = succeeded ? ((string)actual["body"]!["result"]!, (string?)actual["body"]!["type"] ?? "") : ("", "");
            bool same;
            string expectedShown;
            if ((string?)expected["type"] == "Thrown")
            {
                // 計算の途中で例外が出る式。エンジンは、値を返さなければよい。
                same = !succeeded;
                expectedShown = "例外";
            }
            else
            {
                // Values は object の配列なので、int? の値は int として入っている。null 許容の値型は、コンパイル時の型と比べる。
                var expectedType = types[index].EndsWith('?') ? types[index] : (string?)expected["type"] ?? "";
                var expectedResult = (string)expected["result"]!;
                // null は、入れ物の型でしか見えないので、型は比べない。
                same = succeeded && result == expectedResult && (expectedResult == "null" || SameType(type, expectedType));
                expectedShown = $"{expectedResult} : {expectedType}";
            }
            if (same)
            {
                matched++;
                if (pending.Contains(text)) nowMatching.Add(text);
                continue;
            }
            wrong.Add(text);
            if (!pending.Contains(text)) report.Add($"{text}\t{(succeeded ? $"{result} : {type}" : $"失敗: {(string?)actual["message"]}")}\t{expectedShown}");
        }
        Console.WriteLine($"受け付けた式: {matched + wrong.Count} 件中 {matched} 件がコンパイラと一致 (未対応の一覧 {pending.Count} 件)");
        var reportPath = Path.Combine(sweep, $"mismatch-{arch}.tsv");
        File.WriteAllLines(reportPath, report, utf8);
        if (updatePending) File.WriteAllLines(pendingPath, wrong.Prepend(PendingHeader), utf8);
        else failures += Complain(report, $"一覧に無いのに合わない式 (式、エンジン、コンパイラ: {reportPath})") + Complain(nowMatching, "合うようになったのに一覧に残っている式 (-UpdatePending で作り直す)");

        // --- コンパイラが拒んだ式: エンジンも拒むこと ---
        if (rejected)
        {
            var evaluated = new List<string>();
            report = [];
            var nowRejected = new List<string>();
            var total = 0;
            foreach (var entry in File.ReadAllLines(Path.Combine(sweep, "rejected.txt"), utf8))
            {
                var parts = entry.Split('\t');
                var text = parts[0];
                if (filter != null && !filter.IsMatch(text)) continue;
                total++;
                var actual = Evaluate(text);
                if (!(bool)actual["success"]!)
                {
                    if (rejectedPending.Contains(text)) nowRejected.Add(text);
                    continue;
                }
                evaluated.Add(text);
                if (!rejectedPending.Contains(text)) report.Add($"{text}\t{(string?)actual["body"]!["result"]} : {(string?)actual["body"]!["type"]}\t{parts[1]}");
            }
            Console.WriteLine($"拒まれた式: {total} 件中 {total - evaluated.Count} 件をエンジンも拒む (未対応の一覧 {rejectedPending.Count} 件)");
            reportPath = Path.Combine(sweep, $"accepted-{arch}.tsv");
            File.WriteAllLines(reportPath, report, utf8);
            if (updatePending) File.WriteAllLines(rejectedPendingPath, evaluated.Prepend(RejectedPendingHeader), utf8);
            else failures += Complain(report, $"一覧に無いのに評価してしまう式 (式、エンジン、コンパイラのエラー: {reportPath})") + Complain(nowRejected, "拒むようになったのに一覧に残っている式 (-UpdatePending で作り直す)");
        }
        dap.Request("disconnect", new JsonObject { ["terminateDebuggee"] = true });
        if (failures == 0) Console.WriteLine("ok");
        return failures == 0 ? 0 : 1;
    }

    // C# 11 から、nint と System.IntPtr は同じ型 (別名)。エンジンは実体の型の名前で見せ、types.txt はキーワードで書く。
    static bool SameType(string actual, string expected)
    {
        static string Plain(string type) => Regex.Replace(Regex.Replace(type, @"\bnuint\b", "System.UIntPtr"), @"\bnint\b", "System.IntPtr");
        return Plain(actual) == Plain(expected);
    }

    static HashSet<string> ReadList(string path) => File.Exists(path) ? File.ReadAllLines(path, Encoding.UTF8).Where(line => line.Length > 0 && !line.StartsWith('#')).ToHashSet() : [];

    static int Complain(List<string> lines, string what)
    {
        if (lines.Count == 0) return 0;
        Console.WriteLine($"  FAIL  {what}: {lines.Count} 件");
        foreach (var line in lines.Take(10)) Console.WriteLine($"          {(line.Length > 220 ? line[..220] : line)}");
        return 1;
    }
}
