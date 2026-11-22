# 段階 1: 引数とローカル変数の値を読む。表記は netcoredbg に合わせてある。
function Test-Values([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-values-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'values'
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName 'インスタンスメソッドのフレーム名' ($frames[0].name -eq 'Order.Total()') "($($frames[0].name))"

        $r = Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }
        $scope = @($r.body.scopes)[0]
        Check $ArchName 'scopes が Locals を返す' ($r.success -and $scope.name -eq 'Locals' -and $scope.variablesReference -gt 0) "($($r.message))"
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        $byName = @{}
        foreach ($v in $vars) { $byName[$v.name] = $v }
        # 名前、値、型。
        $expected = @(
            @('this', '{Order}', 'Order'),
            @('prices', '{int[2]}', 'int[]'),
            @('rate', '1.5', 'double'),
            @('label', '"a\"b"', 'string'),
            @('ok', 'true', 'bool'),
            @('letter', "120 'x'", 'char'),
            @('big', '1234567890123', 'long'),
            @('boxed', '5', 'int'),
            @('missing', 'null', 'string'),
            @('names', 'Count = 1', 'System.Collections.Generic.List<string>'),
            @('grid', '{int[2, 3]}', 'int[,]'),
            @('shade', 'Dark', 'Shade'),                              # 列挙型は名前で
            @('keys', 'Alt | Shift', 'System.ConsoleModifiers'),      # ビットの組み合わせ
            @('price', '12.50', 'decimal'),
            @('maybe', '7', 'int?'),                                  # Nullable は中身か null
            @('nothing', 'null', 'int?'),
            @('point', '{Point}', 'Point'),                           # 構造体
            @('ratio', '0.25', 'float'),
            @('small', '200', 'byte'),
            @('displayed', 'd has 6 ("ok") <''Missing'' という名前の変数、メンバー、型が見つかりません。>', 'Displayed'),   # DebuggerDisplay の式 (評価できない部分はエラーを書く)
            @('sum', '30', 'int')
        )
        foreach ($e in $expected) {
            $v = $byName[$e[0]]
            Check $ArchName "変数 $($e[0]) の値と型" ($null -ne $v -and $v.value -ceq $e[1] -and $v.type -ceq $e[2]) "($($v.value) : $($v.type))"
        }
        # 日時は ToString() の結果を {…} で囲んで見せる (Visual Studio と同じ。書式は対象のカルチャ)。
        $moment = $byName['moment']
        Check $ArchName '変数 moment (DateTime) は ToString() の結果で見せる' ($moment.value -match '^\{.*2024.*\}$' -and $moment.value -ne '{System.DateTime}' -and $moment.type -eq 'System.DateTime' -and $moment.variablesReference -gt 0) "($($moment.value) : $($moment.type))"
        Check $ArchName 'this、引数、ローカル変数の順'(($vars | Select-Object -First 4 | ForEach-Object name) -join ',' -eq 'this,prices,rate,label') "($(($vars | ForEach-Object name) -join ','))"
        Check $ArchName '中身の無い値は開けない' ($byName['sum'].variablesReference -eq 0 -and $byName['missing'].variablesReference -eq 0 -and $byName['label'].variablesReference -eq 0)

        $fields = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['this'].variablesReference }).body.variables)
        $id = $fields | Where-Object { $_.name -eq 'id' }
        $name = $fields | Where-Object { $_.name -eq 'Name' }
        $statics = $fields | Where-Object { $_.name -eq 'Static members' }
        Check $ArchName 'オブジェクトのフィールド' ($id.value -eq '3' -and $id.type -eq 'int') "($(($fields | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"
        Check $ArchName '自動実装プロパティはプロパティの名前で見える' ($name.value -ceq '"tea"')
        $static = @((Send-DapRequest $s 'variables' @{ variablesReference = $statics.variablesReference }).body.variables)
        # evaluateName (その値を指す式。netcoredbg と同じ形): メンバーは 親.名前、静的メンバーは 型.名前。式として評価できる。
        $created = $static | Where-Object { $_.name -eq 'Created' }
        $evaluated = (Send-DapRequest $s 'evaluate' @{ expression = $id.evaluateName; frameId = $frames[0].id }).body.result
        Check $ArchName 'evaluateName: メンバーは 親.名前、静的メンバーは 型.名前で、評価できる' ($byName['this'].evaluateName -eq 'this' -and $id.evaluateName -eq 'this.id' -and $created.evaluateName -eq 'Order.Created' -and $evaluated -eq '3') "($($id.evaluateName), $($created.evaluateName), $evaluated)"
        Check $ArchName '静的メンバー (その型のものを宣言の順に。定数も出す。静的プロパティは getter を実行して最後に)' ((($static | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' ') -eq 'Created=1 Max=99 PerThread=5 Twice=2') "($(($static | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"

        # プロパティは、対象の getter を実行して読む。フィールドのあと、静的メンバーの前に並ぶ。引数を取るもの (インデクサー) は出さない。
        # List<T> は DebuggerTypeProxy で要素だけを見せ (D-031 の追記)、もとのメンバーは Raw View の中にある。
        $members = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['names'].variablesReference }).body.variables)
        $shown = ($members | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '
        Check $ArchName 'List<T> は DebuggerTypeProxy で要素を見せる' ($shown -ceq '[0]="x" Raw View=') "($shown)"
        $raw = $members | Where-Object { $_.name -eq 'Raw View' }
        $members = @((Send-DapRequest $s 'variables' @{ variablesReference = $raw.variablesReference }).body.variables)
        $shown = ($members | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '
        Check $ArchName 'プロパティを実行して読む (フィールドのあとに並ぶ)' ($shown -match '^_items=\{string\[4\]\} _size=1 _version=1 (_syncRoot=null )?Capacity=4 Count=1 ' -and $members[-1].name -eq 'Static members' -and -not ($members | Where-Object { $_.name -eq 'Item' })) "($shown)"
        $syncRoot = $members | Where-Object { $_.name -eq 'System.Collections.ICollection.SyncRoot' }
        # SyncRoot は、.NET ではリスト自身 (Count = 1)、.NET Framework では専用のオブジェクト ({object}) を返す。
        Check $ArchName 'プロパティの値がオブジェクトなら、さらに開ける' ($syncRoot.value -cin 'Count = 1', '{object}' -and $syncRoot.variablesReference -gt 0) "($($syncRoot.value))"
        # DebuggerBrowsable: Never は出さず (フィールドもプロパティも)、RootHidden は中身をその場に並べる。
        $members = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['shown'].variablesReference }).body.variables)
        $listed = ($members | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '
        Check $ArchName 'DebuggerBrowsable (Never、RootHidden)' ($listed -ceq 'Visible=2 [0]=3 [1]=4 Note="n" Auto="a" VisibleProperty=6') "($listed)"
        # DebuggerTypeProxy: 代わりの型の公開されたメンバーを見せ、もとのメンバーは Raw View に入れる。
        $members = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['bag'].variablesReference }).body.variables)
        $listed = ($members | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '
        Check $ArchName 'DebuggerTypeProxy (代わりの型のメンバー)' ($listed -ceq 'Count=3 [0]=1 [1]=2 [2]=3 Raw View=') "($listed)"
        $raw = $members | Where-Object { $_.name -eq 'Raw View' }
        $listed = (@((Send-DapRequest $s 'variables' @{ variablesReference = $raw.variablesReference }).body.variables) | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '
        Check $ArchName 'DebuggerTypeProxy (Raw View の中は、もとのメンバー)' ($listed -ceq 'raw={int[3]}') "($listed)"
        $items = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['prices'].variablesReference }).body.variables)
        Check $ArchName '配列の要素' (($items | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' ' -eq '[0]=10 [1]=20')
        $cells = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['grid'].variablesReference }).body.variables)
        Check $ArchName '多次元配列の添字' ($cells.Count -eq 6 -and $cells[0].name -eq '[0, 0]' -and $cells[5].name -eq '[1, 2]') "($(($cells | ForEach-Object name) -join ' '))"
        $page = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['grid'].variablesReference; start = 4; count = 1 }).body.variables)
        Check $ArchName 'start と count で要素を切り出せる' ($page.Count -eq 1 -and $page[0].name -eq '[1, 1]')

        $r = Send-DapRequest $s 'evaluate' @{ expression = 'sum'; frameId = $frames[0].id; context = 'watch' }
        Check $ArchName 'evaluate で変数を読む' ($r.success -and $r.body.result -eq '30' -and $r.body.type -eq 'int') "($($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'this'; frameId = $frames[0].id }
        Check $ArchName 'evaluate で this を読む' ($r.success -and $r.body.result -eq '{Order}' -and $r.body.variablesReference -gt 0)
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'names.Find(n => { goto done; })'; frameId = $frames[0].id }
        $withIf = Send-DapRequest $s 'evaluate' @{ expression = 'names.Find(n => { if (n == label) return true; return false; })'; frameId = $frames[0].id }
        Check $ArchName 'ラムダ式: 本体に if と途中の return を書ける' ($withIf.success -and $withIf.body.result -eq 'null') "($($withIf.body.result)$($withIf.message))"
        # ラムダ式（.NET Framework の x64 では、このあとの GC.Collect の評価が、ほかのスレッドを動かさないと終わらなかった）。
        Check $ArchName 'ラムダ式で、止まっている場所の変数を使える' ((Send-DapRequest $s 'evaluate' @{ expression = 'names.Find(n => n != label)'; frameId = $frames[0].id }).body.result -eq '"x"')
        Check $ArchName '評価できない式は理由つきで失敗する' ($r.success -eq $false -and $r.message -match 'ラムダ.*対応していません') "($($r.message))"
        # 文を並べたラムダ式、throw 式。ラムダの中の呼び出しは、デリゲートを呼んだときに実行する (作っただけでは動かない)。
        $evaluate = { param([string]$Expression) $x = Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $frames[0].id } -TimeoutMs 30000; if ($x.success) { $x.body.result } else { "失敗: $($x.message)" } }
        Check $ArchName 'ラムダ式: 文を並べた本体' ((& $evaluate 'names.Find(n => { var m = n; return m == label; })') -eq 'null') "($(& $evaluate 'names.Find(n => { var m = n; return m == label; })'))"
        $before = & $evaluate 'names.Count'
        $made = & $evaluate 'new Action(() => names.Add("z"))'
        $notYet = & $evaluate 'names.Count'
        $called = & $evaluate 'new Action(() => { names.Add("z"); names.Add("w"); })()'
        $after = & $evaluate 'names.Count'
        # あとの確認のために、足した要素を消しておく。
        [void](& $evaluate 'names.RemoveRange(names.Count - 2, 2)')
        Check $ArchName 'ラムダ式の中の呼び出しは、デリゲートを呼んだときに実行する' ($notYet -eq $before -and [int]$after -eq [int]$before + 2) "($before, $made, $notYet, $called, $after)"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'sum < 0 ? 1 : throw new InvalidOperationException("boom")'; frameId = $frames[0].id }
        Check $ArchName 'throw 式: 投げる例外が結果になる' ($r.success -and $r.body.type -eq 'System.InvalidOperationException') "($($r.body.result)$($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'throw new Exception()'; frameId = $frames[0].id }
        Check $ArchName 'throw 式: C# と同じく、書けない場所では断る' ($r.success -eq $false -and $r.message -match 'throw 式は') "($($r.message))"
        # 名前の無い負の値は、もとの整数の型の値で書く (netcoredbg は 64 ビットの符号なしに広げるが、合わせない。D-032)。
        Check $ArchName '列挙型: 名前の無い負の値は符号つきで書く' ((& $evaluate '(Shade)(-3)') -eq '-3') "($(& $evaluate '(Shade)(-3)'))"
        Check $ArchName 'null だけの式は、型が決まらないので断る' ((& $evaluate 'null') -match '^失敗: null だけの式') "($(& $evaluate 'null'))"
        # 読めるが、評価はしない書き方 (unsafe のコード、スタックの上にしか置けない型)。理由を伝える。
        Check $ArchName 'stackalloc は、理由つきで断る' ((& $evaluate 'stackalloc int[4]') -match '^失敗: stackalloc は評価できません') "($(& $evaluate 'stackalloc int[4]'))"
        Check $ArchName 'UTF-8 の文字列リテラルは、理由つきで断る' ((& $evaluate '"abc"u8') -match '^失敗: UTF-8 の文字列リテラル') "($(& $evaluate '"abc"u8'))"
        Check $ArchName '組にならないサロゲートを含む文字列を作れる' ((& $evaluate '"\ud800".Length') -eq '1') "($(& $evaluate '"\ud800".Length'))"
        Check $ArchName 'ラムダ式: 止まっている場所の変数への代入は、理由つきで断る' ((& $evaluate 'new Action(() => { sum = 1; })') -match '止まっている場所の変数') "($(& $evaluate 'new Action(() => { sum = 1; })'))"

        # 名前をたどる式。式と、期待する結果 (値:型)。
        $expressions = @(
            @('this.Name', '"tea":string'),            # プロパティ (自動実装)
            @('Name', '"tea":string'),                 # this のメンバー
            @('id', '3:int'),
            @('prices[1]', '20:int'),                  # 配列の要素
            @('grid[1, 2]', '0:int'),
            @('names.Count', '1:int'),                 # getter を実行
            @('names[0]', '"x":string'),               # インデクサーを実行 (引数に int を渡す)
            @('Order.Created', '1:int'),               # 型名.静的フィールド
            @('Created', '1:int'),                     # いまの型の静的フィールド
            @('System.Environment.NewLine', '"\r\n":string'),  # 名前空間つきの型の、静的プロパティ
            @('this.GetType().FullName', '"Order":string'),     # メソッドの呼び出しをたどる
            @('(this).Name', '"tea":string'),
            @('label.Length', '3:int'),
            @('label.ToUpper()', '"A\"B":string'),
            @('names.Contains("x")', 'true:bool'),     # 文字列のリテラルを対象の中に作って渡す
            @('names.Contains("y")', 'false:bool'),
            @('names[sum - sum]', '"x":string'),       # 添字に演算
            @('sum + 1', '31:int'),                    # 演算子 (エンジンの側で計算する)
            @('sum * 2 - 1', '59:int'),
            @('(sum + 2) / 4', '8:int'),               # 整数の割り算
            @('-sum', '-30:int'),
            @('rate * 2', '3:double'),
            @('1.5 + 1', '2.5:double'),
            @('big + 1', '1234567890124:long'),
            @('sum == 30', 'true:bool'),
            @('sum != 30 || rate < 2', 'true:bool'),
            @('!ok', 'false:bool'),
            @('ok && sum > 100', 'false:bool'),
            @('label == "a\"b"', 'true:bool'),         # 文字列は中身で比べる
            @('missing == null', 'true:bool'),
            @('names != null', 'true:bool'),
            @('shade == Shade.Dark', 'true:bool'),     # 列挙型
            @('shade != Shade.Light && small > 199', 'true:bool'),
            @('label + sum', '"a\"b30":string'),       # 文字列の連結
            @('names.Count > 0 || names[9] == "z"', 'true:bool'),  # 左で決まれば、右 (例外になる添字) は評価しない
            @('sum / 0', $null),
            @('label < 3', $null),
            @('prices[names.Count]', '20:int'),        # 添字に式 (names.Count は 1)
            @('ok.CompareTo(false)', '1:int'),         # プリミティブ型の値のメソッド (対象の中でボックス化して呼ぶ)
            @('big.ToString()', '"1234567890123":string'),
            @('sum.GetType().FullName', '"System.Int32":string'),
            @('sum.Equals(30)', 'true:bool'),
            @('System.Math.Max(3, 4)', '4:int'),       # オーバーロードを引数の型で選ぶ (byte 版や double 版ではない)
            @('System.Math.Sqrt(16)', '4:double'),     # int を double の引数に渡す
            @('System.Convert.ToString(255, 16)', '"ff":string'),
            @('label.IndexOf("b")', '2:int'),
            @('System.Object.ReferenceEquals(names, names)', 'true:bool'),  # 静的メソッドに引数 2 つ
            @('System.Object.ReferenceEquals(names, null)', 'false:bool'),
            @('Program.Inner(41)', '42:int'),          # 利用者の静的メソッド
            @('names.NoSuchMethod()', $null),
            @('$exception', $null)                     # 例外を処理していないスレッド
        )
        foreach ($e in $expressions) {
            $r = Send-DapRequest $s 'evaluate' @{ expression = $e[0]; frameId = $frames[0].id; context = 'hover' }
            if ($null -eq $e[1]) {
                Check $ArchName "式 $($e[0]) は理由つきで失敗する" ($r.success -eq $false -and $r.message.Length -gt 0) "($($r.message))"
            } else {
                Check $ArchName "式 $($e[0])" ($r.success -and "$($r.body.result):$($r.body.type)" -ceq $e[1]) "($($r.message)$($r.body.result):$($r.body.type))"
            }
        }
        $enumMembers = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['shade'].variablesReference }).body.variables)
        $enumStatics = @((Send-DapRequest $s 'variables' @{ variablesReference = ($enumMembers | Where-Object { $_.name -eq 'Static members' }).variablesReference }).body.variables)
        Check $ArchName '列挙型を開くと、中の数と、メンバーの一覧が見える' (($enumMembers | Where-Object { $_.name -eq 'value__' }).value -eq '1' -and (($enumStatics | ForEach-Object { "$($_.name)=$($_.value):$($_.type)" }) -join ' ') -eq 'Light=Light:Shade Dark=Dark:Shade') "($(($enumStatics | ForEach-Object { "$($_.name)=$($_.value):$($_.type)" }) -join ' '))"
        $pointMembers = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['point'].variablesReference }).body.variables)
        Check $ArchName '構造体のフィールドとプロパティ' ((($pointMembers | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' ') -eq 'X=1 Y=2 Area=2 Static members=') "($(($pointMembers | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '))"
        # 列挙値を引数に渡してセッターを呼ぶ (拡張の WinForms のプロパティ変更が使う形。netcoredbg は E_INVALIDARG で失敗する)。
        $thread = 'System.Threading.Thread.CurrentThread'
        $r = Send-DapRequest $s 'evaluate' @{ expression = "$thread.set_Priority(System.Threading.ThreadPriority.AboveNormal)"; frameId = $frames[0].id }
        $after = (Send-DapRequest $s 'evaluate' @{ expression = "$thread.Priority"; frameId = $frames[0].id }).body.result
        [void](Send-DapRequest $s 'evaluate' @{ expression = "$thread.set_Priority(System.Threading.ThreadPriority.Normal)"; frameId = $frames[0].id })
        Check $ArchName 'セッターに列挙値を渡せる' ($r.success -and $after -eq 'AboveNormal') "($($r.message) $after)"
        # ホバー (context = hover) も、ウォッチと同じに評価する (netcoredbg と同じ)。静的プロパティ、構造体のプロパティ。
        foreach ($e in @(@('Order.Twice', '2:int'), @('Twice', '2:int'), @('point.Area', '2:int'), @('Name', '"tea":string'), @('Order.PerThread', '5:int'))) {
            $r = Send-DapRequest $s 'evaluate' @{ expression = $e[0]; frameId = $frames[0].id; context = 'hover' }
            Check $ArchName "ホバー $($e[0])" ($r.success -and "$($r.body.result):$($r.body.type)" -ceq $e[1]) "($($r.message)$($r.body.result):$($r.body.type))"
        }
        # 定数、列挙型、構造体を含む式。
        foreach ($e in @(@('Order.Max', '99:int'), @('Point.Unit', '"px":string'), @('Shade.Dark', 'Dark:Shade'), @('point.Sum()', '3:int'), @('point.ToString()', '"1,2":string'), @('shade.ToString()', '"Dark":string'), @('price.ToString()', '"12.50":string'), @('moment.Year', '2024:int'), @('maybe.HasValue', 'true:bool'))) {
            $r = Send-DapRequest $s 'evaluate' @{ expression = $e[0]; frameId = $frames[0].id }
            Check $ArchName "式 $($e[0])" ($r.success -and "$($r.body.result):$($r.body.type)" -ceq $e[1]) "($($r.message)$($r.body.result):$($r.body.type))"
        }
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'nope.Value'; frameId = $frames[0].id }
        Check $ArchName '知らない名前は、その名前を伝える' ($r.success -eq $false -and $r.message -match 'nope') "($($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'prices[5]'; frameId = $frames[0].id }
        Check $ArchName '範囲の外の添字は理由つきで失敗する' ($r.success -eq $false -and $r.message -match '範囲') "($($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'list.Count'; frameId = $frames[1].id }
        Check $ArchName '呼び出し元のフレームの文脈で評価する' ($r.success -and $r.body.result -eq '3') "($($r.message)$($r.body.result))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'Inner(1)'; frameId = $frames[1].id }
        Check $ArchName '名前だけの呼び出しは、いまの型のメソッドを呼ぶ' ($r.success -and $r.body.result -eq '2') "($($r.message)$($r.body.result))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'System.GC.Collect()'; frameId = $frames[0].id; context = 'repl' }
        Check $ArchName '値を返さないメソッドの呼び出しは、成功として返る' ($r.success -and $r.body.result -eq 'Expression has been evaluated and has no value' -and $r.body.type -eq 'void') "($($r.message)$($r.body.result))"
        # 拡張は、プロパティの書き換えを set_名前(値) の呼び出しで送ってくる。
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'this.set_Name("coffee")'; frameId = $frames[0].id; context = 'repl' }
        $read = Send-DapRequest $s 'evaluate' @{ expression = 'this.Name'; frameId = $frames[0].id }
        Check $ArchName 'setter を呼んでプロパティを書き換える' ($r.success -and $r.body.type -eq 'void' -and $read.body.result -ceq '"coffee"') "($($r.message) $($read.body.result))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'names[5]'; frameId = $frames[0].id }
        Check $ArchName '評価したコードが例外を投げたら、その例外が結果になる' ($r.success -and $r.body.result -eq '{System.ArgumentOutOfRangeException}' -and $r.body.type -eq 'System.ArgumentOutOfRangeException' -and $r.body.variablesReference -gt 0) "($($r.message)$($r.body.result))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'System.GC.Collect().ToString()'; frameId = $frames[0].id }
        Check $ArchName '値を返さない呼び出しの結果は、式の中で使えない' ($r.success -eq $false -and $r.message -match '値を返さない') "($($r.message))"
        $after = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables | ForEach-Object { "$($_.name)=$($_.value)" })
        Check $ArchName '式の評価をくり返したあとも、変数は読める' (($after -contains 'sum=30') -and ($after -contains 'names=Count = 1')) "($($after -join ' '))"
        $caller = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[1].id }).body.scopes)[0].variablesReference }).body.variables)
        Check $ArchName '呼び出し元のフレームの変数' (($caller | Where-Object { $_.name -eq 'args' }).type -eq 'string[]' -and ($caller | Where-Object { $_.name -eq 'list' }).value -ceq 'Count = 3') "($(($caller | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"

        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $r = Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }
        Check $ArchName '続行したあとは前の参照で読めない' ($r.success -eq $false)
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 30000)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
