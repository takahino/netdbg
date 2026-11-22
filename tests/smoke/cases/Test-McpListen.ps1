# 段階 8: --mcp-listen と --mcp-connect。DAP のクライアント (VS Code の役) が始めたセッションを、MCP からも操作する (D-053)。

# MCP の HTTP に JSON-RPC を 1 つ送り、答え (JSON) を返す。トークンが違えば例外 (状態の番号つき)。
function Send-McpHttp([string]$Url, [string]$Token, [string]$Method, $Params) {
    $script:mcpHttpId = 1 + [int]$script:mcpHttpId
    $body = @{ jsonrpc = '2.0'; id = $script:mcpHttpId; method = $Method; params = $Params } | ConvertTo-Json -Depth 20 -Compress
    $headers = @{}
    if ($Token) { $headers.Authorization = "Bearer $Token" }
    return Invoke-RestMethod -Uri $Url -Method Post -Body ([Text.Encoding]::UTF8.GetBytes($body)) -ContentType 'application/json' -Headers $headers -TimeoutSec 120
}

function Invoke-McpHttpTool([string]$Url, [string]$Token, [string]$Name, [hashtable]$Arguments = @{}) {
    $answer = Send-McpHttp $Url $Token 'tools/call' @{ name = $Name; arguments = $Arguments }
    $text = $answer.result.content[0].text
    $value = try { $text | ConvertFrom-Json } catch { $text }
    return @{ Error = [bool]$answer.result.isError; Value = $value; Text = $text }
}

function Test-McpListen([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-mcplisten-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', '--mcp-listen', "--log=`"$log`"")
    $bridge = $null
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $endpoint = (Wait-DapEvent $s 'netdbg.mcpEndpoint').body
        $file = Join-Path $env:LOCALAPPDATA "netdbg\mcp\$($endpoint.enginePid).json"
        Check $ArchName 'MCP listen: initialize のあと、接続先 (URL、トークン) が DAP のイベントで届く' ($endpoint.url -match '^http://127\.0\.0\.1:\d+/mcp$' -and $endpoint.token.Length -eq 32 -and (Test-Path $file)) "($($endpoint.url))"
        $refused = try { [void](Send-McpHttp $endpoint.url 'wrong' 'tools/list' @{}); 0 } catch { [int]$_.Exception.Response.StatusCode }
        Check $ArchName 'MCP listen: トークンの違う要求は 401' ($refused -eq 401) "($refused)"
        $names = @((Send-McpHttp $endpoint.url $endpoint.token 'tools/list' @{}).result.tools | ForEach-Object name)
        Check $ArchName 'MCP listen: 起動のツールは出さない (IDE が始めたセッションを使う)' ($names -contains 'evaluate' -and $names -notcontains 'debug_launch') "($($names.Count))"

        # IDE が起動し、自分のブレークポイント (@values) を置く。AI も同じファイルに (@loop) 置く。
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'values') }) })
        $r = Invoke-McpHttpTool $endpoint.url $endpoint.token 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'loop') }
        Check $ArchName 'MCP listen: AI もブレークポイントを置ける' (-not $r.Error) $r.Text
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $status = (Invoke-McpHttpTool $endpoint.url $endpoint.token 'debug_status').Value
        Check $ArchName 'MCP listen: AI のブレークポイントで止まり、IDE にも MCP にも同じ停止が見える' ($status.state -eq 'stopped' -and $status.at.line -eq (Get-MarkerLine $source 'loop') -and $stop.body.threadId -eq $status.threadId) "($($status | ConvertTo-Json -Compress))"
        $r = Invoke-McpHttpTool $endpoint.url $endpoint.token 'evaluate' @{ expression = 'i' }
        Check $ArchName 'MCP listen: IDE が始めたセッションで式を評価できる' ($r.Value.result -eq '0') $r.Text

        # AI のブレークポイントを外して続行すると、IDE のブレークポイント (同じファイル) で止まる。IDE にも stopped が届く。
        [void](Invoke-McpHttpTool $endpoint.url $endpoint.token 'breakpoint_remove' @{ file = $source; line = (Get-MarkerLine $source 'loop') })
        $r = Invoke-McpHttpTool $endpoint.url $endpoint.token 'debug_continue' @{ waitMs = 30000 }
        $seen = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $seen.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'MCP listen: AI が外しても IDE のブレークポイントは残り、AI の続行で IDE にも停止が届く' ($r.Value.at.line -eq (Get-MarkerLine $source 'values') -and $top.line -eq (Get-MarkerLine $source 'values')) "($($r.Value.at.line) / $($top.line))"

        # 外の AI からの橋渡し (--mcp-connect): 標準入出力の MCP が、同じセッションに届く。
        $info = New-Object System.Diagnostics.ProcessStartInfo
        $info.FileName = $Exe
        $info.Arguments = "--mcp-connect=$($endpoint.enginePid)"
        $info.UseShellExecute = $false
        $info.RedirectStandardInput = $true
        $info.RedirectStandardOutput = $true
        $info.CreateNoWindow = $true
        $bridge = [System.Diagnostics.Process]::Start($info)
        $in = New-Object System.IO.StreamWriter($bridge.StandardInput.BaseStream, (New-Object System.Text.UTF8Encoding($false)))
        $in.WriteLine('{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}')
        $in.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
        $in.WriteLine('{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"debug_status","arguments":{}}}')
        $in.Flush()
        $first = $bridge.StandardOutput.ReadLineAsync(); [void]$first.Wait(30000)
        $second = $bridge.StandardOutput.ReadLineAsync(); [void]$second.Wait(30000)
        $viaBridge = (($second.Result | ConvertFrom-Json).result.content[0].text | ConvertFrom-Json)
        Check $ArchName 'MCP connect: 橋渡しから同じセッションの状態が読める' (($first.Result | ConvertFrom-Json).result.serverInfo.name -eq 'netdbg' -and $viaBridge.state -eq 'stopped' -and $viaBridge.at.line -eq (Get-MarkerLine $source 'values')) "($($second.Result))"
        $in.Close()
        [void]$bridge.WaitForExit(10000)

        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void]$s.Process.WaitForExit(15000)
        Check $ArchName 'MCP listen: エンジンが終わると、接続先のファイルを消す' ($s.Process.HasExited -and -not (Test-Path $file)) "($file)"
    } catch {
        Check $ArchName 'MCP listen: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
    } finally {
        if ($bridge -and -not $bridge.HasExited) { $bridge.Kill() }
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
