<#
.SYNOPSIS
    VB.NET と F# の実際のコードで、どんな型や値の形がどれだけ使われているかを数える。
.DESCRIPTION
    式の構文は C# だけ (docs/DECISIONS.md D-026) なので、VB / F# のコードから式を抜き出すことはしない。代わりに、
    VB / F# で書かれた対象を止めたときに、C# の式で触ることになる値の形 (F# の list / option / レコード / 判別共用体、
    VB の Module / 省略できる引数 / 既定のプロパティなど) が、実際のコードでどれだけ使われているかを数え、
    対象アプリ (tests/apps/ConsoleFS、ConsoleVB) の適合試験に入れる形の優先順位を決める。
    repositories-languages.json のリポジトリを GitHub から取得し (.fs / .vb だけを target\corpus\languages\<グループ>\ に置く)、
    書き方ごとに、1,000 行あたりの回数と、使っているリポジトリの数を出す。報告は target\corpus\languages.report.txt。
.PARAMETER Refresh
    取得し直す (既定では、取得済みのリポジトリはそのまま使う)。
#>
[CmdletBinding()]
param([switch]$Refresh)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Add-Type -AssemblyName System.IO.Compression.FileSystem
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$groups = Get-Content (Join-Path $PSScriptRoot 'repositories-languages.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$store = Join-Path $root 'target\corpus\languages'
New-Item -ItemType Directory -Force $store | Out-Null

foreach ($group in $groups.PSObject.Properties.Name) {
    $extension = $groups.$group.extension
    foreach ($item in $groups.$group.repositories) {
        $dir = Join-Path $store "$group\$($item.repo.Replace('/', '__'))"
        if ((Test-Path $dir) -and -not $Refresh) { continue }
        $zip = Join-Path $store "$($item.repo.Replace('/', '__')).zip"
        $ref = if ($item.branch -eq 'HEAD') { 'HEAD' } else { "refs/heads/$($item.branch)" }
        try {
            Invoke-WebRequest -Uri "https://codeload.github.com/$($item.repo)/zip/$ref" -OutFile $zip -UseBasicParsing -UserAgent 'corpus-sampler'
        } catch {
            Write-Warning "$($item.repo) を取得できません (消されたか、名前が変わったのかもしれません): $($_.Exception.Message)"
            continue
        }
        if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
        New-Item -ItemType Directory -Force $dir | Out-Null
        # 名前は連番にする (もとの深いパスは、Windows のパスの長さの制限を超えることがある)。
        $archive = [IO.Compression.ZipFile]::OpenRead($zip); $n = 0
        try {
            foreach ($entry in $archive.Entries) {
                if ($entry.FullName.EndsWith($extension) -and $entry.Length -gt 0) { $n++; [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $dir "$n$extension"), $true) }
            }
        } finally { $archive.Dispose() }
        Remove-Item $zip
    }
}

# 書き方ごとの正規表現。行ごとに数える (1 行に何度出ても 1 回)。
$patterns = @{
    '.fs' = [ordered]@{
        'レコードの型 (type X = { … })'            = '^\s*(and|type)\s+[^=]+=\s*\{(?!\|)'
        '匿名レコード ({| … |})'                      = '\{\|'
        '判別共用体の場合 (| Case of …)'              = '^\s*\|\s*[A-Z]\w*\s+of\b'
        'option (Some / None)'                        = '\b(Some|None)\b'
        'voption (ValueSome / ValueNone)'             = '\b(ValueSome|ValueNone)\b'
        'Result (Ok / Error)'                         = '\b(Ok|Error)\s+\w|\bResult<'
        'list のモジュール (List.)'                   = '\bList\.\w'
        'list の連結 (::)'                            = '\w\s*::\s*\w'
        '配列 ([| … |]、Array.)'                      = '\[\||\bArray\.\w'
        'seq { … } と Seq.'                           = '\bseq\s*\{|\bSeq\.\w'
        'Map と Set'                                  = '\b(Map|Set)\.\w|\b(Map|Set)<'
        'async { … }'                                 = '\basync\s*\{'
        'task { … }'                                  = '\btask\s*\{'
        'タプル (a, b) の分解 (let (a, b) =)'          = '^\s*let\s+\(\s*\w+\s*,'
        'let mutable'                                 = '\blet\s+mutable\b'
        '参照セル (ref、!x、:=)'                       = '\bref\s+\w|:=|!\w'
        'ResizeArray'                                 = '\bResizeArray\b'
        'Dictionary'                                  = '\bDictionary<'
        'クラス (type X(…) =)'                         = '^\s*type\s+\w+(<[^>]*>)?\s*\('
        'インターフェイスの実装 (interface … with)'      = '^\s*interface\s+\w.*\bwith\b'
        '構造体 ([<Struct>])'                          = '\[<Struct>\]'
        '単位 (float<m>)'                              = '\d<[a-zA-Z]'
        'アクティブパターン ((|X|_|))'                   = '\(\|[A-Z]'
        'lazy'                                        = '\blazy\b'
        'モジュールの関数 (module X = / let f x =)'      = '^\s*let\s+(inline\s+|private\s+)?[a-z]\w*\s+\w'
    }
    '.vb' = [ordered]@{
        'Module'                                      = '^\s*(Public\s+|Friend\s+)?Module\s+\w'
        'Structure'                                   = '^\s*(Public\s+|Friend\s+|Private\s+)?Structure\s+\w'
        'Enum'                                        = '^\s*(Public\s+|Friend\s+|Private\s+)?Enum\s+\w'
        'Optional の引数'                              = '\bOptional\s+(ByVal\s+|ByRef\s+)?\w'
        'ByRef の引数'                                 = '\bByRef\s+\w'
        'Nothing'                                     = '\bNothing\b'
        'Date (DateTime)'                             = '\bAs\s+Date\b|#\d+/\d+/\d+'
        'Decimal'                                     = '\bAs\s+Decimal\b|\d+D\b'
        'null 許容 (Integer? / Nullable(Of))'          = '\bAs\s+\w+\?|Nullable\(Of'
        '配列 (Dim a(…)、As X())'                      = '^\s*Dim\s+\w+\(\d|\bAs\s+\w+\(\)'
        'ReDim'                                       = '\bReDim\b'
        'List(Of …)'                                  = '\bList\(Of\b'
        'Dictionary(Of …)'                            = '\bDictionary\(Of\b'
        'Collection (Microsoft.VisualBasic)'          = '\bNew\s+Collection\b|\bAs\s+Collection\b'
        'Static のローカル変数'                         = '^\s*Static\s+\w'
        'WithEvents'                                  = '\bWithEvents\b'
        'My. (My.Settings など)'                       = '\bMy\.\w'
        'ラムダ (Function(x) / Sub(x))'                 = '\b(Function|Sub)\s*\([^)]*\)\s*[^\s]'
        '匿名型 (New With { … })'                       = '\bNew\s+With\s*\{'
        'クエリ式 (From x In …)'                        = '\bFrom\s+\w+\s+In\b'
        'Tuple / ValueTuple'                          = '\bTuple\(Of\b|\bValueTuple\b|\bTuple\.Create\(|=\s*\(\s*[\w."]+\s*,\s*[\w."]+\s*\)\s*$'
        'Iterator Function'                           = '\bIterator\s+Function\b'
        'Async Function / Await'                      = '\bAsync\s+(Function|Sub)\b|\bAwait\b'
        'Default Property / 引数つきのプロパティ'          = '\bDefault\s+(Public\s+)?(ReadOnly\s+)?Property\b|\bProperty\s+\w+\s*\(\s*\w+\s+As'
        'Shared のメンバー'                             = '\bShared\s+(Function|Sub|Property|ReadOnly|\w+\s+As)'
        'As Object (遅延バインディング)'                  = '\bAs\s+Object\b'
        'VB の関数 (Len、Mid、Left、CInt、CStr …)'        = '\b(Len|Mid|Left|Right|InStr|CInt|CStr|CDbl|CLng|CBool|CDate|Val|IIf|UCase|LCase|Trim)\s*\('
        'If(a, b) / If(c, a, b)'                       = '\bIf\s*\([^)]*,'
        'CType / DirectCast / TryCast'                 = '\b(CType|DirectCast|TryCast)\s*\('
        'Like'                                        = '\s+Like\s+"'
        'XML リテラル'                                  = '=\s*<\w+[\s>]'
    }
}

$report = New-Object System.Text.StringBuilder
foreach ($group in $groups.PSObject.Properties.Name) {
    $extension = $groups.$group.extension
    $files = @(Get-ChildItem (Join-Path $store $group) -Recurse -Filter "*$extension" -File -ErrorAction SilentlyContinue)
    $lines = 0
    $counts = [ordered]@{}; $repos = @{}
    foreach ($name in $patterns[$extension].Keys) { $counts[$name] = 0; $repos[$name] = @{} }
    foreach ($file in $files) {
        $repo = $file.Directory.Name
        foreach ($line in [IO.File]::ReadLines($file.FullName)) {
            $lines++
            # コメントの行は数えない。
            $trimmed = $line.TrimStart()
            if ($trimmed.StartsWith('//') -or $trimmed.StartsWith("'") -or $trimmed.StartsWith('(*')) { continue }
            foreach ($name in $patterns[$extension].Keys) {
                if ($line -match $patterns[$extension][$name]) { $counts[$name]++; $repos[$name][$repo] = $true }
            }
        }
    }
    $repoCount = @($files | ForEach-Object { $_.Directory.Name } | Sort-Object -Unique).Count
    [void]$report.AppendLine("== $group ($repoCount リポジトリ、$($files.Count) ファイル、$lines 行)")
    foreach ($name in ($counts.Keys | Sort-Object { -$counts[$_] })) {
        $per = if ($lines) { 1000.0 * $counts[$name] / $lines } else { 0 }
        [void]$report.AppendLine(('{0,8:N1} / 1000 行  {1,2} リポジトリ  {2}' -f $per, $repos[$name].Count, $name))
    }
    [void]$report.AppendLine()
}
$path = Join-Path $root 'target\corpus\languages.report.txt'
[IO.File]::WriteAllText($path, $report.ToString(), [Text.UTF8Encoding]::new($true))
Write-Host $report.ToString()
