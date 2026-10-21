[CmdletBinding()]
param(
    [string]$Version,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if (-not $root) {
    $root = (Get-Location).Path
}

if (-not $Version) {
    $cargoToml = Join-Path $root 'src\worker\Cargo.toml'
    $match = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' |
        Select-Object -First 1
    if (-not $match) {
        throw 'Could not determine the version from src\worker\Cargo.toml.'
    }
    $Version = $match.Matches[0].Groups[1].Value
}

if ($Version -notmatch '^[0-9A-Za-z][0-9A-Za-z.-]*$') {
    throw "Invalid version string: $Version"
}

$distApp = Join-Path $root 'dist\app'
$distMsix = Join-Path $root 'dist\R1fcConvert.msix'

if (-not $SkipBuild) {
    & (Join-Path $root 'build.ps1')
    if ($LASTEXITCODE -ne 0) {
        throw 'build.ps1 failed.'
    }
}

foreach ($required in @(
    (Join-Path $distApp 'r1fc-convert.exe'),
    (Join-Path $distApp 'r1fc-shell.dll'),
    (Join-Path $distApp 'AppxManifest.xml'),
    (Join-Path $distApp 'Assets\r1fc.ico')
)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Missing build output: $required. Run .\build.ps1 first."
    }
}

$name = 'R1fcConvert-win11-x64'
$releaseRoot = Join-Path $root 'release'
$stage = Join-Path $releaseRoot $name
$zip = Join-Path $releaseRoot "$name.zip"
$msix = Join-Path $releaseRoot "$name.msix"

New-Item -ItemType Directory -Force -Path $releaseRoot | Out-Null

if (Test-Path -LiteralPath $stage) {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
if (Test-Path -LiteralPath $zip) {
    Remove-Item -LiteralPath $zip -Force
}
if (Test-Path -LiteralPath $msix) {
    Remove-Item -LiteralPath $msix -Force
}

New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $distApp -Destination (Join-Path $stage 'app') -Recurse

foreach ($file in @(
    'install.ps1',
    'uninstall.ps1',
    'Install.cmd',
    'Uninstall.cmd',
    'README.md'
)) {
    $source = Join-Path $root $file
    if (-not (Test-Path -LiteralPath $source)) {
        throw "Missing release file: $source"
    }
    Copy-Item -LiteralPath $source -Destination $stage
}

Set-Content -LiteralPath (Join-Path $stage 'VERSION.txt') -Encoding ascii -Value @(
    "R1fc Convert $Version",
    "Windows 11 x64"
)

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

if (Test-Path -LiteralPath $zip) {
    Remove-Item -LiteralPath $zip -Force
}

$archive = [System.IO.Compression.ZipFile]::Open(
    $zip,
    [System.IO.Compression.ZipArchiveMode]::Create
)
try {
    Get-ChildItem -LiteralPath $stage -Recurse -File |
        Sort-Object FullName |
        ForEach-Object {
            $relative = $_.FullName.Substring($stage.Length + 1).Replace('\', '/')
            $entryName = "$name/$relative"
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive,
                $_.FullName,
                $entryName,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
}
finally {
    $archive.Dispose()
}

$releaseNotes = @"
# R1fc Convert $Version

A per-user Windows 11 tool that adds **Convert with R1fc** to the modern File Explorer context menu. It converts images and videos with your installed ``ffmpeg.exe``.

## Install

1. Install ffmpeg: ``winget install Gyan.FFmpeg``
2. Enable Developer Mode (**Settings -> System -> For developers**) or sideloading.
3. Download ``$name.zip`` and extract it.
4. Run ``Install.cmd`` from the extracted folder.
5. Right-click one or more supported files, choose **Convert with R1fc**, then pick a target.

The release also contains ``$name.msix``. It is **unsigned** and cannot be installed directly; it is included for users who want to sign it with their own certificate. Use the zip for the normal per-user install.

## Highlights

- Native Windows 11 context menu with a flyout of valid targets.
- Per-user install under ``%LOCALAPPDATA%``; no elevation prompt once Developer Mode or sideloading is enabled.
- Multi-file selections are supported; existing files are never overwritten.
- Silent image conversions, with a native progress window and Cancel for video/large jobs.
- Settings in ``%LOCALAPPDATA%\R1fc\settings.json``; log in ``%LOCALAPPDATA%\R1fc\logs\convert.log``.
- Uninstall with ``Uninstall.cmd``.

## Supported formats

- Images: PNG, JPG/JPEG, WebP, BMP, TIFF, GIF, PPM family, ICO.
- Video: MP4, M4V, MOV, MKV, WebM, AVI.
- Targets: PNG, JPG, WebP, BMP, TIFF, GIF, MP4, WebM.

No ffmpeg is bundled.

## Checksums

See ``SHA256SUMS.txt``.
"@

Set-Content -LiteralPath (Join-Path $releaseRoot 'RELEASE_NOTES.md') -Value $releaseNotes -Encoding utf8

if (Test-Path -LiteralPath $distMsix) {
    Copy-Item -LiteralPath $distMsix -Destination $msix -Force
}

$sums = @()
foreach ($artifact in @($zip, $msix)) {
    if (Test-Path -LiteralPath $artifact) {
        $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
        $sums += "$hash  $(Split-Path -Leaf $artifact)"
    }
}
Set-Content -LiteralPath (Join-Path $releaseRoot 'SHA256SUMS.txt') -Value $sums -Encoding ascii

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
$entryCount = $archive.Entries.Count
$archive.Dispose()

Write-Host ''
Write-Host "Release folder: $releaseRoot"
Write-Host "Zip:            $zip"
Write-Host "Entries:        $entryCount"
if (Test-Path -LiteralPath $msix) {
    Write-Host "MSIX:           $msix (unsigned)"
}
Write-Host "Checksums:      $(Join-Path $releaseRoot 'SHA256SUMS.txt')"
Write-Host "Release notes:  $(Join-Path $releaseRoot 'RELEASE_NOTES.md')"
