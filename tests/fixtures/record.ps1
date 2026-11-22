# 同じ場面 (SmokeConsole の @values で停止) を、指定したデバッガーに流して、表記を JSON に書き出す。
# netcoredbg と netdbg の出力を比べるために使う:
#   ./tests/fixtures/record.ps1 -Debugger ..\VSCodedotnetX86Debugger\debugger\netcoredbg-x86\netcoredbg.exe -Out tests\fixtures\values.netcoredbg.json
#   ./tests/fixtures/record.ps1 -Debugger dist\x86\netdbg.exe -Out target\values.ours.json
param(
    [Parameter(Mandatory)] [string]$Debugger,
    [Parameter(Mandatory)] [string]$Out
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Import-Module (Join-Path $root 'tests\smoke\Dap.psm1') -Force

$app = Join-Path $root 'target\apps\x86\Console\SmokeConsole.exe'
$source = Join-Path $root 'tests\apps\Console\Program.cs'
$lines = Get-Content $source -Encoding UTF8
$line = 0
for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i] -match '// @values\b') { $line = $i + 1 } }

function Read-Variables($s, [int]$reference, [int]$depth) {
    $result = @()
    foreach ($v in @((Send-DapRequest $s 'variables' @{ variablesReference = $reference }).body.variables)) {
        $entry = [ordered]@{ name = $v.name; value = $v.value; type = $v.type; expandable = ($v.variablesReference -gt 0) }
        if ($v.variablesReference -gt 0 -and $depth -gt 0) { $entry.children = @(Read-Variables $s $v.variablesReference ($depth - 1)) }
        $result += $entry
    }
    return $result
}

# .NET Framework の Process は、標準入力の先頭に BOM を書く (コンソールの入力が UTF-8 のとき)。netcoredbg はそれを読めないので、BOM の無い UTF-8 にしておく。
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
$s = Start-DapSession -Exe (Resolve-Path $Debugger).Path -Arguments @('--interpreter=vscode')
try {
    [void](Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; linesStartAt1 = $true; columnsStartAt1 = $true })
    [void](Send-DapRequest $s 'launch' @{ program = $app; cwd = (Split-Path -Parent $app) })
    [void](Send-DapRequest $s 'setBreakpoints' @{ source = @{ path = $source }; breakpoints = @(@{ line = $line }) })
    [void](Send-DapRequest $s 'configurationDone')
    $stop = Wait-DapEvent $s 'stopped' -TimeoutMs 60000
    $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $stop.body.threadId }).body.stackFrames)
    $scopes = @((Send-DapRequest $s 'scopes' @{ frameId = $frames[0].id }).body.scopes)
    $record = [ordered]@{
        stopped = [ordered]@{ reason = $stop.body.reason; allThreadsStopped = $stop.body.allThreadsStopped }
        threads = @((Send-DapRequest $s 'threads').body.threads | ForEach-Object { $_.name })
        frames  = @($frames | ForEach-Object { [ordered]@{ name = $_.name; line = $_.line; column = $_.column; endLine = $_.endLine; endColumn = $_.endColumn; source = $_.source.name } })
        scopes  = @($scopes | ForEach-Object { [ordered]@{ name = $_.name; expensive = $_.expensive } })
        locals  = @(Read-Variables $s $scopes[0].variablesReference 1)
    }
    $json = $record | ConvertTo-Json -Depth 12
    [IO.File]::WriteAllText((Join-Path (Get-Location) $Out), $json, [Text.UTF8Encoding]::new($false))
    [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true })
} finally {
    if (-not $s.Process.HasExited) { try { $s.Process.Kill() } catch {} }
}