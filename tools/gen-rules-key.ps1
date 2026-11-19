# 规则包签名密钥生成 / 轮换脚本（R32-4b）。
#
# 用途：生成 ECDSA P-256 密钥对，输出
#   1. 私钥 PKCS#8 PEM（写入 -OutPath，默认 %USERPROFILE%\.pureslate\），
#      **离线保存，绝不入库**；CI 侧存为 GitHub Secret `RULES_SIGNING_KEY`；
#   2. 公钥 X/Y 大端 hex 与可直接粘贴的 Rust 表项（src-tauri/src/updates/mod.rs
#      的 RULES_SIGNING_KEYS）。
#
# 轮换顺序（不可颠倒，旧客户端对未知 keyId fail-closed）：
#   ① 生成新 keyId（如 "2"），把公钥加入 RULES_SIGNING_KEYS 并发版（仍用 keyId=1 签包）；
#   ② 新版本覆盖面足够后，CI 换用新私钥，publish-manifest.ps1 -RulesKeyId 2；
#   ③ 旧 keyId 至少保留一个大版本再删除。
#
# 用法（PowerShell 7）：
#   .\tools\gen-rules-key.ps1 -KeyId 2
#   .\tools\gen-rules-key.ps1 -KeyId 2 -OutPath "C:\offline\pureslate-rules-2.pem"

param(
    [Parameter(Mandatory = $true)][string]$KeyId,
    [string]$OutPath = ""
)

$ErrorActionPreference = "Stop"

if (-not $OutPath) {
    $dir = Join-Path $env:USERPROFILE ".pureslate"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $OutPath = Join-Path $dir "rules-signing-key-$KeyId.pem"
}
if (Test-Path $OutPath) { throw "目标私钥已存在，拒绝覆盖: $OutPath" }

$ec = [System.Security.Cryptography.ECDsa]::Create([System.Security.Cryptography.ECCurve]::CreateFromFriendlyName('nistP256'))
try {
    $pem = $ec.ExportPkcs8PrivateKeyPem()
    [IO.File]::WriteAllText($OutPath, $pem, [Text.UTF8Encoding]::new($false))
    $p = $ec.ExportParameters($false)
} finally {
    $ec.Dispose()
}

$hex = { param($b) ($b | ForEach-Object { $_.ToString('x2') }) -join '' }
$x = & $hex $p.Q.X
$y = & $hex $p.Q.Y

function As-RustArray([string]$h) {
    $bytes = for ($i = 0; $i -lt $h.Length; $i += 2) { "0x" + $h.Substring($i, 2) }
    $lines = for ($i = 0; $i -lt $bytes.Count; $i += 15) {
        "        " + (($bytes[$i..([Math]::Min($i + 14, $bytes.Count - 1))]) -join ', ') + ','
    }
    return ($lines -join "`n")
}

Write-Host "[gen-rules-key] 私钥已写入（离线保存，绝不入库）: $OutPath"
Write-Host "[gen-rules-key] GitHub Secret 名称: RULES_SIGNING_KEY（内容 = 该 PEM 全文）"
Write-Host "[gen-rules-key] 公钥 X = $x"
Write-Host "[gen-rules-key] 公钥 Y = $y"
Write-Host ""
Write-Host "把以下表项加入 src-tauri/src/updates/mod.rs 的 RULES_SIGNING_KEYS："
Write-Host ("RulesSigningKey {{`n    key_id: `"{0}`",`n    public_key: [`n{1}`n    ],`n}}," -f $KeyId, (As-RustArray ($x + $y)))
