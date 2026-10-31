$ErrorActionPreference = 'Stop'
$destination = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs/Sayso'))
if ([IO.Path]::GetFullPath($PSScriptRoot) -ne $destination) { throw '请从已安装的 Sayso 目录运行卸载。' }
if (Get-Process Sayso -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $destination 'Sayso.exe') }) { throw '请先从托盘退出 Sayso。' }
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) '言出 Sayso.lnk'
if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut }
foreach ($key in @('HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/Sayso', 'HKCU:/Software/Classes/Sayso.Extension')) {
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse }
}
$association = 'HKCU:/Software/Classes/.sayso'
if (Test-Path -LiteralPath $association) {
    if ((Get-Item -LiteralPath $association).GetValue('') -eq 'Sayso.Extension') { Remove-Item -LiteralPath $association }
}
# 目标已核验为当前用户的安装目录；用户设置和凭据保留在安装目录之外。
Set-Location $env:TEMP
Remove-Item -LiteralPath $destination -Recurse -Force
Write-Host 'Sayso 已卸载，设置与扩展数据已保留。'
