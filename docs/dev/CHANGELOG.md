# Changelog

> 记录契约与规格变更（AGENTS §8）。变更需双端同步 + 记此日志。

## [Unreleased]

### Phase 0（M0 契约冻结）

- 建立仓库骨架：AGENTS.md 落库至根，SPEC/TASKS/SAFETY 落库至 `docs/dev/`；
- 建立 `src/types/ipc.ts`（TS 契约唯一事实源）与 `src-tauri/src/contract.rs`（serde 镜像）；
- 全部 IPC 命令注册为 stub，`app_meta` 作为首个打通命令；
- **契约冻结**：M0 起，IPC 契约双端视为冻结，任何修改须双端同步 + 记本日志 + 不破坏既有签名（新增字段须 optional）。