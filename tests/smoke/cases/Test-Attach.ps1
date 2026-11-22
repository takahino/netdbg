# 段階 1: 動いているプロセスにつなぎ、ブレークポイントで止め、切り離す。切り離したあとも対象は動き続ける。
function Test-Attach([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-attach-$ArchName.log"
    $target = Start-WaitingApp $app
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        # processId は文字列でも受け付ける (VS Code のプロセス選択は文字列を返す)。
        $r = Send-DapRequest $s 'attach' @{ processId = "$($target.Id)" }
        Check $ArchName 'attach が成功する' ($r.success -eq $true) "($($r.message))"
        $r = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'tick') }) }
        $r = Send-DapRequest $s 'configurationDone'
        Check $ArchName 'attach のあとの configurationDone' ($r.success -eq $true) "($($r.message))"
        # process の通知 (netcoredbg と同じ形)。拡張は、この番号でメモリのグラフと UI のフリーズの見張りを始める。
        $process = Wait-DapEvent $s 'process'
        Check $ArchName 'attach: process の通知に、プロセス番号と exe のパス' ($process.body.systemProcessId -eq $target.Id -and $process.body.startMethod -eq 'attach' -and $process.body.isLocalProcess -eq $true -and $process.body.name -like '*.exe') "($($process.body | ConvertTo-Json -Compress))"
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName 'アタッチした対象がブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint')
        $module = Wait-DapEvent $s 'module' -Where { param($m) $m.body.module.name -eq $script:flavor.Module }
        Check $ArchName 'すでに読み込まれていたモジュールが届く' ($module.body.module.symbolStatus -eq 'Symbols loaded.')
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName 'アタッチした対象のフレーム名' ($frames[0].name -eq 'Program.Main()') "($($frames[0].name))"
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)[0].variablesReference }).body.variables)
        $tick = $vars | Where-Object { $_.name -eq 'tick' }
        Check $ArchName 'アタッチした対象のローカル変数' ($null -ne $tick -and $tick.type -eq 'int') "($(($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"
        $threads = @((Send-DapRequest $s 'threads').body.threads)
        $names = @($threads | ForEach-Object name)
        Check $ArchName 'アタッチした対象のスレッド名' ($names -contains 'worker-a') "($($names -join ', '))"
        # Main Thread は、プロセスの最初のスレッド (Main を実行している)。アタッチでは、知らされる順が当てにならない。
        $main = $threads | Where-Object { $_.name -eq 'Main Thread' } | Select-Object -First 1
        $mainFrames = if ($main) { @((Send-DapRequest $s 'stackTrace' @{ threadId = $main.id }).body.stackFrames) | ForEach-Object name } else { @() }
        Check $ArchName 'アタッチ: Main Thread は Main を実行しているスレッドで、一覧の先頭' ($threads[0].name -eq 'Main Thread' -and ($mainFrames -contains 'Program.Main()')) "($($names -join ', ') / $($mainFrames -join ' <- '))"

        $r = Send-DapRequest $s 'disconnect'
        Check $ArchName 'disconnect で切り離す' ($r.success -eq $true)
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
        # 対象を道連れにしていないこと。止まったまま (スレッドが止められたまま) になっていないこと。
        Start-Sleep -Milliseconds 500
        Check $ArchName '切り離したあとも対象は生きている' (-not $target.HasExited)
        $target.Refresh()
        $suspended = @($target.Threads | Where-Object { $_.ThreadState -eq 'Wait' -and "$($_.WaitReason)" -eq 'Suspended' }).Count
        Check $ArchName '切り離したあと、対象のスレッドは止められたままではない' ($suspended -eq 0) "(止められたスレッド $suspended)"
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        if (-not $target.HasExited) { $target.Kill() }
    }
}

# 観察専用のアタッチ (D-089): 値の書き換えと、利用者の頼む関数の評価を断る。フィールドとローカル変数は読める。
function Test-AttachObserveOnly([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $target = Start-WaitingApp $app
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode')
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'attach' @{ processId = $target.Id; observeOnly = $true }
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'tick') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $process = Wait-DapEvent $s 'process'
        Check $ArchName '観察専用: attach が成功し、process の通知に印が付く' ($r.success -and $process.body.netdbgObserveOnly -eq $true) "($($r.message) $($process.body | ConvertTo-Json -Compress))"
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)[0].variablesReference
        $local = Send-DapRequest $s 'evaluate' @{ expression = 'tick'; frameId = $frames[0].id; context = 'watch' }
        $set = Send-DapRequest $s 'setVariable' @{ variablesReference = $scope; name = 'tick'; value = '5' }
        $call = Send-DapRequest $s 'evaluate' @{ expression = 'tick.ToString()'; frameId = $frames[0].id; context = 'watch' }
        $property = Send-DapRequest $s 'evaluate' @{ expression = 'System.Environment.ProcessorCount'; frameId = $frames[0].id; context = 'watch' }
        Check $ArchName '観察専用: ローカル変数は読める' ($local.success -and $local.body.type -eq 'int') "($($local.message))"
        Check $ArchName '観察専用: 値の書き換えは理由つきで断る' (-not $set.success -and "$($set.message)" -match '観察専用') "($($set.message))"
        Check $ArchName '観察専用: メソッドの呼び出しとプロパティの getter は理由つきで断る' (-not $call.success -and "$($call.message)" -match '観察専用' -and -not $property.success -and "$($property.message)" -match '観察専用') "($($call.message) / $($property.message))"
        $after = Send-DapRequest $s 'evaluate' @{ expression = 'tick'; frameId = $frames[0].id; context = 'watch' }
        Check $ArchName '観察専用: 断ったあとも値は変わっていない' ($after.body.result -eq $local.body.result) "($($local.body.result) -> $($after.body.result))"
        # 評価の欄の代入 (関数の評価を通らずに書く): ローカル変数、静的フィールド、++ も断る。
        $ticks = (Send-DapRequest $s 'evaluate' @{ expression = 'Program.WorkerTicks'; frameId = $frames[0].id; context = 'watch' }).body.result
        $writes = @('tick = 5', 'Program.WorkerTicks = 0', 'tick++') | ForEach-Object { Send-DapRequest $s 'evaluate' @{ expression = $_; frameId = $frames[0].id; context = 'repl' } }
        $tickAfter = (Send-DapRequest $s 'evaluate' @{ expression = 'tick'; frameId = $frames[0].id; context = 'watch' }).body.result
        $ticksAfter = (Send-DapRequest $s 'evaluate' @{ expression = 'Program.WorkerTicks'; frameId = $frames[0].id; context = 'watch' }).body.result
        Check $ArchName '観察専用: 評価の欄の代入 (ローカル変数、静的フィールド、++) も理由つきで断り、値は変わらない' (@($writes | Where-Object { $_.success -or "$($_.message)" -notmatch '観察専用' }).Count -eq 0 -and $tickAfter -eq $local.body.result -and $ticksAfter -eq $ticks) "($(($writes | ForEach-Object { "$($_.success):$($_.message)" }) -join ' / ') / tick $($local.body.result) -> $tickAfter / WorkerTicks $ticks -> $ticksAfter)"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
        # MCP: 観察専用で繋いだら、窓の操作 (ui_window) と入力 (ui_set_text) も断る (断る判定は窓を探す前)。
        $m = Start-McpSession $Exe (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-observe-mcp-$ArchName.log")
        try {
            [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
            $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
            $attached = Invoke-McpTool $m 'debug_attach' @{ processId = $target.Id; observeOnly = $true; waitMs = 0 }
            $window = Invoke-McpTool $m 'ui_window' @{ action = 'front' }
            $text = Invoke-McpTool $m 'ui_set_text' @{ name = 'nameBox'; value = 'x' }
            Check $ArchName '観察専用 (MCP): 窓の操作 (ui_window) と入力 (ui_set_text) を理由つきで断る' (-not $attached.Error -and $window.Error -and $window.Text -match '観察専用' -and $text.Error -and $text.Text -match '観察専用') "($($attached.Text) / $($window.Text) / $($text.Text))"
            $m.In.Close()
            if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
        } catch {
            Check $ArchName '観察専用 (MCP): 例外なしで終わる' $false $_.Exception.Message
            if (-not $m.Process.HasExited) { $m.Process.Kill() }
        }
    } catch {
        Check $ArchName '観察専用: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        if (-not $target.HasExited) { $target.Kill() }
    }
}
