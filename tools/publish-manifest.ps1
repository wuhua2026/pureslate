# P4-06 发布清单生成脚本（R26 · SignPath 流水线配套）。
#
# 生成两份发布产物：
#   1. 规则包 rules/pureslate-rules-<version>.json
#      （P4-01 RulesPack 格式 {version, files:[{name,content}]}；sha256 对整包字节；
#       whitelist.xml 不入包——白名单只随应用资源目录，不随更新包下发，见 P4-01 备注）
#   2. update-manifest.json（P4-01 SPEC §6.5 契约：appVersion/appUrl/appSha256/
#      rulesVersion/rulesUrl/rulesSha256；R32-4b 加性新增 rulesSig/rulesKeyId；
#      jsDelivr/ghproxy/raw 三通道均读 main 分支的该文件，故发布 job 会把它 commit 回 main）
#
# 规则包签名（R32-4b）：
#   - 私钥 = ECDSA P-256 PKCS#8 PEM，离线保存，CI 侧放 GitHub Secret `RULES_SIGNING_KEY`；
#   - 签名对象 = 规则包文件**原始字节**，格式 IEEE P1363 裸 `r||s`（64 字节），
#     manifest 里以 128 位小写 hex 写入 `rulesSig`，`rulesKeyId` 标识所用公钥；
#   - 客户端公钥表见 src-tauri/src/updates/mod.rs 的 RULES_SIGNING_KEYS（未知 keyId 拒绝）；
#   - 生成/轮换密钥：tools/gen-rules-key.ps1（顺序：先发公钥、后换签名）。
#
# 用法（PowerShell 7）：
#   .\tools\publish-manifest.ps1                                          # 全自动（需已构建 NSIS 包）
#   .\tools\publish-manifest.ps1 -RepoSlug "org/repo" -Version "0.1.0"
#   .\tools\publish-manifest.ps1 -SigningKeyPath "C:\keys\rules.pem" -RequireSignature
#   $env:RULES_SIGNING_KEY_PATH = "C:\keys\rules.pem"   # 也可用环境变量指定私钥路径
#
# 约束：REPO_SLUG 必须与 src-tauri/src/updates/mod.rs 的 REPO_SLUG 常量一致
# （发布侧取 GITHUB_REPOSITORY，代码侧为占位常量——真实仓库定稿后两处自然对齐）。
param(
    [string]$Version = "",
    [string]$RepoSlug = "",
    [string]$SourceRoot = "",
    [string]$OutputDir = "",
    [string]$SigningKeyPath = "",
    [string]$RulesKeyId = "1",
    [switch]$RequireSignature
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

# ---- 1.5) 规则包签名（ECDSA P-256 / SHA-256，裸 r||s，hex）----
if (-not $SigningKeyPath -and $env:RULES_SIGNING_KEY_PATH) { $SigningKeyPath = $env:RULES_SIGNING_KEY_PATH }
$rulesSig = ""
if ($SigningKeyPath) {
    if (-not (Test-Path $SigningKeyPath)) { throw "签名私钥不存在: $SigningKeyPath" }
    # 需 PowerShell 7（.NET 5+ 才有 ImportFromPem / IEEE P1363 输出）。
    $ec = [System.Security.Cryptography.ECDsa]::Create()
    try {
        $ec.ImportFromPem([IO.File]::ReadAllText($SigningKeyPath))
        $sigBytes = $ec.SignData(
            [IO.File]::ReadAllBytes($rulesPackPath),
            [System.Security.Cryptography.HashAlgorithmName]::SHA256,
            [System.Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)
        if (-not $ec.VerifyData(
                [IO.File]::ReadAllBytes($rulesPackPath),
                $sigBytes,
                [System.Security.Cryptography.HashAlgorithmName]::SHA256,
                [System.Security.Cryptography.DSASignatureFormat]::IeeeP1363FixedFieldConcatenation)) {
            throw "规则包签名自校验失败"
        }
        $rulesSig = ($sigBytes | ForEach-Object { $_.ToString('x2') }) -join ''
    } finally {
        $ec.Dispose()
    }
    Write-Host "[publish-manifest] 规则包已签名 keyId=$RulesKeyId sig=$($rulesSig.Substring(0,16))..."
} elseif ($RequireSignature) {
    throw "缺少规则包签名私钥（-SigningKeyPath 或 RULES_SIGNING_KEY_PATH）；发布必须带签名"
} else {
    Write-Warning "[publish-manifest] 未提供签名私钥：本次 manifest 不含 rulesSig（仅限本地调试，发布勿用）"
}

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
if ($rulesSig) {
    $manifest.rulesSig = $rulesSig
    $manifest.rulesKeyId = $RulesKeyId
}
$manifestPath = Join-Path $OutputDir "update-manifest.json"
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json), $utf8NoBom)
Write-Host "[publish-manifest] 清单: $manifestPath"

Write-Host "[publish-manifest] 完成。发布 job 应将 update-manifest.json 与 rules/ 提交回 main 分支（jsDelivr/raw 通道读取处），并将安装包与规则包上传 GitHub Release $tag。"
