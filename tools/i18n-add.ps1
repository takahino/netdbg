<#
.SYNOPSIS
    訳の表 (i18n/<言語>.json) に、英語の文と訳の組を足す (D-084)。表は、英語の文の順に並べ直して書く。
.PARAMETER Pairs
    足す組を書いた UTF-8 のファイル。1 行に「英語の文<TAB>訳」。空の行と # で始まる行は読まない。
    文の中の改行は \n と書く。
.PARAMETER Lang
    言語の札 (既定 ja)。
.PARAMETER Replace
    同じ英語の文が既にあるとき、訳を置き換える (既定では、違う訳なら止まる)。
.PARAMETER SkipExisting
    同じ英語の文が既にあるとき、いまの訳を残す。
#>
param(
    [Parameter(Mandatory)][string]$Pairs,
    [string]$Lang = 'ja',
    [switch]$Replace,
    # 同じ英語の文が既にあれば、いまの訳を残す (足す側の訳は捨てる)。
    [switch]$SkipExisting
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$path = Join-Path $root "i18n\$Lang.json"
$catalog = [System.Collections.Generic.SortedDictionary[string, string]]::new([StringComparer]::Ordinal)
if (Test-Path $path) {
    $json = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
    # ConvertFrom-Json は名前の大文字と小文字を区別しない (Text と text がぶつかる) ので、区別する読み手を使う。
    Add-Type -AssemblyName System.Web.Extensions
    $serializer = New-Object System.Web.Script.Serialization.JavaScriptSerializer
    $serializer.MaxJsonLength = [int]::MaxValue
    $object = $serializer.Deserialize($json, [System.Collections.Generic.Dictionary[string, string]])
    foreach ($entry in $object.GetEnumerator()) { $catalog[$entry.Key] = $entry.Value }
}
foreach ($line in [IO.File]::ReadAllLines($Pairs, [Text.Encoding]::UTF8)) {
    # # で始まり、タブの無い行は注記 (訳の組は、# で始まってもタブで分けてある)。
    if (-not $line.Trim() -or ($line.StartsWith('#') -and -not $line.Contains("	"))) { continue }
    $parts = $line.Split("`t")
    if ($parts.Count -ne 2) { throw "英語の文と訳をタブで分けてください: $line" }
    $source = $parts[0].Replace('\n', "`n")
    $translated = $parts[1].Replace('\n', "`n")
    if ($catalog.ContainsKey($source) -and $SkipExisting) { continue }
    if ($catalog.ContainsKey($source) -and $catalog[$source] -ne $translated -and -not $Replace) {
        throw "違う訳が既にあります (-Replace で置き換え): $source => $($catalog[$source]) / $translated"
    }
    $catalog[$source] = $translated
}
# JSON の文字列 (ASCII 以外はそのまま。制御文字と " \ だけを逃がす)。
function Quote([string]$text) {
    $builder = [Text.StringBuilder]::new('"')
    foreach ($c in $text.ToCharArray()) {
        switch ($c) {
            '"' { [void]$builder.Append('\"') }
            '\' { [void]$builder.Append('\\') }
            "`n" { [void]$builder.Append('\n') }
            "`r" { [void]$builder.Append('\r') }
            "`t" { [void]$builder.Append('\t') }
            default {
                if ([int]$c -lt 0x20) { [void]$builder.AppendFormat('\u{0:x4}', [int]$c) } else { [void]$builder.Append($c) }
            }
        }
    }
    [void]$builder.Append('"')
    $builder.ToString()
}
$lines = foreach ($entry in $catalog.GetEnumerator()) { '  ' + (Quote $entry.Key) + ': ' + (Quote $entry.Value) }
[IO.File]::WriteAllText($path, "{`n" + ($lines -join ",`n") + "`n}`n", [Text.UTF8Encoding]::new($false))
Write-Host "$path : $($catalog.Count) 件"
