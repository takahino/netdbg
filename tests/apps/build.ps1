<#
.SYNOPSIS
    スモーク試験の対象アプリを、ビット数ごとに target\apps\<arch>\<名前> へビルドする。
.DESCRIPTION
    必要なもの: .NET SDK。自己完結の分は、初回にランタイムのパッケージを NuGet から取得する。
#>
[CmdletBinding()]
param(
    [ValidateSet('x86', 'x64')]
    [string[]]$Arch = @('x86', 'x64')
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

# 自己完結でも作るアプリ (ランタイムを同梱した形。対象のフォルダーにある mscordbi.dll でデバッグできることを確かめる)。
$selfContained = @('Console')

# .NET 11 の SDK。システムには入れず、target の下に展開したものを使う (docs/STATUS.md に取得の手順)。
$dotnet11 = Join-Path $root 'target\dotnet11\sdk\dotnet.exe'

function Build-App([string]$Project, [string]$Name, [string]$A, [bool]$SelfContained) {
    # net11.0 を対象にするプロジェクトは、.NET 11 の SDK でビルドする。無ければ飛ばす (ほかの対象の試験は続けられる)。
    $dotnet = 'dotnet'
    if ((Get-Content (Get-ChildItem $Project -File | Where-Object { $_.Extension -in '.csproj', '.vbproj', '.fsproj' } | Select-Object -First 1).FullName -Raw) -match '<TargetFramework>net11\.0</TargetFramework>') {
        if (-not (Test-Path $dotnet11)) {
            Write-Host "==> $Name ($A): .NET 11 の SDK が $dotnet11 に無いので飛ばします" -ForegroundColor Yellow
            return
        }
        $dotnet = $dotnet11
    }
    $out = Join-Path $root "target\apps\$A\$Name"
    # 中間ファイルは、作り方ごとに別のフォルダーへ出す。プロジェクトの obj を使い回すと、前の作り方の
    # 中間ファイル (ビット数、自己完結かどうか) が混ざる。
    $obj = (Join-Path $root "target\apps\obj\$Name-$A") -replace '\\', '/'
    Write-Host "==> $Name ($A)" -ForegroundColor Cyan
    $ErrorActionPreference = 'Continue'
    & $dotnet build $Project -c Debug -r "win-$A" "-p:PlatformTarget=$A" "-p:SelfContained=$($SelfContained.ToString().ToLower())" "-p:BaseIntermediateOutputPath=$obj/" -o $out -nologo -v q
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        throw "$Name ($A) のビルドに失敗しました (exit code $LASTEXITCODE)。"
    }
}

# 単一ファイルの自己完結のアプリ (アセンブリもランタイムも exe の中。mscordbi.dll は同梱されない。D-050)。
function Publish-SingleFile([string]$Project, [string]$Name, [string]$A) {
    $out = Join-Path $root "target\apps\$A\$Name"
    $obj = (Join-Path $root "target\apps\obj\$Name-$A") -replace '\\', '/'
    Write-Host "==> $Name ($A, 単一ファイル)" -ForegroundColor Cyan
    $ErrorActionPreference = 'Continue'
    & dotnet publish $Project -c Debug -r "win-$A" "-p:PlatformTarget=$A" -p:PublishSingleFile=true --self-contained true "-p:BaseIntermediateOutputPath=$obj/" -o $out -nologo -v q
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -ne 0) {
        throw "$Name ($A) の発行に失敗しました (exit code $LASTEXITCODE)。"
    }
}

# 総当たり試験 (Sweep) と呼び出しオーバーロード試験 (Calls) の式は、tools/exprgen が作る。
# 対象アプリをビルドする前に、target\sweep と target\calls に用意しておく。
function Build-Exprgen {
    $obj = (Join-Path $root 'target\exprgen\obj') -replace '\\', '/'
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $root 'tools\exprgen') -c Release -o (Join-Path $root 'target\exprgen\bin') "-p:BaseIntermediateOutputPath=$obj/" -nologo -v q
    if ($LASTEXITCODE -ne 0) { throw "tools\exprgen のビルドに失敗しました (exit code $LASTEXITCODE)。" }
    $ErrorActionPreference = 'Stop'
}
function Build-SweepExpressions {
    Write-Host '==> exprgen (総当たり試験の式)' -ForegroundColor Cyan
    Build-Exprgen
    $ErrorActionPreference = 'Continue'
    & (Join-Path $root 'target\exprgen\bin\exprgen.exe') sweep (Join-Path $PSScriptRoot 'Sweep\Program.cs') (Join-Path $root 'target\sweep')
    if ($LASTEXITCODE -ne 0) { throw "exprgen sweep が失敗しました (exit code $LASTEXITCODE)。" }
    $ErrorActionPreference = 'Stop'
}
function Build-CallsExpressions {
    Write-Host '==> exprgen (呼び出しオーバーロードの式)' -ForegroundColor Cyan
    Build-Exprgen
    $ErrorActionPreference = 'Continue'
    & (Join-Path $root 'target\exprgen\bin\exprgen.exe') calls (Join-Path $PSScriptRoot 'Calls\Program.cs') (Join-Path $root 'target\calls')
    if ($LASTEXITCODE -ne 0) { throw "exprgen calls が失敗しました (exit code $LASTEXITCODE)。" }
    $ErrorActionPreference = 'Stop'
}
Build-SweepExpressions
Build-CallsExpressions

foreach ($project in Get-ChildItem $PSScriptRoot -Directory) {
    # 以前の作り方で出来たプロジェクト直下の obj / bin が残っていると、その中の .cs まで拾われる。
    foreach ($stale in 'obj', 'bin') {
        $path = Join-Path $project.FullName $stale
        if (Test-Path $path) { Remove-Item $path -Recurse -Force }
    }
    foreach ($a in $Arch) {
        # Console11 と RuntimeAsync は、機械に .NET 11 が無くても動くように、いつもランタイムを同梱する。
        Build-App $project.FullName $project.Name $a ($project.Name -eq 'Console11' -or $project.Name -eq 'RuntimeAsync')
        if ($selfContained -contains $project.Name) {
            Build-App $project.FullName "$($project.Name)SelfContained" $a $true
            Publish-SingleFile $project.FullName "$($project.Name)Single" $a
        }
    }
}