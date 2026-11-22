# 段階 3: 例外。条件に合う型が投げられたら止まる。誰も捕まえない例外では、条件に関係なく止まる。
function Test-Exceptions([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-exceptions-$($script:flavor.Dir)-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $r = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName '例外のフィルターを all と user-unhandled で知らせる' ((@($r.body.exceptionBreakpointFilters | ForEach-Object filter) -join ',') -eq 'all,user-unhandled' -and $r.body.supportsExceptionInfoRequest -eq $true)
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('throw') })
        $r = Send-DapRequest $s 'setExceptionBreakpoints' @{ filters = @(); filterOptions = @(@{ filterId = 'all'; condition = 'System.FormatException, System.IO.IOException' }) }
        Check $ArchName 'setExceptionBreakpoints が成功する' ($r.success -eq $true) "($($r.message))"
        [void](Send-DapRequest $s 'configurationDone')

        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName '条件に合う型が投げられたら止まる' ($stop.body.reason -eq 'exception' -and $stop.body.text -match "^Exception thrown: 'System\.FormatException' in ") "($($stop.body.text))"
        $info = (Send-DapRequest $s 'exceptionInfo' @{ threadId = $stop.body.threadId }).body
        Check $ArchName 'exceptionInfo: 投げられた例外' ($info.exceptionId -eq 'CLR/System.FormatException' -and $info.breakMode -eq 'always' -and $info.details.fullTypeName -eq 'System.FormatException' -and $info.details.typeName -eq 'FormatException' -and $info.details.message.Length -gt 0 -and $info.description -eq "$($stop.body.text): '$($info.details.message)'") "($($info | ConvertTo-Json -Depth 5 -Compress))"
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        $mine = $frames | Where-Object { $_.source.path } | Select-Object -First 1
        Check $ArchName '例外で止まった場所の、自分のコードのフレーム' ($mine.name -eq 'Program.Main()' -and $mine.line -eq (Get-MarkerLine $source 'caught')) "($(($frames | ForEach-Object { "$($_.name):$($_.line)" }) -join ' <- '))"
        $r = Send-DapRequest $s 'evaluate' @{ expression = '$exception'; frameId = $mine.id }
        Check $ArchName '$exception で例外のオブジェクトを読む' ($r.success -and $r.body.result -eq '{System.FormatException}' -and $r.body.variablesReference -gt 0) "($($r.message)$($r.body.result))"
        $r = Send-DapRequest $s 'exceptionInfo' @{ threadId = 1 }
        Check $ArchName '例外で止まっていないスレッドの exceptionInfo は失敗する' ($r.success -eq $false)

        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName '条件に無い型は投げられても止まらず、誰も捕まえなければ止まる' ($stop.body.reason -eq 'exception' -and $stop.body.text -eq "An unhandled exception of type 'System.InvalidOperationException' occurred in $($script:flavor.Module)") "($($stop.body.text))"
        # 先に変数の参照をもらっておき、exceptionInfo (対象のコードを実行して Message と StackTrace を読む) のあとで開く。
        $before = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $argsVariable = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $before.id }).body.scopes)[0].variablesReference }).body.variables) | Where-Object { $_.name -eq 'args' }
        $info = (Send-DapRequest $s 'exceptionInfo' @{ threadId = $stop.body.threadId }).body
        # 文字列はランタイムが作る。.NET Framework は OS の言語で返し (「場所 … 行 36」)、x64 では行番号が次の文にずれることがある。
        Check $ArchName 'exceptionInfo: スタックトレース (対象のプロパティを実行して読む)' ($info.details.stackTrace -match 'Program\.Main\(String\[\] args\)' -and $info.details.stackTrace -match 'Program\.cs') "($($info.details.stackTrace))"
        $items = @((Send-DapRequest $s 'variables' @{ variablesReference = $argsVariable.variablesReference }).body.variables)
        Check $ArchName '対象のコードを実行したあとも、前にもらった変数の参照を開ける' ($items.Count -eq 1 -and $items[0].value -ceq '"throw"') "($(($items | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '))"
        Check $ArchName 'exceptionInfo: 誰も捕まえない例外' ($info.breakMode -eq 'unhandled' -and $info.details.message -eq 'boom' -and $info.details.formattedDescription -eq "**System.InvalidOperationException** 'boom'" -and $info.details.source -eq [IO.Path]::GetFileNameWithoutExtension($script:flavor.Module)) "($($info | ConvertTo-Json -Depth 5 -Compress))"
        $top = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '誰も捕まえない例外は、投げた行で止まる' ($top.name -eq 'Program.Main()' -and $top.line -eq (Get-MarkerLine $source 'thrown')) "($($top.name):$($top.line))"

        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        # デバッガーの下では、誰も捕まえない例外のあと続行すると終了コードは 0 になる (netcoredbg でも同じ。デバッガー無しなら 0xE0434352)。
        Check $ArchName '誰も捕まえない例外のあと続行すると対象が終わる' ($exited.body.exitCode -eq 0) "($($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
