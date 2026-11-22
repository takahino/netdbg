# 段階 9: Edit and Continue (D-059)。対象を一時フォルダーに写してビルドし、止まっている間にソースを書き換えて
# netdbg.applyChanges で当てる。次の呼び出しから新しいコードになること (第 1 段階)、止まっている関数が止まった所から
# 新しいコードに乗り換わること (第 2 段階)、メソッドの追加、行のずれ、許されない書き換えとコンパイルのエラーを断ること、
# MCP から当てること、走っている間に (止めずに) 当てること。
function Test-Enc([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    Test-EncWith $ArchName $Exe $tfm 'sdk'
    # 古い形のプロジェクト (Visual Studio の MSBuild でビルドする .NET Framework の csproj)。Visual Studio が無ければ飛ばす。
    if ($tfm -eq 'net48' -and (Get-VsMsBuild)) {
        Test-EncWith $ArchName $Exe $tfm 'legacy'
        Test-EncWindowsPdb $ArchName $Exe
    }
}

# Visual Studio の MSBuild (vswhere で探す)。無ければ $null。
function Get-VsMsBuild {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    @(& $vswhere -latest -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe')[0]
}

# Edit and Continue の対象をビルドする。sdk は SDK 形式の csproj を dotnet build で、legacy は古い形の csproj
# (Legacy.csproj.txt と AssemblyInfo.cs) を Visual Studio の MSBuild で。終了コードを返す。
function Build-EncApp([string]$Folder, [string]$Out, [string]$Tfm, [string]$ArchName, [string]$Style, [string]$DebugType = 'portable') {
    $template = Join-Path $root 'tests\smoke\enc'
    $ErrorActionPreference = 'Continue'
    if ($Style -eq 'legacy') {
        New-Item -ItemType Directory -Force (Join-Path $Folder 'Properties') | Out-Null
        [IO.File]::WriteAllText((Join-Path $Folder 'SmokeEnc.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Legacy.csproj.txt')) -replace '\{PLATFORM\}', $ArchName -replace '\{DEBUGTYPE\}', $DebugType))
        [IO.File]::WriteAllText((Join-Path $Folder 'Properties\AssemblyInfo.cs'), [IO.File]::ReadAllText((Join-Path $template 'AssemblyInfo.cs.txt')))
        & (Get-VsMsBuild) (Join-Path $Folder 'SmokeEnc.csproj') -nologo -v:q -p:Configuration=Debug "-p:OutputPath=$Out\" | Out-Null
    } else {
        [IO.File]::WriteAllText((Join-Path $Folder 'SmokeEnc.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Enc.csproj.txt')) -replace '\{TFM\}', $Tfm))
        & dotnet build $Folder -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o $Out -nologo -v q | Out-Null
    }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    return $code
}

# 古い形のプロジェクトの既定 (DebugType full = Windows PDB) では当てず、portable にする直し方だけを伝える (D-078)。
function Test-EncWindowsPdb([string]$ArchName, [string]$Exe) {
    $template = Join-Path $root 'tests\smoke\enc'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-windowspdb-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force $work | Out-Null
    $source = Join-Path $work 'Program.cs'
    $v0 = [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt'))
    [IO.File]::WriteAllText($source, $v0)
    if ((Build-EncApp $work (Join-Path $work 'bin') 'net48' $ArchName 'legacy' 'full') -ne 0) {
        Check $ArchName 'EnC (古い形、Windows PDB): 対象をビルドできる' $false
        return
    }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = (Join-Path $work 'bin\SmokeEnc.exe') })
        $lines = $v0 -split "`r?`n"
        $loop = 1 + [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match '// @loop(?![\w-])' })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $loop }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        [IO.File]::WriteAllText($source, $v0.Replace('Console.WriteLine("loop "', 'Console.WriteLine("edited "'))
        $r = Send-DapRequest $s 'netdbg.applyChanges' @{ files = @($source) } -TimeoutMs 120000
        $details = @($r.body.details) -join ' | '
        Check $ArchName 'EnC (古い形、Windows PDB): 止まれて、当てるときは portable にする直し方だけを伝える' ($stop.body.reason -eq 'breakpoint' -and $r.body.status -eq 'failed' -and $details -match '<DebugType>portable</DebugType>' -and @($r.body.details).Count -eq 1) "($details)"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'EnC (古い形、Windows PDB): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}

# Edit and Continue の場面を、ビルドの形 (`$Style`: sdk / legacy) ごとに流す。
function Test-EncWith([string]$ArchName, [string]$Exe, [string]$tfm, [string]$Style) {
    # 古い形の場面は、確かめる項目の名前の頭に付けて分ける。
    $label = if ($Style -eq 'legacy') { '古い形の csproj: ' } else { '' }
    $template = Join-Path $root 'tests\smoke\enc'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-$Style-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    # ソース生成器は SDK 形式のプロジェクトだけ。
    if ($Style -eq 'sdk') { Test-EncGenerated $ArchName $Exe $tfm $template (Join-Path $work 'generated') }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    $v0 = [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt'))
    [IO.File]::WriteAllText($source, $v0)
    if ((Build-EncApp (Join-Path $work 'src') (Join-Path $work 'bin') $tfm $ArchName $Style) -ne 0) {
        Check $ArchName ($label + 'EnC: 対象をビルドできる') $false
        return
    }
    $app = Join-Path $work 'bin\SmokeEnc.exe'

    # 書き換え 1: Compute に 1 行足して Helper を呼ぶ (以降の行が 1 行ずれる)。Loop の出力の文字を変える (止まっている
    # Loop の乗り換え)。Main のあとに Helper を足す。
    $v1 = $v0.Replace("        return x + 1; // @compute", "        int y = Helper(x);`r`n        return y; // @compute").Replace('Console.WriteLine("loop "', 'Console.WriteLine("edited "')
    # Helper は、足した型 Added と、足した静的フィールド extra も使う (型とフィールドの追加)。
    $v1 = $v1.Substring(0, $v1.LastIndexOf('}')) + "`r`n    static int extra;`r`n`r`n    static int Helper(int x)`r`n    {`r`n        extra++;`r`n        return x * 100 + new Added().Zero();`r`n    }`r`n}`r`n`r`nclass Added`r`n{`r`n    public int Zero() => 0;`r`n}`r`n"
    if ($v0 -notmatch "`r`n") { $v1 = $v1 -replace "`r`n", "`n" }
    $lineOf = { param([string]$Text, [string]$Marker) $lines = $Text -split "`r?`n"; for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i] -match "// @$Marker(?![\w-])") { return $i + 1 } } }
    $apply = {
        param([string]$Text)
        [IO.File]::WriteAllText($source, $Text)
        Send-DapRequest $s 'netdbg.applyChanges' @{ files = @($source) } -TimeoutMs 120000
    }
    $top = {
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        @{ Stop = $stop; Frame = $frame }
    }
    $value = { param($At, [string]$Expression) (Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $At.Frame.id }).body.result }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-$Style-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName ($label + 'EnC: capabilities に netdbgSupportsApplyChanges') ($init.body.netdbgSupportsApplyChanges -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (& $lineOf $v0 'loop') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $at = & $top
        Check $ArchName ($label + 'EnC: Loop の 1 回目で止まる') ($at.Frame.name -eq 'Program.Loop()' -and (& $value $at 'r') -eq '11') "($($at.Frame.name):$($at.Frame.line))"

        $s.Events.Clear()
        $r = & $apply $v1
        # 通知は応答のあとに届くので、別の要求の応答まで読んでから数える。
        [void](Send-DapRequest $s 'threads')
        $applied = @($s.Events | Where-Object { $_.event -eq 'netdbg.changesApplied' })
        Check $ArchName ($label + 'EnC: 本体の更新、メソッドの追加、行のずれを当てる') ($r.success -and $r.body.status -eq 'applied') "($($r.body.status) $($r.message) $($r.body.problems | ConvertTo-Json -Compress -Depth 5))"
        Check $ArchName ($label + 'EnC: 当てたことを netdbg.changesApplied で知らせる') ($applied.Count -eq 1 -and $applied[0].body.status -eq 'applied')

        # VS Code と同じく、動いた行のブレークポイントを置き直す。
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (& $lineOf $v1 'loop') }, @{ line = (& $lineOf $v1 'below') }) })
        [void](Send-DapRequest $s 'continue' @{ threadId = $at.Stop.body.threadId })
        $at = & $top
        Check $ArchName ($label + 'EnC: 次の呼び出しから新しいコード (足した Helper を呼ぶ)') ($at.Frame.name -eq 'Program.Loop()' -and $at.Frame.line -eq (& $lineOf $v1 'loop') -and (& $value $at 'r') -eq '1000') "($($at.Frame.name):$($at.Frame.line) r=$(& $value $at 'r'))"
        Check $ArchName ($label + 'EnC: 足した型と静的フィールドも使え、足したフィールドを評価できる') ((& $value $at 'Program.extra') -eq '1' -and (& $value $at 'new Added().Zero()') -eq '0') "(extra=$(& $value $at 'Program.extra'))"
        Check $ArchName ($label + 'EnC: 止まっていた Loop は、止まった所から新しいコードに乗り換わる (ローカル変数は保たれる)') ((& $value $at 'total') -eq '11' -and (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'edited 0 11' }).Count -eq 1)) "(total=$(& $value $at 'total'))"

        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (& $lineOf $v1 'below') }) })
        [void](Send-DapRequest $s 'continue' @{ threadId = $at.Stop.body.threadId })
        $at = & $top
        Check $ArchName ($label + 'EnC: 行だけ動いたメソッドは、動いた行で止まって見える') ($at.Frame.name -eq 'Program.Below()' -and $at.Frame.line -eq (& $lineOf $v1 'below')) "($($at.Frame.name):$($at.Frame.line))"

        # 消したフィールド (名前を変えた) は、場所と理由つきで断る。対象には何も当てない。
        $v2 = $v1.Replace('static int counter;', 'static int counter2;').Replace('counter++', 'counter2++').Replace('+ counter)', '+ counter2)')
        $r = & $apply $v2
        Check $ArchName ($label + 'EnC: 消したメンバーは、理由と場所つきで断る') ($r.success -and $r.body.status -eq 'rude' -and @($r.body.problems | Where-Object { $_.message -match 'counter' }).Count -ge 1) "($($r.body.status) $($r.body.problems | ConvertTo-Json -Compress -Depth 5))"
        $v3 = $v1.Replace('extra++;', 'extra++ +;')
        $r = & $apply $v3
        Check $ArchName ($label + 'EnC: コンパイルのエラーは、場所つきで断る') ($r.success -and $r.body.status -eq 'errors' -and @($r.body.problems).Count -ge 1 -and $r.body.problems[0].line -gt 0) "($($r.body.status))"
        # 既にある型に足した静的フィールドの初期値は実行されない (型の初期化は終わっている) ので、黙って 0 で動かさずに断る。
        $r = & $apply ($v1.Replace('    static int extra;', "    static int extra;`r`n    static int seeded = 5;"))
        Check $ArchName ($label + 'EnC: 初期値つきの静的フィールドを既にある型に足すと、理由つきで断る') ($r.success -and $r.body.status -eq 'rude' -and @($r.body.problems | Where-Object { $_.message -match 'seeded' -and $_.message -match '初期値' }).Count -eq 1) "($($r.body.status) $($r.body.problems | ConvertTo-Json -Compress -Depth 5))"
        $r = & $apply $v1
        Check $ArchName ($label + 'EnC: 当てたソースに戻せば、当てるものは無い') ($r.success -and $r.body.status -eq 'noChanges') "($($r.body.status) $($r.message))"

        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $at.Stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $output = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName ($label + 'EnC: 最後まで新しいコードで走る') ($output -match 'edited 1 1011' -and $output -match 'edited 2 2011' -and $output -match 'end 2011 5 1') "($($output -replace "`r?`n", ' | '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName ($label + 'EnC: 例外なしで終わる') $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # MCP (--mcp): AI が直したソースを debug_apply_changes で当てる。ビルドし直した対象で、始めから。
    [IO.File]::WriteAllText($source, $v0)
    [void](Build-EncApp (Join-Path $work 'src') (Join-Path $work 'bin') $tfm $ArchName $Style)

    $mcpLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-mcp-$Style-$tfm-$ArchName.log"
    $m = Start-McpSession $Exe $mcpLog
    try {
        [void](Send-Mcp $m 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        $m.In.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}'); $m.In.Flush()
        $names = @((Send-Mcp $m 'tools/list' @{}).result.tools | ForEach-Object name)
        Check $ArchName ($label + 'EnC (MCP): ツールの一覧に debug_apply_changes がある') ($names -contains 'debug_apply_changes')
        [void](Invoke-McpTool $m 'breakpoint_set' @{ file = $source; line = (& $lineOf $v0 'loop') })
        [void](Invoke-McpTool $m 'debug_launch' @{ program = $app; waitMs = 30000 })
        [IO.File]::WriteAllText($source, $v1)
        $r = Invoke-McpTool $m 'debug_apply_changes' @{ files = @($source) } -TimeoutMs 120000
        Check $ArchName ($label + 'EnC (MCP): debug_apply_changes で当てる') (-not $r.Error -and $r.Value.status -eq 'applied') $r.Text
        [void](Invoke-McpTool $m 'breakpoint_remove' @{ file = $source; line = (& $lineOf $v0 'loop') })
        [void](Invoke-McpTool $m 'debug_continue' @{ waitMs = 30000 })
        $out = Invoke-McpTool $m 'debug_output' @{ lines = 50 }
        Check $ArchName ($label + 'EnC (MCP): 新しいコードで最後まで走る') ($out.Value.output -match 'end 2011 5 1') $out.Text
        $m.In.Close()
        if (-not $m.Process.WaitForExit(10000)) { $m.Process.Kill() }
    } catch {
        Check $ArchName ($label + 'EnC (MCP): 例外なしで終わる') $false "$($_.Exception.Message) (ログ: $mcpLog)"
        if (-not $m.Process.HasExited) { $m.Process.Kill() }
    }

    # 走っている間に当てる: エンジンが当てる間だけ止め、また動かす。利用者には止まったように見えない (stopped を送らない)。
    # 走っている Main は、Thread.Sleep から戻った次の文で新しいコードに乗り換わる。
    $runSource = Join-Path $work 'running\Program.cs'
    New-Item -ItemType Directory -Force (Split-Path $runSource) | Out-Null
    $r0 = [IO.File]::ReadAllText((Join-Path $template 'Running.cs.txt'))
    [IO.File]::WriteAllText($runSource, $r0)
    if ((Build-EncApp (Join-Path $work 'running') (Join-Path $work 'running-bin') $tfm $ArchName $Style) -ne 0) {

        Check $ArchName ($label + 'EnC (走行中): 対象をビルドできる') $false "(exit code $LASTEXITCODE)"
        return
    }
    $runLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-running-$Style-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$runLog`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = (Join-Path $work 'running-bin\SmokeEnc.exe') })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $runSource }; breakpoints = @(@{ line = (& $lineOf $r0 'tick') }) })
        [void](Send-DapRequest $s 'configurationDone')
        # 先に、止まっている行そのものを書き換える: その行は古いコードのまま実行し、書き換えていない次の文で乗り換える。
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $r1 = $r0.Replace('Console.WriteLine("tick "', 'Console.WriteLine("edited "')
        [IO.File]::WriteAllText($runSource, $r1)
        $r = Send-DapRequest $s 'netdbg.applyChanges' @{ files = @($runSource) } -TimeoutMs 120000
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $runSource }; breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'output' -Where { param($e) $e.body.output -match 'edited 1 1' } -TimeoutMs 30000)
        $said = @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match '書き換えていない文に来るまで' }).Count
        Check $ArchName ($label + 'EnC: 止まっている行を書き換えると、その行は古いコードで実行し、次の文で乗り換える') ($r.body.status -eq 'applied' -and @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'tick 0 1' }).Count -eq 1 -and $said -eq 1) "($($r.body.status) said=$said)"

        # 走っている間に、Value と、走っている Main の出力の文字を書き換える。
        $r2 = $r1.Replace('return 1; // @value', 'return 2; // @value').Replace('Console.WriteLine("edited "', 'Console.WriteLine("running "')
        [IO.File]::WriteAllText($runSource, $r2)
        $r = Send-DapRequest $s 'netdbg.applyChanges' @{ files = @($runSource) } -TimeoutMs 120000
        Check $ArchName ($label + 'EnC (走行中): 止めずに当てる') ($r.success -and $r.body.status -eq 'applied') "($($r.body.status) $($r.message) $($r.body.problems | ConvertTo-Json -Compress -Depth 5))"
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $stops = @($s.Events | Where-Object { $_.event -eq 'stopped' })
        $output = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName ($label + 'EnC (走行中): 利用者には止まったように見えない (stopped が無い)') ($stops.Count -eq 0) "($($stops | ConvertTo-Json -Compress -Depth 5))"
        Check $ArchName ($label + 'EnC (走行中): 動いたまま新しいコードになり (走っていた Main も乗り換わる)、最後まで走る') ($output -match 'running \d+ 2' -and $output -match '(?m)^end' -and $output -notmatch '(tick|edited) \d+ 2') "($($output -replace "`r?`n", ' | '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName ($label + 'EnC (走行中): 例外なしで終わる') $false "$($_.Exception.Message) (ログ: $runLog)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}

# ソース生成器を使う対象: 型にフィールドを足すと、補助が生成器を走らせ直し、生成されたメソッドの変化も当てる。
# 生成器は自前のもの (<ProjectReference OutputItemType="Analyzer">)。補助は project.assets.json から見つける。
function Test-EncGenerated([string]$ArchName, [string]$Exe, [string]$Tfm, [string]$Template, [string]$Work) {
    New-Item -ItemType Directory -Force (Join-Path $Work 'generator'), (Join-Path $Work 'app') | Out-Null
    [IO.File]::WriteAllText((Join-Path $Work 'generator\DescribeGenerator.cs'), [IO.File]::ReadAllText((Join-Path $Template 'Generator.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $Work 'generator\SmokeEncGenerator.csproj'), [IO.File]::ReadAllText((Join-Path $Template 'Generator.csproj.txt')))
    $source = Join-Path $Work 'app\Program.cs'
    $v0 = [IO.File]::ReadAllText((Join-Path $Template 'Generated.cs.txt'))
    [IO.File]::WriteAllText($source, $v0)
    [IO.File]::WriteAllText((Join-Path $Work 'app\SmokeEnc.csproj'), ([IO.File]::ReadAllText((Join-Path $Template 'Generated.csproj.txt')) -replace '\{TFM\}', $Tfm -replace '\{ARCH\}', $ArchName))
    $ErrorActionPreference = 'Continue'
    # -o を付けない (付けると生成器もモジュールの横に出る。ふだんのビルドと同じく、生成器のプロジェクトの bin から探させる)。
    & dotnet build (Join-Path $Work 'app') -c Debug -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'EnC (生成器): 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $line = @($v0 -split "`r?`n" | Select-String -Pattern '// @fields' | ForEach-Object LineNumber)[0]
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-enc-generated-$Tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = (Join-Path $Work "app\bin\Debug\$Tfm\win-$ArchName\SmokeEnc.exe") })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $line }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        [IO.File]::WriteAllText($source, $v0.Replace('    public int X;', "    public int X;`r`n    public int Y;"))
        $r = Send-DapRequest $s 'netdbg.applyChanges' @{ files = @($source) } -TimeoutMs 120000
        Check $ArchName 'EnC (生成器): 生成器を走らせ直し、生成されたコードの変化も当てる (古いままの知らせが無い)' ($r.success -and $r.body.status -eq 'applied' -and @($r.body.details).Count -eq 0) "($($r.body | ConvertTo-Json -Compress -Depth 5))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $output = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'EnC (生成器): 生成されたメソッドが、足したフィールドを返す' ($output -match 'fields 0 X,Y' -and $output -match 'fields 2 X,Y') "($($output -replace "`r?`n", ' | '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'EnC (生成器): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
