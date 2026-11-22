# 段階 4: ブレークポイントの条件。条件が成り立つときだけ止まる。
function Test-Conditions([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-conditions-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $r = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName '条件つきブレークポイントに対応していると答える' ($r.body.supportsConditionalBreakpoints -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        # ループの中 (3 回通る) に、変数とメソッドの呼び出しを含む条件。別の行に、評価できない条件と、いつも偽の条件。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(
            @{ line = (Get-MarkerLine $source 'loop'); condition = 'i == 2 && list.Count == 2' },
            @{ line = (Get-MarkerLine $source 'filter-call'); condition = 'list.Count > 100' },
            @{ line = (Get-MarkerLine $source 'call'); condition = 'nosuch > 1' }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'i'; frameId = $top.id }
        Check $ArchName '条件が成り立つ回だけ止まる (3 回のうち最後)' ($stop.body.reason -eq 'breakpoint' -and $top.line -eq (Get-MarkerLine $source 'loop') -and $r.body.result -eq '2') "(line $($top.line), i=$($r.body.result))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '条件が偽の行は通り過ぎ、評価できない条件の行では止まる' ($top.line -eq (Get-MarkerLine $source 'call')) "(line $($top.line))"
        $told = @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'nosuch > 1' })
        Check $ArchName '評価できない条件は、理由を伝える' ($told.Count -eq 1 -and $told[0].body.output -match 'nosuch') "($(($told | ForEach-Object { $_.body.output.Trim() }) -join ' / '))"

        # ステップの途中で、条件が偽のブレークポイントを通っても、ステップは続く。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'work-first'); condition = 'n == 0' }) })
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '条件が偽のブレークポイントは、進行中のステップを止めない' ($stop.body.reason -eq 'step' -and $top.line -eq (Get-MarkerLine $source 'after-call')) "($($stop.body.reason) line $($top.line))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '条件つきのあとも最後まで走る' ($exited.body.exitCode -eq 7 -and @($s.Events | Where-Object { $_.event -eq 'stopped' }).Count -eq 0)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
