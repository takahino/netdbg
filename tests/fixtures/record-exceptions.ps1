# 例外の場面 (SmokeConsole を throw で起動) を、指定したデバッガーに流して、表記を JSON に書き出す。使い方は record.ps1 と同じ。
# -Mode で、ほかの場面 (break: Debugger.Break()、inner: 内側の例外とタスクの中の例外) も記録できる。
param(
    [Parameter(Mandatory)] [string]$Debugger,
    [Parameter(Mandatory)] [string]$Out,
    [string[]]$Filters = @('all'),
    [string]$Mode = 'throw'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Import-Module (Join-Path $root 'tests\smoke\Dap.psm1') -Force
$app = Join-Path $root 'target\apps\x86\Console\SmokeConsole.exe'

# netcoredbg は標準入力の先頭の BOM を読めない (record.ps1 と同じ)。
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
$s = Start-DapSession -Exe (Resolve-Path $Debugger).Path -Arguments @('--interpreter=vscode')
try {
    $init = Send-DapRequest $s 'initialize' @{ adapterID = 'netdbg'; linesStartAt1 = $true; columnsStartAt1 = $true }
    [void](Send-DapRequest $s 'launch' @{ program = $app; cwd = (Split-Path -Parent $app); args = @($Mode) })
    $set = Send-DapRequest $s 'setExceptionBreakpoints' @{ filters = $Filters }
    [void](Send-DapRequest $s 'configurationDone')
    $stops = @()
    while ($true) {
        $next = $null
        while (-not $next) {
            for ($i = 0; $i -lt $s.Events.Count; $i++) { if ($s.Events[$i].event -in 'stopped', 'terminated', 'exited') { $next = $s.Events[$i]; $s.Events.RemoveAt($i); break } }
            if (-not $next) { $m = Read-DapMessage $s 60000; if ($m.type -eq 'event') { [void]$s.Events.Add($m) } }
        }
        if ($next.event -ne 'stopped') { break }
        $body = [ordered]@{}
        foreach ($property in $next.body.PSObject.Properties | Where-Object { $_.Name -ne 'threadId' }) { $body[$property.Name] = $property.Value }
        $info = Send-DapRequest $s 'exceptionInfo' @{ threadId = $next.body.threadId }
        $frames = @((Send-DapRequest $s 'stackTrace' @{ threadId = $next.body.threadId; levels = 20 }).body.stackFrames)
        $stops += [ordered]@{
            stopped       = $body
            exceptionInfo = if ($info.success) { $info.body } else { "失敗: $($info.message)" }
            frames        = @($frames | ForEach-Object { "$($_.name) $($_.source.name):$($_.line)" })
        }
        [void](Send-DapRequest $s 'continue' @{ threadId = $next.body.threadId })
    }
    $record = [ordered]@{
        mode                       = $Mode
        filters                    = $Filters
        exceptionBreakpointFilters = $init.body.exceptionBreakpointFilters
        capabilities               = [ordered]@{}
        setExceptionBreakpoints    = $set.body
        stops                      = $stops
    }
    foreach ($property in $init.body.PSObject.Properties | Where-Object { $_.Name -ne 'exceptionBreakpointFilters' } | Sort-Object Name) { $record.capabilities[$property.Name] = $property.Value }
    # 記録にこの機械のパスを残さない (スタックトレースにソースのフルパスが入る)。
    $json = ($record | ConvertTo-Json -Depth 12).Replace(($root -replace '\\', '\\'), '<repo>')
    [IO.File]::WriteAllText((Join-Path (Get-Location) $Out), $json, [Text.UTF8Encoding]::new($false))
    try { [void](Send-DapRequest $s 'disconnect' @{ terminateDebuggee = $true }) } catch {}
} finally {
    if (-not $s.Process.HasExited) { try { $s.Process.Kill() } catch {} }
}