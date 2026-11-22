# 段階 0: initialize に答え、ランタイムを読み込まず、disconnect で終わる。
function Test-Skeleton([string]$ArchName, [string]$Exe) {
    $bits = if ($ArchName -eq 'x86') { 32 } else { 64 }
    $version = & $Exe --version
    Check $ArchName "--version が $bits bit と答える" ($version -match "\($bits bit\)") "($version)"

    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $r = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; linesStartAt1 = $true; columnsStartAt1 = $true; pathFormat = 'path' }
        Check $ArchName 'initialize が成功する' ($r.success -eq $true)
        $e = Wait-DapEvent $s 'initialized'
        Check $ArchName 'initialized イベントが届く' ($null -ne $e)

        $modules = Get-DapModuleNames $s $ArchName
        Check $ArchName 'モジュール一覧が取れる' ($modules -contains 'netdbg.exe') "($($modules.Count) 個)"
        foreach ($runtime in 'coreclr.dll', 'clr.dll', 'hostfxr.dll') {
            Check $ArchName "$runtime を読み込んでいない" ($modules -notcontains $runtime)
        }

        $r = Send-DapRequest $s 'noSuchRequest'
        Check $ArchName '知らない要求は失敗の応答になる' ($r.success -eq $false -and $r.message)

        $r = Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true }
        Check $ArchName 'disconnect が成功する' ($r.success -eq $true)
        $code = Stop-DapSession $s
        Check $ArchName '終了コードが 0' ($code -eq 0) "($code)"
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
    $logged = (Test-Path $log) -and ((Get-Content $log -Raw) -match '"command":"initialize"')
    Check $ArchName '--log にやり取りが残る' $logged
}
