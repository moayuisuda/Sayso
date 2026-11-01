# 用法：./scripts/build-windows.ps1 [-Test] [-Install] [-Version 1.2.0] [-InstallerCompiler path/to/ISCC.exe]
# -Version 写入程序版本号（发布流水线从 tag 传入），默认使用 Sayso.csproj 中的版本。
param([switch]$Install, [switch]$Test, [string]$Version, [string]$InstallerCompiler)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
if (Test-Path "$env:USERPROFILE/.dotnet/sdk") { $dotnet = "$env:USERPROFILE/.dotnet/dotnet.exe" }
elseif (-not $dotnet) { throw '请安装 .NET 10 SDK。' }
Push-Location $repo
try {
    & cargo build --manifest-path core/Cargo.toml -p sayso-windows-bridge --release --locked
    if ($LASTEXITCODE) { throw 'Rust core 构建失败。' }
    # @() 保证始终是数组：单元素数组从 if 表达式返回时会被展开成字符串，展开后按字符传参。
    $versionArgs = @(if ($Version) { "-p:Version=$Version" })
    & $dotnet publish windows/Sayso/Sayso.csproj -c Release -p:Platform=x64 -o build/windows @versionArgs
    if ($LASTEXITCODE) { throw 'Windows 客户端构建失败。' }
    if ($Test) {
        & cargo test --manifest-path core/Cargo.toml --workspace --locked
        if ($LASTEXITCODE) { throw 'Rust 测试失败。' }
        $report = "$repo/build/windows/smoke-results.json"
        if (Test-Path -LiteralPath $report) { Remove-Item -LiteralPath $report }
        $process = Start-Process "$repo/build/windows/Sayso.exe" -ArgumentList '--smoke-test' -PassThru -WindowStyle Hidden
        if (-not $process.WaitForExit(180000)) { throw 'Windows 集成测试超时。' }
        $results = Get-Content "$repo/build/windows/smoke-results.json" -Raw | ConvertFrom-Json
        $results | Format-Table name, passed, error
        if ($results | Where-Object { -not $_.passed }) { throw 'Windows 集成测试失败。' }
    }
    $installer = & "$PSScriptRoot/package-windows.ps1" -Compiler $InstallerCompiler
    if ($Test) { & "$PSScriptRoot/test-windows-installer.ps1" -Installer $installer }
    if ($Install) {
        $process = Start-Process -FilePath $installer -PassThru
        # 只等待安装向导；完成页启动的托盘应用可继续运行。
        $process.WaitForExit()
        if ($process.ExitCode) { throw "安装未完成，退出码：$($process.ExitCode)。" }
    }
    Write-Host "安装包：$installer"
} finally { Pop-Location }
