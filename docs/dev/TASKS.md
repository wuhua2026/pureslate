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
| P1-01 | 规则引擎 R01：模型+加载 | P0-03 | rules/model+loader：解析 SPEC §4.1 XML；非法 category 丢弃+日志；规则缓存（mtime 失效）；至少 2 个真实 ruleset：system-temp(green)、cache.wechat(yellow 含 guard) | cargo test：合法/非法/覆盖加载用例 |
| P1-02 | 规则引擎 R01：遍历+匹配 | P1-01 | scanner/walk：walkdir 遍历 target、白名单过滤（SAFETY §2）、glob 匹配、取消令牌、进度回调；reparse point 不跟随 | cargo test：临时目录沙盒匹配/排除/取消用例 |
| P1-03 | **DG-1 性能决策门** | P1-02 | tools/bench-scan：真机 1TB SSD 三维度计时 → `docs/verify/bench-scan.json`。**≤120s → 记录通过；>120s → 在本表插入 P1-02b（MFT 直读）并执行，完成后再判** | 基准报告落盘 + 结论记 CHANGELOG |
| P1-02b | （条件）MFT 直读枚举 | P1-03 | scanner/mft：管理员权限声明+降级回退 walkdir；MFT 枚举替代遍历供 target 匹配 | bench 复测达标；无管理员时自动回退 |
| P1-04 | 安全分级 R02 | P1-01 | safety/grade+whitelist：按规则 risk 判定；无法判定默认 red；白名单常量+whitelist.xml 加载 | cargo test：三档/默认红/白名单拦截用例 |
| P1-05 | 扫描编排 R03 | P1-02,P1-04 | scan 编排：profile→各维度执行→聚合→事件推送（scan_progress 节流/scan_done）；**全程零写盘**（审计验证：对扫描目标目录做前后快照对比） | 集成测试 + 快照对比脚本绿 |
| P1-06 | 外壳 R16 | P0-04 | pages/Home：导航/磁盘占用条/一键体检入口/快速建议区；无托盘无推广；设计 token 落 CSS | vitest + 手动走查屏 1 要素齐 |
| P1-07 | 报告页 R13/R14 | P0-04 | pages/Report（mock）：总览可释放量/风险分布/逐项可解释（reason）/去向预告/A·B 选项对比；🔴 折叠灰禁；GradeBadge | vitest：分级渲染+灰禁交互 |
| P1-08 | 真数据联调 | P1-05,P1-07 | 报告页接真扫描（mock 开关切换）；扫描中页（屏 5：进度/当前路径/实时累计/取消） | 真机走查：体检→报告→取消全流程 |
| P1-09 | [HUMAN] 数据采集 | P0-04 | 本机+同学机 5–10 台（SSD/HDD）：各盘容量/分类占用/大文件分布/扫描耗时 → `docs/verify/data-baseline.md`（提供采集脚本模板） | data-baseline.md 落库 |

**Phase 1 门禁（=M1）**：真机只读链路全通；扫描零写盘（快照对比）；DG-1 有结论；报告页 8 屏要素中屏 1/2/5/7 完成。

---

## Phase 2 · 信任底座与清理闭环 → M2

> 顺序即信任链：隔离区→日志→清理。**P2-01..03 完成前不得开始 P2-04。**

| ID | 任务 | 依赖 | 产出 / DoD | 验证 |
|----|------|------|-----------|------|
| P2-01 | [DESTRUCTIVE] 隔离区核心 R25 | P1-05 | quarantine/store+manifest：同盘 move、manifest 行、冲突路径、跨盘降级 copy+hash 校验（SAFETY §4.2）；目录 hidden+system | cargo test：移入/manifest/跨盘/占用用例；评审关卡文档 |
| P2-02 | [DESTRUCTIVE] 还原引擎 R25 | P2-01 | quarantine/restore：按 manifest 还原、冲突兜底目录、state 迁移；**还原率脚本 tools/test-restore.ps1（先 100 次循环）≥95%** | 脚本报告 ≥95%（M12 千次版在 P4-05） |
| P2-03 | 审计日志 R09 | P2-01 | logging/audit：JSONL（SPEC §4.4）按天滚动；clean/restore/purge 覆盖 100%；log_query/log_export 命令 | 集成测试：每操作必有对应日志行 |
| P2-04 | [DESTRUCTIVE] 清理执行 R04 | P2-02,P2-03 | cleaner/execute+journal+recycle：🟢 direct+recycle 双路径；两阶段 journal（SAFETY §3）；单类可取消；跨类并行≤2；🔴 项无 token 拒绝 | cargo test：journal 恢复/中断回滚/边界用例（SAFETY §6.3 的 1–7）；评审关卡 |
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
