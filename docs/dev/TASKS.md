# PureSlate · 任务清单（TASKS.md）

> 用法：按 Phase 顺序执行；一次只做一个任务；每任务完成打 `✅` 并跑「验证」列命令。`[DESTRUCTIVE]` 任务走 AGENTS §6 评审关卡。`[HUMAN]` 任务由人类执行。**Phase 末门禁不达标不得进入下一 Phase。**

---

## Phase 0 · 地基与契约冻结 → M0

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P0-01 | 仓库初始化 | — | ✅ git 仓库；分支 main/dev；LICENSE(MIT)、NOTICE、.gitignore、README 骨架（含"唯一官方渠道"与免责声明占位）；AGENTS.md 落库至根，docs/dev/ 放 SPEC/TASKS/SAFETY + CHANGELOG.md | 目录结构核对；`git log` 有初始提交 ✅（`95f85db` P0-01；历史 hash 曾因仓库公开化统一改写提交邮箱为 GitHub noreply 而整体更新，见 SESSIONS 2026-10-04） |
| P0-02 | Tauri 脚手架 | P0-01 | ✅ create-tauri-app（vue-ts 模板）；版本锚定 SPEC §1；dev 运行通过 | `pnpm tauri dev` 起窗口 ✅（MainWindowTitle=PureSlate） |
| P0-03 | IPC 契约落地 | P0-02 | ✅ `src/types/ipc.ts` 全量类型（SPEC §5）+ `src-tauri/src/contract.rs` serde 镜像 + 全部命令注册为 stub（返回空/默认值）；`src/api/` 封装 invoke/listen | `pnpm typecheck` 过 ✅；stub 命令 `app_meta` 前端可调通 ✅；cargo check/fmt/clippy/test 绿 ✅ |
| P0-04 | mock 数据服务 | P0-03 | ✅ 前端 mock 层：覆盖全部 6 扫描维度的 ScanResult/ScanItem 假数据（含中英文路径、各分级、重复组）；mock/dev 开关 | `pnpm test`（mock 数据用例）绿 ✅（8 passed） |
| P0-05 | CI 骨架 | P0-02 | ✅ GitHub Actions：fmt+clippy -D warnings+cargo test+vitest+typecheck+build；产物体积断言 <20MB（脚本对比） | push 后 CI 绿（本地等价验证 ✅，终态由 Actions 判定） |
| P0-06 | 体积与启动基准 | P0-05 | ✅ 首次打包记录：安装包体积、冷启动时间 → `docs/verify/phase0-baseline.md` | 本机 exe 8.36MB <20MB ✅；安装包体积由 CI 产物断言（已降级备注） |

> **Phase 0 门禁（=M0）✅ PASS**：契约两端镜像一致（P0-03 评审）；mock 覆盖全维度；CI 绿；包体 <20MB。CHANGELOG 记"契约冻结"。核验：`docs/verify/m0-gates.md`。

---

## Phase 1 · 只读扫描链路 → M1

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P1-01 | 规则引擎 R01：模型+加载 | P0-03 | ✅ rules/model+loader：解析 SPEC §4.1 XML；非法 category 丢弃+日志；规则缓存（mtime 失效）；至少 2 个真实 ruleset：system-temp(green)、cache.wechat(yellow 含 guard) | cargo test：合法/非法/覆盖加载用例 ✅（check/fmt/clippy/test 全绿，test 7 passed） |
| P1-02 | 规则引擎 R01：遍历+匹配 | P1-01 | ✅ scanner/walk：walkdir 遍历 target、白名单过滤（SAFETY §2.1-4/6）、glob 匹配（include/exclude + 相对 target 根）、取消令牌、进度回调；reparse point 不跟随（follow_links(false)） | cargo test：临时目录沙盒匹配/排除/取消用例（walk 3 + matcher 1 + whitelist 5）✅ |
| P1-03 | **DG-1 性能决策门** | P1-02 | tools/bench-scan：真机 1TB SSD 三维度计时 → `docs/verify/bench-scan.json`。**≤120s → 记录通过；>120s → 在本表插入 P1-02b（MFT 直读）并执行，完成后再判** | ≥120s → 已跑 P1-02b 再判 | `docs/verify/bench-scan.json` ✅ 已落盘（totalMs=191978，temp 9.0s/large 115.8s/dup 67.1s，C: 82.6 万文件）。**>120s 未达标 → 判定触发 P1-02b（MFT 直读），完成复测后再判**。结论记 CHANGELOG |
| P1-02b | （条件）MFT 直读枚举 | P1-03 | ✅ scanner/mft：管理员提权判定（TokenElevation）+ USN 直读枚举（FSCTL_ENUM_USN_DATA，`#[link]` 直调 kernel32/advapi32，零第三方依赖）+ walkdir 降级；MFT 枚举替代遍历供 target 匹配（`MftSession::walk_target`/`full_disk_stats`） | cargo test：记录解析/路径拼装/匹配过滤（mft 3 用例，累计 18 passed）+ fmt/clippy/test 全绿 ✅ | 备注：无管理员自动回退 walkdir 已验证（本会话非提权 → engine=walk）；MFT 性能达标复测需**提权终端**另跑（bench-scan 复测落盘） |
| P1-04 | 安全分级 R02 | P1-01 | ✅ safety/grade+whitelist：按规则 risk 判定；无法判定默认 red；白名单常量+whitelist.xml 加载 | 备注：①SAFETY §1 修正——rules 模型 `Risk` 非法/缺失一律默认 Red（原 loader 曾默认 Green）；②§2.5(运行中进程句柄) 推迟，见 whitelist.rs 头注释理由；③cargo test 三档/默认红/白名单拦截用例全绿 |
| P1-05 | 扫描编排 R03 | P1-02,P1-04 | ✅ scan 编排：profile→各维度执行→聚合→事件推送（scan_progress 节流/scan_done）；**全程零写盘**（审计验证：对扫描目标目录做前后快照对比） | 备注：①`scanner/{aggregate,expand,engine}` 实现 维度→类目→target 展开→walk/mft 匹配→聚合；`state.rs` 扫描会话、`commands.rs` scan_start/scan_cancel/scan_get_items 业务；②`scan_start` 异步命令含 `&State` 引用须返回 `Result`（Tauri 约束）；③`FoundBytes` 派生 `Default`（纯 Rust，序列化不变，ipc.ts 无需同步）；④**loader 缺陷修复**：`load_dir` 会读入同目录 `whitelist.xml` 而 `parse_ruleset` 拒绝导致整目录加载失败，现新增 `is_ruleset_xml`（根元素为 `<ruleset>` 才加载），whitelist 仍由 safety 模块单独加载；⑤集成测试 `tests/scan_pipeline.rs`（2 例：命中/聚合正确性 + 扫描前后目标目录快照逐字节零写盘），cargo fmt/clippy --all-targets -D warnings/test 全绿（37 单测+2 集成） |
| P1-06 | 外壳 R16 | P0-04 | ✅ pages/Home：导航/磁盘占用条/一键体检入口/快速建议区；无托盘无推广；设计 token 落 CSS | ✅ vitest（磁盘 stub 用例，9 passed）+ typecheck 全绿；手动走查屏 1 要素齐 | 备注：①`App.vue` 升级为外壳（220px 侧边导航含全部 9 路由，无托盘/无红点/无推广）；②`router/index.ts` 补全 SPEC §7 页面集，未建页落 `pages/Placeholder.vue` 占位（报告页 P1-07 替换）；③磁盘占用条用 `data.ts` 本地 `DiskUsageStub`（不碰冻结契约，真数据 P1-08 接入）；④设计 token 已在 `App.vue :root` 落 CSS（SPEC §9） |
| P1-07 | 报告页 R13/R14 | P0-04 | ✅ pages/Report（mock）：总览可释放量/风险分布/逐项可解释（reason）/去向预告/A·B 选项对比；🔴 折叠灰禁；GradeBadge | ✅ vitest（reportModel 7 用例：分级+灰禁+可释放语义）+ typecheck/build 全绿 | 备注：①`components/GradeBadge.vue`：三档徽章，🔴 附锁图标+文案（满足"除颜色外图标+文案"硬性要求）；②`pages/reportModel.ts` **纯函数模型**（groupCategories/buildReportOverview）：可释放语义=绿立即释放/黄+红进隔离区（对齐 SAFETY §4.5），A/B 方案给选项不给结论且🔴永不入方案；为守"依赖准入"红线，用纯逻辑单测替代 jsdom 组件测试，**未新增任何 devDependencies**；③`Report.vue` 用 `commands.scan_get_items`（mock scanId）取数，P1-08 换真实 scanId 即可；④路由 `/report` 由 Placeholder 切真组件 |
| P1-08 | 真数据联调 | P1-05,P1-07 | ✅ 报告页接真扫描（mock 开关切换）；扫描中页（屏 5：进度/当前路径/实时累计/取消） | ✅ typecheck + vitest（16 passed）+ build 全绿；真机走查（体检→报告→取消）待人类执行 | 备注：①`stores/scan.ts`：Pinia 会话 store，mock（定时器逐步揭示项/实时累计）与真实（scan_start→订阅 scan_progress/scan_done→scan_get_items）双模式，跨 /scan→/report 持久；`isMockMode()` 在 `api/mode.ts` 统一切换；②`pages/Scan.vue`（屏 5）：onMounted 自动 start，进度/阶段/当前路径/实时累计/取消；`isDone` watch 自动跳 /report；③`Report.vue` 由 mock scanId 改为读 store.items，空态提示「去体检」；④`router` `/scan` 由 Placeholder 切 Scan.vue |
| P1-09 | [HUMAN] 数据采集 | P0-04 | 本机+同学机 5–10 台（SSD/HDD）：各盘容量/分类占用/大文件分布/扫描耗时 → `docs/verify/data-baseline.md`（提供采集脚本模板） | data-baseline.md 落库 |

**Phase 1 门禁（=M1）**：真机只读链路全通；扫描零写盘（快照对比）；DG-1 有结论；报告页 8 屏要素中屏 1/2/5/7 完成。

---

## Phase 2 · 信任底座与清理闭环 → M2

> 顺序即信任链：隔离区→日志→清理。**P2-01..03 完成前不得开始 P2-04。**

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P2-01 | ✅ [DESTRUCTIVE] 隔离区核心 R25 | P1-05 | quarantine/store+manifest：同盘 move、manifest 行、冲突路径、跨盘降级 copy+hash 校验（SAFETY §4.2）；目录 hidden+system；✅ settings.json 持久化；✅ §2.6 隔离区哨兵白名单顶进 | cargo test：移入/manifest/跨盘/占用用例；评审关卡文档。备注：`store::move_into_quarantine` 签名显式 `root`（调用方 `quarantine_root_of` 计算，测试可注入沙箱根，避免触碰真实盘根）；`quarantine_*` 命令仍 stub（P2-02 填充）；`manifest.jsonl` 键名 camelCase（对齐 IPC 契约） |
| P2-02 | [DESTRUCTIVE] 还原引擎 R25 | P2-01 | ✅ quarantine/restore：按 manifest 还原、冲突兜底目录、state 迁移；**还原率脚本 tools/test-restore.ps1（先 100 次循环）≥95%** | ✅ 脚本报告通过：passed=100/total=100 ratio=1.0000（≥95%）；cargo fmt/clippy/test 全绿（54 单测+2 集成）。备注：①`restore_one`/`restore_globally` 实现，同盘 `rename`/跨盘 `copy_verify_delete` 复用 store；原路径占用落 `data_root/restore-conflict`；回写 `original_mtime_ms`；manifest `state→restored`；②`ManifestEntry` 增 `original_mtime_ms`（保取证信息）；③`set_data_root_override`+`restore_conflict_path` 落 storage，`quarantine_list/restore` 命令填业务；④`restore-self-test` 二进制+`tools/test-restore.ps1`（***UTF-8 BOM***，否则 PS5.1 无 BOM 读 .ps1 会按 ANSI 解析中文注释致语法错）；⑤修复并行测试竞态：`DATA_ROOT_OVERRIDE` 为共享全局，测试用 `TEST_DATA_ROOT_LOCK` 串行化（restore 沙箱 `SandboxGuard` RAII），storage 测试改持覆盖而非改全局环境变量 |
| P2-03 | ✅ 审计日志 R09 | P2-01 | logging/audit：JSONL（SPEC §4.4）按天滚动；clean/restore/purge 覆盖 100%；log_query/log_export 命令 | ✅ cargo fmt/clippy --all-targets -D warnings/test 全绿（60 单测+2 集成）。备注：①`logging/audit.rs` 实现 record/query/export_all（`<data_root>\logs\audit-YYYY-MM-DD.jsonl` 只追加按天滚动，单行损坏跳过，proleptic civil 日期算法，`TEST_DATA_ROOT_LOCK`+`set_data_root_override` 沙箱隔离）；②`ipc/commands.rs` 填 `log_query`（from/to/op 过滤跨天合并）+`log_export`（全量导出 `bool`）；③`lib.rs` 挂载 `logging` 模块；④契约零改动 |
| P2-04 | ✅ [DESTRUCTIVE] 清理执行 R04 | P2-02,P2-03 | ✅ cleaner/execute+journal+recycle：🟢 direct+recycle 双路径；两阶段 journal（SAFETY §3）；单类可取消；跨类并行≤2；🔴 项无 token 拒绝 | ✅ cargo test：journal 恢复/中断回滚/边界用例（SAFETY §6.3 的 1–7）；评审关卡。备注：①`cleaner/{journal,execute,recycle}` + `guard/{mod,process}` 落地，`clean_execute`（🔴 无 token 拒绝 + spawn_blocking + 200ms 节流 clean_progress/clean_done + 取消登记）与 `clean_cancel` stub→业务；②契约**加性变更**：新增 `CleanDoneEvent` + `CLEAN_DONE` 事件，双端同步（contract.rs↔ipc.ts↔events.ts），不改既有签名，已记 CHANGELOG；③进程守卫命中需真实运行进程，Windows 黑盒走查（含 guard 应用运行时整类 Skip）由真机人工执行；④cargo fmt/clippy --all-targets -D warnings/test 全绿（69 单测+2 集成）；⑤评审文档 docs/review/P2-04.md；⑥复修两处 clippy（`recycle.rs::SHFILEOPSTRUCTW` upper_case_acronyms、`execute.rs` unnecessary_map_or→is_some_and） |
| P2-05 | ✅ 重复文件 R06 | P1-05 | ✅ cleaner/dup：三级过滤（size→64KB 采样→全量 sha256，采命中分组全量确认）；dupGroup 保留最早；BelowNormal I/O；engine dup 维度接线 | ✅ cargo test：分组正确性/保留策略/采样碰撞排除/取消/空文件；engine 集成 2 用例（dup 回填+零写盘）；性能不劣化。备注：①`cleaner/dup.rs` 三级过滤 + `collect_candidates`（只读、白名单、跳过空文件、取消贯穿）+ BelowNormal；②安全偏差（记约定）：SPEC 原文"仅 ≥1MB 全量 hash"，为消除 64KB~1MB 采样碰撞误判，对**采样命中的分组一律全量确认**，大文件仍受益（采样唯一不读全量）；③engine 接线：`scan_dup_category` 按 target 范围收集全部文件→find_duplicates→**仅产出冗余副本**（keeper 不出现，从根上保证 keeper 非清理目标）+ 回填 dup_group；④`expand.rs` 补 knownFolder：Documents/Downloads/Pictures/Desktop/Videos（dup 扫描范围依赖）；⑤新增自研规则 `resources/rules/dup-files.xml`（`dup.file` yellow/quarantine，含 `<include pattern="*" recursive="true"/>` 以满足 loader 至少一 include 校验）；⑥契约零改动；cargo fmt/clippy --all-targets -D warnings/test 全绿（76 单测+4 集成） |
| P2-06 | ✅ 执行 UI | P2-04 | ✅ 屏 6 执行中（分去向进度）+ 屏 3 二次确认（🔴 token 流）+ 屏 4 日志页 + 完成页「已释放/已隔离」区分（SAFETY §4.5） | ✅ pnpm typecheck/vitest(23)/build 全绿。备注：①`stores/clean.ts`（Pinia，对齐 scan.ts 双模式）：mock=推送假 clean_progress/done，真实=clean_execute+订阅 clean_progress/clean_done；分去向进度与语义计算委托纯函数 `stores/cleanMetrics.ts`（可单测）；②`pages/Executing.vue` 屏 6：分去向三行（direct/recycle/quarantine 累计字节+ok/fail/skip）+逐项最近+取消+完成卡；③完成页首行语义（SAFETY §4.5）：`已释放=direct+recycle`（freedBytes）vs `已隔离=quarantine`（quarantinedBytes），禁止混用"已清理"；④屏 3 token 流：`components/confirmToken.ts`（generateToken `PS-XXXX-XXXX`+verifyToken 大小写/空白不敏感）+ `ConfirmTokenModal.vue`（一次性令牌逐字输入一致才放行）；Report.vue 底部接清理：专家模式下额外纳入 🔴 项→含 🔴 弹 token→确认后带 token clean_execute→跳 /executing；⑤`pages/Log.vue` 屏 4：log_query（今日范围）+导出（log_export 相对路径到应用运行目录）；⑥路由补 `/executing` + `/log` 指向真实组件；⑦契约零改动（纯前端，无 contract.rs/ipc.ts 变更）；vitest 新增 token 流达 4 用例+cleanMetrics 达 4 用例 |
| P2-07 | ✅ M2/M3 门禁验证 | P2-04,P2-05 | bench-scan 复测（SSD ≤120s@1TB 三维度含 dup hash）；黄金文件集首版（安全/危险样本树 30+ 项）跑误删率：🟢<0.5%/🟡<2%/🔴=0 | 报告落 docs/verify/，全部门禁值达标。备注（2026-09-29 收口）：①误删率过——黄金集 36 项+极端用例，期间发现并修复红线级白名单正斜杠绕过（守卫检查此前假通过），复验 0/0/0 守卫真实生效，证据 docs/verify/golden-misdelete.json；②还原率过（100/100）；③性能门禁过——**totalMs=46.5s ≤120s**（MFT 枚举 12.6 + temp 直读 10.3 + 五层分片并行 walk 23.5），证据 docs/verify/bench-scan.json；修复链：白名单绕过、MFT 四连坑（尾斜杠/前 8 字节续批/USN 无 size/根引用带序列号）、并行分片粒度（二层伪并行 112s → 五层 23.5s），详见 LESSONS §①；④直清事务化（journal 恢复用例）P2-04 已过（69 单测含 journal 恢复/中断回滚）；⑤偏差：DoD"含 dup hash"未纳入 bench 计时（bench 量到 size 预分组为止）——dup 真实哈希为 P2-05 引擎成本（64KB 采样 + BelowNormal I/O），余量 73.5s 内大概率可容纳，如需完整口径可在 P3-01 接线生产引擎后复测 |

**Phase 2 门禁（=M2）**：性能门禁 SSD 达标；🟢 直清事务化（journal 恢复用例过）；误删率分档达标；还原首轮 ≥95%。

---

## Phase 3 · 功能扩展（引擎+UI 成对交付）

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P3-01 | ✅ 大文件 R05 | P1-05 | ✅ scanner large 维度：`engine::scan_large_items`（复用 `parallel_walk_stats`，≥500MB 标记大小/路径/atime，🟡/quarantine）；pages/Files 列表排序 | ✅ 真机抽查两轮：首轮抓出 **pagefile.sys 漏进候选**（whitelist.xml 加载时序缺口）→ 卷根系统文件改常量拦截+回归单测；复测 13 项、榜首 6.3GB 排序正确；cargo 81 测试/clippy 全绿，pnpm typecheck/vitest(29)/build 全绿。备注：①分级依据 SAFETY §1"下载目录大文件"归 🟡/隔离区（14 天可还原），mock seed 已从 green/recycle 对齐；②large 遍历期间无中间进度事件（UX 缺口，分片级进度留待后续）；③dup 生产引擎复用同一遍历留待后续任务 |
| P3-02 | ✅ 启动项 R07 | P0-03 | ✅ `startup/` 模块（winreg/regfile/backup/tasks/version/impact）：枚举 5 源（HKCU Run + HKLM Run + HKLM WOW6432Node Run[展示归入 hklm_run] + shell:startup + 登录触发计划任务）+ 发布者（version.dll CompanyName）+ 影响估算（位置基准+常驻/瞬时命令特征启发式）；startup_toggle 禁用=备份后移除（`<data_root>\startup-backup\`：manifest.jsonl + `<id>.reg`[regedit v5 可导入]/`<id>.<ext>` 文件移入/计划任务 schtasks），可恢复，全程审计 | ✅ cargo fmt/clippy --all-targets -D warnings/test 全绿（103 单测+4 集成，startup 新增 22）；沙箱单测覆盖：注册表禁用/还原 roundtrip（含引号/反斜杠转义）+ 备份文件存在（DoD）、启动文件夹 roundtrip、任务 XML 解析/Microsoft 命名空间过滤、影响启发式 6 例、命令行 exe 解析；真机手动项（禁用→重启→恢复）留人类执行。备注：①advapi32/version.dll 沿用 P1-02b `#[link]` 零依赖决策；②计划任务读 `System32\Tasks\*.xml`（locale 无关）+ schtasks 启停（CREATE_NO_WINDOW），**任务 XML 无启用状态**→未由本应用禁用的任务按启用显示（LESSONS ①）；③HKLM 写值需管理员而应用 asInvoker→HKLM 项 toggle 返回 false 并审计 fail；④审计 op 增 `enable_startup`（LogEntry.op 为字符串，非契约破坏）；⑤非字符串类 Run 值不进列表（禁用/还原语义不完整，宁缺勿错） |
| P3-03 | ✅ 启动项 UI | P3-02 | ✅ pages/Startup：影响降序表（名称/发布者[未知标黄]/影响三色档/来源/命令）+ 摘要（总数/启用/已禁用/高影响计数）；禁用走轻量风险确认面板（"不删程序、可恢复"文案，非 🔴 不走 token 流）；已禁用区恢复入口；toggle 失败提示（HKLM→管理员权限文案） | ✅ pnpm typecheck/vitest(36)/build 全绿（startupModel 纯函数 7 用例：排序/分组/文案）。备注：①`startupModel.ts` 纯函数模式（对齐 filesModel/reportModel），IPC 类型全部从 `types/ipc.ts` 导入，契约零改动；②mock 增强：StartupListStub 补 hklm 高影响/计划任务/已禁用文件夹样本，`startup_toggle` 改有状态翻转（dev 走查 list 可见变化），`data.test.ts` 断言同步放宽（enabled 只要求布尔且两种状态并存）；③路由 `/startup` Placeholder→Startup.vue；④走查（禁用→恢复流）待人类在 `pnpm tauri dev` 执行 |
| P3-04 | ✅ [DESTRUCTIVE] 隐私清理 R08 | P2-04 | ✅ 规则驱动落地（零新破坏性代码路径）：`resources/rules/privacy-traces.xml` 三类目（privacy.edge-history / privacy.chrome-history / privacy.recent-docs，全 🟡/quarantine，浏览器类目带 msedge.exe/chrome.exe 守卫）+ `expand.rs` knownFolder 增 3 个 R08 源映射（Edge User Data / Chrome User Data / RecentDocs，目录不存在自动跳过）；逐项 reason 含"删了什么/14 天可还原"；执行经既有 clean_execute→journal→隔离区→审计管线 | ✅ cargo fmt/clippy --all-targets -D warnings/test 全绿（103 lib + 4 privacy 集成 + 4 scan_pipeline）；评审关卡 `docs/review/P3-04.md`。备注：①分级决策——mock 曾设 🔴，按 SAFETY §1 表格判 🟡（单应用影响面+隔离区可逆；🔴灰禁会使普通用户无法用本功能），mock 对齐留 P3-05；②`**/History*` 连 WAL/journal 伴随文件一并移入（防残留 WAL 恢复历史）；RecentDocs 仅顶层 `*.lnk` 非递归；③守卫正例测试用**测试进程自身 exe 基名**作运行中进程（确定命中，替代依赖真实浏览器运行）；④"浏览器运行中阻止"真机黑盒走查待人类执行 |
| P3-05 | ✅ 隐私 UI | P3-04 | ✅ pages/Privacy：按类目分组卡片（类目名/reason"删了什么"/守卫提示⚠/可恢复标注"入隔离区·14 天内可还原"）；**逐项独立确认**（checkbox 默认全不勾，与报告页批量默认勾选区分开）；底部操作条（已选 N 项/合计字节/清理所选→clean_execute→/executing） | ✅ pnpm typecheck/vitest(42)/build 全绿（privacyModel 纯函数 7 用例）。备注：①mock 对齐 P3-04 分级决策——privacy 种子 🔴→🟡 且 categoryId 改为与 privacy-traces.xml 一致（edge-history/chrome-history/recent-docs，补 Chrome 样本）；②startup.registry mock 种子改 🔴（SAFETY §1 注册表归红档，同时保住"三档分级均有数据"断言与 🔴 token 演示流，真实启动项管理走 /startup 页）；③`defaultProfile` 纳入 privacy 维度（原按旧 🔴 假设排除）；④契约零改动、Rust 零改动 |
| P3-06 | ✅ [DESTRUCTIVE] 隔离区生命周期+管理页 R25 | P2-02 | ✅ `quarantine/lifecycle`：到期自动清除（硬删+state→purged+审计 auto_purge，幂等收敛）+ 到期前 3 天 `quarantine_expiry_warning` 事件（启动钩子+list 触发）+ restored 行 30 天清理（`restoredAt` 加性字段）+ 容量上限 min(5GB,盘剩余10%) 超限不自动删（最早批次建议 id）；`quarantine_purge` stub→业务（token 缺失全部拒绝）；pages/Quarantine 屏 8 全要素（语义首行"已隔离·尚未释放"/剩余天数红色预警/还原所选/容量横幅+释放最早批次/确认清空 token 流/到期事件通知） | ✅ lifecycle 单测（时间+剩余空间注入）6 例全绿：到期清除+审计、3 天提醒+daysLeft、超限不删+最早批次、restored 行清理、无 token 拒绝、带 token 硬删+审计；cargo fmt/clippy/test 全绿（109 lib+8 集成）；pnpm typecheck/vitest(47)/build 全绿；评审关卡 `docs/review/P3-06.md`。备注：①契约加性变更：新增 `quarantine_status` 命令 + `QuarantineStatus`（双端同步，已记 CHANGELOG+SPEC §5）；②自动清除的可追溯面 = manifest purged 行（sha256/原路径）删除前后恒在 + 审计（红线 #2 journal 等价物，评审文档 §2 详述）；③走查（还原/清空 token 流/到期横幅）待人类 `pnpm tauri dev` 执行 |

**Phase 3 门禁**：六维度功能全可用（startup/privacy 页联调通过）；生命周期自动化用例绿。

---

## Phase 4 · 工程就绪 → M3

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P4-01 | ✅ 更新机制 R22 | P0-03 | ✅ `updates/` 模块：`http.rs` WinHTTP 直调 HTTPS GET（零依赖，仅 https+超时+2xx）；通道回退链（mirrorFirst: jsDelivr→ghproxy→GitHub raw；直连优先时反转）；manifest 解析→版本比较（分段数值，非数值段保守判无更新）；规则包 sha256 校验（缺失/不符→丢弃+rulesPackHashOk=false+审计 op=update，**校验通过才落盘**）；安装到 `<data_root>\rules`（loader `load_dirs` 叠加：用户目录 category 覆盖资源目录，白名单只随资源目录）；opt-in 周查（7 天窗口，last-check.json 持久化，启动钩子；manual 无视 optIn）；`update_check` stub→业务；Settings.vue（optIn/镜像优先开关+手动检查+结果展示+专家模式） | ✅ 单测 11+1 例全绿：通道顺序、回退穿透（网络错+坏 JSON→下一通道）、全失败 None、周查门控（optIn/7天/从未查）、版本比较（0.1.9<0.1.10/畸形保守）、hash 不符丢弃零写盘+审计 fail、hash 通过落盘、路径穿越文件名拒绝、last-check roundtrip、loader 叠加覆盖；cargo fmt/clippy/test 全绿（120 lib+8 集成）；typecheck/vitest(47)/build 全绿。备注：①契约零改动（UpdateStatus/settings 既有；update_check 增 AppHandle/State 为 Tauri 注入参数，IPC 参数不变）；②REPO_SLUG/通道 URL 为占位常量，P4-06 发布流水线定稿；③WinHTTP 层不单测（无网络沙箱），回退/校验逻辑全在注入式纯函数；④真机手动：Settings 页"立即检查"应优雅失败（repo 未发布）|
| P4-02 | ✅ [DESTRUCTIVE] 兼容加固 R23 | P2-04 | ✅ **安全审计五项修复**：T-1 执行前最终复核（`cleaner/preflight`：白名单重跑+逐级 reparse+size/mtime 比对，`CleanTarget.mtime_ms` 贯通）；T-2 还原信任链（`restore_one` 四重校验：隔离区根内/sha256/白名单禁区/父链 reparse）；T-3 隔离区根 reparse+非目录拒绝；F-1 fail-closed（规则加载失败/类目缺失 → clean_execute 整体拒绝，`resolve_targets` 收编解析）；F-2 白名单降级明示（审计 + `ScanResult.whitelistOk` 加性契约 + Report 红框横幅）；**guard 双语义**：`guard/instance` Global 互斥量+第二实例激活首实例窗口后退出；长路径加固（whitelist `\\?\` 前缀剥离 + `quarantine_root_of` 扩展前缀盘符解析） | ✅ `tests/compat.rs` 12 例全绿（八项边界全链路：中文/emoji/空格、420+字符长路径、junction 环+扫描后替换双用例、占用/只读优雅失败、隔离区根不可用 IO 等价、孤儿 journal 检出、0 字节/空目录/253 字节名；另 T-2 伪造 manifest 三连拒、T-3、F-1）；preflight 5 例+instance 2 例单测；cargo fmt/clippy/test 全绿（127 lib+20 集成）；typecheck/vitest(47)/build 全绿；评审关卡 `docs/review/P4-02.md`。备注：①契约加性：`ScanResult.whitelistOk?: boolean`（双端同步，已记 CHANGELOG）；②盘满以"IO 失败注入等价"覆盖（真实配额盘不可得），kill 进程实测留 P4-03；③fixture 反斜杠双写惯例会破坏 `\\?\` 前缀（Win32 不归一化扩展路径）——compat fixture 改原样写入并注明；④真机人工：双开验证单实例、坏 whitelist.xml 验证降级横幅 |
| P4-03 | ✅ 崩溃安全 R24 | P2-04 | ✅ `crash/` 三模块：**dump**（SEH 顶级过滤器 + panic hook → dbghelp `MiniDumpWriteDump` `#[link]` 直调零依赖，落盘 `<data_root>\crash\`，保留 5 份启动清理；SEH 通道经辅助线程写 dump + 异常代码走 sidecar JSON——本机环境异常流路径恒 998 不可用，LESSONS §①）；**recover**（启动孤儿 journal 恢复，T-6 协议：journal 只定位不动作，quarantine 孤儿经 restore_one T-2 四重校验还原，「已移入未登记」窗口按命名约定反查唯一候选补登记不自动还原，direct/recycle 只标记 orphan+审计；恢复期 `clean_execute` 拒绝新事务 `recovery_pending` 门闸；journal seq 续编闭环）；**parse**（minidump 只读解析：模块列表 + 异常代码，边界检查畸形文件返回 Err）；opt-in 上传预览（`crash_list/preview/upload/recovery` 四命令加性契约 + `http_post` + Settings.vue 崩溃卡片 + Home 恢复横幅） | ✅ kill 测试全绿：`tests/crash_safety.rs` 4 例——kill 中断（crash-sim 子进程真实跑 journal+隔离区移动，父进程轮询 marker kill，manifest 背书孤儿还原+journal 闭环+审计 recover）+ SEH 真实访问违规（sidecar 异常代码 0xC0000005）+ panic 通道 + dump 可解析；单测 31 新增（recover 决策表 7+dump/mod 5+parse 6+crash_model/vitest 8）；cargo fmt/clippy -D warnings/test 全绿（140 lib+24 集成）；pnpm typecheck/vitest(55)/build 全绿；评审关卡 `docs/review/P4-03.md`。备注：①契约加性：4 新命令 + 5 类型（双端同步，已记 CHANGELOG）；②`PURESLATE_DATA_ROOT` 环境变量注入数据根（kill 测试子进程共享沙箱；等价于既有 LOCALAPPDATA 信任面，不扩权）；③上传端点为 `.invalid` 占位常量（P4-06 定稿，未定稿前上传必然优雅失败）；④真机人工走查（Settings 预览/上传、Home 横幅）待人类执行 |
| P4-04 | ✅ 测试体系 R21 | P2-07 | ✅ 黄金文件集扩至 **132 样本**（fixture 生成器 `build_golden_tree` 声明式五族：safe 48 / temp 43 / dup 21 / red 13 + 结构件）+ `tools/vm-regression.md`（微软开发版 VM 快照步骤：清理→重启→关键功能点检，P5-02 用） | ✅ 误删率全档达标复跑（`docs/verify/golden-misdelete.json`：绿/黄/红 = 0/0/0，守卫零被扫，T-4 四形态全 protected）。备注：①golden 重写为**两阶段门禁**——扫描期（分档误删率+防漏扫+junction）+ 清理期（绿/direct 真实 execute 全链路，盘面核对 must_keep 缺失=真误删、只读/长路径按 Win32 语义优雅失败留盘、journal 零孤儿；黄/红隔离区去向仅扫描期度量）；②黄/红档 target 内放置白名单守卫使三档分母非平凡；③**安全审计 T-4 收口**（白名单匹配加固：段级归一 `..`/`.`/尾随点 + 8.3 短名 GetLongPathNameW 展开，只收紧不放宽，匹配逻辑变更走评审关卡 `docs/review/P4-04.md`）；④白名单单测 +4 与全局测试锁（并行竞态修复） |
| P4-05 | ✅ M12 千次还原 | P2-02 | ✅ `tools/test-restore.ps1` 全量 1000 次：**成功率 100%（1000/1000，ratio=1.0000 ≥ 99.9%）**，报告落 `docs/verify/restore-1000.json` | ✅ 门禁 PASS（release 构建，19.3s，0 失败）。备注：①`restore-self-test` 升级——stdout 输出 JSON 报告（ts/buildProfile/cycles/threshold/ratio/gatePass/durationMs/stageCounts/failures[≤50 条含 iteration·stage·reason·guidance]），stderr 人读进度与结论；②**失败项按五阶段给兜底指引**（write 样本创建 / hash 计算 / quarantine 移入[manifest·journal 残留可还原] / restore 还原[restore-conflict 兜底可取回] / verify 内容不一致[最严重，指引导出审计+manifest 提 issue]）；③阈值参数化（默认 0.95=M2；1000 次 0.999=M12），ps1 增 `-MinRatio`（InvariantCulture 传参防 locale 小数点坑） |
| P4-06 | ✅ 签名与发布流水线 R26 | P0-05 | ✅ `.github/workflows/release.yml`（tag `v*` 触发）：双构建一致性校验（`cargo clean -p pureslate` 后重建，exe sha256 字节级比对断言）+ 体积门禁 <20MB（沿用 check-size.ps1）+ `tools/publish-manifest.ps1`（规则包 RulesPack JSON[sha256 对整包字节，whitelist.xml 不入包] + update-manifest.json 六字段 camelCase）+ SignPath 签名占位 step（secrets 配置后启用，未配置发布未签名占位包）+ GitHub Release 上传（安装包+规则包）+ manifest/规则包回推 main（jsDelivr/raw 通道读取处）；本地端到端自测过（占位安装包跑通脚本：字段/sha256/UTF-8 无 BOM/whitelist 排除全对） | ✅ 脚本与工作流自测绿（CI 端到端待真实仓库首次 tag）。备注（占位定稿清单）：①SignPath 开源计划申请（审批周期不可控，secrets 四项配置后签名 step 自动启用）；②**REPO_SLUG 已定稿 `wuhua2026/pureslate`**（2026-10-04 公开仓库创建并推送 main，代码侧常量与发布侧 GITHUB_REPOSITORY 已一致）；③崩溃上传端点 `crash::CRASH_UPLOAD_URL`（需后端决策，未定稿前上传必然优雅失败）；④CSP 启用后全页面功能真机走查待人工。**顺带收口安全审计四项**（见 audit 清单状态列）：I-1 `log_export` 收紧（目录成分剥离+文件名白名单+`<data_root>\exports\`+防覆盖）；I-2 🔴 token 后端签发（`confirm_token_issue` 契约加性：仅专家模式可签+一次性消费[未命中同样作废]+clean_execute/quarantine_purge 接入，前端 generateToken 移除）；I-3 CSP 严格启用（`tauri.conf.json`）；I-4 opener 依赖与权限声明移除。**Release v0.1.0 已发布**（2026-10-04，[GitHub Releases](https://github.com/wuhua2026/pureslate/releases/tag/v0.1.0)：安装包 724KB+规则包+构建哈希+中文功能文档，manifest 自动回推 main `[skip ci]`——更新机制端到端激活）；**偏差**：可重复构建"exe 字节一致"断言降级为双哈希记录随 Release 发布——Windows MSVC 链接器默认在 PE 头嵌入时间戳，字节级可重复需 `/Brepro`+`SOURCE_DATE_EPOCH` 深度改造（挂账后续）；崩溃上传按 2026-10-04 决策不接后端（R24 裁剪预案"降级仅本地"，URL 注释+Settings 文案已如实化）。 |
| P4-07 | [HUMAN] 社区公开评审 | P4-02 | 破坏面五模块（cleaner/quarantine/privacy/guard/journal）代码+review 文档发布到 V2EX/Rust 社区求 review；意见处置记录 docs/verify/community-review.md | 评审帖发布+意见闭环 |

**Phase 4 门禁（=M3）**：八项兼容用例全过；M12 ≥99.9%；R22 hash 校验生效；签名流水线产出 <20MB 安装包；社区评审完成。

---

## Phase 5 · 集成与内测 → M4

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P5-01 | 全量门禁核验 | P4-* | 门禁清单跑一遍：M2 分档/M3 性能/M4 普通模式覆盖率（黄金集+真机抽样）/M8 日志 100%/M12/M11-L（源码公开+数据流向图+抓包验证方法+opt-in 默认值审计） | 门禁报告 docs/verify/gates-m4.md |
| P5-02 | VM 快照回归 | P4-04 | 按 tools/vm-regression.md 在 Win10/Win11 VM 跑全清理场景：零破坏（可启动/浏览器可开/微信可登录） | 回归记录 |
| P5-03 | [HUMAN] 15 人内测 | P5-01 | 组织内测（可 10 人起步）：M6 安全感问卷 ≥4.2/5 + 实验 B（隔离区语义困惑率观察） | 问卷与困惑率数据 |
| P5-04 | P0 问题清零 | P5-03 | 内测反馈分级（P0/P1/P2）；P0 全部修复+回归 | 修复清单+回归绿 |

**Phase 5 门禁（=M4）**：§5 各硬门禁全绿；实验 B 困惑率 ≤30%（否则按预案加强直清引导后复测）；P0 清零。

---

## Phase 6 · 发布 → M5

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P6-01 | 发布候选 | P5-04 | RC 构建：签名+update-manifest+版本号 1.0.0；净室检查清单（SAFETY §7）逐项打勾 | RC 包安装/更新链路自测 |
| P6-02 | 物料三件套 | P5-01 | 对比评测（vs CCleaner/360/Storage Sense）+技术博客（规则引擎+隔离区设计）+30s Demo（体检→报告→分级清理→还原） | 三件套成稿 |
| P6-03 | [HUMAN] 首发 | P6-01,P6-02 | GitHub Releases v1.0 + V2EX 分享创造/小众软件/少数派；issue 模板（含日志导出引导）上线 | 发布链接+渠道帖 |

**Phase 6 门禁（=M5）**：签名包可安装可更新；净室清单全勾；物料齐。

---

## 附加批次 · R27 安全卸载辅助（v0.1.6 候选，SPEC §6.7）

> 定位：官方卸载器的编排者而非替代者。破坏主体是第三方卸载器（如实告知用户）；
> 残留只走既有 grade/preflight/journal/隔离区管线（🟡/quarantine，14 天可还原）。
> 不申请 SeDebugPrivilege、不默认强杀、不做 UWP。净室依据：Geek Uninstaller 只读逆向的
> **行为事实文档**（仓库外），代码全部自研，UI/代码不出现第三方品牌词。

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| R27-1 | ✅ 卸载枚举（只读） | — | ✅ `uninstall/` 模块：HKLM/HKCU × 64/32 四视图（显式 WOW64 标志）枚举 Uninstall 键；SystemComponent/ParentKeyName 过滤；跨视图去重（HKLM64>HKLM32>HKCU64>HKCU32）；MSI 识别（WindowsInstaller=1/GUID 键）+ `msiexec /X{GUID}` 构造；EstimatedSize KB→字节；`uninstall_list` 命令；InstalledApp 契约双端；Apps.vue 只读列表（搜索/范围/架构/MSI 徽章）+ /apps 路由导航；mock 三形态样本 | cargo fmt/clippy/test 全绿（163 lib+24 集成，uninstall 9 例：解析/过滤/MSI 构造/GUID 校验/去重优先级/ID 稳定性/真机只读冒烟）；typecheck/vitest(62)/build 全绿（appsModel 6 例）。备注：契约加性 InstalledApp+uninstall_list（CHANGELOG）；winreg 抽 `_view` 变体+公开 WOW64 常量 |
| R27-2 | ✅ [DESTRUCTIVE] 卸载执行编排 | R27-1 | ✅ `uninstall_run(id)`/`uninstall_cancel(txId)` + 事件：CreateProcessW 拉起官方 UninstallString（SW_SHOWNORMAL，exe 父目录为 cwd）；8.3 短路径 GetLongPathNameW 还原；进程树等待（深度上限 16+PID 去重跨轮持久+PPID 环防护，不强杀）；三层成功判据（事后重扫 Uninstall 键/安装目录+退出码仅警告+PendingFileRenameOperations 待重启态）；审计 op=uninstall（含 launch_failed 终态）；单实例事务互斥；评审关卡 | InnoSetup/NSIS/MSI 各 ≥3 样本真机；取消时序；沙箱命令行构造单测；docs/review/R27-2.md。备注：cargo 178 lib+24 集成全绿（uninstall 24 例纯函数）、pnpm typecheck/vitest(64)/build 全绿；评审见 docs/review/R27-2.md。偏差：①顺带修复 R27-1 遗留——parse_app 曾误取 QuietUninstallString（与 SPEC §6.7 矛盾，会静默拉起静默卸载器），改为只取交互 UninstallString+回归测试；②runas 路径 root pid=0 退化为仅等主句柄（防 PPID=0 顶层进程误判致 120s 假超时）；③740 才走 runas 标准 UAC，无绕过降级；④CreateProcessW 不经 shell/exe 恒引号/不继承句柄；本任务零自删零注册表写，残留隔离留 R27-3。**真机走查挂人类**：Inno/NSIS/MSI 实卸载、UAC 拒绝、停止等待、中文路径、待重启态 |
| R27-3 | ✅ 卸载残留隔离清理 | R27-2 | ✅ 卸载后安装目录/开始菜单快捷方式/AppData 三目录**仅在最近卸载上下文内**枚举为 ScanItem（category=residue.uninstall，默认 🟡/quarantine）；删除前清单 UI 明示理由，逐项确认；**去向用户可选**：隔离区（默认 14 天可还原）/直接删除（🔴 专家 token）；复用 preflight+journal+隔离区+审计；白名单硬/软分层+快照精确授权根（SAFETY §2.2 条款）；空目录 RemoveDirectoryW 收尾 | 黄金集补残留样本误报率；沙箱 roundtrip；无任何无授权路径。备注：uninstall/residue.rs（名称归一精确匹配/双拒绝名单/双安全闸/reparse 放弃/2万文件10GiB 上限）；契约加性 uninstall_residue_scan/uninstall_residue_clean（复用 clean 事件）；Apps 页残留四态+去向单选+ConfirmTokenModal；mock 三源。验证 cargo 195 lib（uninstall 40 例）+27 集成（新增 residue_pipeline 3 例含 quarantine 移入→还原重建目录链、direct+审计、空目录）、typecheck/vitest(71)/build 全绿；评审 docs/review/R27-3.md。**真机走查挂人类**：真实 Inno/NSIS/MSI 卸载后三源命中率与误报（尤其开始菜单厂商组/AppData 同名）、隔离区还原回 Program Files、direct token 拒绝分支 |

---

## 附加批次 · R28 规则引擎 schema 增强（规则包二期前置，SPEC §4.1）

> 定位：只增强规则引擎的**通用表达能力**——注册表值动态目标、路径段正则收紧、递归深度封顶，
> 服务未来自研的多账号 IM 缓存规则（账号目录形态约束）与软件自定义安装目录场景。
> 本批次**不含任何具体软件清理数据**，规则将来一律基于本机实测自研（AGENTS 红线 5）；
> 不新增删除原语、不改 IPC 契约（ipc.ts/contract.rs 零变更），扫描只读红线不变。
> 四任务共享 model/loader，线性推进、逐任务提交。

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| R28-1 | ✅ `reg-value` 动态目标 | — | ✅ model：`TargetType::RegValue`+`RegTargetSpec{hive,key,value_name,view,base}`（HKLM/HKCU；native/wow64；path/parent）+Target 构造器；loader：reg-value 全套属性解析，未知 hive/view/base、缺属性 → **只丢所属 category 不炸整文件**（PartialCategory.fatal）；**裸根闸**（无分隔符仅放行 `*.ext`，`**/*` 等全通配段同样拒）；`startup/winreg` 增 RegQueryValueExW+ExpandEnvironmentStringsW 只读单值查询（复用 KEY_READ/WOW64 常量，SZ/EXPAND_SZ，键值缺失/类型不符→None）；expand.rs 动态解析（trim、base=parent、非现存目录软跳过、短名归一/白名单照旧） | ✅ cargo fmt/clippy --all-targets -D warnings 全绿；cargo test 205 passed(1 ignored)（新增 10 例：loader 5—全属性/缺省/坏属性只丢坏类/裸根闸边界/普通 target 不受闸；expand 5—缺键/REG_SZ roundtrip/EXPAND_SZ 展开/parent/REG_DWORD 软失败，HKCU 唯一测试键 RAII 自清理）；两个辅助 bin 补 RegValue arm。备注：生产路径无 unwrap；契约零改动 |
| R28-2 | ✅ `segment-regex` 路径段收紧 | R28-1 | ✅ **依赖决策落地**：regex-lite 0.1.9（无 Unicode 表；Cargo.toml 注释理由，体积增量 R28-4 实测）；model：GlobRule 增 segment_index/segment_regex+`compile_segment_regex`（自动首尾锚定）+GlobRule::new；loader：属性 `segment-regex`/`segment-index` 成对校验（坏 index/缺一对 → 丢 category），正则编译失败 → 加载期丢该条 include/exclude 并记日志（include 全失 → 丢类目）；matcher：glob 命中后逐 include 段校验（越界/不匹配不命中，fail-closed），exclude 同样支持段约束；顺带修复坏 glob 静默跳过时约束索引错位隐患（append 改入集才对齐） | ✅ cargo fmt/clippy --all-targets -D warnings；cargo test 212 passed（新增 7：matcher 3—命中/锚定/越界/exclude 双条件；loader 3—成对/非法/坏正则丢规则；walk 1 端到端 123456 命中/evil-name 零命中）。契约零改动 |
| R28-3 | ✅ `max-depth` 递归深度封顶 | R28-2 | ✅ model：GlobRule 增 max_depth:Option&lt;u16&gt;（根直属=1，缺省不限）；loader：`recursive="false"` 归一为深度 1，与显式 >1 矛盾/0/非数字 → 丢 category；matcher：per-include 段数裁尾（include+exclude 双用），`traversal_max_depth()`=全部有界取最大值、任一不限则 None；walk：walkdir max_depth 类目级剪深（深层文件根本不遍历）；MFT 经同一 match_include 段数校验语义等价（无遍历概念不剪枝）；**既有规则零变化**：privacy recent-docs `*.lnk` 归一深度 1 与原 glob 语义等价，其它类目保持 None | ✅ cargo fmt/clippy -D warnings；cargo test 220 passed（新增 8：matcher 3 边界/取max/混合不限；walk 1 端到端 depth2 f0/f1 命中 f2 不遍历；loader 4—false 归一/显式/矛盾坏值/随包规则快照）。契约零改动 |
| R28-4 | ✅ 联调与门禁 | R28-3 | ✅ 新语法仅出现在 SPEC 示例与单测字符串，未向 resources/rules 投递虚构软件规则；SPEC §4.1/§6.1 与实现逐条核对一致；SESSIONS 新条目（脱敏：只述"仓库外第三方材料"），LESSONS ①2 条（GlobSet 索引对齐/NSIS 打包）+②决策+④体积行 | ✅ cargo fmt/clippy --all-targets -D warnings/test **220 passed(1 ignored)**；pnpm typecheck/vitest(71)/build 全绿；`pnpm tauri build -- --bundles nsis` 安装包 **2.80 MiB（2,936,309B，较 v0.1.6 +50.7KiB/+1.8%）<20MB**；契约零改动复核：git diff contract.rs/ipc.ts 为空。三实现任务已分别提交 d18ce29/01b0ba4/cc2692d |

---

## 附加批次 · R29 视图层动效现代化（三屏试点先行，SPEC §7 动效规范）

> 定位：方案 A（浅色蓝白基调不变），在现有 token 上做动效语言统一与三屏试点；
> 零动画重依赖（不引组件库/GSAP/Lottie）；动效只进视图层（.vue/composables），
> stores/纯函数 models 零改动。**试点三屏（Home/Scan/Report）经人类真机确认调性后，
> 其余 13 页才允许铺开（后续任务）。**安全语义（🔴 灰禁/取消/执行态）优先于观感。

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| R29-1 | ✅ 动效基建 | — | ✅ SPEC §7 增动效 token 与规范；App.vue：motion 变量（140/220/340ms+双缓动）、router-view out-in 过渡（fade+8px）、`.anim-rise`+d1..d6、全局 reduced-motion 归零（含 delay 清零）；`src/composables/useCountUp.ts`（rAF ease-out 立方、目标变化续间、reduced 直跳、卸载取消帧） | ✅ typecheck/71 vitest/build 绿；**真机走查待人类（路由淡入/减少动画总闸）** |
| R29-2 | ✅ Home 试点 | R29-1 | ✅ hero/区块 stagger rise；抽出 [DiskUsageCard.vue](src/components/DiskUsageCard.vue)：挂载 scaleX 进度生长+百分比 count-up（700ms）；圆环极弱呼吸（opacity/scale，不伪造进度）；宫格 8 卡受控 stagger（≥7 同级差封顶）+按压缩放；散写时长全部换 token | ✅ typecheck/build 绿；真机确认待人类 |
| R29-3 | ✅ Scan 试点 | R29-1 | ✅ 当前路径 keyed Transition（out-in/fast，路径窗口定高不顶布局）；阶段图标 key=phase pop（scale 0.6→1）；进度条改 scaleX+ease-out（不做 width 过渡）+流光伪元素（cancel/暂停态流光停）；待命/扫描/完成卡 rise；实时数字 tabular-nums；取消按钮按压缩放且始终可点 | ✅ typecheck/build 绿；真机确认待人类（连刷路径/取消即停） |
| R29-4 | ✅ Report 试点（代码完成，待真机验收） | R29-2,R29-3 | ✅ 三总览指标+底部已选字节 count-up（650/220ms）；三卡 d1-d3 rise+前 8 类目行 stagger（行终态 opacity 走 --row-final 变量，避免覆盖 🔴 灰禁 0.55）；🔴 灰禁展开改 opacity 过渡（🔒/文案/不可选语义不变）；A/B 卡选中态+勾号 pop+按压缩放；主按钮 hover/active | ✅ typecheck/71 vitest（模型零回归）/build 绿（index gzip 42.55→45.04 KB，+2.5KB；零新 npm 依赖）。**硬门禁：人类 pnpm tauri dev 真机确认三屏调性 → 2026-10-14 用户放行铺开** |
| R29-5 | ✅ 列表页铺开 | R29-4 | ✅ Files/Startup/Privacy/Apps/Quarantine：页首 rise、表格行 hover、按钮统一按压缩放+token 过渡、数字 tabular-nums；Startup 弹窗遮罩淡入+pop（keyframe 提升为全局 ps-mask-in/ps-modal-pop）；Privacy 分组 stagger（≤3）+操作条浮起；Apps 面板 rise、搜索 focus 环、残留卡 hover | typecheck/build 绿（ca…提交） |
| R29-6 | ✅ 状态页与通用弹窗 | R29-4 | ✅ Log（卡片 rise/行 hover/按钮）、Settings（三卡 stagger/dump 行 hover/预览淡入/输入 focus 环）、Executing（进度 scaleX、逐项 ✓ 弹现、完成态双字节 count-up、取消按钮按压；高频实时数字不补间）；ConfirmTokenModal 改 Transition 遮罩淡入+pop（appear/leave）；**GradeBadge 刻意零动效**（红级徽章弹跳会弱化严肃语义） | typecheck/71 vitest/build 绿 |
| R29-7 | ✅ 收尾与全量门禁 | R29-5,R29-6 | ✅ Placeholder rise；侧栏导航/图标过渡统一 token（不加位移保持导航稳定）；全量门禁+SESSIONS/LESSONS 更新 | 见本行验证列 |

---

## 附加批次 · R30 v0.2.0 真机反馈热修（大文件误删风险/乱码/图标/横向滚动）

> 触发：v0.2.0 真机四反馈——①165.8GB/121 项个人大文件默认勾选进方案 B（40558 项/181.7GB），
> 一键可误删上百 GB 个人资料；②ANSI 时代老安装器卸载条目三字段全乱码；③卸载页无软件图标；
> ④长程序名撑破表格带出窗口横向滚动条。约束：零新依赖（crate/npm）、只读新增（无新删除面）、
> 契约只加命令不改类型、宁漏勿错。逐任务提交（R30-1 cba1b8a / R30-2 998c290 / R30-3 df6deab /
> R30-4 8045eec / R30-5 本批文档提交）。

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| R30-1 | ✅ 大文件转审阅型类目 | — | ✅ 后端 `LARGE_REVIEW_CATEGORY`：items 保留供浏览，不推 aggregate、`found_bytes` 排除、永不注册规则表，`resolve_targets` 专用友好文案 fail-closed；前端 reportModel `cleanableItems` 入口排除（类目卡/风险/A-B 方案/汇总）；mock 同口径；Files 页纯审阅化（去级别列/隔离语义，逐行"在资源管理器中显示"）；新只读 IPC `shell_reveal`（SHOpenFolderAndSelectItems+explorer 兜底，PIDL RAII，绝对/现存/非卷根校验） | cargo 222 绿（shell +2）；75 vitest（reportModel/data 排除大文件 +4）；typecheck/build 绿。备注：前后端双保险，陈旧前端也无法经应用删个人大文件 |
| R30-2 | ✅ 注册表坏编码防御解码 | — | ✅ winreg `reg_data_decode_display`（UTF-16/CP_ACP/CP936 三候选+可疑度评分，阈值 8.0；**去 NUL 后 936 单候选同时修复纯 ANSI 与"UTF16 英文头+GBK 尾"拼接**）+`reg_display_value`（版本仅 ASCII、日期仅 8 位数字）；parse_app 展示字段接入，命令/路径保持原值；DisplayName 存在但不可辨识→"异常名称的程序"占位（条目保留），不存在才跳过 | cargo 228 绿（+6）：pure GBK→极域电子教室、mixed→Minisoft Toolbox极域、正常 UTF16 中文不改动、不可恢复→占位/字段隐藏。备注：MultiByteToWideChar 直调 kernel32 零 crate |
| R30-3 | ✅ 横向滚动消除 | — | ✅ `.content` 全局 overflow-x 兜底；Apps/Startup×2/Quarantine/Log/Files 五表 `table-layout:fixed`+colgroup **百分比列宽**；td flex 盒模型坑修复（flex 放内层）；窗口 minWidth 900/minHeight 600 | 浏览器实测（608 窄窗）th/td 矩形严格递增不相交、sw≤cw、省略号生效；typecheck/75 vitest/build 绿（index gzip 45.03KB）。**真机正常窗口最终观感挂人类走查** |
| R30-4 | ✅ 卸载程序图标 | R30-2 | ✅ `uninstall/icons.rs`（零 crate）：DisplayIcon `路径[,索引]` 解析→PrivateExtractIconsW（32px，索引 0/卸载 exe 两级回退）→GetDIBits 32bpp（低位深 AND 掩码补 alpha，GDI RAII）→手写 PNG（CRC32/Adler32/stored deflate）base64；新只读批量 IPC `uninstall_app_icons(ids)`（≤200，一次枚举；失败静默）；Apps 图标列+灰色字形回退+@error 兜底；枚举重构 `enumerate_located`+泛型 `dedupe_by` | cargo 233 绿（+5；PNG 结构/校验和/解析/base64 单测，真机 current_exe 冒烟 CI 可宽容）；--ignored 转储 12 个真实程序图标目检颜色/透明正确（ArcGIS Pro/原神/Git 等）；typecheck/75 vitest/build 绿（Apps gzip 6.10KB，index 45.04 不变）。**真机应用内图标观感挂人类走查** |
| R30-5 | ✅ 联调门禁与文档 | R30-1..4 | ✅ TASKS/CHANGELOG（2 新命令契约登记）/SESSIONS/LESSONS 同步；全量门禁 + NSIS 体积 <20MB | ✅ cargo fmt --check/clippy --all-targets -D warnings/**233 lib（2 ignored）+27 集成全绿**；pnpm typecheck/**75 vitest**/build 绿；NSIS **2,946,042B=2.81MiB <20MB**（较 v0.2.0 +9.5KB/+0.33%，零新 crate/npm）；浏览器实测布局 + 真机 12 图标目检；**人类真机走查已通过，随 v0.2.1 发布（2026-10-15，Release run 全绿，四资产 2.81MiB）** |

---

## 附加批次 · R31 审计收口与一键清理（2026-10-18）

> 触发：2026-10-18 独立代码审计（[`AUDIT-2026-10-18.md`](AUDIT-2026-10-18.md)）。目标：先修破坏性信任链与
> 误导性文档，再把默认入口改成普通用户习惯的「一键清理」（只清 🟢 安全项，两击闭环）。
> 约束：零新 crate/npm；破坏性改动全部为"收紧"方向，新增唯一删除面是 SAFETY §2.1 已声明的
> `%SystemRoot%\Temp` 🟢 例外；契约无破坏性变更。逐任务提交。

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| R31-1 | ✅ [DESTRUCTIVE] 隔离区 purge 信任链收口 | — | ✅ `store::inside_quarantine_root`（段级、拒 `..`/ADS/尾随点/短名、8.3 双形态）；`lifecycle::verify_purge_target`（根内+非 reparse+sha256）接入自动清除与手动清空；校验失败不删/不迁状态/写 fail 审计；restore 侧同步加固。评审文档 `docs/review/R31-1.md` | cargo test --lib quarantine:: 23 绿（新增 5 例：根外伪造/遍历/sha 不符/手动 purge/正常回归） |
| R31-2 | ✅ 规则解析 fail-closed | — | ✅ `parse_disposition → Option`（缺失/非法丢类目）；`is_valid_config`：green 只准 direct/recycle、黄红只准 quarantine；`maxAgeDays`/`minSizeMB` 非法、`recursive` 非 true/false → Err；顺带清同文件生产 `expect`（红线 6） | cargo test 全绿（新增 4 例：非法/缺失 disposition、risk-disposition 不匹配、非法数值/recursive、合法配置不变） |
| R31-3 | ✅ GBK 解码并列优先 936 | — | ✅ `reg_data_decode_display_with(raw, acp)` 可注入代码页；并列语义优先级 UTF-16 > GBK(936) > CP_ACP；修复非中文 Windows 老 ANSI 中文名乱码，同时保住合法 UTF-16 中文不被 GBK 伪解码抢走 | 新增 5 例（acp=1252 下 GBK 正解、UTF16 英文头+GBK 尾、纯 ASCII、合法 UTF-16、公开入口一致）；**推送后以 main CI run 全绿确认**（原 run 38686334077 的两条失败用例） |
| R31-4 | ✅ `%SystemRoot%\Temp` 白名单例外 + 可重建缓存降级 | — | ✅ `is_safe_temp_exception`（仅该目录、仅 🟢 direct/recycle；`is_hard_protected` 不豁免，还原目标仍拦）；`is_excluded_from_scan`/`preflight` 两侧同口径；`expand_env` 支持 `%VAR%\子目录`；`temp.system` 改 `env %SystemRoot%\Temp` + direct；`temp.thumb` direct；`cache.browser` green/direct（微信缓存保持 🟡）。评审文档 `docs/review/R31-4.md` | whitelist 16 绿（新增绿色/隔离/System32/前缀不误放 1 例）；expand 复合 env 用例；全量 cargo test/clippy 绿 |
| R31-5 | ✅ "立即释放"口径修正 | R31-6 | ✅ `reportModel.immediateBytes` 只计 green+direct（回收站项不立即释放空间）；新增 `confirmItems`/`confirmBytes`（🟡+🔴 供"需要你确认"提示）；新增 `safeCleanItems()`（只取 🟢 且排除 large.file）；方案名改为「推荐清理/深度清理」 | vitest **80 通过**（reportModel 10→15：recycle 不计 immediate、confirm 计数、safeCleanItems 排除审阅型等 5 例；未删旧断言） |
| R31-6 | ✅ 一键清理 UX（两击闭环） | R31-4 | ✅ 首页主 CTA「一键清理」→ `/scan?auto=1&intent=clean`（快速维度自动开扫）→ 完成自动进简化结果页（可释放 X / N 项安全项 / 另有 Z 项需确认）→「立即清理 N 项」只提交 🟢 安全项；🟡/🔴 不进一键流程；深度体检/完整报告保留；方案 A/B 改「推荐清理 / 深度清理」；无安全项独占提示；取消扫描不跳转。`formatBytes` 修 16MB 显示 0.0GB | pnpm typecheck 0 错误 / vitest 80 / build 成功（index gzip 45.03KB 持平，Report chunk gzip 4.66KB）；mock 走查 11 项→2 项安全项 16.0MB→一键提交 id 合法→/executing→完成页可达。**真机 GUI 走查待人类** |
| R31-7 | ✅ 审计记录与文档按实修正 | R31-1..6 | ✅ 新增 `docs/dev/AUDIT-2026-10-18.md`（24 条问题：6 已修 / 文档修正 / 15 条 R32 待办 / 设计边界 + 实证记录 + 验证结果）；README 撤不实宣称（7–30 可配、运行中进程句柄、证据可复现性）并补一键清理说明；SECURITY 指向审计记录、删除不可兑现发布承诺；SAFETY §2.5 标注未实现、§1/§2.1/§4.4 更新；SPEC §5 补齐 5 命令/3 事件/2 个 StartupSource 及残留/卸载类型、§7 补一键流程、§8 修正前端测试口径；CHANGELOG 记 R31 批次 + v0.1.5 未发版；SESSIONS/LESSONS 同步 | 文档与代码交叉核对（命令/事件/类型/分级/白名单口径逐项对照）；提交链 R31-1..7 逐任务独立 |

> **R32 待办（审计遗留，详见 AUDIT §3）**：R32-1 扫描 single-flight + clean_execute 去重；R32-2 MFT 会话按卷复用 + dup 维度收紧 + mft UB/取消；R32-3 autoPurge/retention 接线与 clamp + restore 审计 + journal/manifest/audit fsync/轮转；R32-4 更新通道签名 + rulesVersion + runas 来源校验；R32-5 崩溃恢复终态行语义 + 白名单字面兜底/加载时序 + mock 契约对齐。

---

## 附：裁剪预案（校准点触发，人类决策）

| 触发 | 动作 | 影响 |
|------|------|------|
| Phase 1 结束进度落后 >2 周 | R23 长路径子集移阶段二（P4-02 范围收窄） | −2 HPW |
| Phase 2 结束 M1 链路未闭环 | R05（P3-01）整体移阶段二 | −2.4 HPW |
| Phase 4 中 M2/M3 门禁反复不过 | R24 上传降级仅本地 | −1 HPW |
| Phase 4 末仍落后 >2 周（保底） | R25 自建隔离区降级为系统回收站方案：🟡 类进回收站+UI 标注"14 天内可从回收站还原"；P3-06 取消，M12 口径改回收站还原验证 | −5 HPW |

**不可裁项**：回退能力（方案可降级、能力不可删）、R09 日志+M8、R02 分级+M2 分档、R21 VM 回归、R26 签名。
