# 埋め込み PDB (DebugType embedded) の対象。隣に .pdb が無くても、アセンブリの中の PDB で止まり、変数が読め、ステップできる。
# netcoredbg も止まる (D-041)。
function Test-EmbeddedPdb([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\ConsoleEmbedded\SmokeConsoleEmbedded.exe"
    if (-not (Test-Path $app)) {
        Check $ArchName '埋め込み PDB: 対象アプリがある' $false '(tests\apps\build.ps1 を先に実行してください)'
        return
    }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-embedded-$ArchName.log"
    Check $ArchName '埋め込み PDB: 隣に .pdb が無い' (-not (Test-Path ([IO.Path]::ChangeExtension($app, '.pdb'))))
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $app $source 'work-first'
        Check $ArchName '埋め込み PDB: ブレークポイントで止まる' ($null -ne $stop -and $stop.body.reason -eq 'breakpoint')
        $module = Wait-DapEvent $s 'module' -Where { param($m) $m.body.module.name -eq 'SmokeConsoleEmbedded.dll' }
        Check $ArchName '埋め込み PDB: symbolStatus はシンボルあり' ($module.body.module.symbolStatus -eq 'Symbols loaded.')
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '埋め込み PDB: フレームのファイルと行' ($frame.source.path -eq $source -and $frame.line -eq (Get-MarkerLine $source 'work-first')) "($($frame.source.path):$($frame.line))"

        [void](Send-DapRequest $s 'next' @{ threadId = $stop.body.threadId })
        $stepped = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stepped.body.threadId; levels = 1 }).body.stackFrames)[0]
        Check $ArchName '埋め込み PDB: ステップで次の行へ' ($stepped.body.reason -eq 'step' -and $frame.line -eq (Get-MarkerLine $source 'work')) "($($stepped.body.reason) line $($frame.line))"
        $scope = @((Send-DapRequest $s 'scopes' @{ frameId = $frame.id }).body.scopes)[0]
        $locals = @((Send-DapRequest $s 'variables' @{ variablesReference = $scope.variablesReference }).body.variables)
        $doubled = $locals | Where-Object { $_.name -eq 'doubled' } | Select-Object -First 1
        Check $ArchName '埋め込み PDB: ローカル変数の名前と値' ($doubled.value -eq '42') "($(($locals | ForEach-Object { "$($_.name)=$($_.value)" }) -join ', '))"

        [void](Send-DapRequest $s 'continue' @{ threadId = $stepped.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '埋め込み PDB: 最後まで走る' ($exited.body.exitCode -eq 7) "($($exited.body.exitCode))"
        $unreadable = @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'シンボルを読み込めません' })
        Check $ArchName '埋め込み PDB: 読めないという知らせが無い' ($unreadable.Count -eq 0) "($(($unreadable | ForEach-Object { $_.body.output.Trim() }) -join ' / '))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName '埋め込み PDB: エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '埋め込み PDB: 例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}