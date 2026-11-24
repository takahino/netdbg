<#
.SYNOPSIS
    ラムダを渡す呼び出しのオーバーロード試験。機械的に作った式を、エンジンに評価させて、C# のコンパイラの結果と比べる。
.DESCRIPTION
    式は tools/exprgen が作る (tests/apps/build.ps1 が実行する)。コンパイラが受け付けた式は、対象アプリ (tests/apps/Calls) の
    中で計算されて Calls.Values に並ぶ。同じ式をエンジンに評価させ、値と型が同じことを確かめる。コンパイラが拒んだ式
    (-Rejected) は、エンジンも拒むこと。

    まだ合わない式は calls-pending.txt (受け付けた式) と calls-rejected-pending.txt (拒んだ式) に並べてあり、そこに無い式は
    すべて合うこと。一覧にあるのに合うようになった式があれば、それも失敗にする。
.PARAMETER Arch
    x86 (既定) か x64。
.PARAMETER Rejected
    コンパイラが拒んだ式も確かめる。
.PARAMETER Match
    この正規表現に合う式だけを確かめる (直している途中の確認用)。
.PARAMETER UpdatePending
    一覧を、いまの結果で作り直す (合わない式を調べ終えて、一覧に載せると決めたときに使う)。
#>
[CmdletBinding()]
param(
    [ValidateSet('x86', 'x64')]
    [string]$Arch = 'x86',
    [switch]$Rejected,
    [string]$Match = '',
    [switch]$UpdatePending
)
if ($Match -and $UpdatePending) { throw '-Match と -UpdatePending は、一緒には使えません (一覧が、一部の式だけになってしまう)。' }

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

$exe = Join-Path $root "dist\$Arch\netdbg.exe"
$app = Join-Path $root "target\apps\$Arch\Calls\Calls.exe"
$source = Join-Path $root 'tests\apps\Calls\Program.cs'
$calls = Join-Path $root 'target\calls'
$runner = Join-Path $root 'target\exprgen\bin\exprgen.exe'
foreach ($needed in $exe, $app, $runner, (Join-Path $calls 'texts.txt')) {
    if (-not (Test-Path $needed)) { throw "$needed がありません。先に ./build.ps1 と ./tests/apps/build.ps1 を実行してください。" }
}

$arguments = @('run', $exe, $app, $source, $calls, $PSScriptRoot, $Arch, '--marker=@calls', '--suite=calls')
if ($Rejected) { $arguments += '--rejected' }
if ($UpdatePending) { $arguments += '--update-pending' }
if ($Match) { $arguments += "--match=$Match" }
& $runner @arguments
exit $LASTEXITCODE