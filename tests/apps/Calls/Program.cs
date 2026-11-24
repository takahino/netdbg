// ラムダを渡す呼び出しのオーバーロード試験の対象。
//
// tools/exprgen が、下の受け手とテンプレートから式を作り、C# のコンパイラに通す。通った式は Generated.cs
// （target/calls に作られる）に入り、ここで計算されて Values に並ぶ。試験（tests/conformance/calls.ps1）は、
// 同じ式をデバッガーに評価させて、値と型を Values と比べる。コンパイラが拒んだ式は、デバッガーも拒むこと。
//
// Pick は int のオーバーロードを先に置く。本体の型がそのまま合うものを先に選ぶ規則（D-104）が無いと、
// decimal の本体が int に変換されて結び付く。OnlyInt は変換を挟む周だけでも選べることを確かめる。

using System;
using System.Linq;

sealed class Line
{
    public int Qty { get; }
    public long Id { get; }
    public float Weight { get; }
    public double Factor { get; }
    public decimal Subtotal { get; }
    public byte Tag { get; }

    public Line(int qty, long id, float weight, double factor, decimal subtotal, byte tag)
    {
        Qty = qty;
        Id = id;
        Weight = weight;
        Factor = factor;
        Subtotal = subtotal;
        Tag = tag;
    }
}

// 計算の途中で例外が出た式の、結果の代わり。デバッガーの側は、評価に失敗すればよい。
sealed class Thrown
{
    public readonly string Type;

    public Thrown(Exception e)
    {
        Type = e.GetType().FullName;
    }
}

// int を先に置く。本体が decimal のとき、変換を挟む周だけだとここに結び付く。
static class Pick
{
    public static int F(Func<Line, int> selector) => selector(Calls.Sample);
    public static long F(Func<Line, long> selector) => selector(Calls.Sample);
    public static float F(Func<Line, float> selector) => selector(Calls.Sample);
    public static double F(Func<Line, double> selector) => selector(Calls.Sample);
    public static decimal F(Func<Line, decimal> selector) => selector(Calls.Sample);
}

// Func<Line, int> だけ。byte の本体は変換を挟んで渡せる。decimal は渡せない。
static class OnlyInt
{
    public static int F(Func<Line, int> selector) => selector(Calls.Sample);
}

static partial class Calls
{
    static int[] numbers = [10, 20, 30];
    static string[] words = ["hi", "there", "x"];
    static int?[] maybe = [1, null, 3];
    static Line[] lines =
    [
        new Line(2, 100L, 1.5f, 0.5, 12.50m, 7),
        new Line(3, 200L, 2.0f, 1.0, 7.25m, 9),
    ];
    public static Line Sample = lines[0];

    public static object[] Values = new object[Count];

    static int Main()
    {
        RunAll();
        Console.WriteLine("calls: " + Count); // @calls
        return 0;
    }
}
