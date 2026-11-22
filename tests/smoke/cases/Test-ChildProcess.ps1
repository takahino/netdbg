# 段階 12: 子プロセス (Process.Start で起動したプロセス) を記録し、追う設定なら止めたまま startDebugging を頼む。
# 頼まれたアタッチ (__resumeChild) を 2 つめのエンジンで行い、子の行で止まることを確かめる (クライアントの役は、この試験)。
function Test-ChildProcess([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\children'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-children-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    $bin = Join-Path $work 'bin'
    $sources = @{}
    foreach ($name in 'Parent', 'Child') {
        $src = Join-Path $work $name
        New-Item -ItemType Directory -Force $src | Out-Null
        $sources[$name] = Join-Path $src 'Program.cs'
        [IO.File]::WriteAllText($sources[$name], [IO.File]::ReadAllText((Join-Path $template "$name\Program.cs.txt")))
        [IO.File]::WriteAllText((Join-Path $src "Smoke$name.csproj"), ([IO.File]::ReadAllText((Join-Path $template "$name\Smoke$name.csproj.txt")) -replace '\{TFM\}', $tfm))
        $ErrorActionPreference = 'Continue'
        & dotnet build $src -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o $bin -nologo -v q | Out-Null
        $ErrorActionPreference = 'Stop'
        if ($LASTEXITCODE -ne 0) {
            Check $ArchName '子プロセス: 対象をビルドできる' $false "($name, exit code $LASTEXITCODE)"
            return
        }
    }
    $app = Join-Path $bin 'SmokeParent.exe'
    $line = { param([string]$File, [string]$Marker) [Array]::FindIndex([IO.File]::ReadAllLines($File), [Predicate[string]] { param($l) $l -match "// @$Marker\b" }) + 1 }

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-children-$tfm-$ArchName.log"
    $childLog = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-children-child-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    $c = $null
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; supportsStartDebuggingRequest = $true })
        [void](Send-DapRequest $s 'launch' @{ program = $app; childProcesses = 'follow' })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $sources.Parent }; breakpoints = @(@{ line = (& $line $sources.Parent 'done') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $request = Wait-DapReverseRequest $s 'startDebugging' -TimeoutMs 60000
        $config = $request.arguments.configuration
        Check $ArchName '子プロセス: 追う設定では、子を止めたまま startDebugging (attach) を頼む' ($request.arguments.request -eq 'attach' -and $config.processId -gt 0 -and $config.__resumeChild -eq $true -and $config.name -match '^SmokeChild\.exe \(pid \d+\)$') "($($request.arguments | ConvertTo-Json -Compress -Depth 4))"

        # クライアントの役: 頼まれた設定で、2 つめのエンジンを子にアタッチさせる。
        $c = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$childLog`"")
        [void](Send-DapRequest $c 'initialize' @{ adapterID = 'netdbg' })
        $attach = @{}; foreach ($p in $config.PSObject.Properties) { $attach[$p.Name] = $p.Value }
        $attached = Send-DapRequest $c 'attach' $attach
        [void](Send-DapRequest $c 'setBreakpoints' @{ source = @{ path = $sources.Child }; breakpoints = @(@{ line = (& $line $sources.Child 'child') }) })
        [void](Send-DapRequest $c 'configurationDone')
        $childStop = Wait-DapEvent $c 'stopped' -TimeoutMs 60000
        $top = @((Send-DapRequest $c 'stackTrace' @{ threadId = $childStop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $value = (Send-DapRequest $c 'evaluate' @{ expression = 'value'; frameId = $top.id }).body.result
        Check $ArchName '子プロセス: 頼まれたアタッチで、子の行に止まれる' ($attached.success -and $childStop.body.reason -eq 'breakpoint' -and $top.name -eq 'Program.Main()' -and $top.line -eq (& $line $sources.Child 'child') -and $value -eq '7') "($($attached.message) $($top.name):$($top.line) value=$value)"
        [void](Send-DapRequest $c 'continue' @{ threadId = $childStop.body.threadId })
        [void](Wait-DapEvent $c 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $c)
        $c = $null

        $done = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
        $records = @((Send-DapRequest $s 'netdbg.journal' @{ kinds = @('process') }).body.records)
        $kind = if ($tfm -eq 'net48') { '.NET Framework' } else { '.NET' }
        $r = $records[0]
        Check $ArchName '子プロセス: 起動したファイル、引数、番号、種類、ビット数、呼び出し元を記録する' ($records.Count -eq 1 -and $r.fileName -like '*SmokeChild.exe' -and $r.arguments -eq '7' -and $r.processId -eq $config.processId -and $r.kind -eq $kind -and $r.bitness -eq $ArchName -and $r.followed -eq $true -and $r.caller.name -eq 'Program.Main()' -and $r.caller.source.path -eq $sources.Parent) "($($r | ConvertTo-Json -Compress -Depth 4))"
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $done.body.threadId; levels = 1 }).body.stackFrames)[0]
        $exit = (Send-DapRequest $s 'evaluate' @{ expression = 'child.ExitCode'; frameId = $frame.id }).body.result
        Check $ArchName '子プロセス: 子が終わったあと、親は先へ進む' ($done.body.reason -eq 'breakpoint' -and $exit -eq '7') "($exit)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $done.body.threadId })
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '子プロセス: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log / $childLog)"
        if ($c -and -not $c.Process.HasExited) { $c.Process.Kill() }
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
        Get-Process SmokeChild, SmokeParent -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$work*" } | Stop-Process -Force
    }
}