// 逆コンパイルの確かめ（tests/decompile/roundtrip.ps1）の見本: 段階 2 の式と制御構造。
// 逆コンパイルした C# をコンパイルし直して動かし、出力がこのプログラムと同じになることを確かめる。
using System;
using System.Collections.Generic;
using System.Text;

namespace Samples
{
    public enum Color : byte
    {
        Red = 1,
        Green = 2,
        Blue = 4,
    }

    public struct Point
    {
        public int X;
        public int Y;

        public Point(int x, int y)
        {
            X = x;
            Y = y;
        }

        public int Manhattan()
        {
            return Math.Abs(X) + Math.Abs(Y);
        }
    }

    public class Counter
    {
        private int count;

        public int Count
        {
            get { return count; }
            set { count = value; }
        }

        public void Add(int amount)
        {
            count += amount;
        }
    }

    public static class Program
    {
        private static readonly string[] Words = { "alpha", "beta", "gamma" };

        private static int Classify(int value)
        {
            if (value < 0)
            {
                return -1;
            }
            else if (value == 0)
            {
                return 0;
            }
            return 1;
        }

        private static int SumTo(int limit)
        {
            int total = 0;
            for (int i = 1; i <= limit; i++)
            {
                if (i % 3 == 0)
                {
                    continue;
                }
                if (total > 1000)
                {
                    break;
                }
                total += i;
            }
            return total;
        }

        private static int CountDown(int start)
        {
            int steps = 0;
            do
            {
                start -= 2;
                steps++;
            }
            while (start > 0);
            return steps;
        }

        private static string Name(int day)
        {
            switch (day)
            {
                case 0:
                    return "sun";
                case 1:
                    return "mon";
                case 2:
                case 3:
                    return "mid";
                default:
                    return "other";
            }
        }

        private static int Divide(int a, int b)
        {
            try
            {
                return a / b;
            }
            catch (DivideByZeroException)
            {
                return -1;
            }
            finally
            {
                Console.WriteLine("divided " + a);
            }
        }

        private static long Nested(int n)
        {
            long product = 1;
            for (int i = 1; i <= n; i++)
            {
                int j = 0;
                while (j < i)
                {
                    product += i * j;
                    j++;
                }
            }
            return product;
        }

        private static bool Flags(int value, Color color)
        {
            bool even = value % 2 == 0;
            bool blue = (color & Color.Blue) != 0;
            return even != blue;
        }

        private static double Average(int[] values)
        {
            if (values.Length == 0)
            {
                return 0.0;
            }
            double sum = 0;
            for (int i = 0; i < values.Length; i++)
            {
                sum += values[i];
            }
            return sum / values.Length;
        }

        private static string Join(List<string> parts, char separator)
        {
            StringBuilder builder = new StringBuilder();
            for (int i = 0; i < parts.Count; i++)
            {
                if (i > 0)
                {
                    builder.Append(separator);
                }
                builder.Append(parts[i]);
            }
            return builder.ToString();
        }

        private static int Twice(int x)
        {
            return x * 2;
        }

        private static uint Unsigned(uint a, uint b)
        {
            return a / b + (a % b);
        }

        public static int Main()
        {
            Console.WriteLine(Classify(-5) + " " + Classify(0) + " " + Classify(7));
            Console.WriteLine(SumTo(20));
            Console.WriteLine(CountDown(9));
            Console.WriteLine(Name(0) + Name(2) + Name(9));
            Console.WriteLine(Divide(10, 2) + " " + Divide(1, 0));
            Console.WriteLine(Nested(5));
            Console.WriteLine(Flags(4, Color.Blue) + " " + Flags(3, Color.Red));
            Console.WriteLine(Average(new[] { 1, 2, 3, 4 }));
            Console.WriteLine(Join(new List<string>(Words), '-'));
            Point point = new Point(3, -4);
            Console.WriteLine(point.Manhattan());
            Counter counter = new Counter();
            counter.Count = 5;
            counter.Add(3);
            Console.WriteLine(counter.Count);
            Func<int, int> twice = Twice;
            Console.WriteLine(twice(21));
            Console.WriteLine(Unsigned(4000000000u, 7u));
            char letter = 'x';
            Console.WriteLine(letter == 'x' ? "x" : "?");
            return 0;
        }
    }
}
