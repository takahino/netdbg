# COM オブジェクトの漏れ (D-092): ヒープの統計に COM オブジェクト (RCW) の数と、前の統計からの増分が出る。
function Test-ComLeak([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('com-leak') })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'com-before') }, @{ line = (Get-MarkerLine $source 'com-after') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $first = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $before = (Send-DapRequest $s 'netdbg.heapStats' @{}).body.comObjects
        $s.Events.Clear()
        [void](Send-DapRequest $s 'continue' @{ threadId = $first.body.threadId })
        [void](Wait-DapEvent $s 'stopped' -TimeoutMs 30000)
        $after = (Send-DapRequest $s 'netdbg.heapStats' @{}).body.comObjects
        Check $ArchName 'COM の漏れ: ヒープの統計に COM オブジェクトの数と、前の統計からの増分が出る' ($null -ne $before -and $null -eq $before.delta -and $after.delta -ge 10 -and $after.count -ge 11) "(before $($before | ConvertTo-Json -Compress) / after $($after | ConvertTo-Json -Compress))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'COM の漏れ: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}