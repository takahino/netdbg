# 段階 12: ダイアログを出した箇所 (MessageBox.Show、ShowDialog) を記録し、条件に合えば出す前に止まる。
function Test-DialogLog([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0-windows' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\dialogs'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dialogs-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeDialogs.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Dialogs.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'ダイアログ: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeDialogs.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $done = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @done' }) + 1

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dialogs-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $done }) })
        $set = (Send-DapRequest $s 'netdbg.setDialogLog' @{ breakOn = @(@{ text = 'delete order' }) }).body
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 10 }).body.stackFrames | ForEach-Object name)
        Check $ArchName 'ダイアログ: 条件に合うダイアログを出す前に止まる' ($init.body.netdbgSupportsDialogLog -eq $true -and $set.enabled -eq $true -and $stop.body.reason -eq 'breakpoint' -and $stop.body.description -eq 'Dialog: Confirm: Delete order 7?' -and $stop.body.hitBreakpointIds[0] -eq $set.breakOn[0].id -and @($frames | Where-Object { $_ -eq 'Program.Confirm()' }).Count -eq 1) "($($stop.body | ConvertTo-Json -Compress) $($frames -join ' < '))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $last = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $records = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('dialog') }).body.records)
        Check $ArchName 'ダイアログ: 止まらないものも、文字、題、呼び出し元と記録する' ($last.body.reason -eq 'breakpoint' -and $records.Count -eq 3 -and $records[0].kind -eq 'messageBox' -and $records[0].text -eq 'Saved 3 orders' -and $records[0].caption -eq 'Orders' -and $records[0].caller.name -eq 'Program.Main()' -and $records[0].caller.source.path -eq $source -and -not $records[0].stopped -and $records[1].stopped -eq $true -and $records[1].caller.name -eq 'Program.Confirm()') "($($records | ConvertTo-Json -Compress -Depth 4))"
        Check $ArchName 'ダイアログ: ShowDialog はフォームの型を記録する (ShowDialog() の中の ShowDialog(owner) は 1 つ)' ($records[2].kind -eq 'dialog' -and $records[2].form -eq 'EditForm' -and $records[2].caller.name -eq 'Program.Main()') "($($records[2] | ConvertTo-Json -Compress -Depth 4))"
        $events = @($s.Events | Where-Object { $_.event -eq 'netdbg.dialogShown' })
        $outputs = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'ダイアログ: 記録ごとに netdbg.dialogShown を送り、デバッグ コンソールに出す' ($events.Count -eq 3 -and $outputs -match 'Dialog: Orders: Saved 3 orders') "($($events.Count))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $last.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'ダイアログ: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI も同じ記録を読み、止める条件を置ける。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dialogs-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        $b = Invoke-McpTool $m 'dialog_log' @{ breakOn = @(@{ caller = 'Confirm' }) }
        $launched = Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 }
        $r = Invoke-McpTool $m 'dialog_log' @{}
        Check $ArchName 'ダイアログ (MCP): dialog_log で止める条件を置き、記録を読める' (-not $b.Error -and $launched.Text -match 'Dialog: Confirm: Delete order 7\?' -and @($r.Value.dialogs).Count -eq 2 -and $r.Value.dialogs[0].text -eq 'Saved 3 orders') "($($launched.Text) / $($r.Text))"
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'ダイアログ (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}