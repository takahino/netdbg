# .NET 11 の runtime-async（状態機械を作らない async）。await をまたぐステップと、元の名前のローカル。
# net48 にはこの形が無い。SDK でビルドしてあるときだけ試す。
function Test-RuntimeAsync([string]$ArchName, [string]$Exe) {
    # 種類ごとの繰り返しでは、同じ対象を何度も試さない。
    if ($script:flavor.Label) { return }
    $app = Join-Path $root "target\apps\$ArchName\RuntimeAsync\SmokeRuntimeAsync.exe"
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\RuntimeAsync\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-runtime-async-$ArchName.log"
    $awaitLine = Get-MarkerLine $source 'ra-await'
    $afterLine = Get-MarkerLine $source 'ra-after'
    $innerLine = Get-MarkerLine $source 'ra-inner'
    $innerAfter = Get-MarkerLine $source 'ra-inner-after'

    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'ra-await'
        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 8 }).body.stackFrames)[0]
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $top.id }).body.scopes)[0].variablesReference }).body.variables)
        $value = ($vars | Where-Object name -eq 'value').value
        Check $ArchName 'runtime-async: next は await の次の行で、フレームは MoveNext ではない' ($stop.body.reason -eq 'step' -and $top.name -eq 'Program.Outer()' -and $top.line -eq $afterLine -and $value -eq '21') "($($stop.body.reason) $($top.name):$($top.line) value=$value)"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'runtime-async: next が例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'ra-await'
        [void](Send-DapRequest $s 'stepIn' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 4 }).body.stackFrames)[0]
        Check $ArchName 'runtime-async: stepIn は呼び出し先のメソッドに入り、MoveNext ではない' ($stop.body.reason -eq 'step' -and $top.name -eq 'Program.Inner()' -and $top.line -le $innerLine -and $top.line -ge ($innerLine - 2)) "($($stop.body.reason) $($top.name):$($top.line))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'runtime-async: stepIn が例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'ra-inner-after'
        $stack = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 12 }).body.stackFrames)
        $caller = @($stack | Where-Object { $_.name -eq 'Program.Outer()' -and $_.line -eq $awaitLine })
        $top = $stack[0]
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $top.id }).body.scopes)[0].variablesReference }).body.variables)
        $seen = ($vars | Where-Object name -eq 'seen').value
        Check $ArchName 'runtime-async: await の次の行で、元の名前のローカルが読める' ($top.name -eq 'Program.Inner()' -and $top.line -eq $innerAfter -and $seen -eq '21' -and $top.name -notmatch 'MoveNext') "($($top.name):$($top.line) seen=$seen)"
        Check $ArchName 'runtime-async: 論理スタックに、呼び出し元の await がある' ($caller.Count -ge 1) "($(($stack | ForEach-Object { "$($_.name):$($_.line)" }) -join ' | '))"
        [void](Send-DapRequest $s 'stepOut' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 4 }).body.stackFrames)[0]
        Check $ArchName 'runtime-async: stepOut は呼び出し元の await で止まり、MoveNext ではない' ($stop.body.reason -eq 'step' -and $top.name -eq 'Program.Outer()' -and $top.line -eq $awaitLine) "($($stop.body.reason) $($top.name):$($top.line))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'runtime-async: stepOut が例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
