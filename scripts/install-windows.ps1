param([string]$Source = (Join-Path (Split-Path $PSScriptRoot -Parent) 'build/windows'))
$ErrorActionPreference = 'Stop'
$sourcePath = (Resolve-Path -LiteralPath $Source).Path
$destination = Join-Path $env:LOCALAPPDATA 'Programs/Sayso'
if (-not (Test-Path -LiteralPath (Join-Path $sourcePath 'Sayso.exe'))) { throw '请先运行 scripts/build-windows.ps1。' }
$running = Get-Process Sayso -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $destination 'Sayso.exe') }
if ($running) { throw '请先从托盘退出 Sayso，再安装。' }
New-Item -ItemType Directory -Path $destination -Force | Out-Null
Copy-Item -Path (Join-Path $sourcePath '*') -Destination $destination -Recurse -Force -Exclude 'smoke-results.json'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'uninstall-windows.ps1') -Destination $destination -Force
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) '言出 Sayso.lnk'))
$shortcut.TargetPath = Join-Path $destination 'Sayso.exe'
$shortcut.WorkingDirectory = $destination
$shortcut.IconLocation = Join-Path $destination 'Assets/Sayso.ico'
$shortcut.Save()
$uninstall = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/Sayso'
New-Item -Path $uninstall -Force | Out-Null
New-ItemProperty -Path $uninstall -Name DisplayName -Value '言出 Sayso' -Force | Out-Null
New-ItemProperty -Path $uninstall -Name DisplayVersion -Value '0.1.0' -Force | Out-Null
New-ItemProperty -Path $uninstall -Name InstallLocation -Value $destination -Force | Out-Null
New-ItemProperty -Path $uninstall -Name DisplayIcon -Value (Join-Path $destination 'Assets/Sayso.ico') -Force | Out-Null
$uninstallScript = Join-Path $destination 'uninstall-windows.ps1'
New-ItemProperty -Path $uninstall -Name UninstallString -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$uninstallScript`"" -Force | Out-Null
New-ItemProperty -Path $uninstall -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $uninstall -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null
New-Item -Path 'HKCU:/Software/Classes/.sayso' -Force | Out-Null
Set-Item -Path 'HKCU:/Software/Classes/.sayso' -Value 'Sayso.Extension'
New-Item -Path 'HKCU:/Software/Classes/Sayso.Extension/shell/open/command' -Force | Out-Null
Set-Item -Path 'HKCU:/Software/Classes/Sayso.Extension/shell/open/command' -Value ('"' + (Join-Path $destination 'Sayso.exe') + '" "%1"')
Write-Host "已安装：$destination"
Start-Process (Join-Path $destination 'Sayso.exe') -WindowStyle Hidden
