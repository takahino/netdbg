# 段階 3 / 5: シンボルの探し方。別のビルドの PDB は使わない。シンボルディレクトリ (<フォルダー>\<MVID>\symbols.pdb) の PDB を使う。
# MVID を PowerShell から読めるのは .NET Framework のアセンブリなので、net48 の対象で確かめる。
function Test-Symbols([string]$ArchName, [string]$Exe) {
    $script:flavor = $flavors[1]
    $other = if ($ArchName -eq 'x86') { 'x64' } else { 'x86' }
    $app = Get-AppPath $ArchName
    $otherPdb = [IO.Path]::ChangeExtension((Get-AppPath $other), '.pdb')
    if (-not (Test-Path $app) -or -not (Test-Path $otherPdb)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-symbols-$ArchName"
    if (Test-Path $work) { Remove-Item $work -Recurse -Force }
    $copy = Join-Path $work 'app'
    [void](New-Item -ItemType Directory $copy)
    Copy-Item $app, "$app.config" $copy
    $copied = Join-Path $copy (Split-Path -Leaf $app)
    $s = $null
    try {
        # 1. 隣にあるのが別のビルド (もう一方のビット数) の PDB。
        Copy-Item $otherPdb ([IO.Path]::ChangeExtension($copied, '.pdb'))
        $s = Start-DapSession -Exe $Exe
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $copied })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = (Get-MarkerLine $source 'work') }) })
        [void](Send-DapRequest $s 'configurationDone')
        $exited = Wait-DapEvent $s 'exited' -TimeoutMs 30000
        $told = @($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match 'このビルドのものではありません' })
        Check $ArchName '別のビルドの PDB は使わず、理由を伝える' ($exited.body.exitCode -eq 7 -and $told.Count -eq 1) "($(($s.Events | Where-Object { $_.event -eq 'output' -and $_.body.category -eq 'console' } | ForEach-Object { $_.body.output.Trim() }) -join ' / '))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)

        # 2. 隣に PDB が無く、シンボルディレクトリにある。MVID は別のプロセスで読む (同じ名前のアセンブリを、このプロセスに
        #    ビット数ごとに 2 回読み込むことはできない)。ファイルを開いたままにしないよう、バイト列から読む。
        Remove-Item ([IO.Path]::ChangeExtension($copied, '.pdb'))
        $mvid = & powershell -NoProfile -NonInteractive -Command "[Reflection.Assembly]::ReflectionOnlyLoad([IO.File]::ReadAllBytes('$app')).ManifestModule.ModuleVersionId.ToString('D')"
        $symbols = Join-Path $work 'symbols'
        [void](New-Item -ItemType Directory (Join-Path $symbols $mvid))
        Copy-Item ([IO.Path]::ChangeExtension($app, '.pdb')) (Join-Path $symbols "$mvid\symbols.pdb")
        $env:NETDBG_SYMBOL_DIRS = "$(Join-Path $work 'nothing-here');$symbols"
        $s = Start-DapSession -Exe $Exe
        Remove-Item Env:NETDBG_SYMBOL_DIRS
        $stop = Start-AtMarker $s $copied $source 'work'
        $module = Wait-DapEvent $s 'module' -Where { param($m) $m.body.module.name -eq $script:flavor.Module }
        Check $ArchName 'シンボルディレクトリの PDB でブレークポイントに止まる' ($stop.body.reason -eq 'breakpoint' -and $module.body.module.symbolStatus -eq 'Symbols loaded.')
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '例外なしで終わる' $false $_.Exception.Message
        if ($s -and -not $s.Process.HasExited) { $s.Process.Kill() }
    } finally {
        Remove-Item Env:NETDBG_SYMBOL_DIRS -ErrorAction SilentlyContinue
        $script:flavor = $flavors[0]
        Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}
