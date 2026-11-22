' スモーク試験の対象 (VB.NET)。デバッガーは IL と PDB を見るので、言語が違っても同じように止まり、同じように値を読めること。
' VB のコンパイラが作る名前 (クロージャの $VB$Local_ など) を、もとの変数の名前で見せられること。
' 行の終わりの「' @名前」は、試験がブレークポイントを置く目印。

Imports System
Imports System.Collections.Generic
Imports System.Linq
Imports System.Threading.Tasks

Public Enum Tint
    Pale
    Dark
End Enum

Public Structure PointV
    Public X As Integer
    Public Y As Integer
End Structure

Public Class Notifier
    Public Property Name As String = "n"
    Public Event Changed As EventHandler
    Public Sub Raise()
        RaiseEvent Changed(Me, EventArgs.Empty)
    End Sub
End Class

Public Class Order
    Public Shared Created As Integer = 7
    Public Property Name As String
    Public Property Count As Integer
    Private ReadOnly _tags As New List(Of String)
    Public WithEvents Source As New Notifier()

    Public Sub New(name As String, count As Integer)
        Me.Name = name
        Me.Count = count
        _tags.Add("new")
    End Sub

    Public Function Total(price As Double) As Double
        Dim result = price * Count ' @vb-method
        Return result
    End Function

    Public Function Describe(Optional times As Integer = 1, Optional mark As String = ":") As String
        Return Name & mark & (Count * times).ToString()
    End Function

    Private Sub OnChanged(sender As Object, e As EventArgs) Handles Source.Changed
        Count += 1
    End Sub
End Class

' 式の適合試験 (tests/apps/Console の Conformance と同じ仕組み)。C# の式の文字列と、同じ意味の値を VB で計算したものを並べる。
' VB で書かれた対象を止めて、C# の式で VB の値 (Nothing、WithEvents、Optional の引数、Decimal、Date、Structure など) に
' 触れることを確かめる。書き方は、実際の VB のコードで多いもの (tests/corpus/languages.ps1 の報告) から選んだ。
Public Module Conformance
    Public ReadOnly Texts As New List(Of String)
    Public ReadOnly Values As New List(Of Object)
    Public ReadOnly Lifted As New List(Of Boolean)

    Private Sub Add(Of T)(text As String, value As T)
        Texts.Add(text)
        Values.Add(value)
        Lifted.Add(Nullable.GetUnderlyingType(GetType(T)) IsNot Nothing)
    End Sub

    Public Function Run() As Integer
        Dim order = New Order("A-1", 4)
        Dim nothingOrder As Order = Nothing
        Dim items = New List(Of Integer) From {1, 2, 3}
        Dim table = New Dictionary(Of String, Integer) From {{"k", 9}}
        Dim numbers() As Integer = {10, 20, 30}
        Dim grid(1, 2) As Integer
        grid(1, 2) = 5
        Dim price As Decimal = 12.5D
        Dim day As Date = New Date(2024, 2, 29)
        Dim maybe As Integer? = 5
        Dim none As Integer? = Nothing
        Dim boxed As Object = 42
        Dim text As Object = "t"
        Dim pair = (1, "x")
        Dim tup = Tuple.Create(2, "y")
        Dim shade = Tint.Dark
        Dim anon = New With {.A = 1, .B = "b"}
        Dim pt As New PointV With {.X = 1, .Y = 2}
        Dim col As New Microsoft.VisualBasic.Collection()
        col.Add("first")
        Dim inc As Func(Of Integer, Integer) = Function(x) x + 1
        ' オブジェクト、Nothing、WithEvents、Shared、Optional の引数
        Add("order.Name", order.Name)
        Add("order.Count * 2", order.Count * 2)
        Add("order.Total(2.5)", order.Total(2.5))
        Add("nothingOrder == null", nothingOrder Is Nothing)
        Add("order.Source.Name", order.Source.Name)
        Add("Order.Created + 1", Order.Created + 1)
        Add("order.Describe()", order.Describe())
        Add("order.Describe(3)", order.Describe(3))
        Add("order.Describe(mark: ""="")", order.Describe(mark:="="))
        ' List(Of …)、Dictionary(Of …)、配列
        Add("items.Count", items.Count)
        Add("items[1]", items(1))
        Add("items.Sum()", items.Sum())
        Add("items.Where(x => x > 1).Count()", items.Where(Function(x) x > 1).Count())
        Add("items.Select(x => x * 10).Last()", items.Select(Function(x) x * 10).Last())
        Add("table[""k""]", table("k"))
        Add("numbers.Length", numbers.Length)
        Add("numbers[2]", numbers(2))
        Add("grid.GetLength(1)", grid.GetLength(1))
        Add("grid[1, 2]", grid(1, 2))
        ' Decimal、Date、null 許容、Object
        Add("price * 2", price * 2)
        Add("day.Year", day.Year)
        Add("day.DayOfWeek", day.DayOfWeek)
        Add("maybe.Value", maybe.Value)
        Add("none.HasValue", none.HasValue)
        Add("maybe ?? 0", If(maybe, 0))
        Add("(int)boxed + 1", CInt(boxed) + 1)
        Add("text is string", TypeOf text Is String)
        Add("boxed is int k && k > 40", TypeOf boxed Is Integer AndAlso CInt(boxed) > 40)
        Add("boxed.ToString()", boxed.ToString())
        ' タプル、列挙型、匿名型、Structure、Collection、デリゲート
        Add("pair.Item2", pair.Item2)
        Add("tup.Item1", tup.Item1)
        Add("shade", shade)
        Add("(int)shade", CInt(shade))
        Add("anon.A", anon.A)
        Add("pt.X + pt.Y", pt.X + pt.Y)
        Add("col.Count", col.Count)
        Add("inc(2)", inc(2))
        Add("(long)items[0]", CLng(items(0)))
        Add("Program.Twice(3)", Program.Twice(3))
        GC.KeepAlive(New Object() {order, nothingOrder, items, table, numbers, grid, price, day, maybe, none, boxed, text, pair, tup, shade, anon, pt, col, inc})
        Return Texts.Count ' @conformance
    End Function
End Module

Module Program
    Private counter As Integer = 3

    Function Twice(value As Integer) As Integer
        Return value * 2 ' @vb-twice
    End Function

    Async Function WaitAndAdd(a As Integer, b As Integer) As Task(Of Integer)
        Dim before = a + 1
        Await Task.Delay(10)
        Dim after = before + b ' @vb-async
        Return after
    End Function

    Function Scaled(numbers As Integer(), scale As Integer) As Integer()
        Dim offset = 1
        Dim result = numbers.Select(Function(n) n * scale + offset).ToArray()
        Return result ' @vb-closure
    End Function

    Sub Main(args As String())
        If args.Length > 0 AndAlso args(0) = "conformance" Then
            Conformance.Run()
            Return
        End If
        Dim order = New Order("A-1", 4)
        Dim numbers = New Integer() {10, 20, 30}
        Dim shade = Tint.Dark
        Dim label As String = "vb"
        Dim sum = 0
        For Each n In numbers
            sum += n ' @vb-loop
        Next
        Dim doubled = Twice(sum) ' @vb-call
        Dim total = order.Total(2.5)
        Dim grown = Scaled(numbers, 3)
        Dim awaited = WaitAndAdd(1, 2).Result
        Console.WriteLine($"{label} {sum} {doubled} {total} {awaited} {shade} {grown.Length} {counter}") ' @vb-end
    End Sub
End Module
