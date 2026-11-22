# 段階 13: 専用の画面 (--ui)。エンジンの側 (画面の口、守り、DAP と SSE、同じ口の MCP、画面だけの口、ビット数での
# 任せ、終了) を HTTP で確かめ、ブラウザ (headless の Chrome か Edge) で出口の場面 (PDB の無い WinForms の exe を
# 開き、止め、ステップし、変数と画面の木を見る。AI の操作が履歴に出る) を通す。
# ブラウザの場面は、窓を開くので NETDBG_SKIP_UI_TESTS=1 で飛ばす。

function Invoke-UiHttp([string]$Url, [string]$Method = 'GET', [hashtable]$Headers = @{}, [string]$Body = $null, [string]$HostName = $null) {
    $request = [Net.HttpWebRequest]::Create($Url)
    $request.Method = $Method
    $request.Timeout = 60000
    if ($HostName) { $request.Host = $HostName }
    foreach ($name in $Headers.Keys) {
        if ($name -eq 'Content-Type') { $request.ContentType = $Headers[$name] } else { $request.Headers[$name] = $Headers[$name] }
    }
    if ($Body) {
        $bytes = [Text.Encoding]::UTF8.GetBytes($Body)
        if (-not $request.ContentType) { $request.ContentType = 'application/json' }
        $stream = $request.GetRequestStream(); $stream.Write($bytes, 0, $bytes.Length); $stream.Close()
    } elseif ($Method -eq 'POST') {
        $request.ContentLength = 0
    }
    try { $response = $request.GetResponse() } catch [Net.WebException] { $response = $_.Exception.Response }
    if (-not $response) { return @{ Status = 0; Type = ''; Text = ''; Length = 0 } }
    $reader = New-Object IO.StreamReader($response.GetResponseStream(), [Text.Encoding]::UTF8)
    $text = $reader.ReadToEnd(); $response.Close()
    return @{ Status = [int]$response.StatusCode; Type = $response.ContentType; Text = $text; Length = $text.Length }
}

# SSE の流れを開く。Wait-UiEvent で、条件に合うメッセージが来るまで読む。
function Open-UiEvents([string]$Url) {
    $request = [Net.HttpWebRequest]::Create($Url)
    $request.Timeout = 30000; $request.ReadWriteTimeout = 30000
    $response = $request.GetResponse()
    return @{ Response = $response; Reader = (New-Object IO.StreamReader($response.GetResponseStream(), [Text.Encoding]::UTF8)); Seen = New-Object System.Collections.ArrayList }
}

function Wait-UiEvent($Events, [scriptblock]$Match, [int]$TimeoutMs = 30000) {
    foreach ($m in $Events.Seen) { if (& $Match $m) { return $m } }
    $deadline = [DateTime]::Now.AddMilliseconds($TimeoutMs)
    while ([DateTime]::Now -lt $deadline) {
        try { $line = $Events.Reader.ReadLine() } catch { return $null }
        if ($null -eq $line) { return $null }
        if (-not $line.StartsWith('data: ')) { continue }
        $message = $line.Substring(6) | ConvertFrom-Json
        [void]$Events.Seen.Add($message)
        if (& $Match $message) { return $message }
    }
    return $null
}

# エンジンを --ui --no-open で起こし、標準出力の URL を読む。
function Start-UiEngine([string]$Exe, [string[]]$Arguments, [string]$Name) {
    $out = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-$Name.out"
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-$Name.log"
    Remove-Item $out -ErrorAction SilentlyContinue
    $all = @("--log=`"$log`"", '--ui', '--no-open') + ($Arguments | ForEach-Object { if ($_ -match '\s') { "`"$_`"" } else { $_ } })
    $process = Start-Process $Exe -ArgumentList $all -PassThru -RedirectStandardOutput $out -WindowStyle Hidden
    $url = $null
    for ($i = 0; $i -lt 100 -and -not $url; $i++) {
        Start-Sleep -Milliseconds 100
        $match = Get-Content $out -ErrorAction SilentlyContinue | Select-String 'UI: (http://\S+)'
        if ($match) { $url = $match.Matches[0].Groups[1].Value }
    }
    $token = ([regex]::Match("$url", 'token=(\w+)')).Groups[1].Value
    $base = ([regex]::Match("$url", '^(http://[^/]+)')).Groups[1].Value
    return @{ Process = $process; Url = $url; Token = $token; Base = $base }
}

function Stop-UiEngine($Engine) {
    if (-not $Engine.Process.HasExited) {
        [void](Invoke-UiHttp "$($Engine.Base)/quit" 'POST' @{ Authorization = "Bearer $($Engine.Token)" })
        if (-not $Engine.Process.WaitForExit(15000)) { Stop-Process -Id $Engine.Process.Id -Force -ErrorAction SilentlyContinue }
    }
}

function Test-Ui([string]$ArchName, [string]$Exe) {
    $app = Get-AppPath $ArchName
    if (-not (Test-Path $app)) { return }
    $engine = Start-UiEngine $Exe @($app) "http-$($script:flavor.Dir)-$ArchName"
    $events = $null
    try {
        Check $ArchName '画面: --ui で口を開き、トークンつきの URL を出す' ($engine.Url -match '^http://127\.0\.0\.1:\d+/\?token=\w{32}$') "($($engine.Url))"
        $file = Join-Path $env:LOCALAPPDATA "netdbg\mcp\$($engine.Process.Id).json"
        $endpoint = Get-Content $file -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
        Check $ArchName '画面: MCP の接続先のファイルに、画面の URL と同じ口の MCP' ($endpoint.ui -eq $engine.Url -and $endpoint.url -eq "$($engine.Base)/mcp") "($file)"

        $bearer = @{ Authorization = "Bearer $($engine.Token)" }
        $page = Invoke-UiHttp $engine.Url
        $wasm = Invoke-UiHttp "$($engine.Base)/ui_bg.wasm"
        Check $ArchName '画面: ページと wasm を返す' ($page.Status -eq 200 -and $page.Type -like 'text/html*' -and $page.Text -match '/ui\.js' -and $wasm.Status -eq 200 -and $wasm.Type -eq 'application/wasm') "($($page.Status) $($page.Type) $($wasm.Status) $($wasm.Type))"
        $noToken = Invoke-UiHttp "$($engine.Base)/"
        $noBearer = Invoke-UiHttp "$($engine.Base)/dap" 'POST' @{} '{"command":"threads"}'
        $otherOrigin = Invoke-UiHttp "$($engine.Base)/dap" 'POST' ($bearer + @{ Origin = 'http://evil.example' }) '{"command":"threads"}'
        $otherHost = Invoke-UiHttp "$($engine.Base)/dap" 'POST' $bearer '{"command":"threads"}' -HostName 'evil.example'
        Check $ArchName '画面: トークン無し、別の Origin、別の Host を断る' ($noToken.Status -eq 401 -and $noBearer.Status -eq 401 -and $otherOrigin.Status -eq 401 -and $otherHost.Status -eq 403) "($($noToken.Status) $($noBearer.Status) $($otherOrigin.Status) $($otherHost.Status))"

        $startup = (Invoke-UiHttp "$($engine.Base)/startup" 'GET' $bearer).Text | ConvertFrom-Json
        $again = (Invoke-UiHttp "$($engine.Base)/startup" 'GET' $bearer).Text | ConvertFrom-Json
        Check $ArchName '画面: 起動の引数のプログラムを 1 回だけ渡す' ($startup.startup.program -eq $app -and $null -eq $again.startup -and $startup.live -eq $false) "($($startup | ConvertTo-Json -Compress))"

        $events = Open-UiEvents "$($engine.Base)/events?token=$($engine.Token)"
        $dap = { param([string]$Command, $Arguments = @{}) (Invoke-UiHttp "$($engine.Base)/dap" 'POST' $bearer (@{ command = $Command; arguments = $Arguments } | ConvertTo-Json -Depth 10 -Compress)).Text | ConvertFrom-Json }
        $init = & $dap 'initialize' @{ adapterID = 'netdbg' }
        $launch = & $dap 'launch' @{ program = $app; stopAtEntry = $true }
        $done = & $dap 'configurationDone'
        $stopped = Wait-UiEvent $events { param($m) $m.event -eq 'stopped' }
        Check $ArchName '画面: POST /dap で起動し、SSE で止まった知らせが届く' ($init.success -and $init.request_seq -ge 1 -and $launch.success -and $done.success -and $stopped.body.reason -eq 'entry') "($($launch.message) $($stopped | ConvertTo-Json -Compress))"
        $process = Wait-UiEvent $events { param($m) $m.event -eq 'process' } 1000
        $sources = & $dap 'loadedSources'
        $programSource = Join-Path $root 'tests\apps\Console\Program.cs'
        Check $ArchName '画面: loadedSources で、止める前に開けるソースが分かる' ($init.body.supportsLoadedSourcesRequest -eq $true -and @($sources.body.sources | Where-Object { $_.path -eq $programSource }).Count -eq 1) "($($sources | ConvertTo-Json -Compress -Depth 5))"

        # 画面だけの口 (対象が止まっている間に。プロセスの一覧に、対象が出る)。
        $folder = Split-Path $app
        $list = (Invoke-UiHttp "$($engine.Base)/fs/list?path=$([Uri]::EscapeDataString($folder))" 'GET' $bearer).Text | ConvertFrom-Json
        $sjisFile = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-sjis.cs"
        # ANSI (システムのコード ページ) で書いたソースを、その文字のまま読めること。日本語の Windows なら Shift_JIS、
        # 英語の Windows (CI) なら Windows-1252 なので、そのコード ページで書ける文で確かめる。
        $ansiPage = [Globalization.CultureInfo]::CurrentCulture.TextInfo.ANSICodePage
        $ansiComment = if ($ansiPage -eq 932) { '// 日本語のコメント' } else { '// café naïve' }
        [IO.File]::WriteAllBytes($sjisFile, [Text.Encoding]::GetEncoding($ansiPage).GetBytes("$ansiComment`r`nclass A {}`r`n"))
        $read = (Invoke-UiHttp "$($engine.Base)/fs/read?path=$([Uri]::EscapeDataString($sjisFile))" 'GET' $bearer).Text | ConvertFrom-Json
        $processes = (Invoke-UiHttp "$($engine.Base)/sys/processes" 'GET' $bearer).Text | ConvertFrom-Json
        $target = @($processes.processes | Where-Object { $_.pid -eq $process.body.systemProcessId })
        Check $ArchName '画面: フォルダーの一覧、ANSI (日本語なら Shift_JIS) のソース、プロセスの一覧' (@($list.entries | Where-Object { $_.name -eq (Split-Path $app -Leaf) -and $_.kind -eq 'exe' }).Count -eq 1 -and $read.text.StartsWith($ansiComment) -and $read.encoding -eq 'ansi' -and $target.Count -eq 1 -and $target[0].managed -eq $true -and $target[0].bits -eq $ArchName) "($($read | ConvertTo-Json -Compress) $($target | ConvertTo-Json -Compress))"

        # 同じ口の MCP で AI が続行すると、画面に、続行と AI の操作が届く。
        $call = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'debug_continue'; arguments = @{} } } | ConvertTo-Json -Depth 5 -Compress
        $mcp = Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer $call
        $continued = Wait-UiEvent $events { param($m) $m.event -eq 'continued' }
        $started = Wait-UiEvent $events { param($m) $m.event -eq 'netdbg.agentAction' -and $m.body.state -eq 'started' }
        $finished = Wait-UiEvent $events { param($m) $m.event -eq 'netdbg.agentAction' -and $m.body.state -eq 'finished' }
        Check $ArchName '画面: AI (MCP) の操作が、出来事と履歴 (agentAction) で画面に届く' ($mcp.Status -eq 200 -and $continued -and $started.body.tool -eq 'debug_continue' -and $finished.body.id -eq $started.body.id) "($($mcp.Status) $($mcp.Text))"

        # 対象が終わってもエンジンは次を待ち、もう一度起動できる。
        $ended = Wait-UiEvent $events { param($m) $m.event -eq 'terminated' }
        $second = & $dap 'initialize' @{ adapterID = 'netdbg' }
        Check $ArchName '画面: 対象が終わってもエンジンは残り、次のセッションを始められる' ($ended -and -not $engine.Process.HasExited -and $second.success)

        # 止めずに読む記録 (U3): 例外の記録を入れて起動すると、捕まえた例外も記録の出来事で届く。
        [void](& $dap 'launch' @{ program = $app; args = @('throw'); exceptionLog = $true })
        [void](& $dap 'configurationDone')
        $caught = Wait-UiEvent $events { param($m) $m.event -eq 'netdbg.exceptionThrown' -and $m.body.typeName -eq 'System.FormatException' }
        Check $ArchName '画面: 例外の記録 (捕まえたものも) が、出来事で届く' ($caught -and $caught.body.seq -gt 0 -and $caught.body.snapshot) "($($caught | ConvertTo-Json -Compress -Depth 3))"
        [void](& $dap 'disconnect' @{ terminateDebuggee = $true })
    } finally {
        if ($events) { $events.Response.Close() }
        Stop-UiEngine $engine
    }
    Check $ArchName '画面: 終了でエンジンが終わり、接続先のファイルを消す' ($engine.Process.HasExited -and -not (Test-Path $file))

    # ビット数の違う対象は、隣のビット数のエンジンに任せる (dist の x86 と x64)。
    $other = if ($ArchName -eq 'x86') { 'x64' } else { 'x86' }
    $otherApp = $app -replace "\\$ArchName\\", "\$other\"
    $otherEngine = Join-Path (Split-Path (Split-Path $Exe)) "$other\netdbg.exe"
    if ((Test-Path $otherApp) -and (Test-Path $otherEngine)) {
        $handed = Start-UiEngine $Exe @($otherApp) "handover-$ArchName"
        try {
            $owner = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'netdbg\mcp') -Filter '*.json' | ForEach-Object { Get-Content $_.FullName -Raw | ConvertFrom-Json } | Where-Object { $_.ui -eq $handed.Url }
            $bits = if ($other -eq 'x64') { 64 } else { 32 }
            Check $ArchName "画面: $other の対象は $other のエンジンに任せる" ($handed.Url -and $owner.bits -eq $bits -and $owner.enginePid -ne $handed.Process.Id) "($($handed.Url) $($owner | ConvertTo-Json -Compress))"
        } finally {
            Stop-UiEngine $handed
        }
    }

    if ($env:NETDBG_SKIP_UI_TESTS -ne '1') {
        Test-UiBrowser $ArchName $Exe
        Test-UiCoverage $ArchName $Exe
        Test-UiExceptions $ArchName $Exe
    }
}

# ブラウザでカバレッジ (B1): 開く画面で「カバレッジを取る」を選んで PDB のある WinForms の exe を起動し、ソースの行に
# 通った / 通っていない / 分岐が一部の印が付く。AI がボタンを押すと、その処理の行の印が、動いている間に変わる。
function Test-UiCoverage([string]$ArchName, [string]$Exe) {
    Import-Module (Join-Path $PSScriptRoot '..\Cdp.psm1') -Force
    $program = Join-Path $root "target\apps\$ArchName\Gui48\SmokeGui48.exe"
    if (-not (Test-Path $program)) { return }
    $source = Join-Path $root 'tests\apps\Gui\Program.cs'
    $greet = Get-MarkerLine $source 'greet'
    $engine = Start-UiEngine $Exe @() "coverage-$ArchName"
    $browser = $null
    $js = { param([string]$Expression) Invoke-BrowserScript $browser $Expression }
    $press = { param([string]$Label) [void](& $js "[...document.querySelectorAll('button')].find(b => b.innerText.startsWith('$Label')).click()") }
    $mark = { param([int]$Line) "(() => { const l = [...document.querySelectorAll('.line')].find(l => l.querySelector('.gutter').innerText.trim() == '$Line'); return l && l.querySelector('.cov') ? l.querySelector('.cov').className + '|' + l.querySelector('.cov').title : ''; })()" }
    try {
        $browser = Start-Browser $engine.Url
        [void](Wait-Browser $browser "!!document.querySelector('.toolbar')")
        & $press '開く'
        [void](Wait-Browser $browser "!!document.querySelector('.dialog')")
        $escaped = $program.Replace('\', '\\')
        [void](& $js "(() => { const i = document.querySelectorAll('.dialog label input')[0]; i.value = '$escaped'; i.dispatchEvent(new Event('input', { bubbles: true })); const a = document.querySelectorAll('.dialog label input')[1]; a.value = '--close-after=300'; a.dispatchEvent(new Event('input', { bubbles: true })); const c = document.querySelector('.coverage-option'); if (!c.checked) c.click(); })()")
        [void](Wait-Browser $browser "document.querySelector('.coverage-option').checked && !document.querySelector('.dialog button.primary').disabled")
        # 「起動」は、開く画面のタブの「起動する」と前の文字が同じなので、ボタンを直接押す。
        [void](& $js "document.querySelector('.dialog button.primary').click()")
        Check $ArchName '画面 (カバレッジ): 開く画面で選んで起動すると、上の帯に率が出る' (Wait-Browser $browser "!!document.querySelector('.coverage-summary') && /行 \d+%/.test(document.querySelector('.coverage-summary').innerText)" 30000) "($(& $js "document.querySelector('.status').innerText + ' | ' + [...document.querySelectorAll('.output .out')].map(e => e.innerText).slice(-5).join(' / ')"))"
        [void](Wait-Browser $browser "[...document.querySelectorAll('.sources li')].some(li => li.innerText.includes('Program.cs'))" 30000)
        [void](& $js "[...document.querySelectorAll('.sources li')].find(li => li.innerText.includes('Program.cs')).click()")
        [void](Wait-Browser $browser "document.querySelector('.source-title .path') && document.querySelector('.source-title .path').innerText.endsWith('Program.cs')")
        # 押していないのに ◐ になったときは、同じ位置に窓を開くほかの試験 (別のセッションのスモーク) の実入力を疑う。
        # 見分けるため、詳細に次の行の印と、そのとき動いている試験アプリを出す。
        Check $ArchName '画面 (カバレッジ): Main の行は ●、まだ押していないボタンの処理の行は ○' ((Wait-Browser $browser "/cov-hit/.test($(& $mark 12))" 15000) -and (Wait-Browser $browser "/cov-miss/.test($(& $mark $greet))")) "($(& $js (& $mark 12)) / $(& $js (& $mark $greet)) / 次の行 $(& $js (& $mark ($greet + 1))) / 動いている試験アプリ: $((Get-Process SmokeGui* -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Id) $($_.Path)" }) -join ', '))"
        $call = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_invoke'; arguments = @{ name = 'greetButton'; waitMs = 1000 } } } | ConvertTo-Json -Depth 5 -Compress
        for ($try = 0; $try -lt 20; $try++) {
            $answer = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $call).Text | ConvertFrom-Json
            if (-not $answer.result.isError) { break }
            Start-Sleep -Milliseconds 500
        }
        # agreeBox は外れたままなので、`agreeBox.Checked ? "!" : ""` は偽の側だけ通る (呼び出しの戻り値で分かれる形)。
        $changed = Wait-Browser $browser "/cov-part/.test($(& $mark $greet)) && /偽の側だけ/.test($(& $mark $greet))" 15000
        Check $ArchName '画面 (カバレッジ): AI がボタンを押すと、動いている間に印が ◐ に変わり、片方だけ通った分岐を出す' $changed "($(& $js (& $mark $greet)))"
        Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-coverage-$ArchName.png")
        & $press 'XML を書く'
        Check $ArchName '画面 (カバレッジ): XML を書くと、書いた先が出力に出る' (Wait-Browser $browser "[...document.querySelectorAll('.output .out')].some(e => /カバレッジを書きました: .*\.cobertura\.xml/.test(e.innerText))" 15000)

        # 成績書のタブ (B3): 番号と項目を入れて始めると、AI の操作が記録中の一覧に増え、OK で終えると書いた先が出る。
        & $press '成績書'
        [void](Wait-Browser $browser "!!document.querySelector('.report-id-input')")
        [void](& $js "(() => { const set = (e, v) => { e.value = v; e.dispatchEvent(new Event('input', { bubbles: true })); }; set(document.querySelector('.report-id-input'), '2-1'); set(document.querySelector('.report .bar .grow'), '画面から挨拶する'); })()")
        [void](& $js "document.querySelector('.report-start').click()")
        $recording = Wait-Browser $browser "/記録中: 2-1/.test(document.querySelector('.report-status').innerText) && !document.querySelector('.report-ok').disabled" 15000
        # 書く先は、画面の「保存先」に入れた試験の一時フォルダー (その下に <プログラム名>-<日時> ができる)。終えるときに渡す。
        $reportWork = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-report-$ArchName"
        if (Test-Path $reportWork) { Remove-Item -Recurse -Force $reportWork }
        [void](& $js "(() => { const e = document.querySelector('.report .output-folder'); e.value = $(ConvertTo-Json $reportWork); e.dispatchEvent(new Event('change', { bubbles: true })); })()")
        $set = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_set_text'; arguments = @{ name = 'nameBox'; value = 'Hanako'; waitMs = 300 } } } | ConvertTo-Json -Depth 5 -Compress
        [void](Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $set)
        $operation = Wait-Browser $browser "[...document.querySelectorAll('.report-operations li')].some(li => /AI/.test(li.innerText) && /ui_set_text/.test(li.innerText))" 15000
        Check $ArchName '画面 (成績書): 始めると記録中になり、AI の操作がその場で一覧に増える' ($recording -and $operation) "($(& $js "document.querySelector('.report') ? document.querySelector('.report').innerText : 'no tab'"))"
        [void](& $js "document.querySelector('.report-ok').click()")
        $ended = Wait-Browser $browser "/成績書: .*成績書\.html/.test(document.querySelector('.report-status').innerText) && [...document.querySelectorAll('.report-list tr')].some(tr => /2-1/.test(tr.innerText) && tr.querySelector('.badge.report-ok'))" 30000
        Check $ArchName '画面 (成績書): OK で終えると、一覧に OK が付き、書いた成績書のパスが出る' ($ended -and @(Get-ChildItem $reportWork -Filter '成績書.xlsx' -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.Directory.Name -match '^SmokeGui48-\d{8}-\d{6}$' }).Count -eq 1) "($(& $js "document.querySelector('.report') ? document.querySelector('.report').innerText : 'no tab'"))"
        # 手順を流す (D-086): 書いた .steps.json が欄に入る。値を変えてから流すと、記録した値に戻る。
        $filled = Wait-Browser $browser "/\.steps\.json$/.test(document.querySelector('.replay-file').value)"
        $bearer = @{ Authorization = "Bearer $($engine.Token)" }
        $change = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_set_text'; arguments = @{ name = 'nameBox'; value = 'changed'; waitMs = 300 } } } | ConvertTo-Json -Depth 5 -Compress
        [void](Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer $change)
        [void](& $js "document.querySelector('.replay').click()")
        $replayed = Wait-Browser $browser "/1 手のうち 1 手を流しました/.test(document.querySelector('.report-status').innerText)" 30000
        $find = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_find'; arguments = @{ name = 'nameBox' } } } | ConvertTo-Json -Depth 5 -Compress
        $nameBox = "$(((Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer $find).Text | ConvertFrom-Json).result.content[0].text)"
        Check $ArchName '画面 (成績書): 書いた手順が欄に入り、「流す」で記録した値に戻る' ($filled -and $replayed -and $nameBox -match 'Hanako') "($(& $js "document.querySelector('.replay-file').value + ' | ' + document.querySelector('.report-status').innerText") / $nameBox)"
        Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-report-$ArchName.png")

        # PDB の無い exe (Test-UiBrowser が作ったもの): 逆コンパイルしたソースの上で、同じように印が付いて変わる。
        $plain = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-gui-$ArchName\bin\SmokeGui48.exe"
        if (Test-Path $plain) {
            & $press '終わらせる'
            [void](Wait-Browser $browser "document.querySelector('.status').innerText.startsWith('終了')" 15000)
            & $press '開く'
            [void](Wait-Browser $browser "!!document.querySelector('.dialog')")
            $escaped = $plain.Replace('\', '\\')
            [void](& $js "(() => { const i = document.querySelectorAll('.dialog label input')[0]; i.value = '$escaped'; i.dispatchEvent(new Event('input', { bubbles: true })); const a = document.querySelectorAll('.dialog label input')[1]; a.value = '--close-after=300'; a.dispatchEvent(new Event('input', { bubbles: true })); })()")
            [void](Wait-Browser $browser "document.querySelector('.coverage-option').checked && !document.querySelector('.dialog button.primary').disabled")
            [void](& $js "document.querySelector('.dialog button.primary').click()")
            $opened = Wait-Browser $browser "[...document.querySelectorAll('.sources li')].some(li => li.title.includes('decompiled') && li.innerText.includes('Program.cs'))" 30000
            if ($opened) { [void](& $js "[...document.querySelectorAll('.sources li')].find(li => li.title.includes('decompiled') && li.innerText.includes('Program.cs')).click()") }
            [void](Wait-Browser $browser "document.querySelector('.source-title .path') && /decompiled.*Program\.cs$/i.test(document.querySelector('.source-title .path').innerText)")
            $decompiled = [IO.File]::ReadAllLines((& $js "document.querySelector('.source-title .path').innerText"))
            $line = 1 + [Array]::FindIndex($decompiled, [Predicate[string]] { param($l) $l.Contains('"Hello, "') -and $l.Contains('?') })
            [void](& $js "(() => { const s = document.querySelector('.source'); s.scrollTop = ($line - 10) * 18; s.dispatchEvent(new Event('scroll')); })()")
            $before = Wait-Browser $browser "/cov-miss/.test($(& $mark $line))"
            for ($try = 0; $try -lt 20; $try++) {
                $answer = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $call).Text | ConvertFrom-Json
                if (-not $answer.result.isError) { break }
                Start-Sleep -Milliseconds 500
            }
            $after = Wait-Browser $browser "/cov-part/.test($(& $mark $line)) && /偽の側だけ/.test($(& $mark $line))" 15000
            Check $ArchName '画面 (カバレッジ): PDB の無い exe も、逆コンパイルしたソースの行に印が付き、押すと ◐ に変わる' ($opened -and $line -gt 0 -and $before -and $after) "(line ${line}: $(& $js (& $mark $line)))"
            Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-coverage-decompiled-$ArchName.png")
        }

        # 差分のタブ (B2): Test-Diff が作った後の版を、カバレッジつきで走らせ、前の版と比べる。変えた Adjust と足した Label が、
        # どちらも通っていない (再試験の範囲) と出る。
        $diffWork = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-diff-net10.0-$ArchName"
        if (Test-Path (Join-Path $diffWork 'V2\bin\SmokeDiff.exe')) {
            if (& $js "!!document.querySelector('.status') && !document.querySelector('.status').innerText.startsWith('終了')") {
                & $press '終わらせる'
                [void](Wait-Browser $browser "document.querySelector('.status').innerText.startsWith('終了')" 15000)
            }
            & $press '開く'
            [void](Wait-Browser $browser "!!document.querySelector('.dialog')")
            $escaped = (Join-Path $diffWork 'V2\bin\SmokeDiff.exe').Replace('\', '\\')
            [void](& $js "(() => { const i = document.querySelectorAll('.dialog label input')[0]; i.value = '$escaped'; i.dispatchEvent(new Event('input', { bubbles: true })); const a = document.querySelectorAll('.dialog label input')[1]; a.value = ''; a.dispatchEvent(new Event('input', { bubbles: true })); })()")
            [void](Wait-Browser $browser "document.querySelector('.coverage-option').checked && !document.querySelector('.dialog button.primary').disabled")
            [void](& $js "document.querySelector('.dialog button.primary').click()")
            [void](Wait-Browser $browser "document.querySelector('.status').innerText.startsWith('終了')" 30000)
            & $press '差分'
            $old = (Join-Path $diffWork 'V1\bin\SmokeDiff.exe').Replace('\', '\\')
            [void](Wait-Browser $browser "!!document.querySelector('.diff-old')")
            [void](& $js "(() => { const i = document.querySelector('.diff-old'); i.value = '$old'; i.dispatchEvent(new Event('input', { bubbles: true })); })()")
            & $press '比べる'
            $listed = Wait-Browser $browser "document.querySelectorAll('.diff-list tr').length == 2 && /再試験の範囲 2/.test(document.querySelector('.diff .hint').innerText) && [...document.querySelectorAll('.diff-list tr')].every(tr => tr.classList.contains('retest') && tr.querySelector('.diff-mark').innerText == '○')" 30000
            $methods = & $js "[...document.querySelectorAll('.diff-list .method')].map(e => e.innerText).join(', ')"
            Check $ArchName '画面 (差分): 前の版と比べ、変えたものと足したものに再試験の印 (○) が付く' ($listed -and $methods -match 'Program\.Adjust\(int\)' -and $methods -match 'Program\.Label\(\)') "($methods / $(& $js "document.querySelector('.diff .hint') ? document.querySelector('.diff .hint').innerText : 'no hint'"))"
            Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-diff-$ArchName.png")
        }
        & $press '終了'
        [void]($engine.Process.WaitForExit(15000))
    } catch {
        Check $ArchName '画面 (カバレッジ): 例外なしで終わる' $false $_.Exception.Message
    } finally {
        Stop-Browser $browser
        Stop-UiEngine $engine
        Get-Process SmokeGui48 -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $program } | Stop-Process -Force
    }
}

# ブラウザで例外の設定 (D-085): 窓で型を選んでおくと、捕まえられる例外でも、投げられた所で止まる。設定はブラウザに覚える。
function Test-UiExceptions([string]$ArchName, [string]$Exe) {
    Import-Module (Join-Path $PSScriptRoot '..\Cdp.psm1') -Force
    $program = Get-AppPath $ArchName
    if (-not (Test-Path $program)) { return }
    $engine = Start-UiEngine $Exe @() "exceptions-$ArchName"
    $browser = $null
    $js = { param([string]$Expression) Invoke-BrowserScript $browser $Expression }
    try {
        $browser = Start-Browser $engine.Url
        [void](Wait-Browser $browser "!!document.querySelector('.exceptions-button')")
        [void](& $js "document.querySelector('.exceptions-button').click()")
        [void](Wait-Browser $browser "!!document.querySelector('.dialog.exceptions')")
        # 閉じた <details> の中は innerText が空なので、textContent で探す。
        [void](& $js "[...document.querySelectorAll('.exception-type')].find(l => l.textContent.trim() == 'FormatException').querySelector('input').click()")
        $remembered = Wait-Browser $browser "(localStorage.getItem('netdbg.exceptionSettings') || '').includes('System.FormatException') && document.querySelector('.exceptions-button').classList.contains('active')"
        Check $ArchName '画面 (例外): 窓で型を選ぶと、ブラウザに覚え、ツールバーに印が付く' $remembered "($(& $js "localStorage.getItem('netdbg.exceptionSettings')"))"
        [void](& $js "document.querySelector('.dialog.exceptions .tabs button').click()")
        [void](& $js "[...document.querySelectorAll('.toolbar button')].find(b => b.innerText.startsWith('開く')).click()")
        [void](Wait-Browser $browser "!!document.querySelector('.dialog label input')")
        $escaped = $program.Replace('\', '\\')
        [void](& $js "(() => { const i = document.querySelectorAll('.dialog label input'); i[0].value = '$escaped'; i[0].dispatchEvent(new Event('input', { bubbles: true })); i[1].value = 'throw'; i[1].dispatchEvent(new Event('input', { bubbles: true })); })()")
        [void](Wait-Browser $browser "!document.querySelector('.dialog button.primary').disabled")
        [void](& $js "document.querySelector('.dialog button.primary').click()")
        $stopped = Wait-Browser $browser "/\(exception\)/.test(document.querySelector('.status').innerText) && /System\.FormatException/.test(document.querySelector('.status').innerText)" 30000
        Check $ArchName '画面 (例外): 選んだ型は、捕まえられる例外でも投げられた所で止まる' $stopped "($(& $js "document.querySelector('.status').innerText"))"
        [void](& $js "[...document.querySelectorAll('.toolbar button')].find(b => b.innerText.startsWith('終了')).click()")
        [void]($engine.Process.WaitForExit(15000))
    } catch {
        Check $ArchName '画面 (例外): 例外なしで終わる' $false $_.Exception.Message
    } finally {
        Stop-Browser $browser
        Stop-UiEngine $engine
    }
}
# ブラウザで出口の場面: PDB の無い WinForms の exe (.NET Framework) を開き、止める前にソースの一覧から逆コンパイルした
# ソースを開いてブレークポイントを置き、AI の操作で止まる、ステップ、変数、履歴、画面の木と写し、一時停止、終了。
function Test-UiBrowser([string]$ArchName, [string]$Exe) {
    Import-Module (Join-Path $PSScriptRoot '..\Cdp.psm1') -Force
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-gui-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $root 'tests\apps\Gui48') -c Release -p:DebugType=none "-p:PlatformTarget=$ArchName" "-p:BaseIntermediateOutputPath=$work\obj\" -o "$work\bin" -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    $program = Join-Path $work 'bin\SmokeGui48.exe'
    if ($LASTEXITCODE -ne 0 -or (Test-Path (Join-Path $work 'bin\SmokeGui48.pdb'))) {
        Check $ArchName '画面 (ブラウザ): PDB の無い WinForms の対象をビルドできる' $false "(exit code $LASTEXITCODE)"
        return
    }
    # アタッチして止め、切り離すと、対象は動き続ける。止めずに読む入口 (段階 11、12) のブレークポイントも外してから
    # 切り離す (残っていると切り離しに失敗し、対象は止まったまま残っていた)。
    $target = Start-Process $program -ArgumentList '--close-after=120' -PassThru
    Start-Sleep -Seconds 2
    $attached = Start-UiEngine $Exe @() "detach-$($script:flavor.Dir)-$ArchName"
    try {
        $bearer = @{ Authorization = "Bearer $($attached.Token)" }
        $dap = { param([string]$Command, $Arguments = @{}) (Invoke-UiHttp "$($attached.Base)/dap" 'POST' $bearer (@{ command = $Command; arguments = $Arguments } | ConvertTo-Json -Depth 10 -Compress)).Text | ConvertFrom-Json }
        [void](& $dap 'initialize' @{ adapterID = 'netdbg' })
        $attach = & $dap 'attach' @{ processId = $target.Id }
        [void](& $dap 'configurationDone')
        Start-Sleep -Seconds 1
        [void](& $dap 'pause' @{ threadId = 0 })
        Start-Sleep -Milliseconds 500
        $detach = & $dap 'disconnect' @{ terminateDebuggee = $false }
        Start-Sleep -Seconds 1
        $target.Refresh()
        $responding = (-not $target.HasExited) -and $target.Responding
    } finally {
        Stop-UiEngine $attached
    }
    Start-Sleep -Milliseconds 500
    $target.Refresh()
    Check $ArchName '画面: アタッチして止めたものを切り離すと、対象は応答し続け、エンジンを終えても残る' ($attach.success -and $detach.success -and $responding -and -not $target.HasExited -and $target.Responding) "($($attach.message) $($detach.message))"
    if (-not $target.HasExited) { $target.Kill() }

    $engine = Start-UiEngine $Exe @($program, '--close-after=300') "browser-$($script:flavor.Dir)-$ArchName"
    $browser = $null
    $js = { param([string]$Expression) Invoke-BrowserScript $browser $Expression }
    $press = { param([string]$Label) [void](& $js "[...document.querySelectorAll('button')].find(b => b.innerText.startsWith('$Label')).click()") }
    $status = { param([string]$Prefix) "document.querySelector('.status') && document.querySelector('.status').innerText.startsWith('$Prefix')" }
    try {
        $browser = Start-Browser $engine.Url
        Check $ArchName '画面 (ブラウザ): 起動の引数の exe を始める' (Wait-Browser $browser (& $status '実行中') 30000)
        # 止める前に、ソースの一覧から逆コンパイルして開く (PDB の無い exe)。
        $listed = Wait-Browser $browser "[...document.querySelectorAll('.sources li.pending')].some(li => li.innerText.includes('SmokeGui48.exe'))"
        & $press '逆コンパイルして開く'
        $decompiled = Wait-Browser $browser "document.querySelector('.source-title .path') && /decompiled/i.test(document.querySelector('.source-title .path').innerText) && [...document.querySelectorAll('.sources li')].some(li => li.innerText.includes('Program.cs'))"
        Check $ArchName '画面 (ブラウザ): 動いている間に、ソースの一覧から逆コンパイルした C# を開く' ($listed -and $decompiled)
        $path = & $js "document.querySelector('.source-title .path').innerText"
        $lines = [IO.File]::ReadAllLines($path)
        # もとのソースの 1 つの式の行 (逆コンパイラが一時変数で文に分けると止まれなかった。分かれ道で配列を持ち回す形。C1)。
        $line = 1 + [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l.Contains('"Hello, "') -and $l.Contains('?') })
        # その行で宣言する変数の名前 (ラムダの中の変数は、外の変数と重ならない名前になる。text、text2 など)。
        $textName = if ($line -gt 0 -and $lines[$line - 1] -match 'string (\w+) = ') { $Matches[1] } else { 'text' }
        [void](& $js "(() => { const s = document.querySelector('.source'); s.scrollTop = ($line - 10) * 18; s.dispatchEvent(new Event('scroll')); })()")
        [void](Wait-Browser $browser "[...document.querySelectorAll('.line .gutter')].some(g => g.innerText.trim() == '$line')")
        [void](& $js "[...document.querySelectorAll('.line .gutter')].find(g => g.innerText.trim() == '$line').click()")
        Check $ArchName '画面 (ブラウザ): 行の番号を押してブレークポイントを置く' (Wait-Browser $browser "[...document.querySelectorAll('.line')].some(l => l.innerText.trim().startsWith('$line') && l.querySelector('.bp:not(.unverified)'))")
        & $press '続行'
        [void](Wait-Browser $browser (& $status '実行中'))
        $call = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_invoke'; arguments = @{ name = 'greetButton'; waitMs = 5000 } } } | ConvertTo-Json -Depth 5 -Compress
        # 起動の直後は、まだ窓ができていないことがある。押せるまで繰り返す。
        for ($try = 0; $try -lt 20; $try++) {
            $answer = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $call).Text | ConvertFrom-Json
            if (-not $answer.result.isError) { break }
            Start-Sleep -Milliseconds 500
        }
        $hit = Wait-Browser $browser "document.querySelector('.status').innerText.includes('breakpoint') && document.querySelector('.line.current') && document.querySelector('.line.current .gutter').innerText.trim() == '$line'"
        Check $ArchName '画面 (ブラウザ): AI がボタンを押すと、もとの 1 つの式の行で止まる' ($hit -and $lines[$line - 1].Contains("string $textName = "))
        # 1 行進めて、text に値が入ってから、通った行の値と値の窓を確かめる。
        & $press 'ステップ オーバー'
        Check $ArchName '画面 (ブラウザ): ステップで次の行へ進む' (Wait-Browser $browser "document.querySelector('.line.current') && document.querySelector('.line.current .gutter').innerText.trim() != '$line'")
        # 止まったメソッドの、止まった行までの行に、変数の値を // で添える (Visual Studio のインライン値)。
        Check $ArchName '画面 (ブラウザ): 通った行に、変数の値を // で出す' (Wait-Browser $browser "[...document.querySelectorAll('.line .inline')].some(s => /\b$textName = .*Hello/.test(s.innerText))") "(名前 $textName / $(& $js "[...document.querySelectorAll('.line .inline')].map(s => s.innerText).join(' | ').slice(0, 300)") / いまの行 $(& $js "document.querySelector('.line.current') ? document.querySelector('.line.current').innerText.slice(0, 120) : 'none'"))"
        # 名前にマウスを載せると、その値の窓が出る (データヒント)。
        $moved = Move-BrowserMouse $browser '.line .name' $textName
        Check $ArchName '画面 (ブラウザ): 名前にマウスを載せると、値の窓が出る' ($moved -and (Wait-Browser $browser "document.querySelector('.hover') && /Hello/.test(document.querySelector('.hover').innerText)")) "($(& $js "document.querySelector('.hover') ? document.querySelector('.hover').innerText : 'no popup'"))"
        # マウスを外すと窓は消える (次の行に重なった窓が、次の名前を隠さないように、消えてから載せる)。
        [void](Send-Cdp $browser 'Input.dispatchMouseEvent' @{ type = 'mouseMoved'; x = 5; y = 5 })
        Check $ArchName '画面 (ブラウザ): マウスを外すと、値の窓が消える' (Wait-Browser $browser "!document.querySelector('.hover')" 5000)
        $movedObject = Move-BrowserMouse $browser '.line.current .name' 'resultLabel'
        Check $ArchName '画面 (ブラウザ): オブジェクトの値の窓は、中身を 1 段開いて見せる' ($movedObject -and (Wait-Browser $browser "document.querySelector('.hover') && document.querySelectorAll('.hover li').length > 2 && /resultLabel/.test(document.querySelector('.hover').innerText)"))
        Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-hover-$ArchName.png")
        & $press '変数'
        Check $ArchName '画面 (ブラウザ): 変数の木に値が出る' (Wait-Browser $browser "[...document.querySelectorAll('.tree li')].some(li => li.innerText.includes('Hello'))") "($(& $js "document.querySelector('.tree') ? document.querySelector('.tree').innerText.slice(0, 300) : 'no tree'"))"
        & $press 'AI の操作'
        Check $ArchName '画面 (ブラウザ): AI の操作の履歴に ui_invoke' (Wait-Browser $browser "[...document.querySelectorAll('.agent li')].some(li => li.innerText.includes('ui_invoke'))")
        & $press '続行'
        [void](Wait-Browser $browser (& $status '実行中'))
        & $press '画面'
        & $press '画面を読む'
        $tree = Wait-Browser $browser "[...document.querySelectorAll('.controls li')].some(li => li.innerText.includes('greetButton'))"
        [void](& $js "[...document.querySelectorAll('.controls li')].find(li => li.innerText.includes('greetButton')).click()")
        Check $ArchName '画面 (ブラウザ): 画面の木でコントロールを選び、写しの上に枠を描く' ($tree -and (Wait-Browser $browser "!!document.querySelector('.shot .frame') && document.querySelector('.shot img').naturalWidth > 0"))
        # 記録 (U3): AI が About を押すと、出したダイアログが記録のタブに出て、詳細に呼び出し元が出る。
        $about = @{ jsonrpc = '2.0'; id = 2; method = 'tools/call'; params = @{ name = 'ui_invoke'; arguments = @{ name = 'aboutButton'; waitMs = 1000 } } } | ConvertTo-Json -Depth 5 -Compress
        [void](Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $about)
        & $press '記録'
        $dialog = Wait-Browser $browser "[...document.querySelectorAll('.record-list tr')].some(tr => tr.querySelector('.badge.dialog'))"
        [void](& $js "[...document.querySelectorAll('.record-list tr')].find(tr => tr.querySelector('.badge.dialog')).click()")
        Check $ArchName '画面 (ブラウザ): ダイアログの記録が出て、詳細に呼び出し元が出る' ($dialog -and (Wait-Browser $browser "document.querySelector('.record-detail') && /呼び出し元/.test(document.querySelector('.record-detail').innerText) && /ShowDialog/.test(document.querySelector('.record-detail').innerText)")) "($(& $js "document.querySelector('.record-detail') ? document.querySelector('.record-detail').innerText : 'no detail'"))"
        $close = @{ jsonrpc = '2.0'; id = 3; method = 'tools/call'; params = @{ name = 'ui_invoke'; arguments = @{ name = 'closeButton'; waitMs = 1000 } } } | ConvertTo-Json -Depth 5 -Compress
        [void](Invoke-UiHttp "$($engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($engine.Token)" } $close)
        & $press '使用量'
        Check $ArchName '画面 (ブラウザ): 動いている間の使用量が出る' (Wait-Browser $browser "document.querySelector('.usage-table') && /MB/.test(document.querySelector('.usage-table').innerText)" 10000)

        & $press '一時停止'
        & $press 'コール スタック'
        Check $ArchName '画面 (ブラウザ): 一時停止で止まり、スタックに Main' (Wait-Browser $browser "[...document.querySelectorAll('.stack li')].some(li => li.innerText.includes('Program.Main'))")
        # 停止のあいだの変化 (D-087): 前の停止 (ステップ) のあとで、ハンドラーが書いた結果の欄が、画面のタブに出る。
        & $press '画面'
        $changed = Wait-Browser $browser "!!document.querySelector('.tree-changes') && /回目の停止/.test(document.querySelector('.tree-changes').innerText) && [...document.querySelectorAll('.tree-changes li')].some(li => li.innerText.includes('resultLabel'))"
        Check $ArchName '画面 (ブラウザ): 止まると、前の停止から変わったコントロールが画面のタブに出る' $changed "($(& $js "document.querySelector('.tree-changes') ? document.querySelector('.tree-changes').innerText : 'no block'"))"
        & $press '使用量'
        & $press 'ヒープの統計'
        Check $ArchName '画面 (ブラウザ): 止まっている間のヒープの統計に型が並ぶ' (Wait-Browser $browser "document.querySelectorAll('.heap tr').length > 5 && /string/.test(document.querySelector('.heap').innerText)" 30000)
        & $press '変数'
        [void](Wait-Browser $browser "!!document.querySelector('.tree li button.mini:not(.watch-writes)')")
        [void](& $js "document.querySelector('.tree li button.mini:not(.watch-writes)').click()")
        Check $ArchName '画面 (ブラウザ): 変数の「経路」で、保持している経路の窓が出る' (Wait-Browser $browser "!!document.querySelector('.dialog.small') && /GC/.test(document.querySelector('.dialog.small').innerText)" 30000) "($(& $js "document.querySelector('.dialog.small') ? document.querySelector('.dialog.small').innerText : 'no panel'"))"
        # 書き込みの見張り (D-093): オブジェクトを開くと、フィールドの行に「書き込みで止める」。.NET Framework の対象なので、理由の帯が出る。
        [void](& $js "document.querySelector('.tree li button.mini:not(.watch-writes)').closest('li').click()")
        $writes = Wait-Browser $browser "!!document.querySelector('.tree li button.watch-writes')" 15000
        if ($writes) { [void](& $js "document.querySelector('.tree li button.watch-writes').click()") }
        Check $ArchName '画面 (ブラウザ): フィールドの「書き込みで止める」は、.NET Framework では理由が出る' ($writes -and (Wait-Browser $browser "!!document.querySelector('.error') && /\.NET Framework/.test(document.querySelector('.error').innerText)" 15000)) "($(& $js "document.querySelector('.error') ? document.querySelector('.error').innerText : (document.querySelector('.tree') ? document.querySelector('.tree').innerText.slice(0, 300) : 'no tree')"))"
        [void](& $js "document.querySelector('.error .close') && document.querySelector('.error .close').click()")
        Save-BrowserScreenshot $browser (Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-ui-records-$ArchName.png")
        & $press '終了'
        Check $ArchName '画面 (ブラウザ): 終了でエンジンが終わる' ($engine.Process.WaitForExit(15000))
    } finally {
        Stop-Browser $browser
        Stop-UiEngine $engine
        Get-Process SmokeGui48 -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$work*" } | Stop-Process -Force
    }
}

# 表示の言語 (D-084): 英語の環境では、頁の言語と画面の文言が英語になる (訳の表の無い言語も英語)。
function Test-UiEnglish([string]$ArchName, [string]$Exe) {
    if ($env:NETDBG_SKIP_UI_TESTS -eq '1') { return }
    Import-Module (Join-Path $PSScriptRoot '..\Cdp.psm1') -Force
    $saved = $env:NETDBG_LANG
    $env:NETDBG_LANG = 'fr-FR'
    try {
        $engine = Start-UiEngine $Exe @() "english-$ArchName"
    } finally {
        $env:NETDBG_LANG = $saved
    }
    $browser = $null
    try {
        $page = Invoke-UiHttp $engine.Url
        $browser = Start-Browser $engine.Url
        $english = Wait-Browser $browser "document.documentElement.lang == 'en' && [...document.querySelectorAll('button')].some(b => b.innerText.trim() == 'Open') && /Nothing is open/.test(document.body.innerText)"
        Check $ArchName '画面 (言語): 訳の表の無い言語 (fr) の環境では、頁と文言が英語になる' ($page.Text.Contains('<html lang="en">') -and $english) "($(if ($browser) { Invoke-BrowserScript $browser 'document.body.innerText.slice(0, 200)' }))"
    } catch {
        Check $ArchName '画面 (言語): 例外なしで終わる' $false $_.Exception.Message
    } finally {
        if ($browser) { Stop-Browser $browser }
        Stop-UiEngine $engine
    }
}

# 表示の言語 (D-099): 中国語 (簡体字と繁体字) と韓国語の環境では、頁の言語がその表の札になり、文言がその表の訳に、
# 漢字とハングルのフォントがその言語のものになる。中国語は地域でも分ける (zh-TW は繁体字)。期待する文言は表から読む。
function Test-UiLanguages([string]$ArchName, [string]$Exe) {
    if ($env:NETDBG_SKIP_UI_TESTS -eq '1') { return }
    Import-Module (Join-Path $PSScriptRoot '..\Cdp.psm1') -Force
    foreach ($case in @(@('zh-CN', 'zh-Hans', 'YaHei'), @('zh-TW', 'zh-Hant', 'JhengHei'), @('ko-KR', 'ko', 'Malgun'))) {
        $environment, $tag, $font = $case
        $catalog = [IO.File]::ReadAllText((Join-Path $root "i18n\$tag.json"), [Text.Encoding]::UTF8)
        $open = ([regex]::Match($catalog, '(?m)^\s*"Open": "([^"]+)"')).Groups[1].Value
        $saved = $env:NETDBG_LANG
        $env:NETDBG_LANG = $environment
        try {
            $engine = Start-UiEngine $Exe @() "lang-$tag-$ArchName"
        } finally {
            $env:NETDBG_LANG = $saved
        }
        $browser = $null
        try {
            $page = Invoke-UiHttp $engine.Url
            $browser = Start-Browser $engine.Url
            $shown = Wait-Browser $browser "document.documentElement.lang == '$tag' && [...document.querySelectorAll('button')].some(b => b.innerText.trim() == '$open') && /$font/.test(getComputedStyle(document.documentElement).getPropertyValue('--cjk'))"
            Check $ArchName "画面 (言語): $environment の環境では、頁が $tag、文言がその訳 ($open)、漢字のフォントが $font になる" ($open -and $page.Text.Contains("<html lang=`"$tag`">") -and $shown) "($(if ($browser) { Invoke-BrowserScript $browser "document.documentElement.lang + ' | ' + getComputedStyle(document.documentElement).getPropertyValue('--cjk') + ' | ' + document.body.innerText.slice(0, 120)" }))"
        } catch {
            Check $ArchName "画面 (言語 $tag): 例外なしで終わる" $false $_.Exception.Message
        } finally {
            if ($browser) { Stop-Browser $browser }
            Stop-UiEngine $engine
        }
    }
}