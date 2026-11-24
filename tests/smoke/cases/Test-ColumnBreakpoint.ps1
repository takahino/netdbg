# 列のブレークポイント。同じ行に文が 2 つあるとき、指定した列の文に結び、応答に結んだ列を返す。
function Test-ColumnBreakpoint([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $lineNo = Get-MarkerLine $source 'two-statements'
    $text = (Get-Content $source -Encoding UTF8)[$lineNo - 1]
    $firstCol = $text.IndexOf('int first') + 1
    $secondCol = $text.IndexOf('int second') + 1
    if ($firstCol -lt 1 -or $secondCol -le $firstCol) {
        Check $ArchName '列のブレークポイント: 行に 2 つの文がある' $false $text
        return
    }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-column-$($script:flavor.Dir)-$ArchName.log"
    $stopAt = {
        param([int]$Column)
        $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; columnsStartAt1 = $true })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('columns') })
        $set = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $lineNo; column = $Column }) }
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        # モジュールが読み込まれる前の応答は未解決。結んだ列は、解決したあとの breakpoint イベントにある。
        $resolved = Wait-DapEvent $s 'breakpoint' -Where { param($m) $m.body.breakpoint.verified -eq $true } -TimeoutMs 10000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 8 }).body.stackFrames) | Where-Object { $_.name -eq 'Program.TwoStatements()' } | Select-Object -First 1
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        return @{ Session = $s; Set = $set; Resolved = $resolved; Stop = $stop; Frame = $frame; Vars = $vars }
    }

    $later = & $stopAt $secondCol
    try {
        $bound = $later.Resolved.body.breakpoint
        $first = ($later.Vars | Where-Object name -eq 'first').value
        Check $ArchName '列のブレークポイント: 後ろの文に置くと、そこで止まり、前の代入は済んでいる' ($bound.verified -eq $true -and $later.Frame.line -eq $lineNo -and $bound.column -ge $secondCol -and $bound.endColumn -gt $bound.column -and $later.Frame.column -eq $bound.column -and $first -eq '1') "(bound $($bound.column)-$($bound.endColumn) frame $($later.Frame.name):$($later.Frame.line):$($later.Frame.column) first=$first $($later.Stop.body.reason) $($later.Set.message))"
    } finally {
        if ($later.Session) {
            try { [void](Send-DapRequest $later.Session 'disconnect') } catch {}
            try { [void](Stop-DapSession $later.Session) } catch {}
        }
    }

    $earlier = & $stopAt $firstCol
    try {
        $bound = $earlier.Resolved.body.breakpoint
        $first = ($earlier.Vars | Where-Object name -eq 'first').value
        Check $ArchName '列のブレークポイント: 前の文に置くと、後ろの代入の前で止まる' ($earlier.Frame.line -eq $lineNo -and $bound.column -le $firstCol -and $earlier.Frame.column -lt $secondCol -and $first -ne '1') "(bound $($bound.column)-$($bound.endColumn) frame $($earlier.Frame.column) first=$first)"
    } finally {
        if ($earlier.Session) {
            try { [void](Send-DapRequest $earlier.Session 'disconnect') } catch {}
            try { [void](Stop-DapSession $earlier.Session) } catch {}
        }
    }
}
