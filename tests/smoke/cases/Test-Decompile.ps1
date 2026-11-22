# 段階 10: エンジンの中の逆コンパイル (D-060)。PDB の無い Release ビルドの対象を起動し、エンジンが逆コンパイルした
# ソースのファイルで、止まる、ブレークポイント、引数とローカル変数、ステップができること。decompile: off なら今までどおり。
function Test-Decompile([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\decompile'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-decompile-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    [IO.File]::WriteAllText((Join-Path $work 'src\Program.cs'), [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeDecompile.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Decompile.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Release -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName '逆コンパイル: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    # ソースは消しておく (PDB が無いので、どのみち使われない)。
    Remove-Item -Recurse -Force (Join-Path $work 'src')
    $app = Join-Path $work 'bin\SmokeDecompile.exe'
    $moduleName = if ($tfm -eq 'net48') { 'SmokeDecompile.exe' } else { 'SmokeDecompile.dll' }
    Check $ArchName '逆コンパイル: 対象に PDB が無い' (@(Get-ChildItem (Join-Path $work 'bin') -Filter *.pdb).Count -eq 0)
    $top = {
        param($ThreadId)
        @((Send-DapRequest $s 'stackTrace' @{ threadId = $ThreadId; levels = 2 }).body.stackFrames)
    }
    $value = { param($Frame, [string]$Expression) (Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $Frame.id }).body.result }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-decompile-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName '逆コンパイル: capabilities に netdbgSupportsDecompile' ($init.body.netdbgSupportsDecompile -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app; stopAtEntry = $true })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $thread = $stop.body.threadId
        $frames = & $top $thread
        Check $ArchName '逆コンパイル: 入口で止まり、フレームが逆コンパイルしたソースの行を指す' ($frames[0].name -eq 'Program.Main()' -and $frames[0].source.path -match '\\netdbg\\decompiled\\' -and $frames[0].line -gt 0) "($($frames[0] | ConvertTo-Json -Compress -Depth 4))"

        $files = @((Send-DapRequest $s 'netdbg.decompile' @{ module = $moduleName }).body.files)
        $programFile = @($files | Where-Object { (Split-Path $_ -Leaf) -eq 'Program.cs' })[0]
        Check $ArchName '逆コンパイル: netdbg.decompile がソースのファイルを返す' ($programFile -and (Test-Path $programFile)) "($($files -join ', '))"
        $lines = @(Get-Content $programFile -Encoding UTF8)
        # Compute の本体は 1 文 (Release のビルドで y はまとめられる)。
        $computeLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match 'return x \* 2 \+ 1;' }) + 1
        Check $ArchName '逆コンパイル: メソッドの本体が C# になる (IL の行が無い)' ($computeLine -gt 0 -and -not ($lines -match '// IL_')) "($($lines -join ' | '))"
        $bp = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $programFile }; breakpoints = @(@{ line = $computeLine }) }
        Check $ArchName '逆コンパイル: ソースのファイルの行にブレークポイントを置ける' ($bp.body.breakpoints[0].verified -eq $true -and $bp.body.breakpoints[0].line -eq $computeLine) "($($bp.body.breakpoints | ConvertTo-Json -Compress))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $thread })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = & $top $stop.body.threadId
        Check $ArchName '逆コンパイル: ブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint' -and $frames[0].name -eq 'Program.Compute()' -and $frames[0].line -eq $computeLine) "($($stop.body.reason) $($frames[0].name):$($frames[0].line))"
        Check $ArchName '逆コンパイル: 引数を名前で読める' ((& $value $frames[0] 'x') -eq '10') "(x=$(& $value $frames[0] 'x'))"
        Check $ArchName '逆コンパイル: 呼び出し元のローカル変数を、型から付けた名前で読める' ((& $value $frames[1] 'num') -eq '0' -and (& $value $frames[1] 'num2') -eq '0') "(num=$(& $value $frames[1] 'num') num2=$(& $value $frames[1] 'num2'))"

        [void](Send-DapRequest $s 'stepOut' @{ threadId = $thread })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = & $top $thread
        Check $ArchName '逆コンパイル: ステップ アウトで呼び出し元に戻る' ($stop.body.reason -eq 'step' -and $frames[0].name -eq 'Program.Main()' -and $frames[0].line -gt 0) "($($stop.body.reason) $($frames[0].name):$($frames[0].line))"
        # ステップ イン: Main の文を進め、次の Compute の呼び出しの中に入る。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $programFile }; breakpoints = @() })
        $entered = $false
        for ($i = 0; $i -lt 12 -and -not $entered; $i++) {
            [void](Send-DapRequest $s 'stepIn' @{ threadId = $thread })
            $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $frames = & $top $thread
            $entered = $frames[0].name -eq 'Program.Compute()'
        }
        Check $ArchName '逆コンパイル: ステップ インで、逆コンパイルしたメソッドの中に入る' ($entered -and $frames[0].line -eq $computeLine -and (& $value $frames[0] 'x') -eq '11') "($($frames[0].name):$($frames[0].line))"

        # 捕まえた変数を持つラムダ: 表示用のクラスは出ず、ラムダの中でも外でも、捕まえた変数を名前で読める。
        # ラムダの本体は複数の行で書く。最初の文 (limit と比べる if) の行で止める。
        $lambdaLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match 'if \(.*v.*limit' }) + 1
        $returnLine = [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match 'return seen;' }) + 1
        Check $ArchName '逆コンパイル: 捕まえた変数を持つラムダが、ラムダに戻る (表示用のクラスが出ない)' ($lambdaLine -gt 0 -and $returnLine -gt 0 -and ($lines -match 'v =>').Count -eq 1 -and -not ($lines -match 'DisplayClass')) "($($lines -join ' | '))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $programFile }; breakpoints = @(@{ line = $lambdaLine }) })
        [void](Send-DapRequest $s 'continue' @{ threadId = $thread })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = & $top $stop.body.threadId
        Check $ArchName '逆コンパイル: ラムダの行で止まり、捕まえた変数を名前で読める' ($frames[0].line -eq $lambdaLine -and (& $value $frames[0] 'limit') -eq '4' -and (& $value $frames[0] 'seen') -eq '0') "($($frames[0].name):$($frames[0].line) limit=$(& $value $frames[0] 'limit') seen=$(& $value $frames[0] 'seen'))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $programFile }; breakpoints = @(@{ line = $returnLine }) })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = & $top $stop.body.threadId
        Check $ArchName '逆コンパイル: 外のメソッドでも、捕まえた変数を名前で読める' ($frames[0].line -eq $returnLine -and (& $value $frames[0] 'seen') -eq '2') "($($frames[0].name):$($frames[0].line) seen=$(& $value $frames[0] 'seen'))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $programFile }; breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $output = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName '逆コンパイル: 最後まで走る' ($output -match 'total 69' -and $output -match 'seen 2') "($($output -replace "`r?`n", ' | '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '逆コンパイル: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # decompile: off なら、今までどおり (ソースの無いフレーム)。
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode')
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; stopAtEntry = $true; decompile = 'off' })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = & $top $stop.body.threadId
        Check $ArchName '逆コンパイル: decompile: off なら逆コンパイルしない' ($frames[0].name -eq 'Program.Main()' -and -not $frames[0].source) "($($frames[0] | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '逆コンパイル (off): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI が decompile で読んで、その行にブレークポイントを置き、止まる。
    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-decompile-mcp-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        $names = @((Send-Mcp $m 'tools/list' @{}).result.tools | ForEach-Object name)
        Check $ArchName '逆コンパイル (MCP): ツールの一覧に decompile がある' ($names -contains 'decompile')
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; stopAtEntry = $true; waitMs = 30000 })
        $r = Invoke-McpTool $m 'decompile' @{}
        Check $ArchName '逆コンパイル (MCP): 対象のモジュールの一覧' (-not $r.Error -and @($r.Value.modules | Where-Object { (Split-Path $_.path -Leaf) -eq $moduleName }).Count -eq 1) $r.Text
        $r = Invoke-McpTool $m 'decompile' @{ module = $moduleName; type = 'Program'; method = 'Compute' }
        $hit = @($r.Value.lines | Where-Object { $_.text -match 'return x \* 2 \+ 1;' })
        Check $ArchName '逆コンパイル (MCP): メソッドの行を読める' (-not $r.Error -and $hit.Count -eq 1 -and (Test-Path $r.Value.file) -and @($r.Value.lines).Count -lt 10) $r.Text
        if ($hit.Count -eq 1) {
            [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $r.Value.file; line = $hit[0].line })
            $stop = Invoke-McpTool $m 'debug_continue' @{ waitMs = 30000 }
            Check $ArchName '逆コンパイル (MCP): 読んだ行のブレークポイントで止まる' ($stop.Value.state -eq 'stopped' -and $stop.Value.at.line -eq $hit[0].line -and $stop.Value.at.file -eq $r.Value.file) $stop.Text
        }
        [void](Invoke-McpTool $m 'debug_terminate')
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName '逆コンパイル (MCP): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }
}
