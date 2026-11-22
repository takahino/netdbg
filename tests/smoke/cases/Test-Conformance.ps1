# 段階 4: 式の適合試験。対象アプリの中で C# のコンパイラが出した結果 (Conformance.Values) と、同じ式 (Conformance.Texts) を
# エンジンに評価させた結果を比べる。まだ評価できない式は conformance-pending.txt に並べてあり、そこに無い式はすべて一致すること。
# 一覧にあるのに一致するようになった式があれば、それも失敗にする (一覧から外し忘れないため)。

# 目印 conformance で止めて、$Holder (Texts / Values / Lifted を持つ型) の式を順に比べる。$Label は、結果の行の頭に付ける名前。
# 止まったままのセッションを返すので、呼んだ側で追加の確かめをしてから閉じる。
function Invoke-Conformance([string]$ArchName, [string]$Exe, [string]$App, [string]$Source, [string]$Holder, [string]$Label, [string]$PendingPath, [scriptblock]$After) {
    $pending = if (Test-Path $PendingPath) { @(Get-Content $PendingPath -Encoding UTF8 | Where-Object { $_ -and -not $_.StartsWith('#') }) } else { @() }
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-conformance-$(Split-Path -Leaf (Split-Path -Parent $App))-$ArchName.log"
    $s = Start-DapSession -Exe $Exe -Arguments @('--interpreter=vscode', "--log=`"$log`"")
    try {
        $stop = Start-AtMarker $s $App $Source 'conformance' -ProgramArgs @('conformance')
        $frame = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId; levels = 1 }).body.stackFrames)[0]
        $evaluate = { param([string]$Expression) Send-DapRequest $s 'evaluate' @{ expression = $Expression; frameId = $frame.id } -TimeoutMs 30000 }
        $count = [int](& $evaluate "$Holder.Texts.Count").body.result
        $wrong = @(); $wrongTexts = @(); $fixed = @(); $matched = 0
        for ($i = 0; $i -lt $count; $i++) {
            # 式の文字列は、表示用に引用符とエスケープが付いて返る。もとの文字列に戻す。
            $shown = (& $evaluate "$Holder.Texts[$i]").body.result
            $text = $shown.Substring(1, $shown.Length - 2).Replace('\"', '"').Replace('\\', '\')
            $expected = (& $evaluate "$Holder.Values[$i]").body
            $actual = & $evaluate $text
            # 一覧に入れると int? は int になる。式の型が null 許容の値型なら、型の名前に ? を足して比べる。
            if ((& $evaluate "$Holder.Lifted[$i]").body.result -eq 'true') { $expected.type = "$($expected.type)?" }
            # null は、入れ物 (object の一覧) の型でしか見えないので、型は比べない。
            $same = $actual.success -and $actual.body.result -ceq $expected.result -and ($expected.result -eq 'null' -or $actual.body.type -ceq $expected.type)
            if ($same) { $matched++ }
            # 失敗の文言は長いので、先頭だけを見せる。
            $described = if ($actual.success) { "$($actual.body.result) : $($actual.body.type)" } else { "失敗: $($actual.message.Substring(0, [Math]::Min(60, $actual.message.Length)))" }
            if ($same -and $pending -contains $text) { $fixed += $text }
            if (-not $same -and $pending -notcontains $text) { $wrongTexts += $text; $wrong += "$text => $described (期待は $($expected.result) : $($expected.type))" }
        }
        Write-Host "        ${Label}適合: $count 件中 $matched 件が一致 (未対応の一覧 $($pending.Count) 件)"
        Check $ArchName "${Label}適合試験: 式がある" ($count -ge 20) "($count)"
        # 環境変数 NETDBG_CONFORMANCE_REPORT にファイル名を入れておくと、一致しなかった式をそこに書き足す (一覧の作り直し用)。
        if ($env:NETDBG_CONFORMANCE_REPORT -and $wrongTexts.Count) { $wrongTexts | Add-Content $env:NETDBG_CONFORMANCE_REPORT -Encoding UTF8 }
        Check $ArchName "${Label}適合試験: 未対応の一覧に無い式は、すべて一致する" ($wrong.Count -eq 0) "($($wrong.Count) 件)`n          $(($wrong | Select-Object -First 12) -join "`n          ")"
        Check $ArchName "${Label}適合試験: 一致するようになった式は、未対応の一覧から外してある" ($fixed.Count -eq 0) "($($fixed -join ' | '))"
        if ($After) { & $After $evaluate }
        [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
        [void](Stop-DapSession $s)
    } catch {
        Check $ArchName "${Label}例外なしで終わる" $false "$($_.Exception.Message) (ログ: $log)"
        if (-not $s.Process.HasExited) { $s.Process.Kill() }
    }
}

function Test-Conformance([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root 'tests\apps\Console\Program.cs'
    $pending = Join-Path $root 'tests\smoke\conformance-pending.txt'
    Invoke-Conformance $ArchName $Exe $app $source 'Conformance' '' $pending {
        param($evaluate)
        # 宣言の型が object のローカル変数どうしの + は、中身が文字列でも、C# ではエラー (宣言の型をローカル変数のシグネチャから読む)。
        $r = & $evaluate 'boxed + boxedText'
        Check $ArchName '適合試験: object のローカル変数に入った文字列は、object として扱う' (-not $r.success) "($($r.body.result))"
        # プログラムにまだ無い並びの匿名型 (適合試験には書けない: 書くと、コンパイラがその型を作ってしまう)。C# と同じ形の型を作る。
        $r = & $evaluate 'new { Zzz = 1, Yyy = "y" }'
        Check $ArchName '匿名型: プログラムに無い並びでも作れる' ($r.success -and $r.body.result -ceq '{ Zzz = 1, Yyy = "y" }' -and $r.body.type -match '^<>f__AnonymousType\d+<int, string>$') "($($r.body.result) : $($r.body.type)$($r.message))"
        $r = & $evaluate 'new { Zzz = 1, Yyy = "y" }.ToString()'
        Check $ArchName '匿名型: ToString は C# と同じ' ($r.body.result -ceq '"{ Zzz = 1, Yyy = y }"') "($($r.body.result)$($r.message))"
        $r = & $evaluate 'new { Zzz = 1 }.Equals(new { Zzz = 1 }) && !new { Zzz = 1 }.Equals(new { Zzz = 2 })'
        Check $ArchName '匿名型: Equals はメンバーの値で比べる' ($r.body.result -eq 'true') "($($r.body.result)$($r.message))"
        $r = & $evaluate 'numbers.Select(n => new { Www = n }).Last().Www'
        Check $ArchName '匿名型: ラムダ式の中でも作れる' ($r.body.result -eq '30') "($($r.body.result)$($r.message))"
    }
}

# C# 以外の言語 (F#、VB.NET) で書かれた対象の適合試験。期待する値は、同じ意味の計算をその言語で書いたもの。
function Test-LanguageConformance([string]$ArchName, [string]$Exe) {
    $fs = Join-Path $root "target\apps\$ArchName\ConsoleFS\SmokeConsoleFS.exe"
    if (Test-Path $fs) { Invoke-Conformance $ArchName $Exe $fs (Join-Path $root 'tests\apps\ConsoleFS\Program.fs') 'Program.Conformance' 'F#: ' (Join-Path $root 'tests\smoke\conformance-pending-fs.txt') $null }
    $vb = Join-Path $root "target\apps\$ArchName\ConsoleVB\SmokeConsoleVB.exe"
    if (Test-Path $vb) { Invoke-Conformance $ArchName $Exe $vb (Join-Path $root 'tests\apps\ConsoleVB\Program.vb') 'Conformance' 'VB: ' (Join-Path $root 'tests\smoke\conformance-pending-vb.txt') $null }
}
