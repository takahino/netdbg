# 段階 3: Debugger.Break()、内側の例外 (innerException)、フィルター user-unhandled。表記は netcoredbg と同じ
# (tests/fixtures/record-exceptions.ps1 -Mode break / inner で見比べた)。
function Test-ExceptionDetails([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-exdetails-$($script:flavor.Dir)-$ArchName.log"
    $launch = {
        param([string]$Mode, [string[]]$Filters)
        $session = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
        [void](Send-DapRequest $session 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $session 'launch' @{ program = $app; args = @($Mode) })
        [void](Send-DapRequest $session 'setExceptionBreakpoints' @{ filters = @($Filters) })
        [void](Send-DapRequest $session 'configurationDone')
        return $session
    }
    $top = { param($session, $stop) @((Send-DapRequest $session 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0] }

    # Debugger.Break(): netcoredbg と同じく reason は pause。止まった行は Break の行。
    $s = & $launch 'break' @()
    try {
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = & $top $s $stop
        Check $ArchName 'Debugger.Break(): pause で止まり、その行を指す' ($stop.body.reason -eq 'pause' -and $frame.line -eq (Get-MarkerLine $source 'break')) "($($stop.body.reason) line $($frame.line))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'Debugger.Break(): 続行すると最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'Debugger.Break(): 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 内側の例外: exceptionInfo の details.innerException と、description の末尾の「Innermost exception」。
    $s = & $launch 'inner' @('all')
    try {
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $info = (Send-DapRequest $s 'exceptionInfo' @{ threadId = $stop.body.threadId }).body
        $inner = @($info.details.innerException)[0]
        Check $ArchName '内側の例外: innerException に型と文言' ($info.details.fullTypeName -eq 'System.InvalidOperationException' -and $inner.fullTypeName -eq 'System.FormatException' -and $inner.message -eq 'inner') "($($info.details.fullTypeName) / $($inner.fullTypeName) $($inner.message))"
        Check $ArchName '内側の例外: description に、いちばん内側の型' ($info.description -match "Innermost exception: System\.FormatException$") "($($info.description))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '内側の例外: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # user-unhandled: 利用者のコード (タスクの中) で投げ、タスクの仕組みが捕まえる例外でだけ止まる。利用者のコードが
    # 捕まえる例外 (outer、AggregateException) では止まらない。
    $s = & $launch 'inner' @('user-unhandled')
    try {
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $info = (Send-DapRequest $s 'exceptionInfo' @{ threadId = $stop.body.threadId }).body
        $frame = & $top $s $stop
        Check $ArchName 'user-unhandled: タスクの中の例外で、投げた行に止まる' ($stop.body.reason -eq 'exception' -and $info.breakMode -eq 'userUnhandled' -and $info.details.fullTypeName -eq 'System.ArgumentException' -and $frame.line -eq (Get-MarkerLine $source 'user-unhandled')) "($($info.breakMode) $($info.details.fullTypeName) line $($frame.line))"
        Check $ArchName 'user-unhandled: 文言は netcoredbg と同じ' ($stop.body.text -eq "An exception of type 'System.ArgumentException' occurred in $($script:flavor.Module) but was not handled in user code") "($($stop.body.text))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $next = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'user-unhandled: 利用者のコードが捕まえる例外では止まらない' ($next.body.exitCode -eq 7 -and @($s.Events | Where-Object { $_.event -eq 'stopped' }).Count -eq 0) "($($next.body.exitCode))"
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'user-unhandled: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}