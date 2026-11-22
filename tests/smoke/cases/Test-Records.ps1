# 段階 11: 止めずに読む記録 (D-063)。SQL ログと例外ログを有効にして ADO.NET のコマンドを実行する対象を走らせ、止まらずに
# 文とパラメーターが記録されること (値を埋めた文、二重の呼び出しを省く、繰り返しの数、利用者が同じメソッドに置いた
# ブレークポイントでは記録したうえで止まる)、捕まえた例外も、投げた所の変数と一緒に記録されることを確かめる。
function Test-Records([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\records'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-records-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeSqlLog.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'SqlLog.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'SQL ログ: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeSqlLog.exe'
    # 障害票用の環境: 設定のファイルの接続文字列 (パスワードは隠して記録する)。
    [IO.File]::WriteAllText("$app.config", '<?xml version="1.0"?><configuration><connectionStrings><add name="Main" connectionString="Data Source=db;User Id=app;Password=secret" providerName="System.Data.SqlClient" /></connectionStrings></configuration>')
    [IO.File]::WriteAllText((Join-Path $work 'bin\appsettings.json'), '{ "ConnectionStrings": { "Reports": "Host=r;Pwd=hidden" } }')
    $lines = [IO.File]::ReadAllLines($source)
    $doneLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @done' }) + 1
    $throwLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @throw' }) + 1
    $client = if ($tfm -eq 'net48') { 'System.Data.SqlClient' } else { 'Microsoft.Data.SqlClient' }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-records-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName 'SQL ログ: capabilities に netdbgSupportsJournal' ($init.body.netdbgSupportsJournal -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app; sqlLog = $true; exceptionLog = $true })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $doneLine }) })
        # 利用者のブレークポイント: プロバイダーのメソッドに (シンボルが無いので anyCode)。
        [void](Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @(@{ name = "$client.SqlCommand.ExecuteScalar"; anyCode = $true }) })
        $trace = (Send-DapRequest $s 'netdbg.setEventTrace' @{ handlers = @(@{ breakpointName = 'Program.OnClick'; label = 'ok.Click'; event = 'Click'; control = 'ok' }) }).body
        $sqlBreaks = (Send-DapRequest $s 'netdbg.setSqlBreakpoints' @{ breakpoints = @(@{ text = 'delete  from audit'; parameter = '@id'; value = '7' }) }).body
        [void](Send-DapRequest $s 'configurationDone')
        # SQL で止める: Id が 8 の文は通り過ぎ、7 の文の実行の直前で止まる。
        $sqlStop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $sqlTop = @((Send-DapRequest $s 'stackTrace' @{ threadId = $sqlStop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $audits = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('sql') }).body.records | Where-Object { $_.commandText -like 'DELETE FROM Audit*' })
        Check $ArchName 'SQL で止める: 条件に合う文の実行の直前で止まり、文を説明に出す' ($init.body.netdbgSupportsSqlBreakpoints -eq $true -and $sqlStop.body.reason -eq 'breakpoint' -and $sqlStop.body.description -eq 'SQL breakpoint: DELETE FROM Audit WHERE Id = 7' -and $sqlStop.body.hitBreakpointIds[0] -eq $sqlBreaks.breakpoints[0].id -and $sqlTop.name -match 'ExecuteNonQuery') "($($sqlStop.body | ConvertTo-Json -Compress) $($sqlTop.name))"
        Check $ArchName 'SQL で止める: 合わない文は記録して続け、合った文の記録に印が付く' ($audits.Count -eq 2 -and -not $audits[0].stopped -and $audits[1].stopped -eq $true -and $audits[1].breakpointIds[0] -eq $sqlBreaks.breakpoints[0].id) "($($audits | ConvertTo-Json -Compress -Depth 2))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $sqlStop.body.threadId })
        $first = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $first.body.threadId; levels = 1 }).body.stackFrames)[0]
        $journal = (Send-DapRequest $s 'netdbg.journal' @{ kinds = @('sql') }).body
        $records = @($journal.records)
        Check $ArchName 'SQL ログ: 利用者が同じメソッドに置いたブレークポイントでは、記録したうえで止まる' ($first.body.reason -eq 'breakpoint' -and $top.name -match 'ExecuteScalar' -and @($records | Where-Object { $_.commandText -eq 'SELECT 2' }).Count -eq 1) "($($first.body.reason) $($top.name); $($records.Count) records)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $first.body.threadId })
        $done = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $at = @((Send-DapRequest $s 'stackTrace' @{ threadId = $done.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'SQL ログ: プローブでは止まらない (次の停止は利用者の行)' ($done.body.reason -eq 'breakpoint' -and $at.name -eq 'Program.Main()' -and $at.line -eq $doneLine) "($($done.body.reason) $($at.name):$($at.line))"

        $records = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('sql') }).body.records)
        $orders = @($records | Where-Object { $_.commandText -like 'SELECT * FROM Orders*' })
        $expected = "SELECT * FROM Orders WHERE Id = 42 AND Name = 'O''Brien' AND Placed >= '2024-04-01T13:05:09.123' AND Data = 0x01ab AND Note = NULL"
        Check $ArchName 'SQL ログ: 値を埋めた文 (数、文字列、日時、バイナリ、DBNull)' ($orders.Count -ge 1 -and $orders[0].sql -eq $expected) "($($orders[0].sql))"
        Check $ArchName 'SQL ログ: ExecuteReader() の中の ExecuteReader(behavior) は記録しない' ($orders.Count -eq 2) "($($orders.Count) records: $(($orders | ForEach-Object method) -join ', '))"
        Check $ArchName 'SQL ログ: 呼び出し元は利用者のコード' ($orders[0].caller.name -match '^Program\.' -and $orders[0].caller.source.path -eq $source) "($($orders[0].caller | ConvertTo-Json -Compress -Depth 4))"
        $ones = @($records | Where-Object { $_.commandText -eq 'SELECT 1' })
        $completed = @($s.Events | Where-Object { $_.event -eq 'netdbg.sqlCompleted' })
        $timed = @($records | Where-Object { $null -ne $_.durationMs })
        Check $ArchName 'SQL の実行時間: 入口から呼び出しが戻るまでを記録する (失敗した実行には印)' ($ones.Count -eq 6 -and @($ones | Where-Object { $_.durationMs -ge 0 -and $_.failed -eq $true }).Count -eq 6 -and $timed.Count -eq $completed.Count -and $timed.Count -ge 10 -and $null -eq ($records | Where-Object { $_.stopped }).durationMs) "($($timed.Count) timed, $($completed.Count) events; $($ones[0] | ConvertTo-Json -Compress -Depth 1))"
        Check $ArchName 'SQL ログ: 同じ文の繰り返しを数え、5 回以上に印を付ける' ($ones.Count -eq 6 -and $ones[5].repeat -eq 6 -and $ones[5].repeated -eq $true -and -not $ones[3].repeated) "($($ones.Count); last repeat $($ones[-1].repeat))"
        $update = @($records | Where-Object { $_.commandText -like 'UPDATE T SET*' })
        $provider = if ($tfm -eq 'net48') { 'OleDb (? の位置)' } else { 'Oracle (:name を名前で)' }
        Check $ArchName "SQL ログ: $provider" ($update.Count -eq 1 -and $update[0].sql -eq "UPDATE T SET A = 'x' WHERE B = 7") "($($update[0].sql))"
        $events = @($s.Events | Where-Object { $_.event -eq 'netdbg.sqlExecuted' })
        Check $ArchName 'SQL ログ: 記録ごとに netdbg.sqlExecuted を送る' ($events.Count -eq $records.Count -and $events[0].body.seq -eq $records[0].seq) "($($events.Count) events, $($records.Count) records)"
        $after = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('sql'); after = $records[-2].seq }).body.records)
        Check $ArchName 'SQL ログ: 番号のあとだけを読める' ($after.Count -eq 1 -and $after[0].seq -eq $records[-1].seq) "($($after.Count))"
        $calls = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('event') }).body.records)
        $handlerLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @handler' }) + 1
        Check $ArchName 'イベント トレース: ハンドラーの入口で、送り手の名前と一緒に記録する' ($trace.count -eq 1 -and $calls.Count -eq 2 -and $calls[0].sender -eq 'ok' -and $calls[0].handler -eq 'Program.OnClick()' -and $calls[0].events[0] -eq 'Click' -and $calls[0].labels[0] -eq 'ok.Click' -and $calls[0].source.path -eq $source -and $calls[0].line -gt 0) "($($calls | ConvertTo-Json -Compress -Depth 3))"
        Check $ArchName 'イベント トレース: 名前の無い送り手は型の名前' ($calls.Count -eq 2 -and $calls[1].sender -eq '(Object)') "($($calls[1].sender))"
        Check $ArchName 'イベント トレース: 記録ごとに netdbg.controlEvent を送る' (@($s.Events | Where-Object { $_.event -eq 'netdbg.controlEvent' }).Count -eq 2)
        $exceptions = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('exception') }).body.records)
        $mine = @($exceptions | Where-Object { $_.typeName -eq 'System.ArgumentException' })
        Check $ArchName '例外ログ: 捕まえた例外も、型と文言、投げた所と一緒に記録する' ($mine.Count -eq 1 -and $mine[0].message -eq 'bad id 42 x42' -and $mine[0].stopped -eq $false -and $mine[0].functionName -eq 'Program.Fail()' -and $mine[0].line -eq $throwLine) "($($mine | ConvertTo-Json -Compress -Depth 2))"
        $locals = @($mine[0].snapshot.frames[0].locals)
        Check $ArchName '例外ログ: 投げた所のローカル変数をスナップショットに残す' (@($locals | Where-Object { $_.name -eq 'id' -and $_.value -eq '42' }).Count -eq 1 -and @($locals | Where-Object { $_.name -eq 'name' -and $_.value -eq '"x42"' }).Count -eq 1 -and $mine[0].snapshot.stackTrace) "($($locals | ConvertTo-Json -Compress))"
        $provider = @($exceptions | Where-Object { $_.typeName -eq 'System.InvalidOperationException' })
        Check $ArchName '例外ログ: .NET の中で投げられた例外は、利用者のコードの場所を見せる' ($provider.Count -ge 1 -and $provider[0].source.path -eq $source) "($($provider.Count); $($provider[0].functionName))"
        $environment = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('environment') }).body.records)
        $e = $environment[0]
        $runtime = if ($tfm -eq 'net48') { '^\.NET Framework 4\.' } else { '^\.NET 10\.' }
        $strings = @($e.connectionStrings)
        Check $ArchName '環境: ランタイムが始まったときに、ビット数、版、カルチャ、接続文字列 (パスワードは隠す) を記録する' ($environment.Count -eq 1 -and $e.bitness -eq $ArchName -and $e.runtime -match $runtime -and $e.culture -and $e.program -eq $app -and ($strings | Where-Object { $_.name -eq 'Main' }).connectionString -eq 'Data Source=db;User Id=app;Password=***' -and ($strings | Where-Object { $_.name -eq 'Reports' }).connectionString -eq 'Host=r;Pwd=***' -and ($e | ConvertTo-Json -Depth 5) -notmatch 'secret|hidden') "($($e | ConvertTo-Json -Compress -Depth 4))"
        $failed = @($exceptions | Where-Object { $_.typeName -eq 'System.InvalidOperationException' })[0]
        $context = @($failed.context)
        Check $ArchName '例外の前: 同じスレッドの、直前の利用者の操作からの流れ (イベント、例外、SQL) を添える' ($context.Count -ge 2 -and $context[0].kind -eq 'event' -and $context[-1].kind -eq 'sql' -and $context[-1].summary -like 'SELECT * FROM Orders*' -and $failed.environmentSeq -eq $e.seq) "($($context | ConvertTo-Json -Compress -Depth 3))"
        $thrownEvents = @($s.Events | Where-Object { $_.event -eq 'netdbg.exceptionThrown' })
        Check $ArchName '例外ログ: 記録ごとに netdbg.exceptionThrown を送る' ($thrownEvents.Count -eq $exceptions.Count) "($($thrownEvents.Count) events, $($exceptions.Count) records)"
        $off = (Send-DapRequest $s 'netdbg.setSqlLog' @{ enabled = $false }).body
        Check $ArchName 'SQL ログ: 止められる' ($off.enabled -eq $false -and @($off.methods).Count -eq 0) "($($off | ConvertTo-Json -Compress))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $done.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'SQL ログ: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI が sql_log で始めて、実行された文を読む。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-records-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'exception_log' @{ enabled = $true })
        [void](Invoke-McpTool $m 'event_trace' @{ handlers = @('Program.OnClick') })
        $on = Invoke-McpTool $m 'sql_log' @{ enabled = $true }
        Check $ArchName 'SQL ログ (MCP): sql_log で始める' (-not $on.Error -and $on.Value.sqlLog.enabled -eq $true) $on.Text
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = $doneLine })
        $break = Invoke-McpTool $m 'sql_break' @{ breakpoints = @(@{ text = 'DELETE FROM Audit'; value = '7' }) }
        $launched = Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 }
        Check $ArchName 'SQL で止める (MCP): sql_break の条件に合う文で止まり、止まった理由が分かる' (-not $break.Error -and @($break.Value.breakpoints).Count -eq 1 -and $launched.Text -match 'SQL breakpoint: DELETE FROM Audit WHERE Id = 7') "($($break.Text) / $($launched.Text))"
        $r = Invoke-McpTool $m 'sql_log' @{}
        $statements = @($r.Value.statements)
        Check $ArchName 'SQL ログ (MCP): 実行された文を、値を埋めて読める' (-not $r.Error -and @($statements | Where-Object { $_.sql -eq "UPDATE T SET A = 'x' WHERE B = 7" }).Count -eq 1 -and -not $statements[0].frames -and $statements[0].caller) $r.Text
        $e = Invoke-McpTool $m 'exception_log' @{}
        Check $ArchName '例外ログ (MCP): 捕まえた例外を読める' (-not $e.Error -and @($e.Value.exceptions | Where-Object { $_.typeName -eq 'System.ArgumentException' -and $_.message -eq 'bad id 42 x42' }).Count -eq 1) $e.Text
        $env = Invoke-McpTool $m 'environment' @{}
        Check $ArchName '環境 (MCP): environment で読める' (-not $env.Error -and $env.Value.bitness -eq $ArchName -and $env.Value.runtime) $env.Text
        $c = Invoke-McpTool $m 'event_trace' @{}
        Check $ArchName 'イベント トレース (MCP): 呼ばれたハンドラーと送り手を読める' (-not $c.Error -and @($c.Value.calls).Count -eq 2 -and $c.Value.calls[0].sender -eq 'ok') $c.Text
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'SQL ログ (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}