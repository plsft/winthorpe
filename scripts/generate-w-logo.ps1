#!/usr/bin/env pwsh
# Render the stylized W logo to PNG + ICO at the sizes the upstream H
# assets used. Mirrors the path geometry in src/components/winthorpe-logo-animated.tsx
# so the static and animated logos always match.

param(
    [string]$OutDir = "C:\Code\Winthorpe\src\assets",
    [string]$IconsDir = "C:\Code\Winthorpe\src-tauri\icons"
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# Bar geometry — keep in sync with src/components/winthorpe-logo-animated.tsx
$bars = @(
    @{ x1=14; y1=14; x2=32; y2=86; w=10 }
    @{ x1=32; y1=86; x2=50; y2=42; w=10 }
    @{ x1=50; y1=42; x2=68; y2=86; w=10 }
    @{ x1=68; y1=86; x2=86; y2=14; w=10 }
)

function Get-BarTileQuads($bar, $size) {
    # Convert 100x100 viewBox coords to pixel coords. Each bar is split
    # into 3 stacked parallelogram tiles separated by a thin gap, mirroring
    # the SVG component's tiling so static + animated logos match exactly.
    $scale = $size / 100.0
    $x1 = $bar.x1 * $scale; $y1 = $bar.y1 * $scale
    $x2 = $bar.x2 * $scale; $y2 = $bar.y2 * $scale
    $w  = $bar.w  * $scale
    $dx = $x2 - $x1
    $dy = $y2 - $y1
    $len = [Math]::Sqrt($dx*$dx + $dy*$dy)
    $ux = $dx / $len; $uy = $dy / $len
    $px = (-$dy / $len) * ($w / 2.0)
    $py = ( $dx / $len) * ($w / 2.0)

    $tilesPerBar = 3
    $gap = 1.5 * $scale
    $tileLen = ($len - $gap * ($tilesPerBar - 1)) / $tilesPerBar
    $stride = $tileLen + $gap

    $quads = @()
    for ($i = 0; $i -lt $tilesPerBar; $i++) {
        $startT = $i * $stride
        $endT = $startT + $tileLen
        $sx = $x1 + $ux * $startT; $sy = $y1 + $uy * $startT
        $ex = $x1 + $ux * $endT;   $ey = $y1 + $uy * $endT
        $quads += , @(
            [System.Drawing.PointF]::new($sx - $px, $sy - $py),
            [System.Drawing.PointF]::new($sx + $px, $sy + $py),
            [System.Drawing.PointF]::new($ex + $px, $ey + $py),
            [System.Drawing.PointF]::new($ex - $px, $ey - $py)
        )
    }
    return ,$quads
}

function Render-WLogo([int]$size, [string]$path, [System.Drawing.Color]$bg, [System.Drawing.Color]$fg, [bool]$rounded = $true) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

    # Background — rounded square if asked (matches the upstream H icon shape)
    $bgBrush = New-Object System.Drawing.SolidBrush $bg
    if ($rounded) {
        $r = [int]($size * 0.18)
        $rect = New-Object System.Drawing.Rectangle 0, 0, $size, $size
        $path2 = New-Object System.Drawing.Drawing2D.GraphicsPath
        $path2.AddArc($rect.X, $rect.Y, $r*2, $r*2, 180, 90)
        $path2.AddArc($rect.Right - $r*2, $rect.Y, $r*2, $r*2, 270, 90)
        $path2.AddArc($rect.Right - $r*2, $rect.Bottom - $r*2, $r*2, $r*2, 0, 90)
        $path2.AddArc($rect.X, $rect.Bottom - $r*2, $r*2, $r*2, 90, 90)
        $path2.CloseFigure()
        $g.FillPath($bgBrush, $path2)
        $path2.Dispose()
    } else {
        $g.Clear($bg)
    }
    $bgBrush.Dispose()

    # W bars — each split into stacked parallelogram tiles
    $fgBrush = New-Object System.Drawing.SolidBrush $fg
    foreach ($bar in $bars) {
        $quads = Get-BarTileQuads $bar $size
        foreach ($quad in $quads) {
            $g.FillPolygon($fgBrush, $quad)
        }
    }
    $fgBrush.Dispose()

    $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "wrote $path ($size x $size)"
}

# Theme colors matching tauri.conf.json's #0E0E0E backgroundColor
$darkBg = [System.Drawing.Color]::FromArgb(255, 14, 14, 14)
$lightBg = [System.Drawing.Color]::FromArgb(255, 250, 250, 250)
$darkFg = [System.Drawing.Color]::FromArgb(255, 250, 250, 250)  # white W on dark bg
$lightFg = [System.Drawing.Color]::FromArgb(255, 14, 14, 14)    # black W on light bg

# Replace src/assets/winthorpe-logo*.png
Render-WLogo 1024 (Join-Path $OutDir 'winthorpe-logo-1024.png') $darkBg $darkFg $true
Render-WLogo 512  (Join-Path $OutDir 'winthorpe-logo.png')      $darkBg $darkFg $true
Render-WLogo 512  (Join-Path $OutDir 'winthorpe-logo-light.png') $lightBg $lightFg $true

# Replace src-tauri/icons/*.png that Tauri uses for the app
Render-WLogo 32  (Join-Path $IconsDir '32x32.png')        $darkBg $darkFg $false
Render-WLogo 64  (Join-Path $IconsDir '64x64.png')        $darkBg $darkFg $false
Render-WLogo 128 (Join-Path $IconsDir '128x128.png')      $darkBg $darkFg $false
Render-WLogo 256 (Join-Path $IconsDir '128x128@2x.png')   $darkBg $darkFg $false
Render-WLogo 512 (Join-Path $IconsDir 'icon.png')         $darkBg $darkFg $false

# Marketing-screenshot variants (in apps/marketing/public + src-tauri/icons/brand)
$brandDirs = @(
    'C:\Code\Winthorpe\src-tauri\icons\brand'
    'C:\Code\Winthorpe\apps\marketing\public'
)
foreach ($dir in $brandDirs) {
    if (Test-Path $dir) {
        Render-WLogo 1024 (Join-Path $dir 'winthorpe-logo-dark.png') $darkBg $darkFg $true -ErrorAction SilentlyContinue
        Render-WLogo 1024 (Join-Path $dir 'winthorpe-logo-light.png') $lightBg $lightFg $true -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "Generating multi-resolution icon.ico..."

# Multi-resolution ICO from the rendered PNGs (mirrors the earlier
# scripts/generate-icon-ico approach embedded in the rename phase).
$sizes = @(16, 32, 48, 64, 128, 256)
$bitmaps = @()
foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear($darkBg)
    $fgBrush = New-Object System.Drawing.SolidBrush $darkFg
    foreach ($bar in $bars) {
        $quads = Get-BarTileQuads $bar $s
        foreach ($quad in $quads) {
            $g.FillPolygon($fgBrush, $quad)
        }
    }
    $fgBrush.Dispose()
    $g.Dispose()
    $bitmaps += $bmp
}

$icoPath = Join-Path $IconsDir 'icon.ico'
$out = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter $out
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$bitmaps.Count)
$pngBytes = @()
foreach ($bmp in $bitmaps) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngBytes += , $ms.ToArray()
    $ms.Dispose()
}
$dataOffset = 6 + 16 * $bitmaps.Count
for ($i = 0; $i -lt $bitmaps.Count; $i++) {
    $sz = $sizes[$i]
    $w = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([byte]$w); $bw.Write([byte]$w)
    $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$pngBytes[$i].Length)
    $bw.Write([uint32]$dataOffset)
    $dataOffset += $pngBytes[$i].Length
}
foreach ($data in $pngBytes) { $bw.Write($data) }
$bw.Flush(); $out.Close()
foreach ($bmp in $bitmaps) { $bmp.Dispose() }
Write-Host "wrote $icoPath ($((Get-Item $icoPath).Length) bytes)"

Write-Host ""
Write-Host "Done. Stale H-shape GIFs in src/assets/ (h-logo-*.gif) are kept" -ForegroundColor Yellow
Write-Host "around for upstream reference; remove them with:" -ForegroundColor Yellow
Write-Host "  Remove-Item C:\Code\Winthorpe\src\assets\h-logo-*.gif" -ForegroundColor Yellow
