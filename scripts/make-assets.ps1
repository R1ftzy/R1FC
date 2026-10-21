[CmdletBinding()]
param(
    [string]$Root
)

$ErrorActionPreference = 'Stop'

if (-not $Root) {
    $Root = Split-Path -Parent $MyInvocation.MyCommand.Path
    $Root = Split-Path -Parent $Root
}

$assets = Join-Path $Root 'packaging\Assets'
$shell = Join-Path $Root 'src\shell'
$sourceSvg = Join-Path $assets 'icon.svg'

New-Item -ItemType Directory -Force -Path $assets, $shell | Out-Null

if (-not (Test-Path -LiteralPath $sourceSvg)) {
    throw "Missing icon source: $sourceSvg"
}

$magickCommand = Get-Command magick.exe -ErrorAction SilentlyContinue
if (-not $magickCommand) {
    throw 'ImageMagick (magick.exe) is required to generate the icons.'
}

$master = Join-Path ([System.IO.Path]::GetTempPath()) ("r1fc-icon-{0}.png" -f ([guid]::NewGuid().ToString('N')))

try {
    & $magickCommand.Source -background none -density 384 $sourceSvg -resize 1024x1024 -strip -depth 8 $master
    if ($LASTEXITCODE -ne 0) { throw 'Could not render icon.svg.' }

    & $magickCommand.Source $master -resize 256x256 -strip (Join-Path $assets 'r1fc-256.png')
    if ($LASTEXITCODE -ne 0) { throw 'Could not render r1fc-256.png.' }

    & $magickCommand.Source $master -define icon:auto-resize=256,128,64,48,32,24,20,16 (Join-Path $assets 'r1fc.ico')
    if ($LASTEXITCODE -ne 0) { throw 'Could not render r1fc.ico.' }

    & $magickCommand.Source $master -resize 50x50 -strip (Join-Path $assets 'StoreLogo.png')
    if ($LASTEXITCODE -ne 0) { throw 'Could not render StoreLogo.png.' }

    & $magickCommand.Source $master -resize 44x44 -strip (Join-Path $assets 'Square44x44Logo.png')
    if ($LASTEXITCODE -ne 0) { throw 'Could not render Square44x44Logo.png.' }

    & $magickCommand.Source $master -resize 150x150 -strip (Join-Path $assets 'Square150x150Logo.png')
    if ($LASTEXITCODE -ne 0) { throw 'Could not render Square150x150Logo.png.' }

    Copy-Item -LiteralPath (Join-Path $assets 'r1fc.ico') -Destination (Join-Path $shell 'icon.ico') -Force
}
finally {
    Remove-Item -LiteralPath $master -Force -ErrorAction SilentlyContinue
}

Get-ChildItem $assets | Select-Object Name, Length
