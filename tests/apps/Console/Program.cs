// スモーク試験の対象。行は `// @名前` の目印で探すので、行を足しても試験は壊れない。
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

class Program
{
    static readonly object Gate = new object();
    static readonly System.Threading.ManualResetEvent GateHeld = new System.Threading.ManualResetEvent(false);

    static void HoldGate()
    {
        for (int round = 0; round < 600; round++)
        {
            lock (Gate)
            {
                GateHeld.Set();
                System.Threading.Thread.Sleep(100);
            }
        }
    }

    // worker-a が数えた回数（wait の試験）。
    static int WorkerTicks;

    static int TakeGate()
    {
        lock (Gate)
        {
            return 42;
        }
    }

    static int Main(string[] args)
    {
        // 一時停止、強制終了、アタッチの試験用。止められるまで（最長 1 分）回り続ける。
        if (args.Length > 0 && args[0] == "wait") // @wait-check
        {
            // worker-a は数え続ける（スレッドの凍結の試験が、止まっていることを数で確かめる）。
            var worker = new System.Threading.Thread(() => { while (true) { System.Threading.Interlocked.Increment(ref WorkerTicks); System.Threading.Thread.Sleep(5); } }) { Name = "worker-a", IsBackground = true };
            worker.Start();
            Console.WriteLine("waiting");
            for (int tick = 0; tick < 600; tick++)
            {
                System.Threading.Thread.Sleep(100); // @tick
            }
            return 1;
        }
        // 評価の行き詰まりの試験用。ほかのスレッドがロックをほとんど持ち続けるので、ロックを取るメソッド（TakeGate）を
        // 評価すると、ほかのスレッドを止めたままでは終わらない。
        if (args.Length > 0 && args[0] == "gate")
        {
            var holder = new System.Threading.Thread(HoldGate) { Name = "gate-holder", IsBackground = true };
            holder.Start();
            GateHeld.WaitOne();
            var spinner = new Spinner();
            System.Threading.Thread.Sleep(100); // @gate
            GC.KeepAlive(spinner);
            return 1;
        }
        // 書き込みの見張りの試験用 (D-093)。フィールドを、ref で別のメソッドから、GC のあとに、ソースの無い .NET の中 (int.TryParse の out) から書く。
        // リフレクションの SetValue (ネイティブのコードが書く) は、x86 では対象が落ちるので使わない。
        if (args.Length > 0 && args[0] == "data-write")
        {
            var order = new WatchedOrder();
            order.Touch(); // @data-before
            WatchedOrder.SetByRef(ref order.Status, 3);
            var garbage = new List<byte[]>();
            for (int i = 0; i < 200; i++) garbage.Add(new byte[10000]);
            garbage = null;
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
            order.Status = 4;
            int.TryParse("7", out order.Status);
            return order.Status; // @data-after
        }
        // COM オブジェクトの漏れの試験用 (D-092)。COM のオブジェクトを 10 個作り、解放しないで持ち続ける。
        if (args.Length > 0 && args[0] == "com-leak")
        {
            var kept = new List<object>();
#pragma warning disable CA1416 // この試験アプリは Windows だけで動かす。
            var type = Type.GetTypeFromProgID("Scripting.FileSystemObject");
#pragma warning restore CA1416
            kept.Add(Activator.CreateInstance(type)); // @com-before
            for (int i = 0; i < 10; i++)
            {
                kept.Add(Activator.CreateInstance(type));
            }
            return kept.Count; // @com-after
        }
        // アセンブリを読めなかった理由の試験用 (D-091)。自分のアセンブリを、無い版で読む (FileNotFound / FileLoad)。
        if (args.Length > 0 && args[0] == "load-fail")
        {
            try
            {
#if NETFRAMEWORK
                // .NET Framework は、厳密な名前の無いアセンブリの版を見ない。厳密な名前のある System.Core を、無い版で読む。
                // 先に System.Core を使って読み込んでおく (読み込み済みの版が、記録に並ぶ)。
                System.Linq.Enumerable.Range(0, 1).Count();
                System.Reflection.Assembly.Load("System.Core, Version=99.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089");
#else
                System.Reflection.Assembly.Load(typeof(Program).Assembly.GetName().Name + ", Version=99.0.0.0, Culture=neutral, PublicKeyToken=null");
#endif
            }
            catch (System.IO.IOException)
            {
                return 7;
            }
            return 1;
        }
        // ネイティブ境界の表示の試験用 (D-090)。user32 の EnumWindows が、マネージドのコールバックを呼び返す。
        if (args.Length > 0 && args[0] == "native-callback")
        {
            return NativeCallback.Run();
        }
        // 式の適合試験用。
        if (args.Length > 0 && args[0] == "conformance")
        {
            return Conformance.Run() > 0 ? 7 : 1;
        }
        // await をまたぐステップの試験用。
        if (args.Length > 0 && args[0] == "async")
        {
            return SlowAsync().GetAwaiter().GetResult();
        }
        // async メソッドから stepOut で、await している呼び出し元へ戻る試験用。
        if (args.Length > 0 && args[0] == "async-out")
        {
            return OuterAsync().GetAwaiter().GetResult(); // @async-out-call
        }
        // 同じ async メソッドを並行して呼ぶ試験用。あとで呼んだ方（短く待つ）が先に再開する。
        if (args.Length > 0 && args[0] == "parallel")
        {
            var slow = Waits(300);
            var quick = Waits(50);
            System.Threading.Tasks.Task.WaitAll(slow, quick);
            return slow.Result + quick.Result > 0 ? 7 : 1;
        }
        // 例外の試験用。捕まえられる例外を一つ投げ、そのあと誰も捕まえない例外を投げる。
        if (args.Length > 0 && args[0] == "throw")
        {
            try
            {
                int.Parse("not a number"); // @caught
            }
            catch (FormatException)
            {
            }
            throw new InvalidOperationException("boom"); // @thrown
        }
        // 1 行に文が 2 つあるときの、列のブレークポイント。
        if (args.Length > 0 && args[0] == "columns")
        {
            return TwoStatements();
        }
        // 対象の出力の文字コードの試験用（ASCII でない文字）。
        if (args.Length > 0 && args[0] == "text")
        {
            Console.WriteLine("text: こんにちは café");
            Console.Error.WriteLine("error: エラー café");
            return 7;
        }
        // Debugger.Break() の試験用。
        if (args.Length > 0 && args[0] == "break")
        {
            System.Diagnostics.Debugger.Break(); // @break
            return 7;
        }
        // 内側の例外（InnerException）と、利用者のコードで投げてランタイム（タスク）が捕まえる例外（user-unhandled）の試験用。
        if (args.Length > 0 && args[0] == "inner")
        {
            try
            {
                throw new InvalidOperationException("outer", new FormatException("inner")); // @inner
            }
            catch (InvalidOperationException)
            {
            }
            var task = System.Threading.Tasks.Task.Run(() => ThrowInTask());
            try
            {
                task.Wait();
            }
            catch (AggregateException)
            {
            }
            return 7;
        }
        var list = new List<string>();
        for (int i = 0; i < 3; i++)
        {
            list.Add("item" + i); // @loop
        }
        var copied = Copy(list); // @copy
        GC.KeepAlive(copied);
        new Order(3, "tea").Total(new[] { 10, 20 }, 1.5);
        Inner(Seven); // @filter-call
        Passes(1); // @passes-call
        int r = Work(21); // @call
        Console.WriteLine("result=" + r + " args=" + string.Join(",", args) + " env=" + Environment.GetEnvironmentVariable("SMOKE_VAR")); // @after-call
        Console.Error.WriteLine("to stderr"); // @stderr
        return 7; // @return
    }

    // ステップの試験用。印の付いたメソッドは「利用者のコードではない」ので、ステップは中で止まらず、その先の Inner に入る。
    [System.Diagnostics.DebuggerStepThrough]
    static int Passes(int n)
    {
        return Inner(n);
    }

    static async System.Threading.Tasks.Task<int> SlowAsync()
    {
        int before = 5; // @async-before
        await System.Threading.Tasks.Task.Delay(50); // @async-await
        int after = before + 1; // @async-after
        return after; // @async-return
    }

    static async System.Threading.Tasks.Task<int> OuterAsync()
    {
        int value = await SlowAsync(); // @outer-await
        return value + 1; // @outer-return
    }

    static async System.Threading.Tasks.Task<int> Waits(int delay)
    {
        await System.Threading.Tasks.Task.Delay(delay); // @waits-await
        return delay; // @waits-return
    }

    // ステップの試験用。ステップインは、既定ではプロパティの中に入らない。
    static int Seven
    {
        get { return 7; } // @getter
    }

    static int Inner(int n)
    { // @inner-open
        return n + 1;
    }

    // 同じ行の前の文と後ろの文を、列で別々に止める。
    static int TwoStatements()
    {
        int first = 1; int second = first + 1; // @two-statements
        return second; // @two-after
    }

    static int Work(int n)
    { // @work-open
        int doubled = n * 2; // @work-first
        return doubled; // @work
    }

    // 参照型の戻り値の試験用（ステップで越えたあと、変数の欄で開ける）。
    static List<string> Copy(List<string> source)
    {
        return new List<string>(source);
    }

    // user-unhandled の試験用。タスクの中で投げ、タスクの仕組み（利用者のコードではない）が捕まえる。
    static int ThrowInTask()
    {
        throw new ArgumentException("in task"); // @user-unhandled
    }
}

// 値の表示の試験用。フィールド、自動実装プロパティ、静的メンバー、定数を一つずつ持つ。
class Order
{
    public static int Created;
    public const int Max = 99;
    private readonly int id;
    public string Name { get; set; }
    // 静的プロパティ（getter を実行して、静的メンバーの節に並べる）。
    public static int Twice { get { return Created * 2; } }
    // スレッドごとの静的フィールド（読むには、そのスレッドのフレームが要る）。
    [ThreadStatic] public static int PerThread;

    public Order(int id, string name)
    {
        this.id = id;
        Name = name;
        Created++;
        PerThread = 5;
    }

    public int Total(int[] prices, double rate)
    {
        string label = "a\"b";
        bool ok = true;
        char letter = 'x';
        long big = 1234567890123L;
        object boxed = 5;
        string missing = null;
        var names = new List<string> { "x" };
        int[,] grid = new int[2, 3];
        Shade shade = Shade.Dark;
        ConsoleModifiers keys = ConsoleModifiers.Alt | ConsoleModifiers.Shift;
        decimal price = 12.50m;
        int? maybe = 7;
        int? nothing = null;
        var point = new Point { X = 1, Y = 2 };
        var moment = new DateTime(2024, 2, 29, 13, 5, 9);
        float ratio = 0.25f;
        byte small = 200;
        var shown = new Shown();
        var bag = new Bag();
        var displayed = new Displayed();
        int sum = 0;
        foreach (var p in prices) sum += p;
        GC.KeepAlive(new object[] { label, ok, letter, big, boxed, missing, names, grid, rate, shade, keys, price, maybe, nothing, point, moment, ratio, small, shown, bag, displayed });
        return sum + id; // @values
    }
}

// 評価の中断の試験用。Spin の getter は終わらない（変数の欄で開くと、getter が時間切れになる）。
class Spinner
{
    public int Ready = 1;

    public int Spin
    {
        get
        {
            while (true) { }
        }
    }

    public int After { get { return 2; } }
}

// DebuggerDisplay の {…} の中の式の試験用。
[System.Diagnostics.DebuggerDisplay("{Name,nq} has {Items.Length * 2} ({Describe()}) {Missing}")]
class Displayed
{
    public string Name = "d";
    public int[] Items = { 1, 2, 3 };

    string Describe()
    {
        return "ok";
    }
}

// DebuggerBrowsable の試験用。
class Shown
{
    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.Never)]
    public int Hidden = 1;
    public int Visible = 2;
    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.RootHidden)]
    public int[] Items = { 3, 4 };
    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.Collapsed)]
    public string Note = "n";
    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.Never)]
    public int HiddenProperty { get { return 5; } }
    public int VisibleProperty { get { return 6; } }
    // 自動実装プロパティ: 裏のフィールドに、コンパイラが DebuggerBrowsable(Never) を付ける。プロパティとしては見えること。
    public string Auto { get; set; } = "a";
    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.Never)]
    public string HiddenAuto { get; set; } = "h";
}

// DebuggerTypeProxy の試験用。中身は、代わりの型（BagView）のメンバーとして見える。
[System.Diagnostics.DebuggerTypeProxy(typeof(BagView))]
class Bag
{
    internal int[] raw = { 1, 2, 3 };
}

class BagView
{
    readonly Bag bag;

    public BagView(Bag bag)
    {
        this.bag = bag;
    }

    public int Count { get { return bag.raw.Length; } }

    [System.Diagnostics.DebuggerBrowsable(System.Diagnostics.DebuggerBrowsableState.RootHidden)]
    public int[] Items { get { return bag.raw; } }
}

// 値の表示の試験用。
enum Shade
{
    Light,
    Dark,
}

struct Point
{
    public const string Unit = "px";
    public int X;
    public int Y;
    // 値型のプロパティ（getter の this は、値の写し）。
    public int Area { get { return X * Y; } }

    public int Sum()
    {
        return X + Y;
    }

    // 位置のパターン（`point is (1, 2)`）の試験用。
    public void Deconstruct(out int x, out int y)
    {
        x = X;
        y = Y;
    }

    public override string ToString()
    {
        return X + "," + Y;
    }
}

// 利用者が定義したデリゲートの型。
delegate int Transform(int value);

// 式の適合試験。同じ式を、コンパイラ（Add の 2 番目の引数）とデバッガー（1 番目の文字列を評価させる）の両方に通して、
// 結果を比べる。デバッガーの式の評価が、C# のコンパイラと同じ値・同じ型を出すことを確かめるためのもの。
// 式を足すときは、文字列と式を必ず同じ内容にする。まだ評価できない式は tests/smoke/conformance-pending.txt に書く。
static class Conformance
{
    public static readonly List<string> Texts = new List<string>();
    public static readonly List<object> Values = new List<object>();
    // 式のコンパイル時の型が、null 許容の値型（int? など）か。Values に入れると int? は int になるので、別に覚えておく。
    public static readonly List<bool> Lifted = new List<bool>();

    static void Add<T>(string text, T value)
    {
        Texts.Add(text);
        Values.Add(value);
        Lifted.Add(Nullable.GetUnderlyingType(typeof(T)) != null);
    }

    public static int Run()
    {
        int a = 7;
        int b = 3;
        long big = 5000000000L;
        double d = 2.5;
        float f = 0.5f;
        bool yes = true;
        bool no = false;
        string s = "héllo";
        string none = null;
        char c = 'x';
        byte small = 200;
        uint u = 4000000000;
        int[] numbers = { 10, 20, 30 };
        var words = new List<string> { "one", "two" };
        var map = new Dictionary<string, int> { { "k", 9 } };
        Shade shade = Shade.Dark;
        object boxed = 42;
        object boxedText = "t";
        object boxedBytes = new byte[] { 1, 0xAB, 3 };
        object boxedNumbers = numbers;
        int q1 = 0, r1 = 0;
        int @class = 3;
        decimal price = 12.50m;
        int? maybe = 5;
        int? nothing = null;
        var point = new Point { X = 1, Y = 2 };

        // 算術
        Add("a + b", a + b);
        Add("a - b * 2", a - b * 2);
        Add("(a + b) * 2", (a + b) * 2);
        Add("a / b", a / b);
        Add("a % b", a % b);
        Add("-a", -a);
        Add("a / 2.0", a / 2.0);
        Add("d * 2", d * 2);
        Add("a + d", a + d);
        Add("1.5 + a", 1.5 + a);
        Add("big + 1", big + 1);
        Add("big * 2", big * 2);
        Add("big / 3", big / 3);
        Add("10L * a", 10L * a);
        Add("small + 1", small + 1);
        Add("small * 2", small * 2);
        Add("u", u);
        Add("u + 1", u + 1);
        Add("u * 2", u * 2);
        Add("f + 1", f + 1);
        Add("f * f", f * f);
        Add("d / 0", d / 0);
        Add("c + 1", c + 1);
        Add("'a' + 'b'", 'a' + 'b');
        Add("price + 1", price + 1);
        Add("price * 2", price * 2);
        // 比較と論理
        Add("a == 7", a == 7);
        Add("a != b", a != b);
        Add("a < b", a < b);
        Add("a >= 7", a >= 7);
        Add("yes && no", yes && no);
        Add("yes || no", yes || no);
        Add("!yes", !yes);
        Add("a > b && s.Length == 5", a > b && s.Length == 5);
        Add("c == 'x'", c == 'x');
        Add("a == b == false", a == b == false);
        Add("maybe == 5", maybe == 5);
        Add("nothing == null", nothing == null);
        Add("none == null", none == null);
        Add("s == \"héllo\"", s == "héllo");
        Add("shade == Shade.Dark", shade == Shade.Dark);
        // ビット演算
        Add("a & b", a & b);
        Add("a | b", a | b);
        Add("a ^ b", a ^ b);
        Add("~a", ~a);
        Add("a << 2", a << 2);
        Add("a >> 1", a >> 1);
        // 条件、null
        Add("a > b ? 1 : 2", a > b ? 1 : 2);
        Add("true ? a : b", true ? a : b);
        Add("none ?? \"x\"", none ?? "x");
        Add("maybe ?? 0", maybe ?? 0);
        Add("none?.Length", none?.Length);
        Add("s?.ToUpper()", s?.ToUpper());
        Add("words.Count > 1 ? words[1] : \"\"", words.Count > 1 ? words[1] : "");
        // リテラル
        Add("'x'", 'x');
        Add("0xFF", 0xFF);
        Add("0b101", 0b101);
        Add("1_000", 1_000);
        Add("1.5f", 1.5f);
        Add("1.5m", 1.5m);
        Add("@\"c:\\x\"", @"c:\x");
        Add("$\"{a}-{b}\"", $"{a}-{b}");
        // キャストと型の検査
        Add("(long)a", (long)a);
        Add("(double)a / b", (double)a / b);
        Add("(int)d", (int)d);
        Add("(int)boxed", (int)boxed);
        Add("(Shade)1", (Shade)1);
        Add("(int)shade", (int)shade);
        // 収まらない定数のキャストは、unchecked の中でだけ書ける（外では、コンパイルのエラー）。
        Add("unchecked((byte)300)", unchecked((byte)300));
        // decimal と null 許容の値・null の組み合わせ（持ち上げた演算子）、列挙型と数の引き算、null 許容の型の検査。
        Add("price + maybe", price + maybe);
        Add("price * nothing", price * nothing);
#pragma warning disable CS0472 // decimal と null の比較は、コンパイラが常に false とする。その結果を評価と突き合わせる。
        Add("price == null", price == null);
#pragma warning restore CS0472
        Add("price > maybe", price > maybe);
        Add("(decimal)shade", (decimal)shade);
        Add("(Shade)price", (Shade)price);
        Add("a - shade", a - shade);
        Add("shade - 0", shade - 0);
        Add("yes + null", yes + null);
        Add("none + none", none + none);
        Add("(object)maybe", (object)maybe);
#pragma warning disable CS0183 // `a` は int。`is int?` が常に真になることを、評価と突き合わせる。
        Add("a is int?", a is int?);
#pragma warning restore CS0183
        Add("-2147483648", -2147483648);
        Add("price is > 10", price is > 10);
        // 宣言の型が object の変数に入った文字列は、相手が string か null のときだけ連結になる（boxed + boxedText はエラー）。
        Add("boxedText + \"!\"", boxedText + "!");
        Add("boxedText + null", boxedText + null);
        Add("(string)boxedText + boxed", (string)boxedText + boxed);
        Add("s is string", s is string);
        Add("boxed is int", boxed is int);
        Add("boxed is string", boxed is string);
        Add("boxed as string", boxed as string);
        Add("a is > 5", a is > 5);
        // 型に関する式
        Add("typeof(int).FullName", typeof(int).FullName);
        Add("nameof(a)", nameof(a));
        Add("default(int)", default(int));
        Add("default(string)", default(string));
        Add("sizeof(int)", sizeof(int));
        Add("int.MaxValue", int.MaxValue);
        Add("string.Empty", string.Empty);
        Add("string.IsNullOrEmpty(none)", string.IsNullOrEmpty(none));
        Add("double.Parse(\"1.5\")", double.Parse("1.5"));
        // メンバー、添字、呼び出し
        Add("s.Length", s.Length);
        Add("s.ToUpper()", s.ToUpper());
        Add("s + a", s + a);
        Add("\"a\" + \"b\"", "a" + "b");
        Add("s[0]", s[0]);
        Add("\"abc\".Substring(1)", "abc".Substring(1));
        Add("s.Length.ToString()", s.Length.ToString());
        Add("numbers[1]", numbers[1]);
        Add("numbers.Length", numbers.Length);
        Add("numbers[a - 6]", numbers[a - 6]);
        Add("words[0]", words[0]);
        Add("words.Count", words.Count);
        Add("words[words.Count - 1]", words[words.Count - 1]);
        Add("words.Contains(\"two\")", words.Contains("two"));
        Add("!words.Contains(\"x\")", !words.Contains("x"));
        Add("map[\"k\"]", map["k"]);
        Add("shade", shade);
        Add("Shade.Light", Shade.Light);
        Add("boxed", boxed);
        Add("price", price);
        Add("maybe.Value", maybe.Value);
        Add("point.X + point.Y", point.X + point.Y);
        Add("point.Sum()", point.Sum());
        Add("a.ToString()", a.ToString());
        Add("a.CompareTo(b)", a.CompareTo(b));
        Add("System.Math.Max(a, b)", System.Math.Max(a, b));
        Add("System.Math.Abs(-5)", System.Math.Abs(-5));
        Add("System.Math.Sqrt(16)", System.Math.Sqrt(16));
        Add("System.String.Concat(s, \"!\")", System.String.Concat(s, "!"));
        // オブジェクトと配列の生成
        Add("new Point().X", new Point().X);
        Add("new int[] { 1, 2 }.Length", new int[] { 1, 2 }.Length);
        Add("new System.Text.StringBuilder(\"ab\").Length", new System.Text.StringBuilder("ab").Length);
        // ラムダ、LINQ、switch 式
        Add("System.Linq.Enumerable.Sum(numbers)", System.Linq.Enumerable.Sum(numbers));
        Add("System.Linq.Enumerable.Count(words, w => w.Length > 2)", System.Linq.Enumerable.Count(words, w => w.Length > 2));
        Add("numbers.Length switch { 3 => \"three\", _ => \"?\" }", numbers.Length switch { 3 => "three", _ => "?" });

        // ---- 文法の棚卸し (docs/DECISIONS.md D-026 の 3) ----
        // C# の仕様の「式」の文法を順に見て、上に無い書き方を一つずつ足したもの。対象外と決めたもの (await、ポインター、
        // stackalloc) は入れていない。代入や ++ は、何度評価しても同じ結果になる形 (式の中で値を入れ直す) で書く。
        int[,] grid = { { 1, 2, 3 }, { 4, 5, 6 } };
        int[][] jagged = { new[] { 1 }, new[] { 2, 3 } };
        var money = new Money(5);
        var indexed = new Indexed();
        Func<int, int> twice = x => x * 2;
        int n1 = 0;
        int n2 = 0;
        int? n3 = null;
        string text = "t";
        var pair = (x: 1, y: "two");
        IComparable comparable = 7;
        var holder = new Holder();
        // リテラル
        Add("1e3", 1e3);
        Add("1.5e-1", 1.5e-1);
        Add("1L", 1L);
        Add("1u", 1u);
        Add("1ul", 1ul);
        Add("0x7FFFFFFFFFFFFFFF", 0x7FFFFFFFFFFFFFFF);
        Add("2147483648", 2147483648);
        Add("'\\n'", '\n');
        Add("'\\u0041'", '\u0041');
        Add("\"a\\tb\"", "a\tb");
        Add("\"\\u00e9\"", "\u00e9");
        Add("@\"a\"\"b\"", @"a""b");
        Add("\"\"\"raw \"x\" text\"\"\"", """raw "x" text""");
        Add("$\"{a,4}|{d:F2}\"", $"{a,4}|{d:F2}");
        Add("$\"{(a > b ? \"y\" : \"n\")}!\"", $"{(a > b ? "y" : "n")}!");
        Add("$@\"{a}\\{b}\"", $@"{a}\{b}");
        // 単項
        Add("+a", +a);
        Add("+small", +small);
        Add("-(-a)", -(-a));
        Add("!(a > b)", !(a > b));
        Add("~small", ~small);
        Add("-u", -u);
        Add("-big", -big);
        // 型の変換
        Add("(char)65", (char)65);
        Add("(int)'A'", (int)'A');
        Add("(float)d", (float)d);
        Add("(decimal)a", (decimal)a);
        Add("(object)a", (object)a);
        Add("(string)null", (string)null);
        Add("(int?)a", (int?)a);
        Add("(IComparable)a", (IComparable)a);
        Add("(int)comparable", (int)comparable);
        Add("(long)(a + b)", (long)(a + b));
        Add("(double)a / (double)b", (double)a / (double)b);
        Add("checked(a + b)", checked(a + b));
        Add("unchecked(a * 1000000000)", unchecked(a * 1000000000));
        // 条件と null: 両側の型の統一
        Add("a > b ? d : a", a > b ? d : a);
        Add("yes ? maybe : nothing", yes ? maybe : nothing);
        Add("no ? 1 : 2.5", no ? 1 : 2.5);
        Add("none ?? none ?? \"z\"", none ?? none ?? "z");
        Add("nothing ?? a", nothing ?? a);
        Add("maybe ?? big", maybe ?? big);
        Add("words?[0]", words?[0]);
        Add("none?[0]", none?[0]);
        Add("none?.Length ?? -1", none?.Length ?? -1);
        Add("s?.Length", s?.Length);
        Add("maybe + 1", maybe + 1);
        Add("nothing + 1", nothing + 1);
        Add("maybe > 3", maybe > 3);
        Add("maybe.HasValue", maybe.HasValue);
        Add("nothing.GetValueOrDefault()", nothing.GetValueOrDefault());
        // パターン
        Add("boxed is int k && k > 40", boxed is int k && k > 40);
        Add("boxed is var v && v != null", boxed is var v && v != null);
        Add("a is 7 or 8", a is 7 or 8);
        Add("a is > 5 and < 10", a is > 5 and < 10);
        Add("a is not 3", a is not 3);
        Add("s is { Length: 5 }", s is { Length: 5 });
        Add("point is { X: 1, Y: 2 }", point is { X: 1, Y: 2 });
        Add("none is null", none is null);
        Add("s is not null", s is not null);
        Add("(a, b) is (7, 3)", (a, b) is (7, 3));
        Add("boxed is int or string", boxed is int or string);
        Add("shade is Shade.Dark", shade is Shade.Dark);
        Add("boxed is 42", boxed is 42);
        Add("c is >= 'a' and <= 'z'", c is >= 'a' and <= 'z');
        // switch 式
        Add("a switch { 7 => \"seven\", _ => \"other\" }", a switch { 7 => "seven", _ => "other" });
        Add("boxed switch { int m when m > 40 => \"big\", string => \"text\", _ => \"?\" }", boxed switch { int m when m > 40 => "big", string => "text", _ => "?" });
        Add("(a, b) switch { (7, 3) => 1, _ => 0 }", (a, b) switch { (7, 3) => 1, _ => 0 });
        Add("shade switch { Shade.Dark => 1, Shade.Light => 2, _ => 0 }", shade switch { Shade.Dark => 1, Shade.Light => 2, _ => 0 });
        // 呼び出し: 型引数、省略できる引数、名前つき引数、params、out、拡張メソッド
        Add("Echo<int>(a)", Echo<int>(a));
        Add("Echo(s)", Echo(s));
        Add("Conformance.Echo<string>(\"x\")", Conformance.Echo<string>("x"));
        Add("Total()", Total());
        Add("Total(1, 2, 3)", Total(1, 2, 3));
        Add("Total(numbers)", Total(numbers));
        Add("Join(\"x\", \"y\")", Join("x", "y"));
        Add("Join(\"x\")", Join("x"));
        Add("Join(b: \"+\", a: \"x\")", Join(b: "+", a: "x"));
        Add("Join(\"x\", times: 2)", Join("x", times: 2));
        Add("TryHalf(8, out _)", TryHalf(8, out _));
        Add("TryHalf(8, out var half) ? half : -1", TryHalf(8, out var half) ? half : -1);
        Add("int.TryParse(\"12\", out var parsed) && parsed == 12", int.TryParse("12", out var parsed) && parsed == 12);
        Add("s.Twice()", s.Twice());
        Add("\"ab\".Twice().Length", "ab".Twice().Length);
        Add("Math.Max(a, big)", Math.Max(a, big));
        Add("Math.Round(d)", Math.Round(d));
        Add("Math.Abs(-2.5)", Math.Abs(-2.5));
        Add("Math.PI > 3", Math.PI > 3);
        Add("string.Join(\",\", numbers)", string.Join(",", numbers));
        Add("string.Join(\"-\", \"a\", \"b\")", string.Join("-", "a", "b"));
        Add("string.Format(\"{0}-{1}\", a, b)", string.Format("{0}-{1}", a, b));
        Add("string.Concat(s, a)", string.Concat(s, a));
        Add("a.Equals(7)", a.Equals(7));
        Add("object.Equals(a, 7)", object.Equals(a, 7));
        Add("object.ReferenceEquals(s, s)", object.ReferenceEquals(s, s));
        Add("s.IndexOf('l')", s.IndexOf('l'));
        Add("s.Substring(1, 2)", s.Substring(1, 2));
        Add("s.Replace(\"l\", \"L\")", s.Replace("l", "L"));
        Add("a.ToString(\"D3\")", a.ToString("D3"));
        Add("a.GetType().Name", a.GetType().Name);
        Add("typeof(int) == a.GetType()", typeof(int) == a.GetType());
        Add("twice(a)", twice(a));
        Add("twice.Invoke(a)", twice.Invoke(a));
        Add("comparable.CompareTo(7)", comparable.CompareTo(7));
        // LINQ とラムダ (ラムダが変数 a を取り込むので、このメソッドの a はクロージャの中にある)
        Add("numbers.Sum()", numbers.Sum());
        Add("numbers.Max()", numbers.Max());
        Add("numbers.Count(n => n > a)", numbers.Count(n => n > a));
        Add("numbers.Where(n => n > 10).Count()", numbers.Where(n => n > 10).Count());
        Add("numbers.Select(n => n * 2).First()", numbers.Select(n => n * 2).First());
        Add("words.Any(w => w == \"two\")", words.Any(w => w == "two"));
        Add("words.Select((w, i) => w + i).Last()", words.Select((w, i) => w + i).Last());
        Add("(from n in numbers where n > 10 select n).Count()", (from n in numbers where n > 10 select n).Count());
        Add("new Func<int, int>(x => x + 1)(a)", new Func<int, int>(x => x + 1)(a));
        Add("((Func<int, int>)(x => x * 3))(a)", ((Func<int, int>)(x => x * 3))(a));
        Add("words.Find(w => w.StartsWith(\"t\"))", words.Find(w => w.StartsWith("t")));
        Add("numbers.OrderByDescending(n => n).First()", numbers.OrderByDescending(n => n).First());
        Add("numbers.Where(n => n % 20 == 0).Sum()", numbers.Where(n => n % 20 == 0).Sum());
        Add("words.Select(w => w.Length).Max()", words.Select(w => w.Length).Max());
        Add("Array.Exists(numbers, n => n == a + 3)", Array.Exists(numbers, n => n == a + 3));
        Add("numbers.Any(n => n > big)", numbers.Any(n => n > big));
        Add("words.Count(w => w.Length > a - 4)", words.Count(w => w.Length > a - 4));
        Add("numbers.Select(n => n * d).Last()", numbers.Select(n => n * d).Last());
        Add("numbers.Where(n => n > 10 && n < 30).Count()", numbers.Where(n => n > 10 && n < 30).Count());
        Add("words.Select(w => w.ToUpper()).First()", words.Select(w => w.ToUpper()).First());
        Add("numbers.Select((n, i) => n * i).Sum()", numbers.Select((n, i) => n * i).Sum());
        // 選ぶ式を渡す集計は、本体の型がそのまま合うオーバーロード (Sum(Func<T, decimal>) など) になる。
        Add("numbers.Sum(n => n * price)", numbers.Sum(n => n * price));
        Add("numbers.Sum(n => (long)n)", numbers.Sum(n => (long)n));
        Add("words.Sum(w => w.Length * 0.5)", words.Sum(w => w.Length * 0.5));
        Add("numbers.Max(n => n / 4.0)", numbers.Max(n => n / 4.0));
        Add("numbers.Average(n => (decimal)n)", numbers.Average(n => (decimal)n));
        Add("((Func<int, int, int>)((x, y) => x * y))(a, b)", ((Func<int, int, int>)((x, y) => x * y))(a, b));
        Add("(from w in words orderby w.Length descending select w).First()", (from w in words orderby w.Length descending select w).First());
        // 利用者が定義した演算子と変換
        Add("(money + money).Amount", (money + money).Amount);
        Add("(money + 1).Amount", (money + 1).Amount);
        Add("(-money).Amount", (-money).Amount);
#pragma warning disable CS1718 // 同じ変数どうしの比較を、評価と突き合わせる。
        Add("money == money", money == money);
#pragma warning restore CS1718
        Add("money != new Money(5)", money != new Money(5));
        Add("(int)money", (int)money);
        Add("((Money)3).Amount", ((Money)3).Amount);
        Add("price + price", price + price);
        Add("price > 10", price > 10);
        Add("price == 12.5m", price == 12.5m);
        Add("-price", -price);
        Add("(int)price", (int)price);
        // 生成
        Add("new Point { X = 3, Y = 4 }.Sum()", new Point { X = 3, Y = 4 }.Sum());
        Add("new Money(9).Amount", new Money(9).Amount);
        Add("new List<int> { 1, 2, 3 }.Count", new List<int> { 1, 2, 3 }.Count);
        Add("new Dictionary<string, int> { [\"k\"] = 1 }[\"k\"]", new Dictionary<string, int> { ["k"] = 1 }["k"]);
        Add("new[] { 1, 2, 3 }.Length", new[] { 1, 2, 3 }.Length);
        Add("new int[3].Length", new int[3].Length);
        Add("new int[2, 3].Length", new int[2, 3].Length);
        Add("new { A = 1, B = \"x\" }.B", new { A = 1, B = "x" }.B);
        Add("new string('a', 3)", new string('a', 3));
        Add("(1, \"a\").Item2", (1, "a").Item2);
        Add("pair.y", pair.y);
        Add("pair.Item1", pair.Item1);
        // 添字
        Add("grid[1, 2]", grid[1, 2]);
        Add("grid.Length", grid.Length);
        Add("grid.GetLength(1)", grid.GetLength(1));
        Add("jagged[1][0]", jagged[1][0]);
        Add("s[s.Length - 1]", s[s.Length - 1]);
        Add("indexed[2]", indexed[2]);
        Add("indexed[\"a\", 1]", indexed["a", 1]);
        Add("map.ContainsKey(\"k\")", map.ContainsKey("k"));
        // 型に関する式
        Add("typeof(List<int>).Name", typeof(List<int>).Name);
        Add("typeof(Dictionary<,>).Name", typeof(Dictionary<,>).Name);
        Add("typeof(int[]).Name", typeof(int[]).Name);
        Add("typeof(Point).IsValueType", typeof(Point).IsValueType);
        Add("default(Shade)", default(Shade));
        Add("default(int?)", default(int?));
        Add("default(Point).X", default(Point).X);
        Add("sizeof(long)", sizeof(long));
        Add("nameof(Point.X)", nameof(Point.X));
        Add("nameof(System.Math)", nameof(System.Math));
        Add("Point.Unit", Point.Unit);
        Add("words is List<string>", words is List<string>);
        Add("(words as System.Collections.IList).Count", (words as System.Collections.IList).Count);
        // 代入と増減 (同じ式を何度評価しても、同じ結果になる形)
        Add("n1 = 5", n1 = 5);
        Add("text = \"z\"", text = "z");
        Add("n3 ??= 9", n3 ??= 9);
        Add("(n2 = 1) + (n2 += 2)", (n2 = 1) + (n2 += 2));
        Add("(n2 = 1) + n2++ + n2", (n2 = 1) + n2++ + n2);
        Add("(n2 = 1) + ++n2", (n2 = 1) + ++n2);
        Add("(n2 = 5) - --n2", (n2 = 5) - --n2);
        Add("(n2 = 3) * (n2 *= 2)", (n2 = 3) * (n2 *= 2));
        // 代入先の種類: 構造体のフィールド、配列の要素、インデクサー、プロパティ、フィールド、静的フィールド、null 許容
        Add("point.X = 1", point.X = 1);
        Add("numbers[0] = 10", numbers[0] = 10);
        Add("words[0] = \"one\"", words[0] = "one");
        Add("holder.Name = \"h\"", holder.Name = "h");
        Add("holder.Count = 3", holder.Count = 3);
        Add("(holder.Count = 3) + holder.Count++ + holder.Count", (holder.Count = 3) + holder.Count++ + holder.Count);
        Add("Shared = 4", Shared = 4);
        Add("Conformance.Shared += 0", Conformance.Shared += 0);
        Add("maybe = 5", maybe = 5);
        Add("small = 200", small = 200);
        Add("(small = 255) + ++small", (small = 255) + ++small);
        Add("(small = 200) + 0", (small = 200) + 0);
        Add("(text = null) ?? (text = \"z\")", (text = null) ?? (text = "z"));
        Add("a == default", a == default);
        Add("default != s", default != s);
        Add("shade == default", shade == default);
        Add("maybe == default", maybe == default);
        Add("(n1 = default) + 1", (n1 = default) + 1);
        Add("new object[] { a, s }.Length", new object[] { a, s }.Length);
        // 古いコードや、まれな書き方（実際のコードの式の検証で見つかったもの）
        Add("global::System.Math.Max(a, b)", global::System.Math.Max(a, b));
        Add("Comparer<int>.Default.Compare(a, b)", Comparer<int>.Default.Compare(a, b));
        Add("EqualityComparer<string>.Default.Equals(s, \"héllo\")", EqualityComparer<string>.Default.Equals(s, "héllo"));
        Add("Holder<int>.Shared", Holder<int>.Shared);
        Add("Holder<string>.Make(\"x\")", Holder<string>.Make("x"));
        Add("words.Find(delegate (string w) { return w.Length == 3; })", words.Find(delegate (string w) { return w.Length == 3; }));
        Add("numbers.Count(n => { return n > a; })", numbers.Count(n => { return n > a; }));
        // クエリ式の group、into、2 つ目の from、join
        Add("(from n in numbers group n by n % 20).Count()", (from n in numbers group n by n % 20).Count());
        Add("(from w in words group w.Length by w[0] into g select g.Key).First()", (from w in words group w.Length by w[0] into g select g.Key).First());
        Add("(from n in numbers group n by n % 20 into g select g.Sum()).Max()", (from n in numbers group n by n % 20 into g select g.Sum()).Max());
        Add("(from n in numbers from m in numbers select n * m).Sum()", (from n in numbers from m in numbers select n * m).Sum());
        Add("(from n in numbers from w in words select w + n).Last()", (from n in numbers from w in words select w + n).Last());
        Add("(from n in numbers join m in numbers on n equals m / 1 select n + m).Sum()", (from n in numbers join m in numbers on n equals m / 1 select n + m).Sum());
        Add("(from n in numbers join w in words on n / 10 equals w.Length into ws select ws.Count()).Sum()", (from n in numbers join w in words on n / 10 equals w.Length into ws select ws.Count()).Sum());
        Add("(from n in numbers select n * 2 into dd where dd > 30 select dd).Count()", (from n in numbers select n * 2 into dd where dd > 30 select dd).Count());
        // throw 式と、文を並べたラムダ式の本体
        Add("maybe ?? throw new InvalidOperationException()", maybe ?? throw new InvalidOperationException());
        Add("a > 0 ? a : throw new ArgumentException()", a > 0 ? a : throw new ArgumentException());
        Add("a < 0 ? throw new ArgumentException() : s", a < 0 ? throw new ArgumentException() : s);
        Add("s ?? throw new ArgumentNullException(nameof(s))", s ?? throw new ArgumentNullException(nameof(s)));
        Add("numbers.Select(n => n > 0 ? n : throw new ArgumentException()).Sum()", numbers.Select(n => n > 0 ? n : throw new ArgumentException()).Sum());
        Add("numbers.Select(n => { var dbl = n * 2; return dbl + 1; }).Sum()", numbers.Select(n => { var dbl = n * 2; return dbl + 1; }).Sum());
        Add("numbers.Select(n => { long wide = n; wide *= 3; return wide; }).Last()", numbers.Select(n => { long wide = n; wide *= 3; return wide; }).Last());
        Add("numbers.Count(n => { var lim = a; return n > lim; })", numbers.Count(n => { var lim = a; return n > lim; }));
        Add("numbers.Select(n => { int k2 = n; k2++; ++k2; return k2; }).First()", numbers.Select(n => { int k2 = n; k2++; ++k2; return k2; }).First());
        Add("words.Select(w => { string tt = w; tt ??= \"x\"; return tt.Length; }).Sum()", words.Select(w => { string tt = w; tt ??= "x"; return tt.Length; }).Sum());
        Add("words.Select(w => { var u = w.ToUpper(); var v = u + w; return v; }).First()", words.Select(w => { var u = w.ToUpper(); var v = u + w; return v; }).First());
        Add("\"\\ud83d\\udcac\".Length", "💬".Length);
        Add("new int[][] { new int[] { 1, 2 }, new int[] { 3 } }[1][0]", new int[][] { new int[] { 1, 2 }, new int[] { 3 } }[1][0]);
        Add("new string[2][].Length", new string[2][].Length);
        Add("new int?[] { 1, null }[1]", new int?[] { 1, null }[1]);
        Add("new List<int>[1].Length", new List<int>[1].Length);
        Add("typeof(int?).Name", typeof(int?).Name);
        Add("typeof(Shade?[]).Name", typeof(Shade?[]).Name);
        Add("(new int?[3])[1].HasValue", (new int?[3])[1].HasValue);
        // 型を周りから決める式（型を省いた new()、コレクション式）
        Holder spare = null;
        Add("Measure(new() { Count = 5 })", Measure(new() { Count = 5 }));
        Add("Sum([1, 2, 3])", Sum([1, 2, 3]));
        Add("SumArray([4, 5, ..numbers])", SumArray([4, 5, .. numbers]));
        Add("Size([1, 2])", Size([1, 2]));
        Add("((List<int>)[7, 8]).Count", ((List<int>)[7, 8]).Count);
        Add("(spare = new() { Count = 4 }).Count", (spare = new() { Count = 4 }).Count);
        Add("Sum([])", Sum([]));
        Add("Measure(default)", Measure(default));
        // 構造体と列挙型の値を object の引数に渡す（ボックス化する）
        Add("string.Format(\"{0}\", point)", string.Format("{0}", point));
        Add("string.Format(\"{0}/{1}\", shade, point.X)", string.Format("{0}/{1}", shade, point.X));
        Add("object.Equals(point, point)", object.Equals(point, point));
#pragma warning disable CA2013 // 値型を ReferenceEquals に渡すと箱に入る。その結果を評価と突き合わせる。
        Add("object.ReferenceEquals(point, point)", object.ReferenceEquals(point, point));
#pragma warning restore CA2013
        // await: GetAwaiter().GetResult() で終わるまで待つ（D-034）。async のラムダは、同期で実行して完了済みの Task を返す。
        Add("await LaterAsync(4)", LaterAsync(4).GetAwaiter().GetResult());
        Add("(await SlowAsync()).Length", SlowAsync().GetAwaiter().GetResult().Length);
        Add("await LaterAsync(1) + await LaterAsync(2)", LaterAsync(1).GetAwaiter().GetResult() + LaterAsync(2).GetAwaiter().GetResult());
        Add("await LaterAsync(a).ConfigureAwait(false)", LaterAsync(a).ConfigureAwait(false).GetAwaiter().GetResult());
        Add("RunTask(async n => await LaterAsync(n) + 1)", RunTask(async n => await LaterAsync(n) + 1));
        Add("RunTask(static n => LaterAsync(n))", RunTask(static n => LaterAsync(n)));
        Add("RunPlain(async () => await Task.Delay(1))", RunPlain(async () => await Task.Delay(1)));
        Add("RunPlain(async () => { await Task.Delay(1); })", RunPlain(async () => { await Task.Delay(1); }));
        Add("words.Select(static w => w.Length).Sum()", words.Select(static w => w.Length).Sum());
        // ラムダ式の本体の文: if、ループ、foreach、switch、try、using、lock、途中の return、メンバーと添字への代入
        Add("new Func<int, int>(n => { if (n > 5) return 1; return 2; })(7)", new Func<int, int>(n => { if (n > 5) return 1; return 2; })(7));
        Add("new Func<int, int>(n => { if (n > 5) { return 1; } else { return 2; } })(3)", new Func<int, int>(n => { if (n > 5) { return 1; } else { return 2; } })(3));
        Add("new Func<int, int>(n => { int total = 0; for (int i = 0; i < n; i++) { if (i == 2) continue; total += i; } return total; })(5)", new Func<int, int>(n => { int total = 0; for (int i = 0; i < n; i++) { if (i == 2) continue; total += i; } return total; })(5));
        Add("new Func<int, int>(n => { int total = 0; while (true) { total += n; if (total > 10) break; } return total; })(4)", new Func<int, int>(n => { int total = 0; while (true) { total += n; if (total > 10) break; } return total; })(4));
        Add("new Func<int, int>(n => { int k = 0; do { k++; } while (k < n); return k; })(3)", new Func<int, int>(n => { int k = 0; do { k++; } while (k < n); return k; })(3));
        Add("new Func<int[], int>(xs => { int total = 0; foreach (var x in xs) total += x; return total; })(numbers)", new Func<int[], int>(xs => { int total = 0; foreach (var x in xs) total += x; return total; })(numbers));
        Add("new Func<List<string>, int>(ws => { int total = 0; foreach (string w in ws) { total += w.Length; } return total; })(words)", new Func<List<string>, int>(ws => { int total = 0; foreach (string w in ws) { total += w.Length; } return total; })(words));
        Add("new Func<int, string>(n => { switch (n) { case 1: return \"one\"; case 2: case 3: return \"few\"; default: return \"many\"; } })(3)", new Func<int, string>(n => { switch (n) { case 1: return "one"; case 2: case 3: return "few"; default: return "many"; } })(3));
        Add("new Func<int, string>(n => { string r = \"none\"; switch (n) { case 1: r = \"one\"; break; default: r = \"many\"; break; } return r; })(9)", new Func<int, string>(n => { string r = "none"; switch (n) { case 1: r = "one"; break; default: r = "many"; break; } return r; })(9));
        Add("new Func<int, string>(n => { try { if (n == 0) throw new InvalidOperationException(\"zero\"); return \"ok\"; } catch (InvalidOperationException e) { return e.Message; } finally { Shared = 4; } })(0)", new Func<int, string>(n => { try { if (n == 0) throw new InvalidOperationException("zero"); return "ok"; } catch (InvalidOperationException e) { return e.Message; } finally { Shared = 4; } })(0));
        Add("new Func<int, int>(n => { using (var stream = new System.IO.MemoryStream()) { stream.WriteByte(1); return (int)stream.Length + n; } })(1)", new Func<int, int>(n => { using (var stream = new System.IO.MemoryStream()) { stream.WriteByte(1); return (int)stream.Length + n; } })(1));
        Add("new Func<int, int>(n => { lock (words) { return n + words.Count; } })(1)", new Func<int, int>(n => { lock (words) { return n + words.Count; } })(1));
        Add("new Func<int, int>(n => { var h = new Holder(); h.Count = n; h.Count += 2; return h.Count; })(5)", new Func<int, int>(n => { var h = new Holder(); h.Count = n; h.Count += 2; return h.Count; })(5));
        Add("new Func<int, int>(n => { var xs = new int[2]; xs[1] = n; xs[1]++; return xs[1]; })(5)", new Func<int, int>(n => { var xs = new int[2]; xs[1] = n; xs[1]++; return xs[1]; })(5));
        // 型の書き方: 多次元の配列、配列の配列、タプル型、null を許す注釈の付いた配列、最後の要素のあとのカンマ
        Add("(new int[,] { { 1, 2 }, { 3, 4 } })[1, 0]", (new int[,] { { 1, 2 }, { 3, 4 } })[1, 0]);
        Add("(new[,] { { 1, 2 }, { 3, 4 } }).Length", (new[,] { { 1, 2 }, { 3, 4 } }).Length);
        Add("(new[] { 1, 2, 3, }).Length", (new[] { 1, 2, 3, }).Length);
        Add("typeof(int[,]).Name", typeof(int[,]).Name);
        Add("typeof(int[][]).Name", typeof(int[][]).Name);
        Add("default((int, string)).Item1", default((int, string)).Item1);
        Add("new List<(int Id, string Name)>().Count", new List<(int Id, string Name)>().Count);
        // 細かい書き方: 整数の部分を省いた小数、名前つきのタプル要素、@ の付いた名前、switch 式の腕の throw と when、
        // 補間の中の逐語的文字列、空白をはさんだ . と ?.
        Add(".5 + .5", .5 + .5);
        Add("(x: 1, y: 2).Item2", (x: 1, y: 2).Item2);
        Add("@class + 1", @class + 1);
        Add("a switch { 7 => \"seven\", _ => throw new InvalidOperationException() }", a switch { 7 => "seven", _ => throw new InvalidOperationException() });
        Add("a switch { 7 when no => 1, 7 when yes => 2, _ => 0 }", a switch { 7 when no => 1, 7 when yes => 2, _ => 0 });
        Add("$\"{s.Replace(@\"l\", @\"L\")}\"", $"{s.Replace(@"l", @"L")}");
        Add("Comparer<int> .Default.Compare(1, 2)", Comparer<int> .Default.Compare(1, 2));
        Add("words? .Count", words? .Count);
        // 分解の代入
        Add("(q1, r1) = (a, b)", (q1, r1) = (a, b));
        Add("q1 * 10 + r1", q1 * 10 + r1);
        Add("(q1, _) = (b, a)", (q1, _) = (b, a));
        Add("q1", q1);
        // 初期化子: 入れ子（メンバーの中へ足す）、値や要素がラムダ式
        Add("new Holder { Count = 2, Items = { 5, 6 } }.Items.Count", new Holder { Count = 2, Items = { 5, 6 } }.Items.Count);
        Add("new Holder { Step = n => n + 1 }.Step(4)", new Holder { Step = n => n + 1 }.Step(4));
        Add("new List<Func<int, int>> { n => n + 1, n => n * 2 }[1](5)", new List<Func<int, int>> { n => n + 1, n => n * 2 }[1](5));
        // クエリ式: let と、範囲の変数が 2 つ以上になったあとの句
        Add("(from n in numbers let m = n * 2 where m > 30 select m + n).Sum()", (from n in numbers let m = n * 2 where m > 30 select m + n).Sum());
        Add("(from x in numbers from w in words where w.Length == 3 && x > 10 select x + w.Length).Count()", (from x in numbers from w in words where w.Length == 3 && x > 10 select x + w.Length).Count());
        Add("(from x in numbers join w in words on x / 10 equals w.Length - 2 let sum = x + w.Length orderby sum descending select sum).First()", (from x in numbers join w in words on x / 10 equals w.Length - 2 let sum = x + w.Length orderby sum descending select sum).First());
        Add("(from object o in words select o).Count()", (from object o in words select o).Count());
        // ラムダ式の中のパターン（is）、switch 式、配列と匿名型の new、switch 文の case のパターン
        object[] mixed = { 1, "two", 3.5, null, 42 };
        int?[] maybes = { 1, null, 30 };
        var pairs = new[] { (1, "a"), (2, "b") };
        Add("numbers.Count(n => n is > 10)", numbers.Count(n => n is > 10));
        Add("numbers.Count(n => n is 10 or 30)", numbers.Count(n => n is 10 or 30));
        Add("numbers.Count(n => n is not 20)", numbers.Count(n => n is not 20));
        Add("numbers.Count(n => n is > 10 and < 30)", numbers.Count(n => n is > 10 and < 30));
        Add("mixed.Count(o => o is int)", mixed.Count(o => o is int));
        Add("mixed.Count(o => o is string)", mixed.Count(o => o is string));
        Add("mixed.Count(o => o is null)", mixed.Count(o => o is null));
        Add("mixed.Count(o => o is not null)", mixed.Count(o => o is not null));
        Add("mixed.Count(o => o is 42)", mixed.Count(o => o is 42));
        Add("mixed.Count(o => o is \"two\")", mixed.Count(o => o is "two"));
        Add("mixed.Count(o => o is > 2)", mixed.Count(o => o is > 2));
        Add("mixed.Count(o => o is int k && k > 10)", mixed.Count(o => o is int k && k > 10));
        Add("mixed.Select(o => o is string t ? t.Length : -1).Sum()", mixed.Select(o => o is string t ? t.Length : -1).Sum());
        Add("mixed.Count(o => o is var v && v != null)", mixed.Count(o => o is var v && v != null));
        Add("mixed.Count(o => o is string { Length: 3 })", mixed.Count(o => o is string { Length: 3 }));
        Add("mixed.Count(o => o is int or double)", mixed.Count(o => o is int or double));
        Add("maybes.Count(m => m is > 5)", maybes.Count(m => m is > 5));
        Add("maybes.Count(m => m is null)", maybes.Count(m => m is null));
        Add("maybes.Count(m => m is 1)", maybes.Count(m => m is 1));
        Add("maybes.Count(m => m is int k && k < 5)", maybes.Count(m => m is int k && k < 5));
        Add("maybes.Count(m => m is { } v && v > 5)", maybes.Count(m => m is { } v && v > 5));
        Add("words.Count(w => w is { Length: > 2 } t && t[0] == 'o')", words.Count(w => w is { Length: > 2 } t && t[0] == 'o'));
        Add("pairs.Count(p => p is (1, _))", pairs.Count(p => p is (1, _)));
        Add("pairs.Count(p => p is (> 1, \"b\") q && q.Item1 == 2)", pairs.Count(p => p is (> 1, "b") q && q.Item1 == 2));
        Add("new[] { Shade.Dark, Shade.Light }.Count(x => x is Shade.Dark)", new[] { Shade.Dark, Shade.Light }.Count(x => x is Shade.Dark));
        Add("numbers.Count(n => boxed is int k && k > n)", numbers.Count(n => boxed is int k && k > n));
        Add("numbers.Count(n => boxedText is int k && k > n)", numbers.Count(n => boxedText is int k && k > n));
        Add("numbers.Select(n => n switch { 10 => \"ten\", 20 => \"twenty\", _ => \"many\" }).Last()", numbers.Select(n => n switch { 10 => "ten", 20 => "twenty", _ => "many" }).Last());
        Add("numbers.Select(n => n switch { > 15 => n * 2, _ => 0.5 }).Sum()", numbers.Select(n => n switch { > 15 => n * 2, _ => 0.5 }).Sum());
        Add("string.Join(\",\", mixed.Select(o => o switch { int k when k > 10 => \"big\", int => \"int\", string t => t, null => \"null\", _ => \"?\" }))", string.Join(",", mixed.Select(o => o switch { int k when k > 10 => "big", int => "int", string t => t, null => "null", _ => "?" })));
        Add("numbers.Select(n => n switch { 10 => 1, _ => throw new InvalidOperationException() }).First()", numbers.Select(n => n switch { 10 => 1, _ => throw new InvalidOperationException() }).First());
        Add("words.Select(w => w.Length switch { 3 => w, _ => null }).First()", words.Select(w => w.Length switch { 3 => w, _ => null }).First());
        Add("words.Select(w => w.Length > 5 ? w : null).First()", words.Select(w => w.Length > 5 ? w : null).First());
        Add("numbers.Select(n => new[] { n, n * 2 }).Last()[1]", numbers.Select(n => new[] { n, n * 2 }).Last()[1]);
        Add("numbers.Select(n => new int[] { n, 1 }).First().Length", numbers.Select(n => new int[] { n, 1 }).First().Length);
        Add("numbers.Select(n => new long[] { n }).First()[0]", numbers.Select(n => new long[] { n }).First()[0]);
        Add("numbers.Select(n => new[] { n, 2.5 }).First()[0]", numbers.Select(n => new[] { n, 2.5 }).First()[0]);
        Add("numbers.Select(n => new object[] { n, \"x\" }).First().Length", numbers.Select(n => new object[] { n, "x" }).First().Length);
        Add("numbers.Select(n => new int[n / 10]).Last().Length", numbers.Select(n => new int[n / 10]).Last().Length);
        Add("numbers.Select(n => new int[2, n / 10]).Last().Length", numbers.Select(n => new int[2, n / 10]).Last().Length);
        Add("numbers.Select(n => new int[n / 10][]).Last().Length", numbers.Select(n => new int[n / 10][]).Last().Length);
        Add("numbers.Select(n => new[,] { { n, 1 }, { 2, n * 2 } }).Last()[1, 1]", numbers.Select(n => new[,] { { n, 1 }, { 2, n * 2 } }).Last()[1, 1]);
        Add("numbers.Select(n => new { A = n, B = \"x\" }).Last().A", numbers.Select(n => new { A = n, B = "x" }).Last().A);
        Add("words.Select(w => new { A = w, B = w.Length }).First().B", words.Select(w => new { A = w, B = w.Length }).First().B);
        Add("words.Select(w => new { w.Length, Text = w }).Last().Text", words.Select(w => new { w.Length, Text = w }).Last().Text);
        Add("words.Select(w => new { w.Length, Text = w }).Where(p => p.Length == 3).Count()", words.Select(w => new { w.Length, Text = w }).Where(p => p.Length == 3).Count());
        Add("new Func<object, string>(o => { switch (o) { case int k when k > 10: return \"big\"; case int: return \"int\"; case string t: return t; case null: return \"null\"; default: return \"?\"; } })(boxed)", new Func<object, string>(o => { switch (o) { case int k when k > 10: return "big"; case int: return "int"; case string t: return t; case null: return "null"; default: return "?"; } })(boxed));
        Add("new Func<int, string>(n => { string r = \"none\"; switch (n) { case > 5 and < 9: r = \"mid\"; break; case 1: case 2: r = \"low\"; break; default: r = \"other\"; break; } return r; })(a)", new Func<int, string>(n => { string r = "none"; switch (n) { case > 5 and < 9: r = "mid"; break; case 1: case 2: r = "low"; break; default: r = "other"; break; } return r; })(a));
        Add("new Func<int, int>(n => { var l = new List<int> { 1 }; l[0] = n; return l[0]; })(5)", new Func<int, int>(n => { var l = new List<int> { 1 }; l[0] = n; return l[0]; })(5));
        // ラムダ式の中の as、??、?.、補間文字列、new とその初期化子、型引数を書いた呼び出し、型を書かない default
        string[] gaps = { "a", null, "ccc" };
        Add("gaps.Select(w => w as object).Count(o => o != null)", gaps.Select(w => w as object).Count(o => o != null));
        Add("mixed.Select(o => o as string).Count(t => t != null)", mixed.Select(o => o as string).Count(t => t != null));
        Add("mixed.Select(o => o as int?).Sum()", mixed.Select(o => o as int?).Sum());
        Add("gaps.Select(w => w ?? \"-\").Last()", gaps.Select(w => w ?? "-").Last());
        Add("string.Concat(gaps.Select(w => w ?? \"-\"))", string.Concat(gaps.Select(w => w ?? "-")));
        Add("maybes.Select(m => m ?? 0).Sum()", maybes.Select(m => m ?? 0).Sum());
        Add("maybes.Select(m => m ?? -1L).Sum()", maybes.Select(m => m ?? -1L).Sum());
        Add("gaps.Select(w => w ?? throw new InvalidOperationException()).First()", gaps.Select(w => w ?? throw new InvalidOperationException()).First());
        Add("gaps.Select(w => w?.Length).Sum()", gaps.Select(w => w?.Length).Sum());
        Add("gaps.Select(w => w?.Length ?? -1).Sum()", gaps.Select(w => w?.Length ?? -1).Sum());
        Add("gaps.Count(w => w?.StartsWith(\"c\") == true)", gaps.Count(w => w?.StartsWith("c") == true));
        Add("string.Concat(gaps.Select(w => w?.ToUpper()))", string.Concat(gaps.Select(w => w?.ToUpper())));
        Add("gaps.Select(w => w?[0]).Count(c => c == 'a')", gaps.Select(w => w?[0]).Count(c => c == 'a'));
        Add("maybes.Select(m => m?.ToString()).Last()", maybes.Select(m => m?.ToString()).Last());
        Add("words.Select(w => $\"<{w}>\").First()", words.Select(w => $"<{w}>").First());
        Add("numbers.Select(n => $\"{n:D4}|{n,5}|{n / 3.0:F2}\").Last()", numbers.Select(n => $"{n:D4}|{n,5}|{n / 3.0:F2}").Last());
        Add("gaps.Select(w => $\"[{w}]{{}}\").Last()", gaps.Select(w => $"[{w}]{{}}").Last());
        Add("words.Select(w => new string(w[0], 2)).First()", words.Select(w => new string(w[0], 2)).First());
        Add("numbers.Select(n => new System.Text.StringBuilder(\"x\").Append(n).ToString()).Last()", numbers.Select(n => new System.Text.StringBuilder("x").Append(n).ToString()).Last());
        Add("numbers.Select(n => new Point { X = n, Y = 1 }).Last().X", numbers.Select(n => new Point { X = n, Y = 1 }).Last().X);
        Add("numbers.Select(n => new Holder { Count = n, Name = \"h\" + n }).Last().Name", numbers.Select(n => new Holder { Count = n, Name = "h" + n }).Last().Name);
        Add("numbers.Select(n => new Holder { Items = { n, n + 1 } }).Last().Items.Sum()", numbers.Select(n => new Holder { Items = { n, n + 1 } }).Last().Items.Sum());
        Add("numbers.Select(n => new List<long> { n, 1 }).First().Sum()", numbers.Select(n => new List<long> { n, 1 }).First().Sum());
        Add("words.Select(w => new Dictionary<string, int> { [w] = w.Length }).Last()[\"two\"]", words.Select(w => new Dictionary<string, int> { [w] = w.Length }).Last()["two"]);
        Add("words.Select(w => new Dictionary<string, int> { { w, 1 } }).First().Count", words.Select(w => new Dictionary<string, int> { { w, 1 } }).First().Count);
        Add("numbers.Select(n => new DateTime(2024, 1, n / 10).Day).Sum()", numbers.Select(n => new DateTime(2024, 1, n / 10).Day).Sum());
        Add("words.Select(w => w.Cast<char>().Count()).Sum()", words.Select(w => w.Cast<char>().Count()).Sum());
        Add("mixed.Select(o => o is int ? (int)o : default).Sum()", mixed.Select(o => o is int ? (int)o : default).Sum());
        Add("numbers.Select(n => n > 15 ? (int?)n : null).Sum()", numbers.Select(n => n > 15 ? (int?)n : null).Sum());
        Add("words.Select(w => w.Length > 3 ? w : null).Count(t => t == null)", words.Select(w => w.Length > 3 ? w : null).Count(t => t == null));
        Add("numbers.Select(n => (long?)n).Sum()", numbers.Select(n => (long?)n).Sum());
        // ラムダ式の中のラムダ式（外側の引数を使うもの）、変数に入ったデリゲートの呼び出し、null 許容の要素の Sum
        int[][] rows = { new[] { 1, 2 }, new[] { 30 } };
        Func<int, int> inc = v => v + 1;
        Add("numbers.Select(n => words.Count(w => w.Length * 10 < n)).Sum()", numbers.Select(n => words.Count(w => w.Length * 10 < n)).Sum());
        Add("numbers.Where(n => numbers.Any(m => m > n)).Count()", numbers.Where(n => numbers.Any(m => m > n)).Count());
        Add("rows.Select(r => r.Where(x => x > 1).Sum()).Last()", rows.Select(r => r.Where(x => x > 1).Sum()).Last());
        Add("rows.Sum(r => r.Sum(x => x * 2))", rows.Sum(r => r.Sum(x => x * 2)));
        Add("rows.SelectMany(r => r.Select(x => x + r.Length)).Sum()", rows.SelectMany(r => r.Select(x => x + r.Length)).Sum());
        Add("words.Select(w => w.Select(c => (int)c).Max()).First()", words.Select(w => w.Select(c => (int)c).Max()).First());
        Add("words.Select(w => numbers.Select(n => n + w.Length).Last()).Sum()", words.Select(w => numbers.Select(n => n + w.Length).Last()).Sum());
        Add("words.Select(w => words.Find(x => x != w)).First()", words.Select(w => words.Find(x => x != w)).First());
        Add("words.Count(w => words.Exists(x => x.Length > w.Length))", words.Count(w => words.Exists(x => x.Length > w.Length)));
        Add("numbers.Select(n => words.ConvertAll(w => w.Length + n).Sum()).Sum()", numbers.Select(n => words.ConvertAll(w => w.Length + n).Sum()).Sum());
        Add("numbers.Select(n => numbers.Sum(m => m * 1.5) + n).Last()", numbers.Select(n => numbers.Sum(m => m * 1.5) + n).Last());
        Add("numbers.Select(n => words.OrderBy(w => w.Length * n).First()).Last()", numbers.Select(n => words.OrderBy(w => w.Length * n).First()).Last());
        Add("numbers.Select(n => new[] { 1, 2 }.Select(x => x * n).ToList()).Last().Sum()", numbers.Select(n => new[] { 1, 2 }.Select(x => x * n).ToList()).Last().Sum());
        Add("numbers.Select(n => inc(n)).Sum()", numbers.Select(n => inc(n)).Sum());
        Add("new Func<Func<int, int>, int>(f => f(4))(inc)", new Func<Func<int, int>, int>(f => f(4))(inc));
        Add("maybes.Sum()", maybes.Sum());
        Add("new List<long?> { 1, null, 2 }.Sum()", new List<long?> { 1, null, 2 }.Sum());
        // ラムダ式の中の呼び出しの、名前つき引数、省略した引数、params、型を渡す先から決める式
        Add("words.Select(w => Join(w, times: 2)).First()", words.Select(w => Join(w, times: 2)).First());
        Add("words.Select(w => Join(w)).Last()", words.Select(w => Join(w)).Last());
        Add("words.Select(w => Join(b: \"+\", a: w)).First()", words.Select(w => Join(b: "+", a: w)).First());
        Add("numbers.Select(n => Total()).Sum()", numbers.Select(n => Total()).Sum());
        Add("numbers.Select(n => Measure(new() { Count = n })).Sum()", numbers.Select(n => Measure(new() { Count = n })).Sum());
        Add("numbers.Select(n => Measure(default)).Sum()", numbers.Select(n => Measure(default)).Sum());
        Add("numbers.Select(n => Sum([n, 1])).Sum()", numbers.Select(n => Sum([n, 1])).Sum());
        Add("numbers.Select(n => SumArray([n, n])).Last()", numbers.Select(n => SumArray([n, n])).Last());
        Add("numbers.Select(n => Size([n, 2, 3])).First()", numbers.Select(n => Size([n, 2, 3])).First());        // 確認項目 (2026-11-18): 利用者が定義したデリゲートの型、メソッドの集まりからデリゲートへの変換
        Transform triple = v => v * 3;
        Add("Apply(v => v + 1, 4)", Apply(v => v + 1, 4));
        Add("((Transform)(v => v * 2))(5)", ((Transform)(v => v * 2))(5));
        Add("new Transform(v => v - 1)(5)", new Transform(v => v - 1)(5));
        Add("numbers.Select(n => Apply(v => v + n, 1)).Sum()", numbers.Select(n => Apply(v => v + n, 1)).Sum());
        Add("triple(4)", triple(4));
        Add("words.Select(string.IsNullOrEmpty).Count(x => x)", words.Select(string.IsNullOrEmpty).Count(x => x));
        Add("numbers.Select(Doubled).Sum()", numbers.Select(Doubled).Sum());
        Add("Array.ConvertAll(numbers, Convert.ToString)[0]", Array.ConvertAll(numbers, Convert.ToString)[0]);
        Add("Apply(Doubled, 6)", Apply(Doubled, 6));
        Add("new Func<int, int>(Doubled)(7)", new Func<int, int>(Doubled)(7));
        Add("((Func<string, int>)int.Parse)(\"8\")", ((Func<string, int>)int.Parse)("8"));
        Add("Apply(Echo<int>, 9)", Apply(Echo<int>, 9));
        Add("words.Select(w => w.Select(char.ToUpper).First()).Last()", words.Select(w => w.Select(char.ToUpper).First()).Last());
        Add("numbers.Select(n => Apply(Doubled, n)).Sum()", numbers.Select(n => Apply(Doubled, n)).Sum());
        // ラムダの中の >>>、checked、unchecked、構造体の with、型を書かない default との比較
        Add("numbers.Select(n => -n >>> 28).Sum()", numbers.Select(n => -n >>> 28).Sum());
        Add("numbers.Select(n => checked(n * 2)).Sum()", numbers.Select(n => checked(n * 2)).Sum());
        Add("numbers.Select(n => unchecked(n * int.MaxValue)).Sum()", numbers.Select(n => unchecked(n * int.MaxValue)).Sum());
        Add("numbers.Select(n => { try { return checked(n * int.MaxValue); } catch (OverflowException) { return -1; } }).Sum()", numbers.Select(n => { try { return checked(n * int.MaxValue); } catch (OverflowException) { return -1; } }).Sum());
        Add("numbers.Select(n => point with { X = n }).Last().X", numbers.Select(n => point with { X = n }).Last().X);
        Add("numbers.Select(n => n == default ? 1 : 0).Sum()", numbers.Select(n => n == default ? 1 : 0).Sum());
        Add("words.Select(w => w != default ? 1 : 0).Sum()", words.Select(w => w != default ? 1 : 0).Sum());
        Add("numbers.Select(n => Sum([n, ..numbers])).Sum()", numbers.Select(n => Sum([n, ..numbers])).Sum());
        // ラムダの中の分解の代入、out、ref、要素が 8 個以上のタプル、new の名前つき引数
        Add("numbers.Select(n => { var (p, q) = (n, 1); return p + q; }).Sum()", numbers.Select(n => { var (p, q) = (n, 1); return p + q; }).Sum());
        Add("numbers.Select(n => { (int p, int q) = point; return p * n + q; }).Sum()", numbers.Select(n => { (int p, int q) = point; return p * n + q; }).Sum());
        Add("words.Select(w => int.TryParse(w, out var v) ? v : -1).Sum()", words.Select(w => int.TryParse(w, out var v) ? v : -1).Sum());
        Add("numbers.Select(n => TryHalf(n, out var h) ? h : -1).Sum()", numbers.Select(n => TryHalf(n, out var h) ? h : -1).Sum());
        Add("numbers.Select(n => { int h; TryHalf(n, out h); return h; }).Sum()", numbers.Select(n => { int h; TryHalf(n, out h); return h; }).Sum());
        Add("numbers.Select(n => { int t = n; AddTo(ref t, 1); return t; }).Sum()", numbers.Select(n => { int t = n; AddTo(ref t, 1); return t; }).Sum());
        Add("numbers.Select(n => (n, 1, 2, 3, 4, 5, 6, 7)).First().Item8", numbers.Select(n => (n, 1, 2, 3, 4, 5, 6, 7)).First().Item8);
        Add("numbers.Select(n => (n, 1, 2, 3, 4, 5, 6, 7, 8, 9)).Last().Item10", numbers.Select(n => (n, 1, 2, 3, 4, 5, 6, 7, 8, 9)).Last().Item10);
        Add("numbers.Select(n => new System.Text.StringBuilder(capacity: n).Capacity).Sum()", numbers.Select(n => new System.Text.StringBuilder(capacity: n).Capacity).Sum());
        // new の引数や初期化子の中の、ラムダの引数を使うラムダ式
        Add("numbers.Select(n => new Holder { Step = x => x + n }.Step(1)).Sum()", numbers.Select(n => new Holder { Step = x => x + n }.Step(1)).Sum());
        Add("numbers.Select(n => new Func<int, int>(x => x + n)(1)).Sum()", numbers.Select(n => new Func<int, int>(x => x + n)(1)).Sum());
        Add("numbers.Select(n => new Lazy<int>(() => n * 2).Value).Sum()", numbers.Select(n => new Lazy<int>(() => n * 2).Value).Sum());
        Add("numbers.Select(n => new List<Func<int, int>> { x => x * n }[0](2)).Sum()", numbers.Select(n => new List<Func<int, int>> { x => x * n }[0](2)).Sum());
        // タプルでない型の位置のパターン（Deconstruct）
        Add("point is (1, 2)", point is (1, 2));
        Add("point is (_, > 1)", point is (_, > 1));
        Add("point switch { (1, var y) => y, _ => 0 }", point switch { (1, var y) => y, _ => 0 });
        Add("numbers.Count(n => point is (1, var y) && y < n)", numbers.Count(n => point is (1, var y) && y < n));
        // async ラムダの中で投げた例外は、返す Task の中に入る
        Add("((Func<Task<int>>)(async () => throw new InvalidOperationException()))().IsFaulted", ((Func<Task<int>>)(async () => throw new InvalidOperationException()))().IsFaulted);
        Add("((Func<Task<int>>)(async () => throw new InvalidOperationException(\"x\")))().Exception.InnerException.Message", ((Func<Task<int>>)(async () => throw new InvalidOperationException("x")))().Exception.InnerException.Message);
        Add("((Func<int, Task<int>>)(async n => n > 5 ? throw new ArgumentException() : n))(3).Result", ((Func<int, Task<int>>)(async n => n > 5 ? throw new ArgumentException() : n))(3).Result);
        Add("((Func<Task>)(async () => throw new InvalidOperationException()))().IsFaulted", ((Func<Task>)(async () => throw new InvalidOperationException()))().IsFaulted);
        // dynamic のメンバー参照と呼び出し
        dynamic dyn = words;
        Add("((dynamic)words).Count", ((dynamic)words).Count);
        Add("((dynamic)s).Length", ((dynamic)s).Length);
        Add("((dynamic)s).ToUpper()", ((dynamic)s).ToUpper());
        Add("((dynamic)words)[1]", ((dynamic)words)[1]);
        Add("((dynamic)s).Substring(1, 2)", ((dynamic)s).Substring(1, 2));
        Add("dyn.Count", dyn.Count);
        Add("dyn[0].Length", dyn[0].Length);
        // ラムダ式の中で、ローカル変数の初期値、戻り値、代入の右辺になるラムダ式
        Add("numbers.Select(n => { Func<int, int> add = x => x + n; return add(1); }).Sum()", numbers.Select(n => { Func<int, int> add = x => x + n; return add(1); }).Sum());
        Add("numbers.Select(n => { var twice2 = (int x) => x * 2; return twice2(n); }).Sum()", numbers.Select(n => { var twice2 = (int x) => x * 2; return twice2(n); }).Sum());
        Add("((Func<int, Func<int, int>>)(p => q => p + q))(2)(3)", ((Func<int, Func<int, int>>)(p => q => p + q))(2)(3));
        Add("new Func<int, Func<int, int>>(x => { return y => x * y; })(4)(5)", new Func<int, Func<int, int>>(x => { return y => x * y; })(4)(5));
        Add("numbers.Select(n => { Func<int, int> f = null; f = x => x + n; return f(1); }).Sum()", numbers.Select(n => { Func<int, int> f = null; f = x => x + n; return f(1); }).Sum());        Add("((Func<int, Holder>)(n => new() { Count = n }))(3).Count", ((Func<int, Holder>)(n => new() { Count = n }))(3).Count);
        Add("new Func<Holder, int>(h => { h.Step = Doubled; return h.Step(4); })(new Holder())", new Func<Holder, int>(h => { h.Step = Doubled; return h.Step(4); })(new Holder()));        GC.KeepAlive(new object[] { triple, dyn });
        GC.KeepAlive(new object[] { mixed, maybes, pairs, gaps, rows, inc });
        GC.KeepAlive(spare);
#if NET10_0_OR_GREATER
        // C# 14 (.NET 10): 拡張メンバー、nameof の型引数の無いジェネリック型、null 条件の代入
        Add("words.IsEmptyNow", words.IsEmptyNow);
        Add("s.Vowels", s.Vowels);
        Add("words.Twice(4)", words.Twice(4));
        Add("IEnumerable<int>.Answer", IEnumerable<int>.Answer);
        Add("IEnumerable<int>.Triple(4)", IEnumerable<int>.Triple(4));
        Add("string.Shout(\"a\")", string.Shout("a"));
        Tally tally = new Tally();
        Add("tally = new Tally()", tally = new Tally());
        Add("tally += 2", tally += 2);
        Add("tally.Total", tally.Total);
        Add("MemoryExtensions.Contains(\"abc\", 'b')", MemoryExtensions.Contains("abc", 'b'));
        Add("MemoryExtensions.IndexOf(numbers, 20)", MemoryExtensions.IndexOf(numbers, 20));
        GC.KeepAlive(tally);
        Add("nameof(List<>)", nameof(List<>));
        Add("nameof(Dictionary<,>)", nameof(Dictionary<,>));
        Add("holder?.Count = 9", holder?.Count = 9);
        Add("none?.Length.ToString()", none?.Length.ToString());
#endif
#if NET11_0_OR_GREATER
        // C# 15 (.NET 11): 共用体、拡張インデクサー、インターフェイスの静的メンバー
        Pet pet = new Dog("Rex");
        IEnumerable<int> sequence = numbers;
        Add("pet is Dog", pet is Dog);
        Add("pet is Cat", pet is Cat);
        Add("pet switch { Dog dog => dog.Name, Cat cat => cat.Name }", pet switch { Dog dog => dog.Name, Cat cat => cat.Name });
        Add("sequence[1]", sequence[1]);
        Add("IMeasurement.ToMeters(250)", IMeasurement.ToMeters(250));
        Add("NewList([with(capacity: 8), 1, 2]).Capacity", NewList([with(capacity: 8), 1, 2]).Capacity);
        Add("numbers.Select(n => NewList([with(capacity: n), 1]).Capacity).Sum()", numbers.Select(n => NewList([with(capacity: n), 1]).Capacity).Sum());
        GC.KeepAlive(new object[] { pet, sequence });
#endif
#if NET
        // 新しいランタイムの型が要るもの (.NET Framework には無い)
        Add("numbers[^1]", numbers[^1]);
        Add("numbers[1..].Length", numbers[1..].Length);
        Add("s[1..3]", s[1..3]);
        Add("numbers is [10, .., 30]", numbers is [10, .., 30]);
        Add("numbers is [_, 20, _]", numbers is [_, 20, _]);
        Add("jagged.Count(r => r is [_, 3])", jagged.Count(r => r is [_, 3]));
        Add("words.Select(w => w[^1]).First()", words.Select(w => w[^1]).First());
        Add("words.Select(w => w[1..]).First()", words.Select(w => w[1..]).First());
        Add("numbers.Select(n => numbers[^1] - n).Sum()", numbers.Select(n => numbers[^1] - n).Sum());
        Add("words.Select(w => w[..^1].Length).Sum()", words.Select(w => w[..^1].Length).Sum());
        Add("numbers.Select(n => new Pair(n, 1) with { B = n * 2 }).Last().B", numbers.Select(n => new Pair(n, 1) with { B = n * 2 }).Last().B);
        Add("numbers.Select(n => { (int p, int q) = new Pair(n, 2); return p * q; }).Sum()", numbers.Select(n => { (int p, int q) = new Pair(n, 2); return p * q; }).Sum());
        Add("new Pair(1, 2) is (1, var bb) && bb == 2", new Pair(1, 2) is (1, var bb) && bb == 2);
        Add("int.Parse(\"12\".AsSpan())", int.Parse("12".AsSpan()));
        Add("int.Parse((ReadOnlySpan<char>)\"34\")", int.Parse((ReadOnlySpan<char>)"34"));
        Add("\"abcd\".AsSpan(1, 2).ToString()", "abcd".AsSpan(1, 2).ToString());
        Add("\"abcd\".AsSpan().IndexOf('c')", "abcd".AsSpan().IndexOf('c'));
        Add("jagged.Count(r => r is [.., > 0])", jagged.Count(r => r is [.., > 0]));
        Add("words.Count(w => w is ['t', .., 'o'])", words.Count(w => w is ['t', .., 'o']));
        Add("(new Pair(1, 2) with { A = 5 }).A", (new Pair(1, 2) with { A = 5 }).A);
        Add("new Pair(1, 2) == new Pair(1, 2)", new Pair(1, 2) == new Pair(1, 2));
        Add("s.Contains('l')", s.Contains('l'));
        Add("(q1, r1) = new Pair(4, 5)", (q1, r1) = new Pair(4, 5));
        Add("q1 + r1", q1 + r1);
        Add("new Func<Dictionary<string, int>, int>(m => { int total = 0; foreach (var (k, v) in m) total += v + k.Length; return total; })(map)", new Func<Dictionary<string, int>, int>(m => { int total = 0; foreach (var (k, v) in m) total += v + k.Length; return total; })(map));
        Add("await new ValueTask<int>(5)", new ValueTask<int>(5).GetAwaiter().GetResult());
        Add("((Func<ValueTask<int>>)(async () => await LaterAsync(2)))().Result", ((Func<ValueTask<int>>)(async () => await LaterAsync(2)))().Result);
#endif
        GC.KeepAlive(new object[] { grid, jagged, money, indexed, twice, n1, n2, n3, text, pair, comparable, holder });
        GC.KeepAlive(new object[] { a, b, big, d, f, yes, no, s, none, c, small, u, numbers, words, map, shade, boxed, price, maybe, nothing, point });
        // ポインター（unsafe のコード）。メソッド全体は unsafe にできない（async のラムダを含むため）ので、終わりだけを unsafe の
        // ブロックにし、止める行もその中に置く。ラムダに取り込まれた変数（a、point）は番地を取れないので、別の変数を使う。
        unsafe
        {
            int target = 7;
            var spot = new Point { X = 1, Y = 2 };
            int* pa = &target;
            Point* pp = &spot;
            int* stack = stackalloc int[] { 10, 20, 30 };
            byte* pb = (byte*)pa;
            void* pv = null;
            Add("*pa", *pa);
            Add("*pa + 1", *pa + 1);
            Add("pa[0]", pa[0]);
            Add("pp->X", pp->X);
            Add("(*pp).Y", (*pp).Y);
            Add("pp->Sum()", pp->Sum());
            Add("stack[1]", stack[1]);
            Add("*(stack + 2)", *(stack + 2));
            Add("(stack + 2)[-1]", (stack + 2)[-1]);
            Add("&stack[2] - stack", &stack[2] - stack);
            Add("stack + 1 == &stack[1]", stack + 1 == &stack[1]);
            Add("stack < stack + 1", stack < stack + 1);
            Add("*&target", *&target);
            Add("&target == pa", &target == pa);
            Add("pb[0] + pb[1]", pb[0] + pb[1]);
            Add("pv == null", pv == null);
            Add("*(int*)pb", *(int*)pb);
            Add("(long)(stack + 1) - (long)stack", (long)(stack + 1) - (long)stack);
            // 配列の型へのキャスト（SQL ログがパラメーターの byte[] を読む）。
        Add("((byte[])boxedBytes).Length", ((byte[])boxedBytes).Length);
        Add("((byte[])boxedBytes)[1]", ((byte[])boxedBytes)[1]);
        Add("((int[])boxedNumbers)[2]", ((int[])boxedNumbers)[2]);
        return Texts.Count; // @conformance
        }
    }

    // ---- 文法の棚卸しで使う部品 ----
    public static int Shared = 4;

    static Task<int> LaterAsync(int value)
    {
        return Task.FromResult(value * 2);
    }

    // 呼んだ時点では終わっていない Task（終わらせるのは、ほかのスレッド）。
    static async Task<string> SlowAsync()
    {
        await Task.Delay(30).ConfigureAwait(false);
        return "done";
    }

    static int RunTask(Func<int, Task<int>> work)
    {
        return work(3).GetAwaiter().GetResult();
    }

    static bool RunPlain(Func<Task> work)
    {
        work().GetAwaiter().GetResult();
        return true;
    }

    static T Echo<T>(T value)
    {
        return value;
    }

    static int Apply(Transform transform, int value)
    {
        return transform(value);
    }

    static int Doubled(int value)
    {
        return value * 2;
    }

    static void AddTo(ref int target, int amount)
    {
        target += amount;
    }

    static int Total(params int[] values)
    {
        int sum = 0;
        foreach (var value in values) sum += value;
        return sum;
    }

    static string Join(string a, string b = "-", int times = 1)
    {
        string result = a;
        for (int i = 0; i < times; i++) result += b;
        return result;
    }

    static int Measure(Holder holder)
    {
        return holder == null ? -1 : holder.Count;
    }

    static int Sum(List<int> items)
    {
        int sum = 0;
        foreach (var item in items) sum += item;
        return sum;
    }

    static int SumArray(int[] items)
    {
        return Sum(new List<int>(items));
    }

    static int Size(IEnumerable<int> items)
    {
        return System.Linq.Enumerable.Count(items);
    }

    static List<int> NewList(List<int> items)
    {
        return items;
    }

    static bool TryHalf(int value, out int half)
    {
        half = value / 2;
        return value % 2 == 0;
    }
}

// 利用者が定義した演算子と変換。
struct Money
{
    public int Amount;

    public Money(int amount)
    {
        Amount = amount;
    }

    public static Money operator +(Money a, Money b)
    {
        return new Money(a.Amount + b.Amount);
    }

    public static Money operator -(Money a)
    {
        return new Money(-a.Amount);
    }

    public static bool operator ==(Money a, Money b)
    {
        return a.Amount == b.Amount;
    }

    public static bool operator !=(Money a, Money b)
    {
        return a.Amount != b.Amount;
    }

    public static implicit operator Money(int amount)
    {
        return new Money(amount);
    }

    public static explicit operator int(Money money)
    {
        return money.Amount;
    }

    public override bool Equals(object other)
    {
        return other is Money money && money.Amount == Amount;
    }

    public override int GetHashCode()
    {
        return Amount;
    }
}

// ジェネリック型の静的メンバーの試験用（型引数ごとに、別の静的フィールドを持つ）。
static class Holder<T>
{
    public static int Shared = typeof(T) == typeof(int) ? 11 : 22;

    public static string Make(T value)
    {
        return "<" + value + ">";
    }
}

// 代入先になるプロパティとフィールド。
class Holder
{
    public string Name { get; set; }
    public int Count;
    public List<int> Items = new List<int>();
    public Func<int, int> Step;
}

// 利用者が定義したインデクサー。
class Indexed
{
    public int this[int index]
    {
        get { return index * 10; }
    }

    public string this[string key, int number]
    {
        get { return key + number; }
    }
}

static class TextExtensions
{
    public static string Twice(this string text)
    {
        return text + text;
    }
}

#if NET
record Pair(int A, int B);
#endif

#if NET10_0_OR_GREATER
// C# 14 の拡張メンバー（extension ブロック）の試験用。
static class SequenceExtensions
{
    extension<T>(IEnumerable<T> source)
    {
        public bool IsEmptyNow => !source.Any();

        public int Twice(int value) => value * 2;
    }

    extension(string text)
    {
        public int Vowels => text.Count(c => "aeiou".IndexOf(c) >= 0);
    }

    // 静的な拡張メンバー（`IEnumerable<int>.Answer`、`string.Shout("a")`）。
    extension(IEnumerable<int>)
    {
        public static int Answer => 42;

        public static int Triple(int value) => value * 3;
    }

    extension(string)
    {
        public static string Shout(string text) => text.ToUpper() + "!";
    }
}

// C# 14 の、利用者が定義した複合代入の演算子。
class Tally
{
    public int Total;

    public void operator +=(int amount)
    {
        Total += amount * 10;
    }

    public override string ToString()
    {
        return "Tally " + Total;
    }
}
#endif

#if NET11_0_OR_GREATER
// C# 15 の共用体、拡張インデクサー、インターフェイスの静的メンバーの試験用。
public record class Cat(string Name);
public record class Dog(string Name);
public union Pet(Cat, Dog);

static class IndexerExtensions
{
    extension(IEnumerable<int> sequence)
    {
        public int this[int index] => sequence.ElementAt(index);
    }
}

public interface IMeasurement
{
    public static double ToMeters(double centimeters) => centimeters / 100;
}
#endif

/// ネイティブ境界の表示の試験用: マネージド → user32!EnumWindows → マネージドのコールバック。
static class NativeCallback
{
    delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    public static int Run()
    {
        int seen = 0;
        EnumWindows((window, parameter) => Visit(window, ref seen), IntPtr.Zero);
        return seen > 0 ? 7 : 1;
    }

    static bool Visit(IntPtr window, ref int seen)
    {
        seen++;
        return false; // @native-callback
    }
}

/// 書き込みの見張りの試験用 (D-093)。
class WatchedOrder
{
    public int Status;

    public void Touch() { }

    public static void SetByRef(ref int target, int value)
    {
        target = value; // @data-ref
    }
}
