# 段階 3: async メソッドの中のステップ。await で待っている間に呼び出し元へ抜けず、続きの行で止まる。
function Test-AsyncStep([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-async-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'async-await' -ProgramArgs @('async')
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        # 続きは別のスレッドで走ることがあるので、止まったスレッドは stopped から取り直す。
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'await をまたぐ next は、続きの行で止まる' ($stop.body.reason -eq 'step' -and $top.name -match 'SlowAsync' -and $top.line -eq (Get-MarkerLine $source 'async-after')) "($($stop.body.reason) $($top.name):$($top.line))"
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $top.id }).body.scopes)[0].variablesReference }).body.variables)
        Check $ArchName 'await のあとも、前の変数を元の名前で読める' (($vars | Where-Object { $_.name -eq 'before' }).value -eq '5') "($(($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '))"
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'そのあとの next はふつうに次の行' ($top.line -eq (Get-MarkerLine $source 'async-return')) "($($top.name):$($top.line))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'async メソッドの結果で終わる' ($exited.body.exitCode -eq 6 -and @($s.Events | Where-Object { $_.event -eq 'stopped' }).Count -eq 0) "($($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # async メソッドの続きからの stepOut: await している呼び出し元の、await の行に戻る。その呼び出し元からの stepOut は、
    # GetResult() で待っている Main に戻る (netcoredbg と同じ。D-047)。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'async-after' -ProgramArgs @('async-out')
        $out = {
            [void](Send-DapRequest $s 'stepOut' @{ threadId = $stop.body.threadId })
            $script:stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $script:stop.body.threadId; levels = 1 }).body.stackFrames)[0]
            return "$($script:stop.body.reason) $($top.name):$($top.line)"
        }
        $where = & $out
        $stop = $script:stop
        Check $ArchName 'async の続きからの stepOut: await している呼び出し元の await の行' ($where -eq "step Program.<OuterAsync>d__8.MoveNext():$(Get-MarkerLine $source 'outer-await')") "($where)"
        $where = & $out
        $stop = $script:stop
        Check $ArchName 'async の呼び出し元からの stepOut: GetResult() で待っている Main' ($where -eq "step Program.Main():$(Get-MarkerLine $source 'async-out-call')") "($where)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'async の stepOut のあと、止まらずに最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'async の stepOut: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 同じ async メソッドが並行して走っているとき: 300 ミリ秒待つ呼び出しで await を next でまたぐと、先に再開する
    # 50 ミリ秒の呼び出しではなく、同じ呼び出しの続きで止まる。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('parallel') })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'waits-await'); condition = 'delay == 300' }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        # 再開の前に、もう一方の呼び出しが同じ行を通るよう、ブレークポイントは外しておく。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @() })
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $delay = (Send-DapRequest $s 'evaluate' @{ expression = 'delay'; frameId = $top.id }).body.result
        Check $ArchName '並行する同じ async メソッド: 同じ呼び出しの続きで止まる' ($stop.body.reason -eq 'step' -and $top.line -eq (Get-MarkerLine $source 'waits-return') -and $delay -eq '300') "($($stop.body.reason) line $($top.line) delay=$delay)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '並行する同じ async メソッド: 最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '並行する同じ async メソッド: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
