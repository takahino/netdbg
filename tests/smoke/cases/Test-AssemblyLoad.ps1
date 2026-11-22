# アセンブリを読めなかった理由 (D-091): FileNotFound / FileLoad の記録に、要求した名前と版、読み込み済みの同じ名前の版、
# 構成ファイルのバインディング リダイレクト (.NET Framework) が添えられ、デバッグ コンソールにも 1 行出る。
function Test-AssemblyLoad([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; args = @('load-fail'); exceptionLog = $true })
        [void](Send-DapRequest $s 'configurationDone')
        # 終わるまで待ってから、記録を見る (見つからないときに、届いた記録を詳細に出す)。
        [void](Wait-DapEvent $s 'terminated' -TimeoutMs 30000)
        $records = @($s.Events | Where-Object { $_.event -eq 'netdbg.exceptionThrown' })
        $record = $records | Where-Object { $null -ne $_.body.assemblyLoad } | Select-Object -First 1
        if (-not $record) { Check $ArchName '読めなかった理由: 記録に assemblyLoad がある' $false "($(($records | ForEach-Object { "$($_.body.typeName): $($_.body.message)" }) -join ' / '))"; [void](Send-DapRequest $s 'disconnect'); [void](Stop-DapSession $s); return }
        $load = $record.body.assemblyLoad
        # .NET Framework は厳密な名前のある System.Core を、.NET は自分のアセンブリを、無い版で読む。
        $own = if ($script:flavor.Dir -match '48') { 'System.Core' } else { [IO.Path]::GetFileNameWithoutExtension($app) }
        Check $ArchName '読めなかった理由: 要求した名前と版、読み込み済みの版が記録に入る' ($load.requested.name -eq $own -and $load.requested.version -eq '99.0.0.0' -and @($load.loaded).Count -ge 1) "($($load | ConvertTo-Json -Compress -Depth 4))"
        if ($script:flavor.Dir -match '48') {
            $redirect = @($load.redirects | Where-Object { $_.newVersion -eq '4.0.0.0' })
            Check $ArchName '読めなかった理由: 構成ファイルのバインディング リダイレクトが入る (.NET Framework)' ($redirect.Count -eq 1 -and "$($load.configFile)" -like '*.config') "($($load | ConvertTo-Json -Compress -Depth 4))"
        }
        $line = $s.Events | Where-Object { $_.event -eq 'output' -and $_.body.output -match '要求: .*99\.0\.0\.0.*読み込み済み' } | Select-Object -First 1
        Check $ArchName '読めなかった理由: デバッグ コンソールに要求と読み込み済みの版' ($null -ne $line)
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '読めなかった理由: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}