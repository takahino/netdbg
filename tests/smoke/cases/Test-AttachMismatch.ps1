# 段階 1: ビット数の合わないプロセスへのアタッチは、つなぐ前にエラーにする。
function Test-AttachMismatch([string]$ArchName, [string]$Exe) {
    $other = if ($ArchName -eq 'x86') { 'x64' } else { 'x86' }
    $app = Get-AppPath $other
    if (-not (Test-Path $app)) { return }
    $target = Start-WaitingApp $app
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'attach' @{ processId = $target.Id }
        Check $ArchName "$other のプロセスへのアタッチを断り、使うエンジンを伝える" ($r.success -eq $false -and $r.message -match "$other の netdbg.exe") "($($r.message))"
        $r = Send-DapRequest $s 'attach' @{ }
        Check $ArchName 'processId の無い attach は理由つきで失敗する' ($r.success -eq $false -and $r.message -match 'processId')
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
        Check $ArchName '断ったあとも対象は生きている' (-not $target.HasExited)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        if (-not $target.HasExited) { $target.Kill() }
    }
}
