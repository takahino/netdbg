# 段階 2: .NET Framework の対象は、.NET の部品 (dbgshim.dll、.NET のランタイム) が無くてもデバッグできる。
# エンジンの exe だけを空のフォルダーに置いて確かめる。(.NET を入れていない機械そのものの代わり。実機での確認は別に要る。)
function Test-FrameworkWithoutDotnet([string]$ArchName, [string]$Exe) {
    $script:flavor = $flavors[1]
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $alone = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-alone-$ArchName"
    if (Test-Path $alone) { Remove-Item $alone -Recurse -Force }
    [void](New-Item -ItemType Directory $alone)
    Copy-Item $Exe $alone
    $s = Start-DapSession -Exe (Join-Path $alone 'netdbg.exe')
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = $app }
        Check $ArchName 'dbgshim.dll が無くても launch が成功する' ($r.success -eq $true) "($($r.message))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'work') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName 'dbgshim.dll が無くてもブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint' -and $frames[0].name -eq 'Program.Work()') "($($frames[0].name))"
        $modules = Get-DapModuleNames $s $ArchName
        $dotnetParts = @($modules | Where-Object { $_ -in 'dbgshim.dll', 'coreclr.dll', 'hostfxr.dll', 'hostpolicy.dll', 'clr.dll' })
        Check $ArchName 'エンジンは .NET の部品もランタイムも読み込んでいない' ($dotnetParts.Count -eq 0 -and ($modules -contains 'mscordbi.dll')) "($($dotnetParts -join ' '))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '終了コードが伝わる' ($exited.body.exitCode -eq 7)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)

        # .NET の対象には dbgshim.dll が要る。無ければ、置き場所を伝えて断る。
        $script:flavor = $flavors[0]
        $s = Start-DapSession -Exe (Join-Path $alone 'netdbg.exe')
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = (Get-AppPath $ArchName) }
        Check $ArchName 'dbgshim.dll が無いと .NET の対象は理由つきで断る' ($r.success -eq $false -and $r.message -match 'dbgshim\.dll') "($($r.message))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        $script:flavor = $flavors[0]
        Remove-Item $alone -Recurse -Force -ErrorAction SilentlyContinue
    }
}
