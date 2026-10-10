# Draws src-tauri/icons/icon.ico: a green disc with a white "Z", 256 px, stored as PNG inside the
# ICO (Windows Vista and later read that). Run once; the result is committed.
Add-Type -AssemblyName System.Drawing

$size = 256
$bitmap = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bitmap)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$g.Clear([System.Drawing.Color]::Transparent)
$green = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(46, 184, 92))
$g.FillEllipse($green, 8, 8, 240, 240)
$font = New-Object System.Drawing.Font 'Segoe UI', 150, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
$format = New-Object System.Drawing.StringFormat
$format.Alignment = [System.Drawing.StringAlignment]::Center
$format.LineAlignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('Z', $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF 0, 0, $size, $size), $format)
$g.Dispose()

$png = New-Object System.IO.MemoryStream
$bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$bytes = $png.ToArray()

$out = Join-Path $PSScriptRoot '..\src-tauri\icons\icon.ico'
New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
$writer = New-Object System.IO.BinaryWriter ([System.IO.File]::Create($out))
$writer.Write([UInt16]0)               # ICONDIR: reserved
$writer.Write([UInt16]1)               # type: icon
$writer.Write([UInt16]1)               # one image
$writer.Write([Byte]0)                 # width 256 is written as 0
$writer.Write([Byte]0)                 # height 256 is written as 0
$writer.Write([Byte]0)                 # no palette
$writer.Write([Byte]0)                 # reserved
$writer.Write([UInt16]1)               # planes
$writer.Write([UInt16]32)              # bits per pixel
$writer.Write([UInt32]$bytes.Length)   # image size
$writer.Write([UInt32]22)              # image offset: 6 + 16
$writer.Write($bytes)
$writer.Close()
Write-Output "written $out"
