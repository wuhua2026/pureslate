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