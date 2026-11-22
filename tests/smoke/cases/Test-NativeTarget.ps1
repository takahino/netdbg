# 段階 1: ランタイムを読み込まない対象 (ネイティブの exe)。起動したら、待ち続けずに理由と終了コードを知らせて終える。
# アタッチは、ランタイムが読み込まれていなければ、待ち続けずに断る。
function Test-NativeTarget([string]$ArchName, [string]$Exe) {
    # エンジンと同じビット数の cmd.exe (x86 は SysWOW64)。
    $cmd = if ($ArchName -eq 'x86') { "$env:SystemRoot\SysWOW64\cmd.exe" } else { "$env:SystemRoot\System32\cmd.exe" }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $cmd; args = @('/c', 'ping -n 3 127.0.0.1 > nul & exit 3') })
        [void](Send-DapRequest $s 'configurationDone')
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 20000
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 5000)
        $told = @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'ランタイムを読み込まないまま' })
        Check $ArchName 'ネイティブの exe: 待ち続けずに、理由と終了コードを知らせて終える' ($exited.body.exitCode -eq 3 -and $told.Count -eq 1) "($($exited.body.exitCode) / $($told.Count))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'ネイティブの exe: エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName 'ネイティブの exe: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
    $native = Start-Process $cmd -ArgumentList '/c', 'ping -n 20 127.0.0.1 > nul' -WindowStyle Hidden -PassThru
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'attach' @{ processId = $native.Id }
        Check $ArchName 'ネイティブのプロセスへのアタッチは、理由つきで断る' (-not $r.success -and $r.message -match 'ランタイム') "($($r.message))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'ネイティブのプロセスへのアタッチ: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        $native | Stop-Process -Force -ErrorAction SilentlyContinue
    }
}