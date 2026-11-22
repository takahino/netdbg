# 書き込みの見張り (D-093): 選んだフィールドが書かれたら止まる。ref で別のメソッドから書いても、GC のあとに書いても、
# ソースの無い .NET の中 (int.TryParse の out) から書いても止まる。.NET Framework は理由を付けて断る。MCP の break_on_write も。
# data-write の試験アプリを起動し、見張りを置く前の行 (@data-before) で止める。initialize の答え、process の通知、止まった
# 通知、いちばん上のフレームを返す。
function Start-DataWrite($Session, [string]$App, [string]$Source) {
    $init = Send-DapRequest $Session 'initialize' @{ adapterID = 'netdbg' }
    [void](Send-DapRequest $Session 'launch' @{ program = $App; args = @('data-write') })
    [void](Send-DapRequest $Session 'setBreakpoints' @{ source = @{ path = $Source }; breakpoints = @(@{ line = (Get-MarkerLine $Source 'data-before') }) })
    [void](Send-DapRequest $Session 'configurationDone')
    $process = Wait-DapEvent $Session 'process' -TimeoutMs 30000
    $stop = Wait-DapEvent $Session 'stopped' -TimeoutMs 30000
    $frame = @((Send-DapRequest $Session 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)[0]
    return @{ Init = $init; Process = $process; Stop = $stop; Frame = $frame }
}
function Test-DataBreakpoint([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $framework = $script:flavor.Dir -match '48'
    $s = Start-DapSession -Exe $Exe
    try {
        $start = Start-DataWrite $s $app $source
        $init = $start.Init
        $stop = $start.Stop
        $frame = $start.Frame
        $info = Send-DapRequest $s 'dataBreakpointInfo' @{ expression = 'order.Status'; frameId = $frame.id }
        if ($framework) {
            Check $ArchName '書き込みの見張り: .NET Framework は理由を付けて断る' ($init.body.supportsDataBreakpoints -and $null -eq $info.body.dataId -and "$($info.body.description)" -match '\.NET Framework') "($($info.body | ConvertTo-Json -Compress))"
            [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
            [void](Stop-DapSession $s)
            return
        }
        $set = Send-DapRequest $s 'setDataBreakpoints' @{ breakpoints = @(@{ dataId = $info.body.dataId; accessType = 'write' }) }
        Check $ArchName '書き込みの見張り: フィールドを選んで置ける' ($info.body.dataId -and $set.body.breakpoints[0].verified -eq $true) "($($info.body | ConvertTo-Json -Compress) $($set.body | ConvertTo-Json -Compress -Depth 3))"
        $expected = @(
            @{ Name = 'ref で別のメソッドから書くと、書いたメソッドで止まる'; Top = 'SetByRef' },
            @{ Name = 'GC のあとに書いても止まる (オブジェクトを固定している)'; Top = '^Program\.Main' },
            @{ Name = 'ソースの無い .NET の中から out で書いても止まる'; Top = '^System\.Number\.' }
        )
        $thread = $stop.body.threadId
        foreach ($e in $expected) {
            $s.Events.Clear()
            [void](Send-DapRequest $s 'continue' @{ threadId = $thread })
            $hit = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $thread = $hit.body.threadId
            $names = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread }).body.stackFrames | ForEach-Object name)
            Check $ArchName "書き込みの見張り: $($e.Name)" ($hit.body.reason -eq 'data breakpoint' -and $names[0] -match $e.Top) "($($hit.body | ConvertTo-Json -Compress) / $($names[0..3] -join ' <- '))"
        }
        $s.Events.Clear()
        [void](Send-DapRequest $s 'continue' @{ threadId = $thread })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '書き込みの見張り: 見張ったまま、対象は最後まで動く' ($exited.body.exitCode -eq 7) "(終了コード $($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '書き込みの見張り: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
    if ($framework) { return }

    # 見張りを置いたまま切り離すと、見張りは外れる (デバッグ レジスターが残ると、デバッガーの無い対象が次の書き込みで落ちる)。
    $s = Start-DapSession -Exe $Exe
    try {
        $start = Start-DataWrite $s $app $source
        $target = [Diagnostics.Process]::GetProcessById([int]$start.Process.body.systemProcessId)
        # 終わる前にハンドルを開いておく (開いていないと終了コードを読めない)。
        [void]$target.Handle
        $frame = $start.Frame
        # 変数の欄の形 (開いたオブジェクトの番号と、フィールドの名前) で引く。VS Code と --ui はこの形で送る。
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $order = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables | Where-Object { $_.name -eq 'order' })[0]
        $info = Send-DapRequest $s 'dataBreakpointInfo' @{ variablesReference = $order.variablesReference; name = 'Status' }
        Check $ArchName '書き込みの見張り: 変数の欄のフィールド (番号と名前) からも選べる' ($null -ne $info.body.dataId) "($($info.body | ConvertTo-Json -Compress))"
        [void](Send-DapRequest $s 'setDataBreakpoints' @{ breakpoints = @(@{ dataId = $info.body.dataId; accessType = 'write' }) })
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $false })
        [void](Stop-DapSession $s)
        $ended = $target.WaitForExit(30000)
        Check $ArchName '書き込みの見張り: 置いたまま切り離しても、対象は最後まで動く' ($ended -and $target.ExitCode -eq 7) "(終了コード $(if ($ended) { $target.ExitCode } else { '(終わらない)' }))"
        if (-not $ended) { $target.Kill() }
    } catch {
        Check $ArchName '書き込みの見張り: 切り離し: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP の break_on_write。
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-mcp-databreak-$ArchName.log"
    $m = Start-McpSession $Exe $log
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'data-before') })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; args = @('data-write'); waitMs = 30000 })
        $r = Invoke-McpTool $m 'break_on_write' @{ expression = 'order.Status' }
        [void](Invoke-McpTool $m 'breakpoint_clear')
        $hit = Invoke-McpTool $m 'debug_continue' @{ waitMs = 30000 }
        Check $ArchName 'MCP: break_on_write で、書いたメソッドで止まる' (-not $r.Error -and $r.Value.watching -eq 'order.Status' -and $hit.Value.reason -eq 'data breakpoint' -and $hit.Value.at.function -match 'SetByRef') "($($r.Text) / $($hit.Text))"
        $r = Invoke-McpTool $m 'break_on_write' @{ clear = $true }
        $end = Invoke-McpTool $m 'debug_continue' @{ waitMs = 30000 }
        Check $ArchName 'MCP: break_on_write の clear で外すと、そのまま終わる' (-not $r.Error -and $end.Value.state -eq 'exited') "($($r.Text) / $($end.Text))"
        $m.In.Close()
        [void]$m.Process.WaitForExit(10000)
    } catch {
        Check $ArchName 'MCP: break_on_write: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
    } finally {
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}