# PureSlate 项目对话记忆（SESSIONS.md）

> 定位：每次对话一条摘要，**最新在最上**。记"做了什么 / 关键结论 / 下一步"。
> 分工：TASKS.md 管任务状态（✅），本文件管对话上下文与思路延续。
> 维护（AGENTS.md §9）：任务完成时 agent 自动追加/更新当前会话条目，随任务同一提交。格式：`## YYYY-MM-DD | 主题`。
> 本初版由历史会话记忆回溯整理（截至 2026-09-29），细节以 git 历史与 TASKS.md 为准。

## 2026-10-04 | P4-06 签名与发布流水线交付 + 安全审计 I 组四项收口

- **做了什么**：①`.github/workflows/release.yml`（tag `v*` 触发）：双构建一致性（`cargo clean -p pureslate` 重建后 exe sha256 比对）→ 体积门禁 → `tools/publish-manifest.ps1`（RulesPack 规则包[排除 whitelist.xml] + update-manifest 六字段）→ SignPath 占位 step（secrets 四项配置后启用）→ gh release 上传 → manifest 回推 main；②**I-1** `log_export` 收紧：`audit::export_path_for`（目录成分剥离+`[A-Za-z0-9._-]` 白名单+固定 `<data_root>\exports\`+已存在改时间戳防覆盖，同毫秒碰撞循环 5 次）；③**I-2** token 后端签发：契约加性 `confirm_token_issue`（PS-XXXX-XXXX 由 uuid v4 字节映射，仅专家模式可签），`AppState::consume_pending_confirm` 取出即失效（未命中同样作废防暴力重试），clean_execute/quarantine_purge 接入，前端 `generateToken` 移除改 `issueToken`，模态框异步签发（签发失败展示+禁输入）；④**I-3** 严格 CSP 启用（default/script-src 'self'+style/img/font 补充，connect-src ipc:）；⑤**I-4** tauri-plugin-opener 依赖与 capabilities 权限声明移除；⑥审计清单 7 条状态列更新（I-1/I-2/I-4/I-5/T-4/T-6 已收口，I-3 部分处置）。
- **关键结论**：①**本地 NSIS 打包不可行**（tauri 首次打包下载 NSIS 工具链走 GitHub 超时，与 P0-06 WiX 同因；CI windows-latest 无此问题，发布流水线端到端待真实仓库 tag）——用占位安装包跑通脚本全流程自测（manifest 六字段/sha256 双向一致/规则包无 BOM/whitelist 排除）；②"剥离优于拒绝"的导出收口设计：webview 传任意路径只取末段文件名，`..` 因 file_name()=None 自然落拒绝分支；③一次性令牌"未命中也作废"语义（take 先于比对）防暴力重试，代价是输错须重新签发（UX 可接受）；④uuid v4 字节映射字母表替代引入 rand crate（依赖准入红线）。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（169 passed，+2：export_path 拒绝与防覆盖/consume_pending_confirm 一次性）；pnpm typecheck/vitest(54)[generateToken 用例移除]/build 全绿；脚本端到端自测全对。
- **下一步**：P4-07 [HUMAN] 社区公开评审（发布破坏面五模块代码+review 文档到 V2EX/Rust 社区）。**M3 门禁工程侧全绿，仅差社区评审**。人类决策待办：SignPath 申请、真实仓库定稿（REPO_SLUG/identifier）、崩溃上传后端、CSP 真机走查、历次积压真机走查。

## 2026-10-04 | P4-05 M12 千次还原交付（1000/1000 = 100% ≥ 99.9%）

- **做了什么**：`restore-self-test` 升级——①JSON 报告（stdout 落 `docs/verify/restore-1000.json`：buildProfile/threshold/ratio/gatePass/durationMs/stageCounts/failures），stderr 人读进度（每 100 轮）与结论；②**失败项按五阶段兜底指引**（write/hash/quarantine[manifest·journal 残留可还原]/restore[restore-conflict 可取回]/verify[最严重，导出审计+manifest 提 issue]），failures 上限 50 条防报告膨胀（全量计数在 stageCounts）；③阈值参数化（M2=0.95 默认 / M12=0.999），ps1 增 `-MinRatio`（InvariantCulture 传参，防 zh-CN 之外 locale 逗号小数点破坏参数）。
- **关键结论**：①**M12 门禁 PASS：1000/1000 = 100% ≥ 99.9%（release，19.3s，零失败）**——P2-02 还原引擎（同盘 rename/跨盘 copy_verify_delete/mtime 回写/state 迁移）在千次规模无劣化，T-2 四重校验零误拒；②沙箱自测对"冲突兜底"路径覆盖不到（样本原路径必空闲，Conflict 分支由 restore 单测覆盖）——千次口径主要验证主路径稳定性，已在报告 stageCounts 空=无失败佐证。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（167 passed）；M12 evidence `docs/verify/restore-1000.json`（gatePass=true）；冒烟 100 次 debug 2.3s。
- **下一步**：P4-06 签名与发布流水线 R26（SignPath 占位包+可重复构建+update-manifest 生成+体积断言；顺带定稿 REPO_SLUG 与崩溃上传端点、处置审计 I-1/I-2/I-3/I-4）。遗留：P3-04/P3-06/P4-02 评审关卡人类正式评审仍欠。

## 2026-10-04 | P4-04 测试体系 R21 交付（黄金集 132 样本两阶段门禁 + T-4 白名单加固收口 + VM 回归手册）

- **做了什么**：①`golden-misdelete-test` 全量重写——fixture 生成器声明式五族 132 样本（safe 48 极端命名 8 / temp 43 含只读 4·零字节 2·长路径深层 / dup 21 十组判重 / red 13），三档 target 内守卫使误删率分母非平凡（黄/红各 1）；测试升级两阶段：扫描期（分档误删率+防漏扫+junction 零跟随）+ **清理期**（绿/direct 真实 `resolve_targets→execute` 全链路，数据根注入沙箱，盘面核对 must_keep 缺失=真误删、只读/长路径按 Win32 语义优雅失败留盘、journal 零孤儿）；②**安全审计 T-4 收口**：白名单段级归一（双侧共用：`..`/`.` 段解析、逐段剥尾随点/空格）+ XML 根 8.3 短名 `GetLongPathNameW` 展开，golden 四形态守卫进门禁全 protected；③`tools/vm-regression.md`（微软开发版 VM 快照步骤：10 步清理场景+9 项重启点检+记录模板）。
- **关键结论**：①**T-4 定级"低（声明健壮性）"**——候选侧由 walkdir 长名产物保证，攻击面在白名单声明侧；加固只收紧不放宽（四种书写形式从"保护静默丢失"变"命中"），匹配逻辑变更按 SAFETY §2 纪律出评审关卡 `docs/review/P4-04.md`；②**GetShortPathNameW 返回值坑**：NULL/0 尺寸查询惯用法（GetLongPathNameW 同族）在此不可靠，且短名必短于长名——"足量缓冲单次调用+与原串比较（相同=无短形态）"才稳；守卫样本必须先创建后查询短名；③whitelist 单测共享 XML_ROOTS 的并行竞态（旧用例靠运气绿）→ 全局测试锁（LESSONS ① 同型第 2 例）；④黄/红隔离区去向在 golden 中不可注入（`quarantine_root_of` 真实卷根）→ 仅扫描期度量，边界已在头注释声明；⑤只读文件 DeleteFileW 必失败（ACCESS_DENIED）、>260 字符路径 DeleteFileW 失败而 std fs::操作内部处理长路径——两条 Win32 硬边界成为 golden 的确定性断言。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（167 passed，whitelist +4）；golden 复跑 pass=true（132 样本，rates 0/0/0，guardsSwept=[]，T-4 四形态 protected，clean executed=43/ok=38/fail=5 预期内/orphans=0）；pnpm typecheck/vitest(55)/build 全绿；证据 `docs/verify/golden-misdelete.json`。
- **下一步**：P4-05 M12 千次还原（`tools/test-restore.ps1` 全量 1000 次 ≥99.9%，报告 docs/verify/restore-1000.json）。遗留：I-1/I-2（审计"前小修"）仍未归属任务，建议随 P4-06 发布前处理。

## 2026-10-04 | P4-03 崩溃安全 R24 交付（minidump + T-6 孤儿恢复 + opt-in 上传预览）

- **做了什么**：`crash/{mod,dump,parse,recover}` 四模块——dump：SEH 顶级过滤器 + panic hook 双通道，dbghelp `MiniDumpWriteDump` `#[link]` 直调（零依赖），落 `<data_root>\crash\` 保留 5 份；recover：启动孤儿 journal 恢复（T-6 协议先行定稿：journal 只定位 manifest 记录绝不按 journal 路径动手，quarantine 孤儿经 restore_one T-2 四重校验还原，「已移入未登记」崩溃窗口按 `<sha8>_<原名>` 约定反查**唯一**未登记候选补登记（🟡/crash.recovered，不自动还原），direct/recycle 不可逆只标记 orphan+审计；恢复期 `clean_execute` 拒绝新事务）；parse：minidump 头/目录/模块流/异常流只读解析（checked 偏移，畸形返回 Err）；四命令 `crash_list/preview/upload/recovery` 加性契约 + `http_post` + Settings 崩溃卡片 + Home 恢复横幅；`bin/crash-sim` + `tests/crash_safety.rs` 4 例（kill/SEH/panic/dump）。
- **关键结论**：①**本机环境 `MiniDumpWriteDump` 异常流路径不可用（带异常参数恒 998 ERROR_NOACCESS，故障线程直调与干净辅助线程+自有快照均拒）**——工程决策：dump 保证落盘（无异常流兜底成功），异常代码由过滤器深拷贝 EXCEPTION_RECORD 首 u32 写 sidecar `<dump>.json` 携带，`crash_preview` 解析合并；②kill 测试设计：crash-sim 子进程逐文件 intent→移入（写 manifest）→停 300ms→result，父进程轮询 marker 出现即 kill，得到确定性的「intent+manifest 已写、result 未写」窗口形态；③集成测试进程无 lib 的 `TEST_DATA_ROOT_LOCK`（#[cfg(test)]），须本文件私有锁；子进程数据根经新增 `PURESLATE_DATA_ROOT` 环境变量注入（等价 LOCALAPPDATA 信任面）；④clippy 新版规则：`&[x.clone()]`→`std::slice::from_ref`、RawHandle 已是 `*mut c_void` 无需转换、`chunks_exact(2)`/`%2`→`as_chunks`/`is_multiple_of`。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（140 lib + 24 集成，crash_safety 4 例全过：kill 中断→manifest 背书孤儿还原+journal 闭环+审计 recover）；pnpm typecheck/vitest(55)/build 全绿；评审关卡 `docs/review/P4-03.md`。真机人工：Settings 崩溃卡片预览/上传、Home 恢复横幅待走查。
- **下一步**：P4-04 测试体系 R21（黄金集扩至 100+ 样本 + fixture 生成器 + tools/vm-regression.md；顺带处置安全审计 T-4 白名单旁路验证样本）。遗留：I-1/I-2（审计清单「P4 前小修」）仍未归属任务。

## 2026-10-03 | P4-02 兼容加固 R23 交付（安全审计五项修复 + 八项边界）

- **做了什么**：T-1 `cleaner/preflight`（执行前白名单重跑+逐级 reparse+size/mtime 比对，`CleanTarget.mtime_ms` 贯通，apply_one 接线）；T-2 `restore_one` 四重校验（根内/sha256/白名单禁区/父链）；T-3 `ensure_quarantine_root` reparse 拒绝；F-1 `resolve_targets`+clean_execute 规则失败整体拒绝（原 fail-open 已修）；F-2 审计+`ScanResult.whitelistOk` 加性契约+Report 降级横幅；`guard/instance` 单实例互斥量+激活首实例；whitelist `\\?\` 前缀剥离+`quarantine_root_of` 长路径盘符解析；`tests/compat.rs` 12 例八项边界。
- **关键结论**：①fixture 反斜杠双写惯例会破坏 `\\?\` 扩展前缀（Win32 不归一化扩展路径）——XML 属性反斜杠本无须转义，旧惯例靠普通路径归一化侥幸工作；②junction 创建用 `mklink /J`（免特权），std 把 junction 映射为 is_symlink；③测试锁要抗中毒（unwrap_or_else into_inner），否则首败级联废全部用例；④盘满边界以 IO 失败注入等价覆盖。
- **验证**：cargo fmt/clippy/test 全绿（127 lib+12 compat+8 集成）；preflight 5+instance 2 新单测；typecheck/vitest(47)/build 全绿；评审关卡 `docs/review/P4-02.md`。真机人工：双开单实例、坏 whitelist 横幅。
- **下一步**：P4-03 崩溃安全 R24（minidump 落盘+孤儿 journal 恢复+kill 测试；T-6 恢复协议防 confused deputy 并入）。

## 2026-10-01 | P4-01 更新机制 R22 交付（Phase 4 开工）

- **做了什么**：`updates/{mod,http}`：WinHTTP 直调 HTTPS（零依赖）+ 通道回退链（jsDelivr→ghproxy→GitHub raw，mirrorFirst 反转）+ manifest 解析/分段数值版本比较（非数值段保守判无更新）+ 规则包 sha256 校验（失败丢弃+审计，通过才落盘到 `<data_root>\rules`）+ opt-in 7 天周查（last-check.json + 启动钩子）+ `update_check` stub→业务 + Settings.vue（更新开关/手动检查/专家模式，路由 /settings Placeholder 退场）。loader 增 `load_dirs` 叠加（用户目录覆盖资源目录；白名单只随资源目录），scan/clean_execute 全部切到叠加链。
- **关键结论**：①WinHTTP 层不单测（无网络沙箱），回退/校验/周查门控全在注入式纯函数（fetch/download 闭包注入）；②规则包是远端内容——文件名白名单校验（仅 [A-Za-z0-9._-]）防路径穿越 + "校验全通过才落盘"；③契约零改动（update_check 增 AppHandle/State 为 Tauri 注入参数，IPC 不变）；④REPO_SLUG 为占位常量，P4-06 定稿。
- **验证**：cargo fmt/clippy/test 全绿（120 lib+8 集成，updates 11 + loader 叠加 1 新增）；typecheck/vitest(47)/build 全绿。真机手动：Settings"立即检查"应优雅失败（repo 未发布）。
- **下一步**：P4-02 [DESTRUCTIVE] 兼容加固 R23（SAFETY §6.3 八项边界 + guard 单实例 + 安全审计 T-1/T-2/T-3/F-1/F-2 预置项，任务备注已列）。

## 2026-09-30 | P3-06 隔离区生命周期+管理页交付（Phase 3 收官）

- **做了什么**：`quarantine/lifecycle`（到期自动清除+审计 auto_purge、3 天 `quarantine_expiry_warning`、restored 行 30 天清理[restoredAt 加性字段]、容量 min(5GB,盘剩余10%) 超限不自动删+最早批次建议）；`quarantine_purge` stub→业务（token 拒绝）；新增 `quarantine_status` 命令（契约加性，双端同步+CHANGELOG+SPEC §5）；启动钩子跑生命周期；`Quarantine.vue` 屏 8 全要素 + `quarantineModel.ts`；mock 有状态化（list 过滤/restore/purge 翻转）。
- **关键结论**：①时间+剩余空间双注入使 lifecycle 单测完全不依赖真实时钟/磁盘；②自动清除的可追溯面=manifest purged 行恒在+审计（journal 等价物，评审文档 §2 论证）；③`purge_across(roots,...)` roots 可注入——全局函数遍历真实固定盘，沙箱测试必须走可注入核心（与 P2-01 store 同教训）；④测试坑：seed 双写同 id manifest 行导致断言错乱（Quarantined+Restored 同 id 并存）。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（109 lib+8 集成，lifecycle 6）；pnpm typecheck/vitest(47)/build 全绿；评审关卡 `docs/review/P3-06.md`。走查（还原/清空 token/到期横幅）待人类。
- **下一步**：Phase 3 任务全部 ✅；门禁自查（startup/privacy 页联调+生命周期自动化=已绿）后可进 Phase 4。遗留：P3-04/P3-06 两个 [DESTRUCTIVE] 评审关卡的人类正式评审、三批真机走查（启动项禁用→重启→恢复、Edge 运行中阻止、隔离区清空 token 流）。

## 2026-09-30 | P3-05 隐私 UI 交付

- **做了什么**：`pages/privacyModel.ts`（纯函数：privacy 条目过滤/类目分组/守卫提示/可恢复标注/选中合计）+ `pages/Privacy.vue`（类目分组卡片、逐项 checkbox 默认全不勾、底部操作条→clean_execute→/executing）；路由 `/privacy` Placeholder→真组件；mock 对齐：privacy 种子 🔴→🟡、categoryId 与 privacy-traces.xml 一致（补 Chrome 样本）、startup.registry 种子改 🔴（SAFETY §1 注册表归红档+保住三档断言与 token 演示流）、`defaultProfile` 纳入 privacy 维度。
- **关键结论**：①"逐项独立确认"落地为默认全不勾的 checkbox（与报告页批量默认勾选区分）；②mock 启动项种子语义过时（R07 已独立成页不走扫描维度）但保留作 🔴 演示样本，reason 注明引导去 /startup 页；③契约零改动、Rust 零改动。
- **验证**：pnpm typecheck/vitest(42)/build 全绿（privacyModel 7 用例）。走查（勾选→清理→隔离区）待人类 `pnpm tauri dev` 执行。
- **下一步**：P3-06 [DESTRUCTIVE] 隔离区生命周期+管理页 R25（lifecycle 时间注入单测+Quarantine.vue 屏 8+评审关卡）。另：P3-04 评审关卡的正式人类评审（对照 docs/review/P3-04.md）尚未做。

## 2026-09-30 | P3-04 隐私清理 R08 交付（规则驱动，[DESTRUCTIVE] 评审关卡）

- **做了什么**：`resources/rules/privacy-traces.xml` 三类目（edge-history/chrome-history/recent-docs，全 🟡/quarantine，浏览器带进程守卫）+ `expand.rs` knownFolder 增 3 个源映射；隐私项完全走既有 rules→engine→clean_execute→journal→隔离区→审计管线，零新破坏性代码路径、契约零改动。集成测试 4 例：端到端扫描（中文/emoji/空格/0字节/非递归排除+零写盘快照）、守卫正例（测试进程自身 exe 作运行中浏览器→整类 skip 文件原样）、隔离区 roundtrip（沙箱根+manifest 类目）、出厂规则包加载回归。
- **关键结论**：①`**/History*` 连 WAL 伴随文件一并移入（防残留 WAL 恢复历史）；②分级决策 🟡 而非 mock 的 🔴（SAFETY §1 表格：单应用+可逆；🔴灰禁会让功能不可用，mock 对齐留 P3-05）；③守卫正例测试技巧=用测试进程自身 exe 基名当"运行中进程"（确定命中）；④集成测试进程与 lib 单测进程分离，DATA_ROOT_OVERRIDE 只需文件内串行。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（103+4+4）；评审关卡文档 `docs/review/P3-04.md`（含 §6.3 边界对照：1/4/8 本任务复测，2/3/5/6/7 继承 P2-04）。真机黑盒（Edge 运行中阻止）待人类。
- **下一步**：P3-05 隐私 UI（Privacy.vue：逐项独立确认/可恢复标注；顺带把 mock privacy 分级 🔴→🟡 对齐）。

## 2026-09-30 | P3-03 启动项 UI 交付

- **做了什么**：`pages/startupModel.ts`（纯函数：影响降序/启用禁用分组/来源·影响·发布者文案/禁用风险文案/HKLM 失败文案）+ `pages/Startup.vue`（摘要计数、影响三色档表格、未知发布者标黄、禁用轻量风险确认面板、已禁用区恢复入口、toggle 失败提示）；路由 `/startup` Placeholder→真组件；mock 增强（stub 补 hklm 高影响/计划任务/已禁用样本，startup_toggle 有状态翻转，data.test 断言放宽）。
- **关键结论**：①禁用确认用轻量面板而非 token 流（token 留给 🔴 清理/quarantine_purge 语义）；②mock toggle 翻转 stub 状态使 dev 走查能真实看到禁用→恢复循环；③契约零改动、Rust 零改动。
- **验证**：pnpm typecheck/vitest(36)/build 全绿（startupModel 7 用例）。走查（禁用→恢复流）待人类 `pnpm tauri dev` 执行。
- **下一步**：P3-04 [DESTRUCTIVE] 隐私清理 R08（浏览器历史+RecentDocs，进程守卫，评审关卡）。

## 2026-09-30 | P3-02 启动项 R07 交付

- **做了什么**：新建 `src-tauri/src/startup/`（winreg=advapi32 直调、regfile=regedit v5 .reg 生成/解析、backup=startup-backup manifest、tasks=Tasks\*.xml+quick-xml+schtasks、version=version.dll CompanyName、impact=纯函数启发式）；`startup_list/startup_toggle` stub→业务；lib.rs 挂载。禁用流=备份先行（.reg/文件移入/manifest）→移除原项→审计 disable_startup；启用=按备份还原+审计 enable_startup。
- **关键结论**：①任务 XML 无启用状态（COM 运行态才存），未由本应用禁用的任务按启用显示，自家禁用走 manifest；②HKLM 写值需管理员而应用 asInvoker→HKLM 项 toggle 返回 false+审计 fail（契约 bool 所限）；③测试坑：锁守卫存结构体字段才护满测试体（LESSONS ①）；④契约零改动（WOW6432Node 归入 hklm_run）。
- **验证**：cargo fmt/clippy -D warnings/test 全绿（103+4，startup 新增 22）；typecheck/vitest(29) 全绿；沙箱覆盖注册表/文件夹 roundtrip+备份存在 DoD。真机手动项（禁用→重启验证→恢复）待人类执行。
- **下一步**：P3-03 启动项 UI（Startup.vue：影响排序/发布者/禁用风险提示/恢复入口，路由 /startup 仍指向 Placeholder）。

## 2026-09-30 | 安全审计（方案 A）：引入 cloudflare/security-audit-skill 对照，产出 IPC/TOCTOU 缺口清单

- **做了什么**：评估 cloudflare/security-audit-skill（MIT，AI agent 安全审计技能包）对本项目的适用性 → 结论"方法论可借鉴、不解决误删主线"；随后按方案 A 以其 DESKTOP-MOBILE-AND-LOCAL-IPC 攻击类别为对照框架，审计两端契约、命令层、清理执行、隔离区/还原、journal、白名单与 Tauri 配置，产出 `docs/verify/security-audit-ipc-toctou.md`（13 条缺口 + 正面确认 + P4 归属映射）。
- **关键结论**：①新增主要发现——T-1 执行期无最终路径复核（提权+junction 中间目录替换，高）、T-2 还原信任可写 manifest 且不校验 sha256（提权任意文件移动，高）、F-1/F-2 规则/白名单加载失败 fail-open（中）、I-1 log_export 任意路径 truncate 写入（中）、I-2 🔴 token 仅查非空（中）；②严重度前提为"应用以管理员运行"（DG-1），asInvoker 形态下 T-1/T-2 降为中；③单实例（P4-02）、journal 恢复（P4-03）为已计划项，清单补充了安全语义要求（数据目录独占锁、恢复协议防 confused deputy）；④T-4 白名单字符串旁路（短名/尾随点/`..`）待黄金集验证定级。
- **下一步**：处置按清单 §6 顺序（T-2/T-3 → T-1 → F-1/F-2 → I-1/I-2）；已把 T-1/T-2/T-3/F-1/F-2 的修复要求与验证口径预置为 TASKS.md P4-02 任务备注（清单总览表同步）；P4-03 备注并入 T-6，T-4 待黄金集验证。

## 2026-09-30 | P3-01 大文件维度交付：真机抽查再抓一个红线缺口

- **做了什么**：①`engine::scan_large_items` 接线 `parallel_walk_stats`（large 维度 ≥500MB，标记大小/路径/atime，🟡/quarantine per SAFETY §1"下载目录大文件"）；②pages/Files 列表页（filesModel 纯函数 + 大小降序 + GradeBadge）；③mock seed large 从 green/recycle 对齐为 yellow/quarantine；④真机抽查两轮。
- **关键结论**：首轮真机抽查抓出 **pagefile.sys（21.7GB）漏进 large 候选**——白名单 XML 加载时序缺口，卷根系统文件已改常量拦截（含回归单测），复测 13 项干净、榜首 6.3GB 排序正确。全链路绿：cargo 81 测试 + clippy 零警告，pnpm typecheck/vitest(29)/build。
- **下一步**：P3-02 启动项 R07（枚举 5 源 + toggle 备份恢复）；遗留备忘：large 遍历无中间进度事件、dup 生产引擎复用并行遍历。

## 2026-09-29 | 素材归集：竞品研究资料本地化（非开发任务）

- **做了什么**：经用户拍板（本地参考+git 隔离 / 完整范围），将外部工作区研究素材复制到 `docs/research/`：联想电脑管家 Launcher 逆向 case（20 文件）+ Dism++ 架构拆解 1 份，共 21 文件约 121KB，`diff -r` 逐字节校验一致；`.gitignore` 追加 `docs/research/`；生成 `MANIFEST.md`（含 SHA-256 清单与使用边界）。素材原位置完整保留，未做移动/删除。
- **关键结论**：研究素材与实现隔离，符合红线 5 净室要求；其中竞品原始采集物已标注「仅作架构对照，严禁参考其 UI 文案/品牌词/清理参数」。
- **下一步**：不影响 P2-07 主线（FSCTL 卡点照旧）。本次产生的 `.gitignore` 与 SESSIONS.md 改动待与下次任务同批提交。

## 2026-09-29 | P2-07 收口：M2 门禁全绿（bench 46.5s ≤120s）；连修 6 个深层 Bug

- **做了什么**：①审计结论记入 LESSONS §②；②黄金集扩测至 36 项+极端用例——抓出并修复红线级白名单正斜杠绕过；③MFT 权限问题闭环（四连坑：卷路径尾斜杠 / 输出前 8 字节续批引用号 / USN 无 size / 根引用带序列号致路径解析全断）；④用户拍板 Plan B：`walk::parallel_walk_stats` 五层分片并行遍历（std::thread 零依赖）；⑤bench 计时口径修正（MFT 枚举计入 totalMs、large/dup 共用一次遍历、杜绝假 PASS）。
- **关键结论**：**bench 门禁 totalMs=46.5s ≤120s PASS（余量 61%）**：MFT 枚举 12.6s + temp 直读 10.3s（22,037 项真实产出）+ 并行 walk 23.5s（二层分片伪并行 112s → 五层 23.5s，4.8×）。对照最初 walkdir 基线 188.75s = **4.06× 总提速**。M2 四项门禁全绿：性能 ✅ / 误删率 0-0-0 ✅ / 还原率 100% ✅ / 直清事务化（P2-04）✅。偏差：DoD"含 dup hash"未纳入 bench（量到 size 预分组；余量 73.5s 大概率可容，P3-01 后可复测完整口径）。
- **下一步**：P2-07 已 ✅（TASKS 备注含偏差记录）→ **Phase 2 收官，可进 Phase 3**（P3-01 大文件 large 维度接线 `parallel_walk_stats` + dup 生产引擎复用同一遍历）。

## 2026-08-26 | P2-05/P2-06 完成，P2-07 启动即遇 MFT 卡点

- **做了什么**：P2-05 重复文件检测（size 分组 → 64KB 采样 hash → 全量 sha256 三级过滤，保留 mtime 最早者）接线 engine；P2-06 执行 UI（Executing 页 / ConfirmTokenModal / Log 页 + clean store，严格区分"已释放/已隔离"语义）。P2-07 门禁验证启动。
- **关键结论**：M2 门禁三项中误删率、还原率可过；性能必须靠 MFT 直读。`is_admin()` 恒 false 是首个拦路虎。
- **下一步**：修提权判定 → 打通 MFT → bench 复测。

## 2026-08-25 | Phase 1 主线扫清（P1-02b..P1-08）

- **做了什么**：P1-02b MFT 直读初版（FSCTL_ENUM_USN_DATA + 管理员判定 + walkdir 回退，零第三方依赖）；P1-04 安全分级（grade.rs + whitelist.xml，无效 risk 默认 Red）；P1-05 扫描编排（engine/expand/aggregate/state/commands，200ms 节流进度事件，零写入快照对比验证，提交 930deaa）；P1-06 外壳（220px 侧边栏 9 路由 + Home 磁盘占用条）；P1-07（提交 d141a0e）；P1-08 真数据联调（scan store、Scan 进度页、Report 对接，提交 3d261c8）。
- **关键结论**：契约冻结纪律生效；MFT 首版留有提权判定隐患（后证实为 bug，2026-09-29 修复）；loader 白名单误解析、Tauri async 返回类型两个坑当场修掉（详见 LESSONS §①）。
- **下一步**：P1-09 数据采集（用户决定：先本机全固定盘自测）→ Phase 2。

## 2026-08-25 | Phase 0 收口，M0 契约冻结

- **做了什么**：仓库初始化（9f0ff40）→ Tauri 脚手架 → IPC 契约两端镜像（ipc.ts / contract.rs）→ mock 服务（6 扫描维度，vitest 8 过）→ CI 骨架（fmt/clippy/test/typecheck/vitest/build + 体积断言）→ 包体基线 8.36MB（提交 6678a66）。
- **关键结论**：契约冻结，`src/types/ipc.ts` 为 TS 唯一事实源。两个坑：ScanDimension 缺 Hash derive；WiX 下载受限改为 CI 断言安装包体积。
- **下一步**：进 Phase 1 只读扫描链路（M1）。
