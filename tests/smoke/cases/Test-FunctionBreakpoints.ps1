# 段階 3 / 5: 関数ブレークポイント。名前に合うメソッドの先頭で止まる。anyCode を付けると、シンボルの無いコードにも置く。
function Test-FunctionBreakpoints([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-function-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $r = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName '関数ブレークポイントに対応していると答える' ($r.body.supportsFunctionBreakpoints -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        $r = Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @(@{ name = 'Program.Work' }, @{ name = 'Total(int[], double)' }, @{ name = 'System.Console.WriteLine' }, @{ name = 'Program.NoSuchMethod' }) }
        $ids = @($r.body.breakpoints | ForEach-Object id)
        Check $ArchName '起動前の関数ブレークポイントは未確定' ($r.success -and $ids.Count -eq 4 -and @($r.body.breakpoints | Where-Object verified).Count -eq 0) "($($r.message))"
        [void](Send-DapRequest $s 'configurationDone')

        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName 'メソッド名だけ (引数つき) の関数ブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint' -and $top.name -eq 'Order.Total()' -and $stop.body.hitBreakpointIds -contains $ids[1]) "($($stop.body.reason) $($top.name))"
        $changed = @($s.Events | Where-Object { $_.event -eq 'breakpoint' -and $_.body.breakpoint.verified } | ForEach-Object { $_.body.breakpoint.id })
        Check $ArchName 'シンボルのあるメソッドだけが確定する' ((($changed | Sort-Object) -join ',') -eq ((@($ids[0], $ids[1]) | Sort-Object) -join ',')) "($($changed -join ','))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '型.メソッド の関数ブレークポイントは、メソッドの先頭で止まる' ($top.name -eq 'Program.Work()' -and $top.line -eq (Get-MarkerLine $source 'work-open') -and $stop.body.hitBreakpointIds -contains $ids[0]) "($($top.name):$($top.line))"

        # 止まっている間に入れ替える。anyCode を付けると、シンボルの無いライブラリのメソッドにも置ける。
        $r = Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @(@{ name = 'System.Console.WriteLine'; anyCode = $true }) }
        Check $ArchName 'anyCode の関数ブレークポイントはすぐ確定する' ($r.body.breakpoints[0].verified -eq $true)
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName 'anyCode: ライブラリのメソッドの中で止まる' ($frames[0].name -eq 'System.Console.WriteLine()' -and -not $frames[0].source -and ($frames | ForEach-Object name) -contains 'Program.Main()') "($(($frames | ForEach-Object name) -join ' <- '))"

        $r = Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @() }
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '外したあとは止まらずに最後まで走る' ($exited.body.exitCode -eq 7 -and @($s.Events | Where-Object { $_.event -eq 'stopped' }).Count -eq 0)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
