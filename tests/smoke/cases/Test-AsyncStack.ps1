# 段階 12: 非同期の論理コールスタック (await のあとで再開した async メソッドを、誰が await していたか)。
function Test-AsyncStack([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\asyncstack'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-asyncstack-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeAsyncStack.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'AsyncStack.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName '非同期のスタック: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeAsyncStack.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $at = { param([string]$Marker) [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match "// @$Marker\b" }) + 1 }

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-asyncstack-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (& $at 'inner') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        $label = [Array]::FindIndex($frames, [Predicate[object]] { param($f) $f.name -eq '[Awaited by]' })
        $logical = if ($label -ge 0) { @($frames[($label + 1)..($label + 3)]) } else { @() }
        $shown = ($frames | ForEach-Object { "$($_.name):$($_.line)" }) -join ' < '
        Check $ArchName '非同期のスタック: 再開した async メソッドの下に、区切りと await している呼び出し元を挟む' ($frames[0].name -eq 'Program.<Inner>d__0.MoveNext()' -and $label -eq 1 -and $frames[1].presentationHint -eq 'label') "($shown)"
        Check $ArchName '非同期のスタック: 呼び出し元は近い順で、それぞれ await している行' ($logical.Count -eq 3 -and $logical[0].name -eq 'Program.<Middle>d__1.MoveNext()' -and $logical[0].line -eq (& $at 'middle') -and $logical[1].name -eq 'Program.<Outer>d__2.MoveNext()' -and $logical[1].line -eq (& $at 'outer') -and $logical[2].name -eq 'Program.<Main>d__3.MoveNext()' -and $logical[2].line -eq (& $at 'main') -and $logical[0].source.path -eq $source) "($shown)"
        $scopes = Send-DapRequest $s 'scopes' @{ frameId = $logical[0].id }
        $eval = Send-DapRequest $s 'evaluate' @{ expression = 'x'; frameId = $logical[0].id }
        Check $ArchName '非同期のスタック: 論理的なフレームは変数が空で、式は理由つきで断る' ($scopes.success -and @($scopes.body.scopes).Count -eq 0 -and -not $eval.success -and $eval.message -match 'await') "(scopes $($scopes | ConvertTo-Json -Compress -Depth 4); eval $($eval | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '非同期のスタック: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI のスタックにも出る。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-asyncstack-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = (& $at 'inner') })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 60000 })
        $r = Invoke-McpTool $m 'stack_trace' @{}
        Check $ArchName '非同期のスタック (MCP): stack_trace に await している呼び出し元が出る' (-not $r.Error -and $r.Text -match 'Awaited by' -and $r.Text -match '<Outer>d__2') $r.Text
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName '非同期のスタック (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}