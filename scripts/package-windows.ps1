# 将已发布的客户端封装为安装包；版本号只从 Sayso.exe 读取。
param([string]$Compiler)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$source = Join-Path $repo 'build/windows'
if (-not $Compiler) {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    $Compiler = if ($command) { $command.Source } else {
        $installation = Get-ItemProperty -Path @(
            'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/Inno Setup 7_is1',
            'HKLM:/Software/Microsoft/Windows/CurrentVersion/Uninstall/Inno Setup 7_is1',
            'HKLM:/Software/WOW6432Node/Microsoft/Windows/CurrentVersion/Uninstall/Inno Setup 7_is1'
        ) -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($installation) { Join-Path $installation.InstallLocation 'ISCC.exe' }
    }
}
if (-not $Compiler) { throw '请安装 Inno Setup 7，或通过 -Compiler 指定 ISCC.exe。' }
foreach ($file in @('Sayso.exe', 'sayso_windows_bridge.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $source $file))) { throw "缺少 $file，请先构建 Windows 客户端。" }
}
$version = (Get-Item -LiteralPath (Join-Path $source 'Sayso.exe')).VersionInfo.ProductVersion
$output = Join-Path $repo 'build/installers'
$name = "Sayso-$version-windows-x64-Setup"
& $Compiler /Qp "/DAppVersion=$version" "/DPublishDir=$source" "/O$output" "/F$name" "$PSScriptRoot/windows-installer.iss" | Out-Host
if ($LASTEXITCODE) { throw 'Windows 安装包构建失败。' }
Join-Path $output "$name.exe"
