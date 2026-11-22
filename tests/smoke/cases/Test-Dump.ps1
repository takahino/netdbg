# 段階 12: クラッシュダンプを開く (launch の dumpPath。読むだけ)。対象を例外のフィルターの中で待たせてダンプに書き出し、
# エンジンで開いて、止まった場所、スタック、変数、例外、ヒープを読む。x86 では、64bit のツールで書いたダンプ (WOW64) も試す。
function Test-Dump([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\dumps'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dumps-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeDump.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'SmokeDump.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'ダンプ: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $lines = [IO.File]::ReadAllLines($source)
    $at = { param([string]$Marker) [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match "// @$Marker\b" }) + 1 }

    # ダンプを書く。x86 の対象は、32bit の PowerShell で書くと 32bit のダンプ、64bit の PowerShell で書くと WOW64 のダンプになる。
    $writer = Join-Path $root 'tests\smoke\write-dump.ps1'
    $shells = if ($ArchName -eq 'x86') { [ordered]@{ x86 = "$env:windir\SysWOW64\WindowsPowerShell\v1.0\powershell.exe"; wow64 = "$env:windir\System32\WindowsPowerShell\v1.0\powershell.exe" } } else { [ordered]@{ x64 = "$env:windir\System32\WindowsPowerShell\v1.0\powershell.exe" } }
    $app = Start-WaitingApp (Join-Path $work 'bin\SmokeDump.exe')
    try {
        foreach ($kind in $shells.Keys) {
            $ErrorActionPreference = 'Continue'
            & $shells[$kind] -NoProfile -ExecutionPolicy Bypass -File $writer -ProcessId $app.Id -Path (Join-Path $work "$kind.dmp") | Out-Null
            $ErrorActionPreference = 'Stop'
        }
    } finally {
        $app.Kill()
    }

    $first = $true
    foreach ($kind in $shells.Keys) {
        $dumpPath = Join-Path $work "$kind.dmp"
        $label = if ($kind -eq 'wow64') { 'ダンプ (64bit のツールで書いた 32bit のもの): ' } else { 'ダンプ: ' }
        if (-not (Test-Path $dumpPath)) {
            Check $ArchName "${label}ダンプを書き出せる" $false "($dumpPath)"
            continue
        }
        $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dumps-$kind-$tfm-$ArchName.log"
        $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
        try {
            $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
            [void](Send-DapRequest $s 'launch' @{ dumpPath = $dumpPath })
            $done = Send-DapRequest $s 'configurationDone'
            $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
            $thread = $stop.body.threadId
            $threads = @((Send-DapRequest $s 'threads').body.threads)
            Check $ArchName "${label}開くと、例外を投げている途中のスレッドで止まった形になる" ($init.body.netdbgSupportsDumps -eq $true -and $done.success -and $stop.body.reason -eq 'exception' -and $stop.body.text -match "System\.InvalidOperationException' occurred in SmokeDump\.(exe|dll)" -and $threads[0].id -eq $thread -and $threads[0].name -eq 'Main Thread') "($($done.message) $($stop.body | ConvertTo-Json -Compress) $($threads | ConvertTo-Json -Compress))"

            $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $thread }).body.stackFrames)
            $shown = ($frames | ForEach-Object { "$($_.name):$($_.line)" }) -join ' < '
            $named = { param([string]$Name) @($frames | Where-Object { $_.name -eq $Name }) }
            $validate = (& $named 'Program.Validate()')[0]
            $process = (& $named 'Program.Process()')[-1]
            Check $ArchName "${label}コール スタックをソースの行で読める" ((& $named 'Program.Hang()')[0].line -eq (& $at 'hang') -and $validate.line -eq (& $at 'throw') -and $validate.source.path -eq $source -and $frames[-1].name -eq 'Program.Main()') "($shown)"

            $locals = { param($Frame) $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $Frame.id }).body.scopes)[0]; $map = @{}; foreach ($v in @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)) { $map[$v.name] = $v.value }; $map }
            $inValidate = & $locals $validate
            $inProcess = & $locals $process
            $customer = (Send-DapRequest $s 'evaluate' @{ expression = 'order.Customer'; frameId = $validate.id }).body.result
            Check $ArchName "${label}ローカル変数、引数、フィールドをたどる式を読める" ($inValidate.limit -eq '20' -and $inProcess.batch -eq '"batch-7"' -and $inProcess.retries -eq '3' -and $customer -eq '"C2"') "(Validate $($inValidate | ConvertTo-Json -Compress); Process $($inProcess | ConvertTo-Json -Compress); order.Customer $customer)"

            if ($first) {
                $info = Send-DapRequest $s 'exceptionInfo' @{ threadId = $thread }
                Check $ArchName "${label}例外の型と文言" ($info.success -and $info.body.exceptionId -eq 'CLR/System.InvalidOperationException' -and $info.body.details.message -eq 'Amount 31.0 exceeds 20') "($($info.message) $($info.body | ConvertTo-Json -Compress -Depth 4))"
                $orders = @((Send-DapRequest $s 'netdbg.heapStats').body.types | Where-Object { $_.name -eq 'Order' })
                Check $ArchName "${label}ヒープの統計に、型ごとの数" ($orders.Count -eq 1 -and $orders[0].count -eq 3) "($($orders | ConvertTo-Json -Compress))"
                $continued = Send-DapRequest $s 'continue' @{ threadId = $thread }
                $called = Send-DapRequest $s 'evaluate' @{ expression = 'order.ToString()'; frameId = $validate.id }
                Check $ArchName "${label}続行と関数の評価は、読むだけだと理由を付けて断る" (-not $continued.success -and $continued.message -match 'クラッシュダンプは読むだけ' -and -not $called.success -and $called.message -match 'クラッシュダンプは読むだけ') "($($continued.message) / $($called.message))"
            }
            [void](Send-DapRequest $s 'disconnect')
            $code = Stop-DapSession $s
            if ($first) {
                Check $ArchName "${label}閉じるとエンジンが終わる (終了コード 0)" ($code -eq 0) "($code)"
            }
        } catch {
            Check $ArchName "${label}例外なしで終わる" $false "$($_.Exception.Message) (ログ: $log)"
            if (-not $s.Process.HasExited) { $s.Process.Kill() }
        }
        $first = $false
    }

    # MCP (--mcp): AI が debug_open_dump で開き、スタックを読む。
    $dumpPath = Join-Path $work "$(@($shells.Keys)[0]).dmp"
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-dumps-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        $opened = Invoke-McpTool $m 'debug_open_dump' @{ dumpPath = $dumpPath }
        $stack = Invoke-McpTool $m 'stack_trace' @{}
        Check $ArchName 'ダンプ (MCP): debug_open_dump で開くと止まった形になり、スタックを読める' (-not $opened.Error -and $opened.Text -match 'stopped' -and -not $stack.Error -and $stack.Text -match 'Program\.Validate') "($($opened.Text) / $($stack.Text))"
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName 'ダンプ (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}