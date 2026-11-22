# 段階 1: 止まっている対象を disconnect で終わらせる。対象のプロセスが残らないこと。
function Test-DisconnectWhileStopped([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Start-AtMarker $s $app $source 'work')
        $r = Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true }
        Check $ArchName '止まっている対象を disconnect で終わらせる' ($r.success -eq $true)
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
        $left = @(Get-Process $script:flavor.Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $app })
        Check $ArchName '対象のプロセスが残っていない' ($left.Count -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
