<#
.SYNOPSIS
  THIRD-PARTY-NOTICES.txt を作る。netdbg.exe に入るもの (エンジンのクレート、埋め込んだ画面 (wasm) のクレート) と、
  組み立てで隣に置く dbgshim.dll の、著作権の表示とライセンスの本文を並べる。

.DESCRIPTION
  依存の一覧は cargo metadata から取る (エンジンは i686-pc-windows-msvc、画面は wasm32-unknown-unknown の、通常の依存)。
  ライセンスの本文は、各クレートのフォルダーの LICENSE* / COPYING* / NOTICE* / UNLICENSE* から読む。本文が無いクレートは、
  ライセンスの名前だけを書いて警告する。手続き型マクロ (組み立てのときだけ動き、exe には入らない) も含める (多めに載せる)。
  依存を変えたら作り直す。
#>
param(
    [string]$Output = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'THIRD-PARTY-NOTICES.txt')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent

# 通常の依存 (dev と build の依存は除く) を、根から辿って集める。
function Get-Packages([string]$Manifest, [string]$Target) {
    $meta = & cargo metadata --format-version 1 --manifest-path $Manifest --filter-platform $Target | ConvertFrom-Json
    $byId = @{}
    foreach ($p in $meta.packages) { $byId[$p.id] = $p }
    $nodes = @{}
    foreach ($n in $meta.resolve.nodes) { $nodes[$n.id] = $n }
    $seen = @{}
    $stack = New-Object System.Collections.Stack
    foreach ($m in $meta.workspace_members) { $stack.Push($m) }
    while ($stack.Count) {
        $id = $stack.Pop()
        if ($seen.ContainsKey($id)) { continue }
        $seen[$id] = $true
        foreach ($d in $nodes[$id].deps) {
            if (@($d.dep_kinds | Where-Object { $null -eq $_.kind }).Count) { $stack.Push($d.pkg) }
        }
    }
    $members = @($meta.workspace_members)
    $seen.Keys | Where-Object { $members -notcontains $_ } | ForEach-Object { $byId[$_] }
}

$packages = @{}
foreach ($p in @(Get-Packages (Join-Path $root 'Cargo.toml') 'i686-pc-windows-msvc') + @(Get-Packages (Join-Path $root 'ui\Cargo.toml') 'wasm32-unknown-unknown')) {
    $packages["$($p.name) $($p.version)"] = $p
}

$sb = New-Object System.Text.StringBuilder
$rule = '-' * 79
[void]$sb.AppendLine('netdbg - Third Party Notices')
[void]$sb.AppendLine('============================')
[void]$sb.AppendLine('')
[void]$sb.AppendLine('netdbg.exe statically links the Rust crates listed below (the engine, and the browser UI that is built to')
[void]$sb.AppendLine('WebAssembly and embedded in the exe). The build also places the .NET debugging shim (dbgshim.dll) next to it.')
[void]$sb.AppendLine('The original copyright notices and license terms are reproduced below.')
$missing = @()
# 配布物にライセンスの本文が無いクレートの、補いのファイル (crate=ファイル名)。
$extra = @{}
foreach ($line in Get-Content (Join-Path $PSScriptRoot 'extra\map.txt') -Encoding UTF8) {
    if ($line -match '^\s*([^#=\s][^=]*)=(.+)$') { $extra[$Matches[1].Trim()] = $Matches[2].Trim() }
}
foreach ($key in ($packages.Keys | Sort-Object)) {
    $p = $packages[$key]
    $dir = Split-Path $p.manifest_path -Parent
    $files = @(Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)' } | Sort-Object Name)
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine($rule)
    [void]$sb.AppendLine("$($p.name) $($p.version) ($($p.license))")
    if ($p.repository) { [void]$sb.AppendLine($p.repository) }
    [void]$sb.AppendLine($rule)
    if ($files.Count -eq 0 -and $extra.ContainsKey($p.name)) {
        # 配布物に本文が無いので、上流のリポジトリなどから補ったもの (tools/notices/extra)。
        [void]$sb.AppendLine('')
        [void]$sb.AppendLine(([IO.File]::ReadAllText((Join-Path $PSScriptRoot "extra\$($extra[$p.name])"))).TrimEnd())
        continue
    }
    if ($files.Count -eq 0) {
        $missing += $key
        [void]$sb.AppendLine("Licensed under $($p.license). The license text is not included in the crate.")
        continue
    }
    foreach ($f in $files) {
        [void]$sb.AppendLine('')
        [void]$sb.AppendLine("[$($f.Name)]")
        [void]$sb.AppendLine(([IO.File]::ReadAllText($f.FullName)).TrimEnd())
    }
}
[void]$sb.AppendLine('')
[void]$sb.AppendLine($rule)
[void]$sb.AppendLine('.NET Runtime (dbgshim.dll, placed next to netdbg.exe by build.ps1; from the Microsoft.Diagnostics.DbgShim package)')
[void]$sb.AppendLine('https://github.com/dotnet/runtime, https://github.com/dotnet/diagnostics')
[void]$sb.AppendLine($rule)
[void]$sb.AppendLine('')
[void]$sb.AppendLine((Get-Content (Join-Path $PSScriptRoot 'dotnet-runtime-license.txt') -Raw).TrimEnd())
[IO.File]::WriteAllText($Output, ($sb.ToString() -replace "`r?`n", "`r`n"), (New-Object System.Text.UTF8Encoding $false))
Write-Host "書きました: $Output ($($packages.Count) クレート)"
if ($missing.Count) { Write-Warning "ライセンスの本文が無いクレート: $($missing -join ', ')" }
