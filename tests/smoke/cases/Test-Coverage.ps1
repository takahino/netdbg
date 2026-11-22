# B1: デバッグしているアプリの行と分岐のカバレッジ (D-079)。Cobertura XML に、通った行と通らなかった行、分かれ道の向きが出る。
# 偽の「通った」を出さないことを確かめる (入っていない if の中の行、片方だけ通った分岐、数え始める前に通った行)。

# Cobertura の class の lines から、ファイル名が $FileName で終わる class の $Line 行。
function Get-CoverageLine($Xml, [string]$FileName, [int]$Line) {
    foreach ($class in @($Xml.coverage.packages.package.classes.class)) {
        if (-not $class.filename.EndsWith($FileName)) { continue }
        foreach ($entry in @($class.lines.line)) { if ([int]$entry.number -eq $Line) { return $entry } }
    }
    return $null
}

# 対象が終わるまで続け、止まった理由を並べて返す。
function Wait-CoverageExit($Session) {
    $reasons = @()
    while ($true) {
        $next = $null
        for ($i = 0; $i -lt $Session.Events.Count; $i++) {
            if ($Session.Events[$i].event -in @('stopped', 'exited')) { $next = $Session.Events[$i]; $Session.Events.RemoveAt($i); break }
        }
        if (-not $next) {
            $message = Read-DapMessage $Session 60000
            if ($message.type -eq 'event' -or $message.type -eq 'request') { [void]$Session.Events.Add($message) }
            continue
        }
        if ($next.event -eq 'exited') { return , $reasons }
        $reasons += $next.body.reason
        [void](Send-DapRequest $Session 'continue' @{ threadId = $next.body.threadId })
    }
}

function Test-Coverage([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $waitCheck = Get-MarkerLine $source 'wait-check'
    $tick = Get-MarkerLine $source 'tick'
    $loop = Get-MarkerLine $source 'loop'
    $copy = Get-MarkerLine $source 'copy'
    # `for (int i = 0; i < 3; i++)` は @loop の 2 行上 (間に `{`)。
    $for = $loop - 2
    $out = Join-Path ([IO.Path]::GetTempPath()) "netdbg-coverage-$ArchName-$($script:flavor.Dir).xml"

    # 1. 最後まで走らせて、書き出す。
    Remove-Item $out -ErrorAction SilentlyContinue
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; coverage = @{ output = $out } })
        [void](Send-DapRequest $s 'configurationDone')
        # 知らせは届いた順に読む。書いた知らせを見つけた時点で exited をまだ読んでいなければ、その前に届いている。
        $written = Wait-DapEvent $s 'netdbg.coverage' -TimeoutMs 60000
        $before = -not ($s.Events | Where-Object { $_.event -eq 'exited' })
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 60000)
        Check $ArchName 'カバレッジ: 終わる前に、書いたファイルの知らせが届く' ($before -and $written.body.path -eq $out -and (Test-Path $out)) "($($written.body | ConvertTo-Json -Compress))"
        $xml = [xml](Get-Content $out -Raw -Encoding UTF8)
        $packages = @($xml.coverage.packages.package | ForEach-Object { $_.name })
        Check $ArchName 'カバレッジ: アプリだけを数え、.NET の部品は数えない' ($packages.Count -ge 1 -and -not ($packages | Where-Object { $_ -like 'System*' -or $_ -like 'Microsoft*' })) "($($packages -join ','))"
        $class = @($xml.coverage.packages.package.classes.class | Where-Object { $_.filename.EndsWith('Program.cs') })[0]
        $file = Join-Path $xml.coverage.sources.source $class.filename
        Check $ArchName 'カバレッジ: sources と filename から、ソースのファイルに辿り着ける' (Test-Path $file) "($file)"
        $entry = Get-CoverageLine $xml 'Program.cs' $waitCheck
        Check $ArchName 'カバレッジ: 通った if の行は hits 1、分岐は片方ずつだけ (2/4)' ($entry -and $entry.hits -eq '1' -and $entry.branch -eq 'true' -and $entry.'condition-coverage' -eq '50% (2/4)') "($($entry.OuterXml))"
        $entry = Get-CoverageLine $xml 'Program.cs' $tick
        Check $ArchName 'カバレッジ: 入っていない if の中の行は hits 0' ($entry -and $entry.hits -eq '0') "($($entry.OuterXml))"
        $entry = Get-CoverageLine $xml 'Program.cs' $for
        Check $ArchName 'カバレッジ: for の条件は両方の向きを通る (2/2)' ($entry -and $entry.hits -eq '1' -and $entry.'condition-coverage' -eq '100% (2/2)') "($($entry.OuterXml))"
        $entry = Get-CoverageLine $xml 'Program.cs' $loop
        Check $ArchName 'カバレッジ: ループの中の行は hits 1' ($entry -and $entry.hits -eq '1') "($($entry.OuterXml))"
        # 終わったあとも、次の起動まで読める。
        $lines = @((Send-DapRequest $s 'netdbg.coverage' @{ file = $source }).body.lines)
        $line = @($lines | Where-Object { $_.line -eq $waitCheck })[0]
        $ways = @($line.decisions | ForEach-Object { $_.outcomes | ForEach-Object { $_.seen } })
        Check $ArchName 'カバレッジ: 終わったあと、ファイルの行ごとの状態を読める' ($line.hit -and $line.branchesCovered -eq 2 -and $line.branchesValid -eq 4 -and ($ways | Where-Object { $_ }).Count -eq 2) "($($line | ConvertTo-Json -Compress -Depth 5))"
        [void](Send-DapRequest $s 'disconnect')
        Check $ArchName 'エンジンの終了コードが 0' ((Stop-DapSession $s) -eq 0)
    } catch {
        Check $ArchName 'カバレッジ: 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 2. 同じ場所の重なり: 入口で止める (Main の IL 0) と、行のブレークポイント (ループの中)。それぞれ取りこぼさず、二重にも止まらない。
    Remove-Item $out -ErrorAction SilentlyContinue
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        [void](Send-DapRequest $s 'launch' @{ program = $app; stopAtEntry = $true; coverage = @{ output = $out } })
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $loop }) })
        [void](Send-DapRequest $s 'configurationDone')
        $reasons = Wait-CoverageExit $s
        Check $ArchName 'カバレッジ: 入口と行のブレークポイントで、1 回ずつ止まる' (($reasons -join ',') -eq 'entry,breakpoint,breakpoint,breakpoint') "($($reasons -join ','))"
        $xml = [xml](Get-Content $out -Raw -Encoding UTF8)
        $entry = Get-CoverageLine $xml 'Program.cs' $loop
        Check $ArchName 'カバレッジ: ブレークポイントのある行も数える' ($entry -and $entry.hits -eq '1') "($($entry.OuterXml))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'カバレッジ (重なり): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 3. PDB の無い exe: 読み込みのときに逆コンパイルし、そのソースの上で数える (Test-Decompile が作った対象を使う)。
    $tfm = switch ($script:flavor.Dir) { 'Console' { 'net10.0' } 'Console48' { 'net48' } default { $null } }
    $plain = if ($tfm) { Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-decompile-$tfm-$ArchName\bin\SmokeDecompile.exe" } else { '' }
    if ($plain -and (Test-Path $plain)) {
        Remove-Item $out -ErrorAction SilentlyContinue
        $s = Start-DapSession -Exe $Exe
        try {
            [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
            [void](Send-DapRequest $s 'launch' @{ program = $plain; coverage = @{ output = $out } })
            [void](Send-DapRequest $s 'configurationDone')
            [void](Wait-DapEvent $s 'exited' -TimeoutMs 60000)
            $xml = [xml](Get-Content $out -Raw -Encoding UTF8)
            $sourceRoot = [string]$xml.coverage.sources.source
            $class = @($xml.coverage.packages.package | Where-Object { $_.name -eq 'SmokeDecompile' } | ForEach-Object { $_.classes.class } | Where-Object { $_.name -eq 'Program' })[0]
            $lines = @($class.lines.line)
            $loopLine = @($lines | Where-Object { $_.'condition-coverage' -eq '100% (2/2)' })
            Check $ArchName 'カバレッジ (PDB 無し): 逆コンパイルしたソースの上で、通った行とループの条件の両方の向きを数える' ($sourceRoot -match '\\netdbg\\decompiled\\' -and (Test-Path (Join-Path $sourceRoot $class.filename)) -and @($lines | Where-Object { $_.hits -eq '1' }).Count -ge 4 -and $loopLine.Count -ge 1) "($sourceRoot $($class.OuterXml))"
            [void](Send-DapRequest $s 'disconnect')
            [void](Stop-DapSession $s)
        } catch {
            Check $ArchName 'カバレッジ (PDB 無し): 例外なしで終わる' $false $_.Exception.Message
            if (-not $s.Process.HasExited) { $s.Process.Kill() }
        }
    }

    # 4. 途中から数え始める: ループの中で止めてから入れる。それより前に通った行は数えず、止まっている行の 1 回目も数えない。
    Remove-Item $out -ErrorAction SilentlyContinue
    $s = Start-DapSession -Exe $Exe
    try {
        $stopped = Start-AtMarker $s $app $source 'loop'
        $set = Send-DapRequest $s 'netdbg.setCoverage' @{ enabled = $true; output = $out }
        [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @() })
        [void](Send-DapRequest $s 'continue' @{ threadId = $stopped.body.threadId })
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 60000)
        Check $ArchName 'カバレッジ (途中から): 入れられて、終わるときに書く' ($set.success -and (Test-Path $out)) "($($set.message))"
        $xml = [xml](Get-Content $out -Raw -Encoding UTF8)
        $passed = Get-CoverageLine $xml 'Program.cs' $waitCheck
        Check $ArchName 'カバレッジ (途中から): 入れる前に通った行は数えない' ($passed -and $passed.hits -eq '0') "($($passed.OuterXml))"
        $entry = Get-CoverageLine $xml 'Program.cs' $loop
        $after = Get-CoverageLine $xml 'Program.cs' $copy
        $cond = Get-CoverageLine $xml 'Program.cs' $for
        Check $ArchName 'カバレッジ (途中から): 入れたあとに通った行と分岐は数える' ($entry.hits -eq '1' -and $after.hits -eq '1' -and $cond.'condition-coverage' -eq '100% (2/2)') "($($entry.OuterXml) $($after.OuterXml) $($cond.OuterXml))"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'カバレッジ (途中から): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }

    # 5. 保存先 (folder): その下に <プログラム名>-<日時>.cobertura.xml を書く。ドライブから書いていないフォルダーは断る。
    $folder = Join-Path ([IO.Path]::GetTempPath()) "netdbg-coverage-folder-$ArchName-$($script:flavor.Dir)"
    if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
    $s = Start-DapSession -Exe $Exe
    try {
        [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg' })
        $relative = Send-DapRequest $s 'launch' @{ program = $app; coverage = @{ folder = 'relative\out' } }
        Check $ArchName 'カバレッジ (保存先): ドライブから書いていないフォルダーは断る' (-not $relative.success -and $relative.message -match 'ドライブから') "($($relative.message))"
        [void](Send-DapRequest $s 'launch' @{ program = $app; coverage = @{ folder = $folder } })
        [void](Send-DapRequest $s 'configurationDone')
        $written = Wait-DapEvent $s 'netdbg.coverage' -TimeoutMs 60000
        [void](Wait-DapEvent $s 'exited' -TimeoutMs 60000)
        $path = "$($written.body.path)"
        Check $ArchName 'カバレッジ (保存先): 保存先の下に <プログラム名>-<日時>.cobertura.xml を書く' ((Split-Path $path -Parent) -eq $folder -and (Split-Path $path -Leaf) -match '^.+-\d{8}-\d{6}\.cobertura\.xml$' -and (Test-Path $path)) "($path)"
        [void](Send-DapRequest $s 'disconnect')
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName 'カバレッジ (保存先): 例外なしで終わる' $false $_.Exception.Message
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}
