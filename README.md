# PureSlate

Windows 开源（MIT）PC 清理工具。只读扫描 → 安全分级（🟢🟡🔴）→ 人工确认 → 分级去向（🟢直清/回收站、🟡🔴隔离区 14 天可还原）→ 全程结构化日志。

- 技术栈：Tauri 2（Rust 内核）+ Vue 3 + TypeScript(strict) + Vite + Pinia
- 体积预算：安装包 **<20MB**，零广告/零捆绑/默认零数据外传

## 开发命令

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 开发运行（热更）
pnpm tauri build      # 打包
pnpm typecheck        # vue-tsc 严格检查
pnpm test             # vitest 前端测试
cd src-tauri
cargo fmt && cargo clippy -- -D warnings
cargo test            # Rust 单测
```

环境要求：Node 20+ / pnpm / Rust stable(MSVC) + VS Build Tools「C++ 桌面开发」。

## 唯一官方发布渠道

本项目的**唯一官方发布渠道为本仓库的 GitHub Releases**。请勿从其他来源下载或安装任何声称是 PureSlate 的程序或补丁。

> 免责声明：PureSlate 是独立开源项目，与 Dism++、联想及联想电脑管家**无任何关联、不支持、不背书**。使用本工具清理文件存在不可逆风险，请在使用前仔细核对所选项目，并善用隔离区还原能力。

## 文档

- `AGENTS.md` —— AI 编码 agent 工作约定
- `docs/dev/SPEC.md` —— 技术规格与 IPC 契约
- `docs/dev/TASKS.md` —— 任务清单与 Phase 门禁
- `docs/dev/SAFETY.md` —— 安全约束与破坏性操作规范

## License

MIT。详见 [LICENSE](LICENSE) 与 [NOTICE](NOTICE)。