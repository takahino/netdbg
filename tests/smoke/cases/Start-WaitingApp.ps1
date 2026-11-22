# 待ち続ける対象を、デバッガー無しで起動する。ランタイムが動き出す (waiting と出力する) まで待つ。
function Start-WaitingApp([string]$App) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $App
    $info.Arguments = 'wait'
    $info.WorkingDirectory = Split-Path -Parent $App
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::Start($info)
    $line = $process.StandardOutput.ReadLineAsync()
    if (-not $line.Wait(30000) -or $line.Result -ne 'waiting') { $process.Kill(); throw '対象が待機に入りませんでした。' }
    return $process
}
