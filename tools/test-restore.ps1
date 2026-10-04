# P2-02/P4-05 还原率自测脚本（M2：100 次 >= 95%；M12：1000 次 >= 99.9%）。
#
# 调用还原自测二进制：造样本 -> 移入隔离区 -> 还原 -> sha256 比对。
# 全程在临时沙箱内进行，不触碰真实用户数据 / `%LOCALAPPDATA%`。
# stdout 为 JSON 报告（含失败项兜底指引），可重定向落 docs/verify/restore-1000.json。
#
# 用法（PowerShell）：
#   .\tools\test-restore.ps1                              # 默认 100 次，95% 门禁
#   .\tools\test-restore.ps1 -Cycles 1000 -MinRatio 0.999 # M12 千次门禁
#   .\tools\test-restore.ps1 -Cycles 1000 -MinRatio 0.999 -Release  # 门禁复核（release）
param(
    [int]$Cycles = 100,
    [double]$MinRatio = 0.95,
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$bin = "restore-self-test"

Write-Host "[test-restore] 还原率自测开始（$Cycles 次，阈值=$MinRatio，构建=$(if ($Release) {'release'} else {'debug'})）..."

$cargoArgs = @()
if ($Release) { $cargoArgs += "--release" }
$cargoArgs += @("--bin", $bin, "--quiet", "--")
$cargoArgs += [string]$Cycles
$cargoArgs += $MinRatio.ToString([System.Globalization.CultureInfo]::InvariantCulture)

Push-Location "$root\src-tauri"
try {
    cargo run @cargoArgs
    $exit = $LASTEXITCODE
} finally {
    Pop-Location
}

if ($exit -ne 0) {
    Write-Host "[test-restore] 失败：还原率未达 $MinRatio 门禁（或构建失败）。失败项兜底指引见 JSON 报告 failures[].guidance。"
    exit 1
}
Write-Host "[test-restore] 通过：还原率 >= $MinRatio（$(if ($Cycles -ge 1000) {'M12 千次门禁'} else {'M2 百次门禁'})）。"
exit 0