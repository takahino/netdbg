# 段階 4: ポインター (D-044)。表示は netcoredbg と同じ (値の欄に指す先、型の欄にポインターの型、開くと指す先が 1 つ)。
# 式の値は適合試験 (Conformance の終わりの unsafe ブロック) で確かめるので、ここでは表示と書き込みと断る場面を見る。
function Test-Pointers([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-pointers-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'conformance' -ProgramArgs @('conformance')
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $byName = @{}; foreach ($v in @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)) { $byName[$v.name] = $v }
        $shown = { param($name) $v = $byName[$name]; "$($v.value):$($v.type)" }
        $inside = @((Send-DapRequest $s 'variables' @{ variablesReference = $byName['pa'].variablesReference }).body.variables)
        Check $ArchName 'ポインター: 値の欄に指す先、型の欄にポインターの型、開くと指す先が 1 つ' ((& $shown 'pa') -eq '7:int*' -and $inside.Count -eq 1 -and $inside[0].name -eq '' -and $inside[0].value -eq '7') "($(& $shown 'pa') / $(($inside | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"
        Check $ArchName 'ポインター: 構造体を指すポインターと null の void*' ((& $shown 'pp') -eq '{Point}:Point*' -and (& $shown 'pv') -eq 'null:void*') "($(& $shown 'pp'), $(& $shown 'pv'))"
        $evaluate = { param([string]$Expression) Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $frame.id } -TimeoutMs 30000 }
        $r = & $evaluate 'stack + 1'
        Check $ArchName 'ポインター: 計算したポインターも同じ見せ方' ($r.success -and "$($r.body.result):$($r.body.type)" -eq '20:int*' -and $r.body.variablesReference -gt 0) "($($r.body.result):$($r.body.type) $($r.message))"
        # 書き込み: 指す先は対象のメモリに書かれ、もとの変数から見える。
        foreach ($w in @(@('*pa = 9', 'target', '9'), @('stack[1] = 21', 'stack[1]', '21'), @('(stack + 2)[0] = 31', '*(stack + 2)', '31'), @('pp->X = 5', 'spot.X', '5'), @('*pa += 1', 'target', '10'))) {
            $written = & $evaluate $w[0]
            $read = & $evaluate $w[1]
            Check $ArchName "ポインター: $($w[0]) のあと $($w[1]) は $($w[2])" ($written.success -and $read.body.result -eq $w[2]) "($($written.message) $($read.body.result))"
        }
        $r = & $evaluate '*&spot.Y'
        Check $ArchName 'ポインター: 構造体のフィールドの番地' ($r.success -and $r.body.result -eq '2') "($($r.body.result) $($r.message))"
        $r = & $evaluate '*pv'
        Check $ArchName 'ポインター: void* の指す先は、理由つきで断る' (-not $r.success -and $r.message -match 'void\*') "($($r.message))"
        $r = & $evaluate 'stackalloc int[2]'
        Check $ArchName 'ポインター: stackalloc は、理由つきで断る' (-not $r.success -and $r.message -match 'stackalloc') "($($r.message))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        Check $ArchName 'ポインター: エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName 'ポインター: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}