# P2-07 黄金文件集误删率自测包装脚本（M2 门禁）。
#
# 调用 golden-misdelete-test 二进制：构造安全/危险样本树 30+ 项，
# 用真实规则引擎只读扫描沙箱，断言误删率分档（[绿]<0.5% / [黄]<2% / [红]=0）。
# 全程沙箱，绝不触碰真实用户数据 / 系统盘。
#
# 注意：本脚本不写 emoji，避免 PowerShell 5.1 在无 BOM UTF-8 下误读导致解析失败。
#
# 用法（PowerShell）：
#   .\tools\test-misdelete.ps1              # debug 构建
#   .\tools\test-misdelete.ps1 -Release     # release 构建（门禁复核用）
param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = "golden-misdelete-test"

Write-Host "[test-misdelete] golden 误删率自测开始（构建=$(if ($Release) {'release'} else {'debug'})）..."

$cargoArgs = @()
if ($Release) { $cargoArgs += "--release" }
$cargoArgs += @("--bin", $bin, "--quiet")

Push-Location "$root\src-tauri"
try {
    cargo run @cargoArgs
    $exit = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($exit -ne 0) {
    Write-Host "[test-misdelete] 失败：误删率未达分档门禁（或构建失败）。"
    exit 1
}
Write-Host "[test-misdelete] 通过：误删率分档达标（绿<0.5% / 黄<2% / 红=0）。"
exit 0