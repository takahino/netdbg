# 段階 12: WinForms のデータバインディングが飲み込んだ Format / Parse の失敗を、走っている間に記録する。
function Test-BindingLog([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0-windows' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\bindings'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-bindings-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeBindings.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Bindings.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'データバインディング: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeBindings.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $done = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @done' }) + 1

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-bindings-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $done }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $records = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('binding') }).body.records)
        $parse = @($records | Where-Object { $_.direction -eq 'parse' })
        $format = @($records | Where-Object { $_.direction -eq 'format' })
        $controls = @($parse | ForEach-Object control | Sort-Object -Unique)
        Check $ArchName 'データバインディング: 対応していると答え、止めずに走り切る (次の停止は利用者の行)' ($init.body.netdbgSupportsBindingLog -eq $true -and $stop.body.reason -eq 'breakpoint') "($($stop.body.reason))"
        Check $ArchName 'データバインディング: Parse の失敗を、コントロール、プロパティ、メンバー、例外と記録する (FormattingEnabled の有無とも)' (($controls -join ',') -eq 'quantityBox,strictBox' -and @($parse | Where-Object { $_.property -ne 'Text' -or $_.member -ne 'Quantity' -or $_.exceptionType -notmatch 'FormatException' -or $_.dataSource -ne 'Order' }).Count -eq 0 -and @($parse | Where-Object { $_.formattingEnabled }).Count -ge 1 -and @($parse | Where-Object { -not $_.formattingEnabled }).Count -ge 1) "($($records | ConvertTo-Json -Compress -Depth 3))"
        Check $ArchName 'データバインディング: Format の失敗も記録する' ($format.Count -ge 1 -and $format[0].control -eq 'codeUpDown' -and $format[0].property -eq 'Value' -and $format[0].member -eq 'Code' -and $format[0].controlType -eq 'NumericUpDown') "($($format | ConvertTo-Json -Compress -Depth 3))"
        Check $ArchName 'データバインディング: 呼び出し元は利用者のコード' (@($parse | Where-Object { $_.caller.source.path -eq $source }).Count -eq $parse.Count) "($($parse[0].caller | ConvertTo-Json -Compress -Depth 3))"
        $events = @($s.Events | Where-Object { $_.event -eq 'netdbg.bindingFailed' })
        $outputs = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'データバインディング: 記録ごとに netdbg.bindingFailed を送り、デバッグ コンソールに出す' ($events.Count -eq $records.Count -and $outputs -match 'Data binding parse failed: quantityBox\.Text → Quantity: System\.FormatException') "($($events.Count) events, $($records.Count) records)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'データバインディング: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI も同じ記録を読める。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-bindings-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = $done })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 })
        $r = Invoke-McpTool $m 'binding_log' @{}
        Check $ArchName 'データバインディング (MCP): binding_log で失敗を読める' (-not $r.Error -and @($r.Value.failures | Where-Object { $_.control -eq 'quantityBox' -and $_.direction -eq 'parse' }).Count -ge 1 -and $r.Value.last -gt 0) $r.Text
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'データバインディング (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}