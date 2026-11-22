# 段階 3: ステップ (next / stepIn / stepOut)。
function Test-Step([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-step-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'passes-call'
        $thread = $stop.body.threadId

        # ステップして、止まった場所 (フレーム名と行) を返す。
        $step = {
            param([string]$Command)
            $r = Send-DapRequest $s $Command @{ threadId = $thread }
            if (-not $r.success) { return "失敗: $($r.message)" }
            $stopped = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stopped.body.threadId; levels = 1 }).body.stackFrames)[0]
            return "$($stopped.body.reason) $($top.name):$($top.line)"
        }
        $at = { param([string]$Name, [string]$Marker) "step ${Name}:$(Get-MarkerLine $source $Marker)" }
        # いちばん上のフレームの変数のうち、直前のステップの戻り値 (名前が returned で終わるもの)。
        $returned = {
            $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread; levels = 1 }).body.stackFrames)[0]
            $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $top.id }).body.scopes)[0].variablesReference }).body.variables)
            return @($vars | Where-Object { $_.name -like '* returned' } | ForEach-Object { "$($_.name)=$($_.value):$($_.type)" })
        }

        # DebuggerStepThrough の付いた Passes は通り過ぎて、その中から呼ばれる Inner に入る。戻るときも Passes では止まらない。
        $where = & $step 'stepIn'
        Check $ArchName 'stepIn は印の付いたメソッドを通り過ぎて、その先の自分のコードに入る' ($where -eq (& $at 'Program.Inner()' 'inner-open')) "($where)"
        $names = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread }).body.stackFrames | ForEach-Object name)
        # .NET 11 では、Main の下にランタイムの System.Environment.CallEntryPoint() が見える。上の 3 つだけを比べる。
        Check $ArchName '通り過ぎたメソッドもコールスタックには出る' (($names -join ' <- ').StartsWith('Program.Inner() <- Program.Passes() <- Program.Main()')) "($($names -join ' <- '))"
        $where = & $step 'stepOut'
        Check $ArchName 'stepOut は印の付いたメソッドで止まらず、自分のコードまで戻る' ($where -eq (& $at 'Program.Main()' 'passes-call')) "($where)"
        $values = & $returned
        Check $ArchName 'stepOut のあと、戻ってきた呼び出しの戻り値が見える' (($values -join ' ') -eq 'Program.Passes returned=2:int') "($($values -join ' '))"
        $where = & $step 'next'
        Check $ArchName '次の行へ' ($where -eq (& $at 'Program.Main()' 'call')) "($where)"
        $where = & $step 'stepIn'
        Check $ArchName 'stepIn で呼び出し先の先頭に入る' ($where -eq (& $at 'Program.Work()' 'work-open')) "($where)"
        $where = & $step 'next'
        Check $ArchName 'next で次の文に進む' ($where -eq (& $at 'Program.Work()' 'work-first')) "($where)"
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread; levels = 1 }).body.stackFrames)[0].id }).body.scopes)[0].variablesReference }).body.variables)
        Check $ArchName 'ステップで止まった場所の変数を読める' ((($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' ') -eq 'n=21 doubled=0') "($(($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '))"
        $where = & $step 'next'
        Check $ArchName 'next をもう一度' ($where -eq (& $at 'Program.Work()' 'work')) "($where)"
        # 次に実行する行を動かす (gotoTargets / goto)。return の行から、代入の行へ戻す。
        $target = @((Send-DapRequest $s 'gotoTargets' @{ source = @{ path = $source }; line = (Get-MarkerLine $source 'work-first') }).body.targets)[0]
        $r = Send-DapRequest $s 'goto' @{ threadId = $thread; targetId = $target.id }
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'goto で同じメソッドの中の行へ動かす' ($r.success -and $target.label -eq "line $($target.line)" -and $top.line -eq (Get-MarkerLine $source 'work-first')) "($($r.message) $($top.line))"
        Check $ArchName 'goto は stopped を出さない (拡張の側が出す)' (@($s.Events | Where-Object { $_.event -eq 'stopped' }).Count -eq 0)
        $other = @((Send-DapRequest $s 'gotoTargets' @{ source = @{ path = $source }; line = (Get-MarkerLine $source 'values') }).body.targets)[0]
        $r = Send-DapRequest $s 'goto' @{ threadId = $thread; targetId = $other.id }
        Check $ArchName '別のメソッドの行への goto は理由つきで失敗する' ($r.success -eq $false -and $r.message -match '同じメソッド') "($($r.message))"
        $where = & $step 'next'
        Check $ArchName 'goto のあとの next は、動かした先から進む' ($where -eq (& $at 'Program.Work()' 'work')) "($where)"
        $where = & $step 'stepOut'
        Check $ArchName 'stepOut で呼び出し元の行に戻る' ($where -eq (& $at 'Program.Main()' 'call')) "($where)"
        $values = & $returned
        Check $ArchName 'stepOut のあとの戻り値' (($values -join ' ') -eq 'Program.Work returned=42:int') "($($values -join ' '))"
        $where = & $step 'next'
        Check $ArchName '戻ったあとの next は次の行' ($where -eq (& $at 'Program.Main()' 'after-call')) "($where)"
        $values = & $returned
        Check $ArchName '戻り値は、次のステップで消える' ($values.Count -eq 0) "($($values -join ' '))"
        $where = & $step 'next'
        Check $ArchName 'next はライブラリの呼び出しを越える' ($where -eq (& $at 'Program.Main()' 'stderr')) "($where)"
        $values = & $returned
        Check $ArchName 'next で越えた呼び出しの戻り値が、呼ばれた順に見える' (($values -contains 'String.Join returned="":string') -and ($values -contains 'Environment.GetEnvironmentVariable returned=null:string') -and -not ($values -match 'WriteLine')) "($($values -join ' | '))"
        $where = & $step 'stepIn'
        Check $ArchName 'stepIn はシンボルの無いコードに入らない' ($where -eq (& $at 'Program.Main()' 'return')) "($where)"

        [void](Send-DapRequest $s 'continue' @{ threadId = $thread })
        $r = Send-DapRequest $s 'next' @{ threadId = $thread }
        Check $ArchName '動いている間のステップは理由つきで失敗する' ($r.success -eq $false)
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'ステップのあと最後まで走る' ($exited.body.exitCode -eq 7)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 参照型の戻り値: next で越えた呼び出しが返したオブジェクトを、変数の欄で開ける。表記は netcoredbg と同じ (値は
    # DebuggerDisplay を使わない {型名})。開いた中身は、代わりの型 (List<T> の [0]…) で見せる (D-031)。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'copy'
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $top.id }).body.scopes)[0].variablesReference }).body.variables)
        $copied = $vars | Where-Object { $_.name -eq 'Program.Copy returned' } | Select-Object -First 1
        $items = @((Send-DapRequest $s 'variables' @{ variablesReference = $copied.variablesReference }).body.variables)
        Check $ArchName '参照型の戻り値を開ける' ($copied.value -eq '{System.Collections.Generic.List<string>}' -and $copied.variablesReference -gt 0 -and ($items | Where-Object { $_.name -eq '[2]' }).value -eq '"item2"') "($($copied.value) / $(($items | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '参照型の戻り値: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
