# 段階 3: ステップインは、既定ではプロパティの中に入らない (enableStepFiltering)。
function Test-StepFiltering([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    foreach ($filtering in $true, $false) {
        $s = Start-DapSession -Exe $Exe
        try {
            [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
            $launch = @{ program = $app }
            if (-not $filtering) { $launch.enableStepFiltering = $false }
            [void](Send-DapRequest $s 'launch' $launch)
            [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'filter-call') }) })
            [void](Send-DapRequest $s 'configurationDone')
            $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            [void](Send-DapRequest $s 'stepIn' @{ threadId = $stop.body.threadId })
            $stepped = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
            $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stepped.body.threadId; levels = 1 }).body.stackFrames)[0]
            if ($filtering) {
                Check $ArchName 'stepIn はプロパティを通り過ぎて、メソッドに入る' ($top.name -eq 'Program.Inner()' -and $top.line -eq (Get-MarkerLine $source 'inner-open')) "($($top.name):$($top.line))"
            } else {
                Check $ArchName 'enableStepFiltering が false なら、プロパティの中に入る' ($top.line -eq (Get-MarkerLine $source 'getter')) "($($top.name):$($top.line))"
            }
            [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
            [void](Stop-DapSession $s)
        } catch {
            Check $ArchName '例外なしで終わる' $false $_.Exception.Message
            if (-not $s.Process.HasExited) { $s.Process.Kill() }
        }
    }
}
