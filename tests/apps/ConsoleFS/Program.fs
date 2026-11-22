// スモーク試験の対象 (F#)。デバッガーは IL と PDB を見るので、言語が違っても同じように止まり、同じように値を読めること。
// 行の終わりの「// @名前」は、試験がブレークポイントを置く目印。
module Program

open System
open System.Collections.Generic
open System.Threading.Tasks

type Shape =
    | Circle of radius: float
    | Rect of width: float * height: float

type Person = { Name: string; Age: int }

[<Struct>]
type Point = { X: float; Y: float }

let area shape =
    match shape with
    | Circle r -> Math.PI * r * r
    | Rect(w, h) -> w * h // @fs-match

let twice x =
    let result = x * 2 // @fs-twice
    result

// 式の適合試験 (tests/apps/Console の Conformance と同じ仕組み)。C# の式の文字列と、同じ意味の値を F# で計算したものを並べる。
// F# で書かれた対象を止めて、C# の式で F# の値 (list、option、レコード、判別共用体など) に触れることを確かめる。
// 書き方は、実際の F# のコードで多いもの (tests/corpus/languages.ps1 の報告) から選んだ。
module Conformance =
    let Texts = List<string>()
    let Values = List<obj>()
    let Lifted = List<bool>()

    let add (text: string) (value: 'T) =
        Texts.Add text
        Values.Add(box value)
        Lifted.Add(not (isNull (Nullable.GetUnderlyingType typeof<'T>)))

    let run () =
        let numbers = [ 1; 2; 3 ]
        let empty: int list = []
        let arr = [| 10; 20; 30 |]
        let maybe = Some 5
        let nothing: int option = None
        let vmaybe = ValueSome 7
        let person = { Name = "Ann"; Age = 30 }
        let shape = Rect(2.0, 3.0)
        let circle = Circle 1.5
        let ok: Result<int, string> = Ok 4
        let err: Result<int, string> = Error "bad"
        let map = Map.ofList [ ("a", 1); ("b", 2) ]
        let set = Set.ofList [ 3; 1; 2 ]
        let ra = ResizeArray [ 1; 2 ]
        let dict = Dictionary<string, int>()
        dict.["k"] <- 9
        let pair = (1, "x")
        let spair = struct (2, "y")
        let anon = {| A = 1; B = "b" |}
        let mutable counter = 3
        counter <- counter + 0
        let cell = ref 10
        let lz = lazy (6 * 7)
        let sq = seq { 1..3 }
        let pending = Task.FromResult 5
        let pt = { X = 1.0; Y = 2.0 }
        // list (FSharpList)
        add "numbers.Head" numbers.Head
        add "numbers.Tail.Head" numbers.Tail.Head
        add "numbers.Length" numbers.Length
        add "numbers.IsEmpty" numbers.IsEmpty
        add "empty.IsEmpty" empty.IsEmpty
        add "numbers[1]" numbers.[1]
        add "numbers.Sum()" (Seq.sum numbers)
        add "numbers.Where(x => x > 1).Count()" (numbers |> Seq.filter (fun x -> x > 1) |> Seq.length)
        add "numbers.Select(x => x * 2).Last()" (numbers |> Seq.map (fun x -> x * 2) |> Seq.last)
        add "numbers.Count(x => x > counter - 2)" (let limit = counter in numbers |> Seq.filter (fun x -> x > limit - 2) |> Seq.length)
        add "numbers.Select(x => twice(x)).Sum()" (numbers |> Seq.sumBy twice)
        // 配列
        add "arr.Length" arr.Length
        add "arr[2]" arr.[2]
        add "arr.Sum()" (Array.sum arr)
        // option / voption
        add "maybe.Value" maybe.Value
        add "maybe != null" (not (obj.ReferenceEquals(maybe, null)))
        add "nothing == null" (obj.ReferenceEquals(nothing, null))
        add "vmaybe.Value" vmaybe.Value
        add "vmaybe.IsSome" vmaybe.IsSome
        add "maybe.Value * person.Age" (maybe.Value * person.Age)
        // レコード、構造体のレコード、匿名レコード
        add "person.Name" person.Name
        add "person.Age + 1" (person.Age + 1)
        add "person.Equals(person)" (person.Equals(person))
        add "pt.X + pt.Y" (pt.X + pt.Y)
        add "anon.A" anon.A
        add "anon.B" anon.B
        // 判別共用体、Result
        add "shape.IsRect" (match shape with Rect _ -> true | _ -> false)
        add "shape.IsCircle" (match shape with Circle _ -> true | _ -> false)
        add "shape.Tag" (match shape with Circle _ -> 0 | Rect _ -> 1)
        add "circle.IsCircle" (match circle with Circle _ -> true | _ -> false)
        add "ok.ResultValue" (match ok with Ok v -> v | Error _ -> 0)
        add "err.ErrorValue" (match err with Error e -> e | Ok _ -> "")
        add "ok.Tag" (match ok with Ok _ -> 0 | Error _ -> 1)
        // Map、Set、ResizeArray、Dictionary
        add "map[\"a\"]" map.["a"]
        add "map.Count" map.Count
        add "map.ContainsKey(\"b\")" (map.ContainsKey "b")
        add "set.Count" set.Count
        add "set.Contains(2)" (set.Contains 2)
        add "set.MinimumElement" set.MinimumElement
        add "ra.Count" ra.Count
        add "ra[1]" ra.[1]
        add "dict[\"k\"]" dict.["k"]
        // タプル、mutable、参照セル、lazy、seq、Task
        add "pair.Item1" (fst pair)
        add "pair.Item2" (snd pair)
        add "spair.Item1" (let struct (a, _) = spair in a)
        add "counter + 1" (counter + 1)
        add "cell.Value" cell.Value
        add "lz.Value" lz.Value
        add "sq.Count()" (Seq.length sq)
        add "pending.Result" pending.Result
        // モジュールの関数
        add "twice(4)" (twice 4)
        add "Program.twice(5)" (twice 5)
        GC.KeepAlive [| box numbers; box empty; box arr; box maybe; box nothing; box vmaybe; box person; box shape; box circle; box ok; box err; box map; box set; box ra; box dict; box pair; box spair; box anon; box cell; box lz; box sq; box pending; box pt |]
        Texts.Count // @conformance

[<EntryPoint>]
let main args =
    if args.Length > 0 && args.[0] = "conformance" then
        Conformance.run () |> ignore
        0
    else
        let person = { Name = "Ann"; Age = 30 }
        let numbers = [ 1; 2; 3 ]
        let total = numbers |> List.sum
        let shape = Rect(2.0, 3.0)
        let a = area shape // @fs-call
        let doubled = twice total
        let maybe = Some 5
        printfn "%s %d %f %d %A" person.Name total a doubled maybe // @fs-end
        0
