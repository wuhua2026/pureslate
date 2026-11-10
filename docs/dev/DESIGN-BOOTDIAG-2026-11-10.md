# PureSlate 开机诊断（只读）设计稿

- 日期：2026-11-10
- 状态：**设计评审稿，未实现**（本文不修改任何源码、不新增依赖、不提交）
- 适用范围：Windows 10 / Windows 11（x64）
- 关联：SPEC §2（模块）、§5（IPC 契约）、§6.7（卸载）、§9（设计 token）；AGENTS §4（红线）、§8（契约冻结）

---

## 0. 结论摘要（TL;DR）

1. **不调用 PowerShell，也不引入 `windows` crate**。沿用 P1-02b 的 `#[link]` extern 薄封装，新增 `wevtapi` / `powrprof` / `msi` 三个系统 DLL 绑定；安装包增量 ≈ 0（当前基线 2.75MB / 20MB）。
2. **一条只读命令** `boot_diagnose()` 返回四段各自带状态的报告：`timing` / `startup` / `residue` / `power`。单段失败不影响其他段。
3. **事件日志只解析 XML 字段，不解析本地化 `Message`**。事件 100 的字段全部 `Option` 化；注意微软没有正式 schema，且实际字段是 `BootDegradationDelta`（不是 `BootDegradationTime`）。
4. **服务、计划任务、事件日志在本机非提权下均不可得或不可靠**（实测见附录 B）。发布版 `requireAdministrator` 满足前提，但 dev 构建必须显式以管理员启动，否则 UI 显示 `denied` 而不是"0 项"。
5. **残留只做"注册表登记一致性"判定**：Uninstall 条目同时满足「安装目录缺失」+「卸载程序目标缺失」才算候选；MSI 条目单独用 Windows Installer API 规则，**不做软件名模糊匹配**，也不扫描文件系统找孤儿。
6. **唯一需要写操作的场景是"事件日志通道被关闭后重新开启"**。本设计选择**不实现写入**，只给用户手动开启指引，以维持"零写入"承诺。

---

## 1. 背景与边界

### 1.1 只吸收问题清单

参考的论坛 PowerShell 脚本用四类采集回答"为什么开机慢"。我们只吸收它的问题清单，不吸收实现：

| 参考脚本的问题 | 本设计的回答方式 |
|---|---|
| 开机耗时是多少、慢在哪 | 事件 100 的总量/主路径/后启动 + 101–110 明细 |
| 哪些东西在开机时启动 | Run/RunOnce、启动文件夹、自动服务、开机/登录计划任务 |
| 哪些卸载没卸干净 | Uninstall 登记 + 安装目录/卸载程序双缺失；MSI 产品码 |
| 快速启动/休眠开着吗 | `HiberbootEnabled` + `CallNtPowerInformation` |

不复制其代码、清理参数、资源或品牌词（AGENTS §4.5）。

### 1.2 Non-goals（本设计明确不做）

- 不清理、不禁用、不删除、不移动、不修改任何启动项/服务/任务/注册表/电源配置；
- 不做"一键优化"，不给启动项打"高/中/低影响"分（避免伪精度）；
- 不做无上下文的全局孤儿文件扫描（残留仅限注册表登记一致性）；
- 不解析事件描述的本地化文本（只读 XML 字段）；
- 不做后台常驻/实时监控；
- 不把诊断结果持久化到磁盘（仅内存中的一次报告）。

### 1.3 为什么不用 PowerShell 5.1

| 问题 | 具体表现 | 原生 API 的收益 |
|---|---|---|
| 进程启动开销 | 每个采集点一次 `powershell.exe` 冷启动 | 无进程派生，直接调用系统 DLL |
| 本地化文本 | `powercfg /a`、`schtasks /fo LIST` 的字段名随系统语言变化 | XML/结构体字段与语言无关 |
| 编码陷阱 | 实测 `schtasks /query /xml` 重定向输出为 **GBK（CP936）** 而非 UTF-8/UTF-16；按 UTF-8 解析会损坏中文任务名 | 原生 API 返回 UTF-16 / BSTR |
| AV 误报 | 脚本执行策略、命令行特征 | 只读 API 调用，无脚本宿主 |
| 权限口径 | 脚本自身是否提权不透明 | 明确 `is_elevated` + 分段 `denied` 状态 |

---

## 2. 数据来源与调用路径

### 2.1 依赖现状与选型

`src-tauri/Cargo.toml` 当前**没有 `windows` crate**，且 P1-02b 已记录"零第三方依赖、`#[link]` extern 直调"的决策。

**方案 A（推荐）：继续 extern 薄封装**

| 需要新增的链接 | 用途 | 体积影响 |
|---|---|---|
| `wevtapi.dll` | 事件日志查询/渲染/通道配置读取 | 系统 DLL，仅代码体积 |
| `powrprof.dll` | `CallNtPowerInformation` | 同上 |
| `msi.dll` | Windows Installer 产品枚举/信息读取 | 同上 |
| `advapi32.dll`（已有） | 注册表、服务控制管理器 | 无 |

安装包体积增量预计 < 10KB（纯代码），不触碰 20MB 门禁。

**方案 B（仅在确需大量 COM 时考虑）：引入 `windows` crate**

需要 features（已在 crates.io 元数据核实存在）：

```
Win32_System_EventLog
Win32_System_Power
Win32_System_Services
Win32_System_ApplicationInstallationAndServicing
Win32_System_Registry
Win32_System_TaskScheduler   // 若走 COM
Win32_System_Com             // 若走 COM
```

影响：编译时间明显增加，二进制估算 +100~300KB（未实测），且需按 AGENTS §4.7 做依赖准入并在 TASKS.md 记"依赖决策"。**本期不采用。**

### 2.2 开机耗时（事件日志）

**通道**：`Microsoft-Windows-Diagnostics-Performance/Operational`

**调用链（全部只读）**：

```
EvtOpenChannelConfig(NULL, channel, EvtOpenChannelPath)
  → EvtGetChannelConfigProperty(h, EvtChannelConfigEnabled, ...)   // 读通道是否启用
  → EvtClose(h)

EvtQuery(NULL, channel, xpath, EvtQueryChannelPath | EvtQueryReverseDirection)
  → 循环 EvtNext(..., batch=8, timeout=0)
      → EvtRender(NULL, event, EvtRenderEventXml, ...)             // 取 XML
      → quick-xml 解析 <Data Name="...">value</Data>
  → EvtClose(query)
```

**XPath**：`*[System[(EventID >= 100) and (EventID <= 110)]]`
若返回 `ERROR_EVT_INVALID_QUERY`，降级为 `*` 并在客户端按 EventId 过滤（防御性设计，不依赖 XPath 子集能力）。

**窗口**：最多 200 条事件 / 最近 7 天；事件 100 最多保留 10 次开机样本。`truncated=true` 表示被窗口截断。

**事件 100 字段（全部 `Option`）**：

| 字段 | 含义 | 备注 |
|---|---|---|
| `BootTime` | 本次开机总耗时（ms） | = MainPath + PostBoot（实测样本 34857 = 12657 + 22200） |
| `MainPathBootTime` | 从 Windows Logo 到桌面/登录界面（ms） | 核心指标 |
| `BootPostBootTime` | 桌面出现到系统达到 80% 空闲（ms） | 核心指标 |
| `BootDegradationDelta` | 相对基线的退化增量（ms） | **不存在 `BootDegradationTime`** |
| `BootIsDegradation` | 是否判定为退化 | 布尔 |
| `BootNumStartupApps` | 本次开机启动应用数 | 可与启动项数量交叉参考 |
| `BootStartTime` / `BootEndTime` | 起止时间 | 作为时间戳的兜底；优先用 `TimeCreated` |
| `BootKernelInitTime` / `BootDriverInitTime` / `BootDevicesInitTime` / `BootPrefetchInitTime` / `BootSmssInitTime` / `BootCriticalServicesInitTime` / `BootUserProfileProcessingTime` / `BootMachineProfileProcessingTime` / `BootExplorerInitTime` | 各阶段耗时 | 用于堆叠条；缺字段就缺段，不补 0 |
| `SystemBootInstance` / `UserBootInstance` / `BootTsVersion` | 实例号/版本 | 仅用于诊断，不参与 UI 主叙事 |

**事件 101–110（拖慢明细）**：

| EventId | 类别 | 常见名称字段 |
|---|---|---|
| 101 | 应用 | `File Name` / `Friendly Name` / `Version` |
| 102 | 驱动 | 同上 |
| 103 | 服务 | `Service Name` / `Friendly Name` / `Version` |
| 104 / 105 | 微软未说明 | 只显示 EventId |
| 106 | 后台优化 | 可能只有 `Total Time` |
| 107 / 108 | 机器策略 / 用户策略 | 可能只有 `Total Time` |
| 109 | 设备 | 名称字段未文档化，按别名表尝试 |
| 110 | 会话管理器初始化 | 可能只有 `Total Time` |

**解析原则**：不依赖固定顺序，按 `Data@Name` 建映射；`Total Time` / `Degradation Time` 统一转 ms；缺失即 `None`，绝不猜测。

**重要边界**：微软未发布事件 100 的正式 schema；字段名来自微软 Q&A 中的真实事件 XML 与 Microsoft Learn 归档 wiki（附录 A）。实现必须以真实 XML 为准，并保持全字段可选。

### 2.3 启动项来源

| 来源 | 具体位置/API | 状态来源 |
|---|---|---|
| Run / RunOnce（用户） | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`、`...\RunOnce` | StartupApproved（HKCU） |
| Run / RunOnce（机器） | `HKLM\Software\Microsoft\Windows\CurrentVersion\Run`、`...\RunOnce`，分别用 `KEY_WOW64_64KEY` / `KEY_WOW64_32KEY` 打开 | StartupApproved（HKLM） |
| 启动文件夹（用户） | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup` | StartupApproved（HKCU\...\StartupApproved\StartupFolder） |
| 启动文件夹（全体） | `%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup` | StartupApproved（HKLM\...\StartupApproved\StartupFolder） |
| 自动服务 | `OpenSCManagerW(SC_MANAGER_ENUMERATE_SERVICE)` → `EnumServicesStatusExW` → 逐项 `QueryServiceConfigW` | `dwStartType == SERVICE_AUTO_START`；`QueryServiceConfig2W(SERVICE_CONFIG_DELAYED_AUTO_START_INFO)` 取延迟启动 |
| 计划任务（开机/登录） | `%SystemRoot%\System32\Tasks\**\*.xml`，解析 `BootTrigger` / `LogonTrigger` + `Exec/Command` | XML 不含可靠启用状态（见 §3.5） |

**关键纠正**：

1. 不要硬编码 `Wow6432Node` 物理路径。微软明确该位置由系统保留，应使用 `KEY_WOW64_32KEY` 逻辑视图。
2. 现有 `startup/mod.rs` 的 `approved_disabled_names()` **只读 HKCU**；任务管理器对机器级启动项会写 HKLM。本模块必须同时读 HKLM 的 `Run` / `Run32` / `StartupFolder`，否则与任务管理器不一致。
3. `StartupApproved` 是**未文档化**的存储：首字节 `02/03` 视为禁用、`06` 视为启用，属于经验语义（项目已有单测）。UI 只能把它当"展示状态"，不能当安全判定依据。
4. 任务管理器的"启动"页只覆盖 Run/RunOnce/启动文件夹，**不包含服务与计划任务**；因此"与任务管理器一致"只对前三类做比对。

### 2.4 卸载残留登记

**不是**文件系统孤儿扫描，而是"注册表登记是否还站得住"的一致性检查。

| 条目类型 | 判定规则 |
|---|---|
| 普通（非 MSI） | `InstallLocation` 非空且不是 UNC/相对路径，且该目录不存在；同时 `UninstallString` / `QuietUninstallString` 解析出的可执行文件不存在 → 候选 |
| 卸载程序是 `msiexec.exe` | 视为系统卸载器存在 → **永不**因"卸载程序缺失"判残留 |
| MSI | 不套用双条件。用 `MsiEnumProductsExW` 枚举机器/当前用户产品码；若 Uninstall 键存在但产品码已不在枚举结果中 → 登记残留；`MsiGetProductInfoExW(INSTALLPROPERTY_INSTALLLOCATION)` 为空 → 标记"安装位置未知"，不判残留 |
| 排除项 | `SystemComponent=1`、有 `ParentKeyName`、`ReleaseType=Update/Rollback`、无 `DisplayName` |

**禁止**：按软件名/发布者模糊匹配来推测归属；扫描安装目录内容；访问 UNC 路径；把 MSI 的 `InstallLocation` 缺失当残留。

**为什么必须双条件**：本机实测 276 条有显示名的 Uninstall 条目中，**210 条没有 `InstallLocation`**（其中 166 条是 MSI）。单条件会误报约 76%。

**契约边界**：本模块只返回"登记残留"条目，**不返回 `ResidueCandidate`，不提供清理入口**；与既有 `uninstall_residue_scan`（授权根 + 文件级候选 + 隔离区）保持职责分离。

### 2.5 快速启动与休眠

| 数据 | 来源 | 读法 |
|---|---|---|
| 快速启动开关 | `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Power` → `HiberbootEnabled` (REG_DWORD) | 0=关，1=开；值缺失=未知 |
| 休眠能力 | `CallNtPowerInformation(SystemPowerCapabilities, ..., &caps, size_of::<SYSTEM_POWER_CAPABILITIES>())` | `SystemS4`=支持休眠；`HiberFilePresent`=休眠文件存在；`Hiberboot`=支持快速启动；`AoAc`=Modern Standby |
| 一致性 | 注册表 vs 能力 | `HiberbootEnabled=1` 且 `HiberFilePresent=false` → 展示"配置已开启，但休眠文件不可用" |

**不要**用 `C:\hiberfil.sys` 是否存在来判断：本机实测 `HiberbootEnabled=1` 但该文件不存在（真实矛盾态）；且非提权时 ACL 也会让探测失真。必须以 API 返回为准。

---

## 3. 真实 Windows 10/11 可得性与边界

### 3.1 事件日志通道

| 场景 | 依据 | 本设计行为 |
|---|---|---|
| 通道默认是否开启 | 微软未对具体通道承诺默认值；官方说明 admin/operational 通道事件会自动进入事件日志。本机 Win11 26H2 实测 `enabled: true` | 不假设默认值，运行时用 `EvtChannelConfigEnabled` 查询 |
| 通道被关闭 | `EvtChannelConfigEnabled` 只表示"是否继续收集"；既有 `.evtx` 通常仍可读 | 仍然查询；能读到就展示，并标注"通道当前已关闭，数据截至最后一条事件"；读不到则 `unavailable` |
| 通道被清空/滚动 | 日志有大小上限，旧事件会被覆盖 | 返回空样本 + `reason`，不报错 |
| 只看得到部分 | `truncated=true` | UI 显示"仅最近 N 次" |
| 权限不足 | 本机 ACL 为 `(A;;0x7;;;BA)(A;;0x5;;;LS)(A;;0x5;;;NS)(A;;0x1;;;SU)`，**没有交互用户 ACE**；非提权 `wevtutil qe` 返回 Access denied（退出码 5） | `denied` + "需要管理员权限"；不弹 UAC、不自动提权 |
| DPS 策略禁用开机性能检测 | `ADMX_PerformanceDiagnostics` 策略可关闭 Boot Performance 检测 | 无事件 → `unavailable` + 指向策略/通道说明 |

### 3.2 事件 100 的缺失与不完整

| 场景 | 依据/现象 | 处理 |
|---|---|---|
| 某次开机没有事件 100 | 微软支持工程师说明按阈值（PostBoot 30/60s、Boot 60/120s）判定，且"有些开机完全没有记录"；事件级别只有 Warning/Error/Critical，没有 Information | 取最近 N 条，不假设与开机次数 1:1 |
| `MainPathBootTime`/`BootPostBootTime` 缺失 | 旧版本/不同 `BootTsVersion` | `None`，UI 显示"—"，不反推 |
| `BootDegradationDelta` 缺失 | 字段并不总是存在 | `None`；不能用 `BootTime - 基线` 自行计算冒充官方值 |
| 只有 100、没有 101–110 | 归档 wiki 明确"事件可能单独出现"，此时无法从该日志定位原因 | 正常展示，提示"本次没有明细事件" |
| 时间字段格式差异 | `BootStartTime` 可能是 ISO8601 字符串，也可能是 FILETIME 整数 | 优先用 `TimeCreated`；解析失败则忽略该字段 |

### 3.3 休眠/快速启动

| 场景 | 依据 | 处理 |
|---|---|---|
| 系统不支持 S4 | `SYSTEM_POWER_CAPABILITIES.SystemS4 = false`（Modern Standby 机器常见） | `hibernateSupported=false` |
| 休眠文件不存在 | `HiberFilePresent=false`（`powercfg /h off` 后） | 明确展示"休眠已关闭/不可用" |
| 注册表值缺失 | 值可能不存在 | `fastStartupEnabled=None` + `registryPresent=false` |
| 注册表与能力矛盾 | 本机实测 1 + 文件不存在 | 黄色提示，不自动改任何设置 |
| 读取失败 | `CallNtPowerInformation` 返回非 0 NTSTATUS | 该段 `partial`，其余字段保留 |

### 3.4 权限边界汇总

| 数据 | 标准用户 | 管理员（发布版默认） |
|---|---|---|
| 事件 100/101–110 | ❌ Access denied（实测） | ✅ |
| `%SystemRoot%\System32\Tasks` | ❌ 枚举为 0（实测，ACL） | ✅ |
| HKLM Run / StartupApproved | ✅ 读 | ✅ 读 |
| HKCU Run / StartupApproved | ✅ 读 | ✅ 读 |
| 自动服务枚举 | 通常 ✅；个别服务配置可能 ❌ | ✅ |
| Uninstall（机器 + 当前用户） | ✅ 读 | ✅ 读 |
| MSI 当前用户/机器 | ✅ | ✅ |
| MSI 其他用户 | ❌（文档要求管理员） | ✅（本设计仍不枚举其他用户） |
| `HiberbootEnabled` | ✅ 读 | ✅ 读 |
| `CallNtPowerInformation` | ✅ 读 | ✅ 读 |

### 3.5 计划任务的坑

1. 非提权读 `System32\Tasks` 会**静默得到 0 个文件**（实测），不能把"0"当"没有任务"。
2. 现有 `read_to_string` 只接受 UTF-8，且解析失败就 `continue`；任务 XML 的存储编码未在官方文档中说明。必须改为"读字节 + BOM/编码判定 + 统计失败数"，失败时把该段标为 `partial`，而不是静默返回 0。
3. 任务 XML 不含可靠的启用状态（LESSONS 已记录）；因此本设计对计划任务只声明"存在开机/登录触发器"，`enabled` 为 `None`，不冒充任务管理器的口径。
4. 排除 `\Microsoft\` 系统命名空间可减少噪音，但诊断页可选择"显示系统任务"（默认折叠）。

---

## 4. 契约影响（M0 冻结纪律）

### 4.1 新增命令

```rust
// src-tauri/src/ipc/commands.rs
/// 开机诊断（只读，无参数）。重活放 spawn_blocking，避免阻塞 UI 线程。
#[tauri::command]
pub async fn boot_diagnose() -> Result<BootDiagnosisReport, String> { ... }
```

- 只新增这一个命令；不新增取消命令（目标耗时 < 2s）。
- 不使用事件流/进度事件（一次性返回，避免契约膨胀）。

### 4.2 新增类型（Rust 端）

```rust
// ---- 开机诊断（v0.2.x 加性 · 只读）----
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootSectionStatus { Ok, Partial, Unavailable, Denied }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootSection<T> {
    pub status: BootSectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootTimingData {
    pub channel_enabled: bool,
    pub samples: Vec<BootTimingSample>,
    pub issues: Vec<BootIssue>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootTimingSample {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub booted_at: Option<String>,            // RFC3339，来自 TimeCreated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_path_boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_boot_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_delta_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_degradation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_app_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phases: Option<BootPhaseBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPhaseBreakdown {
    #[serde(skip_serializing_if = "Option::is_none")] pub kernel_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub driver_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub devices_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub prefetch_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub smss_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub critical_services_init_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub user_profile_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub machine_profile_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub explorer_init_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootIssueKind {
    Application, Driver, Service, Device, MachinePolicy,
    UserPolicy, BackgroundOptimization, SessionManager, Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootIssue {
    pub event_id: u16,
    pub kind: BootIssueKind,
    #[serde(skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub friendly_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub total_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub degradation_time_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStartupKind {
    Run, RunOnce, StartupFolder, ServiceAuto,
    ScheduledTaskBoot, ScheduledTaskLogon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootStartupScope { User, Machine }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootApprovedState { Enabled, Disabled, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStartupEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub kind: BootStartupKind,
    pub scope: BootStartupScope,
    #[serde(skip_serializing_if = "Option::is_none")] pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub approved_state: Option<BootApprovedState>,
    #[serde(skip_serializing_if = "Option::is_none")] pub delayed_auto_start: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStartupData {
    pub entries: Vec<BootStartupEntry>,
    /// 枚举失败的子源（如 tasks_dir_denied），用于把"0 项"与"读不到"区分开。
    pub failed_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootResidueData {
    pub entries: Vec<BootResidueEntry>,
    /// 因信息不足（如 MSI 安装位置未知）而无法判定的条目数，不冒充残留。
    pub unknown_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootResidueEntry {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    pub scope: BootStartupScope,
    pub arch: String,                       // "x64" / "x86"
    pub is_msi: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub msi_product_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub install_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub uninstall_command: Option<String>,
    pub missing_install_dir: bool,
    pub missing_uninstaller: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPowerData {
    #[serde(skip_serializing_if = "Option::is_none")] pub fast_startup_enabled: Option<bool>,
    pub fast_startup_registry_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub hibernate_supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub hiber_file_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub hiberboot_capability: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub modern_standby: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootDiagnosisReport {
    pub collected_at: String,
    pub is_elevated: bool,
    pub timing: BootSection<BootTimingData>,
    pub startup: BootSection<BootStartupData>,
    pub residue: BootSection<BootResidueData>,
    pub power: BootSection<BootPowerData>,
}
```

### 4.3 TS 镜像（`src/types/ipc.ts`）

同名接口，字段 camelCase，`BootSection<T>` 用泛型；`reason` / `data` 全部可选。示例：

```ts
export type BootSectionStatus = "ok" | "partial" | "unavailable" | "denied";

export interface BootSection<T> {
  status: BootSectionStatus;
  reason?: string;
  data?: T;
}

export interface BootDiagnosisReport {
  collectedAt: string;
  isElevated: boolean;
  timing: BootSection<BootTimingData>;
  startup: BootSection<BootStartupData>;
  residue: BootSection<BootResidueData>;
  power: BootSection<BootPowerData>;
}
```

### 4.4 不触碰的既有契约

- 不修改 `StartupEntry` / `StartupSource` / `InstalledApp` / `ResidueCandidate` / `ResidueKind`；
- 不复用 `Impact` 给服务/任务打分（避免把启发式当事实）；
- 不把新来源硬塞进 `StartupSource`（会破坏前端穷举 switch 的兼容性），改用独立的 `BootStartupKind`。

### 4.5 CHANGELOG 记录内容

在 `docs/dev/CHANGELOG.md` 增一条加性变更：

> **2026-11-10 · 开机诊断（只读）**
> - 新增 IPC 命令 `boot_diagnose`（无参数，只读；不写注册表/不删文件/不改电源配置）。
> - 新增类型：`BootDiagnosisReport`、`BootSection<T>`、`BootSectionStatus`、`BootTimingData`、`BootTimingSample`、`BootPhaseBreakdown`、`BootIssue`、`BootIssueKind`、`BootStartupData`、`BootStartupEntry`、`BootStartupKind`、`BootStartupScope`、`BootApprovedState`、`BootResidueData`、`BootResidueEntry`、`BootPowerData`。
> - `contract.rs` 与 `ipc.ts` 双端同步；未修改任何既有命令签名或既有字段，旧前端调用不受影响。

---

## 5. 只读纪律落实

### 5.1 API 清单（逐条标注）

| # | API | 用途 | 读/写 |
|---|---|---|---|
| 1 | `EvtOpenChannelConfig` | 打开通道配置句柄 | 读 |
| 2 | `EvtGetChannelConfigProperty(EvtChannelConfigEnabled)` | 查询通道是否启用 | 读 |
| 3 | `EvtQuery` | 查询事件 | 读 |
| 4 | `EvtNext` | 取事件句柄 | 读 |
| 5 | `EvtRender(EvtRenderEventXml)` | 渲染为 XML | 读 |
| 6 | `EvtClose` | 关闭句柄 | 资源释放，不改数据 |
| 7 | `RegOpenKeyExW` / `RegEnumKeyExW` / `RegEnumValueW` / `RegQueryValueExW` | 读注册表 | 读 |
| 8 | `OpenSCManagerW(SC_MANAGER_ENUMERATE_SERVICE)` | 打开 SCM | 读 |
| 9 | `EnumServicesStatusExW` | 枚举服务 | 读 |
| 10 | `QueryServiceConfigW` | 读启动类型/命令行 | 读 |
| 11 | `QueryServiceConfig2W(SERVICE_CONFIG_DELAYED_AUTO_START_INFO)` | 读延迟启动 | 读 |
| 12 | `CloseServiceHandle` | 释放句柄 | 资源释放 |
| 13 | `MsiEnumProductsExW` | 枚举 MSI 产品码 | 读 |
| 14 | `MsiGetProductInfoExW` | 读产品名/安装位置/状态 | 读 |
| 15 | `CallNtPowerInformation(SystemPowerCapabilities)` | 读电源能力 | 读 |
| 16 | `fs::read`（任务 XML）/ `fs::metadata`（目录/卸载器存在性） | 只读探测 | 读 |
| 17 | `ExpandEnvironmentStringsW` | 展开 `%ProgramFiles%` 等 | 读 |
| 18 | `GetLastError` / `GetTickCount64` | 错误/耗时 | 读 |

**不链接、不调用**：`RegSetValueExW`、`EvtSetChannelConfigProperty`、`ChangeServiceConfigW`、`MsiConfigureProductExW`、`SetSystemPowerState`、`InitiateSystemShutdownExW`、任何 `WriteFile` / `CreateFileW` 写模式。

### 5.2 注册表路径清单（逐条标注）

| Hive | 路径 | 视图 | 用途 | 读/写 |
|---|---|---|---|---|
| HKCU | `Software\Microsoft\Windows\CurrentVersion\Run` | 默认 | 用户启动项 | 读 |
| HKCU | `Software\Microsoft\Windows\CurrentVersion\RunOnce` | 默认 | 用户一次性启动项 | 读 |
| HKLM | `Software\Microsoft\Windows\CurrentVersion\Run` | 64 / 32 | 机器启动项 | 读 |
| HKLM | `Software\Microsoft\Windows\CurrentVersion\RunOnce` | 64 / 32 | 机器一次性启动项 | 读 |
| HKCU | `Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` | 默认 | 用户 Run 启停状态 | 读 |
| HKCU | `...\StartupApproved\StartupFolder` | 默认 | 用户启动文件夹启停状态 | 读 |
| HKLM | `SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run` | 64 | 机器 Run 启停状态 | 读 |
| HKLM | `...\StartupApproved\Run32` | 64 | 机器 32 位 Run 启停状态 | 读 |
| HKLM | `...\StartupApproved\StartupFolder` | 64 | 全体用户启动文件夹启停状态 | 读 |
| HKLM | `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall` | 64 / 32 | 已安装程序登记 | 读 |
| HKCU | `Software\Microsoft\Windows\CurrentVersion\Uninstall` | 默认 | 当前用户登记 | 读 |
| HKLM | `SYSTEM\CurrentControlSet\Control\Session Manager\Power` | 64 | `HiberbootEnabled` | 读 |

> 实现约束：机器级 32 位视图一律用 `KEY_WOW64_32KEY`，不直接拼 `Wow6432Node`。

### 5.3 文件系统访问

| 路径 | 操作 | 说明 |
|---|---|---|
| `%APPDATA%\...\Startup` | 列目录 | 只列顶层文件，不递归 |
| `%ProgramData%\...\Startup` | 列目录 | 同上 |
| `%SystemRoot%\System32\Tasks\**\*.xml` | 只读文件 | 限制深度/数量，统计失败数 |
| `InstallLocation` | `metadata()` 存在性 | 仅本地绝对路径；UNC/相对路径直接跳过 |
| `UninstallString` 解析出的 exe | `metadata()` 存在性 | 只解析第一个可执行路径，不执行 |

不打开文件内容（任务 XML 除外），不遍历安装目录内部，不访问网络路径。

### 5.4 唯一可能需要写操作的场景

**事件日志通道被关闭**时，开启它需要：

- `EvtSetChannelConfigProperty(EvtChannelConfigEnabled, TRUE)`，或 `wevtutil sl <channel> /e:true`；
- 管理员权限；
- 属于修改系统配置，不是"清理用户数据"，但仍是写操作。

**设计选择：不实现写入。** UI 只显示：

> 该日志通道当前已关闭，无法获得新的开机记录。可在"事件查看器 → 应用程序和服务日志 → Microsoft → Windows → Diagnostics-Performance → Operational"中手动启用；PureSlate 不会替你修改系统设置。

这样维持"零写入"承诺。若未来评审认为必须内置开启，应单独作为 `[DESTRUCTIVE]` 级任务：二次确认 + 审计日志 + 可回滚（记录原值），且不得默认开启。

---

## 6. UI 呈现

### 6.1 位置与信息架构

放在既有 **"启动项"页（`src/pages/Startup.vue`）顶部**，作为"开机诊断"卡片，位于现有启动项列表之上。

理由：

- 用户对"开机慢"的心智入口就是启动项页；
- 不新增一级导航，避免为一个只读诊断增加信息架构成本；
- 启动项列表仍是唯一可操作入口；诊断卡片只读、无禁用按钮。

交互：

1. 默认**不自动执行**，显示一行 CTA："运行开机诊断（只读，约 1–2 秒）"；
2. 点击后调用 `boot_diagnose()`，显示骨架屏；
3. 结果留在组件内存，离开页面即丢弃；提供"重新诊断"；
4. 四段各自显示状态；`denied` 段显示管理员提示，不影响其他段。

### 6.2 组件拆分（不引入组件库）

| 组件 | 职责 | 复杂度 |
|---|---|---|
| `BootDiagnosisPanel.vue` | CTA/加载/刷新/2×2 布局/错误汇总 | 中 |
| `BootTimingCard.vue` | 4 个指标 + 阶段堆叠条 + 最近 N 次表 + 101–110 明细 | 高 |
| `BootStartupSourcesCard.vue` | 来源统计 + 服务/任务表（Run/文件夹指向下方既有列表） | 中 |
| `BootResidueCard.vue` | 只读登记残留表 + "不提供清理"标识 | 低 |
| `BootPowerCard.vue` | 快速启动/休眠两行状态 + 矛盾提示 | 低 |
| `BootStatusChip.vue` | 统一 `ok/partial/unavailable/denied` 徽标 | 低 |

所有组件 `<script setup lang="ts">`，类型从 `src/types/ipc.ts` 导入，不重复定义。

### 6.3 视觉与 token

只使用现有 CSS 变量（`src/App.vue`）：

- 容器：`--surface`、`--border`、`--r-card`、`--shadow-card`；
- 文字：`--text` / `--text-2` / `--text-3`；
- 强调：`--accent` / `--accent-soft`；
- 严重度：`--grade-green` / `--grade-yellow` / `--grade-red`（只用于状态，不用于装饰）；
- 动效：`--dur-fast` / `--dur-base` / `--ease-out`，并尊重 `prefers-reduced-motion`。

**阶段堆叠条**：纯 CSS `display:flex` + `flex-basis: %`，段间 2px `gap`；每段有 `title` 与文本摘要。**不声称各段之和等于总耗时**（事件字段可能缺失/存在未列出的阶段），文案写"可得阶段构成"。

**不用**：环形图库、图表库、组件库、远程字体。

### 6.4 状态矩阵

| 段 | 状态 | 卡片表现 |
|---|---|---|
| timing | `ok` | 指标 + 堆叠条 + 明细 |
| timing | `partial` | 有样本但字段缺失；缺项显示"—" |
| timing | `unavailable` | "没有可用的开机记录" + 通道/策略说明 |
| timing | `denied` | "需要管理员权限读取系统日志" |
| startup | `ok` | 来源统计 + 表 |
| startup | `partial` | 部分来源读不到（`failedSources` 非空），明确列出 |
| residue | `ok` | 表 + "只读登记检查，不提供清理" |
| residue | `partial` | 部分条目"安装位置未知"，单独计数 |
| power | `ok` | 两行状态 |
| power | `partial` | API 失败但注册表可读（或反之） |

### 6.5 文案要求

- `denied` 不写"请以管理员身份运行"就完事，要写清"为什么需要"（系统日志 ACL 默认不授予标准用户）；
- 不出现"建议清理/建议禁用"等行动指令；
- 残留卡片固定标注"只读诊断，不提供清理入口"；
- 事件字段缺失时显示"—"，不显示 0；
- 矛盾态（快速启动注册表=1 但休眠文件缺失）用黄色提示，不自动修复。

### 6.6 性能与线程

- 命令内部 `tauri::async_runtime::spawn_blocking`，避免阻塞 IPC 线程；
- 目标：提权机器 p95 < 1.5s；超过 3s 时 UI 显示"仍在读取"；
- 不缓存到磁盘；同一页面会话内可保留结果；
- 事件查询上限 200 条；任务文件上限 2000 个；Uninstall 条目全量（通常 < 500）。

---

## 7. 验证方式

### 7.1 单元测试能覆盖的层

| 对象 | 测试方式 | 用例要点 |
|---|---|---|
| 事件 100 XML 解析 | 固定 fixture（真实样本 + 缺字段样本） | 34857/12657/22200 精确；缺 `BootPostBootTime` → `None`；`BootDegradationDelta` 不误读成 `BootDegradationTime` |
| 101–110 解析 | 每种 EventId 一个 fixture | 名称别名、`Total Time`、`Degradation Time`；104/105 → `Unknown` 且不丢失 event_id |
| 事件 XPath 降级 | 纯函数测试 | `ERROR_EVT_INVALID_QUERY` 时走客户端过滤 |
| StartupApproved 判定 | 纯函数 | 空值、`02/03/06`、大小写；HKCU+HKLM 合并去重 |
| 服务过滤 | 纯函数 | `SERVICE_AUTO_START` 入选；`DEMAND_START`/`DISABLED` 排除；延迟启动标记 |
| 残留判定 | 注入式 `PathProbe` 表驱动 | 双缺失才入选；`msiexec` 永不因卸载器缺失入选；UNC/相对路径跳过；`SystemComponent` 排除；MSI 走独立规则 |
| 计划任务解析 | fixture | `BootTrigger`、`LogonTrigger`、空触发器、UTF-16/UTF-8/BOM、损坏 XML → 失败计数 |
| 电源结构体 | `offset_of!` + 字节 fixture | `HiberFilePresent`/`SystemS4`/`Hiberboot` 偏移正确；`#[repr(C)]` 布局不被优化 |
| 报告组装 | 纯函数 | 单段失败不影响其他段；`status` 与 `reason` 一致 |

生产代码禁止 `unwrap/expect/panic`（AGENTS §4.6）；所有 FFI 返回值显式处理，句柄用 RAII guard 关闭。

### 7.2 必须真机验证的部分

| 项 | 机器/条件 | 判定 |
|---|---|---|
| 事件日志读取 | 提权 Win10 22H2 / Win11 23H2 / 24H2+ | 最近 5 条与事件查看器 XML 逐字段一致 |
| 非提权降级 | 标准用户或非提权 dev 构建 | `timing.status=denied`，不崩、不显示 0 条 |
| 通道关闭 | VM 上 `wevtutil sl ... /e:false` | 仍能读历史事件并标注；无历史 → `unavailable` |
| 通道清空 | VM 上清空日志 | 空样本 + `reason`，不报错 |
| 休眠关闭 | VM 上 `powercfg /h off` | `hiberFilePresent=false`；快速启动显示矛盾提示 |
| 快速启动开关 | 设置 → 电源选项 | `HiberbootEnabled` 与 UI 一致 |
| 服务口径 | 与 `services.msc` 对照 | 自动/延迟自动集合完全一致 |
| 计划任务口径 | 与 `Get-ScheduledTask` 对照 | Boot/Logon 触发集合一致；中文任务名不乱码 |
| Uninstall 口径 | 与"应用和功能"对照 | 条目集合一致；残留候选人工复核无误报 |
| MSI 口径 | 安装一个 MSI 后卸载/损坏 | 产品码、安装位置、状态正确 |
| 32 位视图 | 仅安装 32 位程序 | 机器级 32 位 Run/Uninstall 不漏 |
| 中文路径 | 中文用户名、中文安装目录 | 不截断、不乱码 |
| 任务 XML 编码 | 提权读取真实 `System32\Tasks` | 编码判定正确；解析失败计数可见 |

### 7.3 无自动化时如何证明"是对的"

用**差分测试**把参考实现当 oracle（只读、只在开发机执行，不进入产品）：

```powershell
# 1) 开机事件：逐字段对照
Get-WinEvent -FilterHashtable @{
  LogName='Microsoft-Windows-Diagnostics-Performance/Operational'; Id=100
} -MaxEvents 5 | ForEach-Object { $_.ToXml() }

# 2) 启动项集合
Get-CimInstance Win32_StartupCommand | Select-Object Name,Command,Location,User

# 3) 自动服务集合
Get-CimInstance Win32_Service -Filter "StartMode='Auto'" |
  Select-Object Name,DisplayName,DelayedAutoStart,PathName

# 4) Boot/Logon 计划任务集合
Get-ScheduledTask | Where-Object {
  $_.Triggers.CimClass.CimClassName -match 'MSFT_TaskBootTrigger|MSFT_TaskLogonTrigger'
} | Select-Object TaskPath,TaskName,State

# 5) 电源状态（仅作第二证据，不解析其本地化文本）
Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power' -Name HiberbootEnabled
```

验收口径：

- 事件：最新 5 条的 `BootTime/MainPathBootTime/BootPostBootTime/BootDegradationDelta` **数值完全相等**；
- 启动项：以 `(scope, kind, name, command)` 为键，集合差为 0（系统命名空间任务除外，需在报告中显式说明）；
- 服务：以 `(name, delayed)` 为键，集合差为 0；
- Uninstall：以 `(scope, arch, key)` 为键，集合差为 0；
- 电源：注册表值完全相等；休眠能力以 API 结果为准，`powercfg /a` 只做人工旁证。

**注意**：差分脚本只用于开发验证，不复制进产品，不作为运行时依赖。

### 7.4 验证清单（评审时逐项打勾）

- [ ] `pnpm typecheck` 通过
- [ ] `cargo fmt && cargo clippy -- -D warnings` 通过
- [ ] `cargo test` 全部通过（含新增单测）
- [ ] 非提权运行：四段状态正确，无 panic
- [ ] 提权运行：`timing` 有数据且与事件查看器一致
- [ ] 通道关闭/清空：状态正确
- [ ] 休眠关闭：状态与矛盾提示正确
- [ ] 中文任务名/中文路径不乱码
- [ ] 残留候选人工复核 20 条，0 误报
- [ ] 安装包体积仍 < 20MB（预期不变）
- [ ] 日志中不出现完整用户路径/卸载命令行（隐私）
- [ ] CHANGELOG / SESSIONS / LESSONS 已更新

### 7.5 差分测试记录（2026-11-10，非提权实跑）

| 项 | 参考（oracle） | 本实现 | 结论 |
|---|---|---|---|
| 启动项 | `Get-CimInstance Win32_StartupCommand` = **4** | `startup_sources::collect()` = **4** | ✅ 集合一致。边界：本实现把 `%windir%` 展开为绝对路径；启动文件夹的 `command` 是完整路径，oracle 只给文件名 |
| 自动服务 | `Win32_Service StartMode='Auto'` = **111** | SCM `QueryServiceConfig.START_TYPE` = **112** | ✅ 差集仅 `LSM`：`sc qc LSM` 明确 `AUTO_START`，WMI 对受保护服务报 `StartMode=Unknown`（oracle 盲区，见 LESSONS §①） |
| Boot/Logon 计划任务 | `Get-ScheduledTask`（COM）= **49** | 文件读取 = **0** + `tasks_dir_empty_or_denied` | ⚠️ 非提权 ACL 边界。状态显式标注，不伪装成"0 条正常"；文件读取与 COM 是两条权限通道 |
| 卸载条目 | 注册表 276 键 → 同口径过滤/去重 = **112** | `uninstall::list_apps` = **112** | ✅ 完全一致；`residue=0`、`unknown=64` |
| MSI 产品 | `MsiEnumProductsExW` = **167** | 同 = **167** | ✅ 同一 API，作回归基线 |
| 电源 | 注册表 `HiberbootEnabled=1`；`powercfg /a`：休眠未启用、S0 低电量待机 | `fast_startup=true`、`HiberFilePresent=false`、`SystemS4=true`、`AoAc=true` | ✅ 配置意图与能力分离，矛盾按设计呈现 |
| 开机事件 | 事件查看器 / `Get-WinEvent` | 非提权 `Denied` | ⛔ 与 oracle 同受 ACL 阻挡，未取得提权样本；按评审决定不做提权入口 |

口径说明：差分脚本只在开发机执行、不进入产品（§7.3）；服务以 SCM 为准、WMI 仅旁证；"未做成"的项按权限边界记录，不折算成"通过"。

---

## 8. 任务拆分与风险

> 任务编号为建议值（B1 系列），评审通过后再写入 `docs/dev/TASKS.md`。复杂度用 S/M/L，不使用时间估算。

| 任务 | 内容 | 复杂度 | 风险 | 主要验证 |
|---|---|---|---|---|
| **B1-00 取证** | 在提权 VM 上确认：事件 100 在 Win10/11 的真实字段；任务 XML 存储编码；HKLM StartupApproved 实际键；MSI 产品枚举权限 | S | 中 | 采样记录写入 LESSONS |
| **B1-01 契约** | `contract.rs` + `ipc.ts` 双端类型、命令占位、mock、CHANGELOG | S | 低 | `pnpm typecheck` + 双端 diff |
| **B1-02 注册表源** | Run/RunOnce 双视图、启动文件夹、HKLM+HKCU StartupApproved、纯函数单测；**同时修复既有 HKCU-only 缺口** | M | 中高 | 与任务管理器逐项对照 |
| **B1-03 事件日志** | wevtapi FFI、通道状态、EvtQuery/Next/Render、XML 解析、XPath 降级、句柄 RAII、fixture 单测 | **L** | **高** | 与事件查看器逐字段对照 |
| **B1-04 服务** | SCM 枚举 + 逐项 QueryServiceConfig/2、延迟启动、失败计数 | M | 中 | 与 `services.msc` 集合对照 |
| **B1-05 计划任务** | 提权读 Tasks XML、BOM/编码判定、Boot/Logon 触发器、失败计数；不沿用静默 `continue` | M | 高 | 与 `Get-ScheduledTask` 对照 |
| **B1-06 残留登记** | 复用 `uninstall::list_apps`、MSI API 枚举/信息、双条件判定、UNC/相对路径守卫、单测 | M | 中高 | 人工复核 20 条 |
| **B1-07 电源** | `CallNtPowerInformation` + `HiberbootEnabled`、`#[repr(C)]` 布局与 `offset_of!` 测试 | S/M | 中 | 开关切换对照 |
| **B1-08 UI** | 6 个组件 + Startup 页集成 + 状态矩阵 + 可访问性 + 原生 CSS/token | M | 低 | 手工走查 + typecheck |
| **B1-09 收尾** | 差分测试记录、TASKS/CHANGELOG/SESSIONS/LESSONS 回填、体积复测 | S | 低 | 清单全绿 |

### 最容易出错的三步

1. **B1-03 事件日志 FFI**：句柄泄漏、`EvtRender` 两次缓冲、XPath 失败、字段别名、权限错误码混淆（`ERROR_ACCESS_DENIED` vs `ERROR_EVT_CHANNEL_NOT_FOUND`），且事件 XML 无正式 schema。建议先写纯解析器 + fixture，再写 FFI。
2. **B1-05 计划任务**：非提权静默 0 项、任务 XML 编码、XML 无启用状态。必须把"读不到"与"没有"分开，且不能沿用现有静默 `continue`。
3. **B1-06 残留误报**：本机 210/276 条无安装目录，单条件必炸；MSI 必须独立规则；任何"按名字猜"都要在评审中直接否决。

---

## 9. 未决问题（需评审拍板）

1. **任务启用状态**：是否接受"计划任务不提供启用状态"（`enabled=None`）？若要，需引入 Task Scheduler COM 或解析非官方存储，成本与风险都会上升。
2. **HKLM StartupApproved 补丁范围**：只在本模块内读，还是顺带修复既有 `startup_list`（会影响 P3-03/R07 的行为）？建议单独任务，避免混提。
3. **卸载命令展示**：残留卡片是否展示完整 `UninstallString`（可能含用户路径）？建议默认折叠、点击展开。
4. **事件窗口**：默认最近 5 次开机 / 7 天是否合适？是否需要设置项（会引入配置契约）。
5. **系统任务/微软服务噪音**：默认折叠还是默认隐藏？建议默认折叠但计数可见。
6. **是否允许内置"开启事件通道"**：本设计选择不允许；若产品坚持，需按写操作单独评审。

---

## 附录 A：参考资料（官方优先）

**事件日志与开机性能**

1. Event ID 100 / Boot Performance Events 100–110（Microsoft Learn 归档 TechNet Wiki）
   https://learn.microsoft.com/en-us/archive/technet-wiki/11559.event-id-100-windows-diagnostics-performance
2. `EvtGetChannelConfigProperty`（winevt.h）
   https://learn.microsoft.com/en-us/windows/win32/api/winevt/nf-winevt-evtgetchannelconfigproperty
3. `EVT_CHANNEL_CONFIG_PROPERTY_ID`（`EvtChannelConfigEnabled`）
   https://learn.microsoft.com/en-us/windows/win32/api/winevt/ne-winevt-evt_channel_config_property_id
4. `EvtQuery`（winevt.h）
   https://learn.microsoft.com/en-us/windows/win32/api/winevt/nf-winevt-evtquery
5. PerformanceDiagnostics ADMX 策略（可关闭 Boot Performance 检测）
   https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-admx-performancediagnostics
6. 微软支持工程师关于事件 100 阈值与"某些开机无记录"的答复（归档论坛，二手但来自 MSFT）
   https://social.technet.microsoft.com/Forums/windows/en-US/48005f5d-5f66-439a-af51-3a2ebb894e31/diagnosticsperformance-log-event-100-critical-error-or-warning-when-and-why
7. 事件 100 真实字段列表（第三方研究，用于交叉印证；不作为唯一依据）
   https://eventlogxp.com/blog/windows-boot-performance-diagnostics-1/

**电源与休眠**

8. `CallNtPowerInformation`（powerbase.h）
   https://learn.microsoft.com/en-us/windows/win32/api/powerbase/nf-powerbase-callntpowerinformation
9. `SYSTEM_POWER_CAPABILITIES`（winnt.h）
   https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_power_capabilities
10. `HiberbootEnabled` 注册表键（HORM 文档）
    https://learn.microsoft.com/en-us/windows/configuration/unified-write-filter/hibernate-once-resume-many-horm
11. 系统电源状态 S4 / fast startup
    https://learn.microsoft.com/en-us/windows/win32/power/system-power-states

**启动项与注册表**

12. Run / RunOnce Registry Keys
    https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys
13. Registry Redirector（Wow6432Node 为系统保留）
    https://learn.microsoft.com/en-us/windows/win32/winprog64/registry-redirector

**卸载与 MSI**

14. Windows Installer Properties for the Uninstall Registry Key
    https://learn.microsoft.com/en-us/windows/win32/msi/uninstall-registry-key
15. `MsiEnumProductsExW`
    https://learn.microsoft.com/en-us/windows/win32/api/msi/nf-msi-msienumproductsexw
16. `MsiGetProductInfoExW`
    https://learn.microsoft.com/en-us/windows/win32/api/msi/nf-msi-msigetproductinfoexw

**服务与计划任务**

17. `EnumServicesStatusExW`
    https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-enumservicesstatusexw
18. `QueryServiceConfigW`
    https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-queryserviceconfigw
19. BootTrigger 元素（Task Scheduler Schema）
    https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-boottrigger-triggergroup-element
20. `schtasks query`（`/xml` 输出全部任务定义）
    https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/schtasks-query

---

## 附录 B：本机实测记录（2026-11-10）

环境：Windows 11 26H2，build 26300.9711，PowerShell 7，**非提权**。

| 探测 | 结果 |
|---|---|
| 当前进程提权 | `False` |
| Diagnostics-Performance 通道 `enabled` | `true` |
| 通道 ACL | `O:BAG:SYD:(D;;0xf0007;;;AN)(D;;0xf0007;;;BG)(A;;0xf0007;;;SY)(A;;0x7;;;BA)(A;;0x5;;;LS)(A;;0x5;;;NS)(A;;0x1;;;SU)`（无交互用户 ACE） |
| `wevtutil qe` 非提权 | `Access is denied`，退出码 5 |
| `HiberbootEnabled` | `1` |
| `C:\hiberfil.sys` | 不存在（`Get-Item` 报"路径不存在"，非权限错误） |
| `System32\Tasks` 文件枚举（非提权） | `0` 个文件 |
| `Get-ScheduledTask`（非提权） | 208 个任务；Boot/Logon 触发 49 个 |
| `schtasks /query /xml`（非提权） | 371ms，约 350KB；`<BootTrigger>` 16 个、`<LogonTrigger>` 34 个；输出编码实测为 **GBK/CP936**（UTF-8 解出 0 个 CJK，CP936 解出 30 个） |
| Uninstall 条目（有 DisplayName） | 276 条；无 `InstallLocation` 210 条；MSI 166 条 |
| 自动服务（`Win32_Service` StartMode=Auto） | 111 个 |
| 安装包基线 | 2.75MB（`docs/verify/size-report.json`，上限 20MB） |
| 发布版提权 | `src-tauri/build.rs` 嵌入 `requireAdministrator` |

结论：

1. 事件日志与任务目录在非提权下不可用，必须分段 `denied`；
2. `schtasks` 的 GBK 输出是真实陷阱，佐证"不用进程 + 文本"的设计选择；
3. 快速启动注册表值与休眠文件可以矛盾，必须交叉校验；
4. 单条件残留判定会大面积误报。
