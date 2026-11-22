# 段階 12: トランザクションの全景 (BeginTransaction から Commit / Rollback まで、その中の SQL、開いたままのもの)。
function Test-Transactions([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\transactions'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-transactions-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeTransactions.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Transactions.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'トランザクション: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeTransactions.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $done = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @done' }) + 1
    # 偽のプロバイダーのメソッドを、SQL ログとトランザクションの入口にする。
    $sqlLog = @{ enabled = $true; methods = @('FakeCommand.ExecuteNonQuery'); transactionMethods = @('FakeConnection.BeginDbTransaction', 'FakeTransaction.Commit', 'FakeTransaction.Rollback') }

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-transactions-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        [void](Send-DapRequest $s 'launch' @{ program = $app; sqlLog = $sqlLog })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $done }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $events = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('transaction') }).body.records)
        $sql = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('sql') }).body.records)
        $bySeq = @{}; foreach ($r in $sql) { $bySeq[[int]$r.seq] = $r.commandText }
        $kinds = ($events | ForEach-Object { $_.event }) -join ','
        Check $ArchName 'トランザクション: 始めと終わりを記録する (BeginTransaction() の中の呼び出しは 1 つ)' ($init.body.netdbgSupportsTransactions -eq $true -and $stop.body.reason -eq 'breakpoint' -and $kinds -eq 'begin,commit,begin,rollback,begin') "($kinds)"
        $commit = $events[1]
        $inside = @($commit.statements | ForEach-Object { $bySeq[[int]$_] })
        Check $ArchName 'トランザクション: コミットに、中で走った SQL を実行順に結び付ける' ($commit.transactionId -eq $events[0].transactionId -and ($inside -join ' | ') -eq 'INSERT INTO Orders VALUES (1) | UPDATE Stock SET N = N - 1' -and $commit.durationMs -ge 0 -and $commit.begin.name -eq 'Program.Save()' -and $commit.caller.name -eq 'Program.Save()') "($($commit | ConvertTo-Json -Compress -Depth 4))"
        $rollback = $events[3]
        Check $ArchName 'トランザクション: ロールバックも、その中の SQL と記録する' ($rollback.transactionId -eq $events[2].transactionId -and @($rollback.statements).Count -eq 1 -and $bySeq[[int]$rollback.statements[0]] -eq 'DELETE FROM Orders') "($($rollback | ConvertTo-Json -Compress -Depth 4))"
        $outside = @($sql | Where-Object { $_.commandText -eq 'SELECT 2' })
        $within = @($sql | Where-Object { $_.commandText -eq 'INSERT INTO Orders VALUES (1)' })
        Check $ArchName 'トランザクション: SQL の記録に、属するトランザクションの番号 (外の SQL には無い)' ($outside.Count -eq 1 -and $null -eq $outside[0].transactionId -and $within[0].transactionId -eq $commit.transactionId) "($($outside[0].transactionId) / $($within[0].transactionId))"
        $open = (Send-DapRequest $s 'netdbg.transactions').body.open
        Check $ArchName 'トランザクション: 開いたままのものを、経過時間、始めた所、中の SQL と返す' (@($open).Count -eq 1 -and $open[0].transactionId -eq $events[4].transactionId -and $open[0].elapsedMs -ge 0 -and $open[0].caller.name -eq 'Program.Leave()' -and $bySeq[[int]$open[0].statements[0]] -eq 'SELECT 1') "($($open | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'トランザクション: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI も開いたままのトランザクションを読める。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-transactions-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'sql_log' @{ enabled = $true; methods = $sqlLog.methods; transactionMethods = $sqlLog.transactionMethods })
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = $done })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 })
        $r = Invoke-McpTool $m 'transactions' @{}
        Check $ArchName 'トランザクション (MCP): transactions で開いたものと出来事を読める' (-not $r.Error -and @($r.Value.open).Count -eq 1 -and $r.Value.open[0].caller.name -eq 'Program.Leave()' -and @($r.Value.events).Count -eq 5) $r.Text
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'トランザクション (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}