# PureSlate · Agent 开发约定（AGENTS.md）

> 本文件是 AI 编码 agent 在本仓库工作的唯一入口约定。战略背景文档（PRD / Dev Plan）**本地保留、不入公开仓库**（2026-10-04 决策），不参与日常开发，**本 docs 包即唯一事实源**；若与任何其他文档冲突，以本包为准。

## 0. 项目是什么（30 秒版）

PureSlate：Windows 开源（MIT）PC 清理工具。Tauri 2（Rust 内核 + Vue3/TS 前端），安装包 <20MB，零广告/零捆绑/默认零数据外传。

核心范式（一句话）：**只读扫描 → 安全分级（🟢🟡🔴）→ 人工确认 → 分级去向（🟢直清/回收站、🟡🔴隔离区 14 天可还原）→ 全程结构化日志**。

不做的事：杀毒、系统加速/注册表优化噱头、任何形式的广告推荐、账号体系、跨端。（完整 Non-goals 见 SPEC §10）

## 1. 文档读取顺序（强制）

1. 本文件（每次会话开始）
2. `docs/dev/SESSIONS.md` **最近 1-2 条** —— 接续上次对话的进度与思路（每次会话开始；禁止通读全文）
3. `docs/dev/SPEC.md` —— 架构、模块、全部数据结构与 IPC 契约
4. `docs/dev/TASKS.md` —— 定位当前 Phase 当前任务，**只精读该任务及其直接依赖**
5. 任务标注 `[DESTRUCTIVE]` 或涉及删除/移动/注册表/进程操作时，**必读 `docs/dev/SAFETY.md`**
6. `docs/dev/LESSONS.md` —— 踩坑/决策/环境结论/门禁趋势；**按需检索相关条目，禁止通读**

纪律：禁止一次性通读全部文档；禁止读取后续 Phase 的任务细节（避免上下文污染与超前设计）。

## 2. 工作流（每个任务）

1. 从 TASKS.md 取当前任务 → 读 SPEC 对应章节；
2. 实现 → 跑任务列出的**全部验证命令**，必须全绿；
3. 更新 TASKS.md：任务行打 `✅`，实际偏差写入任务行的 `备注`；
4. 更新记忆文档（见 §9）：SESSIONS.md 顶部追加/更新本次对话条目；出现新踩坑/新决策/新基准数值时写入 LESSONS.md 对应小节；
5. 按 commit 规范提交（见 §6）；
6. `[DESTRUCTIVE]` 任务额外走 SAFETY.md §5 的评审关卡；
7. 一个任务一次提交，不跨任务混提。

## 3. 常用命令

```bash
pnpm install                # 前端依赖
pnpm tauri dev              # 开发运行（热更）
pnpm tauri build            # 打包（产物体积须 <20MB）
pnpm typecheck              # vue-tsc 严格检查
pnpm test                   # vitest
cd src-tauri
cargo fmt && cargo clippy -- -D warnings
cargo test                  # 单测；性能基准见 SPEC §8
```

## 4. 红线（违反任一条 = 任务失败，无论功能是否正确）

1. **扫描只读**：扫描路径下不得有任何删除/移动/写入用户数据的操作；
2. **先日志后动手**：每个破坏性操作必须先写事务日志（SPEC §6.2 journal）再执行，且可回滚或可追溯；
3. **白名单不可碰**：SAFETY.md §2 列出的路径/文件类型不得进入任何删除候选；
4. **零遥测**：不得引入任何统计上报/埋点；联网仅限 R22 更新检查（opt-in）与 R24 崩溃上传（opt-in 预览后确认）；
5. **净室**：不得复制 Dism++/联想电脑管家的代码、清理参数、资源文件、品牌词；规则 XML 全部自研；
6. **Rust 生产路径禁止 `unwrap()`/`expect()`/`panic!()`**（测试代码除外）；错误用 thiserror 类型传播；
7. **依赖准入**：新增任何 crate/npm 包前自查——是否使包体接近 20MB、是否引入重量运行时；有疑虑先在 TASKS.md 备注"依赖决策"再引入；
8. **🔴 默认灰禁**：任何 🔴 级操作的 UI 入口默认禁用，须专家模式 + 二次确认 token（SPEC §5）。

## 5. 编码约定

- **Rust**：模块结构见 SPEC §2；错误统一 thiserror；优先 `pub(crate)`；`unsafe` 必须附一行理由注释；路径处理一律 UTF-16 安全（`std::path::PathBuf`，禁用 String 拼路径）；
- **TS**：strict 模式；`<script setup lang="ts">` 组合式 API；**IPC 类型一律从 `src/types/ipc.ts` 导入**（契约唯一事实源，禁止组件内重复定义）；
- **样式**：原生 CSS + CSS variables（设计 token 见 SPEC §9），不引入组件库；
- **命名**：Rust snake_case；TS camelCase；IPC 命令 snake_case；常量 SCREAMING_SNAKE；
- **commit**：`<task-id>: <一句话摘要>`，例 `P1-02: walkdir 遍历引擎+取消+进度事件`。

## 6. 三道关评审（仅 [DESTRUCTIVE] 任务）

实现完成后，生成 `docs/review/<task-id>.md`，内容三节：

1. **改动清单**：新增/修改的文件与函数；
2. **破坏面清单**：本次改动新增了哪些"能删文件/移动文件/改注册表/杀进程"的代码路径（逐条列出）；
3. **自测证据**：验证命令输出摘要 + 边界用例清单（SAFETY.md §5.2 规定的必测边界）。

人类会用**无上下文的新会话**对照该文档评审代码。写代码时假设有第二双眼睛专挑你的错。

## 7. 里程碑与门禁

M0 契约冻结 → M1 只读 Demo → M2 清理闭环+性能门禁 → M3 功能全+工程就绪 → M4 内测通过 → M5 发布。

门禁数值见 TASKS.md 各 Phase 末尾的「Phase 门禁」节。**门禁不达标不得进入下一 Phase**；被门禁卡住时，先在 TASKS.md 记录差距与优化方案，不得裁掉门禁本身。

## 8. 契约冻结纪律

`src/types/ipc.ts` 与 `src-tauri/src/contract.rs` 是 IPC 契约的两端镜像，M0 后视为冻结。任何修改必须：
1. 同步两端；
2. 在 `docs/dev/CHANGELOG.md` 记录变更；
3. 不破坏既有命令签名（新增字段须 optional）。

## 9. 记忆文档（SESSIONS / LESSONS）

- `docs/dev/SESSIONS.md`：项目对话记忆，每次对话一条摘要（最新在最上）：做了什么 / 关键结论 / 下一步。任务完成时更新（§2 步骤 4），随任务同一提交入库。
- `docs/dev/LESSONS.md`：踩坑复利日志，四小节——① 踩坑记录（现象→根因→修复）② 决策记录（选型理由）③ 本机环境结论 ④ 门禁/基准趋势。新条目置于小节顶部，只增不删。
- 两文件由 agent 在任务完成时自动维护；人类评审时可对照检查是否漏记。
