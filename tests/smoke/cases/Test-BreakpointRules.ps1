# 段階 11: ブレークポイントに当たったときの判定をエンジンが行う (拡張の中継から移した)。ログポイント、ヒットカウント、
# 条件の欄の印 (#once、#after、#changed、#caller)、区間の計測、値の記録、Step Into Target、値の変化で止める、を一つの対象で順に確かめる。
function Test-BreakpointRules([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\hits'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-hits-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    New-Item -ItemType Directory -Force (Join-Path $work 'src') | Out-Null
    $source = Join-Path $work 'src\Program.cs'
    [IO.File]::WriteAllText($source, [IO.File]::ReadAllText((Join-Path $template 'Program.cs.txt')))
    [IO.File]::WriteAllText((Join-Path $work 'src\SmokeHits.csproj'), ([IO.File]::ReadAllText((Join-Path $template 'Hits.csproj.txt')) -replace '\{TFM\}', $tfm))
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $work 'src') -c Debug -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work 'bin') -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        Check $ArchName 'ブレークポイントの判定: 対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    $app = Join-Path $work 'bin\SmokeHits.exe'
    $lines = [IO.File]::ReadAllLines($source)
    $at = { param([string]$Marker) [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l -match "// @$Marker\b" }) + 1 }
    $once = & $at 'once'
    $done = & $at 'done'
    $breakpoints = @(
        @{ line = (& $at 'logpoint'); logMessage = 'i={i} total={total} in $FUNCTION' },
        @{ line = (& $at 'track'); logMessage = '#track:total;total * 2' },
        @{ line = (& $at 'log'); condition = '#caller B' },
        @{ line = (& $at 'changed'); condition = '#changed x' },
        @{ line = (& $at 'hit'); hitCondition = '==3' },
        @{ line = $once; condition = '#once' },
        @{ line = (& $at 'notyet'); condition = "#after Program.cs:$done" },
        @{ line = (& $at 'dependent'); condition = "#after Program.cs:$once" },
        @{ line = (& $at 'timerstart'); logMessage = '#timer-start:block' },
        @{ line = (& $at 'timerend'); logMessage = '#timer-end:block' },
        @{ line = (& $at 'nested') },
        @{ line = $done }
    )
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-hits-$tfm-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName 'ブレークポイントの判定: ログポイント、ヒットカウント、Step Into Target に対応していると答える' ($init.body.supportsLogPoints -eq $true -and $init.body.supportsHitConditionalBreakpoints -eq $true -and $init.body.supportsStepInTargetsRequest -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = $breakpoints })
        $watched = Send-DapRequest $s 'netdbg.setValueWatches' @{ watches = @(@{ name = 'counter'; sites = @(@{ path = $source; line = (& $at 'write') }) }) }
        [void](Send-DapRequest $s 'configurationDone')
        $stop = { $e = Wait-DapEvent $s 'stopped' -TimeoutMs 60000; $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $e.body.threadId; levels = 2 }).body.stackFrames); @{ Event = $e; Frames = $frames } }
        $value = { param($Frame, [string]$Expression) (Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $Frame.id }).body.result }
        $go = { param($Stopped) [void](Send-DapRequest $s 'continue' @{ threadId = $Stopped.Event.body.threadId }) }

        $first = & $stop
        Check $ArchName 'ブレークポイントの判定: #caller は、そのメソッドが呼び出し元のときだけ止まる' ($first.Frames[0].line -eq (& $at 'log') -and $first.Frames[1].name -eq 'Program.B()') "($($first.Frames[0].line) $($first.Frames[1].name))"
        $outputs = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'ブレークポイントの判定: ログポイントは止まらずに、式と $FUNCTION を埋めて出す' ($outputs -match 'i=1 total=0 in Program\.Main\(\)' -and $outputs -match 'i=3 total=3 in Program\.Main\(\)') "($($outputs -replace "`r?`n", ' | '))"
        $tracked = @($s.Events | Where-Object { $_.event -eq 'netdbg.trackedValues' })
        Check $ArchName 'ブレークポイントの判定: #track: は値を netdbg.trackedValues で渡す' ($tracked.Count -eq 1 -and $tracked[0].body.values[0].expression -eq 'total' -and $tracked[0].body.values[0].value -eq '6' -and $tracked[0].body.values[1].value -eq '12') "($($tracked | ConvertTo-Json -Compress -Depth 4))"
        & $go $first

        $changed = & $stop
        $outputs = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'ブレークポイントの判定: #changed は値が変わったときだけ止まり、前後の値を出す' ($changed.Frames[0].line -eq (& $at 'changed') -and (& $value $changed.Frames[0] 'k') -eq '2' -and $outputs -match 'x: 0 → 5') "($($changed.Frames[0].line) k=$(& $value $changed.Frames[0] 'k'))"
        & $go $changed

        $hit = & $stop
        Check $ArchName 'ブレークポイントの判定: ヒットカウント ==3 は 3 回目だけ止まる' ($hit.Frames[0].line -eq (& $at 'hit') -and (& $value $hit.Frames[0] 'n') -eq '2') "($($hit.Frames[0].line) n=$(& $value $hit.Frames[0] 'n'))"
        & $go $hit

        $first_once = & $stop
        $removes = @($s.Events | Where-Object { $_.event -eq 'netdbg.removeBreakpoint' })
        Check $ArchName 'ブレークポイントの判定: #once は 1 回止まり、外すよう netdbg.removeBreakpoint で頼む' ($first_once.Frames[0].line -eq $once -and (& $value $first_once.Frames[0] 'm') -eq '0' -and $removes.Count -eq 1 -and $removes[0].body.line -eq $once) "($($first_once.Frames[0].line) $($removes.Count))"
        & $go $first_once

        $dependent = & $stop
        Check $ArchName 'ブレークポイントの判定: #after は、相手で止まったあとだけ効く (止まっていない相手なら止まらない)' ($dependent.Frames[0].line -eq (& $at 'dependent')) "($($dependent.Frames[0].line))"
        & $go $dependent

        $nested = & $stop
        $frameId = $nested.Frames[0].id
        $targets = @((Send-DapRequest $s 'stepInTargets' @{ frameId = $frameId }).body.targets)
        Check $ArchName 'Step Into Target: 行の中の呼び出しを、実行する順に並べる' ($nested.Frames[0].line -eq (& $at 'nested') -and (($targets | ForEach-Object label) -join ',') -eq 'Program.Twice,Program.Twice,Program.Plus') "($(($targets | ForEach-Object label) -join ','))"
        [void](Send-DapRequest $s 'stepIn' @{ threadId = $nested.Event.body.threadId; targetId = $targets[2].id })
        $into = & $stop
        Check $ArchName 'Step Into Target: 選んだ呼び出しの中で止まる (前の呼び出しは越える)' ($into.Event.body.reason -eq 'step' -and $into.Frames[0].name -eq 'Program.Plus()' -and (& $value $into.Frames[0] 'a') -eq '2' -and (& $value $into.Frames[0] 'b') -eq '4') "($($into.Event.body.reason) $($into.Frames[0].name) a=$(& $value $into.Frames[0] 'a'))"
        & $go $into

        Check $ArchName '値の変化で止める: 見張る名前と書き込みの行を受け取る' ($init.body.netdbgSupportsValueWatches -eq $true -and $watched.success -and $watched.body.watches[0].name -eq 'counter' -and $watched.body.watches[0].sites -eq 1) "($($watched | ConvertTo-Json -Compress -Depth 5))"
        $data = & $stop
        Check $ArchName '値の変化で止める: 値が変わった書き込みだけで止まり、前後の値を伝える (変わらない書き込みは通り過ぎる)' ($data.Event.body.reason -eq 'data breakpoint' -and $data.Event.body.text -eq 'counter: 0 → 3' -and $data.Frames[0].name -eq 'Program.Bump()' -and (& $value $data.Frames[0] 'by') -eq '3') "($($data.Event.body | ConvertTo-Json -Compress) $($data.Frames[0].name))"
        $leaked = @($s.Events | Where-Object { $_.event -eq 'breakpoint' -and $_.body.breakpoint.line -eq (& $at 'write') })
        Check $ArchName '値の変化で止める: 書き込みの行のブレークポイントは、利用者に見せない' ($leaked.Count -eq 0) "($($leaked.Count))"
        & $go $data

        $last = & $stop
        $timer = @($s.Events | Where-Object { $_.event -eq 'netdbg.trackedValues' -and $_.body.values[0].expression -eq '⏱ block (ms)' })
        $outputs = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'ブレークポイントの判定: 区間の計測は、かかった時間を渡して出す' ($last.Frames[0].line -eq $done -and $timer.Count -eq 1 -and [int]$timer[0].body.values[0].value -ge 40 -and $outputs -match '⏱ block: ≤ \d+ ms') "($($timer | ConvertTo-Json -Compress -Depth 4))"
        & $go $last
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $program = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' } | ForEach-Object { $_.body.output }) -join '')
        Check $ArchName 'ブレークポイントの判定: 止まるべき所のあと、最後まで走る' ($program -match 'done') "($($program -replace "`r?`n", ' | '))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'ブレークポイントの判定: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}