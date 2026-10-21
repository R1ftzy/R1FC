[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [string]$OutFile,

    [int]$SettleMs = 2000,
    [int]$MenuWaitMs = 2500,

    [switch]$RightClick
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class R1fcNative {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder text, int count);
}
'@

$source = (Resolve-Path -LiteralPath $Path).Path
$itemInfo = Get-Item -LiteralPath $source
$isFile = -not $itemInfo.PSIsContainer
$folder = if ($isFile) { Split-Path -Parent $source } else { $source }
$fileName = if ($isFile) { $itemInfo.Name } else { Split-Path -Leaf $source }

if ($isFile) {
    Start-Process explorer.exe -ArgumentList ('/select,"{0}"' -f $source)
} else {
    Start-Process explorer.exe -ArgumentList ('"{0}"' -f $source)
}
Start-Sleep -Milliseconds $SettleMs

function Get-ForegroundTitle {
    $handle = [R1fcNative]::GetForegroundWindow()
    $builder = New-Object System.Text.StringBuilder 512
    [void][R1fcNative]::GetWindowText($handle, $builder, $builder.Capacity)
    return $builder.ToString()
}

Write-Output "foreground before: $(Get-ForegroundTitle)"

$root = [System.Windows.Automation.AutomationElement]::RootElement
$windows = $root.FindAll(
    [System.Windows.Automation.TreeScope]::Children,
    [System.Windows.Automation.Condition]::TrueCondition)

$target = $null
foreach ($window in $windows) {
    $name = $window.Current.Name
    $class = $window.Current.ClassName
    if ($name -like "*$fileName*" -or ($isFile -and $name -like "*$folder*")) {
        if ($class -eq 'CabinetWClass' -or $name -like '*File Explorer*') {
            $target = $window
            break
        }
    }
}

if (-not $target) {
    foreach ($window in $windows) {
        if ($window.Current.ClassName -eq 'CabinetWClass') {
            $target = $window
            break
        }
    }
}

if (-not $target) {
    throw 'Could not find the File Explorer window through UI Automation.'
}

Write-Output "explorer window: '$($target.Current.Name)' [$($target.Current.ClassName)]"

$nameCondition = New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty,
    $fileName)
$item = $target.FindFirst(
    [System.Windows.Automation.TreeScope]::Descendants,
    $nameCondition)

if (-not $item) {
    throw "Could not find the file item '$fileName' in the Explorer window."
}

$rect = $item.Current.BoundingRectangle
Write-Output "item: '$($item.Current.Name)' type=$($item.Current.ControlType.ProgrammaticName) rect=$rect"

try {
    $item.SetFocus()
    Start-Sleep -Milliseconds 400
} catch {
    Write-Warning "SetFocus failed: $($_.Exception.Message)"
}

Write-Output "foreground after focus: $(Get-ForegroundTitle)"

if ($RightClick) {
    $x = [int]($rect.Left + $rect.Width / 2)
    $y = [int]($rect.Top + $rect.Height / 2)
    if ($x -le 0 -or $y -le 0) {
        throw "The file item has an invalid bounding rectangle: $rect"
    }

    [void][R1fcNative]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 200
    [R1fcNative]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [R1fcNative]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
} else {
    [System.Windows.Forms.SendKeys]::SendWait('+{F10}')
}

Start-Sleep -Milliseconds $MenuWaitMs
Write-Output "foreground after menu: $(Get-ForegroundTitle)"

$bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$graphics.Dispose()

$directory = Split-Path -Parent $OutFile
if ($directory -and -not (Test-Path $directory)) {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
}

$bitmap.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()

Write-Output "saved: $OutFile ($($bounds.Width)x$($bounds.Height))"
