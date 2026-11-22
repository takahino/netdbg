// 式の評価の試験に使う式を、機械的に用意する道具。製品には入らない。
//
//   exprgen sweep <tests/apps/Sweep/Program.cs> <出力先のフォルダー>
//       被演算子（Sweep のフィールド）とリテラルを、演算子・キャスト・型の検査と総当たりで組み合わせ、C# のコンパイラに
//       通す。通った式は Generated.cs と texts.txt に、拒まれた式は rejected.txt（式、タブ、エラーの番号）に書く。
//       式の期待値は人が書かない。対象アプリの中で、コンパイラが作ったコードが計算する。
//
//   exprgen run <エンジン> <対象アプリ> <Sweep/Program.cs> <sweep の出力先> <一覧のフォルダー> <x86|x64> [--rejected] [--update-pending] [--match=<正規表現>]
//       sweep で作った式を、エンジンに評価させて比べる（Runner.cs）。ふつうは tests/conformance/sweep.ps1 から呼ぶ。
//
//   exprgen corpus <ソースのフォルダー> <出力のファイル>
//       実際の C# のソースから、式を抜き出す。出力は 1 行に「出てきた回数、タブ、含まれる構文の種類、タブ、式」。
//       デバッガーの式の構文解析が、世の中で書かれている式をどこまで読めるかを調べるのに使う。

if (args.Length == 3 && args[0] == "sweep") return SweepCommand.Run(args[1], args[2]);
if (args.Length == 3 && args[0] == "corpus") return CorpusCommand.Run(args[1], args[2]);
if (args.Length >= 7 && args[0] == "run")
{
    var match = args.FirstOrDefault(a => a.StartsWith("--match="))?["--match=".Length..];
    return RunCommand.Run(args[1], args[2], args[3], args[4], args[5], args.Contains("--rejected"), args.Contains("--update-pending"), match, args[6]);
}
Console.Error.WriteLine("使い方:\n  exprgen sweep <Sweep/Program.cs> <出力先のフォルダー>\n  exprgen corpus <ソースのフォルダー> <出力のファイル>");
return 2;
