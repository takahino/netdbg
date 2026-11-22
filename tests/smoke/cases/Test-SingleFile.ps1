# 段階 1: 単一ファイルで発行した自己完結のアプリ (D-050)。ランタイムもアセンブリも exe の中にあり、mscordbi.dll は同梱されない。
# 合う版の mscordbi.dll を共有ランタイムかランタイムパックから渡し、モジュールは対象のメモリから読む。netcoredbg は落ちる。
function Test-SingleFile([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\ConsoleSingle\SmokeConsole.exe"
    if (-not (Test-Path $app)) {
        Check $ArchName '単一ファイル: 対象アプリがある' $false '(tests\apps\build.ps1 を先に実行してください)'
        return
    }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-single-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'work'
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '単一ファイル: ブレークポイントで止まり、ファイルと行が分かる' ($null -ne $stop -and $frame.name -eq 'Program.Work()' -and $frame.line -eq (Get-MarkerLine $source 'work') -and $frame.source.path -eq $source) "($($frame.name):$($frame.line))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'doubled + new System.Collections.Generic.List<int> { 1 }.Count'; frameId = $frame.id } -TimeoutMs 30000
        Check $ArchName '単一ファイル: 式を評価できる (対象のコードの実行も)' ($r.success -and $r.body.result -eq '43') "($($r.body.result) $($r.message))"
        $module = @($s.Events | Where-Object { $_.event -eq 'module' -and $_.body.module.name -eq 'SmokeConsole' })
        Check $ArchName '単一ファイル: アプリのモジュールにシンボルがある' ($module.Count -eq 1 -and $module[0].body.module.symbolStatus -eq 'Symbols loaded.') "($($module.Count))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '単一ファイル: 最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '単一ファイル: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}