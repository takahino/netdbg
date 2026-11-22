# 段階 11: UI のフリーズの監視と、プロセスの使用量をエンジンが行う (拡張の PowerShell の補助から移した)。
# UI のスレッドが答えなくなったら、そのスレッドで止まって理由を出す。止まっている間は数えない。
function Test-UiFreeze([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0-windows' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\freeze'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-freeze-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeFreeze.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Freeze.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'UI のフリーズ: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeFreeze.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $shown = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @shown' }) + 1

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-freeze-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName 'UI のフリーズ: 監視と使用量に対応していると答える' ($init.body.netdbgSupportsUiFreeze -eq $true -and $init.body.netdbgSupportsProcessUsage -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app; uiFreeze = @{ thresholdMs = 1000 } })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $shown }) })
        [void](Send-DapRequest $s 'configurationDone')
        $first = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        # 止まっている間 (閾値より長く) は、フリーズとして数えない。
        Start-Sleep -Milliseconds 2500
        $usage = Send-DapRequest $s 'netdbg.processUsage'
        $u = $usage.body
        $space = if ($ArchName -eq 'x86') { $u.wow64 -eq $true -and $u.addressUsed -gt 0 -and $u.largestFree -gt 0 -and $u.addressUsed + $u.largestFree -le $u.addressLimit } else { $u.wow64 -eq $false }
        Check $ArchName 'プロセスの使用量: メモリ、GDI / USER、ハンドル (32bit ならアドレス空間も) を読める' ($usage.success -and $u.privateBytes -gt 0 -and $u.workingSet -gt 0 -and $u.gdiObjects -gt 0 -and $u.userObjects -gt 0 -and $u.handles -gt 0 -and $u.gdiQuota -gt 0 -and $space) "($($usage | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $first.body.threadId })
        $frozen = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $frozen.body.threadId; levels = 20 }).body.stackFrames | ForEach-Object name)
        $notice = @($s.Events | Where-Object { $_.event -eq 'netdbg.uiFrozen' })
        Check $ArchName 'UI のフリーズ: 答えない UI のスレッドで止まり、理由を出す (止まっていた間は数えない)' ($frozen.body.reason -eq 'pause' -and $frozen.body.description -match '^UI not responding for \d+\.\d s$' -and @($frames | Where-Object { $_ -match '^Program\.Freeze\(' }).Count -eq 1 -and $notice.Count -eq 1 -and $notice[0].body.threadId -eq $frozen.body.threadId -and $notice[0].body.ms -ge 1000) "($($frozen.body | ConvertTo-Json -Compress) $($frames -join ' < '))"
        $setting = Send-DapRequest $s 'netdbg.setUiFreeze' @{ enabled = $false }
        Check $ArchName 'UI のフリーズ: 監視を切れる' ($setting.success -and $setting.body.enabled -eq $false -and $setting.body.thresholdMs -eq 1000) "($($setting | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'UI のフリーズ: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI も同じ監視で止まり、使用量を読める。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-freeze-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        $set = Invoke-McpTool $m 'ui_freeze' @{ thresholdMs = 1000 }
        $launched = Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 30000 }
        $usage = Invoke-McpTool $m 'process_usage' @{}
        Check $ArchName 'UI のフリーズ (MCP): 起動の前の ui_freeze の閾値で止まり、理由が分かり、使用量を読める' (-not $set.Error -and $set.Value.thresholdMs -eq 1000 -and $launched.Text -match 'UI not responding for 1\.\d s' -and -not $usage.Error -and $usage.Value.gdiObjects -gt 0) "($($launched.Text) / $($usage.Text))"
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'UI のフリーズ (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}