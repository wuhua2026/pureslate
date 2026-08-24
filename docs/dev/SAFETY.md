# PureSlate · 安全规范（SAFETY.md）

> 所有涉及删除/移动/写入用户数据/注册表/进程的任务（TASKS.md 中标注 `[DESTRUCTIVE]`）在动手前必读本文。本文是 PRD 信任主张（O1）的工程化落地，优先级高于任何进度压力。

## 1. 安全分级判定（R02 · safety 模块实现依据）

| 档位 | 判定维度 | 典型特征 | 默认交互 | 去向 |
|------|---------|---------|---------|------|
| 🟢 green | 可逆性极高；影响面无；误删后果无 | 删了随时重建或本就该删：%TEMP%、回收站内容、浏览器缓存、缩略图缓存 | 普通模式默认可勾 | **direct 直清 或 recycle 回收站**（立即释放空间） |
| 🟡 yellow | 可逆性中；影响面单应用；误删后果轻微 | 微信/QQ 缓存、聊天媒体、下载目录大文件 | 默认展开待确认 | **quarantine 隔离区**（14 天可还原） |
| 🔴 red | 可逆性低/不可逆；影响面系统/多应用；后果严重 | 注册表项、系统文件、用户文档、驱动 | 灰禁+专家模式+二次确认 token | **quarantine 隔离区**（14 天可还原） |

实现规则：
- 分级以规则 XML 的 `risk` 属性声明为准（SPEC §4.1）；
- **无法判定时默认 🔴**（保守原则），并记日志；
- 🟢 误删率目标 <0.5%、🟡 <2%、🔴 零容忍（M2 分档门禁，度量方法见 §6）。

## 2. 白名单（初版 · 由规则引擎在匹配阶段强制排除）

以下路径/对象**永远不得进入任何删除候选**，无论规则如何声明：

1. 系统目录：`%SystemRoot%`（C:\Windows）及其子目录（`C:\Windows\Temp` 等显式声明的例外由规则集白名单子句放行）；
2. 程序目录：`%ProgramFiles%`、`%ProgramFiles(x86)%`、`%ProgramData%\Microsoft`；
3. 引导与系统卷信息：`C:\Boot`、`C:\EFI`、`C:\Recovery`、`System Volume Information`、`$Recycle.Bin`（回收站本体走专用 API，不做路径级删除）；
4. 用户核心数据目录（默认 🔴 且不建议清理）：`Documents`、`Desktop`、`Pictures`、`Videos`、`Music` 的**非缓存子路径**（例外：这些目录下被规则显式标记为缓存的路径，如 `Pictures\Thumbnails`）；
5. 运行中进程的可执行文件与已加载 DLL（通过系统句柄快照判定）；
6. 自身设施：`.pureslate-quarantine\` 隔离区目录、journal 目录、manifest、日志文件——只能由对应模块按状态机操作，不进通用清理；
7. OneDrive/云盘占位文件（reparse point，属性含 `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`）：跳过不扫描，UI 提示为已知限制；
8. 符号链接/junction 指向的目标路径不跟随、不删除（见 §4.3）。

白名单维护：白名单初版固化在 `safety/whitelist.rs` 常量 + `resources/rules/whitelist.xml`（可随规则包更新）；变更须走 [DESTRUCTIVE] 评审关卡。

## 3. 事务化删除协议（R04/R24 · cleaner/journal 模块）

**两阶段 journal**，位置 `%LOCALAPPDATA%\PureSlate\journal\<txId>.jsonl`：

```
{"seq":1,"phase":"intent","op":"delete","path":"...","disposition":"direct","ts":"..."}
{"seq":2,"phase":"result","op":"delete","path":"...","ok":true,"ts":"..."}
```

规则：
1. **每个文件操作前写 intent 行，操作后写 result 行**——崩溃后启动扫描 journal，发现无 result 的 intent 即执行恢复；
2. 恢复语义按去向：`quarantine` 的 intent 未完成 → 将已移入文件还原；`recycle`/`direct` 无法逆操作 → journal 记录 `orphan` 状态并在 UI 醒目提示（🟢 类可重建，损害可控）；
3. txId 为 UUID v4，一次 `clean_execute` 一个事务，UI 按 txId 查询进度与结果；
4. journal 文件追加写（O_APPEND），禁止重写整文件；
5. 单文件操作失败不中断整个事务：跳过、记 `result.ok=false`、最终汇总报告。

## 4. 隔离区操作规范（R25 · quarantine 模块）

### 4.1 布局
- **同盘原则**：每个卷根下 `.pureslate-quarantine\`（hidden+system 属性），同盘 move = 瞬时 rename，不跨盘复制；
- 卷内 manifest：`.pureslate-quarantine\manifest.jsonl`（逐行 JSON，SPEC §4.3）；
- 用户数据结构 `%LOCALAPPDATA%\PureSlate\`：settings.json、journal\、logs\。

### 4.2 移入（quarantine）
1. 写 journal intent → 2. `move`（同盘 rename；跨盘时降级 copy+fsync+delete，且必须先校验目标 hash 一致）→ 3. 写 manifest 行 → 4. journal result。
- 目标子路径按 `yyyyMM\<batchId>\<原路径 hash 前缀>_<原文件名>` 组织，防同名冲突；
- 移入后写回文件访问时间为原值（保留取证信息）。

### 4.3 还原（restore）
- 按 manifest 的 `originalPath` 还原；原路径被占用/已存在/权限变化 → 还原到 `%LOCALAPPDATA%\PureSlate\restore-conflict\<日期>\` 并在返回报告中明确告知（计入 M12 分母）；
- 还原成功 → manifest 行 `state` 改 `restored`（保留记录 30 天后清理行）；
- **M12 门禁：还原成功率 ≥99.9%（1000 次自动测试 ≤1 失败）**，测试脚本见 §6.2。

### 4.4 生命周期（lifecycle）
- 默认保留 14 天（settings 可配 7–30）；到期前 3 天发 `quarantine_expiry_warning` 事件（UI 通知）；
- 到期自动清除 = 硬删隔离区文件 + manifest 置 `purged` + 写审计日志（去向字段 `auto-purge`）；
- 容量上限 = min(5GB, 所在盘剩余空间 10%)；超限提示用户显式确认释放最早批次，**不得静默丢弃**；
- 「确认清空」须二次确认 token（同 🔴 语义）。

### 4.5 空间语义（UI 强制）
任何页面**严格区分**「已释放 X GB」（direct/recycle/已 purge）与「已隔离 Y GB（尚未释放）」。完成页、隔离区页首行必须出现该区分文案，禁止混用"已清理"模糊表述。

## 5. 进程守卫（R23 双语义 · guard 模块）

1. **目标应用守卫**：清理某应用的缓存/数据前，检测其主进程（规则 XML `<guard process="WeChat.exe"/>`）是否运行；运行中 → 该类目整体阻止 + UI 提示"请先退出 XX"，**不得只跳过部分文件**（半清缓存比不清更危险）；
2. **自守卫**：启动时检测单实例（命名互斥量 `Global\PureSlateSingleInstance`）；第二实例激活首实例窗口后退出。

## 6. 度量与测试口径

### 6.1 M2 误删率分档
- 黄金文件集（`tests/golden/`，TASKS P4-04 建）：预置已知"安全删/不可删"样本树 → 跑扫描+清理 → 对比预期清单；
- 误删率 = 错删的"不可删"文件数 / 该档位操作总数；🔴 档任一误删即 CI 失败。

### 6.2 M12 还原成功率
- `tools/test-restore.ps1`：循环 1000 次「造样本 → clean_execute(quarantine) → restore → sha256 比对」；
- 失败 ≤1 次为过；脚本输出 JSON 报告存 `docs/verify/`。

### 6.3 [DESTRUCTIVE] 任务必测边界（评审关卡核对清单）
1. 路径含中文/emoji/空格的用户名（如 `C:\Users\小明 🎮\`）；
2. 路径 >260 字符（长路径 `\\?\` 前缀）；
3. junction/symlink 环（mklink 测试样本）；
4. 目标文件被占用（运行中的记事本打开的文件）；
5. 只读属性文件 / ACL 拒绝访问；
6. 磁盘写满时移入隔离区；
7. 操作中途 kill 进程（验证 journal 恢复）；
8. 空文件、0 字节目录、超长文件名（255 字节）。

## 7. 净室合规（每次发版前核对）

- [ ] 源码 100% 自写；无 Dism++ `Data.xml`、`CBSHost`、`NCleaner` 等任何成分；
- [ ] 规则 XML 全部自研，清理参数（路径/通配/保留策略）为自行设计；
- [ ] 无竞品品牌词（PureSlate/净板 之外不含"管家/Dism/联想"等词出现在 UI 与包名）；
- [ ] `cargo deny check` 与 `pnpm dlx license-checker` 通过（无 GPL 传染依赖）；
- [ ] README 含「与 Dism++/联想电脑管家无关联」免责声明 + 「唯一官方发布渠道 = 本仓库 GitHub Releases」。
