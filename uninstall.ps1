[CmdletBinding()]
param(
    [switch]$NoRestart,
    [switch]$PurgeSettings
)

$ErrorActionPreference = 'Stop'
$identity = 'R1fc.Convert'
$package = Get-AppxPackage -Name $identity -ErrorAction SilentlyContinue

if ($package) {
    Write-Host "Removing $($package.PackageFullName)..."
    try {
        Remove-AppxPackage -Package $package.PackageFullName
    } catch {
        Write-Host 'Removal failed; stopping Explorer and retrying...'
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 1
        Remove-AppxPackage -Package $package.PackageFullName
    }
}

$installRoot = Join-Path $env:LOCALAPPDATA 'Programs\R1fc'
if (Test-Path $installRoot) {
    Remove-Item -Recurse -Force $installRoot -ErrorAction SilentlyContinue
}

if ($PurgeSettings) {
    $settingsRoot = Join-Path $env:LOCALAPPDATA 'R1fc'
    if (Test-Path $settingsRoot) {
        Remove-Item -Recurse -Force $settingsRoot -ErrorAction SilentlyContinue
    }
}

if (-not $NoRestart) {
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
        Start-Process explorer.exe
    }
}

Write-Host 'R1fc Convert was uninstalled.'
