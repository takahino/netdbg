<#
.SYNOPSIS
    専用の画面 (段階 13、D-062。ui/) を WebAssembly にビルドし、target\ui\dist に並べる。
.DESCRIPTION
    エンジンのビルド (build.rs) が、target\ui\dist のファイルを exe に埋め込む。build.ps1 は、エンジンの前にこれを呼ぶ。
    必要なもの: Rust のターゲット wasm32-unknown-unknown と、ui\Cargo.lock の wasm-bindgen と同じ版の wasm-bindgen-cli。
    出力: target\ui\dist\{index.html, style.css, ui.js, ui_bg.wasm}
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$ui = Join-Path $root 'ui'
$targetDir = Join-Path $root 'target\ui'
$dist = Join-Path $targetDir 'dist'

$installed = & rustup target list --installed
if ($installed -notcontains 'wasm32-unknown-unknown') {
    throw "Rust のターゲット wasm32-unknown-unknown がありません。'rustup target add wasm32-unknown-unknown' を実行してください。"
}

# cargo は進捗を標準エラーに出す。Windows PowerShell 5.1 は、'Stop' の下でそれを例外にするので、この間だけ 'Continue'。
Write-Host '==> cargo build (ui, wasm32-unknown-unknown)' -ForegroundColor Cyan
$ErrorActionPreference = 'Continue'
& cargo build --release --target wasm32-unknown-unknown --manifest-path (Join-Path $ui 'Cargo.toml') --target-dir $targetDir
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) {
    throw "画面のビルドに失敗しました (exit code $LASTEXITCODE)。上のログを確認してください。"
}

# wasm-bindgen の CLI は、ライブラリと同じ版でないと動かない。版は ui\Cargo.lock から読む。
$lock = Get-Content (Join-Path $ui 'Cargo.lock') -Raw
$version = [regex]::Match($lock, '(?m)^name = "wasm-bindgen"\r?\nversion = "([^"]+)"').Groups[1].Value
$bindgen = Get-Command wasm-bindgen -ErrorAction SilentlyContinue
$actual = if ($bindgen) { ((& wasm-bindgen --version) -split ' ')[1] } else { '' }
if ($actual -ne $version) {
    throw "wasm-bindgen $version が要ります (今: '$actual')。'cargo install wasm-bindgen-cli --version $version --locked' を実行してください。"
}

Write-Host '==> wasm-bindgen' -ForegroundColor Cyan
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null
& wasm-bindgen --target web --no-typescript --out-dir $dist --out-name ui (Join-Path $targetDir 'wasm32-unknown-unknown\release\ui.wasm')
if ($LASTEXITCODE -ne 0) {
    throw "wasm-bindgen に失敗しました (exit code $LASTEXITCODE)。"
}
Copy-Item (Join-Path $ui 'index.html'), (Join-Path $ui 'style.css') $dist -Force
# タブのアイコン (favicon) は、exe のアイコンと同じ元 (assets\netdbg.svg)。
Copy-Item (Join-Path $root 'assets\netdbg.svg') (Join-Path $dist 'icon.svg') -Force

foreach ($file in Get-ChildItem $dist -File) {
    Write-Host ("    {0,-12} {1,10:N0} bytes" -f $file.Name, $file.Length)
}