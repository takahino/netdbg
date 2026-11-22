# スモーク試験用の小さな DAP クライアント。エンジンの exe を起動し、標準入出力で要求と応答をやり取りする。
# Windows PowerShell 5.1 で動く書き方にしてある。

Set-StrictMode -Version Latest

function Start-DapSession {
    param(
        [Parameter(Mandatory)][string]$Exe,
        [string[]]$Arguments = @('--interpreter=vscode')
    )
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Exe
    $info.Arguments = ($Arguments -join ' ')
    $info.WorkingDirectory = Split-Path -Parent $Exe
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::Start($info)
    return @{
        Process = $process
        Out     = $process.StandardOutput.BaseStream
        In      = $process.StandardInput.BaseStream
        Seq     = 1
        Events  = New-Object System.Collections.ArrayList
        # 時間切れで受け取り損ねた読み取り (Read-DapBytes)。
        Pending = $null
    }
}

# 指定したバイト数を読む。時間内に来なければ例外。
function Read-DapBytes {
    param($Session, [int]$Count, [int]$TimeoutMs)
    $buffer = New-Object byte[] $Count
    $read = 0
    while ($read -lt $Count) {
        # 時間切れのとき、読み取りは取り消せずに残る。捨てると、あとで届いたバイトがそちらに読まれて失われ、
        # 次のメッセージの枠がずれる (「Content-Length がありません: ontent-Length」)。残しておき、次の呼び出しで受け取る。
        if ($null -eq $Session.Pending) {
            $chunk = New-Object byte[] ($Count - $read)
            $Session.Pending = @{ Task = $Session.Out.ReadAsync($chunk, 0, $chunk.Length); Buffer = $chunk; Offset = 0 }
        }
        $pending = $Session.Pending
        if (-not $pending.Task.Wait($TimeoutMs)) { throw "エンジンからの応答が ${TimeoutMs} ms 以内にありません。" }
        if ($pending.Task.Result -eq 0) { throw 'エンジンが標準出力を閉じました。' }
        # 前の呼び出しが残した読み取りは、いま要る分より多く読んでいることがある。余りは次の呼び出しに回す。
        $take = [Math]::Min($Count - $read, $pending.Task.Result - $pending.Offset)
        [Array]::Copy($pending.Buffer, $pending.Offset, $buffer, $read, $take)
        $read += $take
        $pending.Offset += $take
        if ($pending.Offset -ge $pending.Task.Result) { $Session.Pending = $null }
    }
    return , $buffer
}

function Read-DapMessage {
    param($Session, [int]$TimeoutMs = 10000)
    $header = New-Object System.Text.StringBuilder
    while (-not $header.ToString().EndsWith("`r`n`r`n")) {
        $b = Read-DapBytes $Session 1 $TimeoutMs
        [void]$header.Append([char]$b[0])
    }
    if ($header.ToString() -notmatch 'Content-Length:\s*(\d+)') { throw "Content-Length がありません: $header" }
    $body = Read-DapBytes $Session ([int]$Matches[1]) $TimeoutMs
    return [System.Text.Encoding]::UTF8.GetString($body) | ConvertFrom-Json
}

# 要求を送り、その応答を返す。途中で届いたイベントは $Session.Events に溜める。
function Send-DapRequest {
    param($Session, [Parameter(Mandatory)][string]$Command, [hashtable]$Arguments = @{}, [int]$TimeoutMs = 10000)
    $seq = $Session.Seq
    $Session.Seq = $seq + 1
    $json = @{ seq = $seq; type = 'request'; command = $Command; arguments = $Arguments } | ConvertTo-Json -Depth 20 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $head = [System.Text.Encoding]::ASCII.GetBytes("Content-Length: $($bytes.Length)`r`n`r`n")
    $Session.In.Write($head, 0, $head.Length)
    $Session.In.Write($bytes, 0, $bytes.Length)
    $Session.In.Flush()
    while ($true) {
        $message = Read-DapMessage $Session $TimeoutMs
        if ($message.type -eq 'response' -and $message.request_seq -eq $seq) { return $message }
        if ($message.type -eq 'event' -or $message.type -eq 'request') { [void]$Session.Events.Add($message) }
    }
}

# 要求を送るだけで、応答を待たない (cancel を試すときなど)。要求の番号を返す。応答は Receive-DapResponse で受け取る。
function Send-DapRequestOnly {
    param($Session, [Parameter(Mandatory)][string]$Command, [hashtable]$Arguments = @{})
    $seq = $Session.Seq
    $Session.Seq = $seq + 1
    $json = @{ seq = $seq; type = 'request'; command = $Command; arguments = $Arguments } | ConvertTo-Json -Depth 20 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $head = [System.Text.Encoding]::ASCII.GetBytes("Content-Length: $($bytes.Length)`r`n`r`n")
    $Session.In.Write($head, 0, $head.Length)
    $Session.In.Write($bytes, 0, $bytes.Length)
    $Session.In.Flush()
    return $seq
}

# Send-DapRequestOnly で送った要求の応答を待つ。ほかの要求の応答が先に届いたら、取っておく。
function Receive-DapResponse {
    param($Session, [Parameter(Mandatory)][int]$Seq, [int]$TimeoutMs = 10000)
    if (-not $Session.Contains('Responses')) { $Session.Responses = @{} }
    while ($true) {
        if ($Session.Responses.ContainsKey($Seq)) {
            $found = $Session.Responses[$Seq]
            $Session.Responses.Remove($Seq)
            return $found
        }
        $message = Read-DapMessage $Session $TimeoutMs
        if ($message.type -eq 'response') { $Session.Responses[[int]$message.request_seq] = $message }
        if ($message.type -eq 'event' -or $message.type -eq 'request') { [void]$Session.Events.Add($message) }
    }
}
# 条件に合うイベントを待つ。すでに届いていればそれを返す。
function Wait-DapEvent {
    param($Session, [Parameter(Mandatory)][string]$Name, [scriptblock]$Where = { $true }, [int]$TimeoutMs = 10000)
    while ($true) {
        for ($i = 0; $i -lt $Session.Events.Count; $i++) {
            if ($Session.Events[$i].event -eq $Name -and (& $Where $Session.Events[$i])) {
                $found = $Session.Events[$i]
                $Session.Events.RemoveAt($i)
                return $found
            }
        }
        $message = Read-DapMessage $Session $TimeoutMs
        if ($message.type -eq 'event' -or $message.type -eq 'request') { [void]$Session.Events.Add($message) }
    }
}

# 対象プロセスが読み込んでいるモジュール名。32bit のプロセスは 32bit の PowerShell からでないと見えないので、
# ビット数に合う powershell.exe に問い合わせる。
function Get-DapModuleNames {
    param($Session, [ValidateSet('x86', 'x64')][string]$Arch)
    $shell = if ($Arch -eq 'x86' -and [Environment]::Is64BitOperatingSystem) {
        Join-Path $env:windir 'SysWOW64\WindowsPowerShell\v1.0\powershell.exe'
    } else {
        Join-Path $env:windir 'System32\WindowsPowerShell\v1.0\powershell.exe'
    }
    $id = $Session.Process.Id
    $names = & $shell -NoProfile -NonInteractive -Command "(Get-Process -Id $id).Modules | ForEach-Object { `$_.ModuleName }"
    return @($names | ForEach-Object { $_.ToLowerInvariant() })
}

# エンジンの終了を待ち、終了コードを返す。時間内に終わらなければ止めて例外。
function Stop-DapSession {
    param($Session, [int]$TimeoutMs = 5000)
    $Session.In.Close()
    if (-not $Session.Process.WaitForExit($TimeoutMs)) {
        $Session.Process.Kill()
        throw "エンジンが ${TimeoutMs} ms 以内に終了しませんでした。"
    }
    return $Session.Process.ExitCode
}

# エンジンからの逆向きの要求 (startDebugging など) を待つ。届いたものは取り出す。
function Wait-DapReverseRequest {
    param($Session, [Parameter(Mandatory)][string]$Command, [int]$TimeoutMs = 10000)
    while ($true) {
        for ($i = 0; $i -lt $Session.Events.Count; $i++) {
            if ($Session.Events[$i].type -eq 'request' -and $Session.Events[$i].command -eq $Command) {
                $found = $Session.Events[$i]
                $Session.Events.RemoveAt($i)
                return $found
            }
        }
        $message = Read-DapMessage $Session $TimeoutMs
        if ($message.type -eq 'event' -or $message.type -eq 'request') { [void]$Session.Events.Add($message) }
    }
}
Export-ModuleMember -Function Wait-DapReverseRequest, Start-DapSession, Read-DapMessage, Send-DapRequest, Send-DapRequestOnly, Receive-DapResponse, Wait-DapEvent, Get-DapModuleNames, Stop-DapSession
