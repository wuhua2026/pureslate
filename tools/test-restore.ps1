# P2-02 还原率自测脚本（M2 门禁：100 次循环还原率 >= 95%）。
#
# 调用还原自测二进制：造样本 -> 移入隔离区 -> 还原 -> sha256 比对。
# 全程在临时沙箱内进行，不触碰真实用户数据 / `%LOCALAPPDATA%`。
#
# 用法（PowerShell）：
#   .\tools\test-restore.ps1            # 默认 100 次（debug 构建）
#   .\tools\test-restore.ps1 -Cycles 300
#   .\tools\test-restore.ps1 -Release   # release 构建（门禁复核用）
param(
    [int]$Cycles = 100,
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = "restore-self-test"

Write-Host "[test-restore] 还原率自测开始（$Cycles 次，构建=$(if ($Release) {'release'} else {'debug'})）..."

$cargoArgs = @()
if ($Release) { $cargoArgs += "--release" }
$cargoArgs += @("--bin", $bin, "--quiet", "--")
$cargoArgs += [string]$Cycles

Push-Location "$root\src-tauri"
try {
    cargo run @cargoArgs
    $exit = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($exit -ne 0) {
    Write-Host "[test-restore] 失败：还原率未达 95% 门禁（或构建失败）。"
    exit 1
}
Write-Host "[test-restore] 通过：还原率 >= 95%。"
exit 0