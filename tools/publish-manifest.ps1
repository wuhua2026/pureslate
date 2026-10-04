# P4-06 发布清单生成脚本（R26 · SignPath 流水线配套）。
#
# 生成两份发布产物：
#   1. 规则包 rules/pureslate-rules-<version>.json
#      （P4-01 RulesPack 格式 {version, files:[{name,content}]}；sha256 对整包字节；
#       whitelist.xml 不入包——白名单只随应用资源目录，不随更新包下发，见 P4-01 备注）
#   2. update-manifest.json（P4-01 SPEC §6.5 契约：appVersion/appUrl/appSha256/
#      rulesVersion/rulesUrl/rulesSha256；jsDelivr/ghproxy/raw 三通道均读 main 分支
#      的该文件，故发布 job 会把它 commit 回 main）
#
# 用法（PowerShell）：
#   .\tools\publish-manifest.ps1                                          # 全自动（需已构建 NSIS 包）
#   .\tools\publish-manifest.ps1 -RepoSlug "org/repo" -Version "0.1.0"
#
# 约束：REPO_SLUG 必须与 src-tauri/src/updates/mod.rs 的 REPO_SLUG 常量一致
# （发布侧取 GITHUB_REPOSITORY，代码侧为占位常量——真实仓库定稿后两处自然对齐）。
param(
    [string]$Version = "",
    [string]$RepoSlug = "",
    [string]$SourceRoot = "",
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

if (-not $SourceRoot) { $SourceRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
if (-not $RepoSlug) {
    if ($env:GITHUB_REPOSITORY) { $RepoSlug = $env:GITHUB_REPOSITORY }
    else { $RepoSlug = "pureslate/pureslate" }  # 与 updates/mod.rs REPO_SLUG 占位同源
}
if (-not $Version) {
    $conf = Get-Content (Join-Path $SourceRoot "src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
    $Version = $conf.version
}
if (-not $OutputDir) { $OutputDir = Join-Path $SourceRoot "dist-publish" }

$tag = "v$Version"
$installerName = "PureSlate_${Version}_x64-setup.exe"
$installerPath = Join-Path $SourceRoot "src-tauri/target/release/bundle/nsis/$installerName"
if (-not (Test-Path $installerPath)) {
    throw "安装包不存在: $installerPath（先运行 pnpm tauri build --bundles nsis）"
}

Write-Host "[publish-manifest] 版本=$Version 仓库=$RepoSlug"

# ---- 1) 规则包（排除 whitelist.xml：白名单只随资源目录，见 P4-01）----
$rulesDir = Join-Path $SourceRoot "src-tauri/resources/rules"
$files = @()
Get-ChildItem -Path $rulesDir -Filter "*.xml" | Sort-Object Name | ForEach-Object {
    if ($_.Name -ne "whitelist.xml") {
        $files += @{ name = $_.Name; content = [IO.File]::ReadAllText($_.FullName, [Text.UTF8Encoding]::new($false)) }
    }
}
if ($files.Count -eq 0) { throw "规则目录为空: $rulesDir" }
$pack = @{ version = $Version; files = $files }

$rulesOutDir = Join-Path $OutputDir "rules"
New-Item -ItemType Directory -Force -Path $rulesOutDir | Out-Null
$rulesPackPath = Join-Path $rulesOutDir "pureslate-rules-$Version.json"
$utf8NoBom = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText($rulesPackPath, ($pack | ConvertTo-Json -Depth 5), $utf8NoBom)

$rulesSha = (Get-FileHash -Algorithm SHA256 $rulesPackPath).Hash.ToLowerInvariant()
Write-Host "[publish-manifest] 规则包: $rulesPackPath (sha256=$($rulesSha.Substring(0,16))...)"

# ---- 2) 安装包 sha256 ----
$installerSha = (Get-FileHash -Algorithm SHA256 $installerPath).Hash.ToLowerInvariant()
Write-Host "[publish-manifest] 安装包: $installerPath (sha256=$($installerSha.Substring(0,16))...)"

# ---- 3) update-manifest.json（P4-01 UpdateManifest 契约，camelCase）----
$manifest = @{
    appVersion  = $Version
    appUrl      = "https://github.com/$RepoSlug/releases/download/$tag/$installerName"
    appSha256   = $installerSha
    rulesVersion = $Version
    rulesUrl    = "https://raw.githubusercontent.com/$RepoSlug/main/rules/pureslate-rules-$Version.json"
    rulesSha256 = $rulesSha
}
$manifestPath = Join-Path $OutputDir "update-manifest.json"
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json), $utf8NoBom)
Write-Host "[publish-manifest] 清单: $manifestPath"

Write-Host "[publish-manifest] 完成。发布 job 应将 update-manifest.json 与 rules/ 提交回 main 分支（jsDelivr/raw 通道读取处），并将安装包与规则包上传 GitHub Release $tag。"
