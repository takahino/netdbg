# 段階 4: setVariable / setExpression (変数の欄での書き換え)。値は式。応答は、書き換えたあとの値と型。
function Test-SetVariable([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-setvariable-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; supportsInvalidatedEvent = $true }
        Check $ArchName 'capabilities: setVariable と setExpression' ($init.body.supportsSetVariable -eq $true -and $init.body.supportsSetExpression -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'values') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        $evaluate = { param([string]$Expression) (Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $frame.id }).body.result }

        $r = Send-DapRequest $s 'setVariable' @{ variablesReference = $scope.variablesReference; name = 'sum'; value = 'sum + 12' }
        Check $ArchName 'setVariable: ローカル変数 (値は式)' ($r.success -and $r.body.value -eq '42' -and $r.body.type -eq 'int' -and (& $evaluate 'sum') -eq '42') "($($r.body.value)$($r.message))"
        $r = Send-DapRequest $s 'setVariable' @{ variablesReference = $scope.variablesReference; name = 'sum'; value = '"text"' }
        Check $ArchName 'setVariable: 型の合わない値は、理由つきで断る' ($r.success -eq $false -and $r.message -match '変換') "($($r.message))"
        $this = $vars | Where-Object name -eq 'this'
        $r = Send-DapRequest $s 'setVariable' @{ variablesReference = $this.variablesReference; name = 'id'; value = '5' }
        Check $ArchName 'setVariable: オブジェクトのフィールド' ($r.success -and (& $evaluate 'id') -eq '5') "($($r.message))"
        $prices = $vars | Where-Object name -eq 'prices'
        $r = Send-DapRequest $s 'setVariable' @{ variablesReference = $prices.variablesReference; name = '[1]'; value = '99' }
        Check $ArchName 'setVariable: 配列の要素' ($r.success -and (& $evaluate 'prices[1]') -eq '99') "($($r.message))"
        $r = Send-DapRequest $s 'setExpression' @{ expression = 'this.Name'; value = '"coffee"'; frameId = $frame.id }
        Check $ArchName 'setExpression: プロパティ (setter を実行。応答は書き換えたあとの値)' ($r.success -and $r.body.value -ceq '"coffee"' -and (& $evaluate 'Name') -ceq '"coffee"') "($($r.body.value)$($r.message))"
        $r = Send-DapRequest $s 'setExpression' @{ expression = 'sum + 1'; value = '3'; frameId = $frame.id }
        Check $ArchName 'setExpression: 代入できない式は、理由つきで断る' ($r.success -eq $false -and $r.message -match '代入できません') "($($r.message))"
        # invalidated: 値を変え得る要求のあとだけ送る (ウォッチやホバーの評価のあとには送らない。取り直しが止まらなくなるので)。
        # イベントは応答のあとに届くので、別の要求 (threads) の応答まで読んでから数える。
        $invalidated = { [void](Send-DapRequest $s 'threads'); @($s.Events | Where-Object { $_.event -eq 'invalidated' }).Count }
        $s.Events.Clear()
        [void](Send-DapRequest $s 'setVariable' @{ variablesReference = $scope.variablesReference; name = 'sum'; value = '1' })
        [void](Send-DapRequest $s 'evaluate' @{ expression = '1 + 1'; frameId = $frame.id; context = 'repl' })
        $afterSet = & $invalidated
        [void](Send-DapRequest $s 'evaluate' @{ expression = 'sum.ToString()'; frameId = $frame.id; context = 'watch' })
        [void](Send-DapRequest $s 'evaluate' @{ expression = 'sum.ToString()'; frameId = $frame.id; context = 'hover' })
        $afterWatch = & $invalidated
        [void](Send-DapRequest $s 'evaluate' @{ expression = 'sum = 2'; frameId = $frame.id; context = 'repl' })
        $afterRepl = & $invalidated
        Check $ArchName 'invalidated: 書き換えたあとだけ送り、ウォッチやホバーのあとには送らない' ($afterSet -eq 1 -and $afterWatch -eq 1 -and $afterRepl -eq 2) "($afterSet, $afterWatch, $afterRepl)"
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'setVariable: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}