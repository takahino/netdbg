<#
.SYNOPSIS
  エンジンの中の逆コンパイル（D-060）で戻した C# が、正しいかを確かめる。

.DESCRIPTION
  tests/decompile/samples/<名前>/Program.cs を、.NET 10 と net48、Debug と Release でビルドし、エンジンの
  `--decompile` で C# に戻す。戻した C# をビルドし直して動かし、もとのプログラムと同じ出力になるかを比べる。
  C# に戻せず IL で書いたメソッドの数、ビルドし直したときのエラーの数も出す。
  見本 Using は、別アセンブリの StringWriter が using に戻ることと、列挙子が using にならないことも見る。

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

function Get-MethodText([string]$all, [string]$name) {
    $at = 0
    while (($found = $all.IndexOf("$name(", $at)) -ge 0) {
        $brace = $all.IndexOf('{', $found)
        if ($brace -lt 0) { return $null }
        $between = $all.Substring($found, $brace - $found)
        if ($between -notmatch ';') {
            $depth = 0
            for ($i = $brace; $i -lt $all.Length; $i++) {
                $ch = $all[$i]
                if ($ch -eq '{') { $depth++ }
                elseif ($ch -eq '}') {
                    $depth--
                    if ($depth -eq 0) { return $all.Substring($brace, $i - $brace + 1) }
                }
            }
            return $null
        }
        $at = $found + $name.Length + 1
    }
    return $null
}

# 見本 Using。StringWriter は別アセンブリの型で、using に戻す。列挙子は using にしない。
function Test-UsingShape([string]$Name, [string]$Folder) {
    if ($Name -ne 'Using') { return @() }
    $all = (Get-ChildItem $Folder -Filter *.cs -Recurse | ForEach-Object { [IO.File]::ReadAllText($_.FullName) }) -join "`n"
    $problems = @()
    foreach ($method in 'Quoted', 'FromCall') {
        $body = Get-MethodText $all $method
        if (-not $body) { $problems += "$method が無い"; continue }
        if ($body -notmatch 'using\s*\(') { $problems += "$method が using になっていない" }
        if ($body -match '\?\.Dispose\s*\(') { $problems += "$method が ?.Dispose のまま" }
    }
    $sum = Get-MethodText $all 'Sum'
    if (-not $sum) { $problems += 'Sum が無い' }
    elseif ($sum -match 'using\s*\(') { $problems += 'Sum の列挙子が using になっている' }
    return $problems
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
            $shape = @(Test-UsingShape $s.Name $decompiled)
            $ok = $rebuilt.Ok -and $same -and $fallback -eq 0 -and $shape.Count -eq 0
            $line = "${label}: IL に落ちたメソッド $fallback、ビルドのエラー $($errors.Count)、出力 $(if ($same) { '同じ' } elseif ($rebuilt.Ok) { '違う' } else { '-' })  ($summary)"
            if ($ok) {
                Write-Host "  ok    $line"
            } else {
                Write-Host "  FAIL  $line" -ForegroundColor Red
                $shape | ForEach-Object { Write-Host "          $_" }
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