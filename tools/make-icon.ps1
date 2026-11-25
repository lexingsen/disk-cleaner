# Generates assets/disk-cleaner.ico (multi-size) for the disk-cleaner app.
# Design: blue rounded tile, white hard-drive glyph with a blue slot + LED,
# plus a white sparkle in the upper right ("clean").
Add-Type -AssemblyName System.Drawing

$outDir = Join-Path $PSScriptRoot '..\assets'
$outDir = [System.IO.Path]::GetFullPath($outDir)
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function New-RoundedPath([double]$x, [double]$y, [double]$w, [double]$h, [double]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    if ($d -gt $w) { $d = $w }
    if ($d -gt $h) { $d = $h }
    $p.AddArc([single]($x), [single]($y), [single]$d, [single]$d, 180, 90)
    $p.AddArc([single]($x + $w - $d), [single]($y), [single]$d, [single]$d, 270, 90)
    $p.AddArc([single]($x + $w - $d), [single]($y + $h - $d), [single]$d, [single]$d, 0, 90)
    $p.AddArc([single]($x), [single]($y + $h - $d), [single]$d, [single]$d, 90, 90)
    $p.CloseFigure()
    return $p
}

function New-SparklePath([double]$cx, [double]$cy, [double]$outer, [double]$inner) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $pts = New-Object 'System.Collections.Generic.List[System.Drawing.PointF]'
    for ($i = 0; $i -lt 8; $i++) {
        $ang = (-90 + 45 * $i) * [Math]::PI / 180
        $r = if ($i % 2 -eq 0) { $outer } else { $inner }
        $pts.Add((New-Object System.Drawing.PointF([single]($cx + $r * [Math]::Cos($ang)), [single]($cy + $r * [Math]::Sin($ang)))))
    }
    $p.AddPolygon($pts.ToArray())
    $p.CloseFigure()
    return $p
}

function New-IconBitmap([int]$S) {
    $bmp = New-Object System.Drawing.Bitmap($S, $S, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $f = [double]$S

    # Tile
    $pad = 0.02 * $f
    $tile = New-RoundedPath $pad $pad ($f - 2 * $pad) ($f - 2 * $pad) (0.22 * $f)
    $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF(0, 0)),
        (New-Object System.Drawing.PointF([single]$f, [single]$f)),
        [System.Drawing.Color]::FromArgb(255, 62, 156, 255),
        [System.Drawing.Color]::FromArgb(255, 10, 82, 210))
    $g.FillPath($grad, $tile)

    # Drive body (white rounded rect)
    $dx = 0.17 * $f; $dy = 0.425 * $f; $dw = 0.66 * $f; $dh = 0.285 * $f
    $drive = New-RoundedPath $dx $dy $dw $dh (0.06 * $f)
    $white = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $g.FillPath($white, $drive)

    # Slot + LED cut-outs (blue)
    $blue = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 16, 96, 224))
    $slot = New-RoundedPath (0.25 * $f) (0.475 * $f) (0.30 * $f) (0.045 * $f) (0.0225 * $f)
    $g.FillPath($blue, $slot)
    $led = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 16, 96, 224))
    $ledR = 0.045 * $f
    $g.FillEllipse($led, [single](0.665 * $f), [single](0.555 * $f), [single]($ledR * 2), [single]($ledR * 2))

    # Sparkle
    $spark = New-SparklePath (0.735 * $f) (0.255 * $f) (0.155 * $f) (0.05 * $f)
    $g.FillPath($white, $spark)

    $g.Dispose()
    return $bmp
}

# Build a 32bpp ICONIMAGE blob (BITMAPINFOHEADER + BGRA XOR bitmap + 1bpp AND mask)
# straight from the bitmap. Going through Icon.FromHandle/Icon.Save downsampled the
# image to 4bpp and wrecked the gradients.
function Get-IconImageData([System.Drawing.Bitmap]$bmp) {
    $W = $bmp.Width; $H = $bmp.Height
    $rect = New-Object System.Drawing.Rectangle(0, 0, $W, $H)
    $locked = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $stride = $locked.Stride
    $raw = New-Object byte[] ($stride * $H)
    [System.Runtime.InteropServices.Marshal]::Copy($locked.Scan0, $raw, 0, $raw.Length)
    $bmp.UnlockBits($locked)

    $xorSize = $W * $H * 4
    $maskStride = [int]([Math]::Floor(($W + 31) / 32) * 4)
    $maskSize = $maskStride * $H
    $out = New-Object byte[] (40 + $xorSize + $maskSize)

    # BITMAPINFOHEADER
    [Array]::Copy([BitConverter]::GetBytes([int]40), 0, $out, 0, 4)
    [Array]::Copy([BitConverter]::GetBytes([int]$W), 0, $out, 4, 4)
    [Array]::Copy([BitConverter]::GetBytes([int](2 * $H)), 0, $out, 8, 4)  # height doubled: XOR + AND
    [Array]::Copy([BitConverter]::GetBytes([uint16]1), 0, $out, 12, 2)     # planes
    [Array]::Copy([BitConverter]::GetBytes([uint16]32), 0, $out, 14, 2)    # bit count
    # compression/size/image/xppm/yppm/clrused/clrimportant stay 0

    $maskBase = 40 + $xorSize
    for ($y = 0; $y -lt $H; $y++) {
        $srcRow = ($H - 1 - $y) * $stride        # DIB rows are bottom-up
        $dst = 40 + $y * $W * 4
        [Array]::Copy($raw, $srcRow, $out, $dst, $W * 4)
        $mRow = $maskBase + $y * $maskStride
        for ($x = 0; $x -lt $W; $x++) {
            if ($raw[$srcRow + $x * 4 + 3] -lt 128) {
                $mi = $mRow + ($x -shr 3)
                $out[$mi] = $out[$mi] -bor (0x80 -shr ($x -band 7))
            }
        }
    }
    return , $out
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
$images = @()
foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    $images += , (Get-IconImageData $bmp)
    if ($s -eq 256) { $bmp.Save((Join-Path $outDir 'icon-256.png'), [System.Drawing.Imaging.ImageFormat]::Png) }
    if ($s -eq 32) { $bmp.Save((Join-Path $outDir 'icon-32.png'), [System.Drawing.Imaging.ImageFormat]::Png) }
    $bmp.Dispose()
}

$icoPath = Join-Path $outDir 'disk-cleaner.ico'
$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$images.Count)
$offset = 6 + 16 * $images.Count
for ($i = 0; $i -lt $images.Count; $i++) {
    $s = $sizes[$i]
    $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s }))
    $bw.Write([byte]$(if ($s -ge 256) { 0 } else { $s }))
    $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$images[$i].Length)
    $bw.Write([uint32]$offset)
    $offset += $images[$i].Length
}
foreach ($img in $images) { $bw.Write($img) }
$bw.Flush(); $bw.Dispose(); $fs.Dispose()

$len = (Get-Item $icoPath).Length
"ico written: $icoPath ($len bytes), sizes: $($sizes -join ',')"
