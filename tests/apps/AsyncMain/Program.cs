using System.Threading.Tasks;

class Program
{
    static async Task<int> Main(string[] args)
    { // @main-open
        int before = args.Length; // @main-first
        await Task.Delay(10);
        return before + 7;
    }
}