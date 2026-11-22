# 段階 2 / B7: Windows PDB (古い形式のプロジェクトの既定の DebugType=full) の対象。PDB を読み、行で止まり、ローカル変数が見える。
function Test-WindowsPdb([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\Console48Full\SmokeConsole48Full.exe"
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $line = Get-MarkerLine $source 'work'
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app })
        $r = Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $line }) }
        [void](Send-DapRequest $s 'configurationDone')
        $stopped = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $module = @($s.Events | Where-Object { $_.event -eq 'module' -and $_.body.module.name -eq 'SmokeConsole48Full.exe' })
        Check $ArchName 'Windows PDB: PDB を読み、シンボルのあるモジュールになる' ($module.Count -ge 1 -and $module[-1].body.module.symbolStatus -eq 'Symbols loaded.') "($($module | ForEach-Object { $_.body.module.symbolStatus }))"
        $verified = @($s.Events | Where-Object { $_.event -eq 'breakpoint' -and $_.body.breakpoint.verified }).Count -gt 0 -or $r.body.breakpoints[0].verified
        Check $ArchName 'Windows PDB: 行のブレークポイントで止まる' ($verified -and $stopped.body.reason -eq 'breakpoint') "($($stopped.body | ConvertTo-Json -Compress))"
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stopped.body.threadId }).body.stackFrames)
        Check $ArchName 'Windows PDB: いちばん上のフレームが、ソースのその行' ($frames[0].name -like '*Work*' -and $frames[0].line -eq $line -and $frames[0].source.path -eq $source) "($($frames[0] | ConvertTo-Json -Compress))"
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)[0]
        $variables = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        $doubled = @($variables | Where-Object { $_.name -eq 'doubled' })
        $n = @($variables | Where-Object { $_.name -eq 'n' })
        Check $ArchName 'Windows PDB: ローカル変数と引数が名前で見える' ($doubled.Count -eq 1 -and $n.Count -eq 1 -and [int]$doubled[0].value -eq 2 * [int]$n[0].value) "($($variables | ConvertTo-Json -Compress))"
        $evaluated = Send-DapRequest $s 'evaluate' @{ expression = 'doubled + 1'; frameId = $frames[0].id }
        Check $ArchName 'Windows PDB: 式でローカル変数を読める' ($evaluated.success -and [int]$evaluated.body.result -eq [int]$doubled[0].value + 1) "($($evaluated.message))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stopped.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName 'Windows PDB: 続行で最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}