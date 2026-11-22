# 段階 1: 対象の出力の文字コード。対象は、コンソールのコードページ (日本語の Windows では Shift_JIS) か UTF-8 で書く。
# どちらでも文字化けしない (UTF-8 として正しくなければ、コードページで読む)。netcoredbg は UTF-8 としてだけ読み、化ける。
function Test-OutputText([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('text') })
        [void](Send-DapRequest $s 'configurationDone')
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 30000)
        $out = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stdout' }) | ForEach-Object { $_.body.output }) -join ''
        $err = (@($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'stderr' }) | ForEach-Object { $_.body.output }) -join ''
        $japanese = [Text.Encoding]::Default.CodePage -eq 932
        $ok = $out -match '^text: ' -and $out -notmatch [char]0xFFFD -and $err -notmatch [char]0xFFFD -and (-not $japanese -or ($out -match 'こんにちは' -and $err -match 'エラー'))
        Check $ArchName '対象の出力: ASCII でない文字が化けない' $ok "($($out.Trim()) / $($err.Trim()))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '対象の出力: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}