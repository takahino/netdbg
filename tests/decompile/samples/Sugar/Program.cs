// 逆コンパイルの確かめ（tests/decompile/roundtrip.ps1）の見本: 段階 3 の書き方の復元。
// 逆コンパイルした C# をコンパイルし直して動かし、出力がこのプログラムと同じになることを確かめる。
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;

namespace Samples
{
    // System.IO.ErrorEventArgs と名前が重なる型（using System.IO と一緒に使う）。
    public class ErrorEventArgs : EventArgs
    {
        public string Message { get; }

        public ErrorEventArgs(string message)
        {
            Message = message;
        }
    }

    public class Account
    {
        public string Owner { get; set; }

        public decimal Balance { get; private set; }

        public event EventHandler<ErrorEventArgs> Failed;

        public event EventHandler Changed
        {
            add { handlers.Add(value); }
            remove { handlers.Remove(value); }
        }

        private readonly List<EventHandler> handlers = new List<EventHandler>();

        public Account(string owner)
        {
            Owner = owner;
        }

        public void Deposit(decimal amount)
        {
            if (amount <= 0)
            {
                Failed?.Invoke(this, new ErrorEventArgs("bad amount"));
                return;
            }
            Balance += amount;
            foreach (EventHandler handler in handlers)
            {
                handler(this, EventArgs.Empty);
            }
        }
    }

    // ラムダ: 何も捕まえないもの (<>c に入る) と、this を使うもの。
    public class Holder
    {
        private int scale = 5;

        private static int Apply(Func<int, int> f, int value)
        {
            return f(value);
        }

        public string Run()
        {
            int total = Apply(x => x * 3 + 1, 4);
            IEnumerable<int> picked = new List<int> { 3, 1, 2 }.Where(n => n > 1).Select(n => n * scale);
            Action log = () => Console.WriteLine("log");
            log();
            return total + ":" + string.Join(",", picked);
        }
    }

    public sealed class Resource : IDisposable
    {
        public static int Disposed;

        public void Dispose()
        {
            Disposed++;
        }
    }

    public static class Program
    {
        private static readonly object Gate = new object();
        private static int counter;

        private static string Describe(int value)
        {
            switch (value)
            {
                case 1:
                    return "one";
                case 2:
                    counter += 10;
                    break;
                case 5:
                case 6:
                    counter++;
                    break;
                default:
                    counter--;
                    break;
            }
            return "n" + counter;
        }

        private static int Total(List<int> values)
        {
            int total = 0;
            foreach (int value in values)
            {
                total += value;
            }
            return total;
        }

        private static string Pick(string first, string fallback, bool flag)
        {
            string chosen = first ?? fallback;
            return flag ? chosen.ToUpper() : chosen;
        }

        // 分かれた道で、out の引数のアドレスを評価スタックに積んだまま進む（ref の一時変数）。
        private static bool Join(object a, object b, out string result)
        {
            result = a?.ToString() + b?.ToString();
            return true;
        }

        private static int Apply(Func<int, int> f, int value)
        {
            return f(value);
        }



        private static int Locked()
        {
            lock (Gate)
            {
                counter <<= 1;
                return counter;
            }
        }

        private static int Used()
        {
            using (Resource resource = new Resource())
            {
                counter ^= 3;
            }
            return Resource.Disposed;
        }

        // 配列を dup したまま ?: を通る連結（分かれ道で配列を持ち回す一時変数。5 つ以上の連結は string.Concat(string[])）。
        static string Greet(string name, bool agree, string color)
        {
            string greeting = "Hello, " + name + (agree ? "!" : "") + " " + color;
            return greeting.Trim();
        }
        public static int Main()
        {
            Console.WriteLine(Describe(1) + Describe(2) + Describe(5) + Describe(9));
            Console.WriteLine(Total(new List<int> { 1, 2, 3, 4 }));
            Console.WriteLine(Pick(null, "fb", true) + Pick("a", "b", false));
            Console.WriteLine(new Holder().Run());
            Join(null, 7, out string joined);
            Console.WriteLine(joined);
            Console.WriteLine(Locked());
            Console.WriteLine(Used() + " " + counter);
            Account account = new Account("ann");
            int changed = 0;
            account.Changed += Count;
            account.Failed += Report;
            account.Deposit(5m);
            account.Deposit(-1m);
            account.Owner += "!";
            Console.WriteLine(account.Owner + " " + account.Balance + " " + changed);
            StringBuilder builder = new StringBuilder();
            for (int i = 0; i < 3; i++)
            {
                builder.Append(i).Append(',');
            }
            Console.WriteLine(builder.ToString());
            Console.WriteLine(Path.GetFileName("a/b.txt"));
            Console.WriteLine(Greet("ann", true, "red") + "|" + Greet("bob", false, "blue"));
            return 0;
        }

        private static void Count(object sender, EventArgs e)
        {
            Console.WriteLine("changed");
        }

        private static void Report(object sender, ErrorEventArgs e)
        {
            Console.WriteLine("failed: " + e.Message);
        }
    }
}
