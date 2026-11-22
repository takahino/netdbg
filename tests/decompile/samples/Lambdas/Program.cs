// 逆コンパイルの段階 3 の残り (TODO 268) の見本: 捕まえた変数を持つラムダ、文が 2 つ以上のラムダ、文字列の switch、
// 文字列の補間、呼び出す相手を先に読む形、フィールドの初期化子。出力が変わらないことと、戻した C# の形を見る。
using System;
using System.Collections.Generic;
using System.Linq;

class Counter
{
    private int count = 3;
    private string label = "counter";
    private readonly List<string> items = new List<string>();
    public static int Created = 10;

    public int Count => count;

    public void Add(string item)
    {
        items.Add(item);
        count++;
    }

    public string Describe() => $"{label}: {count} ({string.Join(",", items)})";
}

/// WinForms のデザイナーと同じ形 (捕まえた変数に、初期化子つきの new を入れる。配列を AddRange する)。
class Box
{
    public string Name { get; set; }
    public int Left { get; set; }
    public List<object> Items { get; } = new List<object>();
}

class Program
{
    static string Receivers()
    {
        var form = new Box { Name = "main", Left = 3 };
        var other = new Box { Name = "other" };
        form.Items.AddRange(new object[] { "Red", "Green" });
        Action show = () => other.Name = form.Name + "!";
        show();
        other.Items.AddRange(new object[] { form, other });
        return other.Name + form.Items.Count + other.Items.Count + form.Left;
    }

    static int Captured(int limit)
    {
        int seen = 0;
        var values = new List<int> { 5, 1, 8, 3, 9 };
        values.ForEach(v => { if (v > limit) seen++; });
        return seen;
    }

    static string MultiStatement(IEnumerable<int> values)
    {
        var parts = values.Select(v =>
        {
            int doubled = v * 2;
            string text = doubled.ToString();
            return text + "!";
        });
        return string.Join(" ", parts);
    }

    static int Classify(string name)
    {
        switch (name)
        {
            case "alpha": return 1;
            case "beta": return 2;
            case "gamma": return 3;
            case "delta": return 4;
            case "epsilon": return 5;
            case "zeta": return 6;
            case "eta": return 7;
            default: return 0;
        }
    }

    static string Interpolated(string name, int count, double ratio)
    {
        return $"{name} has {count} items ({ratio:F2}), next {count + 1}";
    }

    static int CallTarget()
    {
        var counter = new Counter();
        counter.Add("a");
        counter.Add("b");
        var list = new List<int>();
        list.AddRange(new[] { 1, 2, 3 });
        return counter.Count + list.Count;
    }

    static Func<int, int> MakeAdder(int amount)
    {
        return x => x + amount;
    }

    static void Main()
    {
        Console.WriteLine(Captured(4));
        Console.WriteLine(MultiStatement(new[] { 1, 2, 3 }));
        foreach (var name in new[] { "alpha", "gamma", "eta", "omega" })
        {
            Console.WriteLine(Classify(name));
        }
        Console.WriteLine(Interpolated("box", 3, 0.5));
        Console.WriteLine(CallTarget());
        Console.WriteLine(MakeAdder(10)(5));
        var counter = new Counter();
        counter.Add("x");
        Console.WriteLine(counter.Describe());
        Console.WriteLine(Counter.Created);
        Console.WriteLine(Receivers());
    }
}
