# program に dll を渡したとき、dotnet ホストで `dotnet <dll> 引数…` として起動すること (netcoredbg と同じ)。
# .NET の対象だけ (.NET Framework には dll の起動が無い。.NET 11 の対象は、機械に .NET 11 が無いので自己完結の exe で試している)。
function Test-LaunchDll([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\Console\SmokeConsole.dll"
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-launchdll-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = $app; args = @('conformance') }
        Check $ArchName 'program に dll を渡すと、dotnet ホストで起動する' $r.success "($($r.message))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'conformance') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'dll の起動でも、ブレークポイントで止まり、引数が渡っている' ($frame.name -eq 'Conformance.Run()') "($($frame.name))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'dll の起動でも、終了コードが返る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'dll の起動: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}