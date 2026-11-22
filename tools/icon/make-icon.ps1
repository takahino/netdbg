<#
.SYNOPSIS
  アイコンの元 (assets/netdbg.svg) から、exe に埋め込む assets/netdbg.ico と、PNG を作る。

.DESCRIPTION
  SVG はブラウザ (headless の Chrome か Edge。tests/smoke/Cdp.psm1) で、地を透明にして大きさごとに描く。
  .ico は 16、20、24、32、40、48、64、128、256 px。256 は PNG、それより小さいものは 32 ビットの BMP で入れる
  (小さい絵を PNG で入れると、読めない道具がある)。依存は足さない。

.PARAMETER Png
  PNG を書く先と大きさ (例: @{ 'assets\netdbg-256.png' = 256 })。拡張のアイコン (128 px 以上) にも使う。
#>
param(
    [hashtable]$Png = @{}
)
$ErrorActionPreference = 'Stop'
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
Import-Module (Join-Path $root 'tests\smoke\Cdp.psm1') -Force
Add-Type -AssemblyName System.Drawing

$svg = Join-Path $root 'assets\netdbg.svg'
$icoSizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$sizes = @($icoSizes + @($Png.Values)) | Sort-Object -Unique

# 大きさごとに、絵を横に並べた頁を作り、1 枚で撮って切り分ける。
$work = Join-Path ([IO.Path]::GetTempPath()) "netdbg-icon-$PID"
New-Item -ItemType Directory -Force $work | Out-Null
Copy-Item $svg (Join-Path $work 'icon.svg')
$x = 0
$places = @{}
$html = New-Object System.Text.StringBuilder
[void]$html.Append('<!DOCTYPE html><html><head><style>html,body{margin:0;background:transparent;overflow:hidden}img{position:absolute;top:0}</style></head><body>')
foreach ($size in $sizes) {
    [void]$html.Append("<img src='icon.svg' width='$size' height='$size' style='left:${x}px'>")
    $places[$size] = $x
    $x += $size + 8
}
[void]$html.Append('</body></html>')
[IO.File]::WriteAllText((Join-Path $work 'sheet.html'), $html.ToString(), (New-Object Text.UTF8Encoding $false))
$browser = Start-Browser ([Uri](Join-Path $work 'sheet.html')).AbsoluteUri ($x + 64) 400
try {
    # 描く範囲を、画面の拡大率によらず 1 倍の決まった大きさにする (小さい窓だとスクロール バーが出て、絵が切れる)。
    [void](Send-Cdp $browser 'Emulation.setDeviceMetricsOverride' @{ width = $x + 8; height = 264; deviceScaleFactor = 1; mobile = $false })
    [void](Send-Cdp $browser 'Emulation.setDefaultBackgroundColorOverride' @{ color = @{ r = 0; g = 0; b = 0; a = 0 } })
    [void](Wait-Browser $browser "[...document.images].every(i => i.complete && i.naturalWidth > 0)" 10000)
    Start-Sleep -Milliseconds 300
    $shot = Send-Cdp $browser 'Page.captureScreenshot' @{ format = 'png'; clip = @{ x = 0; y = 0; width = $x; height = 256; scale = 1 } }
    $sheetPath = Join-Path $work 'sheet.png'
    [IO.File]::WriteAllBytes($sheetPath, [Convert]::FromBase64String($shot.result.data))
} finally {
    Stop-Browser $browser
}

$sheet = [Drawing.Bitmap]::FromFile($sheetPath)
function Get-Image([int]$Size) {
    $bitmap = New-Object Drawing.Bitmap $Size, $Size, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [Drawing.Graphics]::FromImage($bitmap)
    $g.DrawImage($sheet, (New-Object Drawing.Rectangle 0, 0, $Size, $Size), (New-Object Drawing.Rectangle $places[$Size], 0, $Size, $Size), [Drawing.GraphicsUnit]::Pixel)
    $g.Dispose()
    return $bitmap
}

# 32 ビットの BMP (BITMAPINFOHEADER、下から上の BGRA、AND マスク) を .ico の 1 枚にする。
function Get-DibBytes([Drawing.Bitmap]$Bitmap) {
    $size = $Bitmap.Width
    $stream = New-Object IO.MemoryStream
    $writer = New-Object IO.BinaryWriter $stream
    $maskRow = [int]([Math]::Ceiling($size / 32.0) * 4)
    $writer.Write([int]40); $writer.Write([int]$size); $writer.Write([int]($size * 2))
    $writer.Write([int16]1); $writer.Write([int16]32); $writer.Write([int]0)
    $writer.Write([int]($size * $size * 4 + $maskRow * $size)); $writer.Write([int]0); $writer.Write([int]0); $writer.Write([int]0); $writer.Write([int]0)
    for ($y = $size - 1; $y -ge 0; $y--) {
        for ($x = 0; $x -lt $size; $x++) {
            $c = $Bitmap.GetPixel($x, $y)
            $writer.Write([byte]$c.B); $writer.Write([byte]$c.G); $writer.Write([byte]$c.R); $writer.Write([byte]$c.A)
        }
    }
    $writer.Write((New-Object byte[] ($maskRow * $size)))
    $writer.Flush()
    return $stream.ToArray()
}

function Get-PngBytes([Drawing.Bitmap]$Bitmap) {
    $stream = New-Object IO.MemoryStream
    $Bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
    return $stream.ToArray()
}

# .ico: ICONDIR、ICONDIRENTRY を並べ、そのあとに絵。
$images = foreach ($size in $icoSizes) {
    $bitmap = Get-Image $size
    $bytes = if ($size -ge 256) { Get-PngBytes $bitmap } else { Get-DibBytes $bitmap }
    $bitmap.Dispose()
    , @($size, $bytes)
}
$ico = New-Object IO.MemoryStream
$w = New-Object IO.BinaryWriter $ico
$w.Write([int16]0); $w.Write([int16]1); $w.Write([int16]$images.Count)
$offset = 6 + 16 * $images.Count
foreach ($image in $images) {
    $size, $bytes = $image
    $edge = if ($size -ge 256) { 0 } else { $size }
    $w.Write([byte]$edge); $w.Write([byte]$edge); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([int16]1); $w.Write([int16]32); $w.Write([int]$bytes.Length); $w.Write([int]$offset)
    $offset += $bytes.Length
}
foreach ($image in $images) { $w.Write([byte[]]$image[1]) }
$w.Flush()
[IO.File]::WriteAllBytes((Join-Path $root 'assets\netdbg.ico'), $ico.ToArray())
Write-Host "  assets\netdbg.ico ($($icoSizes -join ', ') px, $($ico.Length) bytes)"

foreach ($target in $Png.Keys) {
    $bitmap = Get-Image $Png[$target]
    $path = if ([IO.Path]::IsPathRooted($target)) { $target } else { Join-Path $root $target }
    $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
    Write-Host "  $path ($($Png[$target]) px)"
}
$sheet.Dispose()
Remove-Item -Recurse -Force $work
