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
