# 段階 4: 副作用のある getter への備え (D-043)。launch.json の expressionEvaluationOptions.allowImplicitFuncEval = false では、
# 変数の欄を開いてもプロパティの getter を実行せず、クリックで評価する形 (DAP の lazy) で見せる。DebuggerDisplay と
# 代わりの型 (DebuggerTypeProxy) も使わない。式 (evaluate) は、これまでどおり評価する。
function Test-NoImplicitEval([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-noimplicit-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; expressionEvaluationOptions = @{ allowImplicitFuncEval = $false } })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'values') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $locals = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        $byName = @{}; foreach ($v in $locals) { $byName[$v.name] = $v }
        $open = { param($v) @((Send-DapRequest $s 'variables' @{ variablesReference = $v.variablesReference } -TimeoutMs 30000).body.variables) }

        $area = (& $open $byName['point']) | Where-Object { $_.name -eq 'Area' }
        Check $ArchName '暗黙の評価なし: プロパティは実行せず、クリックで評価する形 (lazy)' ($area.presentationHint.lazy -eq $true -and $area.variablesReference -gt 0 -and $area.value -notmatch '^2$') "($($area.value))"
        $clicked = @(& $open $area)
        Check $ArchName '暗黙の評価なし: クリックすると getter を実行して値を 1 つ返す' ($clicked.Count -eq 1 -and $clicked[0].name -eq 'Area' -and $clicked[0].value -eq '2') "($(($clicked | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"
        $statics = (& $open $byName['this']) | Where-Object { $_.name -eq 'Static members' }
        $twice = (& $open $statics) | Where-Object { $_.name -eq 'Twice' }
        $clicked = @(& $open $twice)
        Check $ArchName '暗黙の評価なし: 静的プロパティも同じ' ($twice.presentationHint.lazy -eq $true -and $clicked[0].value -eq '2') "($($twice.value) → $($clicked[0].value))"
        $names = @(& $open $byName['names'])
        Check $ArchName '暗黙の評価なし: 代わりの型 (List<T> の [0]) を使わない' (-not ($names | Where-Object { $_.name -eq '[0]' }) -and ($names | Where-Object { $_.name -eq '_size' })) "($(($names | ForEach-Object name) -join ', '))"
        Check $ArchName '暗黙の評価なし: DebuggerDisplay を使わない' ($byName['displayed'].value -eq '{Displayed}') "($($byName['displayed'].value))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'names.Count + point.Area'; frameId = $frame.id } -TimeoutMs 30000
        Check $ArchName '暗黙の評価なし: 式の評価は、これまでどおり getter を実行する' ($r.success -and $r.body.result -eq '3') "($($r.body.result)$($r.message))"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        Check $ArchName '暗黙の評価なし: エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '暗黙の評価なし: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}