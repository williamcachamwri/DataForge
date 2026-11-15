#requires -Version 5.1
<# Generates web/public/og-image.png (1200x630) with System.Drawing. Run from anywhere. #>
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$logoPath = Join-Path $root "public\images\tableminus_logo.png"
$out = Join-Path $root "public\og-image.png"
$W = 1200; $H = 630

$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.TextRenderingHint = 'AntiAliasGridFit'
$g.InterpolationMode = 'HighQualityBicubic'

# Background: near-black with a soft blue glow.
$g.Clear([System.Drawing.ColorTranslator]::FromHtml("#0C0D10"))
$glow = New-Object System.Drawing.Drawing2D.GraphicsPath
$glow.AddEllipse(-200, -300, 1000, 800)
$pgb = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
$pgb.CenterColor = [System.Drawing.Color]::FromArgb(70, 10, 132, 255)
$pgb.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 10, 132, 255))
$g.FillPath($pgb, $glow)

# Technical grid: hairlines and corner crosshairs.
$line = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(40, 255, 255, 255)), 1
foreach ($y in 56, 574) { $g.DrawLine($line, 0, $y, $W, $y) }
foreach ($x in 72, 1128) { $g.DrawLine($line, $x, 0, $x, $H) }
$cross = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(150, 255, 255, 255)), 1
foreach ($p in @(@(72,56), @(1128,56), @(72,574), @(1128,574))) {
    $g.DrawLine($cross, $p[0]-6, $p[1], $p[0]+6, $p[1])
    $g.DrawLine($cross, $p[0], $p[1]-6, $p[0], $p[1]+6)
}

# Logo and wordmark.
$logo = [System.Drawing.Image]::FromFile($logoPath)
$g.DrawImage($logo, 112, 108, 88, 88)
$white = [System.Drawing.Brushes]::White
$muted = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml("#9AA0AB"))
$accent = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml("#3B9BFF"))
$fWord = New-Object System.Drawing.Font "Segoe UI Semibold", 40, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$g.DrawString("TableMinus", $fWord, $white, 216, 128)

# Headline.
$fH = New-Object System.Drawing.Font "Segoe UI Semibold", 68, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$g.DrawString("Native SQL & database", $fH, $white, 108, 232)
$g.DrawString("workbench for Windows.", $fH, $white, 108, 312)

# Tagline and stats.
$fS = New-Object System.Drawing.Font "Segoe UI", 28, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$g.DrawString("Built in Rust  -  Direct3D 11  -  9 database drivers", $fS, $muted, 112, 418)
$fT = New-Object System.Drawing.Font "Consolas", 24, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$g.DrawString("~10 ms cold start   ~55 MB RAM   Setup.exe + Portable.zip", $fT, $accent, 112, 488)

$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$logo.Dispose(); $g.Dispose(); $bmp.Dispose()
Write-Host "Wrote $out"
