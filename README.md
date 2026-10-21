# R1FConvert

Windows 11 context-menu image and video converter using installed ffmpeg.

## Requirements

- Windows 11 x64
- Installed `ffmpeg.exe` (`winget install Gyan.FFmpeg`)
- Developer Mode or sideloading for installation
- Build: Rust/Cargo, CMake 3.20+, Visual Studio 2022+ with **Desktop development with C++** and the Windows 11 SDK, PowerShell 5.1+
- Icons: ImageMagick

## Build

```powershell
.\build.ps1
```

Stages `dist\app` and validates `dist\R1fcConvert.msix`.

## Install

```powershell
.\install.ps1
```

From a release zip, run `Install.cmd`.

Development install:

```powershell
.\install.ps1 -Dev
```

## Run

Right-click one or more supported files -> **Convert with R1fc** -> choose a target.

## Uninstall

```powershell
.\uninstall.ps1
```

From a release zip, run `Uninstall.cmd`.

## Release

```powershell
.\scripts\make-release.ps1
```

Creates `release\R1fcConvert-win11-x64.zip` and an unsigned `.msix`.

## Icons

```powershell
.\scripts\make-assets.ps1
```

## Test

```powershell
.\src\worker\target\release\r1fc-convert.exe --self-test
```

## Formats

- Images: PNG, JPG, WebP, BMP, TIFF, GIF, PPM, ICO
- Video: MP4, M4V, MOV, MKV, WebM, AVI
- Targets: PNG, JPG, WebP, BMP, TIFF, GIF, MP4, WebM

## Files

- Settings: `%LOCALAPPDATA%\R1fc\settings.json`
- Log: `%LOCALAPPDATA%\R1fc\logs\convert.log`
