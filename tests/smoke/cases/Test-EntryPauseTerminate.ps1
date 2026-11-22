# 段階 1: 入口で止める (stopAtEntry)、動いている対象を止める (pause)、終わらせる (terminate)。
function Test-EntryPauseTerminate([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-pause-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $r = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName 'terminate に対応していると答える' ($r.body.supportsTerminateRequest -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('wait'); stopAtEntry = $true })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName 'stopAtEntry で入口に止まる' ($stop.body.reason -eq 'entry') "($($stop.body.reason))"
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName '入口のフレームは Main' ($frames[0].name -eq 'Program.Main()' -and $frames[0].source.path -eq $source) "($($frames[0].name))"

        $r = Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId }
        $continued = Wait-DapEvent $s 'continued'
        Check $ArchName 'continue のあとに continued が届く' ($r.success -and $continued.body.allThreadsContinued -eq $true -and $continued.body.threadId -eq $stop.body.threadId)
        [void](Wait-DapEvent $s 'output' -Where { param($m) $m.body.output -match 'waiting' } -TimeoutMs 30000)

        $r = Send-DapRequest $s 'pause' @{ threadId = $stop.body.threadId }
        $paused = Wait-DapEvent $s 'stopped' -TimeoutMs 10000
        Check $ArchName 'pause で止まる' ($r.success -and $paused.body.reason -eq 'pause' -and $paused.body.threadId -gt 0) "($($r.message))"
        $threads = @((Send-DapRequest $s 'threads').body.threads)
        $threadNames = @($threads | ForEach-Object name)
        Check $ArchName 'スレッドの名前 (Main Thread と Thread.Name)' (($threadNames -contains 'Main Thread') -and ($threadNames -contains 'worker-a')) "($($threadNames -join ', '))"
        $names = foreach ($thread in $threads) { @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread.id }).body.stackFrames) | ForEach-Object name }
        Check $ArchName '止めた対象のコールスタックに Main がある' ($names -contains 'Program.Main()') "($($names -join ', '))"
        $main = $threads | Where-Object { $_.name -eq 'Main Thread' } | Select-Object -First 1
        $mainFrames = if ($main) { @((Send-DapRequest $s 'stackTrace' @{ threadId = $main.id }).body.stackFrames) | ForEach-Object name } else { @() }
        Check $ArchName 'Main Thread は Main を実行しているスレッド' ($mainFrames -contains 'Program.Main()') "($($mainFrames -join ' <- '))"
        # pause で止めたスレッドは、ほとんどの場合 Thread.Sleep の中 (GC の安全な場所ではない) にいて、対象のコードを
        # 実行する式は評価できない。そのときは、理由と直し方を返す (たまたま安全な場所にいれば、評価できる)。
        $main = foreach ($thread in $threads) { @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread.id }).body.stackFrames) | Where-Object { $_.name -eq 'Program.Main()' } }
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'WorkerTicks.ToString()'; frameId = @($main)[0].id } -TimeoutMs 30000
        Check $ArchName 'pause のあと、評価できない停止なら理由と直し方を返す' ($r.success -or ($r.message -match 'GC の安全な場所' -and $r.message -match 'ステップで')) "($($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'WorkerTicks + 1'; frameId = @($main)[0].id } -TimeoutMs 30000
        Check $ArchName 'pause のあとも、対象のコードを実行しない式は評価できる' ($r.success) "($($r.message))"
        $r = Send-DapRequest $s 'pause' @{ threadId = $stop.body.threadId }
        Check $ArchName '止まっている対象への pause は成功する' ($r.success -eq $true) "($($r.message))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $paused.body.threadId })
        $r = Send-DapRequest $s 'stackTrace' @{ threadId = $paused.body.threadId }
        Check $ArchName '動いている間はコールスタックを読めない' ($r.success -eq $false)
        $running = @((Send-DapRequest $s 'threads').body.threads | ForEach-Object name)
        Check $ArchName '動いている間もスレッドの名前を読める' ($running -contains 'worker-a') "($($running -join ', '))"

        $r = Send-DapRequest $s 'terminate'
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        [void](Wait-DapEvent $s 'terminated')
        Check $ArchName 'terminate で対象が終わる' ($r.success -and $null -ne $exited)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
        $left = @(Get-Process $script:flavor.Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $app })
        Check $ArchName '対象のプロセスが残っていない' ($left.Count -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
