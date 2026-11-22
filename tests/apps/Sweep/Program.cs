// 式の総当たり試験の対象。
//
// tools/exprgen が、下のフィールド（被演算子）とリテラルを、演算子・キャスト・型の検査と機械的に組み合わせて式を作り、
// C# のコンパイラに通す。通った式は Generated.cs（target/sweep に作られる）に入り、ここで計算されて Values に並ぶ。
// 試験（tests/conformance/sweep.ps1）は、同じ式をデバッガーに評価させて、Values と比べる。
// コンパイラが拒んだ式は、デバッガーも拒むことを確かめる。
//
// 被演算子を足すときは、フィールドを足すだけでよい。型ごとに、最初に書いたフィールドが「代表」になり、
// ほかのすべての型の代表と組み合わされる。2 番目からは、同じ型どうしの組み合わせ（境界値）に使われる。

using System;

enum Tone { Light, Dark, Deep }

[Flags]
enum Flags : byte { None = 0, A = 1, B = 2, C = 4 }

// 計算の途中で例外が出た式の、結果の代わり。デバッガーの側は、評価に失敗すればよい。
sealed class Thrown
{
    public readonly string Type;

    public Thrown(Exception e)
    {
        Type = e.GetType().FullName;
    }
}

static partial class Sweep
{
    static sbyte sb = -5, sbMax = sbyte.MaxValue, sbMin = sbyte.MinValue;
    static byte by = 200, byZero = 0, byMax = byte.MaxValue;
    static short sh = -300, shMax = short.MaxValue, shMin = short.MinValue;
    static ushort us = 60000, usMax = ushort.MaxValue;
    static int i = 7, j = 3, iZero = 0, iNeg = -1, iMax = int.MaxValue, iMin = int.MinValue;
    static uint u = 4000000000, uOne = 1, uMax = uint.MaxValue;
    static long l = 5000000000, lNeg = -1, lMax = long.MaxValue, lMin = long.MinValue;
    static ulong ul = 10000000000000000000, ulOne = 1, ulMax = ulong.MaxValue;
    static char c = 'x', cZero = '\0', cMax = char.MaxValue;
    static float f = 0.5f, fNeg = -2.5f, fMax = float.MaxValue, fNaN = float.NaN, fInf = float.PositiveInfinity;
    static double d = 2.5, dNeg = -0.1, dMax = double.MaxValue, dNaN = double.NaN, dInf = double.NegativeInfinity, dTiny = double.Epsilon;
    static decimal m = 12.50m, mNeg = -0.3m, mMax = decimal.MaxValue;
    static bool yes = true, no = false;
    static string s = "héllo", empty = "", none = null;
    static object boxed = 42, boxedText = "t", nothing = null;
    static Tone tone = Tone.Dark, toneZero = Tone.Light, toneOdd = (Tone)9;
    static Flags flags = Flags.A | Flags.C, flagsNone = Flags.None;
    static int? maybe = 5, maybeNot = null;
    static bool? perhaps = true, perhapsNot = null;
    static double? dq = 1.5;
    static nint n = 5;

    public static object[] Values = new object[Count];

    static int Main()
    {
        RunAll();
        Console.WriteLine("sweep: " + Count); // @sweep
        return 0;
    }
}
