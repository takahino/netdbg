# 段階 8: MCP の ui_* のツール。WinForms の対象 (tests/apps/Gui。D-054) と WPF の対象 (tests/apps/Wpf。UI Automation。
# D-055) の画面を、同じ項目で読み、操作し、撮る。
# 窓を開き、実際のマウスとキーの入力を送る。環境変数 NETDBG_SKIP_UI_TESTS=1 で飛ばす。

Add-Type -AssemblyName System.Drawing

# ui_screenshot の答えから、画像 (Bitmap) と情報を取り出す。
function Get-McpScreenshot($Session, [hashtable]$Arguments) {
    $answer = Send-Mcp $Session 'tools/call' @{ name = 'ui_screenshot'; arguments = $Arguments }
    if ($answer.result.isError) { return @{ Error = $answer.result.content[0].text } }
    $image = @($answer.result.content | Where-Object { $_.type -eq 'image' })[0]
    $info = (@($answer.result.content | Where-Object { $_.type -eq 'text' })[0].text | ConvertFrom-Json)
    $bytes = [Convert]::FromBase64String($image.data)
    $bitmap = New-Object System.Drawing.Bitmap (New-Object System.IO.MemoryStream (, $bytes))
    return @{ Bitmap = $bitmap; Info = $info; Bytes = $bytes; Mime = $image.mimeType }
}

function Find-UiElement($Tree, [string]$Name) {
    foreach ($w in @($Tree.windows)) { foreach ($e in @($w.elements)) { if ($e.name -eq $Name) { return $e } } }
    return $null
}

# 試す画面の作り方ごとの違い (アプリ、題、型の名前、プログラムの側の式)。
$script:GuiApps = @(
    @{ Label = 'WinForms'; Source = 'Gui'; Dirs = @{ Console = 'Gui'; Console48 = 'Gui48' }; Exe = @{ Console = 'SmokeGui.exe'; Console48 = 'SmokeGui48.exe' }
       Title = 'Smoke GUI'; AboutTitle = 'About Smoke GUI'; CheckType = 'System.Windows.Forms.CheckBox'
       Values = 'nameBox.Text + "/" + agreeBox.Checked + "/" + colorBox.Text'; Click = 'e.X * 1000 + e.Y' }
    @{ Label = 'WPF'; Source = 'Wpf'; Dirs = @{ Console = 'Wpf'; Console48 = 'Wpf48' }; Exe = @{ Console = 'SmokeWpf.exe'; Console48 = 'SmokeWpf48.exe' }
       Title = 'Smoke WPF'; AboutTitle = 'About Smoke WPF'; CheckType = 'CheckBox'
       Values = 'nameBox.Text + "/" + agreeBox.IsChecked + "/" + colorBox.Text'; Click = '(int)p.X * 1000 + (int)p.Y' }
)

function Test-McpGui([string]$ArchName, [string]$Exe) {
    if ($env:NETDBG_SKIP_UI_TESTS -eq '1') { return }
    foreach ($spec in $script:GuiApps) { Test-McpGuiApp $ArchName $Exe $spec }
}

function Test-McpGuiApp([string]$ArchName, [string]$Exe, [hashtable]$Spec) {
    $flavorDir = $script:flavor.Dir
    if (-not $Spec.Dirs.ContainsKey($flavorDir)) { return }
    $dir, $name = $Spec.Dirs[$flavorDir], $Spec.Exe[$flavorDir]
    $app = Join-Path $root "target\apps\$ArchName\$dir\$name"
    if (-not (Test-Path $app)) { return }
    $source = Join-Path $root "tests\apps\$($Spec.Source)\Program.cs"
    $L = $Spec.Label
    $log = Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-mcpgui-$($L.ToLower())-$ArchName.log"
    $s = Start-McpSession $Exe $log
    try {
        [void](Send-Mcp $s 'initialize' @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'smoke'; version = '0' } })
        [void](Invoke-McpTool $s 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'greet') })
        [void](Invoke-McpTool $s 'debug_launch' @{ program = $app; args = @('--close-after=120'); waitMs = 0 })
        $r = Invoke-McpTool $s 'ui_wait_for' @{ name = 'greetButton'; timeoutMs = 30000 } 40000
        Check $ArchName "MCP GUI ($L): 窓が開くのを待てる (ui_wait_for)" ($r.Value.found -eq $true) $r.Text

        $tree = (Invoke-McpTool $s 'ui_tree').Value
        $main = @($tree.windows)[0]
        $nameBox = Find-UiElement $tree 'nameBox'
        $greet = Find-UiElement $tree 'greetButton'
        Check $ArchName "MCP GUI ($L): ui_tree にコントロールの名前、種類、文字、座標" ($main.title -eq $Spec.Title -and $nameBox.kind -eq 'TextBox' -and $greet.kind -eq 'Button' -and $greet.text -eq 'Greet' -and $greet.inWindow.width -gt 0 -and $greet.screen.x -eq ($main.screen.x + $greet.inWindow.x) -and $greet.mark -ge 1) "($(($main | ConvertTo-Json -Depth 4 -Compress).Substring(0, 300)))"

        # 最初の停止の比べる元は、起動のあと最初に読んだ木 (D-087)。窓が開いてから、読む間 (1 秒ごと) を置いて操作する。
        Start-Sleep -Milliseconds 1500
        $r = Invoke-McpTool $s 'ui_set_text' @{ name = 'nameBox'; value = 'Ada' }
        Check $ArchName "MCP GUI ($L): ui_set_text で文字を入れる (止まらなければ running)" (-not $r.Error -and $r.Value.status.state -eq 'running') $r.Text
        $r = Invoke-McpTool $s 'ui_toggle' @{ name = 'agreeBox'; checked = $true }
        Check $ArchName "MCP GUI ($L): ui_toggle でチェックを入れる" (-not $r.Error) $r.Text
        $r = Invoke-McpTool $s 'ui_toggle' @{ name = 'agreeBox'; checked = $true }
        Check $ArchName "MCP GUI ($L): もうその状態なら、ui_toggle は何もしない" ($r.Value.changed -eq $false) $r.Text
        $r = Invoke-McpTool $s 'ui_select' @{ name = 'colorBox'; item = 'Purple' }
        Check $ArchName "MCP GUI ($L): 無い項目は、項目の一覧つきで断る" ($r.Error -and $r.Text -match 'Red, Green, Blue') $r.Text
        [void](Invoke-McpTool $s 'ui_select' @{ name = 'colorBox'; item = 'Green' })
        $tree = (Invoke-McpTool $s 'ui_tree').Value
        Check $ArchName "MCP GUI ($L): 入れた文字、チェック、選んだ項目が ui_tree に出る" ((Find-UiElement $tree 'nameBox').text -eq 'Ada' -and (Find-UiElement $tree 'agreeBox').checked -eq $true -and (Find-UiElement $tree 'agreeBox').kind -eq 'CheckBox' -and (Find-UiElement $tree 'agreeBox').type -eq $Spec.CheckType -and (Find-UiElement $tree 'colorBox').text -eq 'Green') "($(@('nameBox', 'agreeBox', 'colorBox') | ForEach-Object { Find-UiElement $tree $_ } | ConvertTo-Json -Compress))"

        # ボタンを押すと、ハンドラーのブレークポイントで止まり、止まった場所が返る。
        # 画面は、動いている間に読む (D-087)。知らせの来ない変化 (WinForms のチェック、WPF の文字) は 1 秒ごとに読むので、
        # 人の操作ほどの間を置いてから止める。
        Start-Sleep -Milliseconds 1500
        $r = Invoke-McpTool $s 'ui_invoke' @{ name = 'greetButton'; waitMs = 10000 }
        Check $ArchName "MCP GUI ($L): ui_invoke でハンドラーのブレークポイントに止まり、場所が返る" ($r.Value.stopped -eq $true -and $r.Value.status.at.line -eq (Get-MarkerLine $source 'greet')) $r.Text
        # 停止のあいだの変化 (D-087): 動いている間に読んだ画面と比べ、最初の停止では、起動のあとに入れた文字とチェックが出る。
        $c = Invoke-McpTool $s 'ui_changes'
        $nameChange = @($c.Value.changes | Where-Object { $_.control -eq 'nameBox' -and $_.what -eq 'text' -and $_.after -eq 'Ada' })
        $agreeChange = @($c.Value.changes | Where-Object { $_.control -eq 'agreeBox' -and $_.what -eq 'checked' })
        Check $ArchName "MCP GUI ($L): 止まると、動いている間に変わった文字とチェックが ui_changes に出る" ($c.Value.stop -eq 1 -and $nameChange.Count -eq 1 -and $agreeChange.Count -eq 1) $c.Text
        $v = Invoke-McpTool $s 'evaluate' @{ expression = $Spec.Values }
        Check $ArchName "MCP GUI ($L): 画面で入れた値が、プログラムの側で見える" ($v.Value.result -eq '"Ada/True/Green"') $v.Text
        $r = Invoke-McpTool $s 'ui_invoke' @{ name = 'greetButton' }
        Check $ArchName "MCP GUI ($L): 止まっている間の操作は、理由つきで断る" ($r.Error -and $r.Text -match 'debug_continue') $r.Text

        # 止まっている間の画像: 「応答なし」の写しではなく、もとの描画 (canvas の SteelBlue) が写る。
        $tree = (Invoke-McpTool $s 'ui_tree').Value
        $canvas = Find-UiElement $tree 'canvas'
        Check $ArchName "MCP GUI ($L): 止まっている間も、名前 (前に読んだもの) と座標が出る" ($canvas -and $tree.note -match '止まって') "($($tree.note))"
        $shot = Get-McpScreenshot $s @{ scope = 'window' }
        $pixel = $shot.Bitmap.GetPixel($canvas.inWindow.x + [int]($canvas.inWindow.width / 2), $canvas.inWindow.y + [int]($canvas.inWindow.height / 2))
        Check $ArchName "MCP GUI ($L): 止まっている間の ui_screenshot に、もとの描画が写る" ($shot.Mime -eq 'image/png' -and $shot.Bitmap.Width -eq $shot.Info.width -and [Math]::Abs($pixel.R - 70) -lt 8 -and [Math]::Abs($pixel.G - 130) -lt 8 -and [Math]::Abs($pixel.B - 180) -lt 8) "(画素 $pixel、$($shot.Info.width)x$($shot.Info.height)、$($shot.Error))"
        $shot.Bitmap.Save((Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-gui-$($L.ToLower())-$ArchName.png"))
        $control = Get-McpScreenshot $s @{ scope = 'control'; name = 'canvas' }
        Check $ArchName "MCP GUI ($L): コントロールの範囲だけを撮れる" ($control.Bitmap.Width -eq $canvas.screen.width -and $control.Info.origin.x -eq $canvas.screen.x) "($($control.Bitmap.Width) / $($canvas.screen.width) $($control.Error))"

        $r = Invoke-McpTool $s 'debug_continue' @{ waitMs = 0 }
        $r = Invoke-McpTool $s 'ui_wait_for' @{ name = 'resultLabel'; text = 'Hello, Ada! Green'; timeoutMs = 10000 }
        Check $ArchName "MCP GUI ($L): 続行すると、ハンドラーの結果が画面に出る" ($r.Value.found -eq $true) $r.Text

        # 座標でのクリック: canvas の中の (30, 40) を押すと、MouseClick のブレークポイントで止まる。
        [void](Invoke-McpTool $s 'breakpoint_set' @{ file = $source; line = (Get-MarkerLine $source 'canvas-click') })
        # WinForms は、止まる直前に変わったラベルの文字を、止まってから読める (メッセージを送らずに読む)。WPF は 1 秒ごとに
        # 読む分だけなので、間を置く。
        if ($L -eq 'WPF') { Start-Sleep -Milliseconds 1500 }
        $r = Invoke-McpTool $s 'ui_mouse' @{ name = 'canvas'; x = 30; y = 40; waitMs = 10000 }
        Check $ArchName "MCP GUI ($L): ui_mouse の座標のクリックで、ハンドラーに止まる" ($r.Value.stopped -eq $true -and $r.Value.status.at.line -eq (Get-MarkerLine $source 'canvas-click')) $r.Text
        # 2 回目の停止: 前の停止のあとに変わったもの (ハンドラーが書いた結果の欄) だけが出る。
        $c = Invoke-McpTool $s 'ui_changes'
        $label = @($c.Value.changes | Where-Object { $_.control -eq 'resultLabel' -and $_.what -eq 'text' -and $_.after -eq 'Hello, Ada! Green' })
        $again = @($c.Value.changes | Where-Object { $_.control -eq 'nameBox' })
        Check $ArchName "MCP GUI ($L): 2 回目の停止では、前の停止のあとに変わったものだけが出る" ($c.Value.stop -eq 2 -and $label.Count -eq 1 -and $again.Count -eq 0) $c.Text
        $scale = [double]@($tree.windows)[0].programScale
        $dpi = [double]@($tree.windows)[0].dpiScale
        $v = Invoke-McpTool $s 'evaluate' @{ expression = $Spec.Click }
        $x = [Math]::Floor([int]$v.Value.result / 1000); $y = [int]$v.Value.result % 1000
        # プログラムの中の座標: DPI に対応していなければ 画面のピクセル / programScale。WPF は DPI に依らない単位
        # (1/96 インチ) なので、画面のピクセル / (dpiScale / programScale)。
        $near = { param($d) [Math]::Abs($x - 30 / $d) -le 1 -and [Math]::Abs($y - 40 / $d) -le 1 }
        Check $ArchName "MCP GUI ($L): クリックした位置が、コントロールの中の座標と合う" ((& $near 1) -or (& $near $scale) -or (& $near ($dpi / $scale))) "($($v.Text)、倍率 $scale / $dpi)"
        [void](Invoke-McpTool $s 'breakpoint_clear')
        [void](Invoke-McpTool $s 'debug_continue' @{ waitMs = 0 })

        # キー: テキストボックスにフォーカスし、全部を選んで打ち直す。
        $r = Invoke-McpTool $s 'ui_keys' @{ name = 'nameBox'; keys = '{Ctrl+A}Bob' }
        $r = Invoke-McpTool $s 'ui_wait_for' @{ name = 'nameBox'; text = 'Bob'; timeoutMs = 5000 }
        Check $ArchName "MCP GUI ($L): ui_keys で、フォーカスしたコントロールに打てる" ($r.Value.found -eq $true) $r.Text

        # ダイアログ: 開くのを待ち、アプリ全体の画像に写り、閉じるのを待つ。
        [void](Invoke-McpTool $s 'ui_invoke' @{ name = 'aboutButton' })
        $r = Invoke-McpTool $s 'ui_wait_for' @{ name = 'closeButton'; timeoutMs = 10000 }
        $tree = (Invoke-McpTool $s 'ui_tree').Value
        Check $ArchName "MCP GUI ($L): ダイアログが開くと、前の窓として一覧に出る" ($r.Value.found -eq $true -and @($tree.windows).Count -eq 2 -and @($tree.windows)[0].title -eq $Spec.AboutTitle) "($(@($tree.windows | ForEach-Object title) -join ', '))"
        $whole = Get-McpScreenshot $s @{ scope = 'app'; annotate = $true }
        Check $ArchName "MCP GUI ($L): scope app は、ダイアログを含むすべての窓を撮る" ($whole.Bitmap.Width -ge @($tree.windows)[1].screen.width -and $whole.Info.origin.x -le @($tree.windows)[1].screen.x) "($($whole.Bitmap.Width)x$($whole.Bitmap.Height) $($whole.Error))"
        $whole.Bitmap.Save((Join-Path ([IO.Path]::GetTempPath()) "netdbg-smoke-gui-$($L.ToLower())-app-$ArchName.png"))
        [void](Invoke-McpTool $s 'ui_invoke' @{ name = 'closeButton' })
        $r = Invoke-McpTool $s 'ui_wait_for' @{ name = 'closeButton'; state = 'disappear'; timeoutMs = 10000 }
        Check $ArchName "MCP GUI ($L): ダイアログが閉じるのを待てる" ($r.Value.found -eq $false -and -not $r.Value.waited) $r.Text

        $mainId = @($tree.windows)[1].window
        [void](Invoke-McpTool $s 'ui_window' @{ window = $mainId; action = 'move'; x = 200; y = 160 })
        Start-Sleep -Milliseconds 300
        $tree = (Invoke-McpTool $s 'ui_tree').Value
        $moved = @($tree.windows | Where-Object { $_.window -eq $mainId })[0]
        Check $ArchName "MCP GUI ($L): ui_window で窓を動かせる" ([Math]::Abs($moved.screen.x - 200) -le 12 -and [Math]::Abs($moved.screen.y - 160) -le 12) "($($moved.screen | ConvertTo-Json -Compress))"

        $r = Invoke-McpTool $s 'debug_terminate'
        Check $ArchName "MCP GUI ($L): 終える" ($r.Value.state -eq 'exited') $r.Text
    } catch {
        Check $ArchName "MCP GUI ($L): 例外なしで終わる" $false "$($_.Exception.Message) (ログ: $log)"
    } finally {
        $s.In.Close()
        if (-not $s.Process.WaitForExit(10000)) { $s.Process.Kill() }
        Get-Process ([IO.Path]::GetFileNameWithoutExtension($name)) -ErrorAction SilentlyContinue | Stop-Process -Force
    }
}
