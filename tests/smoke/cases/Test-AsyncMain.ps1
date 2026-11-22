# 段階 1 / 3: async Main の stopAtEntry。コンパイラが作った入口 (<Main>) ではなく、利用者の Main の最初の行 ({) で止まる
# (netcoredbg、Visual Studio と同じ。D-047)。
function Test-AsyncMain([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\AsyncMain\SmokeAsyncMain.exe"
    if (-not (Test-Path $app)) {
        Check $ArchName 'async Main: 対象アプリがある' $false '(tests\apps\build.ps1 を先に実行してください)'
        return
    }
    $source = Join-Path $root 'tests\apps\AsyncMain\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-asyncmain-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; stopAtEntry = $true })
        [void](Send-DapRequest $s 'configurationDone')
        $process = Wait-DapEvent $s 'process'
        Check $ArchName 'launch: process の通知に、プロセス番号と program' ($process.body.systemProcessId -gt 0 -and $process.body.startMethod -eq 'launch' -and $process.body.name -eq $app -and $process.body.isLocalProcess -eq $true) "($($process.body | ConvertTo-Json -Compress))"
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'async Main: stopAtEntry は利用者の Main の { で止まる' ($stop.body.reason -eq 'entry' -and $top.name -eq 'Program.<Main>d__0.MoveNext()' -and $top.line -eq (Get-MarkerLine $source 'main-open')) "($($stop.body.reason) $($top.name):$($top.line))"
        # フレームの名前の型引数は、実際の型で書く (netcoredbg と同じ)。
        $names = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 3 }).body.stackFrames) | ForEach-Object name
        Check $ArchName 'フレームの名前: ジェネリックの型引数を実際の型で書く' ($names[2] -eq 'System.Runtime.CompilerServices.AsyncTaskMethodBuilder<int>.Start<Program.<Main>d__0>()') "($($names -join ' <- '))"
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'async Main: そのあとの next は最初の文' ($top.line -eq (Get-MarkerLine $source 'main-first')) "($($top.name):$($top.line))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'async Main: 最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'async Main: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}