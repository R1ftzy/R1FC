[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',

    [switch]$SkipWorker,
    [switch]$SkipShell,
    [switch]$NoPackage
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

$workerManifest = Join-Path $root 'src\worker\Cargo.toml'
$shellSource = Join-Path $root 'src\shell'
$shellBuild = Join-Path $root 'build\shell'
$distApp = Join-Path $root 'dist\app'

if (-not $SkipWorker) {
    $cargo = Get-Command cargo.exe -ErrorAction Stop
    Write-Host 'Building Rust worker...'
    if ($Configuration -eq 'Release') {
        & $cargo.Source build --release --manifest-path $workerManifest
    } else {
        & $cargo.Source build --manifest-path $workerManifest
    }
    if ($LASTEXITCODE -ne 0) {
        throw 'cargo build failed.'
    }
}

if (-not $SkipShell) {
    $cmake = Get-Command cmake.exe -ErrorAction Stop
    $cmakeHelp = & $cmake.Source --help
    $generator = if ($cmakeHelp -match 'Visual Studio 18 2026') {
        'Visual Studio 18 2026'
    } elseif ($cmakeHelp -match 'Visual Studio 17 2022') {
        'Visual Studio 17 2022'
    } else {
        throw 'No supported Visual Studio CMake generator was found.'
    }

    Write-Host "Building shell extension with $generator..."
    & $cmake.Source -S $shellSource -B $shellBuild -G $generator -A x64
    if ($LASTEXITCODE -ne 0) {
        throw 'CMake configure failed.'
    }

    & $cmake.Source --build $shellBuild --config $Configuration
    if ($LASTEXITCODE -ne 0) {
        throw 'CMake build failed.'
    }
}

$configurationFolder = $Configuration.ToLowerInvariant()
$workerExe = Join-Path $root "src\worker\target\$configurationFolder\r1fc-convert.exe"
$shellDll = Join-Path $shellBuild "$Configuration\r1fc-shell.dll"
$manifest = Join-Path $root 'packaging\AppxManifest.xml'
$assets = Join-Path $root 'packaging\Assets'

foreach ($required in @($workerExe, $shellDll, $manifest, $assets)) {
    if (-not (Test-Path $required)) {
        throw "Missing build output: $required"
    }
}

if (Test-Path $distApp) {
    Remove-Item -Recurse -Force $distApp
}
New-Item -ItemType Directory -Force -Path $distApp | Out-Null

Copy-Item $workerExe $distApp
Copy-Item $shellDll $distApp
Copy-Item $manifest $distApp
Copy-Item $assets (Join-Path $distApp 'Assets') -Recurse

Write-Host "Staged application: $distApp"

if (-not $NoPackage) {
    $kitsRoots = @(
        'C:\Program Files (x86)\Windows Kits\10\bin',
        'C:\Program Files\Windows Kits\10\bin'
    )

    $makeappx = $null
    foreach ($kitsRoot in $kitsRoots) {
        if (-not (Test-Path $kitsRoot)) { continue }
        $makeappx = Get-ChildItem $kitsRoot -Directory |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName 'x64\makeappx.exe' } |
            Where-Object { Test-Path $_ } |
            Select-Object -First 1
        if ($makeappx) { break }
    }

    if ($makeappx) {
        $msix = Join-Path $root 'dist\R1fcConvert.msix'
        Write-Host 'Validating package with makeappx...'
        & $makeappx pack /d $distApp /p $msix /o
        if ($LASTEXITCODE -ne 0) {
            throw 'makeappx validation failed.'
        }
        Write-Host "Package validated: $msix"
    } else {
        Write-Warning 'makeappx.exe was not found; package validation was skipped.'
    }
}

Write-Host ''
Write-Host 'Build complete.'
Write-Host "Install with:  .\install.ps1"
Write-Host "Dev install:   .\install.ps1 -Dev"
