# 段階 5: 拡張の独自の要求（オブジェクト ID、スレッドの凍結、ヒープの統計）。要求の形は、拡張が netcoredbg に当てた
# パッチ（0008 / 0010 / 0011）と同じ。
function Test-Inspect([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-inspect-$($script:flavor.Dir)-$ArchName.log"

    # オブジェクト ID とヒープの統計: 適合試験の目印で止める。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'conformance' -ProgramArgs @('conformance')
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $evaluate = { param([string]$Expression) Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $frame.id } -TimeoutMs 30000 }
        $words = (& $evaluate 'words').body
        $first = Send-DapRequest $s 'netdbg.makeObjectId' @{ variablesReference = $words.variablesReference }
        $numbers = (& $evaluate 'numbers').body
        $second = Send-DapRequest $s 'netdbg.makeObjectId' @{ variablesReference = $numbers.variablesReference }
        $again = Send-DapRequest $s 'netdbg.makeObjectId' @{ variablesReference = (& $evaluate 'words').body.variablesReference }
        Check $ArchName 'オブジェクト ID: 1、2 と付き、同じオブジェクトには同じ ID' ($first.body.id -eq 1 -and $second.body.id -eq 2 -and $again.body.id -eq 1) "($($first.body.id), $($second.body.id), $($again.body.id) $($first.message))"
        $shown = (& $evaluate 'words').body.result
        Check $ArchName 'オブジェクト ID: 値のあとに {$1} を見せる' ($shown -match '\{\$1\}$') "($shown)"
        $count = (& $evaluate '__NETDBG_OBJID_1.Count').body.result
        Check $ArchName 'オブジェクト ID: 式の中で使える' ($count -eq '2') "($count)"
        $missing = & $evaluate '__NETDBG_OBJID_9'
        Check $ArchName 'オブジェクト ID: 無い ID は失敗' (-not $missing.success) "($($missing.body.result))"
        $heap = Send-DapRequest $s 'netdbg.heapStats' -TimeoutMs 60000
        $types = @($heap.body.types)
        $strings = $types | Where-Object { $_.name -eq 'string' -or $_.name -eq 'System.String' } | Select-Object -First 1
        Check $ArchName 'ヒープの統計: 型ごとの数と大きさ（大きい順）' ($heap.success -and $types.Count -gt 10 -and $strings.count -gt 0 -and $types[0].size -ge $types[-1].size) "($($types.Count) 種類 $($heap.message))"
        # ハンドルの統計: Program.GateHeld (ManualResetEvent) の SafeWaitHandle は、基底の型 SafeHandle の handle に
        # OS のハンドルを持つ。Dispose していないので生きている。
        $handles = Send-DapRequest $s 'netdbg.handleStats' @{ fields = @(@{ type = 'System.Runtime.InteropServices.SafeHandle'; field = 'handle' }, @{ type = 'No.Such.Type'; field = 'x' }) } -TimeoutMs 60000
        $wait = @($handles.body.types) | Where-Object { $_.name -eq 'Microsoft.Win32.SafeHandles.SafeWaitHandle' } | Select-Object -First 1
        Check $ArchName 'ハンドルの統計: 基底の型のフィールドで、生きているハンドルを数える' ($handles.success -and $wait -and $wait.live -ge 1 -and $wait.count -ge $wait.live -and $wait.field -eq 'System.Runtime.InteropServices.SafeHandle.handle') "($(($handles.body.types | ForEach-Object { "$($_.name) $($_.count)/$($_.live) $($_.field)" }) -join ', ') $($handles.message))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'オブジェクト ID とヒープの統計: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # スレッドの凍結: wait の目印で止め、数え続ける worker-a を凍結する。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'tick' -ProgramArgs @('wait')
        $main = $stop.body.threadId
        $frameOf = { (@((Send-DapRequest $s 'stackTrace' @{ threadId = $main; levels = 1 }).body.stackFrames)[0]).id }
        $ticks = { [int](Send-DapRequest $s 'evaluate' @{ expression = 'WorkerTicks'; frameId = (& $frameOf) } -TimeoutMs 30000).body.result }
        $next = {
            [void](Send-DapRequest $s 'continue' @{ threadId = $main })
            [void](Wait-DapEvent $s 'stopped' -TimeoutMs 30000)
        }
        $worker = @((Send-DapRequest $s 'threads').body.threads) | Where-Object { $_.name -eq 'worker-a' } | Select-Object -First 1
        $frozen = Send-DapRequest $s 'netdbg.freezeThread' @{ threadId = $worker.id; frozen = $true }
        $at = & $ticks
        & $next
        $after = & $ticks
        & $next
        $later = & $ticks
        Check $ArchName 'スレッドの凍結: 続行しても、評価しても、凍結したスレッドは動かない' ($frozen.success -and $after -eq $at -and $later -eq $at) "($at, $after, $later $($frozen.message))"
        $thawed = Send-DapRequest $s 'netdbg.freezeThread' @{ threadId = $worker.id; frozen = $false }
        & $next
        & $next
        $moved = & $ticks
        Check $ArchName 'スレッドの凍結: 解くと、また動く' ($thawed.success -and $moved -gt $at) "($at → $moved)"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'スレッドの凍結: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
