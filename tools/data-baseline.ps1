<#
.SYNOPSIS
  PureSlate P1-09 数据采集基线：扫描（data-collect 只读二进制）+ Windows 容量/盘型，
  合并为 docs/verify/raw/<tag>.json，并汇总渲染 docs/verify/data-baseline.md。

.DESCRIPTION
  - Rust 二进制 data-collect 枚举全部固定盘，产出逐盘分类占用/大文件分布/耗时（匿名，无路径）；
  - PowerShell 侧用 Get-CimInstance 补各盘容量与盘型（SSD/HDD），零依赖；
  - 每机结果写入 docs/verify/raw/<tag>.json（幂等），所有 raw/*.json 合并为 data-baseline.md
    （多机模板：当前可只跑本机一张，后续在同学机再跑并置入 raw，重跑本脚本即合并）。
  - 只读红线：data-collect 不删除/移动任何用户数据；本脚本仅写 docs/verify/ 下文件。

.EXAMPLE
  .\tools\data-baseline.ps1
  .\tools\data-baseline.ps1 -Tag "my-laptop" -RulesDir "src-tauri/resources/rules"
#>
param(
    [string]$RulesDir = "src-tauri/resources/rules",
    [string]$RawDir = "docs/verify/raw",
    [string]$Tag = $(hostname),
    [string]$OutMd = "docs/verify/data-baseline.md"
)

# Continue, not Stop: cargo writes build progress to stderr, and Stop + `2>&1`
# turns each stderr line into a terminating NativeCommandError.
$ErrorActionPreference = "Continue"

$root = Split-Path -Parent $PSScriptRoot
$srcTauri = Join-Path $root "src-tauri"
$rulesAbs = Join-Path $root $RulesDir

if (-not (Test-Path $rulesAbs)) {
    Write-Error "rules dir not found: $rulesAbs"
    exit 1
}

# 机台标识脱敏（与 data-collect 缺省规则一致：仅小写字母/数字/`-`/`_`）。
$Tag = ($Tag -replace '[^A-Za-z0-9\-_]', '-').ToLowerInvariant()

Write-Host ">> rules dir: $rulesAbs"
Write-Host ">> tag: $Tag"

# ---- 工具 ----
function Format-Bytes {
    param([double]$b)
    if ($b -ge 1TB) { return ("{0:N1} TB" -f ($b / 1TB)) }
    if ($b -ge 1GB) { return ("{0:N1} GB" -f ($b / 1GB)) }
    if ($b -ge 1MB) { return ("{0:N1} MB" -f ($b / 1MB)) }
    return ("{0:N0} B" -f $b)
}

# ---- Step1 容量/盘型（Win32_LogicalDisk + Get-PhysicalDisk）----
function Get-DriveCapacity {
    # 固定盘(DriveType=3) 容量与剩余。
    $logical = Get-CimInstance -ClassName Win32_LogicalDisk -ErrorAction SilentlyContinue |
        Where-Object { $_.DriveType -eq 3 }

    # 盘型：物理盘 MediaType -> 盘号映射（HDD/SSD/Firmware/Unspecified -> unknown）。
    $diskMap = @{}
    try {
        Get-CimInstance -ClassName MSFT_PhysicalDisk -Namespace root/Microsoft/Windows/Storage -ErrorAction Stop |
            ForEach-Object {
                foreach ($n in $_.DeviceId) { $diskMap[[int]$n] = $_.MediaType }
            }
    } catch {
        Write-Host ">> 盘型不可用（MSFT_PhysicalDisk 查询失败或需管理员），记 unknown"
    }

    $cap = @{}
    foreach ($ld in $logical) {
        $drive = "$($ld.DeviceID)"; if ($drive -notmatch '^[A-Za-z]:$') { continue }
        $cap[$drive] = @{
            totalBytes = $ld.Size
            freeBytes  = $ld.FreeSpace
            diskType   = "unknown"
        }
    }

    # 尝试用 Win32_DiskDrive 每盘对应到 LogicalDisk（DiskIndex -> 盘）。
    try {
        $part = Get-CimInstance -ClassName Win32_LogicalDiskToPartition -ErrorAction SilentlyContinue
        $ptod = Get-CimInstance -ClassName Win32_DiskDriveToDiskPartition -ErrorAction SilentlyContinue
        foreach ($p in $part) {
            $dep = $p.Dependent -as [string]
            $ant = $p.Antecedent -as [string]
            if ($dep -notmatch 'DeviceID="([A-Za-z]:)"') { continue }
            $drv = $Matches[1]
            # Antecedent 是 partition；再经 DiskDriveToDiskPartition 查盘号。
            foreach ($pp in $ptod) {
                if (($pp.Dependent -as [string]) -like "*$($p.Antecedent)*" ) {
                    $dop = $pp.Antecedent -as [string]
                    if ($dop -match 'Index=(\d+)') {
                        $idx = [int]$Matches[1]
                        if ($diskMap.ContainsKey($idx) -and $cap.ContainsKey($drv)) {
                            $cap[$drv].diskType = switch ($diskMap[$idx]) {
                                3 { "HDD" }
                                4 { "SSD" }
                                5 { "SCM" }
                                default { "unknown" }
                            }
                        }
                    }
                    break
                }
            }
        }
    } catch {
        Write-Host ">> 盘号反查失败，全部记 unknown"
    }
    return $cap
}

# ---- Step2 调 data-collect（只读扫描）----
Write-Host ">> building + running data-collect in --release (first build may take minutes)..."

Push-Location $srcTauri
try {
    $stdout = & cargo run --release --bin data-collect `
        -- --rules-dir $rulesAbs --tag $Tag 2>&1 | Out-String
}
finally {
    Pop-Location
}

if ($LASTEXITCODE -ne 0) {
    Write-Error "data-collect failed (exit $LASTEXITCODE):`n$stdout"
    exit 1
}

# 提取 JSON 数组（首 '{' 至尾 '}'）。
$jsonText = ""
$start = $stdout.IndexOf("```json")
if ($start -lt 0) { $start = $stdout.IndexOf('[') }
if ($start -ge 0) {
    $endIdx = $stdout.LastIndexOf(']')
    if ($endIdx -gt $start) {
        $jsonText = $stdout.Substring($start, $endIdx - $start + 1)
        $jsonText = $jsonText -replace '^```json\s*', ''
    }
}
if (-not $jsonText) {
    Write-Error "could not parse JSON from data-collect output:`n$stdout"
    exit 1
}

$scans = $jsonText | ConvertFrom-Json
if (-not $scans -or $scans.Count -eq 0) {
    Write-Error "data-collect returned no drives"
    exit 1
}

# ---- Step3 合并容量/盘型 -> raw/<tag>.json（幂等）----
$cap = Get-DriveCapacity
foreach ($s in $scans) {
    $d = $s.drive
    if ($cap.ContainsKey($d)) {
        $s.totalBytes    = $cap[$d].totalBytes
        $s.freeBytes     = $cap[$d].freeBytes
        $s.diskType      = $cap[$d].diskType
    } else {
        $s.totalBytes    = $null
        $s.freeBytes     = $null
        $s.diskType      = "unknown"
    }
}

$rawDirAbs = Join-Path $root $RawDir
New-Item -ItemType Directory -Force -Path $rawDirAbs | Out-Null
$rawFile = Join-Path $rawDirAbs "$Tag.json"
($scans | ConvertTo-Json -Depth 6) | Out-File $rawFile -Encoding utf8
Write-Host ">> raw written: $rawFile"

# ---- Step4 汇总渲染 data-baseline.md ----
$mdLines = New-Object System.Collections.Generic.List[string]
$mdLines.Add("# PureSlate 数据采集基线（data-baseline）")
$mdLines.Add("")
$mdLines.Add("> 生成：tools/data-baseline.ps1 · 只读扫描 · **匿名聚合**（不落任何真实路径/文件名）· 多机合并：各机 `raw/<tag>.json` 重跑即汇总")
$mdLines.Add("")

# §1 口径
$mdLines.Add("## 1. 采集说明与方法口径")
$mdLines.Add("")
$mdLines.Add("- **只读**：扫描引擎不删除/移动任何数据；仅写 `docs/verify/` 下文件。")
$mdLines.Add("- **匿名**：每机仅记录聚合数值 + 类别 label + 脱敏 tag，无真实路径/文件名。")
$mdLines.Add("- **盘范围**：全部固定盘（Win32 DriveType=3）。")
$mdLines.Add("- **引擎**：管理员+NTFS → MFT 直读；否则回落 walkdir（逐盘记录 `engine`）。")
$mdLines.Add("- **大文件分档**：<100MB / 100–500MB / 500MB–1GB / 1–10GB / >10GB。")
$mdLines.Add("- **分类占用**：本阶段仅已实现引擎的 temp 维度类目（temp.*）；large/dup/privacy/startup 待 P2/P3。")
$mdLines.Add("- **dup 候选**：size 组内 ≥2 份的文件数与字节（未哈希，P2-07 哈希）。")
$mdLines.Add("")

# §2 机台元数据表
$mdLines.Add("## 2. 机台元数据")
$mdLines.Add("")
$mdLines.Add("| tag | 盘 | 盘型 | 总容量 | 已用 | 引擎 | 文件数 | 遍历耗时 |")
$mdLines.Add("|-----|----|------|--------|------|------|--------|----------|")
foreach ($s in $scans) {
    $used = if ($null -ne $s.totalBytes -and $null -ne $s.freeBytes) { ($s.totalBytes - $s.freeBytes) } else { $null }
    $mdLines.Add(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7}s |" -f
        $s.tag, $s.drive, $s.diskType,
        (& { if ($null -ne $s.totalBytes) { Format-Bytes $s.totalBytes } else { "-" } }),
        (& { if ($null -ne $used) { Format-Bytes $used } else { "-" } }),
        $s.engine, $s.files, [math]::Round(($s.ms / 1000.0), 1)))
}
$mdLines.Add("")

# §3 大文件分布
$mdLines.Add("## 3. 大文件分布（逐盘）")
$mdLines.Add("")
$mdLines.Add("| 盘 | <100MB | 100–500MB | 500MB–1GB | 1–10GB | >10GB |")
$mdLines.Add("|----|--------|----------|-----------|--------|-------|")
foreach ($s in $scans) {
    $b = $s.sizeBuckets
    $mdLines.Add(("| {0} | {1} | {2} | {3} | {4} | {5} |" -f
        $s.drive, $b.lt100mb, $b.x100m_500mb, $b.x500m_1gb, $b.x1gb_10gb, $b.x10gbUp))
}
$mdLines.Add("")

# §4 分类占用
$mdLines.Add("## 4. 分类占用（temp.*）")
$mdLines.Add("")
$mdLines.Add("| tag | 盘 | 类目 | 等级 | 件数 | 字节 |")
$mdLines.Add("|-----|----|------|------|------|------|")
foreach ($s in $scans) {
    foreach ($c in $s.categories) {
        $mdLines.Add(("| {0} | {1} | {2} | {3} | {4} | {5} |" -f
            $s.tag, $s.drive, $c.label, $c.grade, $c.items, (& { if ($c.bytes -gt 0) { Format-Bytes $c.bytes } else { "0" } })))
    }
}
$mdLines.Add("")

# §5 dup 候选
$mdLines.Add("## 5. 重复候选（未哈希）")
$mdLines.Add("")
$mdLines.Add("| 盘 | 候选文件数 | 候选字节 |")
$mdLines.Add("|----|-----------|----------|")
foreach ($s in $scans) {
    $mdLines.Add(("| {0} | {1} | {2} |" -f
        $s.drive, $s.dupCandidates, (& { if ($s.dupCandidateBytes -gt 0) { Format-Bytes $s.dupCandidateBytes } else { "0" } })))
}
$mdLines.Add("")

# §6 局限与后续
$mdLines.Add("## 6. 局限与后续")
$mdLines.Add("")
$mdLines.Add("- **分类占用**仅覆盖已实现 temp 维度；large/dup/startup/privacy 待 P2/P3 实际实现后在引擎内补齐。")
$mdLines.Add("- **盘型/容量**来自 `Get-CimInstance`；磁盘型号反查不到时记 `unknown`。")
$mdLines.Add("- **同学机扩充**：在各自机器跑本脚本（随包置 `raw/<tag>.json`），收集后放入 `docs/verify/raw/` 重跑即合并。")
$mdLines.Add("- **dup 哈希**延迟到 P2-07；MFT 直读需管理员（否则回落 walk，耗时偏高可对比 `engine` 列）。")

$mdLines | Out-File -FilePath (Join-Path $root $OutMd) -Encoding utf8
Write-Host ">> markdown written: $(Join-Path $root $OutMd)"
exit 0