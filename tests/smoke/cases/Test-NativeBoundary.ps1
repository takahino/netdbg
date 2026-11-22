# ネイティブ境界の表示 (D-090): マネージド → user32!EnumWindows → マネージドのコールバックで止めると、コールバックと
# 呼んだ側のあいだに、渡った先のネイティブのモジュール (user32.dll) が 1 行出る。ランタイムの橋渡しは出さない。
function Test-NativeBoundary([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('native-callback') })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'native-callback') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        $names = @($frames | ForEach-Object name)
        $native = [array]::FindIndex([string[]]$names, [Predicate[string]]{ param($n) $n -match '^\[ネイティブ\] user32\.dll\+0x[0-9A-F]+$' })
        $run = [array]::FindIndex([string[]]$names, [Predicate[string]]{ param($n) $n -match 'NativeCallback\.Run' })
        $label = if ($native -ge 0) { $frames[$native] } else { $null }
        Check $ArchName 'ネイティブ境界: 呼び返しのコールバックと呼んだ側のあいだに user32.dll が出る' ($native -gt 0 -and $run -gt $native -and $label.presentationHint -eq 'label') "($($names -join ' <- '))"
        Check $ArchName 'ネイティブ境界: ランタイムの橋渡し (coreclr / clr) は出さない' (-not ($names | Where-Object { $_ -match '^\[ネイティブ\] (coreclr|clr)\.dll' })) "($($names -join ' <- '))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'ネイティブ境界: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}