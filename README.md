# PureSlate

**Windows 开源（MIT）PC 清理工具。** 只读扫描 → 安全分级（🟢🟡🔴）→ 一键清理（仅 🟢 安全项）/ 人工确认（🟡🔴）→ 分级去向（🟢直清/回收站、🟡🔴隔离区 14 天可还原）→ 全程结构化审计日志。

技术栈：Tauri 2（Rust 内核）+ Vue 3 + TypeScript(strict)，安装包约 2.8MB，零广告/零捆绑/默认零数据外传。

## 下载安装

最新版本：**[v0.2.1](https://github.com/wuhua2026/pureslate/releases/tag/v0.2.1)**（从 [**GitHub Releases**](https://github.com/wuhua2026/pureslate/releases/latest) 始终获取最新版），下载 `PureSlate_0.2.1_x64-setup.exe`：

- 系统要求：Windows 10 / 11（x64）；
- 安装包 sha256 见 Release 页附件与 `update-manifest.json`；
- **签名状态**：开源签名计划（SignPath）申请中，当前版本安装包未签名——SmartScreen 提示"Windows 已保护你的电脑"时，点击"更多信息"→"仍要运行"；
- **管理员权限**：本程序启动时会弹出 UAC 确认（"你要允许此应用对你的设备进行更改吗"）——这是启用 NTFS 直读快速扫描与完整启动项管理所必需，选择"是"即可；

> ⚠️ **唯一官方发布渠道 = 本仓库的 GitHub Releases。** 请勿从其他来源下载或安装任何声称是 PureSlate 的程序。

## 功能

| 维度 | 说明 | 默认去向 |
|------|------|---------|
| 临时文件 | %TEMP%、系统临时目录（%SystemRoot%\Temp）、缩略图缓存 | 🟢 直清 / 回收站 |
| 大文件 | ≥500MB 个人文件**仅定位不清理**（大小/路径/访问时间，可一键在资源管理器中显示），不进任何清理方案 | 审阅型（不清理） |
| 重复文件 | 大小 → 64KB 采样 → 全量 sha256 三级判重，保留最早一份 | 🟡 隔离区 |
| 应用缓存 | 浏览器缓存（Chrome/Edge，🟢 直清）；微信等聊天缓存（🟡 隔离区）；进程运行中自动整类阻止 | 🟢 直清 / 🟡 隔离区 |
| 隐私痕迹 | Edge/Chrome 浏览历史、最近文档 | 🟡 隔离区 |
| 启动项 | 注册表 Run / 启动文件夹 / 登录触发计划任务；禁用 = 备份后移除，可恢复 | — |
| 软件卸载 | 调起**官方卸载器**（不接管删除、不强杀进程），事后扫描安装目录/开始菜单/AppData 残留 | 🟡 隔离区 / 🔴 直删 |

- **一键清理**：首页「一键清理」只扫描并清理 🟢 安全项（可重建的临时文件/缓存），快速释放空间；🟡/🔴 项不会进入一键流程，需要你逐项确认。个人大文件（审阅型）永不进入任何清理方案；
  - **清理范围**：`%TEMP%`、`%SystemRoot%\Temp`、缩略图/图标缓存数据库、浏览器缓存；**不包含**用户目录，也**不包含**数据盘根下叫 `Temp`/`tmp`/`_scratch` 的目录（这些是用户文件区）；
- 🔴 高风险项默认灰禁：需开启专家模式 + 逐字输入应用后端签发的一次性确认令牌；
- **隔离区**：每卷根 `.pureslate-quarantine`（隐藏），默认保留 14 天（保留期配置入口开发中，当前版本固定 14 天）内一键还原，到期自动清除、前 3 天提醒、容量上限；
- **审计日志**：每次清理/还原/清除/启动项变更逐条 JSONL 记录，可查询导出；
- **崩溃安全**：异常时本地落盘 minidump；崩溃中断的清理事务下次启动按安全协议自动恢复。

## 安全设计

- **扫描只读**：扫描过程对目标路径零写入；
- **白名单强制**：系统目录、引导区、用户文档目录、页面文件等永不进入清理候选（含短名/长路径/junction 等形态归一防护，见 `docs/review/` 评审文档）。规则声明的进程守卫会让运行中的应用缓存整类跳过；**系统级"运行中进程句柄快照"白名单尚未实现**（R02 推迟，见 `docs/dev/SAFETY.md` §2.5 注）；
- **两阶段事务**：每个删除/移动先写 journal 再执行，崩溃后孤儿事务按协议恢复；
- **执行前复核**：清理动手前对每个目标重跑白名单 + junction 检查 + 大小/修改时间一致性比对；
- **隔离区信任校验**：到期清除/确认清空硬删前，校验 manifest 路径必须位于隔离区根内、非 junction、sha256 一致；伪造 manifest 不会导致删除根外文件；
- **零遥测**：不收集、不上传任何数据；崩溃转储仅保存在本机（上传功能未接入后端）。

> **质量证据说明**：`docs/verify/` 下的误删率/还原率/性能 JSON 是**一次性人工验证结果**，含统计值与时间戳但未绑定 commit/环境，也**未纳入 CI**，第三方无法直接复现；请把它当作"作者验证记录"而非可复现门禁。已知问题、修复与处置见 [docs/dev/AUDIT-2026-10-18.md](docs/dev/AUDIT-2026-10-18.md)。

## 从源码构建

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发运行（热更）
pnpm tauri build --bundles nsis   # 打包（NSIS 安装包）
pnpm typecheck        # vue-tsc 严格检查
pnpm test             # vitest 前端测试
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

环境要求：Node 22+ / pnpm 11 / Rust stable (MSVC) + VS Build Tools「C++ 桌面开发」。

## 文档

- [`docs/dev/SPEC.md`](docs/dev/SPEC.md) —— 技术规格与 IPC 契约
- [`docs/dev/SAFETY.md`](docs/dev/SAFETY.md) —— 安全约束与破坏性操作规范
- [`docs/dev/TASKS.md`](docs/dev/TASKS.md) —— 任务清单与阶段门禁
- [`docs/dev/CHANGELOG.md`](docs/dev/CHANGELOG.md) —— 变更记录
- [`docs/dev/LESSONS.md`](docs/dev/LESSONS.md) —— 踩坑复利日志
- [`docs/dev/AUDIT-2026-10-18.md`](docs/dev/AUDIT-2026-10-18.md) —— 独立代码审计结论与处置记录（问题 / 修复 / 剩余风险）
- [`docs/review/`](docs/review/) —— 破坏性任务三道关评审文档
- [`docs/verify/`](docs/verify/) —— 一次性人工验证记录（误删率/还原率/性能基准，未纳入 CI）

## 反馈与贡献

- 问题反馈：[GitHub Issues](https://github.com/wuhua2026/pureslate/issues)（附 日志页 导出的审计 JSONL 更佳）；
- 安全漏洞：请勿公开提 issue，见 [SECURITY.md](SECURITY.md) 的披露渠道；
- 欢迎对照 `docs/review/` 的破坏面清单审阅代码——清理工具的信任来自公开可查。

## License

MIT。详见 [LICENSE](LICENSE) 与 [NOTICE](NOTICE)。
