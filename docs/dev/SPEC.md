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
│   │   ├── uninstall/            # R27: 安全卸载辅助（枚举 Uninstall 四视图/编排官方卸载器；残留复用清理管线）
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
- **审阅型类目（R30）**：`large.file` 仅"发现供浏览"——items 照常返回，但不进报告/清理方案、不计 found 字节、规则表永不注册（`clean_execute` 对其 fail-closed），UI 无删除入口，只提供 shell 定位。个人大文件（视频/数据集/镜像）处置权完全在用户。
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
  <!-- R28 能力示例：注册表值动态目标 + 账号目录段正则收紧 + 递归深度封顶 -->
  <category id="cache.im-accounts" label="多账号 IM 缓存（语法示例）" risk="yellow" disposition="quarantine"
            description="仅收形态正确的账号目录下缓存；安装根目录由注册表值动态解析">
    <target type="reg-value" hive="HKCU" key="Software\ExampleVendor\ExampleApp"
            value="InstallDir" view="native" base="path"/>
    <include pattern="data\accounts\*\cache\**" recursive="true"
             segment-regex="[0-9]{6,12}" segment-index="2" max-depth="8"/>
  </category>
</ruleset>
```

字段语义：
- `risk`：green|yellow|red；`disposition`：direct|recycle|quarantine（green 只准 direct/recycle）；
- `target type`：
  - `env`（环境变量）｜`path`（绝对路径）｜`knownFolder`（Known Folder API）；
  - `reg-value`（R28，注册表字符串值动态路径）：属性 `hive`（HKLM｜HKCU）、`key`（子键路径，反斜杠分隔）、`value`（值名）、`view`（native=64 位视图，默认；wow64=32 位视图）、`base`（path=值本身即目标目录，默认；parent=注册表值是文件路径时取其父目录）。仅接受 REG_SZ / REG_EXPAND_SZ（后者先经 ExpandEnvironmentStrings 展开）；以 KEY_READ=0x20019（不含写位）只读打开，错误类型传播、生产路径无 unwrap（AGENTS 红线 6）。**机器相关软失败**：键/值不存在、类型不符、值为空、展开结果为空或不存在 → 跳过该 target（不丢弃 category，语义等同"本机未安装或不在默认位置"）并记日志；XML 属性非法（未知 hive/view/base、缺必填属性）→ 加载期丢弃整个 category。
  - **reg-value 安全约束**：注册表值是外部数据，解析出的路径与其它 target 一样必经白名单（SAFETY §2）与分级去向，无任何豁免；另加一道 loader 闸——含 reg-value target 的 category，其每条 include 必须**含路径分隔符**（具体子目录）或为**纯扩展名形态**（`*.ext`），禁止裸根全收模式（`*`｜`**`｜`*.*`｜`**/*`），违反 → category 丢弃。该闸只防"异常注册表值指向卷根→整盘入候选"的失控面，不替代白名单。
- `include/exclude`：glob（`*` `**`）；`maxAgeDays`=0 表示不限；可选 `minSizeMB`；
  - `segment-regex` + `segment-index`（R28）：glob 命中之后的**单路径段收紧条件**。相对 target 根把路径按分隔符切段（index 0 起，含文件名所在段），第 index 段必须存在且整段匹配该正则（引擎自动首尾锚定，规则里不用写 `^$`）；段不存在或不匹配 → 该 include 不命中（**fail-closed**）。语法为 regex-lite 支持的语法（字符类、量词、`\d` 等，无反向引用）；正则编译失败 → **加载期丢弃该条 include/exclude 并记日志**（收紧约束失效时绝不退化为无约束的裸 glob）。典型用途：把多账号软件的账号目录从 `*` 收紧为形态确定的 ID 目录；具体形态必须基于本机实测，规则全部自研（AGENTS 红线 5）。
  - `max-depth`（R28）：相对 target 根允许命中的最大路径段数（根直属文件 = 1）。category 遍历深度取全部 include 的最大值（walkdir `max_depth`），命中时每条 include 再按各自值裁尾；缺省 = 不限。`recursive="false"` 明确等价于 `max-depth="1"`（此前仅靠 glob 形状隐式约束）；二者同时声明且矛盾（recursive=false 且 max-depth>1）→ category 丢弃。
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
| `quarantine_status` | — → `QuarantineStatus` | 容量上限状态（P3-06 加性新增） |
| `startup_list` | — → `StartupEntry[]` | |
| `uninstall_list` | — → `InstalledApp[]` | R27 只读枚举（§6.7；R30 展示字段防御解码，乱码见 §6.7） |
| `shell_reveal` | `path: string` → `null` | R30 只读：资源管理器定位并选中（大文件审阅页；绝对/现存/非卷根，否则 Err） |
| `uninstall_app_icons` | `appIds: string[]` → `Record<string,string>` | R30 只读批量（≤200）：程序图标 PNG data URL map；缺失/失败 id 不在 map |
| `startup_toggle` | `id, enabled` → `boolean` | 禁用=备份后移除启动项，不删源程序 |
| `log_query` | `{from, to, op?}` → `LogEntry[]` | |
| `log_export` | `path` → `boolean` | 导出指定范围 |
| `settings_get` / `settings_set` | — / `AppSettings` | set 全量覆盖 |
| `update_check` | `manual: boolean` → `UpdateStatus` | manual=true 无视 optIn |
| `confirm_token_issue` | — → `string`（`PS-XXXX-XXXX`） | 🔴 二次确认令牌**后端签发**（I-2 · P4-06 加性新增）：仅专家模式可签，一次性消费（`clean_execute`/`quarantine_purge` 校验并作废）；未签发/不匹配/已消费一律拒绝 |
| `crash_list` | — → `CrashDumpInfo[]` | 本地转储列表（时间倒序；P4-03 加性新增） |
| `crash_preview` | `{fileName}` → `CrashDumpPreview` | 模块列表/异常代码摘要；解析失败回 error 字段 |
| `crash_upload` | `{fileName}` → `CrashUploadReport` | opt-in 门禁（crashUploadOptIn=false 拒绝联网） |
| `crash_recovery` | — → `CrashRecoveryReport` | 最近一次启动孤儿恢复摘要（ranAt=0 尚未运行） |
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
export interface InstalledApp {
  id: string; name: string; publisher?: string; version?: string;
  installDate?: string; installLocation?: string; uninstallCommand: string;
  isMsi: boolean; arch: 'x64' | 'x86'; scope: 'machine' | 'user';
  estimatedSizeBytes?: number;
}
export interface UpdateStatus {
  currentVersion: string; latestVersion?: string; hasUpdate: boolean;
  rulesPackHashOk?: boolean; channel: 'github' | 'mirror'; checkedAt: number;
}
export interface RestoreReport { requested: number; restored: number; conflict: number; failures: { id: string; reason: string }[]; }
```

**confirmToken 语义（I-2 · P4-06 已实现）**：UI 对 🔴 项或 `quarantine_purge` 弹二次确认 → 前端调 `confirm_token_issue` 从**后端**签发一次性令牌（仅专家模式）→ 用户逐字输入一致后随命令提交 → 后端取出比对并**消费**（未命中同样作废，防暴力重试）。前端自造值不再被信任；`log_export` 同批收口：参数只取文件名成分，固定落 `<data_root>\exports\`（I-1）。

## 6. 关键机制

### 6.1 扫描引擎（R01/R03）
- walkdir 遍历目标集合（由规则 target 展开），先过白名单（SAFETY §2）再匹配规则；reparse point 不跟随；取消令牌随时生效；
- **DG-1 决策门（Phase 1）**：真机 1TB SSD 全维度扫描基准 >120s → 启用 `scanner/mft`（NTFS MFT 直读枚举，需管理员权限声明）；详见 TASKS P1-02；
- R28 规则能力：target 支持 `reg-value`（注册表字符串值只读解析为路径，键值缺失按"未安装"软跳过）；遍历按 include `max-depth` 封顶深度；glob 命中后过 `segment-regex` 单段收紧（不匹配即不命中，编译失败丢规则，fail-closed）；
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

### 6.6 崩溃安全（R24，P4-03）
- minidump 本地始终落盘（`<data_root>\crash\`，保留最近 5 份）：SEH 顶级过滤器（`SetUnhandledExceptionFilter`）+ Rust panic hook 双通道；dbghelp `MiniDumpWriteDump` 以 `#[link]` 直调（零第三方依赖，依赖决策同 P1-02b 记 LESSONS）。SEH 通道经干净辅助线程写 dump；异常代码经 sidecar `<dump>.json` 携带（本机环境异常流路径不可用，LESSONS §①）；
- opt-in 上传（`crashUploadOptIn`，默认 false）：上传前 `crash_preview` 展示内容预览（模块列表摘要 + 异常代码），用户在设置页显式点击才发送；端点为占位常量（P4-06 定稿）；
- 主程序启动时扫描孤儿 journal 执行恢复（SAFETY §3 / T-6 协议，`crash/recover`）：**journal 只定位、manifest 唯一信任锚**——quarantine 孤儿经 `restore_one`（T-2 四重校验）自动还原；「已移入未登记」崩溃窗口按命名约定反查唯一候选补登记（不自动还原）；direct/recycle 不可逆去向只标记 orphan + 审计 + UI 提示；恢复期间 `clean_execute` 拒绝新事务。

### 6.7 安全卸载辅助（R27，v0.1.5+）
- **定位：官方卸载器的编排者，不是替代者**。破坏性删除的主体是程序自带卸载器（第三方进程，其删除不进本应用 journal/隔离区，必须在 UI 如实告知用户）；PureSlate 只做只读枚举、拉起、等待、事后判定，并把卸载**残留**纳入既有安全清理管线。
- **枚举（只读）**：HKLM/HKCU 各 64/32 位视图的 `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`（x64 进程显式带 KEY_WOW64_64KEY/32KEY）；过滤 `SystemComponent≠0` 与 `ParentKeyName` 非空（补丁/更新子项）；同 `DisplayName+Publisher+Version` 跨视图去重，优先级 HKLM64 > HKLM32 > HKCU64 > HKCU32；MSI 条目（`WindowsInstaller=1` 或子键名为 `{GUID}`）卸载命令构造为 `msiexec /X{GUID}`；命令行含 8.3 短路径（`~`）时 `GetLongPathNameW` 还原。**UWP（Remove-AppxPackage）不在范围内**。
- **执行（后续批次）**：`CreateProcessW` 拉起官方 UninstallString（SW_SHOWNORMAL，交互卸载器走自己的 UI）；**不申请 SeDebugPrivilege、默认不强杀**（占用冲突沿用 guard"提示退出"语义，强杀只能是专家模式+保护名单+用户显式确认）；进程树等待加递归深度上限 16 + 已访问 PID 集合去重；**不读单一退出码判成功**——事后重扫 Uninstall 键/安装目录是否消失，并检测 `PendingFileRenameOperations` 将相关项标"待重启完成"而非失败。
- **残留**：卸载后对安装目录、开始菜单快捷方式、AppData 三目录的候选项产出为 ScanItem，**仅在一次卸载刚完成的后端上下文内**枚举（不做无上下文的全局孤儿扫描）；默认 🟡/quarantine，复用 preflight + journal + 隔离区 + 审计（14 天可还原）。**去向由用户逐项选择**：移入隔离区（默认、可还原）或直接删除（永久、🔴 语义，含任一项即须专家模式 + 二次确认 token）；删除前 UI 明示完整清单与"判定为残留的理由"，逐项确认。为精确卸载残留开白名单口子有严格前提：白名单常量根分**硬根**（SystemRoot/引导卷/卷信息/隔离哨兵/自身设施/XML 声明根，永不豁免）与 **§2.2 程序目录软根**（ProgramFiles*/ProgramData\Microsoft，仅可被后端快照签发的**精确候选授权根**豁免——候选 id 必须出自最近残留扫描快照、路径必须在授权根内、硬保护/reparse/TOCTOU 复核不绕过；见 SAFETY §2.2 R27-3 条款）。空目录（0 文件）候选经 `RemoveDirectoryW` 只删空目录收尾（非空必然失败，不递归删内容）。
- **审计**：新增 op=`uninstall`（拉起/等待结论/待重启）；残留删除仍走既有 clean/quarantine op（category=`residue.uninstall`），覆盖率口径不变。
- **契约**：`uninstall_list() → InstalledApp[]`、`uninstall_run(id)` / `uninstall_cancel(txId)` 与 `uninstall_progress`/`uninstall_done` 事件（执行批次）；R27-3 加性新增 `uninstall_residue_scan() → ResidueCandidate[]` 与 `uninstall_residue_clean(ResidueCleanParams) → txId`（复用 clean_progress/clean_done/clean_cancel）。
- **R30 加性**：①展示字段（DisplayName/Publisher/DisplayVersion/InstallDate）防御解码——UTF-16/CP_ACP/CP936 三候选评分（ANSI 时代老安装器坏 REG_SZ），不可辨识的名称给"异常名称的程序"占位（条目保留可卸载），命令/路径字段仍用原值；②`uninstall_app_icons(appIds)` 批量返回 DisplayIcon 提取的 PNG data URL（32px，失败静默，前端字形回退），图标不影响卸载可用性；③大文件页定位复用 `shell_reveal(path)`。

## 7. UI 规范

页面（vue-router）：`/`（Home 屏1）`/scan`（屏5）`/report`（屏2/7）`/files`（大文件）`/startup` `/privacy` `/apps`（R27 软件卸载）`/quarantine`（屏8）`/log`（屏4）`/settings`。

设计 token（CSS variables，WCAG AA）：

```css
--grade-green: #2E9E5B;  --grade-yellow: #D9971C;  --grade-red: #C4382E;
--bg: #FAFAFA; --surface: #FFFFFF; --text: #1F2430; --text-2: #5A6472;
--border: #E4E7EC; --accent: #3B82F6; --font: system-ui, "Microsoft YaHei UI";
```

动效 token（R29，全应用唯一来源，禁止散写时长/缓动）：

```css
--dur-fast: 140ms;      /* 悬停/按压等即时反馈 */
--dur-base: 220ms;      /* 路由切换/折叠/状态切换 */
--dur-slow: 340ms;      /* 首屏入场/数字补间上限 */
--ease-out: cubic-bezier(0.16, 1, 0.3, 1);   /* 入场（减速）*/
--ease-inout: cubic-bezier(0.4, 0, 0.2, 1); /* 状态双向切换 */
```

动效规范（R29）：
- **只动 `transform` 与 `opacity`**（合成层属性），禁止动画 width/height/top/left；需要展开收起用 `grid-template-rows: 0fr→1fr` 或 opacity+transform，不触发布局；
- 单元素动画时长 ≤ `--dur-slow`（340ms），清理工具的"快感"优先；stagger 仅用于首屏卡片（每页 ≤8 级、级差 45ms），**禁止给长列表每行挂 delay**；
- 数字补间（容量/进度汇总）统一走 `composables/useCountUp.ts`（rAF + ease-out），高频更新的实时扫描字节数不补间、仅用 `font-variant-numeric: tabular-nums` 防跳动；
- 必须服从系统「减少动态效果」：`@media (prefers-reduced-motion: reduce)` 下全局动画/过渡一律归零（token 层统一处理，页面不各自判断）；
- **安全语义优先于观感**：🔴 灰禁/锁定、取消按钮、执行中状态的可见性与可点性不得被动画延迟、遮挡或弱化；完成勾号等反馈动画 ≤220ms；
- 动效只允许存在于视图层（.vue/composables），stores 与纯函数 models 零动画依赖（测试体系不变）；不引入动画重依赖（不使用 GSAP/Lottie/组件库自带过渡体系）。

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

不做杀毒/实时防护；不做"系统提速/注册表优化/内存整理"；无广告/推荐/全家桶（不可妥协）；不抄袭任何第三方工具实现；MVP 不做端侧中文意图搜索；不做跨端；不做账号/云存储/远程控制。R27 卸载辅助仅编排官方卸载器 + 隔离式残留清理：不做默认强杀进程、不做 UWP 应用卸载、不做驱动/服务级移除、不替代系统"应用和功能"。

## 10. 术语 ↔ PRD 需求映射

R01→rules+scanner｜R02→safety｜R03→scan 编排｜R04→cleaner｜R05→scanner.large+pages/files｜R06→cleaner.dup（§6.3）｜R07→startup｜R08→privacy｜R09→logging｜R16→外壳｜R13/R14→pages/report｜R21→测试体系｜R22→updates｜R23→guard+兼容｜R24→crash+journal 恢复｜R25→quarantine｜R26→CI/签名｜R27→uninstall（§6.7）。
