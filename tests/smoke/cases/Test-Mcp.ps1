# 段階 8: MCP (--mcp)。標準入出力で 1 行に 1 つの JSON-RPC を流し、AI のクライアントと同じ道でデバッグする (D-053)。

# MCP のエンジンを起動する。標準入力は BOM の無い UTF-8 にする (Process は、コンソールが UTF-8 のとき先頭に BOM を書く)。
function Start-McpSession([string]$Exe, [string]$Log) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Exe
    $info.Arguments = "--mcp `"--log=$Log`""
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $info.StandardOutputEncoding = New-Object System.Text.UTF8Encoding($false)
    $process = [System.Diagnostics.Process]::Start($info)
    $writer = New-Object System.IO.StreamWriter($process.StandardInput.BaseStream, (New-Object System.Text.UTF8Encoding($false)))
    return @{ Process = $process; In = $writer; Id = 0; Pending = $null }
}

# 要求を送り、同じ id の答えを待つ (読み取りは時間切れつき。取り損ねた読み取りは次に持ち越す)。
function Send-Mcp($Session, [string]$Method, $Params, [int]$TimeoutMs = 60000) {
    $Session.Id++
    $id = $Session.Id
    $Session.In.WriteLine((@{ jsonrpc = '2.0'; id = $id; method = $Method; params = $Params } | ConvertTo-Json -Depth 20 -Compress))
    $Session.In.Flush()
    $deadline = [DateTime]::Now.AddMilliseconds($TimeoutMs)
    while ($true) {
        if (-not $Session.Pending) { $Session.Pending = $Session.Process.StandardOutput.ReadLineAsync() }
        $left = [int]($deadline - [DateTime]::Now).TotalMilliseconds
        if ($left -le 0 -or -not $Session.Pending.Wait($left)) { throw "MCP の答えが $TimeoutMs ms で来ません ($Method)。" }
        $line = $Session.Pending.Result
        $Session.Pending = $null
        if ($null -eq $line) { throw "MCP のエンジンが終わりました ($Method)。" }
        $message = $line | ConvertFrom-Json
        if ($message.id -eq $id) { return $message }
    }
}

# ツールを呼び、結果 (JSON の文字列なら読んだもの) と、失敗したかを返す。
function Invoke-McpTool($Session, [string]$Name, [hashtable]$Arguments = @{}, [int]$TimeoutMs = 60000) {
    $answer = Send-Mcp $Session 'tools/call' @{ name = $Name; arguments = $Arguments } $TimeoutMs
    $text = $answer.result.content[0].text
    $value = try { $text | ConvertFrom-Json } catch { $text }
    return @{ Error = [bool]$answer.result.isError; Value = $value; Text = $text }
}

function Test-Mcp([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-mcp-$ArchName.log"
    $s = Start-McpSession $Exe $log
    try {
        $init = Send-Mcp $s 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } }
        Check $ArchName 'MCP: initialize に名前と版が返る' ($init.result.serverInfo.name -eq 'netdbg' -and $init.result.protocolVersion -eq '2025-06-18')
        $s.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $s.In.Flush()
        $names = @((Send-Mcp $s 'tools/list' @{}).result.tools | ForEach-Object name)
        Check $ArchName 'MCP: ツールの一覧に起動とデバッグのツールがある' ((@('debug_launch', 'breakpoint_set', 'debug_wait_for_stop', 'variables', 'evaluate', 'debug_stop_context', 'trace_log') | Where-Object { $names -notcontains $_ }).Count -eq 0) "($($names -join ', '))"
        $unknown = Send-Mcp $s 'no/such/method' @{}
        Check $ArchName 'MCP: 知らないメソッドは -32601' ($unknown.error.code -eq -32601)

        $r = Invoke-McpTool $s 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'loop'); logMessage = 'i={i}' }
        Check $ArchName 'MCP: トレースポイントを置ける' (-not $r.Error) $r.Text
        [void](Invoke-McpTool $s 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'values') })
        $r = Invoke-McpTool $s 'debug_launch' @{ program = $app; waitMs = 30000 }
        Check $ArchName 'MCP: 起動して、ブレークポイントで止まった場所が返る' ($r.Value.state -eq 'stopped' -and $r.Value.reason -eq 'breakpoint' -and $r.Value.at.line -eq (Get-MarkerLine $source 'values')) $r.Text
        $trace = (Invoke-McpTool $s 'trace_log').Value
        Check $ArchName 'MCP: トレースポイントは止まらずに式を記録する' ((@($trace.entries | ForEach-Object message) -join ',') -eq 'i=0,i=1,i=2' -and $trace.cursor -eq 3) "($(@($trace.entries | ForEach-Object message) -join ','))"

        $context = (Invoke-McpTool $s 'debug_stop_context' @{ contextLines = 1 }).Value
        $current = @($context.source | Where-Object { $_.current })
        Check $ArchName 'MCP: 停止の文脈に、場所、前後のソース、ローカル変数' ($context.at.function -eq 'Order.Total()' -and @($context.source).Count -eq 3 -and $current[0].text -match '@values' -and @($context.locals | Where-Object { $_.name -eq 'sum' }).Count -eq 1) $(($context | ConvertTo-Json -Depth 3 -Compress).Substring(0, 200))
        $r = Invoke-McpTool $s 'variables' @{ depth = 1 }
        $this = @($r.Value.variables | Where-Object { $_.name -eq 'this' })[0]
        Check $ArchName 'MCP: variables は深さを指定して開ける' (@($this.members | Where-Object { $_.name -eq 'id' -and $_.value -eq '3' }).Count -eq 1) $r.Text.Substring(0, [Math]::Min(200, $r.Text.Length))
        $r = Invoke-McpTool $s 'evaluate' @{ expression = 'names.Count + id' }
        Check $ArchName 'MCP: evaluate は C# の値と型' ($r.Value.result -eq '4' -and $r.Value.type -eq 'int') $r.Text
        $r = Invoke-McpTool $s 'set_variable' @{ target = 'sum'; value = '100' }
        $after = (Invoke-McpTool $s 'evaluate' @{ expression = 'sum' }).Value.result
        Check $ArchName 'MCP: set_variable で値を変えられる' (-not $r.Error -and $after -eq '100') "($($r.Text) / $after)"
        $r = Invoke-McpTool $s 'stack_trace' @{ levels = 5 }
        Check $ArchName 'MCP: stack_trace に呼び出し元まで' (@($r.Value.frames).Count -ge 2 -and $r.Value.frames[1].function -eq 'Program.Main()') $r.Text.Substring(0, [Math]::Min(200, $r.Text.Length))
        $r = Invoke-McpTool $s 'debug_step' @{ kind = 'over' }
        Check $ArchName 'MCP: debug_step は止まった次の行を返す' ($r.Value.state -eq 'stopped' -and $r.Value.reason -eq 'step' -and $r.Value.at.line -eq (Get-MarkerLine $source 'values') + 1) $r.Text
        $r = Invoke-McpTool $s 'evaluate' @{ expression = 'nope' }
        Check $ArchName 'MCP: 評価できない式は、理由つきの失敗' ($r.Error -and $r.Text -match 'nope') $r.Text

        [void](Invoke-McpTool $s 'breakpoint_clear')
        $r = Invoke-McpTool $s 'debug_continue' @{ waitMs = 30000 }
        Check $ArchName 'MCP: 続行して終わると、終了コード' ($r.Value.state -eq 'exited' -and $r.Value.exitCode -eq 7) $r.Text
        $output = (Invoke-McpTool $s 'debug_output' @{ search = 'result=' }).Value
        Check $ArchName 'MCP: debug_output に対象の出力、search で絞れる' ($output.output -match '^result=' -and $output.output -notmatch 'stderr' -and $output.cursor -ge 2) "($($output.output))"
        $since = (Invoke-McpTool $s 'debug_output' @{ sinceCursor = $output.cursor }).Value
        Check $ArchName 'MCP: カーソルより後の出力だけ (いまは無い)' ($since.output -eq '') "($($since.output))"

        # 終わったあと、もう一度起動できる (新しいセッションで)。止まる場所が無いので、そのまま終わる。
        $r = Invoke-McpTool $s 'debug_launch' @{ program = $app; waitMs = 30000 }
        Check $ArchName 'MCP: 終わったあと、もう一度起動できる' ($r.Value.state -eq 'exited' -and $r.Value.exitCode -eq 7) $r.Text
        # 例外の設定: 投げられた FormatException で止まり、停止の文脈に例外が入る。そのあと、誰も捕まえない例外でも止まる。
        $r = Invoke-McpTool $s 'exception_breakpoints' @{ mode = 'all'; types = @('System.FormatException') }
        Check $ArchName 'MCP: exception_breakpoints で止まる例外を選べる' (-not $r.Error) $r.Text
        $r = Invoke-McpTool $s 'debug_launch' @{ program = $app; args = @('throw'); waitMs = 30000 }
        Check $ArchName 'MCP: 選んだ型の例外が投げられたら止まり、自分のコードの行 (userFrame) が分かる' ($r.Value.reason -eq 'exception' -and $r.Value.userFrame.line -eq (Get-MarkerLine $source 'caught')) $r.Text
        $context = (Invoke-McpTool $s 'debug_stop_context' @{ contextLines = 0 }).Value
        $mine = @($context.source | Where-Object { $_.current })
        Check $ArchName 'MCP: 例外で止まった文脈に、例外の型と文言、自分のコードのソース' ($context.exception.type -eq 'CLR/System.FormatException' -and $context.exception.message -and $mine[0].text -match '@caught') "($($context.exception | ConvertTo-Json -Compress))"
        $r = Invoke-McpTool $s 'debug_continue' @{ waitMs = 30000 }
        Check $ArchName 'MCP: 誰も捕まえない例外でも止まる' ($r.Value.reason -eq 'exception' -and $r.Value.at.line -eq (Get-MarkerLine $source 'thrown')) $r.Text
        $r = Invoke-McpTool $s 'debug_terminate'
        Check $ArchName 'MCP: debug_terminate で対象が終わる' ($r.Value.state -eq 'exited') $r.Text
        # 起動の前のビルド: 失敗したら、ビルドのエラーを理由に起動しない。
        $r = Invoke-McpTool $s 'debug_launch' @{ program = $app; buildProject = (Join-Path $root 'tests\apps\NoSuchProject\NoSuchProject.csproj') }
        Check $ArchName 'MCP: buildProject のビルドが失敗したら、理由つきで起動しない' ($r.Error -and $r.Text -match 'ビルドに失敗しました') $r.Text.Substring(0, [Math]::Min(200, $r.Text.Length))
        $s.In.Close()
        Check $ArchName 'MCP: 標準入力を閉じると、エンジンが終わる' ($s.Process.WaitForExit(10000) -and $s.Process.ExitCode -eq 0) "(終了コード $($s.Process.ExitCode))"
    } catch {
        Check $ArchName 'MCP: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
    } finally {
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
