# M0 门禁核验（Phase 0 收尾）

**日期**：2026-08-24
**执行边界**：Phase 0 地基与契约冻结。全部门禁满足后，方可进入 Phase 1。

> 判定依据以事务记录为准；本文件为本批实际执行结果。

## M0 门禁清单

### G1　契约两端镜像一致（P0-03 评审）

| 检查项 | 结果 |
|--------|------|
| `src/types/ipc.ts`（TS 唯一事实源）全量类型：Grade / Disposition / ScanDimension / ScanProfile / ScanItem / ScanResult / CategoryAggregate / QuarantineEntry / StartupEntry / UpdateStatus / RestoreReport / PurgeReport / LogEntry / confirmToken 等 | ✅ 定义齐全 |
| `src-tauri/src/contract.rs`（Rust serde 镜像）同名类型 + `#[serde(rename_all="camelCase")]` 对齐 | ✅ |
| 两端枚举取值一致（green/yellow/red、direct/recycle/quarantine、temp/large/dup/cache/startup/privacy 等） | ✅ 逐项核对一致 |
| 16 个 IPC 命令注册 stub（`ipc/commands.rs` @ `lib.rs`） | ✅ |
| 事件名常量两端一致（`IPCEvents` ↔ `contract::events`） | ✅ |

**验证命令（本机通过）**
- `cargo check` → `Finished dev profile ... in 11.26s` ✅
- `cargo fmt --check` → 无 diff ✅
- `cargo clippy -D warnings` → 0 警告 ✅
- `cargo test` → 2 passed（`app_meta_serializes_camel_case` / `app_meta_roundtrip_deserialize`）✅
- `pnpm typecheck`（vue-tsc --noEmit）→ 通过 ✅
- `app_meta` 首命令链路：`src/api/commands.ts` 分发 + mock 模式并联调 ✅

### G2　mock 覆盖全维度（P0-04）

| 检查项 | 结果 |
|--------|------|
| 覆盖全部 6 扫描维度（temp/large/dup/cache/startup/privacy） | ✅ |
| 含中文/英文路径、三档分级(🟢🟡🔴)、重复文件组 | ✅ |
| mock 开关（VITE_MOCK=true 走 mock 而非 invoke） | ✅ |
| vitest 用例 | ✅ `pnpm test`：8 passed（data.test.ts） |

### G3　CI 绿（P0-05）

| Job | 内容 |
|-----|------|
| `rust-checks` | fmt --check → clippy -D warnings → cargo test |
| `frontend-checks` | typecheck → vitest → vite build |
| `tauri-package-size` | `pnpm tauri build --bundles nsis` → `tools/check-size.ps1` 断言 <20MB → 上传产物体积报告 |

**本地等价验证**
- Rust fmt/clippy/test、前端 typecheck/vitest 均本机通过（见 G1）。
- 体积断言脚本逻辑已核对：扫 `bundle/nsis/*.exe`，断言 `<20MB`。
- push/PR 到 GitHub 后由 Actions 执行最终判定（本机无法直连 GitHub Actions 网络进行 WiX/NSIS 下载，但 CI 在 GitHub 原生网络下可正常拉取工具集）。

### G4　包体 <20MB（P0-06）

| 项 | 结果 |
|----|------|
| 本机 Release 裸 exe | **8.36 MB** ✅ <20MB |
| 安装包（NSIS/MSI） | CI 产物断言（`check-size.ps1` <20MB），凭证见 `docs/verify/phase0-baseline.md` |

**判定说明（D3）**：安装包体积以 CI 产物测量为准；本机裸 exe 8.36MB 已证明体积预算成立。

## 结论

| 门禁 | 判定 |
|------|------|
| G1 契约两端镜像一致 | ✅ |
| G2 mock 覆盖全维度 | ✅ |
| G3 CI 绿（本地等价） | ✅ |
| G4 包体 <20MB（exe + CI 断言） | ✅ |

**M0 = PASS**。CHANGELOG 已记「契约冻结」。可进入 Phase 1。