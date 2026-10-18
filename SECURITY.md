# Security Policy（安全政策）

> 中文说明（英文摘要见下）：如果你发现 PureSlate 的安全漏洞，请**不要**在公开 issue 中描述细节。

## 报告漏洞

1. 优先使用 GitHub 的**私密漏洞报告**（仓库页 → Security → Report a vulnerability）；
2. 或通过 [GitHub Issues](https://github.com/wuhua2026/pureslate/issues) 私下联系维护者（先只说明"存在安全影响"，细节走私密渠道）。

请在报告中包含：复现步骤、影响范围、涉及版本（Release 页可查）。我们会在 **7 天内**首次响应。

## 支持的版本

| 版本 | 支持状态 |
|------|---------|
| latest release（见 [Releases](https://github.com/wuhua2026/pureslate/releases)） | ✅ 支持安全修复 |
| 更早版本 | ❌ 请升级 |

## 审计与评审状态

- **2026-10-18 独立代码审计**（3 路静态分析 + 实测复核）：结论、已修复项与剩余风险见 [`docs/dev/AUDIT-2026-10-18.md`](docs/dev/AUDIT-2026-10-18.md)。其中隔离区 purge 信任链、规则 `disposition` fail-open、非中文 Windows 解码、系统临时目录白名单例外等已在 R31 批次修复；
- 内部安全审计（IPC 越权/TOCTOU/文件信任链方向）已完成并修复，修复记录见 [`docs/dev/CHANGELOG.md`](docs/dev/CHANGELOG.md) 与 [`docs/review/`](docs/review/) 各破坏性任务评审文档；
- 破坏面集中在 `cleaner`（清理执行/事务）、`quarantine`（隔离区/还原/清除）、`guard`（进程守卫/单实例）三个模块——欢迎对照 [`docs/review/`](docs/review/) 审阅。

## 安全设计要点（供审阅者参考）

- 扫描只读；所有删除/移动操作走两阶段 journal 事务（先日志后动手）；
- 白名单（系统目录/用户文档/页面文件等）多层强制，含路径形态归一（短名/长路径/分隔符/`..`）；**规则进程守卫**会在应用运行中整类跳过对应缓存；系统级"运行中进程句柄快照"白名单**尚未实现**（R02 推迟，见 SAFETY §2.5 注）；
- 隔离区清除前校验 manifest：路径必须位于隔离区根内、非 junction/symlink、sha256 一致（伪造 manifest 不会删除根外文件）；
- 🔴 高风险操作需专家模式 + 应用后端签发的一次性确认令牌；
- 更新包 sha256 校验通过才落盘；崩溃转储仅保存在本机。

## English Summary

Found a security issue? Please **do not** open a public issue with details. Use GitHub's private vulnerability reporting (Security → Report a vulnerability) on this repository, or contact the maintainer privately. First response within 7 days. PureSlate collects no telemetry; crash dumps stay on the local machine. The destructive surface lives in the `cleaner`, `quarantine`, and `guard` modules — reviews against `docs/review/` are welcome.
