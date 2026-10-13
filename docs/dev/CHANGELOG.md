# Changelog

> 记录契约与规格变更（AGENTS §8）。变更需双端同步 + 记此日志。

## [Unreleased]

### Phase 0（M0 契约冻结）

- 建立仓库骨架：AGENTS.md 落库至根，SPEC/TASKS/SAFETY 落库至 `docs/dev/`；
- 建立 `src/types/ipc.ts`（TS 契约唯一事实源）与 `src-tauri/src/contract.rs`（serde 镜像）；
- 全部 IPC 命令注册为 stub，`app_meta` 作为首个打通命令；
- **契约冻结**：M0 起，IPC 契约双端视为冻结，任何修改须双端同步 + 记本日志 + 不破坏既有签名（新增字段须 optional）。

### Phase 1（只读扫描链路）

- **DG-1 性能决策门（P1-03）**：新增 `tools/bench-scan`（`src-tauri/src/bin/bench-scan.rs` + `tools/bench-scan.ps1`），按 SPEC §8 口径冷缓存 5GB + `temp+large+dup` 三维度只读遍历计时，输出 `docs/verify/bench-scan.json`。
- DG-1 门禁结果：`totalMs=191978`（temp 9.0s / large 115.8s / dup 67.1s，C: 盘 82.6 万文件）**>120s 未达标** → 触发 **P1-02b（MFT 直读枚举）**；MFT 落地后复测达标后方可判定通过。
- **P1-02b（MFT 直读枚举）**：新增 `src-tauri/src/scanner/mft.rs`，实现管理员提权判定（TokenElevation，advapi32）、USN 直读全盘枚举（`FSCTL_ENUM_USN_DATA`，分块解析 V2/V3 记录，拼装全路径 + size 预分组）、以及 `MftSession`（单会话复用于 temp/large/dup）+ `walk_matching` 统一入口（非管理员/非 NTFS → 自动回退 walkdir）。依赖决策：经 AGENTS §4.7 评审后**不引入 `windows` crate**，所需少量 Win32 API 以 `#[link]` extern "system" 直调 kernel32/advapi32（零依赖、无重量运行时、维持 <20MB 门禁）。`tools/bench-scan` 接入：优先打开系统盘 `MftSession`，三维度复用同一会话。
- **验证**：cargo fmt / clippy `-D warnings` / test（18 passed，含 mft 记录解析/路径拼装/匹配过滤 3 用例）全绿；`pnpm typecheck` 绿。无管理员 → `is_admin()=false` → `try_open` 返回 `NotAdmin`，`walk_matching`/bench 自动回退 walkdir（本会话已实测 engine=walk）。**MFT 性能达标复测需提权终端执行 `tools/bench-scan.ps1`，达标（totalMs≤120s）后方判 DG-1 通过**。

### R02 安全分级与白名单（P1-04）

- **安全分级 `safety/grade`**：新增 `from_risk(Risk)→Grade`（三档直映）与 `from_declaration(Option<Risk>)→Grade`（未声明/非法/来源缺失 → 保守默认 🔴 并记日志，`undetermined_count` 供测试断言）。去向约束仍由 `rules::Category::is_valid_config` 校验。
- **白名单支持 `whitelist.xml`**：`safety/whitelist` 根分两层——常量根（P1-02）+ XML 附加根。新增 `parse_whitelist_xml` / `load_from_dir` / `set_xml_roots`，`is_whitelisted` 对两层根统一段级前缀匹配。初始 `resources/rules/whitelist.xml` 列系统关键根文件（pagefile.sys / hiberfil.sys / swapfile.sys）。
- **SAFETY §1 一致性修正**：`rules::model::Risk` 的 `Default` 由 Green 改为 **Red**；`loader::parse_risk` 对非法/缺失 risk 由默认为 Green 改为保守默认 **Red**（危险方向——任何"无法判定"都应向红靠拢，而非绿）。
- **§2.5（运行中进程句柄判定）推迟**：属独立句柄快照 ffI 逻辑，不与路径级白名单混排；在未做边界验证前不引入脆弱的删除前判定，理由与后续安排见 `safety/whitelist.rs` 头注释。不构成 `[DESTRUCTIVE]`（本任务零删除/零写盘）。
- **验证**：cargo fmt / clippy `-D warnings` / test 全绿（grade 3 用例、whitelist 新增 4 用例、rules loader 默认红 1 用例）。

### 清理执行（P2-04，[DESTRUCTIVE]）

- **两阶段事务 journal**：新增 `cleaner/journal`，`<data_root>\journal\<txId>.jsonl` 只追加；每文件 `intent`（操作前）→ `result`（操作后含 ok/detail）；`detect_orphans` 供崩溃后扫孤。
- **分级清理执行**：新增 `cleaner/execute` 按类目串行逐文件执行（journal.intent → 按去向 apply → journal.result → 审计 record → 进度回调）；🟢 direct=Win32 `DeleteFileW`、🟢 recycle=`SHFileOperationW`（FOF_ALLOWUNDO 可回收站还原）、🟡/🔴 = 移入隔离区（复用 P2-01 store）；单文件失败记 `ok=false` 不中断事务。
- **进程守卫（R23 一语义）**：新增 `guard/process`，`CreateToolhelp32Snapshot` 枚举运行进程与规则 `<guard process>` 比对，命中 → 整类 Skip 不做半清（零依赖，`#[link] kernel32`）。
- **🔴 无 token 拒绝（M0 语义落地）**：`clean_execute` 含 Red 项而 `confirmToken` 缺失/空 → 拒绝；`clean_cancel` 置位一次性取消令牌。
- **契约加性变更（双端同步 + 记此日志）**：新增事件 payload `CleanDoneEvent` 与常量 `CLEAN_DONE`（`clean_done`），`ipc.ts`↔`contract.rs`↔`events.ts` 三端一致；不破坏任何既有命令签名，无字段删除/改写。
- **验证**：cargo fmt / clippy `--all-targets -D warnings` / test 全绿（69 单测 + 2 集成），含 journal 孤儿判定、missing-source 中断不panic、guard 整类阻止、direct 双路径/审计/journal 齐备、🔴 无 token 拒绝等用例。评审文档 `docs/review/P2-04.md`。
### 隔离区生命周期与管理页（P3-06，[DESTRUCTIVE]）

- **契约加性变更（双端同步 + 记此日志）**：新增命令 `quarantine_status`（— → `QuarantineStatus{usedBytes, quotaBytes, overQuota, earliestBatchIds}`），ipc.ts ↔ contract.rs ↔ api/commands.ts 三端一致；既有命令签名零改动（`quarantine_list` 的 AppHandle 为 Tauri 注入参数，IPC 参数不变）。
- **生命周期引擎**：`quarantine/lifecycle` 到期自动清除（硬删+state→purged+审计 auto_purge）、到期前 3 天 `quarantine_expiry_warning` 事件、restored 行 30 天清理（`ManifestEntry` 加性字段 `restoredAt`，serde 兼容旧行）、容量上限 min(5GB, 盘剩余 10%) 超限不自动删（返回最早批次建议 id 供 UI 显式确认）；`quarantine_purge` stub→业务（token 缺失全部拒绝，🔴 语义）。
- **验证**：cargo fmt/clippy --all-targets -D warnings/test 全绿（109 单测+8 集成，lifecycle 新增 6）；pnpm typecheck/vitest(47)/build 全绿。评审文档 docs/review/P3-06.md。

### 兼容加固 R23（P4-02，[DESTRUCTIVE]）

- **契约加性变更（双端同步 + 记此日志）**：`ScanResult` 增 optional 字段 `whitelistOk?: boolean`（whitelist.xml 加载失败时为 false，UI 清理前明示降级；正常序列化省略，旧消费者不受影响），ipc.ts ↔ contract.rs 一致。
- **安全审计五项修复**（docs/verify/security-audit-ipc-toctou.md）：T-1 执行前最终复核（白名单重跑+逐级 reparse+size/mtime 比对）；T-2 restore_one 四重校验（隔离区根内/sha256/白名单禁区/父链 reparse）；T-3 隔离区根 reparse 拒绝；F-1 clean_execute 规则加载失败 fail-closed；F-2 白名单降级审计+UI 明示。
- **验证**：cargo fmt/clippy --all-targets -D warnings/test 全绿（127 单测+20 集成，tests/compat 八项边界 12 例）；pnpm typecheck/vitest(47)/build 全绿。评审文档 docs/review/P4-02.md。

### 崩溃安全 R24（P4-03）
- **契约加性变更（双端同步 + 记此日志）**：新增命令 `crash_list`（— → `CrashDumpInfo[]`）、`crash_preview`（`{fileName}` → `CrashDumpPreview`）、`crash_upload`（`{fileName}` → `CrashUploadReport`，opt-in 门禁）、`crash_recovery`（— → `CrashRecoveryReport`）及配套类型；ipc.ts ↔ contract.rs ↔ api/commands.ts 三端一致；既有命令签名零改动。
- **minidump 本地落盘**（`crash/dump`）：dbghelp `MiniDumpWriteDump` `#[link]` 直调（零依赖，AGENTS §4.7）；SEH 顶级过滤器 + panic hook 双通道；SEH 通道经辅助线程写 dump、异常代码经 sidecar JSON 携带（本机环境异常流路径不可用，见 LESSONS）；dump 保留最近 5 份（启动清理）。
- **启动孤儿 journal 恢复**（`crash/recover`，安全审计 T-6 协议）：journal 内容只用于定位 manifest 记录——quarantine 孤儿经 `restore_one`（T-2 四重校验）还原；「已移入未登记」崩溃窗口按命名约定反查唯一候选补登记（不自动还原）；direct/recycle 不可逆去向只标记 orphan + 审计；恢复期间 `clean_execute` 拒绝新事务（`AppState.recovery_pending` 门闸）。
- **opt-in 上传预览 UI**：Settings 页崩溃数据卡片（optIn 开关 + 本地转储列表 + 预览[模块列表/异常代码] + 手动上传）；Home 页启动恢复横幅；`updates/http` 增 `http_post`（上传通道，仅 R24 opt-in 联网，红线 #4 边界内）。
- **验证**：cargo fmt/clippy --all-targets -D warnings/test 全绿（140 lib + 24 集成，crash_safety 4 例：kill 中断恢复/SEH/panic/dump）；pnpm typecheck/vitest(55)/build 全绿。评审文档 docs/review/P4-03.md。

### 测试体系 R21（P4-04）

- **白名单匹配加固（安全审计 T-4 收口，匹配逻辑变更走评审关卡 docs/review/P4-04.md）**：`safety/whitelist` 段级归一（双侧共用）——`..`/`.` 段文本化解析、逐段剥尾随点/空格；XML 附加根加载时 8.3 短名 `GetLongPathNameW` 展开（长短并存）。**只收紧不放宽**：旧行为下四种声明书写形式会导致保护静默丢失，现全部命中；扫除候选只减不增。
- **黄金文件集扩至 132 样本**（fixture 生成器，原 36 项）：五族 + 三档 target 内守卫（黄/红档误删率分母非平凡）+ T-4 四形态守卫；测试升级为两阶段门禁（扫描期分档误删率 + 清理期真实 execute 盘面核对）。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（167 passed，whitelist +4）；误删率复跑 绿/黄/红 = 0/0/0（docs/verify/golden-misdelete.json）；pnpm typecheck/vitest(55)/build 全绿。

### 签名与发布流水线 R26（P4-06）

- **契约加性变更（双端同步 + 记此日志）**：新增命令 `confirm_token_issue`（— → `string`，`PS-XXXX-XXXX`）：🔴 二次确认令牌后端签发，仅专家模式可签，一次性消费（未命中同样作废）；`clean_execute`/`quarantine_purge` 的 confirm_token 校验从"非空"升级为"消费后端签发令牌"，前端自造值不再被信任。SPEC §5 confirmToken 语义段同步更新为已实现形态。
- **安全审计 I 组收口**（docs/verify/security-audit-ipc-toctou.md 状态列已更新）：I-1 `log_export` 任意路径写入收口（目录成分剥离+文件名白名单+`<data_root>\exports\`+防覆盖）；I-3 严格 CSP 启用（tauri.conf.json，真机走查待人工）；I-4 tauri-plugin-opener 依赖与权限声明移除。
- **发布流水线**：`.github/workflows/release.yml`（tag 触发：双构建 exe 哈希一致性断言 → 体积门禁 → 清单生成 → 可选 SignPath 签名 → Release 上传 → manifest 回推 main）+ `tools/publish-manifest.ps1`（RulesPack 规则包排除 whitelist.xml + update-manifest 六字段）。占位待定稿（人类决策）：SignPath secrets、REPO_SLUG（与 `updates::REPO_SLUG` 常量对齐）、崩溃上传端点。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（169 passed，+2）；pnpm typecheck/vitest(54)/build 全绿；脚本端到端自测（占位安装包）字段/哈希/格式全对。

### v0.1.1（发布缺陷修复 + 便携版）

- **规则资源布局修复（v0.1.0 安装包在非开发机上不可用）**：tauri.conf 资源由 list 形式（落 `$RESOURCE/resources/rules/`，保留结构）改 **map 形式**（`"resources/rules/*.xml": "rules/"`，glob 平铺到 `$RESOURCE/rules/`），与 `rules_dirs` 的 `resource_dir()/rules` 读取匹配；`rules_dirs` 增双布局探测兜底（`rules/` → `resources/rules/` → 开发态源码目录）。v0.1.0 缺陷表现：非开发机上规则加载失败 → 扫描空结果 / 清理 fail-closed 拒绝。
- **便携版（portable zip）**：Release 新增资产 `PureSlate-<ver>-portable.zip`（exe + rules/ 全量含 whitelist.xml + README.txt 使用说明），release.yml 构建后 Compress-Archive 打包；`resource_dir()` 对裸 exe 解析到 exe 目录，便携形态资源解析与安装版一致。
- **验证**：本地 `pnpm tauri build --no-bundle` 确认 `target/release/pureslate.exe` + `rules/`（5 XML 全量）布局正确；cargo/pnpm 全量绿。版本号 0.1.1（tauri.conf + Cargo.toml）。

### v0.1.2（真机反馈：体检体验重构 + 磁盘真实数据）

- **契约加性变更（双端同步 + 记此日志）**：①`AppMeta` 增 optional `isElevated?: boolean`（提权状态——MFT 快速引擎提示依据）；②新增命令 `disk_usage`（— → `DiskUsageInfo[]`：letter/label?/totalBytes/freeBytes，`GetDiskFreeSpaceExW`+`GetVolumeInformationW` 枚举固定盘）。
- **体检分两档（真机反馈修复）**：默认体检曾全开六维度，large（全卷遍历）+dup（哈希）在便携非提权形态 + 2.5TB 盘上十几分钟且 large 无中间进度事件——用户实测"只见临时文件"。现 `scan` store 拆 QUICK（temp/cache/startup/privacy）与 DEEP（large/dup）两组，默认快速；首页新增"深度体检"入口（`setDeepScan` 经 store 传递）；Scan.vue 深度 + 非提权时显示慢速原因提示（`app_meta.isElevated`）。
- **首页磁盘占用接真实数据**：`disk_usage` 命令替代 `DiskUsageStub` 硬编码（stub 恰为开发机盘，用户无从分辨——P1-06 遗留"真数据后接"至此收口）；失败回退 stub。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（170 passed，contract app_meta 用例更新）；pnpm typecheck/vitest(54)/build 全绿。版本 0.1.2（tauri.conf/Cargo.toml/mock）。

### v0.1.3（自动管理员提权）

- **requireAdministrator 清单（release 构建嵌入）**：`build.rs` 按 `PROFILE` 区分——release 嵌入 `requireAdministrator` manifest（保留 Common-Controls v6 依赖），debug/test 保持 asInvoker（tauri dev 与 cargo test 非提权终端不受影响）。启动 UAC 自动提权：MFT 快速引擎双形态恒启用（深度体检大容量盘分钟级）、HKLM 启动项管理恢复可写（P3-02 的 asInvoker 限制解除）。提权不改变数据位置（当前用户 %LOCALAPPDATA%）。本地验证：release exe 含 requireAdministrator/trustInfo，debug exe 不含。
- 版本 0.1.3（tauri.conf/Cargo.toml/mock）。便携 README、主 README 增 UAC 行为说明。

### v0.1.4（外部代码审查修复：3 高危 + 5 中危）

- **H1 白名单 §2.4 与 quarantine 扫描分层（评审关卡 docs/review/P4-08.md，SAFETY §2.4 修订）**：五用户目录（Documents/Desktop/Pictures/Videos/Music）从通用白名单根拆出（`is_user_core_data` + `is_excluded_from_scan(path, disposition)`）——dup 维度 4/5 target 曾被自家白名单整树剪除（静默只剩 Downloads）；quarantine 去向放行（隔离可还原+逐项确认）、direct/recycle 仍整树拦（preflight 同步）、restore 目标检查同步（恢复用户文档合法）。
- **H2** `guard_process` → `guard_processes: Vec<String>`：多 `<guard>` 声明全部生效（原后者覆盖前者，chrome.exe 守卫被静默顶掉——SAFETY §5.1 缺口）；多守卫回归测试。
- **H3** `maxAgeDays`/`minSize` per-include 实现落地（原解析后从未消费）：`match_include` 返回命中约束 + `constraint_allows` 校验（mtime 注入可测，异常一律排除=宁缺勿错）；类目级剪枝下界 max→min；temp.user 7 天/微信缓存 30 天保护窗从此真实生效。
- **M1** journal intent 写失败 → 该目标 fail-closed 跳过（原吞错后照常删除，违反红线 #2）；result 写失败显式告警（恢复协议兜底）。**M2** manifest 状态迁移/行清理改同盘 temp+rename 原子替换（崩溃窗口不再丢全部行）。**M3** rules_url 域白名单（三通道域+REPO_SLUG 绑定，域外拒绝+审计）。**M5** OneDrive 占位文件（RECALL_ON_DATA_ACCESS/OFFLINE）walk/mft/dup 三处跳过（SAFETY §2.7 落地）。**M6** whitelist 两处 expect → into_inner（红线 #6 清零）。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（176 passed，+5 测试）；pnpm typecheck/vitest(54)/build 全绿。评审文档 docs/review/P4-08.md（破坏面：H1 为"扩大扫描面、收窄删除面"变更，direct 候选在 §2.4 仍整树拦）。挂账：M4/M7/低危 L 系列下批。

### v0.1.5 真机反馈批量修复（多盘扫描/报告错误处理/动效/启动项扩展）

- **契约修正（镜像不一致）**：`StartupSource` Rust 端 serde 由 camelCase（"hkcuRun"）改 **snake_case**（"hkcu_run"）——与 ipc.ts/mock 既定事实源对齐（原不一致导致真实模式来源列显示空白，从未被走查发现）。
- **契约加性**：①`StartupSource` 增 `startup_folder_all_users` / `active_setup` 两源；②`StartupEntry.systemDisabled?: boolean`（StartupApproved 系统禁用态，只读展示）；③`ScanResult.volume` 语义扩展为"覆盖卷串"（全部固定盘如 "C:+D:"，单盘机器与旧行为一致；消费面已核：仅前端展示/断言非空，后端不解析）。
- **反馈 1（多盘）**：大文件维度枚举全部固定盘逐卷遍历（原仅首个 target 所在卷）；temp.user 追加非系统固定盘根下 Temp/tmp/_scratch 顶层目录（代码级枚举）。
- **反馈 2（动效）**：Scan 页旋转扫描动效+阶段图标+多卷"第 N/共 M 卷"提示（盘数取 disk_usage，零契约改动）。
- **反馈 3（报告空）**：scan_get_items 拉取失败 → itemsError 错误分支+重试（闭环外部审查 L13）；事件过滤器竞态自愈（scanId 空时采纳首事件）；完成态显式出口（取消后"查看已收集结果"）；空态文案细分。
- **反馈 4（启动项）**：新增全体用户启动文件夹（ProgramData）+ Active Setup（HKLM/HKCU Installed Components StubPath，HKLM 优先）两源，枚举+禁用（.reg/文件备份）+还原同批交付；StartupApproved 只读判定系统禁用态（不写系统批准状态）。
- **验证**：cargo fmt/clippy --all-targets -D warnings/test 全绿（154 lib+24 集成，startup 新增 Active Setup/全体用户文件夹 roundtrip、approved 字节语义等 4 例）；pnpm typecheck/vitest(56)/build 全绿。

### R27-1 安全卸载辅助：只读枚举（SPEC §6.7，契约加性）

- **契约加性（双端同步 + 记此日志）**：新增 `InstalledApp`（id/name/publisher?/version?/installDate?/installLocation?/uninstallCommand/isMsi/arch/scope/estimatedSizeBytes?）与命令 `uninstall_list() → InstalledApp[]`；不改既有签名。
- **枚举**：HKLM/HKCU × 64/32 位四视图 `...\Uninstall`（显式 KEY_WOW64_64KEY/32KEY）；过滤 SystemComponent≠0 / ParentKeyName 非空；name+publisher+version 跨视图去重（HKLM64 > HKLM32 > HKCU64 > HKCU32）；MSI（WindowsInstaller=1 或 {GUID} 键名）构造 `msiexec /X{GUID}`；EstimatedSize(KB)→字节；winreg 增 `enum_*_view` 与公开 WOW64 常量（既有调用零改动）。
- **前端**：/apps 只读列表页（搜索、本机/用户、x64/x86、MSI 徽章、大小/日期格式化），卸载按钮置灰（执行编排为 R27-2）；mock 三形态样本；appsModel 纯函数 6 例。
- **边界**：全程零写入；UWP 不枚举；SPEC §9 Non-goals 同步声明（不默认强杀/不做 UWP/不替代系统"应用和功能"）。
- **验证**：cargo fmt/clippy --all-targets -D warnings/test 全绿（163 单测+24 集成，uninstall 新增 9 例含真机只读冒烟）；pnpm typecheck/vitest(62)/build 全绿。

### R27-2 安全卸载辅助：执行编排（SPEC §6.7，[DESTRUCTIVE]，契约加性）

- **契约加性（双端同步 + 记此日志）**：新增命令 `uninstall_run(appId) → txId:string`（后台 spawn_blocking 编排，立即返回事务 id；单实例互斥，已有事务进行中则 Err）、`uninstall_cancel(txId) → bool`（只取消等待，不强杀）；新增事件 `uninstall_progress`（`UninstallProgressEvent{txId,message}`）与 `uninstall_done`（`UninstallDoneEvent{txId,appId,name,result,exitCode?,keyStillPresent,installLocationStillPresent?,rebootPending,detail}`，result ∈ removed/reboot_pending/still_present/cancelled/timeout/launch_failed）。ipc.ts ↔ contract.rs ↔ commands.rs/events.ts 四端一致；既有命令签名零改动。
- **内核**：`uninstall/runner.rs` 编排官方卸载器——命令行纯函数拆分（环境变量展开/引号/.exe 边界，拒绝空白与未闭合引号）+ `CreateProcessW`（SW_SHOWNORMAL，8.3 短路径 GetLongPathNameW 还原）+ ERROR_ELEVATION_REQUIRED(740) 回退 `ShellExecuteExW "runas"` 标准 UAC；Toolhelp 进程树等待（BFS 深度上限 16 + PID 去重集合跨轮持久化防收养丢失 + PPID 环防死循环，250ms 轮询响应取消）；**不读单一退出码**——事后重扫 Uninstall 键 + InstallLocation 存在性 + `PendingFileRenameOperations`（REG_MULTI_SZ 解析，`\??\`/`\\?\`/斜杠/尾分隔符/大小写归一后的目录边界前缀匹配）三层判据；审计 op=uninstall（start/removed/reboot_pending/still_present/timeout/cancelled，含退出码警告）。
- **R27-1 遗留修正（加性备注）**：`parse_app` 曾误取 `QuietUninstallString`（代码与注释/SPEC 矛盾），会静默拉起**静默卸载器**——改为只取交互 `UninstallString`，仅 Quiet 串的非 MSI 条目命令留空（UI 禁用），回归单测固化。
- **前端**：/apps 三态执行流（确认面板明示三条边界 → 调起/等待进度（停止等待不强杀）→ 六结果语气结果态）；卸载按钮按 `canUninstall` 启用；mock 有状态（执行后 stub 移除）+ 阶段事件模拟；appsModel 新增 canUninstall/uninstallResultMeta 纯函数 2 组单测。
- **边界**：本任务零自删（无 DeleteFile/MoveFile/注册表写）；破坏主体是第三方卸载器，UI 如实告知其删除不进 journal/隔离区；不申请 SeDebugPrivilege、不默认强杀、不碰 UWP；残留隔离清理属 R27-3。评审文档 `docs/review/R27-2.md`。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（178 lib+24 集成；uninstall 模块 24 例：命令行拆分 7 例/进程树闭包 3 例/待重启匹配 2 例/分类全矩阵/枚举 11 例含 Quiet 回归与真机冒烟）；pnpm typecheck/vitest(64)/build 全绿。

### R27-3 安全卸载辅助：卸载残留隔离清理（SPEC §6.7，[DESTRUCTIVE]，契约加性）

- **契约加性（双端同步 + 记此日志）**：新增命令 `uninstall_residue_scan() → ResidueCandidate[]`（无参数；消费后端最近卸载上下文）与 `uninstall_residue_clean(ResidueCleanParams) → txId:string`（复用 clean_progress/clean_done/clean_cancel）；新增类型 `ResidueCandidate{id,kind(install_dir/start_menu/app_data),appName,rootPath,reason,sizeBytes,fileCount,items:ScanItem[]}`、`ResidueSelectionParams{candidateId,disposition}`、`ResidueCleanParams{selections,confirmToken?}`；`UninstallDoneEvent` 加性补 `installLocation?`（不破坏既有字段）。ipc.ts↔contract.rs↔api/mocks 同步。
- **残留枚举（只读，`uninstall/residue.rs`）**：仅在 `state.last_uninstall`（R27-2 removed/reboot_pending 写入）上下文内扫描三源——①注册表 InstallLocation 安装目录（外部值走严格安全闸）；②开始菜单 Programs×2 的产品程序组/厂商\产品两层/散落 .lnk（深度 2，组内必须确有产品名快捷方式才收，绝不整收厂商组）；③AppData Local/Roaming/LocalLow 精确产品目录（厂商目录只收名下匹配产品的子目录）。名称归一纯函数（去版本/架构括号、.lnk/.exe 后缀、法人后缀）+ **只精确匹配不做模糊包含**；通用产品名/共享目录段双拒绝名单；reparse 子树整组放弃；单组 2 万文件/10GiB 上限防爆。
- **去向用户可选**：每组二选一，隔离区（默认 🟡/quarantine，14 天可还原）/ 直接删除（🔴 direct 永久，含任一项须专家模式一次性令牌）；不提供回收站档；同批可混用。清理复用 cleaner::execute 全套（journal 两阶段/审计 op=clean category=residue.uninstall/preflight TOCTOU/隔离区同盘 rename/取消/失败不中断）；文件移走后 `RemoveDirectoryW` 自底向上只删**空**目录（含空目录候选；非空必然失败不递归）。
- **白名单分层（SAFETY §2.2 R27-3 条款，架构性变更）**：常量根拆**硬根**（SystemRoot/自身设施 + 既有引导卷/卷关键文件/隔离哨兵/XML 声明根，永不豁免）与**程序目录软根**（ProgramFiles*/ProgramData\Microsoft，仅可被后端快照签发的精确 authorized_root 在 preflight 豁免）；`is_whitelisted` 对外行为不变，新增 `is_hard_protected`/`is_soft_program_root`/`is_within` 与 pub(crate) `normalize_segments`；CleanTarget 加性字段 `authorized_root:Option<PathBuf>`（普通清理恒 None）；restore 仅放行 residue.uninstall manifest 还原回软根（硬根/sha256/root 内/reparse/冲突不覆盖全部保留）。
- **状态与信任链**：AppState 新增 `last_uninstall`/`residue_snapshot`；clean 候选 id 必须出自快照（前端无法伪造路径/扩大授权根），快照一次性消费、重扫/新卸载即失效；守卫跳过的组快照放回并返回错误。
- **前端**：Apps 页结果态新增"扫描卸载残留"→ 残留四态（扫描中/清单逐项勾选+理由+根路径+每组去向单选/进度/结果）；direct 选择复用 ConfirmTokenModal（专家令牌）；residueModel 纯函数 7 例；mock 三源有状态样本。
- **边界**：不做无上下文全局孤儿扫描；不碰 UWP/驱动/服务；本批不新增任何独立删除路径（直删走既有 DeleteFileW 通道且受 preflight+token 双闸）；测试钩子 `quarantine::set_quarantine_root_override`（仅测试构建注入，生产无调用）。评审 `docs/review/R27-3.md`。
- **验证**：cargo fmt/clippy --all-targets -D warnings 全绿；uninstall 模块 40 例全绿（residue.rs 16 例：名称归一/发布者/精确匹配/双闸安全/三源枚举/junction 放弃/快照信任/空目录收尾；preflight 新增授权分层 1 例；其余为 R27-1/2 既有 24 例，全库 195 lib）+27 集成（新增 residue_pipeline 3 例：quarantine 移入→manifest→空目录收尾→还原重建目录链 roundtrip、direct 永久删除+审计、空目录候选）；pnpm typecheck/vitest(71)/build 全绿。
