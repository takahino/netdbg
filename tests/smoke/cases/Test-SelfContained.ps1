# 段階 1: 自己完結アプリ (ランタイムを同梱)。機械に入っている .NET ではなく、アプリのフォルダーのランタイムで動く。
function Test-SelfContained([string]$ArchName, [string]$Exe) {
    $app = Join-Path $root "target\apps\$ArchName\ConsoleSelfContained\SmokeConsole.exe"
    if (-not (Test-Path $app)) {
        Check $ArchName '自己完結の対象アプリがある' $false '(tests\apps\build.ps1 を先に実行してください)'
        return
    }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-selfcontained-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $r = Send-DapRequest $s 'launch' @{ program = $app }
        Check $ArchName '自己完結: launch が成功する' ($r.success -eq $true) "($($r.message))"
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'work') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        Check $ArchName '自己完結: ブレークポイントで止まる' ($stop.body.reason -eq 'breakpoint')
        $corelib = Wait-DapEvent $s 'module' -Where { param($m) $m.body.module.name -eq 'System.Private.CoreLib.dll' }
        Check $ArchName '自己完結: ランタイムはアプリのフォルダーのもの' ($corelib.body.module.path -like (Join-Path (Split-Path -Parent $app) '*')) "($($corelib.body.module.path))"
        $modules = Get-DapModuleNames $s $ArchName
        Check $ArchName '自己完結: エンジンは coreclr.dll を読み込んでいない' (($modules -contains 'mscordbi.dll') -and ($modules -notcontains 'coreclr.dll'))
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
        Check $ArchName '自己完結: フレーム名' ($frames[0].name -eq 'Program.Work()' -and $frames[1].name -eq 'Program.Main()') "($(($frames | ForEach-Object name) -join ', '))"
        $vars = @((Send-DapRequest $s 'variables' @{ variablesReference = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)[0].variablesReference }).body.variables)
        Check $ArchName '自己完結: 引数とローカル変数' ((($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' ') -eq 'n=21 doubled=42') "($(($vars | ForEach-Object { "$($_.name)=$($_.value)" }) -join ' '))"
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        Check $ArchName '自己完結: 終了コードが伝わる' ($exited.body.exitCode -eq 7)
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName '例外なしで終わる' $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
