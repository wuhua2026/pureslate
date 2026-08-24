# Phase 0 体积与启动基准（P0-06）

**日期**：2026-08-24
**环境**：Windows 11 / x64 / MSVC toolchain（rustc 1.98.0）/ Node v22.16 / pnpm 11.17
**目标**：SPEC §1 体积预算「安装包 <20MB」的首次真实验证，作为 M3 门禁基线来源。

## 1. 体积实测（本机）

| 项 | 值 | 门禁 | 结论 |
|----|-----|------|------|
| release 裸可执行文件 `src-tauri/target/release/pureslate.exe` | **8.36 MB** | <20MB | ✅ PASS |
| release 编译 | `cargo build --release`，2m54s | — | ✅ |

> 说明：本机 Release 可执行文件体积 8.36MB，已显著低于 20MB 预算。

## 2. 安装包体积（降级为 CI 产物断言）

**状态说明（D3 降级路径）**：本机 MSI/NSIS 安装包打包所需的 WiX/NSIS 工具集从 `github.com` 下载，受当前网络限制（镜像下载速度 <2KB/s，39MB 预计 7 小时+）无法本机完成。按 Phase 0 计划 §P0-06 与 D3 决策：

- **本机**：以裸可执行文件 8.36MB 作为本环节序与体积预算的初步实测（远低于 <20MB 门禁）。
- **CI**：安装包（NSIS）体积由 `.github/workflows/ci.yml` 的 `tauri-package-size` job 生成，并用 `tools/check-size.ps1` 断言 `<20MB`，报告写入 `docs/verify/ci-size-report.json`。**安装包体积 ≥ 裸 exe，CI 绿且 <20MB 即代表 SPEC §1 预算成立。**

**验收口径**：本机门禁（契约一致 / CI 配置全绿 / mock 全维度 / exe 8.36MB <20MB）全部满足；「安装包 <20MB」以 CI 产物断言为准，CI 绿即可判定通过。

## 3. 启动基准（记录，不作为 M0 门禁）

- 本机 `pnpm tauri dev`：Vite 就绪 ~1.0s，cargo dev 编译 19.5s，`pureslate.exe` 成功拉起并显示主窗口（MainWindowTitle = `PureSlate`）。
- Release 冷启动耗时未本机计时（GUI 窗口环境）；后续 M3 前由基准脚本补充。

## 4. 数据留存

- 体积断言脚本：`tools/check-size.ps1`
- CI 体积报告终态：`docs/verify/ci-size-report.json`（由 CI 生成）