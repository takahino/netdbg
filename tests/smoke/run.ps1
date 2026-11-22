<#
.SYNOPSIS
    dist\ の exe に DAP を流すスモーク試験。先に build.ps1 を実行しておく。
.DESCRIPTION
    段階が進むごとに項目を足す。出口の判定は x86。x64 が落ちても x86 が通っていれば終了コードは 0 だが、
    結果には FAIL と出る（docs/STATUS.md に記録して段階 7 で直す）。
    Test-McpGui は窓を開き、実際のマウスとキーの入力を送る。作業中に回すときは、環境変数
    NETDBG_SKIP_UI_TESTS=1 で飛ばす。
.PARAMETER Arch
    試すビット数。既定は両方。
.PARAMETER Only
    一つの場面だけを回す (cases\Test-<名前>.ps1 の名前。直している途中の確認用)。
.PARAMETER ConformanceOnly
    式の適合試験 (Test-Conformance と、F# / VB の Test-LanguageConformance) だけを回す。
#>
[CmdletBinding()]
param(
    [ValidateSet('x86', 'x64')]
    [string[]]$Arch = @('x86', 'x64'),
    # 式の適合試験だけを回す (式の評価を直しているときの、速い確認用)。
    [switch]$ConformanceOnly,
    [string]$Only = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
# 表示の言語 (D-084): 確かめる文は日本語で書いてあるので、日本語にそろえる (OS の言語によらない)。英語は、場面ごとに
# 環境変数を英語にして確かめる (Test-Ui の英語の頁)。
$env:NETDBG_LANG = 'ja'
Import-Module (Join-Path $PSScriptRoot 'Dap.psm1') -Force

$script:failures = @{ x86 = 0; x64 = 0 }

# 同じ試験を、.NET と .NET Framework の対象で回す。違うのは、対象のファイル、アプリ本体のモジュール名、
# エンジンが読み込んではいけないランタイム、エンジンが使う部品。
$flavors = @(
    @{ Label = ''; Dir = 'Console'; Exe = 'SmokeConsole.exe'; Process = 'SmokeConsole'; Module = 'SmokeConsole.dll'; Runtime = 'coreclr.dll'; Shim = 'dbgshim.dll' },
    @{ Label = 'net48: '; Dir = 'Console48'; Exe = 'SmokeConsole48.exe'; Process = 'SmokeConsole48'; Module = 'SmokeConsole48.exe'; Runtime = 'clr.dll'; Shim = 'mscoree.dll' }
)
# .NET 11 の対象は、.NET 11 の SDK でビルドしてあるときだけ (tests/apps/build.ps1 が、SDK が無ければ飛ばす)。
if (Test-Path (Join-Path $root 'target\apps\x86\Console11\SmokeConsole11.exe')) {
    $flavors += @{ Label = 'net11: '; Dir = 'Console11'; Exe = 'SmokeConsole11.exe'; Process = 'SmokeConsole11'; Module = 'SmokeConsole11.dll'; Runtime = 'coreclr.dll'; Shim = 'dbgshim.dll' }
} else {
    Write-Host '(.NET 11 の対象はビルドされていないので、試しません)' -ForegroundColor Yellow
}
$script:flavor = $flavors[0]

function Get-AppPath([string]$ArchName) {
    return Join-Path $root "target\apps\$ArchName\$($script:flavor.Dir)\$($script:flavor.Exe)"
}

function Check([string]$ArchName, [string]$What, [bool]$Ok, [string]$Detail = '') {
    $What = "$($script:flavor.Label)$What"
    if ($Ok) {
        Write-Host "  ok    $What"
    } else {
        Write-Host "  FAIL  $What $Detail" -ForegroundColor Red
        $script:failures[$ArchName]++
    }
}

# 目印 `// @名前` (VB では `' @名前`) のある行番号 (1 始まり)。`@work` が `@work-open` に当たらないよう、名前の終わりまで見る。
function Get-MarkerLine([string]$File, [string]$Marker) {
    # Windows PowerShell 5.1 は BOM の無い UTF-8 を ANSI として読み、行がずれる。
    $lines = Get-Content $File -Encoding UTF8
    for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i] -match "(//|') @$Marker(?![\w-])") { return $i + 1 } }
    throw "目印 @$Marker が $File にありません。"
}

# 多くの場面に共通の始まり: 対象を起動し、目印の行にブレークポイントを置いて、そこで止まるまで進める。
# 戻り値は、止まったときの `stopped` の通知 (届かなければ $null)。途中の応答を確かめる場面は、これを使わずに自分で送る。
function Start-AtMarker($Session, [string]$Program, [string]$Source, [string]$Marker, [string[]]$ProgramArgs) {
    [void](Send-DapRequest $Session 'initialize' @{ adapterID = 'netdbg' })
    $launch = @{ program = $Program }
    if ($ProgramArgs) { $launch.args = @($ProgramArgs) }
    [void](Send-DapRequest $Session 'launch' $launch)
    [void](Send-DapRequest $Session 'setBreakpoints' @{ source = @{ path = $Source }; breakpoints = @(@{ line = (Get-MarkerLine $Source $Marker) }) })
    [void](Send-DapRequest $Session 'configurationDone')
    return Wait-DapEvent $Session 'stopped' -TimeoutMs 30000
}

# 試験は、場面ごとに cases\ の下の 1 ファイルにしてある。ここで読み込む (このスクリプトの変数と関数を、そのまま使う)。
foreach ($case in Get-ChildItem (Join-Path $PSScriptRoot 'cases') -Filter *.ps1) { . $case.FullName }

foreach ($a in $Arch) {
    $exe = Join-Path $root "dist\$a\netdbg.exe"
    Write-Host "== $a ($exe)" -ForegroundColor Cyan
    if (-not (Test-Path $exe)) {
        Check $a 'exe がある' $false '(build.ps1 を先に実行してください)'
        continue
    }
    if ($Only) {
        # 対象の種類 (.NET、.NET Framework、.NET 11) ごとに回す。種類を見ない場面は、同じことを繰り返すだけ。
        foreach ($f in $flavors) {
            $script:flavor = $f
            & "Test-$Only" $a $exe
        }
        $script:flavor = $flavors[0]
        continue
    }
    if ($ConformanceOnly) {
        foreach ($f in $flavors) {
            $script:flavor = $f
            Test-Conformance $a $exe
        }
        $script:flavor = $flavors[0]
        Test-LanguageConformance $a $exe
        continue
    }
    Test-Skeleton $a $exe
    foreach ($f in $flavors) {
        $script:flavor = $f
        Test-Launch $a $exe
        Test-OutputText $a $exe
        Test-Values $a $exe
        Test-Step $a $exe
        Test-StepFiltering $a $exe
        Test-AsyncStep $a $exe
        Test-Exceptions $a $exe
        Test-ExceptionDetails $a $exe
        Test-Conditions $a $exe
        Test-BreakpointLines $a $exe
        Test-EvalDeadlock $a $exe
        Test-SetVariable $a $exe
        Test-NoImplicitEval $a $exe
        Test-Conformance $a $exe
        Test-Pointers $a $exe
        Test-Inspect $a $exe
        Test-FunctionBreakpoints $a $exe
        Test-EntryPauseTerminate $a $exe
        Test-Attach $a $exe
        Test-AttachObserveOnly $a $exe
        Test-AttachMismatch $a $exe
        Test-DisconnectWhileStopped $a $exe
        Test-BitnessMismatch $a $exe
        Test-Mcp $a $exe
        Test-McpListen $a $exe
        Test-McpGui $a $exe
        Test-Enc $a $exe
        Test-Decompile $a $exe
        Test-Records $a $exe
        Test-BreakpointRules $a $exe
        Test-UiFreeze $a $exe
        Test-BindingLog $a $exe
        Test-RetentionPath $a $exe
        Test-DialogLog $a $exe
        Test-NativeBoundary $a $exe
        Test-AssemblyLoad $a $exe
        Test-ComLeak $a $exe
        Test-DataBreakpoint $a $exe
        Test-Transactions $a $exe
        Test-AsyncStack $a $exe
        Test-ChildProcess $a $exe
        Test-Dump $a $exe
        Test-Coverage $a $exe
        Test-Diff $a $exe
    }
    $script:flavor = $flavors[0]
    Test-Ui $a $exe
    Test-UiEnglish $a $exe
    Test-UiLanguages $a $exe
    Test-Report $a $exe
    Test-SelfContained $a $exe
    Test-SingleFile $a $exe
    Test-LaunchDll $a $exe
    Test-AsyncMain $a $exe
    Test-NativeTarget $a $exe
    Test-FrameworkWithoutDotnet $a $exe
    Test-OldFramework $a $exe
    Test-WindowsPdb $a $exe
    Test-EmbeddedPdb $a $exe
    Test-Symbols $a $exe
    Test-Languages $a $exe
    Test-LanguageConformance $a $exe
}

Write-Host ''
Write-Host ("x86: {0} 件の失敗 / x64: {1} 件の失敗" -f $script:failures.x86, $script:failures.x64)
# 判定は x86。
if ($script:failures.x86 -gt 0) { exit 1 }
exit 0
