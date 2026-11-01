# 在真实系统上验证安装生命周期；只允许在未安装 Sayso 的账户中运行。
param([Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = 'Stop'
$uninstallKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/Sayso_is1'
$classKey = 'HKCU:/Software/Classes/Sayso.Extension'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Sayso/Sayso.lnk'
if ((Test-Path -LiteralPath $uninstallKey) -or (Test-Path -LiteralPath $classKey) -or (Test-Path -LiteralPath $shortcut)) {
    throw '安装包测试需要未安装 Sayso 的账户，避免覆盖现有安装。'
}

$repo = Split-Path $PSScriptRoot -Parent
$buildDirectory = [IO.Path]::GetFullPath((Join-Path $repo 'build'))
$testDirectory = Join-Path $buildDirectory ("installer-test-" + [guid]::NewGuid().ToString('N'))
$destination = Join-Path $testDirectory 'Sayso 安装测试'
$association = 'HKCU:/Software/Classes/.sayso'
$associationExisted = Test-Path -LiteralPath $association
$originalDefault = if ($associationExisted) { (Get-Item -LiteralPath $association).GetValue('') }
$dataDirectory = Join-Path $env:LOCALAPPDATA 'Sayso'
$dataExisted = Test-Path -LiteralPath $dataDirectory
$sentinel = Join-Path $dataDirectory ("installer-test-" + [guid]::NewGuid().ToString('N'))
$uninstaller = Join-Path $destination 'unins000.exe'

function Invoke-Installer([string]$Path, [string[]]$Arguments) {
    # 卸载器会派生临时进程删除自身，必须等待整个进程树，而非只等待入口进程。
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode) { throw "安装包测试失败，退出码 $($process.ExitCode)：$Path" }
}

function Assert-Installed {
    $registration = Get-ItemProperty -LiteralPath $uninstallKey
    $executable = Join-Path $destination 'Sayso.exe'
    if ($registration.DisplayVersion -ne (Get-Item -LiteralPath $executable).VersionInfo.ProductVersion) {
        throw '卸载入口与程序版本不一致。'
    }
    if ($registration.InstallLocation.TrimEnd('\') -ne $destination) { throw '安装位置注册错误。' }
    # WScript.Shell 的 WshShortcut 按 ANSI 代码页转换路径，非 ASCII 安装目录会被读成 '?'；Shell.Application 走 Unicode 接口。
    $folder = (New-Object -ComObject Shell.Application).NameSpace((Split-Path $shortcut -Parent))
    $link = $folder.ParseName((Split-Path $shortcut -Leaf)).GetLink
    if ($link.Path -ne $executable -or $link.WorkingDirectory -ne $destination) {
        throw "开始菜单快捷方式错误：目标 '$($link.Path)'，工作目录 '$($link.WorkingDirectory)'。"
    }
    $command = (Get-Item -LiteralPath "$classKey/shell/open/command").GetValue('')
    if ($command -ne ('"' + $executable + '" "%1"')) { throw '扩展包打开命令错误。' }
    if ((Get-Item -LiteralPath "$association/OpenWithProgids").GetValue('Sayso.Extension', $null) -ne '') {
        throw '扩展包打开方式未注册。'
    }
    # 逐文件核对真实发布目录，防止安装包遗漏 WinUI 或 Rust 原生依赖。
    $source = Join-Path $repo 'build/windows'
    foreach ($file in Get-ChildItem -LiteralPath $source -Recurse -File) {
        if ($file.Extension -eq '.pdb' -or $file.Name -eq 'smoke-results.json') { continue }
        $relativePath = $file.FullName.Substring($source.Length).TrimStart('\', '/')
        $installedFile = Join-Path $destination $relativePath
        if ((Get-FileHash -LiteralPath $file.FullName).Hash -ne (Get-FileHash -LiteralPath $installedFile).Hash) {
            throw "安装文件不一致：$relativePath"
        }
    }
    if (Get-ChildItem -LiteralPath $destination -Recurse -File -Include '*.pdb', 'smoke-results.json') {
        throw '安装包包含调试符号或测试报告。'
    }
}

New-Item -ItemType Directory -Path $testDirectory, $dataDirectory -Force | Out-Null
Set-Content -LiteralPath $sentinel -Value 'Sayso installer data retention check'
$passed = $false
try {
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/DIR=`"$destination`"")
    Invoke-Installer $Installer ($arguments + "/LOG=`"$testDirectory/install.log`"")
    Assert-Installed
    Write-Host '首次安装、依赖完整性、快捷方式和文件关联通过。'

    Invoke-Installer $Installer ($arguments + "/LOG=`"$testDirectory/reinstall.log`"")
    Assert-Installed
    Write-Host '覆盖安装通过。'

    Invoke-Installer $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$testDirectory/uninstall.log`"")
    if ((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $uninstallKey) -or
        (Test-Path -LiteralPath $classKey) -or (Test-Path -LiteralPath $shortcut)) { throw '卸载存在残留。' }
    if (-not $associationExisted -and (Test-Path -LiteralPath $association)) { throw '卸载留下了扩展名注册项。' }
    if ((Test-Path -LiteralPath "$association/OpenWithProgids") -and
        $null -ne (Get-Item -LiteralPath "$association/OpenWithProgids").GetValue('Sayso.Extension', $null)) {
        throw '卸载留下了扩展包打开方式。'
    }
    if ((Get-Content -LiteralPath $sentinel) -ne 'Sayso installer data retention check') { throw '卸载修改了用户数据。' }
    $currentDefault = if (Test-Path -LiteralPath $association) { (Get-Item -LiteralPath $association).GetValue('') }
    if ($currentDefault -ne $originalDefault) { throw '安装或卸载修改了用户的默认文件关联。' }
    Write-Host '卸载清理、用户数据和默认文件关联保留通过。'
    $passed = $true
} finally {
    if (Test-Path -LiteralPath $uninstaller) {
        Invoke-Installer $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    }
    Remove-Item -LiteralPath $sentinel
    if (-not $dataExisted -and -not (Get-ChildItem -LiteralPath $dataDirectory -Force)) {
        Remove-Item -LiteralPath $dataDirectory
    }
    # 只清理本脚本在工作区 build 目录内创建的独立测试目录。
    $resolvedTestDirectory = (Resolve-Path -LiteralPath $testDirectory).Path
    if (-not $resolvedTestDirectory.StartsWith($buildDirectory + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw '测试目录超出工作区 build，拒绝清理。'
    }
    if ($passed) { Remove-Item -LiteralPath $resolvedTestDirectory -Recurse -Force }
    else { Write-Host "安装包测试日志：$resolvedTestDirectory" }
}
