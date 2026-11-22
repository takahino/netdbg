# 段階 12: オブジェクトを保持している経路 (GC のルートから、どの参照で生きているか)。
function Test-RetentionPath([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\retention'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-retention-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeRetention.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Retention.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName '保持している経路: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeRetention.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $stopLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @stop' }) + 1

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-retention-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $stopLine }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $path = { param([string]$Expression) Send-DapRequest $s 'netdbg.retentionPath' @{ expression = $Expression; frameId = $frame.id } -TimeoutMs 60000 }

        $kept = & $path 'cache.Items[1]'
        $k = $kept.body
        $steps = @($k.path | ForEach-Object { $_.next }) -join ' '
        Check $ArchName '保持している経路: 静的なフィールドから、リストの中の要素までの参照をたどる' ($init.body.netdbgSupportsRetentionPath -eq $true -and $k.found -eq $true -and $k.path[-1].type -eq 'Order' -and $k.target.type -eq 'Order' -and $steps -match '\.Items' -and $steps -match '\[1\]' -and @($k.path | Where-Object { $_.type -eq 'Cache' }).Count -eq 1 -and $k.root.kind -match 'handle') "($($kept | ConvertTo-Json -Compress -Depth 6))"
        $boxed = (& $path 'holders[1].Value').body
        $last = @($boxed.path)[-2]
        Check $ArchName '保持している経路: 構造体の配列の、要素の欄からの参照' ($boxed.found -eq $true -and $last.type -eq 'Holder[]' -and $last.next -eq '[1].Value') "($($boxed | ConvertTo-Json -Compress -Depth 6))"
        $local = (& $path 'local').body
        Check $ArchName '保持している経路: ローカル変数から生きているなら、スタックとその変数の名前' ($local.found -eq $true -and $local.length -eq 0 -and $local.root.kind -eq 'stack' -and $local.root.local.name -eq 'local' -and $local.root.local.function -eq 'Program.Main()') "($($local | ConvertTo-Json -Compress -Depth 6))"
        $null = Send-DapRequest $s 'netdbg.retentionPath' @{ expression = 'cache.Items[1]'; frameId = $frame.id; maxDepth = 1 } -TimeoutMs 60000
        $short = (Send-DapRequest $s 'netdbg.retentionPath' @{ expression = 'cache.Items[1]'; frameId = $frame.id; maxDepth = 1 } -TimeoutMs 60000).body
        Check $ArchName '保持している経路: 深さの上限より長い経路は、見つからないと理由つきで答える' ($short.found -eq $false -and $short.reason -match 'No path of 1 references') "($($short | ConvertTo-Json -Compress -Depth 4))"
        $bad = Send-DapRequest $s 'netdbg.retentionPath' @{ expression = 'cache.Items.Count'; frameId = $frame.id }
        Check $ArchName '保持している経路: 値型には理由つきで断る' (-not $bad.success -and $bad.message -match '値型') "($($bad.message))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '保持している経路: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI も同じ経路を読める。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-retention-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = $stopLine })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 })
        $r = Invoke-McpTool $m 'retention_path' @{ expression = 'cache.Items[1]' }
        Check $ArchName '保持している経路 (MCP): retention_path で経路を読める' (-not $r.Error -and $r.Value.found -eq $true -and $r.Value.path[-1].type -eq 'Order') $r.Text
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName '保持している経路 (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}