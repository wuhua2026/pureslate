# PureSlate · 技术规格（SPEC.md）

> 本文定义架构、模块、数据结构与 IPC 契约。实现与本文冲突时，先改本文（记 CHANGELOG）再改代码。

## 1. 技术栈（锚定，升级须记 CHANGELOG）

| 层 | 选型 | 说明 |
|----|------|------|
| 壳 | Tauri 2.x | WebView2 前端 + Rust 内核 |
| 内核 | Rust（edition 2021），MSRV 随 Tauri | serde / serde_json / quick-xml / walkdir / sha2 / thiserror / uuid / windows（windows-rs）|
| 前端 | Vue 3 + TypeScript(strict) + Vite + Pinia + vue-router | **不引入 UI 组件库**（体积预算） |
| 测试 | cargo test（Rust）+ vitest（前端）+ 黄金文件集 + VM 快照回归 | SPEC §8 |
| 构建/分发 | pnpm + vite + tauri-cli；NSIS 安装包；GitHub Releases；SignPath 签名 | CI 可重复构建 |

体积预算：安装包 **<20MB**（CI 断言）；冷启动 <3s。

## 2. 仓库与模块结构

```
pureslate/
├── AGENTS.md                     # 根约定（本包 AGENTS.md 落库至此）
├── docs/dev/                     # SPEC/TASKS/SAFETY（本包）
│   └── CHANGELOG.md              # 契约与规格变更记录
├── src/                          # Vue3 前端
│   ├── types/ipc.ts              # ★ IPC 契约 TS 端（唯一事实源）
│   ├── api/                      # invoke/listen 封装（薄层，禁止散落调用）
│   ├── stores/                   # pinia: scan / clean / quarantine / settings
│   ├── pages/                    # Home Scan Report Startup Privacy Files Quarantine Log Settings
│   └── components/               # GradeBadge DiskBar ConfirmTokenModal ...
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs / main.rs      # 命令注册、单实例互斥
│   │   ├── contract.rs           # ★ IPC 契约 Rust 端（serde，镜像 ipc.ts）
│   │   ├── rules/                # R01: model(解析) loader(XML→内存) matcher(路径匹配) cache
│   │   ├── scanner/              # walk(walkdir 遍历+取消) mft(DG-1 条件启用) aggregate(聚合)
│   │   ├── safety/               # R02: grade(判定) whitelist(白名单)
│   │   ├── guard/                # R23: process(目标进程守卫) instance(单实例)
│   │   ├── cleaner/              # R04/R06/R05/R08 执行: execute journal(事务) recycle(回收站API)
│   │   ├── quarantine/           # R25: store(移入) manifest restore lifecycle
│   │   ├── logging/              # R09: audit(JSONL)
│   │   ├── startup/              # R07: 枚举(注册表/启动文件夹/计划任务) toggle(禁用不删源)
│   │   ├── privacy/              # R08: 浏览器历史/最近文档枚举
│   │   ├── updates/              # R22: 版本检查 manifest校验 镜像回退
│   │   ├── crash/                # R24: minidump 本地落盘
│   │   └── ipc/                  # 命令 handler（薄层，调内部模块，不含业务）
│   ├── resources/rules/*.xml     # 规则包（外置，改 XML 即改行为）
│   └── tests/                    # 集成测试 + golden
├── tests/golden/                 # 黄金文件集（P4-04 建）
└── tools/                        # bench-scan / test-restore / vm-regression 脚本
```

数据落盘位置：`%LOCALAPPDATA%\PureSlate\`（settings.json / logs/audit.jsonl / journal/ / restore-conflict/）；隔离区按卷：`<卷根>\.pureslate-quarantine\`（见 SAFETY §4.1）。

## 3. 核心概念

- **扫描维度 ScanDimension**：`temp`（临时/回收站/%TEMP%）｜`large`（≥500MB 大文件）｜`dup`（重复 hash）｜`cache`（应用缓存）｜`startup`（启动项）｜`privacy`（隐私痕迹）。profile 可开关各维度。
- **分级 Grade**：`green|yellow|red`（判定规则 SAFETY §1）。
- **去向 Disposition**：`direct`（直清，仅 🟢）｜`recycle`（系统回收站，仅 🟢）｜`quarantine`（隔离区，🟡/🔴）。
- **信任链**：R25 隔离区 → R09 日志 → R04 清理，先有后悔药再动刀（Phase 2 顺序依据）。

## 4. 数据结构

### 4.1 规则 XML schema（resources/rules/*.xml）

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="system-temp" version="1" lang="zh-CN">
  <category id="temp.user" label="临时文件" risk="green" disposition="direct"
            description="程序临时文件，删除后自动重建">
    <target type="env" value="%TEMP%"/>
    <target type="path" value="C:\Windows\Temp"/>
    <include pattern="*" recursive="true" maxAgeDays="7"/>
    <exclude pattern="*.lock"/>
    <guard process=""/>                          <!-- 空=无进程守卫 -->
  </category>
  <category id="cache.wechat" label="微信缓存" risk="yellow" disposition="quarantine"
            description="清理后需重新登录；14 天内可从隔离区还原">
    <target type="knownFolder" value="WeChat Files"/>
    <include pattern="FileStorage\Cache\**" recursive="true"/>
    <guard process="WeChat.exe"/>
  </category>
</ruleset>
```

字段语义：
- `risk`：green|yellow|red；`disposition`：direct|recycle|quarantine（green 只准 direct/recycle）；
- `target type`：`env`（环境变量）｜`path`（绝对）｜`knownFolder`（Known Folder API）；
- `include/exclude`：glob（`*` `**`）；`maxAgeDays`=0 表示不限；可选 `minSizeMB`；
- `guard process`：清理前进程守卫（SAFETY §5.1）；
- 解析失败/字段非法 → 该 category 整体丢弃并记日志（**宁缺勿错**）；
- 多 ruleset 同 id category 时后加载覆盖先加载（规则包更新语义）。

### 4.2 扫描结果（contract.rs ↔ ipc.ts 共享）

```ts
export type Grade = 'green' | 'yellow' | 'red';
export type Disposition = 'direct' | 'recycle' | 'quarantine';
export type ScanDimension = 'temp' | 'large' | 'dup' | 'cache' | 'startup' | 'privacy';

export interface ScanProfile { dimensions: Partial<Record<ScanDimension, boolean>>; }

export interface ScanItem {
  id: string;                 // sha1(categoryId + path) 前 16 位，稳定 ID
  categoryId: string;         // 规则类目，如 temp.user
  label: string;              // 类目显示名
  path: string;               // 完整路径
  sizeBytes: number;
  grade: Grade;
  disposition: Disposition;   // 去向预告
  reason: string;             // 可解释文案（来自规则 description + 匹配说明）
  mtime?: number; atime?: number;   // epoch ms
  dupGroup?: string;          // R06: 同 hash 组 ID（组内默认保留一份=最早修改者）
}

export interface CategoryAggregate {
  categoryId: string; label: string; grade: Grade; disposition: Disposition;
  totalBytes: number; itemCount: number; reason: string;
}

export interface ScanResult {
  scanId: string; startedAt: number; finishedAt: number;
  volume: string;                       // 如 C:
  aggregates: CategoryAggregate[];      // 报告页直用
  itemCount: number; totalBytes: { green: number; yellow: number; red: number };
  // items 不整包返回，经 scan_get_items 分页拉取
}
```

### 4.3 隔离区 manifest（.pureslate-quarantine\manifest.jsonl）

```json
{"id":"uuid-v4","originalPath":"C:\\Users\\u\\...","quarantinePath":"C:\\.pureslate-quarantine\\202609\\b1\\a3f2_缓存.db","sizeBytes":1048576,"sha256":"...","grade":"yellow","categoryId":"cache.wechat","movedAt":1756000000000,"expiresAt":1757210000000,"state":"quarantined"}
```

`state ∈ quarantined | restored | purged`；restored 行保留 30 天后由 lifecycle 清理。

### 4.4 审计日志（%LOCALAPPDATA%\PureSlate\logs\audit.jsonl，R09/M8）

```json
{"ts":1756000000000,"op":"clean|restore|purge|auto_purge|disable_startup|scan","txId":"...","categoryId":"temp.user","path":"...","sizeBytes":123,"disposition":"direct|recycle|quarantine","result":"ok|skip|fail","detail":""}
```

要求：破坏性操作覆盖率 100%（M8）；只追加；按天滚动（audit-2026-08-24.jsonl）；可导出。

### 4.5 设置（settings.json）

```ts
export interface AppSettings {
  quarantineRetentionDays: number;   // 默认 14，范围 7–30
  quarantineAutoPurge: boolean;      // 默认 true
  updateOptIn: boolean;              // 默认 false（R22）
  crashUploadOptIn: boolean;         // 默认 false（R24）
  expertMode: boolean;               // 默认 false（解锁 🔴）
  mirrorFirst: boolean;              // 默认 true（国内镜像优先）
}
```

## 5. IPC 契约（M0 冻结 · 两端镜像 ipc.ts / contract.rs）

命令（`invoke('name', args)`）：

| 命令 | 参数 → 返回 | 备注 |
|------|------------|------|
| `scan_start` | `profile: ScanProfile` → `scanId: string` | 异步，进度/完成走事件 |
| `scan_cancel` | `scanId` → `boolean` | 只读，随时可停 |
| `scan_get_items` | `scanId, offset, limit, filter?` → `ScanItem[]` | 分页；filter 按 grade/categoryId |
| `clean_execute` | `items: string[]`（item id 列表）, `confirmToken?: string` → `txId` | 含 🔴 时 token 必填（见下） |
| `clean_cancel` | `txId` → `boolean` | 当前项完成后停止 |
| `quarantine_list` | — → `QuarantineEntry[]` | 含剩余天数 |
| `quarantine_restore` | `ids: string[]` → `RestoreReport` | 冲突兜底见 SAFETY §4.3 |
| `quarantine_purge` | `ids: string[]`, `confirmToken` → `PurgeReport` | 硬删+审计 |
| `startup_list` | — → `StartupEntry[]` | |
| `startup_toggle` | `id, enabled` → `boolean` | 禁用=备份后移除启动项，不删源程序 |
| `log_query` | `{from, to, op?}` → `LogEntry[]` | |
| `log_export` | `path` → `boolean` | 导出指定范围 |
| `settings_get` / `settings_set` | — / `AppSettings` | set 全量覆盖 |
| `update_check` | `manual: boolean` → `UpdateStatus` | manual=true 无视 optIn |
| `app_meta` | — → `{version, rulesVersion, channel}` | |

事件（`listen('event', cb)`）：

| 事件 | payload | 说明 |
|------|---------|------|
| `scan_progress` | `{scanId, phase: 'walking'|'hashing'|'aggregating', percent, currentPath, foundBytes:{green,yellow,red}, elapsedMs}` | 屏 5 数据源，节流 200ms |
| `scan_done` | `ScanResult` | |
| `clean_progress` | `{txId, itemPath, disposition, doneBytes, totalBytes, state: 'ok'|'fail'|'skip'}` | 屏 6 数据源，逐项 |
| `quarantine_expiry_warning` | `{ids: string[], daysLeft: 3}` | 到期前 3 天 |
| `update_available` | `UpdateStatus` | opt-in 周查触发 |

补充类型：

```ts
export interface QuarantineEntry {
  id: string; originalPath: string; sizeBytes: number; grade: Grade;
  categoryId: string; movedAt: number; expiresAt: number; daysLeft: number;
  state: 'quarantined' | 'restored' | 'purged';
}
export interface StartupEntry {
  id: string; name: string; publisher?: string; command: string;
  source: 'hkcu_run' | 'hklm_run' | 'startup_folder' | 'task_scheduler';
  impact: 'high' | 'medium' | 'low'; enabled: boolean;
}
export interface UpdateStatus {
  currentVersion: string; latestVersion?: string; hasUpdate: boolean;
  rulesPackHashOk?: boolean; channel: 'github' | 'mirror'; checkedAt: number;
}
export interface RestoreReport { requested: number; restored: number; conflict: number; failures: { id: string; reason: string }[]; }
```

**confirmToken 语义**：UI 对 🔴 项或 `quarantine_purge` 弹二次确认 → 用户确认后由 `app_meta` 风格的握手命令获取一次性 token（实现为：Rust 端生成 UUID 注入弹窗上下文，提交时校验一次性消费）。M0 先定接口语义，UI 流程 Phase 2 落地。

## 6. 关键机制

### 6.1 扫描引擎（R01/R03）
- walkdir 遍历目标集合（由规则 target 展开），先过白名单（SAFETY §2）再匹配规则；reparse point 不跟随；取消令牌随时生效；
- **DG-1 决策门（Phase 1）**：真机 1TB SSD 全维度扫描基准 >120s → 启用 `scanner/mft`（NTFS MFT 直读枚举，需管理员权限声明）；详见 TASKS P1-02；
- 聚合输出 CategoryAggregate[]；items 全量驻留内存后端（PathBuf 紧凑存储），前端分页拉取。

### 6.2 事务与清理（R04）
journal 协议见 SAFETY §3。🟢 direct：`RemoveFile`（Windows API，绕过回收站）+ journal；🟢 recycle：`windows` crate 的 recycle-bin API（SHFileOperation 现代替代 `IFileOperation`）；跨类目并行度 ≤2，单类目串行保证可取消。

### 6.3 重复文件（R06）
三级过滤：size 分组 → 前 64KB 采样 hash 分组 → 全量 sha256 确认；仅对 ≥1MB 文件做全量 hash；dupGroup 默认保留组内 mtime 最早者，其余为候选；I/O 优先级 BelowNormal，不拖垮交互（M3 门禁一部分）。

### 6.4 启动项（R07）
枚举源：`HKCU\...\Run`、`HKLM\...\Run`、`HKLM\...\WOW6432Node\Run`、`shell:startup` 文件夹、计划任务（登录触发类）；禁用 = 导出到 `%LOCALAPPDATA%\PureSlate\startup-backup\<id>.reg|.lnk` 后移除，UI 提供"恢复"（还原即重新导入）——**不删源程序**。

### 6.5 更新（R22）
- `update-manifest.json`（GitHub Releases latest）：`{appVersion, appUrl, appSha256, rulesVersion, rulesUrl, rulesSha256}`；
- 通道顺序：`mirrorFirst=true` 时 jsDelivr（`https://cdn.jsdelivr.net/gh/<org>/<repo>@latest/...`）→ ghproxy → GitHub 直连，任一成功即停；
- 下载后 sha256 校验失败 → 丢弃 + `rulesPackHashOk=false` + 审计日志；**规则包写入前须校验通过**（规则 XML 是可执行内容）；
- opt-in 周查请求仅含版本号路径参数，无任何标识（AGENTS 红线 #4）。

### 6.6 崩溃安全（R24）
minidump 本地始终落盘（crash/，基于 `crash-handler` 或 windows-rs MiniDumpWriteDump）；opt-in 上传前 UI 展示 dump 内容预览（模块列表摘要），确认后才发；主程序启动时扫描孤儿 journal 执行恢复（SAFETY §3）。

## 7. UI 规范

页面（vue-router）：`/`（Home 屏1）`/scan`（屏5）`/report`（屏2/7）`/files`（大文件）`/startup` `/privacy` `/quarantine`（屏8）`/log`（屏4）`/settings`。

设计 token（CSS variables，WCAG AA）：

```css
--grade-green: #2E9E5B;  --grade-yellow: #D9971C;  --grade-red: #C4382E;
--bg: #FAFAFA; --surface: #FFFFFF; --text: #1F2430; --text-2: #5A6472;
--border: #E4E7EC; --accent: #3B82F6; --font: system-ui, "Microsoft YaHei UI";
```

硬性要求：🔴 除颜色外必须同时有图标+文案；无托盘/无开机推广/无红点角标；「已释放 vs 已隔离」语义区分（SAFETY §4.5）；线框参照 PRD 7.4 八屏（Home/报告/二次确认/日志/扫描中/执行中/完成/隔离区）。

## 8. 测试与度量

| 层 | 工具 | 覆盖 |
|----|------|------|
| Rust 单测 | cargo test | rules 匹配/safety 判定/journal/quarantine manifest 优先 |
| 前端 | vitest + mock 数据 | 页面渲染、分级交互、二次确认流 |
| 集成 | src-tauri/tests | scan→clean→restore 全链路（临时目录沙盒） |
| 黄金文件集 | tests/golden + fixture 生成器 | M2 误删率分档度量（SAFETY §6.1） |
| VM 回归 | tools/vm-regression.md + 微软开发版 VM 快照 | 清理后系统可启动、关键功能正常 |
| 性能基准 | tools/bench-scan（--release，输出 JSON） | M3：SSD 1TB ≤120s；HDD 记参考值 |
| 还原率 | tools/test-restore.ps1 | M12 ≥99.9%（SAFETY §6.2） |

基准口径：`temp+large+dup` 三维度全开，冷缓存（先读写释放 5GB 干扰文件），报告分维度耗时。

## 9. Non-goals（继承 PRD 第 8 章）

不做杀毒/实时防护；不做"系统提速/注册表优化/内存整理"；无广告/推荐/全家桶（不可妥协）；不抄 Dism++/联想任何实现；MVP 不做端侧中文意图搜索；不做跨端；不做账号/云存储/远程控制。

## 10. 术语 ↔ PRD 需求映射

R01→rules+scanner｜R02→safety｜R03→scan 编排｜R04→cleaner｜R05→scanner.large+pages/files｜R06→cleaner.dup（§6.3）｜R07→startup｜R08→privacy｜R09→logging｜R16→外壳｜R13/R14→pages/report｜R21→测试体系｜R22→updates｜R23→guard+兼容｜R24→crash+journal 恢复｜R25→quarantine｜R26→CI/签名。
