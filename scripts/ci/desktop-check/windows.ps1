# Checks a released Windows installer on a real Windows machine (CI runner):
# silent per-user install, the Apps entry, launch in light and dark with screenshots,
# the tray icon, and a silent uninstall that leaves nothing behind.
#
#   windows.ps1 -Installer Teitunnel_0.1.0_x64-setup.exe -Out shots
param(
  [Parameter(Mandatory)] [string] $Installer,
  [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
$failures = [System.Collections.Generic.List[string]]::new()
function Fail($message) { Write-Host "::error::$message"; $failures.Add($message) }

function Save-Screen($name) {
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
  $bitmap.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $graphics.Dispose(); $bitmap.Dispose()
}

function Set-Theme([bool] $dark) {
  $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
  New-Item -Force -Path $key | Out-Null
  $value = if ($dark) { 0 } else { 1 }
  Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value $value -Type DWord
  Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value $value -Type DWord
}

function Find-Tray {
  # The notification area and its overflow; names come from the tray tooltip.
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $names = foreach ($class in 'Shell_TrayWnd', 'NotifyIconOverflowWindow', 'TopLevelWindowForOverflowXamlIsland') {
    $condition = New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::ClassNameProperty, $class)
    $window = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $condition)
    if ($window) {
      $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition) |
        ForEach-Object { $_.Current.Name } | Where-Object { $_ }
    }
  }
  $names | Sort-Object -Unique
}

function Start-App($exe, $name) {
  $process = Start-Process -FilePath $exe -PassThru
  Start-Sleep -Seconds 12
  if ($process.HasExited) { Fail "Teitunnel exited on launch ($name), code $($process.ExitCode)" }
  Save-Screen $name
  $process
}

function Stop-App {
  Get-Process -Name Teitunnel -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Seconds 2
}

$os = Get-CimInstance Win32_OperatingSystem
Write-Host "Windows: $($os.Caption) $($os.Version) ($env:PROCESSOR_ARCHITECTURE)"

# Install for this user, silently, as winget does.
$install = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($install.ExitCode -ne 0) { Fail "Installer exited with $($install.ExitCode)" }
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Teitunnel'
$entry = Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue
if (-not $entry) { throw "No Apps entry at $uninstallKey" }
$entry | Select-Object DisplayName, DisplayVersion, Publisher, InstallLocation, UninstallString |
  Format-List | Out-String | Tee-Object -FilePath (Join-Path $Out 'apps-entry.txt') | Write-Host
$dir = $entry.InstallLocation.Trim('"')
$exe = Join-Path $dir 'Teitunnel.exe'
if (-not (Test-Path $exe)) { Fail "No Teitunnel.exe in $dir" }
if (-not (Test-Path (Join-Path $dir 'teitunnel-cli.exe'))) { Fail 'teitunnel-cli.exe is not installed' }
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Teitunnel.lnk'
if (-not (Test-Path $shortcut)) { Fail 'No Start menu shortcut' }

Set-Theme $false
Start-App $exe 'light' | Out-Null
$tray = Find-Tray
$tray | Out-File (Join-Path $Out 'tray.txt')
if (-not ($tray -match 'Teitunnel')) { Write-Host '::warning::No Teitunnel icon found in the notification area' }
Stop-App

Set-Theme $true
Start-App $exe 'dark' | Out-Null
Stop-App
Set-Theme $false

# Uninstall silently; the app's folder and entry must be gone.
$uninstaller = $entry.UninstallString.Trim('"')
Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null
Start-Sleep -Seconds 5
if (Test-Path $exe) { Fail "Still installed after uninstall: $exe" }
if (Test-Path $uninstallKey) { Fail 'The Apps entry is still there after uninstall' }
if (Test-Path $shortcut) { Fail 'The Start menu shortcut is still there after uninstall' }

if ($failures.Count -gt 0) { exit 1 }
Write-Host 'Windows desktop check passed'
