<#
.SYNOPSIS
    実際のコードの式で、エンジンの構文解析がどこまで読めるかを、コードの出どころ(年代や規模)ごとに測る。
.DESCRIPTION
    repositories.json のリポジトリを GitHub から取得し (.cs だけを target\corpus\groups\<グループ>\ に置く)、
    tools/exprgen corpus で式を抜き出して、単体試験の corpus_report で構文解析に通す。グループごとに、読めた割合を出す。
    報告は target\corpus\<グループ>.tsv.report.txt (読めなかった理由の多い順と、ラムダ式の本体を式木に組み立てられない書き方の多い順)。
    必要なもの: ./tests/apps/build.ps1 を一度実行してあること (tools/exprgen をビルドする)。
.PARAMETER Refresh
    取得し直す (既定では、取得済みのリポジトリはそのまま使う)。
#>
[CmdletBinding()]
param([switch]$Refresh)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Add-Type -AssemblyName System.IO.Compression.FileSystem
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$exprgen = Join-Path $root 'target\exprgen\bin\exprgen.exe'
if (-not (Test-Path $exprgen)) { throw "$exprgen がありません。先に ./tests/apps/build.ps1 を実行してください。" }
$groups = Get-Content (Join-Path $PSScriptRoot 'repositories.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$store = Join-Path $root 'target\corpus\groups'
New-Item -ItemType Directory -Force $store | Out-Null

foreach ($group in $groups.PSObject.Properties.Name) {
    foreach ($item in $groups.$group.repositories) {
        $dir = Join-Path $store "$group\$($item.repo.Replace('/', '__'))"
        if ((Test-Path $dir) -and -not $Refresh) { continue }
        $zip = Join-Path $store "$($item.repo.Replace('/', '__')).zip"
        # HEAD は、既定のブランチ。
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
                if ($entry.FullName.EndsWith('.cs') -and $entry.Length -gt 0) { $n++; [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $dir "$n.cs"), $true) }
            }
        } finally { $archive.Dispose() }
        Remove-Item $zip
    }
}

Push-Location $root
try {
    foreach ($group in $groups.PSObject.Properties.Name) {
        $tsv = Join-Path $root "target\corpus\$group.tsv"
        & $exprgen corpus (Join-Path $store $group) $tsv | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "exprgen corpus が失敗しました ($group)。" }
        $env:NETDBG_EXPR_CORPUS = $tsv
        $ErrorActionPreference = 'Continue'
        cargo test --quiet corpus_report -- --ignored *>&1 | Out-Null
        $ErrorActionPreference = 'Stop'
        $report = Get-Content "$tsv.report.txt" -Encoding UTF8
        Write-Host ("{0,-8} {1}" -f $group, $report[0])
        # 読めた式のうち、ラムダ式の本体を式木に組み立てられないもの (評価で断る。一覧は <グループ>.tsv.lambda-gaps.txt)。
        Write-Host ("{0,-8} {1}" -f '', ($report | Where-Object { $_ -like 'ラムダ式を含む式*' }))
    }
} finally {
    Remove-Item Env:\NETDBG_EXPR_CORPUS -ErrorAction SilentlyContinue
    Pop-Location
}