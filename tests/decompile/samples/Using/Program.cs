// 別アセンブリの型の using（StringWriter）は using に戻す。コンパイラは finally で
// null のとき Dispose を飛ばす。列挙子の foreach は using にしない。
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;

namespace Samples
{
    public static class Program
    {
        [MethodImpl(MethodImplOptions.NoInlining)]
        private static string Quoted()
        {
            using (StringWriter writer = new StringWriter())
            {
                writer.Write("ok");
                return writer.ToString();
            }
        }

        [MethodImpl(MethodImplOptions.NoInlining)]
        private static StringWriter Open()
        {
            return new StringWriter();
        }

        [MethodImpl(MethodImplOptions.NoInlining)]
        private static string FromCall()
        {
            using (StringWriter writer = Open())
            {
                writer.Write("call");
                return writer.ToString();
            }
        }

        [MethodImpl(MethodImplOptions.NoInlining)]
        private static int Sum(List<int> list)
        {
            int total = 0;
            foreach (int item in list)
            {
                total += item;
            }
            return total;
        }

        public static int Main()
        {
            Console.WriteLine(Quoted());
            Console.WriteLine(FromCall());
            Console.WriteLine(Sum(new List<int> { 1, 2, 3 }));
            return 0;
        }
    }
}
