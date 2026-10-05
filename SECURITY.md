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

- 内部安全审计（IPC 越权/TOCTOU/文件信任链方向）已完成并修复，修复记录见 [`docs/dev/CHANGELOG.md`](docs/dev/CHANGELOG.md) 与 [`docs/review/`](docs/review/) 各破坏性任务评审文档；
- 破坏面集中在 `cleaner`（清理执行/事务）、`quarantine`（隔离区/还原）、`guard`（进程守卫/单实例）三个模块——欢迎对照 [`docs/review/`](docs/review/) 审阅；
- 公开评审进行中（P4-07），意见处置记录将发布于 `docs/verify/community-review.md`。

## 安全设计要点（供审阅者参考）

- 扫描只读；所有删除/移动操作走两阶段 journal 事务（先日志后动手）；
- 白名单（系统目录/用户文档/运行中进程文件等）多层强制，含路径形态归一（短名/长路径/分隔符/`..`）；
- 🔴 高风险操作需专家模式 + 应用后端签发的一次性确认令牌；
- 更新包 sha256 校验通过才落盘；崩溃转储仅保存在本机。

## English Summary

Found a security issue? Please **do not** open a public issue with details. Use GitHub's private vulnerability reporting (Security → Report a vulnerability) on this repository, or contact the maintainer privately. First response within 7 days. PureSlate collects no telemetry; crash dumps stay on the local machine. The destructive surface lives in the `cleaner`, `quarantine`, and `guard` modules — reviews against `docs/review/` are welcome.
