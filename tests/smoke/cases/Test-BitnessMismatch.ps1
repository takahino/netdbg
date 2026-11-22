# 段階 1: ビット数の合わない対象は、走らせる前にエラーにする。
function Test-BitnessMismatch([string]$ArchName, [string]$Exe) {
    $other = if ($ArchName -eq 'x86') { 'x64' } else { 'x86' }
    $app = Get-AppPath $other
    if (-not (Test-Path $app)) { return }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = $app }
        Check $ArchName "$other の対象を断り、使うエンジンを伝える" ($r.success -eq $false -and $r.message -match "$other の netdbg.exe") "($($r.message))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
