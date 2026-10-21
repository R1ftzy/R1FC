[CmdletBinding()]
param(
    [string]$Source,
    [switch]$Dev,
    [switch]$NoRestart
)

$ErrorActionPreference = 'Stop'

if (-not $Source) {
    $base = $PSScriptRoot
    if (-not $base) {
        $base = (Get-Location).Path
    }

    $candidates = @(
        (Join-Path $base 'app'),
        (Join-Path $base 'dist\app'),
        $base
    )

    $Source = $candidates |
        Where-Object { Test-Path -LiteralPath (Join-Path $_ 'AppxManifest.xml') } |
        Select-Object -First 1
}

if (-not $Source) {
    throw 'Could not find AppxManifest.xml. Put the app folder next to install.ps1, or pass -Source <folder>.'
}

$identity = 'R1fc.Convert'
$sourcePath = (Resolve-Path -LiteralPath $Source -ErrorAction Stop).Path

$manifest = Join-Path $sourcePath 'AppxManifest.xml'
if (-not (Test-Path $manifest)) {
    throw "AppxManifest.xml was not found in $sourcePath. Run .\build.ps1 first."
}

$unlock = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -ErrorAction SilentlyContinue
if (-not $unlock -or $unlock.AllowDevelopmentWithoutDevLicense -ne 1) {
    Write-Warning 'Developer Mode does not appear to be enabled. Loose package registration may fail.'
}

$existing = Get-AppxPackage -Name $identity -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Removing existing package $($existing.PackageFullName)..."
    Remove-AppxPackage -Package $existing.PackageFullName
    Start-Sleep -Milliseconds 500
}

if (-not $NoRestart -and $existing) {
    Write-Host 'Stopping Explorer to release the old shell extension...'
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
}

if ($Dev) {
    $target = $sourcePath
} else {
    $installRoot = Join-Path $env:LOCALAPPDATA 'Programs\R1fc'
    $target = Join-Path $installRoot 'app'

    if (Test-Path $installRoot) {
        try {
            Remove-Item -Recurse -Force $installRoot
        } catch {
            throw "Could not replace $installRoot. Close File Explorer windows, restart Explorer, and try again. $($_.Exception.Message)"
        }
    }

    New-Item -ItemType Directory -Force -Path $target | Out-Null
    Copy-Item (Join-Path $sourcePath '*') $target -Recurse -Force
}

foreach ($required in @('r1fc-convert.exe', 'r1fc-shell.dll', 'Assets\r1fc.ico')) {
    if (-not (Test-Path (Join-Path $target $required))) {
        throw "The staged package is incomplete: $required is missing from $target"
    }
}

Write-Host "Registering $identity from $target..."
Add-AppxPackage -Register (Join-Path $target 'AppxManifest.xml') -ErrorAction Stop

$package = Get-AppxPackage -Name $identity
if (-not $package) {
    throw 'The package was not registered.'
}

Write-Host "Installed: $($package.PackageFullName)"
Write-Host "Location:  $($package.InstallLocation)"

if (-not $NoRestart) {
    Write-Host 'Restarting Explorer so the new context menu is loaded...'
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
        Start-Process explorer.exe
        Start-Sleep -Seconds 1
    }
    Write-Host 'Explorer restarted.'
} else {
    Write-Host 'Explorer was not restarted. Restart it manually to load the context menu.'
}
