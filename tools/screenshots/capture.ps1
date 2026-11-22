<#
.SYNOPSIS
  README と guide の画像 (guide/images/<言語>/) を、見本のアプリ (samples/OrderDemo) を netdbg の画面 (--ui) で
  デバッグしながら撮る。

.DESCRIPTION
  見本のアプリを中立のフォルダー ($Demo) に写して組み立て、エンジンの LOCALAPPDATA と TEMP もそこへ向ける (画面に
  出るパスに、利用者の名前が入らないように)。ブラウザは見えない窓 (headless) で動かす。見本のアプリの窓は開くが、
  操作は MCP (/mcp) で行う。撮ったあとは、すべての画像を目で確かめること。

.PARAMETER Lang
  撮る言語 (en、ja、zh-Hans、zh-Hant、ko。既定は README と guide に載せる en と ja)。画面の言語は NETDBG_LANG で
  決まる。ボタンの文言は訳の表 (i18n/<言語>.json) から引く。

.PARAMETER Out
  画像を書くフォルダー (既定は guide\images)。その下の <言語> に書く。確かめのために別の場所へ撮るときに使う。

.PARAMETER Scene
  撮る場面 (既定はすべて): overview (overview.gif、breakpoint.png)、mcp (mcp-ui.png、mcp-session.png)、
  records (records.png、windows.png、usage.png)、decompile (decompile.png)、coverage (coverage.png、report.png)。

.PARAMETER Exe
  エンジン (既定は dist\x86\netdbg.exe)。
#>
param(
    [ValidateSet('en', 'ja', 'zh-Hans', 'zh-Hant', 'ko')][string[]]$Lang = @('en', 'ja'),
    [string]$Out = '',
    [string[]]$Scene = @('overview', 'mcp', 'records', 'decompile', 'coverage'),
    [string]$Demo = 'C:\netdbg-demo',
    [string]$Exe = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Exe) { $Exe = Join-Path $root 'dist\x86\netdbg.exe' }
Import-Module (Join-Path $root 'tests\smoke\Cdp.psm1') -Force
Add-Type -AssemblyName System.Drawing, PresentationCore, WindowsBase

# 画面の文言 (ボタンとタブ)。英語の文で書き、ほかの言語は訳の表 (i18n/<言語>.json) から引く。
$englishLabels = @{ Open = 'Open'; Continue = 'Continue (F5)'; StepOver = 'Step Over (F10)'; Variables = 'Variables'; Agent = 'AI Actions'; Windows = 'Windows'; ReadTree = 'Read the windows'; Records = 'Records'; Usage = 'Usage'; Heap = 'Heap statistics'; Report = 'Test Report'; Decompile = 'Decompile and open'; Pause = 'Pause'; Sources = 'Sources'; Running = 'Running'; Stopped = 'Stopped' }
function Get-Labels([string]$Language) {
    $labels = @{}
    $catalog = if ($Language -ne 'en') { [IO.File]::ReadAllText((Join-Path $root "i18n\$Language.json"), [Text.Encoding]::UTF8) }
    foreach ($key in $englishLabels.Keys) {
        $english = $englishLabels[$key]
        $labels[$key] = if ($catalog) { ([regex]::Match($catalog, '(?m)^\s*"' + [regex]::Escape($english) + '": "([^"]+)"')).Groups[1].Value } else { $english }
        if (-not $labels[$key]) { throw "i18n\$Language.json に「$english」の訳がありません。" }
    }
    return $labels
}

# AI の操作の例の、依頼の文 (例であることを書き添える)。
$requests = @{
    en = @('Three teas should be 1,140, but the total says 1,026. Find out why.', 'The request is an example; the tool calls and answers below were made for real through netdbg''s MCP server.')
    ja = @('お茶を 3 つ頼むと、合計が 1,140 ではなく 1,026 になる。理由を調べて。', '依頼の文は例です。下のツールの呼び出しと答えは、netdbg の MCP で実際に行ったものです。')
    'zh-Hans' = @('点 3 杯茶，合计应该是 1,140，却显示 1,026。请查明原因。', '请求文字只是示例；下面的工具调用和应答是通过 netdbg 的 MCP 实际执行的。')
    'zh-Hant' = @('點 3 杯茶，合計應該是 1,140，卻顯示 1,026。請查明原因。', '要求文字只是範例；下面的工具呼叫和回應是透過 netdbg 的 MCP 實際執行的。')
    ko = @('차 3잔이면 합계가 1,140이어야 하는데 1,026으로 나옵니다. 원인을 찾아 주세요.', '요청 문장은 예시입니다. 아래의 도구 호출과 응답은 netdbg의 MCP로 실제로 실행한 것입니다.')
}
function Invoke-Http([string]$Url, [string]$Method = 'GET', [hashtable]$Headers = @{}, [string]$Body = $null) {
    $request = [Net.HttpWebRequest]::Create($Url)
    $request.Method = $Method
    $request.Timeout = 120000
    foreach ($name in $Headers.Keys) { $request.Headers[$name] = $Headers[$name] }
    if ($Body) {
        $bytes = [Text.Encoding]::UTF8.GetBytes($Body)
        $request.ContentType = 'application/json'
        $stream = $request.GetRequestStream(); $stream.Write($bytes, 0, $bytes.Length); $stream.Close()
    } elseif ($Method -eq 'POST') {
        $request.ContentLength = 0
    }
    try { $response = $request.GetResponse() } catch [Net.WebException] { $response = $_.Exception.Response }
    $reader = New-Object IO.StreamReader($response.GetResponseStream(), [Text.Encoding]::UTF8)
    $text = $reader.ReadToEnd(); $response.Close()
    return $text
}

# 見本のアプリを $Demo に写して組み立てる (PDB あり と 無し)。
function Initialize-Demo {
    foreach ($dir in 'OrderDemo', 'localappdata', 'temp', 'reports') { New-Item -ItemType Directory -Force (Join-Path $Demo $dir) | Out-Null }
    Get-ChildItem (Join-Path $root 'samples\OrderDemo') -File | Copy-Item -Destination (Join-Path $Demo 'OrderDemo') -Force
    # 中間のフォルダーは、2 つの組み立てで分け、どちらもソースのフォルダーの外に置く (中に置くと、生成した
    # AssemblyInfo をもう一方が拾い、属性が重複する)。
    foreach ($dir in 'obj', 'obj-nopdb', 'app', 'app-nopdb', 'OrderDemo\obj', 'OrderDemo\bin') { Remove-Item -Recurse -Force (Join-Path $Demo $dir) -ErrorAction SilentlyContinue }
    $ErrorActionPreference = 'Continue'
    & dotnet build (Join-Path $Demo 'OrderDemo') -c Debug -o (Join-Path $Demo 'app') "-p:BaseIntermediateOutputPath=$Demo\obj\" -nologo -v q | Out-Null
    & dotnet build (Join-Path $Demo 'OrderDemo') -c Release -p:DebugType=none -o (Join-Path $Demo 'app-nopdb') "-p:BaseIntermediateOutputPath=$Demo\obj-nopdb\" -nologo -v q | Out-Null
    $ErrorActionPreference = 'Stop'
    $built = (Test-Path (Join-Path $Demo 'app\OrderDemo.pdb')) -and (Test-Path (Join-Path $Demo 'app-nopdb\OrderDemo.exe')) -and -not (Test-Path (Join-Path $Demo 'app-nopdb\OrderDemo.pdb'))
    if (-not $built) { throw "見本のアプリを組み立てられません。dotnet build $Demo\OrderDemo の出力を確かめてください。" }
}

# エンジンを --ui --no-open で起こす (LOCALAPPDATA と TEMP は $Demo の中)。
function Start-Engine([string]$Language, [string[]]$Arguments = @()) {
    $saved = @{ LOCALAPPDATA = $env:LOCALAPPDATA; TEMP = $env:TEMP; TMP = $env:TMP; NETDBG_LANG = $env:NETDBG_LANG }
    $env:LOCALAPPDATA = Join-Path $Demo 'localappdata'; $env:TEMP = Join-Path $Demo 'temp'; $env:TMP = $env:TEMP; $env:NETDBG_LANG = $Language
    try {
        $out = Join-Path $Demo "temp\engine-$Language.out"
        Remove-Item $out -ErrorAction SilentlyContinue
        $all = @('--ui', '--no-open') + ($Arguments | ForEach-Object { if ($_ -match '\s') { "`"$_`"" } else { $_ } })
        $process = Start-Process $Exe -ArgumentList $all -PassThru -RedirectStandardOutput $out -WindowStyle Hidden
    } finally {
        foreach ($k in $saved.Keys) { Set-Item "env:$k" $saved[$k] }
    }
    $url = $null
    for ($i = 0; $i -lt 100 -and -not $url; $i++) {
        Start-Sleep -Milliseconds 100
        $match = Get-Content $out -ErrorAction SilentlyContinue | Select-String 'UI: (http://\S+)'
        if ($match) { $url = $match.Matches[0].Groups[1].Value }
    }
    if (-not $url) { throw '画面の URL を読めません。' }
    return @{ Process = $process; Url = $url; Token = ([regex]::Match($url, 'token=(\w+)')).Groups[1].Value; Base = ([regex]::Match($url, '^(http://[^/]+)')).Groups[1].Value }
}

function Stop-Engine($Engine) {
    if ($Engine -and -not $Engine.Process.HasExited) {
        [void](Invoke-Http "$($Engine.Base)/quit" 'POST' @{ Authorization = "Bearer $($Engine.Token)" })
        if (-not $Engine.Process.WaitForExit(15000)) { Stop-Process -Id $Engine.Process.Id -Force -ErrorAction SilentlyContinue }
    }
    Get-Process OrderDemo -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Demo\*" } | Stop-Process -Force -ErrorAction SilentlyContinue
}

# MCP のツールを呼ぶ (画面と同じ口の /mcp)。答えの本文 (JSON なら読んだもの) と、失敗したか。
function Invoke-Mcp($Engine, [string]$Name, [hashtable]$Arguments = @{}) {
    $body = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = $Name; arguments = $Arguments } } | ConvertTo-Json -Depth 10 -Compress
    $answer = (Invoke-Http "$($Engine.Base)/mcp" 'POST' @{ Authorization = "Bearer $($Engine.Token)" } $body) | ConvertFrom-Json
    $text = @($answer.result.content | Where-Object { $_.type -eq 'text' })[0].text
    $value = try { $text | ConvertFrom-Json } catch { $text }
    return @{ Error = [bool]$answer.result.isError; Value = $value; Text = $text; Raw = $answer }
}

function Invoke-Js($Browser, [string]$Expression) { Invoke-BrowserScript $Browser $Expression }

# ボタンやタブを、文言の頭で押す。
function Press($Browser, [string]$Label) {
    [void](Wait-Browser $Browser "[...document.querySelectorAll('button')].some(b => b.innerText.trim().startsWith('$Label'))" 15000)
    [void](Invoke-Js $Browser "[...document.querySelectorAll('button')].find(b => b.innerText.trim().startsWith('$Label')).click()")
    Start-Sleep -Milliseconds 300
}

function Wait-Status($Browser, [string]$Prefix, [int]$TimeoutMs = 30000) {
    if (-not (Wait-Browser $Browser "document.querySelector('.status') && document.querySelector('.status').innerText.startsWith('$Prefix')" $TimeoutMs)) {
        Save-BrowserScreenshot $Browser (Join-Path $Demo 'temp\failed.png')
        throw "画面が「$Prefix」になりません (いまの画面: $Demo\temp\failed.png): $(Invoke-Js $Browser "document.querySelector('.status') ? document.querySelector('.status').innerText : ''")"
    }
}

# ソースの一覧からファイルを開き、文字を含む行の番号を返す。
function Open-Source($Browser, [string]$File, [string]$Contains) {
    [void](Wait-Browser $Browser "[...document.querySelectorAll('.sources li')].some(li => li.innerText.includes('$File'))" 30000)
    [void](Invoke-Js $Browser "[...document.querySelectorAll('.sources li')].find(li => li.innerText.includes('$File')).click()")
    [void](Wait-Browser $Browser "document.querySelector('.source-title .path') && document.querySelector('.source-title .path').innerText.endsWith('$File')" 30000)
    $path = Invoke-Js $Browser "document.querySelector('.source-title .path').innerText"
    $lines = [IO.File]::ReadAllLines($path)
    return @{ Path = $path; Line = 1 + [Array]::FindIndex($lines, [Predicate[string]] { param($l) $l.Contains($Contains) }) }
}

# 行の番号を押してブレークポイントを置く (外す)。行が見えるように流してから。
function Toggle-Breakpoint($Browser, [int]$Line) {
    [void](Invoke-Js $Browser "(() => { const s = document.querySelector('.source'); s.scrollTop = Math.max(0, ($Line - 12) * 18); s.dispatchEvent(new Event('scroll')); })()")
    [void](Wait-Browser $Browser "[...document.querySelectorAll('.line .gutter')].some(g => g.innerText.trim() == '$Line')")
    [void](Invoke-Js $Browser "[...document.querySelectorAll('.line .gutter')].find(g => g.innerText.trim() == '$Line').click()")
    Start-Sleep -Milliseconds 500
}

# 下の欄の式の行に式を書いて Enter (結果は「出力」に出る)。
function Invoke-Evaluate($Browser, [string]$Expression) {
    $text = ConvertTo-Json $Expression
    [void](Invoke-Js $Browser "(() => { const i = document.querySelector('input.expression'); i.value = $text; i.dispatchEvent(new Event('input', { bubbles: true })); i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); })()")
    Start-Sleep -Milliseconds 800
}

# 下の欄 (出力、記録など) の高さを変える。画面の既定は 260px で、記録や画面の木の写しには狭い。
function Set-BottomHeight($Browser, [int]$Pixels) {
    [void](Invoke-Js $Browser "document.querySelector('.layout').style.gridTemplateRows = 'minmax(0, 1fr) ${Pixels}px'")
    Start-Sleep -Milliseconds 300
}

function Save-Shot($Browser, [string]$Path) {
    Start-Sleep -Milliseconds 400
    Save-BrowserScreenshot $Browser $Path
    Write-Host "  $Path"
}

# PNG を並べて、こま送りの GIF にする (繰り返し、こまごとの間隔 (1/100 秒))。幅は $Width に縮める。
function New-Gif([string[]]$Frames, [int[]]$Delays, [string]$Path, [int]$Width = 1000) {
    $encoder = New-Object System.Windows.Media.Imaging.GifBitmapEncoder
    foreach ($frame in $Frames) {
        $image = [Drawing.Image]::FromFile($frame)
        $height = [int]($image.Height * $Width / $image.Width)
        $small = New-Object Drawing.Bitmap $Width, $height
        $g = [Drawing.Graphics]::FromImage($small); $g.InterpolationMode = 'HighQualityBicubic'; $g.DrawImage($image, 0, 0, $Width, $height); $g.Dispose(); $image.Dispose()
        $stream = New-Object IO.MemoryStream; $small.Save($stream, [Drawing.Imaging.ImageFormat]::Png); $small.Dispose(); $stream.Position = 0
        $encoder.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($stream, 'None', 'OnLoad'))
    }
    $buffer = New-Object IO.MemoryStream; $encoder.Save($buffer); $bytes = $buffer.ToArray()
    # こまごとの間隔 (Graphic Control Extension の 21 F9 04) を書き、最初のこまの前に繰り返しの印 (NETSCAPE2.0) を入れる。
    $list = New-Object System.Collections.Generic.List[byte]
    $index = 0; $first = $true
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        if ($i + 7 -lt $bytes.Length -and $bytes[$i] -eq 0x21 -and $bytes[$i + 1] -eq 0xF9 -and $bytes[$i + 2] -eq 0x04) {
            if ($first) { $list.AddRange([byte[]](@(0x21, 0xFF, 0x0B) + [Text.Encoding]::ASCII.GetBytes('NETSCAPE2.0') + @(0x03, 0x01, 0x00, 0x00, 0x00))); $first = $false }
            $delay = $Delays[[Math]::Min($index, $Delays.Count - 1)]
            $list.AddRange([byte[]]@(0x21, 0xF9, 0x04, $bytes[$i + 3], ($delay -band 0xFF), ($delay -shr 8), $bytes[$i + 6]))
            $i += 6; $index++
            continue
        }
        $list.Add($bytes[$i])
    }
    [IO.File]::WriteAllBytes($Path, $list.ToArray())
    Write-Host "  $Path ($index こま)"
}

# ---- 場面 ----

# 止める: ソースを開いてブレークポイント、AI (MCP) で注文を入れて止まり、ステップ。GIF と静止画。
function Capture-Overview([string]$L, [string]$Out) {
    $t = Get-Labels $L
    $engine = Start-Engine $L @((Join-Path $Demo 'app\OrderDemo.exe'))
    $browser = $null
    $frames = @()
    try {
        $browser = Start-Browser $engine.Url 1280 800
        [void](Wait-Status $browser $t.Running)
        [void](Invoke-Mcp $engine 'ui_wait_for' @{ name = 'addButton'; timeoutMs = 30000 })
        $source = Open-Source $browser 'Orders.cs' 'decimal subtotal = line.Subtotal;'
        $frames += Join-Path $Demo "temp\f1-$L.png"; Save-Shot $browser $frames[-1]
        Toggle-Breakpoint $browser $source.Line
        $frames += Join-Path $Demo "temp\f2-$L.png"; Save-Shot $browser $frames[-1]
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'customerBox'; value = 'Alice' })
        [void](Invoke-Mcp $engine 'ui_select' @{ name = 'productBox'; index = 1 })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '3' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 5000 })
        [void](Wait-Status $browser $t.Stopped)
        [void](Wait-Browser $browser "!!document.querySelector('.line.current')")
        $frames += Join-Path $Demo "temp\f3-$L.png"; Save-Shot $browser $frames[-1]
        foreach ($k in 1..3) {
            Press $browser $t.StepOver
            [void](Wait-Status $browser $t.Stopped)
            Start-Sleep -Milliseconds 600
            $frames += Join-Path $Demo "temp\f$(3 + $k)-$L.png"; Save-Shot $browser $frames[-1]
        }
        Press $browser $t.Variables
        Invoke-Evaluate $browser 'subtotal * DiscountRate'
        Invoke-Evaluate $browser 'Lines.Where(l => l.Quantity >= BulkQuantity).Select(l => l.Product.Name).ToArray()'
        Save-Shot $browser (Join-Path $Out 'breakpoint.png')
        $frames += Join-Path $Out 'breakpoint.png'
        New-Gif $frames @(150, 120, 200, 120, 120, 150, 400) (Join-Path $Out 'overview.gif')
        Toggle-Breakpoint $browser $source.Line
        Press $browser $t.Continue
    } finally {
        Stop-Browser $browser
        Stop-Engine $engine
    }
}

# AI が MCP で操作する: 実際の呼び出しと答えを記録し、画面の「AI の操作」と見本のアプリの窓を並べた画像と、
# やり取りを会話の形に並べた画像を作る。
function Capture-Mcp([string]$L, [string]$Out) {
    $t = Get-Labels $L
    $engine = Start-Engine $L @((Join-Path $Demo 'app\OrderDemo.exe'))
    $browser = $null
    $calls = New-Object System.Collections.ArrayList
    $call = {
        param([string]$Name, [hashtable]$Arguments, [scriptblock]$Summary)
        $r = Invoke-Mcp $engine $Name $Arguments
        [void]$calls.Add(@{ Name = $Name; Arguments = $Arguments; Answer = (& $Summary $r) })
        $r
    }
    try {
        $browser = Start-Browser $engine.Url 1280 800
        [void](Wait-Status $browser $t.Running)
        [void](Invoke-Mcp $engine 'ui_wait_for' @{ name = 'addButton'; timeoutMs = 30000 })
        $orders = Join-Path $Demo 'OrderDemo\Orders.cs'
        $line = 1 + [Array]::FindIndex([IO.File]::ReadAllLines($orders), [Predicate[string]] { param($l) $l.Contains('return total;') })
        [void](& $call 'breakpoint_set' @{ file = $orders; line = $line } { param($r) "set at Orders.cs:$line" })
        [void](& $call 'ui_set_text' @{ name = 'customerBox'; value = 'Bob' } { param($r) "$($r.Value.status.state)" })
        [void](& $call 'ui_select' @{ name = 'productBox'; item = 'Tea (380)' } { param($r) "$($r.Value.status.state)" })
        [void](& $call 'ui_set_text' @{ name = 'quantityBox'; value = '3' } { param($r) "$($r.Value.status.state)" })
        [void](& $call 'ui_invoke' @{ name = 'addButton'; waitMs = 5000 } { param($r) "stopped: $($r.Value.status.reason) at $($r.Value.status.at.function) line $($r.Value.status.at.line)" })
        [void](& $call 'debug_stop_context' @{ contextLines = 1 } { param($r) "$($r.Value.at.function) line $($r.Value.at.line); locals: $(($r.Value.locals | Select-Object -First 4 | ForEach-Object { "$($_.name) = $($_.value)" }) -join ', ')" })
        foreach ($e in 'Lines.Sum(l => l.Subtotal)', 'total', 'DiscountRate', 'DiscountedLines().Count()') {
            $expression = $e
            [void](& $call 'evaluate' @{ expression = $expression } { param($r) "$($r.Value.result) ($($r.Value.type))" })
        }
        # 画面の「AI の操作」と、見本のアプリの窓 (止まっている間の写し) を並べる。
        Press $browser $t.Agent
        $left = Join-Path $Demo "temp\mcp-left-$L.png"; Save-Shot $browser $left
        $shot = Invoke-Mcp $engine 'ui_screenshot' @{ scope = 'window' }
        $image = @($shot.Raw.result.content | Where-Object { $_.type -eq 'image' })[0]
        $right = Join-Path $Demo "temp\mcp-right-$L.png"; [IO.File]::WriteAllBytes($right, [Convert]::FromBase64String($image.data))
        # 見本のアプリの写しは画面の拡大率のぶん大きいので、画面の半分ほどの高さに縮めて右に置く。
        $a = [Drawing.Image]::FromFile($left); $b = [Drawing.Image]::FromFile($right)
        $scale = [Math]::Min(1.0, $a.Height * 0.55 / $b.Height)
        $bw = [int]($b.Width * $scale); $bh = [int]($b.Height * $scale)
        $canvas = New-Object Drawing.Bitmap ($a.Width + $bw + 48), $a.Height
        $g = [Drawing.Graphics]::FromImage($canvas); $g.InterpolationMode = 'HighQualityBicubic'; $g.Clear([Drawing.Color]::FromArgb(30, 30, 30)); $g.DrawImage($a, 0, 0, $a.Width, $a.Height); $g.DrawImage($b, $a.Width + 24, [int](($a.Height - $bh) / 2), $bw, $bh); $g.Dispose()
        $canvas.Save((Join-Path $Out 'mcp-ui.png'), [Drawing.Imaging.ImageFormat]::Png); $canvas.Dispose(); $a.Dispose(); $b.Dispose()
        Write-Host "  $(Join-Path $Out 'mcp-ui.png')"
        [void](& $call 'debug_continue' @{ waitMs = 1000 } { param($r) "$($r.Value.status.state)$($r.Value.state)" })
        [void](Invoke-Mcp $engine 'breakpoint_remove' @{ file = $orders; line = $line })
        # やり取りを会話の形に並べる (依頼は例の文。呼び出しと答えは、上で実際に行ったもの)。
        $request, $note = $requests[$L]
        $html = New-Object System.Text.StringBuilder
        [void]$html.Append('<!DOCTYPE html><html><head><meta charset="utf-8"><style>body{font:15px "Segoe UI",sans-serif;background:#1e1e1e;color:#ddd;margin:0;padding:24px;width:900px}.req{background:#264f78;color:#fff;border-radius:10px;padding:12px 16px;margin:0 0 6px 120px}.note{color:#888;font-size:12px;margin:0 0 18px 120px}.call{background:#252526;border-left:3px solid #4ec9b0;border-radius:6px;padding:8px 12px;margin:8px 0}.name{color:#4ec9b0;font-weight:600;font-family:Consolas,monospace}.args{color:#ce9178;font-family:Consolas,monospace;font-size:13px}.ans{color:#b5cea8;font-family:Consolas,monospace;font-size:13px;margin-top:4px}.ans:before{content:"\2192  ";color:#888}</style></head><body>')
        [void]$html.Append("<div class='req'>$([Net.WebUtility]::HtmlEncode($request))</div><div class='note'>$([Net.WebUtility]::HtmlEncode($note))</div>")
        foreach ($c in $calls) {
            $args = ($c.Arguments.GetEnumerator() | Sort-Object Name | ForEach-Object { $v = if ($_.Value -is [string]) { if ($_.Name -eq 'file') { "`"...\$([IO.Path]::GetFileName($_.Value))`"" } else { "`"$($_.Value)`"" } } else { $_.Value }; "$($_.Name): $v" }) -join ', '
            [void]$html.Append("<div class='call'><span class='name'>$($c.Name)</span> <span class='args'>{ $([Net.WebUtility]::HtmlEncode($args)) }</span><div class='ans'>$([Net.WebUtility]::HtmlEncode($c.Answer))</div></div>")
        }
        [void]$html.Append('</body></html>')
        $page = Join-Path $Demo "temp\mcp-session-$L.html"
        [IO.File]::WriteAllText($page, $html.ToString(), (New-Object Text.UTF8Encoding $false))
        [void](Send-Cdp $browser 'Page.navigate' @{ url = ([Uri]$page).AbsoluteUri })
        Start-Sleep -Milliseconds 800
        $height = [int](Invoke-Js $browser 'document.body.scrollHeight')
        $shot = Send-Cdp $browser 'Page.captureScreenshot' @{ format = 'png'; captureBeyondViewport = $true; clip = @{ x = 0; y = 0; width = 948; height = $height; scale = 1 } }
        [IO.File]::WriteAllBytes((Join-Path $Out 'mcp-session.png'), [Convert]::FromBase64String($shot.result.data))
        Write-Host "  $(Join-Path $Out 'mcp-session.png')"
    } finally {
        Stop-Browser $browser
        Stop-Engine $engine
    }
}

# 記録 (SQL と例外)、画面の木、使用量とヒープの統計。止めずに記録を取り、最後だけ止めてヒープを数える。
function Capture-Records([string]$L, [string]$Out) {
    $t = Get-Labels $L
    $engine = Start-Engine $L @((Join-Path $Demo 'app\OrderDemo.exe'))
    $browser = $null
    try {
        $browser = Start-Browser $engine.Url 1600 860
        [void](Wait-Status $browser $t.Running)
        [void](Invoke-Mcp $engine 'ui_wait_for' @{ name = 'addButton'; timeoutMs = 30000 })
        [void](Invoke-Mcp $engine 'sql_log' @{ enabled = $true })
        [void](Invoke-Mcp $engine 'exception_log' @{ enabled = $true })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'customerBox'; value = 'Carol' })
        # 在庫 (2) より多いチーズケーキは、捕まえる例外になる (止まらずに記録に残る)。
        [void](Invoke-Mcp $engine 'ui_select' @{ name = 'productBox'; index = 2 })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '5' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 1000 })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '2' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 1000 })
        [void](Invoke-Mcp $engine 'ui_select' @{ name = 'productBox'; index = 0 })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '3' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 1000 })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'placeButton'; waitMs = 2000 })
        Set-BottomHeight $browser 470
        Press $browser $t.Records
        [void](Wait-Browser $browser "document.querySelectorAll('.record-list tr').length > 2" 15000)
        # 最初の SQL の行を選んで、詳細 (値を埋めた文とパラメーター) を出す。
        [void](Invoke-Js $browser "[...document.querySelectorAll('.record-list tr')].find(tr => /INSERT INTO Orders /.test(tr.innerText)).click()")
        [void](Wait-Browser $browser "!!document.querySelector('.record-detail .sql')")
        Save-Shot $browser (Join-Path $Out 'records.png')
        Press $browser $t.Windows
        Press $browser $t.ReadTree
        [void](Wait-Browser $browser "[...document.querySelectorAll('.controls li')].some(li => li.innerText.includes('placeButton'))" 15000)
        [void](Invoke-Js $browser "[...document.querySelectorAll('.controls li')].find(li => li.innerText.includes('addButton')).click()")
        [void](Wait-Browser $browser "!!document.querySelector('.shot .frame') && document.querySelector('.shot img').naturalWidth > 0")
        Save-Shot $browser (Join-Path $Out 'windows.png')
        Press $browser $t.Pause
        [void](Wait-Status $browser $t.Stopped)
        Press $browser $t.Usage
        Press $browser $t.Heap
        [void](Wait-Browser $browser "document.querySelectorAll('.grid.heap tr').length > 3" 60000)
        Save-Shot $browser (Join-Path $Out 'usage.png')
        Press $browser $t.Continue
    } finally {
        Stop-Browser $browser
        Stop-Engine $engine
    }
}

# PDB の無い exe: 動いている間に逆コンパイルして開き、そのソースの行で止める。
function Capture-Decompile([string]$L, [string]$Out) {
    $t = Get-Labels $L
    $engine = Start-Engine $L @((Join-Path $Demo 'app-nopdb\OrderDemo.exe'))
    $browser = $null
    try {
        $browser = Start-Browser $engine.Url 1280 800
        [void](Wait-Status $browser $t.Running)
        [void](Invoke-Mcp $engine 'ui_wait_for' @{ name = 'addButton'; timeoutMs = 30000 })
        # ソースの一覧の、PDB の無いモジュールの行の「逆コンパイルして開く」(ほかのモジュールの行にもあるので、行の中で押す)。
        $listed = Wait-Browser $browser "[...document.querySelectorAll('.sources li.pending')].some(li => li.innerText.includes('OrderDemo'))" 30000
        if (-not $listed) { Save-BrowserScreenshot $browser (Join-Path $Demo 'temp\decompile-failed.png'); throw "ソースの一覧に OrderDemo がありません: $(Invoke-Js $browser "[...document.querySelectorAll('.sources li')].map(li => li.className + ':' + li.innerText).join(' | ')")" }
        [void](Invoke-Js $browser "[...document.querySelectorAll('.sources li.pending')].find(li => li.innerText.includes('OrderDemo')).querySelector('button').click()")
        $source = Open-Source $browser 'Order.cs' '* DiscountRate'
        Toggle-Breakpoint $browser $source.Line
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '4' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 5000 })
        [void](Wait-Status $browser $t.Stopped)
        Press $browser $t.Variables
        Save-Shot $browser (Join-Path $Out 'decompile.png')
        Toggle-Breakpoint $browser $source.Line
        Press $browser $t.Continue
    } finally {
        Stop-Browser $browser
        Stop-Engine $engine
    }
}

# カバレッジと成績書: 開く画面でカバレッジを選んで起動し、成績書の項目を記録しながら AI が操作する。
function Capture-Coverage([string]$L, [string]$Out) {
    $t = Get-Labels $L
    $engine = Start-Engine $L
    $browser = $null
    try {
        $browser = Start-Browser $engine.Url 1280 800
        Press $browser $t.Open
        [void](Wait-Browser $browser "!!document.querySelector('.dialog')")
        $program = ConvertTo-Json (Join-Path $Demo 'app\OrderDemo.exe')
        [void](Invoke-Js $browser "(() => { const i = document.querySelectorAll('.dialog label input')[0]; i.value = $program; i.dispatchEvent(new Event('input', { bubbles: true })); const c = document.querySelector('.coverage-option'); if (!c.checked) c.click(); })()")
        [void](Wait-Browser $browser "document.querySelector('.coverage-option').checked && !document.querySelector('.dialog button.primary').disabled")
        [void](Invoke-Js $browser "document.querySelector('.dialog button.primary').click()")
        [void](Wait-Browser $browser "!!document.querySelector('.coverage-summary')" 30000)
        [void](Invoke-Mcp $engine 'ui_wait_for' @{ name = 'addButton'; timeoutMs = 30000 })
        # 成績書: 番号と項目を入れて始め、AI の操作を記録する。
        Press $browser $t.Report
        [void](Wait-Browser $browser "!!document.querySelector('.report-id-input')")
        $folder = ConvertTo-Json (Join-Path $Demo 'reports')
        $title = ConvertTo-Json $(if ($L -eq 'ja') { 'まとめ買いの割引' } else { 'Bulk discount' })
        [void](Invoke-Js $browser "(() => { const set = (e, v) => { e.value = v; e.dispatchEvent(new Event('input', { bubbles: true })); }; set(document.querySelector('.report-id-input'), '1-1'); set(document.querySelector('.report .bar .grow'), $title); const f = document.querySelector('.report .output-folder'); f.value = $folder; f.dispatchEvent(new Event('change', { bubbles: true })); })()")
        [void](Invoke-Js $browser "document.querySelector('.report-start').click()")
        [void](Wait-Browser $browser "!document.querySelector('.report-ok').disabled" 15000)
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'customerBox'; value = 'Dave' })
        [void](Invoke-Mcp $engine 'ui_select' @{ name = 'productBox'; index = 1 })
        [void](Invoke-Mcp $engine 'ui_set_text' @{ name = 'quantityBox'; value = '3' })
        [void](Invoke-Mcp $engine 'ui_invoke' @{ name = 'addButton'; waitMs = 1000 })
        [void](Invoke-Js $browser "document.querySelector('.report-capture').click()")
        Start-Sleep -Seconds 1
        [void](Invoke-Js $browser "document.querySelector('.report-ok').click()")
        [void](Wait-Browser $browser "[...document.querySelectorAll('.report-list tr')].some(tr => /1-1/.test(tr.innerText) && tr.querySelector('.badge.report-ok'))" 30000)
        Start-Sleep -Seconds 1
        # カバレッジ: Orders.cs の印 (●○◐) と、上の帯の率。
        Press $browser $t.Sources
        $source = Open-Source $browser 'Orders.cs' 'public decimal Total()'
        [void](Invoke-Js $browser "(() => { const s = document.querySelector('.source'); s.scrollTop = Math.max(0, ($($source.Line) - 14) * 18); s.dispatchEvent(new Event('scroll')); })()")
        Start-Sleep -Milliseconds 800
        Save-Shot $browser (Join-Path $Out 'coverage.png')
        # 書いた成績書 (HTML) を開いて写す。
        $html = Get-ChildItem (Join-Path $Demo 'reports') -Filter *.html -Recurse | Sort-Object LastWriteTime | Select-Object -Last 1
        if ($html) {
            # 表紙の番号から、項目のページ (操作ごとの行と写し) を開く。
            [void](Send-Cdp $browser 'Page.navigate' @{ url = ([Uri]$html.FullName).AbsoluteUri })
            [void](Wait-Browser $browser "[...document.querySelectorAll('a')].some(a => a.innerText.trim() == '1-1')" 15000)
            [void](Invoke-Js $browser "[...document.querySelectorAll('a')].find(a => a.innerText.trim() == '1-1').click()")
            Start-Sleep -Seconds 2
            Save-Shot $browser (Join-Path $Out 'report.png')
        } else {
            Write-Warning '成績書の HTML が見つかりません。'
        }
    } finally {
        Stop-Browser $browser
        Stop-Engine $engine
    }
}

# ---- 流す ----
Initialize-Demo
# PowerShell の変数名は大文字と小文字を区別しないので、引数の $Out と別の名前にする。
$imagesRoot = if ($Out) { $Out } else { Join-Path $root 'guide\images' }
foreach ($L in $Lang) {
    $folder = Join-Path $imagesRoot $L
    New-Item -ItemType Directory -Force $folder | Out-Null
    Write-Host "== $L"
    if ($Scene -contains 'overview') { Capture-Overview $L $folder }
    if ($Scene -contains 'mcp') { Capture-Mcp $L $folder }
    if ($Scene -contains 'records') { Capture-Records $L $folder }
    if ($Scene -contains 'decompile') { Capture-Decompile $L $folder }
    if ($Scene -contains 'coverage') { Capture-Coverage $L $folder }
}