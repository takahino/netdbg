<#
.SYNOPSIS
    headless の Chrome (または Edge) を DevTools Protocol で動かす、試験用の小さな道具 (専用の画面の試験。段階 13)。
.DESCRIPTION
    Start-Browser でページを開き、Invoke-BrowserScript でページの中の式を評価し (DOM を読む、要素を押す)、
    Stop-Browser で閉じる。画面そのものは Rust だけで書いてあり、ここの式は試験の道具の側のもの。
#>

$ErrorActionPreference = 'Stop'

function Find-Browser {
    $candidates = @(
        "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
        "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
        "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
        "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe"
    )
    return $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}

function Start-Browser([string]$Url, [int]$Width = 1400, [int]$Height = 900) {
    $exe = Find-Browser
    if (-not $exe) { throw 'Chrome も Edge も見つかりません。' }
    # 開くたびに別のプロファイルにする (同じプロファイルで続けて開くと、閉じ終わっていない前のブラウザに渡されて、
    # DevTools の口が開かないことがある)。
    $profile = Join-Path ([IO.Path]::GetTempPath()) "netdbg-cdp-$PID-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $process = Start-Process $exe -ArgumentList @('--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', "--window-size=$Width,$Height", "--user-data-dir=$profile", "--remote-debugging-port=$port", 'about:blank') -PassThru -WindowStyle Hidden
    $page = $null
    for ($i = 0; $i -lt 100 -and -not $page; $i++) {
        Start-Sleep -Milliseconds 100
        # Windows PowerShell 5.1 の Invoke-RestMethod は、JSON の配列を 1 つのものとして流すので、いったん変数に受ける。
        try { $targets = Invoke-RestMethod "http://127.0.0.1:$port/json/list"; $page = $targets | Where-Object { $_.type -eq 'page' } | Select-Object -First 1 } catch {}
    }
    if (-not $page) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue; throw 'ブラウザの DevTools につながりません。' }
    $socket = [Net.WebSockets.ClientWebSocket]::new()
    $socket.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
    $browser = [pscustomobject]@{ Process = $process; Socket = $socket; Profile = $profile; Id = 0 }
    Send-Cdp $browser 'Page.navigate' @{ url = $Url } | Out-Null
    return $browser
}

function Send-Cdp($Browser, [string]$Method, $Params = @{}) {
    if ($Browser.Socket.State -ne 'Open') { throw "DevTools の接続が閉じています ($($Browser.Socket.State))。" }
    $Browser.Id++
    $json = @{ id = $Browser.Id; method = $Method; params = $Params } | ConvertTo-Json -Depth 20 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($json)
    $Browser.Socket.SendAsync([ArraySegment[byte]]$bytes, 'Text', $true, [Threading.CancellationToken]::None).Wait()
    $buffer = New-Object byte[] 65536
    while ($true) {
        $stream = New-Object IO.MemoryStream
        do {
            $received = $Browser.Socket.ReceiveAsync([ArraySegment[byte]]$buffer, [Threading.CancellationToken]::None).Result
            $stream.Write($buffer, 0, $received.Count)
        } while (-not $received.EndOfMessage)
        $message = [Text.Encoding]::UTF8.GetString($stream.ToArray()) | ConvertFrom-Json
        if ($message.id -eq $Browser.Id) { return $message }
    }
}

# ページの中で式を評価して、値を返す (Promise は待つ)。
function Invoke-BrowserScript($Browser, [string]$Expression) {
    $answer = Send-Cdp $Browser 'Runtime.evaluate' @{ expression = $Expression; returnByValue = $true; awaitPromise = $true }
    if ($answer.result.exceptionDetails) { throw "ページの式が失敗しました: $Expression : $($answer.result.exceptionDetails.exception.description)" }
    return $answer.result.result.value
}

# 式が真になるまで待つ。真になれば $true。
function Wait-Browser($Browser, [string]$Condition, [int]$TimeoutMs = 15000) {
    $deadline = [DateTime]::Now.AddMilliseconds($TimeoutMs)
    while ([DateTime]::Now -lt $deadline) {
        if (Invoke-BrowserScript $Browser "!!($Condition)") { return $true }
        Start-Sleep -Milliseconds 150
    }
    return $false
}

# ページの中の、見えている要素 (CSS のセレクターと、innerText の一致) の真ん中にマウスを動かす (本物の入力として。mouseenter が届く)。
function Move-BrowserMouse($Browser, [string]$Selector, [string]$Text) {
    $center = Invoke-BrowserScript $Browser "(() => { const e = [...document.querySelectorAll('$Selector')].find(e => { const r = e.getBoundingClientRect(); return e.innerText.trim() == '$Text' && r.top >= 0 && r.bottom <= innerHeight; }); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()"
    if (-not $center) { return $false }
    Send-Cdp $Browser 'Input.dispatchMouseEvent' @{ type = 'mouseMoved'; x = 5; y = 5 } | Out-Null
    Send-Cdp $Browser 'Input.dispatchMouseEvent' @{ type = 'mouseMoved'; x = $center.x; y = $center.y } | Out-Null
    return $true
}

# 画面を PNG に写す。
function Save-BrowserScreenshot($Browser, [string]$Path) {
    $answer = Send-Cdp $Browser 'Page.captureScreenshot' @{ format = 'png' }
    [IO.File]::WriteAllBytes($Path, [Convert]::FromBase64String($answer.result.data))
}

function Stop-Browser($Browser) {
    if (-not $Browser) { return }
    try { $Browser.Socket.Dispose() } catch {}
    Stop-Process -Id $Browser.Process.Id -Force -ErrorAction SilentlyContinue
    # headless の Chrome は子のプロセスを残すことがあるので、同じプロファイルのものを止める。
    Get-CimInstance Win32_Process -Filter "Name='chrome.exe' OR Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*$($Browser.Profile)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
    Remove-Item -Recurse -Force $Browser.Profile -ErrorAction SilentlyContinue
}

Export-ModuleMember -Function Start-Browser, Send-Cdp, Invoke-BrowserScript, Wait-Browser, Move-BrowserMouse, Save-BrowserScreenshot, Stop-Browser