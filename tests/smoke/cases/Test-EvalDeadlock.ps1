# 段階 4: 評価の行き詰まり。評価したコードが、ほかのスレッドの持っているロックを待つと、ほかのスレッドを止めたままでは
# 終わらない。時間切れのあと、ほかのスレッドも動かして評価を終えること。そのあとも、止まったまま式を評価できること。
function Test-EvalDeadlock([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-deadlock-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'gate' -ProgramArgs @('gate')
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        # DAP の cancel: 終わらない評価を、利用者が取り消す。時間切れ (5 秒) を待たずに中断し、そのあとも評価できること。
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $evaluation = Send-DapRequestOnly $s 'evaluate' @{ expression = 'TakeGate()'; frameId = $frame.id }
        Start-Sleep -Milliseconds 500
        $cancel = Send-DapRequestOnly $s 'cancel' @{ requestId = $evaluation }
        $r = Receive-DapResponse $s $evaluation 30000
        $elapsed = $clock.ElapsedMilliseconds
        $c = Receive-DapResponse $s $cancel 30000
        Check $ArchName 'cancel: 評価を、時間切れを待たずに取り消す' ($r.success -eq $false -and $r.message -match 'cancelled' -and $elapsed -lt 4000 -and $c.success) "($($r.message), $elapsed ms)"
        $r = Send-DapRequest $s 'evaluate' @{ expression = '1 + 1'; frameId = $frame.id } -TimeoutMs 30000
        Check $ArchName 'cancel: 取り消したあとも、止まったまま式を評価できる' ($r.success -and $r.body.result -eq '2') "($($r.body.result)$($r.message))"
        $clock.Restart()
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'TakeGate()'; frameId = $frame.id } -TimeoutMs 30000
        $waited = $clock.ElapsedMilliseconds
        Check $ArchName 'ほかのスレッドの持つロックを待つ評価も、ほかのスレッドを動かして終える' ($r.success -and $r.body.result -eq '42') "($($r.body.result)$($r.message))"
        # ほかのスレッドを動かして終えたことは、利用者に知らせる (DAP の output、category important)。
        # ロックを持つスレッドは、回るたびに一瞬だけロックを放す。そこで止まっていれば、評価はすぐ終わり (時間切れ
        # にならず)、知らせも無い。時間切れ (5 秒) まで待ったときだけ確かめる。
        if ($waited -ge 4500) {
            $notice = try { Wait-DapEvent $s 'output' -Where { $args[0].body.category -eq 'important' } -TimeoutMs 3000 } catch { $null }
            Check $ArchName 'ほかのスレッドを動かして終えたことを、利用者に知らせる' ($notice -and $notice.body.output -match 'ほかのスレッドも動かして') "($($notice.body.output))"
        } else {
            Write-Host "  (ロックが空いていて $waited ms で終わったので、知らせの確認は飛ばします)" -ForegroundColor Yellow
        }
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'TakeGate() + 1'; frameId = $frame.id } -TimeoutMs 30000
        Check $ArchName 'そのあとも、止まったまま式を評価できる' ($r.success -and $r.body.result -eq '43') "($($r.body.result)$($r.message))"
        # 終わらない getter: 評価は中断して失敗を返し、そのあとも評価できる。
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'spinner.Spin'; frameId = $frame.id } -TimeoutMs 60000
        Check $ArchName '終わらない getter の評価は、中断して失敗を返す' ($r.success -eq $false -and $r.message -match '中断') "($($r.message))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = 'spinner.After + 1'; frameId = $frame.id } -TimeoutMs 30000
        Check $ArchName '中断したあとも、評価できる' ($r.success -and $r.body.result -eq '3') "($($r.body.result)$($r.message))"
        # 変数の欄で開くと、時間切れのプロパティには理由を書き、続行するまで残りのプロパティは読まない (待たされ続けないように)。
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $spinner = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables) | Where-Object name -eq 'spinner'
        $members = @((Send-DapRequest $s 'variables' @{ variablesReference = $spinner.variablesReference } -TimeoutMs 60000).body.variables)
        $listed = ($members | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' | '
        Check $ArchName '時間切れのプロパティには理由を書き、残りのプロパティは読まない' ($listed -match '^Ready=1 \| Spin=<評価が .* \| After=<前の評価が時間切れ') "($listed)"
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $again = @((Send-DapRequest $s 'variables' @{ variablesReference = $spinner.variablesReference } -TimeoutMs 30000).body.variables)
        Check $ArchName 'もう一度開いても、待たされない' ($clock.ElapsedMilliseconds -lt 2000 -and ($again | Where-Object name -eq 'Spin').value -match '前の評価が時間切れ') "($($clock.ElapsedMilliseconds) ms)"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        Check $ArchName '続行すると、最後まで走る' ([bool](Wait-DapEvent $s 'terminated' -TimeoutMs 30000))
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}