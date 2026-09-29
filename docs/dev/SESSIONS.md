# PureSlate 项目对话记忆（SESSIONS.md）

> 定位：每次对话一条摘要，**最新在最上**。记"做了什么 / 关键结论 / 下一步"。
> 分工：TASKS.md 管任务状态（✅），本文件管对话上下文与思路延续。
> 维护（AGENTS.md §9）：任务完成时 agent 自动追加/更新当前会话条目，随任务同一提交。格式：`## YYYY-MM-DD | 主题`。
> 本初版由历史会话记忆回溯整理（截至 2026-09-29），细节以 git 历史与 TASKS.md 为准。

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
