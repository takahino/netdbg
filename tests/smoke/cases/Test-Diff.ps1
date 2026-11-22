# B2: 差し替わった exe の差分 (D-080)。PDB の無い Release の前後の版で、変えたメソッドと足したメソッドだけが出る
# (前に足したメソッドで、ほかのメソッドのトークンとラムダの名前の番号がずれても出ない)。カバレッジと重ねると、
# 変わったのに通っていないメソッドが再試験の範囲に出る。
function Test-Diff([string]$ArchName, [string]$Exe) {
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    if (-not $tfm) { return }
    $template = Join-Path $root 'tests\smoke\diff'
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-diff-$tfm-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    foreach ($version in 'V1', 'V2') {
        New-Item -ItemType Directory -Force (Join-Path $work "$version\src") | Out-Null
        [IO.File]::WriteAllText((Join-Path $work "$version\src\Program.cs"), [IO.File]::ReadAllText((Join-Path $template "$version.cs.txt")))
        [IO.File]::WriteAllText((Join-Path $work "$version\src\SmokeDiff.csproj"), ([IO.File]::ReadAllText((Join-Path $template 'Diff.csproj.txt')) -replace '\{TFM\}', $tfm))
        $ErrorActionPreference = 'Continue'
        & dotnet build (Join-Path $work "$version\src") -c Release -r "win-$ArchName" "-p:PlatformTarget=$ArchName" -p:SelfContained=false -o (Join-Path $work "$version\bin") -nologo -v q | Out-Null
        $ErrorActionPreference = 'Stop'
        if ($LASTEXITCODE -ne 0) {
            Check $ArchName "差分: $version をビルドできる" $false "(exit code $LASTEXITCODE)"
            return
        }
    }
    $old = Join-Path $work 'V1\bin\SmokeDiff.exe'
    $new = Join-Path $work 'V2\bin\SmokeDiff.exe'

    # コマンドラインの --diff (デバッグはしない)。
    $report = (& $Exe --diff $old $new) -join "`n"
    $rows = @($report -split "`n" | Where-Object { $_ -match '^\| (変わった|足された|消えた) \|' })
    Check $ArchName '差分 (--diff): 変えた Adjust と足した Label だけが出る' ($rows.Count -eq 2 -and $report -match '変わった \| `Program\.Adjust\(int\)`' -and $report -match '足された \| `Program\.Label\(\)`') "($report)"
    # 同じものどうしは、何も変わっていない (型引数だけが違う多重定義 Make<T>(string) と Make(string) を取り違えない)。
    $self = (& $Exe --diff $new $new) -join "`n"
    Check $ArchName '差分 (--diff): 同じ exe どうしは差分なし' ($self -match '変わった 0、足された 0、消えた 0') "($self)"

    # デバッグ中: 後の版をカバレッジつきで起動し、Sum で止めて (Adjust は通らない道筋)、前の版と比べる。
    $out = Join-Path $work 'v2.cobertura.xml'
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $new; coverage = @{ output = $out } })
        [void](Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @(@{ name = 'Program.Sum' }) })
        [void](Send-DapRequest $s 'configurationDone')
        $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 30000
        $answer = Send-DapRequest $s 'netdbg.diffAssemblies' @{ old = $old } -TimeoutMs 30000
        $changes = @($answer.body.changes)
        $adjust = @($changes | Where-Object { $_.name -eq 'Adjust' })[0]
        $label = @($changes | Where-Object { $_.name -eq 'Label' })[0]
        Check $ArchName '差分 (デバッグ中): いまのプログラムと前の版を比べ、変えたものと足したものだけ' ($answer.success -and $changes.Count -eq 2 -and $adjust.kind -eq 'changed' -and $label.kind -eq 'added') "($($answer.message) $($answer.body | ConvertTo-Json -Compress -Depth 5))"
        Check $ArchName '差分 (デバッグ中): 通っていない Adjust と Label が再試験の範囲に出る' ($answer.body.summary.retest -eq 2 -and $adjust.retest -and $label.retest -and $adjust.executed -eq $false) "($($answer.body.summary | ConvertTo-Json -Compress))"
        Check $ArchName '差分 (デバッグ中): 逆コンパイルしたソースの場所が付く' ($adjust.source.path -match '\\netdbg\\decompiled\\' -and $adjust.line -gt 0 -and (Get-Content $adjust.source.path -Encoding UTF8)[$adjust.line - 1] -match 'Adjust|return|\{') "($($adjust | ConvertTo-Json -Compress -Depth 4))"
        [void](Send-DapRequest $s 'setFunctionBreakpoints' @{ breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stop.body.threadId })
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 30000)
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName '差分 (デバッグ中): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 書き出したカバレッジの XML と重ねる (--diff --coverage)。
    if (Test-Path $out) {
        $report = (& $Exe --diff $old $new --coverage $out) -join "`n"
        Check $ArchName '差分 (--diff --coverage): XML と重ねて、再試験の範囲を出す' ($report -match '再試験の範囲 2' -and $report -match 'Adjust\(int\)`.*\*\*再試験\*\*') "($report)"
    } else {
        Check $ArchName '差分: カバレッジの XML が書かれる' $false "($out)"
    }
}
