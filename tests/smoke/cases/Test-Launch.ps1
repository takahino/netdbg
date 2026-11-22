# 段階 1 / 2: コンソールアプリを起動し、行ブレークポイントで止め、コールスタックを読み、最後まで走らせる。
function Test-Launch([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) {
        Check $ArchName '対象アプリがある' $false '(tests\apps\build.ps1 を先に実行してください)'
        return
    }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $workLine = Get-MarkerLine $source 'work'
    $callLine = Get-MarkerLine $source 'call'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-launch-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; linesStartAt1 = $true; columnsStartAt1 = $true; pathFormat = 'path' })
        $r = Send-DapRequest $s 'launch' @{ program = $app; args = @('a b', 'c'); cwd = (Split-Path -Parent $app); env = @{ SMOKE_VAR = 'v1' } }
        Check $ArchName 'launch が成功する' ($r.success -eq $true) "($($r.message))"
        $r = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $workLine }) }
        $bp = $r.body.breakpoints[0]
        Check $ArchName '起動前のブレークポイントは未確定' ($r.success -and $bp.verified -eq $false)
        [void](Send-DapRequest $s 'setExceptionBreakpoints' @{ filters = @() })
        $r = Send-DapRequest $s 'configurationDone'
        Check $ArchName 'configurationDone が成功する' ($r.success -eq $true) "($($r.message))"

        $e = Wait-DapEvent $s 'breakpoint' -TimeoutMs 30000
        Check $ArchName 'モジュールの読み込みでブレークポイントが確定する' ($e.body.breakpoint.id -eq $bp.id -and $e.body.breakpoint.verified -eq $true -and $e.body.breakpoint.line -eq $workLine)
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName 'ブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint' -and $stop.body.hitBreakpointIds -contains $bp.id)
        $module = Wait-DapEvent $s 'module' -Where { param($m) $m.body.module.name -eq $script:flavor.Module }
        Check $ArchName 'module イベントの symbolStatus' ($module.body.module.symbolStatus -eq 'Symbols loaded.')

        $modules = Get-DapModuleNames $s $ArchName
        Check $ArchName "止めている間も $($script:flavor.Runtime) を読み込んでいない" (($modules -contains $script:flavor.Shim) -and ($modules -notcontains $script:flavor.Runtime)) "($($modules -join ' '))"

        $r = Send-DapRequest $s 'threads'
        Check $ArchName 'threads に止まったスレッドがある' (@($r.body.threads | Where-Object { $_.id -eq $stop.body.threadId }).Count -eq 1)
        $r = Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; startFrame = 0; levels = 20 }
        $frames = @($r.body.stackFrames)
        Check $ArchName 'コールスタックの先頭がブレークポイントの行' ($frames.Count -ge 2 -and $frames[0].line -eq $workLine -and $frames[0].source.path -eq $source) "(line $($frames[0].line))"
        Check $ArchName '呼び出し元のフレームの行' ($frames.Count -ge 2 -and $frames[1].line -eq $callLine) "(line $($frames[1].line))"
        Check $ArchName 'フレームの名前が 型.メソッド() の形' ($frames.Count -ge 2 -and $frames[0].name -eq 'Program.Work()' -and $frames[1].name -eq 'Program.Main()') "($($frames[0].name), $($frames[1].name))"
        $r = Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; startFrame = 1; levels = 1 }
        Check $ArchName 'startFrame と levels で切り出せる' (@($r.body.stackFrames).Count -eq 1 -and $r.body.stackFrames[0].line -eq $callLine -and $r.body.totalFrames -eq $frames.Count)

        $r = Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId }
        Check $ArchName 'continue が成功する' ($r.success -eq $true) "($($r.message))"
        $out = Wait-DapEvent $s 'output' -Where { param($m) $m.body.category -eq 'stdout' -and $m.body.output -match 'result=42' } -TimeoutMs 30000
        Check $ArchName '対象の標準出力、引数、環境変数' ($out.body.output -match 'result=42 args=a b,c env=v1') "($($out.body.output.Trim()))"
        $err = Wait-DapEvent $s 'output' -Where { param($m) $m.body.category -eq 'stderr' }
        Check $ArchName '対象の標準エラー' ($err.body.output -match 'to stderr')
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '終了コードが伝わる' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Wait-DapEvent $s 'terminated')
        $r = Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true }
        Check $ArchName '終了後の disconnect' ($r.success -eq $true)
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
