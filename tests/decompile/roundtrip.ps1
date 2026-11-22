<#
.SYNOPSIS
  エンジンの中の逆コンパイル（D-060）で戻した C# が、正しいかを確かめる。

.DESCRIPTION
  tests/decompile/samples/<名前>/Program.cs を、.NET 10 と net48、Debug と Release でビルドし、エンジンの
  `--decompile` で C# に戻す。戻した C# をビルドし直して動かし、もとのプログラムと同じ出力になるかを比べる。
  C# に戻せず IL で書いたメソッドの数、ビルドし直したときのエラーの数も出す。

.PARAMETER Exe
  使うエンジン（既定は dist\x86\netdbg.exe）。

.PARAMETER Sample
  見本の名前（既定はすべて）。
#>
param(
    [string]$Exe = '',
    [string]$Sample = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Exe) { $Exe = Join-Path $root 'dist\x86\netdbg.exe' }
$failures = 0

function New-Project([string]$Folder, [string]$Name, [string]$Tfm) {
    $project = @"
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>$Tfm</TargetFramework>
    <AssemblyName>$Name</AssemblyName>
    <Nullable>disable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <LangVersion>latest</LangVersion>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <PlatformTarget>x86</PlatformTarget>
    <Prefer32Bit>false</Prefer32Bit>
    <NoWarn>CS0162;CS0164;CS0168;CS0219;CS0414;CS0649;CS8321</NoWarn>
  </PropertyGroup>
</Project>
"@
    [IO.File]::WriteAllText((Join-Path $Folder "$Name.csproj"), $project)
}

function Invoke-Build([string]$Folder, [string]$Configuration, [string]$Output) {
    $ErrorActionPreference = 'Continue'
    $log = & dotnet build $Folder -c $Configuration -o $Output -nologo -v q 2>&1 | Out-String
    $ErrorActionPreference = 'Stop'
    return @{ Ok = ($LASTEXITCODE -eq 0); Log = $log }
}

$samples = Get-ChildItem (Join-Path $PSScriptRoot 'samples') -Directory | Where-Object { -not $Sample -or $_.Name -eq $Sample }
foreach ($s in $samples) {
    foreach ($tfm in 'net10.0', 'net48') {
        foreach ($configuration in 'Debug', 'Release') {
            $label = "$($s.Name) $tfm $configuration"
            $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-roundtrip\$($s.Name)-$tfm-$configuration"
            if (Test-Path $work) { Remove-Item -Recurse -Force $work }
            $original = Join-Path $work 'original'
            $decompiled = Join-Path $work 'decompiled'
            New-Item -ItemType Directory -Force $original | Out-Null
            Copy-Item (Join-Path $s.FullName '*.cs') $original
            New-Project $original $s.Name $tfm
            $built = Invoke-Build $original $configuration (Join-Path $work 'original-bin')
            if (-not $built.Ok) {
                Write-Host "  FAIL  ${label}: 見本をビルドできない`n$($built.Log)" -ForegroundColor Red
                $failures++
                continue
            }
            $assembly = Join-Path $work "original-bin\$($s.Name).$(if ($tfm -eq 'net48') { 'exe' } else { 'dll' })"
            $summary = & $Exe --decompile $assembly $decompiled
            $fallback = @(Select-String -Path (Join-Path $decompiled '*.cs') -Pattern 'C# に戻せないので' -ErrorAction SilentlyContinue).Count
            New-Project $decompiled "$($s.Name)" $tfm
            $rebuilt = Invoke-Build $decompiled $configuration (Join-Path $work 'decompiled-bin')
            $errors = @([regex]::Matches($rebuilt.Log, '(\S+\.cs)\((\d+),\d+\): error (CS\d+): ([^\[\r\n]*)') | ForEach-Object { "$([IO.Path]::GetFileName($_.Groups[1].Value))($($_.Groups[2].Value)): $($_.Groups[3].Value) $($_.Groups[4].Value.Trim())" } | Sort-Object -Unique)
            $same = $false
            if ($rebuilt.Ok) {
                $ErrorActionPreference = 'Continue'
                $expected = (& (Join-Path $work "original-bin\$($s.Name).exe") 2>&1) -join "`n"
                $actual = (& (Join-Path $work "decompiled-bin\$($s.Name).exe") 2>&1) -join "`n"
                $ErrorActionPreference = 'Stop'
                $same = $expected -eq $actual
            }
            $ok = $rebuilt.Ok -and $same -and $fallback -eq 0
            $line = "${label}: IL に落ちたメソッド $fallback、ビルドのエラー $($errors.Count)、出力 $(if ($same) { '同じ' } elseif ($rebuilt.Ok) { '違う' } else { '-' })  ($summary)"
            if ($ok) {
                Write-Host "  ok    $line"
            } else {
                Write-Host "  FAIL  $line" -ForegroundColor Red
                $errors | Select-Object -First 15 | ForEach-Object { Write-Host "          $_" }
                if ($rebuilt.Ok -and -not $same) {
                    Write-Host "          もと: $($expected -replace "`n", ' | ')"
                    Write-Host "          戻し: $($actual -replace "`n", ' | ')"
                }
                $failures++
            }
        }
    }
}
Write-Host ''
Write-Host "$failures 件の失敗 (作業フォルダー: $(Join-Path ([IO.Path]::GetTempPath()) 'netdbg-roundtrip'))"
if ($failures -gt 0) { exit 1 }