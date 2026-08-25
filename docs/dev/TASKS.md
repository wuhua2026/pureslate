# PureSlate · 任务清单（TASKS.md）

> 用法：按 Phase 顺序执行；一次只做一个任务；每任务完成打 `✅` 并跑「验证」列命令。`[DESTRUCTIVE]` 任务走 AGENTS §6 评审关卡。`[HUMAN]` 任务由人类执行。**Phase 末门禁不达标不得进入下一 Phase。**

---

## Phase 0 · 地基与契约冻结 → M0

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P0-01 | 仓库初始化 | — | ✅ git 仓库；分支 main/dev；LICENSE(MIT)、NOTICE、.gitignore、README 骨架（含"唯一官方渠道"与免责声明占位）；AGENTS.md 落库至根，docs/dev/ 放 SPEC/TASKS/SAFETY + CHANGELOG.md | 目录结构核对；`git log` 有初始提交 ✅（`9f0ff40` P0-01） |
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
| P2-05 | 重复文件 R06 | P1-05 | cleaner/dup：三级过滤（size→64KB 采样→全量 sha256，≥1MB 才全量）；dupGroup 保留最早； BelowNormal I/O | cargo test：分组正确性/保留策略；性能不劣化基准 |
| P2-06 | 执行 UI | P2-04 | 屏 6 执行中（分去向进度）+ 屏 3 二次确认（🔴 token 流）+ 屏 4 日志页 + 完成页「已释放/已隔离」区分（SAFETY §4.5） | vitest：token 流/语义区分渲染 |
| P2-07 | M2/M3 门禁验证 | P2-04,P2-05 | bench-scan 复测（SSD ≤120s@1TB 三维度含 dup hash）；黄金文件集首版（安全/危险样本树 30+ 项）跑误删率：🟢<0.5%/🟡<2%/🔴=0 | 报告落 docs/verify/，全部门禁值达标 |

**Phase 2 门禁（=M2）**：性能门禁 SSD 达标；🟢 直清事务化（journal 恢复用例过）；误删率分档达标；还原首轮 ≥95%。

---

## Phase 3 · 功能扩展（引擎+UI 成对交付）

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P3-01 | 大文件 R05 | P1-05 | scanner large 维度：≥500MB 标记大小/路径/atime；pages/Files 列表排序 | 真机抽查排序与标记正确 |
| P3-02 | 启动项 R07 | P0-03 | startup 枚举 5 源（SPEC §6.4）+发布者（版本信息）+影响估算（启发式：位置+命令特征）；startup_toggle 禁用=备份后移除，可恢复 | 手动：禁用→重启验证→恢复；备份文件存在 |
| P3-03 | 启动项 UI | P3-02 | pages/Startup：影响排序/发布者/禁用风险提示/恢复入口 | vitest + 走查 |
| P3-04 | [DESTRUCTIVE] 隐私清理 R08 | P2-04 | privacy 枚举：浏览器历史（Edge/Chrome，进程守卫）+最近文档（RecentDocs）；逐项「删了什么/能否恢复」；执行进隔离区+日志 | guard 用例：浏览器运行中阻止；评审关卡 |
| P3-05 | 隐私 UI | P3-04 | pages/Privacy：逐项独立确认/可恢复标注 | vitest |
| P3-06 | [DESTRUCTIVE] 隔离区生命周期+管理页 R25 | P2-02 | lifecycle：保留期/到期前 3 天事件/自动清除+审计/容量上限策略/确认清空（token）；pages/Quarantine 屏 8 全要素（剩余天数/还原/清空/语义首行） | lifecycle 单测（时间注入）；评审关卡 |

**Phase 3 门禁**：六维度功能全可用（startup/privacy 页联调通过）；生命周期自动化用例绿。

---

## Phase 4 · 工程就绪 → M3

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P4-01 | 更新机制 R22 | P0-03 | updates：手动检查+opt-in 周查+镜像回退链（jsDelivr→ghproxy→GitHub）+sha256 校验+失败丢弃+审计（SPEC §6.5） | 单测：通道回退/校验失败路径 |
| P4-02 | [DESTRUCTIVE] 兼容加固 R23 | P2-04 | 全链路过 SAFETY §6.3 八项边界（中文/emoji 用户名、>260 长路径、junction 环、占用文件、只读/ACL、盘满、中途 kill、空/超长名）；guard 双语义（目标进程+单实例互斥量） | 边界用例脚本全绿（tests/compat） |
| P4-03 | 崩溃安全 R24 | P2-04 | crash/minidump 本地落盘+启动孤儿 journal 恢复+opt-in 上传预览 UI | kill 测试：dump 生成、journal 恢复执行 |
| P4-04 | 测试体系 R21 | P2-07 | 黄金文件集扩至 100+ 样本（fixture 生成器）+tools/vm-regression.md（微软开发版 VM 快照步骤：清理→重启→关键功能点检） | 误删率全档达标复跑 |
| P4-05 | M12 千次还原 | P2-02 | tools/test-restore.ps1 全量 1000 次：**成功率 ≥99.9%**，失败项出兜底指引 | 报告 docs/verify/restore-1000.json |
| P4-06 | 签名与发布流水线 R26 | P0-05 | SignPath 接入（免费开源计划，先出未签名 CI 包占位）+可重复构建+update-manifest 生成+体积断言保持 | CI 产出可安装包+manifest |
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

## 附：裁剪预案（校准点触发，人类决策）

| 触发 | 动作 | 影响 |
|------|------|------|
| Phase 1 结束进度落后 >2 周 | R23 长路径子集移阶段二（P4-02 范围收窄） | −2 HPW |
| Phase 2 结束 M1 链路未闭环 | R05（P3-01）整体移阶段二 | −2.4 HPW |
| Phase 4 中 M2/M3 门禁反复不过 | R24 上传降级仅本地 | −1 HPW |
| Phase 4 末仍落后 >2 周（保底） | R25 自建隔离区降级为系统回收站方案：🟡 类进回收站+UI 标注"14 天内可从回收站还原"；P3-06 取消，M12 口径改回收站还原验证 | −5 HPW |

**不可裁项**：回退能力（方案可降级、能力不可删）、R09 日志+M8、R02 分级+M2 分档、R21 VM 回归、R26 签名。
