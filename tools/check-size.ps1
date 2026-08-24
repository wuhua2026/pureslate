<#
.SYNOPSIS
  PureSlate 安装包体积断言脚本。SPEC §1 体积预算：安装包 <20MB。
.DESCRIPTION
  解析打包产物（NSIS .exe 优先），找到最大安装包并断言 <20MB。
  通过则 exit 0 并写体积报告 JSON；失败则 exit 1 并退出。
.EXAMPLE
  .\tools\check-size.ps1 -SearchPath "src-tauri/target/release/bundle" -ResultFile "docs/verify/ci-size-report.json"
#>
param(
    [string]$SearchPath = "src-tauri/target/release/bundle",
    [string]$ResultFile = "docs/verify/size-report.json",
    [long]$LimitBytes = 20MB
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $SearchPath)) {
    Write-Error "产物目录不存在: $SearchPath"
    exit 1
}

# 优先 NSIS .exe 安装包；无则退回找最大 *.exe / *.msi
$installers = Get-ChildItem -Path $SearchPath -Recurse -Filter "*.exe" -File -ErrorAction SilentlyContinue
if (-not $installers) {
    $installers = Get-ChildItem -Path $SearchPath -Recurse -Include "*.msi" -File -ErrorAction SilentlyContinue
}

if (-not $installers) {
    Write-Error "未找到任何安装包产物"
    exit 1
}

$largest = $installers | Sort-Object Length -Descending | Select-Object -First 1
$ok = $largest.Length -le $LimitBytes

$report = [ordered]@{
    "ts"        = (Get-Date -Format o)
    "installer" = $largest.FullName
    "sizeBytes" = $largest.Length
    "sizeMB"    = [math]::Round($largest.Length / 1MB, 2)
    "limitMB"   = 20
    "pass"      = $ok
} | ConvertTo-Json

$outDir = Split-Path -Parent $ResultFile
if ($outDir) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }
$report | Out-File -FilePath $ResultFile -Encoding utf8
Write-Host ("体积报告已写入: {0}" -f $ResultFile)
Write-Host ("安装包: {0}  ({1} MB)" -f $largest.Name, [math]::Round($largest.Length / 1MB, 2))
Write-Host ("体积门禁: {0}MB -> {1}" -f "20", $(if ($ok) { "PASS" } else { "FAIL" }))

if (-not $ok) {
    exit 1
}
exit 0