using System;
using System.Threading.Tasks;

class Program
{
    static int Main()
    {
        return Outer().GetAwaiter().GetResult();
    }

    static async Task<int> Outer()
    {
        int value = 21; // @ra-before
        int doubled = await Inner(value); // @ra-await
        return doubled + 1; // @ra-after
    }

    static async Task<int> Inner(int n)
    {
        int seen = n; // @ra-inner
        await Task.Delay(30); // @ra-inner-await
        return seen * 2; // @ra-inner-after
    }
}
