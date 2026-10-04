# PureSlate · IPC 越权与 TOCTOU 安全审计清单

> 日期：2026-09-30 ｜ 状态：审计完成，待处置 ｜ 用途：供 P4 任务（P4-02/P4-03/P4-05）与发布前加固引用
>
> 方法：以 cloudflare/security-audit-skill（MIT）的 DESKTOP-MOBILE-AND-LOCAL-IPC 攻击类别为对照框架，
> 覆盖"过宽 native bridge / IPC 生命周期混淆 / 特权 helper 混淆代理 / 本地文件 TOCTOU / 安装-更新-修复路径信任"
> 五组类别，对两端契约、命令层、清理执行、隔离区与还原、journal、白名单、Tauri 配置做静态审计。
> 本清单遵循该框架的取证纪律：只记录有具体主体、资源与后果的项；"加固"项明确区别于"漏洞"；无可达证据的项标注"待验证"。

## 0. 威胁模型与评级前提

| 模型 | 攻击者起点能力 | 当前可达性 | 相关条目 |
|------|--------------|-----------|---------|
| A · webview 注入 → IPC | 任意脚本可调用全部 16 个 IPC 命令 | 全库无 `v-html`/`dangerouslySetInnerHTML`，当前无可达注入点 → 相关项按**纵深加固**对待 | I-1..I-4 |
| B · 本地进程（标准权限） | 可写用户级文件、可创建 junction（无需特权）、可篡改隔离区/manifest/journal（数据盘） | **当应用以管理员运行时成立**（SPEC DG-1：MFT 直连需提权；UAC 边界真实存在） | T-1/T-2/T-3 |
| C · 并发实例 | 双击启动第二实例 | 单实例互斥量未实现（已计划 P4-02） | I-5 |

**评级口径**：`高 / 中 / 低 / 加固 / 待验证`。"待验证"不给级别（沿用该框架 needs_validation 纪律）。
**重要限定**：T-1/T-2 的"高"以**应用以管理员运行**为前提；若最终形态定为 asInvoker（MFT 仅可选加速），二者降为"中"（同权限自伤 + 产品安全承诺破坏）。

## 1. 总览

| ID | 缺口 | 级别 | 状态 | 建议归属 |
|----|------|------|------|---------|
| I-1 | `log_export` 任意路径写入 + truncate | 中 | 新增 | P4 前小修 |
| I-2 | `confirm_token` 仅查非空，🔴 红线在 IPC 层无强制 | 中 | 新增 | P4 前小修 |
| I-3 | CSP 未启用 + 命令无输出/来源分层 | 加固 | 新增 | P4-06 前 |
| I-4 | `opener:default` 预置（插件未注册，含"打开任意 URL"能力） | 低 | 新增 | 注册插件时 |
| I-5 | 单实例缺失 → manifest/journal 并发竞态 | 中 | 已计划 | P4-02 |
| T-1 | 执行时无最终路径复核（白名单/reparse/规范化） | 高（提权） | 新增 | P4-02 |
| T-2 | 还原信任可写 manifest（无 hash 校验、路径无约束） | 高（提权） | 新增 | P4-02（须先于 P4-05） |
| T-3 | 隔离区根 `.pureslate-quarantine` 无 reparse 校验 | 中（提权） | 新增 | P4-02 |
| T-4 | 白名单字符串匹配的旁路形式（短名/尾随点/`..`） | 待验证 | 新增 | 黄金集补样本 |
| T-5 | manifest `update_entry_state` 非原子读-改-写 | 低 | 新增 | P3-06 顺带 |
| T-6 | journal 恢复执行（P4-03）的路径信任未定协议 | 设计前置 | 已计划 | P4-03 备注 |
| F-1 | 规则加载失败 → guard 守卫 fail-open | 中 | 新增 | P4-02 |
| F-2 | `whitelist.xml` 加载失败被静默忽略（保护降级） | 中 | 新增 | P4-02 |

## 2. IPC 越权面

### I-1【中】`log_export` 是"任意路径写入 + 覆盖"原语

- **证据**：`src-tauri/src/ipc/commands.rs:441-445` 直接接受前端 `path: String`；`src-tauri/src/logging/audit.rs:102-107` 用 `File::create`（truncate 语义）写入。
- **后果**：webview 注入者（或未来任何 bridge 滥用）可用一条 `invoke("log_export", {path:"C:\\...\\任意文件"})` 截断任意用户可写文件。即便无攻击者，当前前端传**相对路径**（写进程 CWD，见 P2-06 备注）本身是缺陷。
- **修复**：只允许导出到 `data_root` 或用户选择目录 + 唯一文件名；用 `create_new` 拒绝覆盖；或改用保存对话框返回路径并在 Rust 侧校验。
- **验证**：单测——传入已存在文件路径应失败；传入系统目录应失败。

### I-2【中】🔴 二次确认 token 在 IPC 层无强制

- **证据**：`src-tauri/src/ipc/commands.rs:291-295` 只判断 `confirm_token` 非空（任意字符串如 `"x"` 可通过）；`settings_set`（同文件 457-468）可随意置 `expert_mode:true`。
- **后果**：webview 注入 → 开专家模式 → 带假 token 清理 🔴 项，绕过红线 #8"默认灰禁 + 二次确认 token"承诺。
- **修复**：token 由后端签发（绑定 tx/session、一次性、校验 `expert_mode`）；IPC 层不信任前端自造值。
- **验证**：单测——假 token 应被拒绝；未开专家模式应被拒绝。

### I-3【加固】CSP 缺失 + 命令面未分层

- **证据**：`src-tauri/tauri.conf.json:20-22` `"csp": null`；单窗口 main 匹配 `core:default` 后即持有全部 16 个命令（Tauri 2 应用命令默认不列权限清单，精细化需 AppManifest 显式声明）。
- **后果**：当前无注入点，属纵深问题；但与 I-1/I-2 叠加时是"爆炸半径"倍增器。
- **修复**：启用严格 CSP（Tauri 自带 nonce 机制）；给 `clean_execute`/`quarantine_restore`/`log_export` 生成命令权限并在 capability 显式引用；未来多窗口按窗口分组（官方文档：不匹配 capability 的 webview 无 IPC 访问）。
- **验证**：CSP 生效后真机走查全部页面功能无回归。

### I-4【低】`opener:default` 预置了未使用的权限

- **证据**：`src-tauri/capabilities/default.json` 声明 `opener:default`（= `allow-open-url` + `allow-default-urls`(mailto/tel/http/https) + `allow-reveal-item-in-dir`，经 `gen/schemas/acl-manifests.json` 核实）；`src-tauri/src/lib.rs:19-44` 未注册该插件（当前无实际效果）。
- **后果**：未来注册即带来"打开任意 http(s) URL"能力（钓鱼面）。
- **修复**：注册时用自定义 scope 限域（仅官方发布页）；当前不需要则移除声明。

### I-5【中·已计划】单实例缺失

- **证据**：`src-tauri/src/guard/mod.rs:5` 注明"延至 P4-02"。
- **补充结论**：这不只是 UX——双实例并发写同一 `manifest.jsonl`/journal（读-改-写无锁，见 T-5）会造成条目丢失与恢复语义混乱。
- **修复要求**：P4-02 落地时加"数据目录独占锁"（文件锁），而非仅互斥量唤起窗口。

## 3. TOCTOU / 文件操作面

### T-1【高·提权】执行阶段无最终路径复核

- **证据**：`src-tauri/src/cleaner/execute.rs:203-229`（`apply_one` 仅 `exists()` 后即删除/移动）；白名单只在扫描期过滤（walk.rs / mft.rs / dup.rs 三处），`cleaner/execute.rs` 无任何复核。扫描→人工复核→执行窗口可任意长（会话内 items 常驻内存）。
- **攻击链**：攻击者（标准用户）把目标文件的**中间目录**替换为 junction（无需特权）→ 删除/移动跟随到 junction 目标。提权进程 + 用户可写目录（如 %TEMP%）→ 标准用户诱导管理员进程删/移受保护区文件（越过 UAC 边界）。注：`DeleteFileW` 对最终组件不跟随 symlink，但中间目录会被解析。
- **修复（最小有效）**：执行前在最终路径上——①重跑 `is_whitelisted`；②逐级父目录检查 `FILE_ATTRIBUTE_REPARSE_POINT`（`FILE_FLAG_OPEN_REPARSE_POINT` 打开校验）或 canonicalize 与扫描时记录对比；③比对 size/mtime 与扫描快照（对象未变证明）。
- **验证**：SAFETY §6.3 边界 3"junction 环"扩样本——"扫描后替换中间目录为 junction"用例（需提权真机或 VM）。

### T-2【高·提权】还原引擎信任可写 manifest（confused deputy）

- **证据**：`src-tauri/src/quarantine/restore.rs:35-84`——`quarantine_path` 仅 `is_file()`；`original_path` 无任何约束；manifest 自带 `sha256` 字段却**从不校验**；还会 `create_dir_all(parent)`。
- **攻击链**：篡改 manifest 一条记录（隔离区在数据盘时通常用户可写；管理员首建后 ACL 继承可能不严）→ 让（提权的）应用把任意文件"还原"到任意路径 → **把攻击者可控文件写入特权目录**（配合 DLL 加载顺序可升级为本地提权代码执行）。
- **修复**：还原前校验 `sha256`；`quarantine_path` 必须在对应隔离区根内；`original_path` 走规范化 + 不得落在白名单禁区；`create_dir_all` 限于原路径父链。
- **验证**：伪造 manifest 用例（hash 不符应拒绝、越界路径应拒绝）；P4-05 千次还原脚本加同类断言。

### T-3【中·提权】隔离区根无 reparse 校验

- **证据**：`src-tauri/src/quarantine/mod.rs:100-106` `ensure_quarantine_root` 用 `exists` + `create_dir_all`。
- **后果**：数据盘上被预置同名 junction 时，移入文件写到 junction 目标。
- **修复**：创建/打开时校验 reparse 属性，命中即报错拒绝。

### T-4【待验证】白名单是字符串匹配，旁路形式未覆盖

- **证据**：`src-tauri/src/safety/whitelist.rs:157-197`；`normalize_lower`（219-223）只做分隔符/小写/尾斜杠，不处理 8.3 短名（`PROGRA~1`）、尾随点/空格、`.`/`..` 段、卷别名（`\\?\`、Volume GUID）。
- **背景**：P2-07 修复的"正斜杠绕过"是同族第 2 例，本项为第 3 类形态。可达性取决于路径来源（walk 产物一般为长名）→ 先验证再定级。
- **修复方向**：删除/移动前 `GetLongPathNameW` 规范化 + 拒绝 `..`；长路径支持用 `\\?\`（同时规避系统二次求值）并自行完成规范化。
- **验证方法**：黄金文件集加一组样本（短名/尾随点/点段路径声明）跑守卫，确认现行为后定级。

### T-5【低】manifest 状态迁移非原子

- **证据**：`src-tauri/src/quarantine/manifest.rs:102-123` 读-改-写 + `File::create` 截断重写。
- **后果**：崩溃窗口可损坏清单（对比：journal/日志严格只追加是正确示范）。
- **修复**：写临时文件 + 原子 rename；或追加事件行按 id 归并。

### T-6【设计前置·已计划】journal 孤儿恢复（P4-03）的路径信任

- **证据**：`src-tauri/src/cleaner/journal.rs:111` 目前只检测不执行；P4-03 将实现"启动孤儿 journal 恢复"。journal 位于用户可写目录、路径字段来自文件内容。
- **要求（写入 P4-03 备注）**：恢复动作仅限本应用语义（如把已入隔离区的文件还原），不得对 journal 声称的任意 path 做任意操作；路径逐条校验；绝不因 journal 内容触碰白名单区。趁未实现先把协议定死。

## 4. 保护降级链（fail-open）

### F-1【中】规则加载失败 → 进程守卫被跳过

- **证据**：`src-tauri/src/ipc/commands.rs:262-270` 明确"guard 守卫退化为一律不查"。
- **后果**：半清缓存比不清更危险（SAFETY §5 原话）——规则不可用时应当**拒绝执行**而非降级执行。
- **修复**：guard 元数据不可得 → 该类目整体阻止（fail-closed）。

### F-2【中】`whitelist.xml` 加载失败被静默忽略

- **证据**：`src-tauri/src/ipc/commands.rs:145` `let _ = load_from_dir(...)`。
- **后果**：解析失败时附加白名单根（如 `C:\pagefile.sys` 保护）静默丢失，保护面收窄。
- **修复**：失败至少记审计日志 + 清理前 UI 明示降级；扫描入口错误不可吞。

## 5. 已验证良好（应保持）

- 扫描侧 `follow_links(false)` + `is_symlink` 过滤（`scanner/walk.rs:58`）、MFT 路径白名单过滤；
- `move_into_quarantine` 用 `symlink_metadata` 拒绝非普通文件源（`quarantine/store.rs:54-59`）；
- `clean_execute` 目标只能来自扫描会话（item id → 内存解析），webview **不能**传任意路径删除；
- journal"先 intent 后动手"、打开失败即整体中止（fail-closed，红线 #2 正面样本）；
- 跨盘 copy → fsync → hash 校验 → 删源 顺序正确（防传输错误；注意不防替换竞争）；
- 还原冲突兜底 `restore-conflict`，不覆盖已存在文件。

## 6. 建议处置顺序

1. **T-2 + T-3**（还原/隔离区路径信任）——成本低、提权场景收益最大；T-2 应在 P4-05 千次还原前修复；
2. **T-1**（执行前最终复核）——与 P4-02 边界任务合并实现，新增"扫描后替换 junction"用例；
3. **F-1 / F-2**（fail-open 改 fail-closed）——小改动；
4. **I-1 / I-2**（导出路径约束、token 后端签发）——接口小改动；
5. **T-6** 设计前置写进 P4-03 备注；**T-4** 先补验证样本再定级；**I-3 / I-4** 归入发布前加固。

## 附：对照框架映射（cloudflare/security-audit-skill 类别 → 本清单）

| skill 攻击类别 | 命中条目 |
|---------------|---------|
| Over-broad native bridge capabilities | I-1、I-2、I-4 |
| IPC lifecycle and correlation confusion | I-5 |
| Privileged helper as confused deputy | T-1、T-2、T-3 |
| Local file ownership and TOCTOU | T-1、T-4、T-5 |
| Install/update/repair path trust | T-2、T-6、F-1、F-2 |
| Navigation-origin to bridge confusion | I-3（当前无远程内容，防回归） |

依据补充：Tauri 官方 ACL/Capability 文档（应用命令默认不列权限清单、不匹配 capability 的 webview 无 IPC 访问）。