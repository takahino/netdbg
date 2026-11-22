<#
.SYNOPSIS
    VS Code の中で、C# 拡張のデバッガー (vsdbg) と netdbg (netdbg 拡張を通して) の速さを比べる。
.DESCRIPTION
    VS Code を専用のフォルダー (ユーザーデータと拡張) で起動し、tests/bench/vscode/run.js を拡張の試験として実行する。
    利用者の VS Code の設定と拡張には触れない。VS Code のウィンドウが開いて自動で操作されるので、終わるまで触らない。
    vsdbg は x64 だけなので、対象は x64 でビルドしたアプリ (tests/apps/build.ps1)。
.PARAMETER Extension
    netdbg 拡張のフォルダー (コンパイル済みで、debugger\netdbg-x64 にエンジンがあるもの)。
.PARAMETER Work
    専用のユーザーデータと拡張のフォルダー。extensions に C# 拡張 (ms-dotnettools.csharp) を入れておく:
    code --extensions-dir <Work>\extensions --user-data-dir <Work>\user-data --install-extension ms-dotnettools.csharp
    Work は短いパスにする (%TEMP%\vb など)。C# 拡張のデバッガーには長い名前のファイルがあり、260 文字を超えると
    vsdbg-ui.exe が起動してすぐ落ちる (0xC0000409)。
#>
param(
    [Parameter(Mandatory)][string]$Extension,
    [Parameter(Mandatory)][string]$Work,
    [string]$Code = (Join-Path $env:LOCALAPPDATA 'Programs\Microsoft VS Code\Code.exe'),
    [int]$Runs = 5,
    [int]$Repeat = 20,
    [string]$Out = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'target\bench\vscode.json')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path (Join-Path $Work 'extensions'))) { throw "C# 拡張を入れた $Work\extensions がありません。ヘルプの手順で入れてください。" }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
Remove-Item $Out, "$Out.log" -ErrorAction SilentlyContinue
$env:BENCH_ROOT = $root; $env:BENCH_OUT = $Out; $env:BENCH_RUNS = $Runs; $env:BENCH_REPEAT = $Repeat
$arguments = @(
    "--extensions-dir=`"$Work\extensions`"", "--user-data-dir=`"$Work\user-data`"",
    "--extensionDevelopmentPath=`"$PSScriptRoot\vscode`"", "--extensionDevelopmentPath=`"$Extension`"",
    "--extensionTestsPath=`"$PSScriptRoot\vscode\run.js`"",
    '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--new-window'
)
$process = Start-Process -FilePath $Code -ArgumentList $arguments -PassThru
$process.WaitForExit()
if (-not (Test-Path $Out)) { throw "結果がありません。$Out.log と VS Code の出力を確認してください。" }
$result = Get-Content $Out -Raw -Encoding UTF8 | ConvertFrom-Json
foreach ($target in $result.results.PSObject.Properties) {
    "`n## $($target.Name)`n"
    $engines = @($target.Value.PSObject.Properties.Name)
    "| scenario | $($engines -join ' | ') |"
    "|---|$(($engines | ForEach-Object { '---:' }) -join '|')|"
    foreach ($key in $target.Value.($engines[0]).PSObject.Properties.Name) {
        "| $key | $(($engines | ForEach-Object { $v = $target.Value.$_.$key; if ($null -eq $v) { 'n/a' } else { $v } }) -join ' | ') |"
    }
}
