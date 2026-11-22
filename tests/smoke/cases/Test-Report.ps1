# B3: 成績書 (D-081)。PDB のある Gui48 を --ui のエンジンで起動し、テスト項目 1-1 を始め、AI が画面を操作し、写して終える。
# Excel は 1 項目が 1 行 (画像も貼る)、HTML は 1 ファイルで完結 (画像は data URI、script と外部の参照が無い)。
# Start-UiEngine と Invoke-UiHttp は Test-Ui.ps1 のもの。
function Test-Report([string]$ArchName, [string]$Exe) {
    if ($env:NETDBG_SKIP_UI_TESTS -eq '1') { return }
    $program = Join-Path $root "target\apps\$ArchName\Gui48\SmokeGui48.exe"
    if (-not (Test-Path $program)) { return }
    $work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-report-$ArchName"
    if (Test-Path $work) { Remove-Item -Recurse -Force $work }
    # ブラウザは開かないので、起動は DAP で頼む (引数のプログラムは、ページが開いたときに起動される)。
    $engine = Start-UiEngine $Exe @() "report-$ArchName"
    try {
        $bearer = @{ Authorization = "Bearer $($engine.Token)" }
        $dap = { param([string]$Command, $Arguments = @{}) (Invoke-UiHttp "$($engine.Base)/dap" 'POST' $bearer (@{ command = $Command; arguments = $Arguments } | ConvertTo-Json -Depth 10 -Compress)).Text | ConvertFrom-Json }
        $mcp = {
            param([string]$Name, $Arguments)
            $call = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = $Name; arguments = $Arguments } } | ConvertTo-Json -Depth 6 -Compress
            # 起動の直後は、まだ窓ができていないことがある。できるまで繰り返す。
            for ($try = 0; $try -lt 30; $try++) {
                $answer = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer $call).Text | ConvertFrom-Json
                if (-not $answer.result.isError) { return $answer }
                Start-Sleep -Milliseconds 500
            }
            return $answer
        }
        [void](& $dap 'initialize' @{ adapterID = 'netdbg' })
        [void](& $dap 'launch' @{ program = $program; args = @('--close-after=120') })
        [void](& $dap 'configurationDone')
        # 画面ができてから項目を始める (始めの画面の木を読む)。ui_find は、見つからなくても誤りにしないので、中身で確かめる。
        for ($try = 0; $try -lt 40; $try++) {
            $found = & $mcp 'ui_find' @{ name = 'nameBox' }
            if ("$($found.result.content[0].text)" -match 'nameBox') { break }
            Start-Sleep -Milliseconds 500
        }
        # 保存先 (folder) を渡すと、その下に <プログラム名>-<日時> のフォルダーを作る。
        $start = & $dap 'netdbg.testItem' @{ action = 'start'; id = '1-1'; title = '名前を入れて挨拶する'; folder = $work }
        $set = & $mcp 'ui_set_text' @{ name = 'nameBox'; value = 'Taro'; waitMs = 500 }
        $invoke = & $mcp 'ui_invoke' @{ name = 'greetButton'; waitMs = 1000 }
        [void](& $dap 'netdbg.testItem' @{ action = 'step'; text = '結果の欄に挨拶が出ることを見る' })
        # About のダイアログを開いたまま終える: 出したダイアログ (出来事の列) がメッセージに、開いた窓が変化に入る。
        [void](& $mcp 'ui_invoke' @{ name = 'aboutButton'; waitMs = 1000 })
        for ($try = 0; $try -lt 20; $try++) {
            if ("$((& $mcp 'ui_find' @{ name = 'closeButton' }).result.content[0].text)" -match 'closeButton') { break }
            Start-Sleep -Milliseconds 300
        }
        $capture = & $dap 'netdbg.testItem' @{ action = 'capture' }
        $end = & $dap 'netdbg.testItem' @{ action = 'end'; result = 'OK'; note = 'スモーク試験' }
        Check $ArchName '成績書: 項目を始め、AI の操作のあと、写して OK で終える' ($start.success -and -not $set.result.isError -and -not $invoke.result.isError -and $capture.success -and $end.success -and $end.body.result -eq 'OK') "($($start.message) $($capture.message) $($end.message) set: $($set.result.content[0].text) invoke: $($invoke.result.content[0].text))"
        $report = & $dap 'netdbg.testReport' @{}
        $folder = "$($report.body.folder)"
        Check $ArchName '成績書: 保存先の下の <プログラム名>-<日時> に Excel と HTML を書く' ($report.success -and (Test-Path $report.body.xlsx) -and (Test-Path $report.body.html) -and $folder.StartsWith($work) -and (Split-Path $folder -Leaf) -match '^SmokeGui48-\d{8}-\d{6}$' -and $end.body.html -eq $report.body.html) "($($report.message) $folder)"

        # Excel: 1 項目が 1 行 (見出しを除く)。画像も貼ってある。
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($report.body.xlsx)
        try {
            $reader = New-Object IO.StreamReader(($zip.GetEntry('xl/worksheets/sheet1.xml')).Open(), [Text.Encoding]::UTF8)
            $sheet = $reader.ReadToEnd()
            $reader.Close()
            $reader = New-Object IO.StreamReader(($zip.GetEntry('xl/worksheets/dx_sheet1.xml')).Open(), [Text.Encoding]::UTF8)
            $operations = $reader.ReadToEnd()
            $reader.Close()
            # 項目のシート (テンプレートを複製したもの) と、その絵。
            $drawing = @($zip.Entries | Where-Object { $_.FullName -like 'xl/drawings/dx_*.xml' })[0]
            $reader = New-Object IO.StreamReader(($zip.GetEntry('xl/workbook.xml')).Open(), [Text.Encoding]::UTF8)
            $workbook = $reader.ReadToEnd()
            $reader.Close()
            $images = @($zip.Entries | Where-Object { $_.FullName -like 'xl/media/*.png' }).Count
        } finally {
            $zip.Dispose()
        }
        $rows = ([regex]::Matches($sheet, '<row ')).Count
        Check $ArchName '成績書 (Excel): 1 項目が 1 行で、番号と結果と画像がある' ($rows -eq 2 -and $sheet -match '>1-1<' -and $sheet -match '>OK<' -and $images -ge 1) "(rows $rows, images $images)"
        # 項目のシート (名前は番号): 表題、成績書へ戻るリンク、見出し、1 操作が 1 行 (AI 3 つと手順 1 つ)、始め・写す・終わりの画面。AI の操作には、塗った画面とあとの画面の 2 枚。
        # 成績書のシートの番号は、項目のシートへのリンク。
        $operationRows = ([regex]::Matches($operations, '<row ')).Count
        $linked = $sheet.Contains('<hyperlink ref="A2" location="&#39;1-1&#39;!A1"/>') -and $operations.Contains('<hyperlink ref="A2" location="&#39;成績書&#39;!A2"/>') -and $workbook.Contains('<sheet name="1-1" ')
        Check $ArchName '成績書 (Excel): 項目ごとのシートに 1 操作 1 行で操作ごとの画面があり、成績書の行と互いにリンクする' ($operationRows -ge 7 -and $operations.Contains('ui_set_text') -and $null -ne $drawing -and $images -ge 7 -and $linked) "(rows $operationRows, images $images, linked $linked)"
        # HTML: 1 ファイルで完結する。
        $html = [IO.File]::ReadAllText($report.body.html, [Text.Encoding]::UTF8)
        $external = @('<script', '<link', 'src="http', 'href="http', 'url(http', '@import') | Where-Object { $html.Contains($_) }
        Check $ArchName '成績書 (HTML): 画像は data URI で、外部の参照が無い' ($html.Contains('<img src="data:image/png;base64,') -and $external.Count -eq 0) "($($external -join ', '))"
        Check $ArchName '成績書 (HTML): AI の操作、手順、変わったコントロール、exe の版が入る' ($html.Contains('ui_set_text') -and $html.Contains('ui_invoke') -and $html.Contains('結果の欄に挨拶が出ることを見る') -and $html -match 'resultLabel の文字: 「[^」]*」→「Hello, Taro' -and $html -match '\d+\.\d+\.\d+\.\d+') "($([regex]::Match($html, '<h3>変わったコントロール</h3>[\s\S]{0,400}').Value))"
        # 操作ごとの画面: 前の画面に操作した箇所を塗ったもの (AI は始める前に写す) と、操作のあとの画面。
        Check $ArchName '成績書 (HTML): AI の操作ごとに、操作した箇所を塗った画面と、操作のあとの画面がある' ($html.Contains('alt="操作 1 の箇所"') -and $html.Contains('（操作 1 のあと）') -and $html.Contains('alt="操作 2 の箇所"') -and $html.Contains('（操作 4 のあと）')) ""
        Check $ArchName '成績書 (HTML): 出したダイアログがメッセージに、開いた窓が変化に入る' ($html -match '<h3>メッセージ</h3>\s*<ul>\s*<li>[^<]*ShowDialog' -and $html.Contains('窓「About Smoke GUI」が開いた')) "($([regex]::Match($html, '<h3>変わったコントロール</h3>[\s\S]{0,600}').Value))"
        # テンプレート (D-083): 既定のテンプレートを書き出し、現場風に直して (HTML に「現場A」、Excel の見出しを「試験番号」に)、それで書く。
        $templateFolder = Join-Path $work 'template'
        $exported = & $mcp 'test_report' @{ exportTemplate = $templateFolder }
        $again = & $mcp 'test_report' @{ exportTemplate = $templateFolder }
        $files = @('report.html', 'report.xlsx', 'names.txt') | Where-Object { Test-Path (Join-Path $templateFolder $_) }
        Check $ArchName '成績書 (テンプレート): 既定のテンプレートと名前の説明を書き出し、2 回目は上書きしない' (-not $exported.result.isError -and $files.Count -eq 3 -and $again.result.isError -and "$($again.result.content[0].text)" -match '上書きしません') "($($exported.result.content[0].text) / $($again.result.content[0].text))"
        $htmlTemplate = Join-Path $templateFolder 'report.html'
        [IO.File]::WriteAllText($htmlTemplate, [IO.File]::ReadAllText($htmlTemplate, [Text.Encoding]::UTF8).Replace('<h1>成績書</h1>', '<h1>現場A 成績書</h1>'), (New-Object Text.UTF8Encoding $false))
        Add-Type -AssemblyName System.IO.Compression
        $zip = [IO.Compression.ZipFile]::Open((Join-Path $templateFolder 'report.xlsx'), 'Update')
        try {
            $entry = $zip.GetEntry('xl/worksheets/sheet1.xml')
            $reader = New-Object IO.StreamReader($entry.Open(), [Text.Encoding]::UTF8)
            $xml = $reader.ReadToEnd().Replace('>番号<', '>試験番号<')
            $reader.Close()
            $entry.Delete()
            $writer = New-Object IO.StreamWriter(($zip.CreateEntry('xl/worksheets/sheet1.xml')).Open(), (New-Object Text.UTF8Encoding $false))
            $writer.Write($xml)
            $writer.Close()
        } finally {
            $zip.Dispose()
        }
        $siteWork = Join-Path $work 'site'
        $site = & $dap 'netdbg.testReport' @{ output = $siteWork; template = $templateFolder }
        $siteHtml = if ($site.success) { [IO.File]::ReadAllText($site.body.html, [Text.Encoding]::UTF8) } else { '' }
        $siteZip = if ($site.success) { [IO.Compression.ZipFile]::OpenRead($site.body.xlsx) } else { $null }
        try {
            $siteSheet = if ($siteZip) { (New-Object IO.StreamReader(($siteZip.GetEntry('xl/worksheets/sheet1.xml')).Open(), [Text.Encoding]::UTF8)).ReadToEnd() } else { '' }
            $siteBook = if ($siteZip) { (New-Object IO.StreamReader(($siteZip.GetEntry('xl/workbook.xml')).Open(), [Text.Encoding]::UTF8)).ReadToEnd() } else { '' }
        } finally {
            if ($siteZip) { $siteZip.Dispose() }
        }
        Check $ArchName '成績書 (テンプレート): 直したテンプレートで書くと、HTML と Excel の両方に出る' ($site.success -and $siteHtml.Contains('<h1>現場A 成績書</h1>') -and $siteHtml.Contains('ui_set_text') -and $siteSheet.Contains('>試験番号<') -and $siteSheet.Contains('>1-1<') -and $siteBook.Contains('<sheet name="1-1"')) "($($site.message))"
        [IO.File]::WriteAllText($htmlTemplate, '<p>{{番号違い}}</p>', (New-Object Text.UTF8Encoding $false))
        $broken = & $dap 'netdbg.testReport' @{ output = $siteWork; template = $templateFolder }
        Check $ArchName '成績書 (テンプレート): 知らない名前は、名前を挙げて断る' (-not $broken.success -and "$($broken.message)" -match '番号違い') "($($broken.message))"

        # 手順 (D-086): 成績書と一緒に書いた .steps.json に、AI の操作が名前で指す手順として入る。起動し直した exe に流すと、
        # 同じコントロールが同じ値になる。
        $stepsFile = "$($report.body.steps)"
        $stepsJson = if ($stepsFile -and (Test-Path $stepsFile)) { [IO.File]::ReadAllText($stepsFile, [Text.Encoding]::UTF8) | ConvertFrom-Json } else { $null }
        $stepNames = @($stepsJson.items | ForEach-Object { $_.steps } | ForEach-Object { "$($_.tool):$($_.name)" })
        Check $ArchName '成績書 (手順): AI の操作を、名前で指す手順として .steps.json に書く' ($stepNames -contains 'ui_set_text:nameBox' -and $stepNames -contains 'ui_invoke:greetButton' -and $stepNames -contains 'ui_invoke:aboutButton') "($stepsFile $($stepNames -join ', '))"
        [void](& $dap 'disconnect' @{ terminateDebuggee = $true })
        $again = $null
        for ($try = 0; $try -lt 20 -and -not $again.success; $try++) {
            Start-Sleep -Milliseconds 500
            $again = & $dap 'initialize' @{ adapterID = 'netdbg' }
        }
        [void](& $dap 'launch' @{ program = $program; args = @('--close-after=120') })
        [void](& $dap 'configurationDone')
        for ($try = 0; $try -lt 40; $try++) {
            if ("$((& $mcp 'ui_find' @{ name = 'resultLabel' }).result.content[0].text)" -match 'resultLabel') { break }
            Start-Sleep -Milliseconds 500
        }
        # 流すのは 1 回だけ (失敗を繰り返すと、操作が重なる)。
        $call = @{ jsonrpc = '2.0'; id = 1; method = 'tools/call'; params = @{ name = 'ui_replay'; arguments = @{ file = $stepsFile; item = '1-1' } } } | ConvertTo-Json -Depth 6 -Compress
        $replay = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer $call).Text | ConvertFrom-Json
        $label = "$((& $mcp 'ui_find' @{ name = 'resultLabel' }).result.content[0].text)"
        $dialog = "$((& $mcp 'ui_find' @{ name = 'closeButton' }).result.content[0].text)"
        Check $ArchName '成績書 (手順): 起動し直した exe に ui_replay で流すと、同じ値になり、同じダイアログが開く' (-not $replay.result.isError -and "$($replay.result.content[0].text)" -match '"completed": 3' -and $label -match 'Hello, Taro' -and $dialog -match 'closeButton') "(replay: $($replay.result.content[0].text) label: $label)"
        $missing = (Invoke-UiHttp "$($engine.Base)/mcp" 'POST' $bearer (@{ jsonrpc = '2.0'; id = 2; method = 'tools/call'; params = @{ name = 'ui_replay'; arguments = @{ steps = @(@{ tool = 'ui_invoke'; name = 'noSuchButton' }); timeoutMs = 500 } } } | ConvertTo-Json -Depth 6 -Compress)).Text | ConvertFrom-Json
        Check $ArchName '成績書 (手順): 見つからない相手の手で止まり、何手目かと理由を返す' ($missing.result.isError -and "$($missing.result.content[0].text)" -match '1 手のうち 1 手目' -and "$($missing.result.content[0].text)" -match 'noSuchButton') "($($missing.result.content[0].text))"
    } catch {
        Check $ArchName '成績書: 例外なしで終わる' $false $_.Exception.Message
    } finally {
        Stop-UiEngine $engine
        Get-Process SmokeGui48 -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $program } | Stop-Process -Force
    }
}
