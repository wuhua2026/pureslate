# PureSlate 踩坑复利日志（LESSONS.md）

> 定位：跨会话复利的经验沉淀，四个小节：① 踩坑记录（现象→根因→修复）② 决策记录（选型理由）③ 本机环境结论 ④ 门禁/基准趋势。
> 维护规则（AGENTS.md §9）：任务完成时 agent 自动追加；新条目置于小节**顶部**；只增不删；单条须给出可复现的代码位置/命令，假设读者是下次会话的自己。
> 本初版由历史会话记忆回溯整理（截至 2026-09-29），细节以 git 历史与代码为准。

## ① 踩坑记录

### 白名单防线不应依赖 XML 加载时序——卷根系统文件须常量拦截（2026-09-30，P3-01 真机抽查）
- 现象：P3-01 真机抽查 large 清单榜首是 `C:\pagefile.sys`（21.7GB）——SAFETY §2 明令禁碰的系统关键文件。
- 根因：pagefile.sys 只存在于 whitelist.xml（P1-04），whitelist.rs 常量根无文件级条目；引擎级直接调用（`scan_large_items`）在 XML 未加载时防线失效。
- 修复：[whitelist.rs](../../src-tauri/src/safety/whitelist.rs) `is_whitelisted` 增加卷根关键文件**常量判定**（pagefile/hiberfil/swapfile/dumpstack*，仅卷根一层，子目录同名用户文件不受影响）+ 回归单测。
- 复利结论：①安全关键条目必须在最内层常量固化，外置配置（XML）只做增量；②"真机抽查"验证列不是走过场——沙箱单测造不出 pagefile.sys 这种真实系统形态，第二轮抽查确认榜首变为 6.3GB ArcMap.cache。

### MFT 第四坑 + 并行分片粒度命门（2026-09-29，门禁收尾时发现）
- 现象一：MFT 模式 bench 首次"PASS"（101s）但 temp=0 项——**假胜利**。
- 根因一：`resolve_path` 用裸 `reference == 5` 判根，而 USN 记录的父子引用都是 **(序列号<<48)|记录号**（probe 取证 parent=0x0005_0000_0000_0005），且根记录不在枚举结果里 → 所有路径解析在最后一跳断裂。修复：按**低 48 位记录号**判根 + 递归深度防御 + 带序列号根的回归单测；修复后 temp 22,037 项与 walk 模式同量级。
- 现象二：并行 walkdir 首版（二层分片）112s ≈ 串行 101s——零并行收益。
- 根因二：分片粒度太粗，AppData 巨型子树独占单线程（其余线程围观）。修复：BFS 展开至**第 5 层**分片（`AppData\Local\<app>` 粒度）→ **23.5s（4.8×）**。
- 复利结论：①"加了线程"≠"并行了"——并行优化必须核对负载分布；②假 PASS 的识别信号 = 分项计数异常（temp=0、files=1 时先怀疑测量而非庆祝）。

### MFT/USN 枚举三连坑：卷路径尾斜杠、输出前 8 字节、USN 无 size（2026-09-29）
- 现象一：提权后 `FSCTL_ENUM_USN_DATA` 报 os error 5（卷句柄能打开）。
- 根因一：卷设备路径必须 `\\.\C:`（**无尾反斜杠**）；带尾 `\` 打开的是卷根目录而非卷设备，FSCTL 要求卷设备句柄。probe-fsctl 五种访问掩码组合全部 ENUM_OK，逐项对差定位到唯一差异就是这一个字符。
- 现象二：修好后 try_open 成功但整卷只解出 1 条记录（bench large.files=1、temp 0 项）。
- 根因二：`FSCTL_ENUM_USN_DATA` 输出布局 = **[8 字节 NextFileReferenceNumber][USN 记录...]**——前 8 字节是续批引用号，记录从偏移 +8 起。旧代码从 0 解析，把前 8 字节当"伪记录"（rec_len=631）一步跳飞整批；且用末记录 ref（而非前 8 字节值）续批，一轮即断。修复后整卷 150 万记录 6.3s 全解出（BATCH_SIZES 连续推进）。
- 现象三（架构级事实）：**USN 记录 V2/V3 均无文件长度/mtime 字段**（V3 只是 ReFS 的 128 位引用号变体）——P1-02b 设计假设"V3 有 FileLength"是错的。后果：MFT 直读只能加速**名字匹配类**维度（temp/cache/privacy），large/dup 需要 size 的维度拿不到数据。
- 修复：[mft.rs](../../src-tauri/src/scanner/mft.rs) `enumerate_ffi`（+8 偏移、next_ref 续批、防死循环守卫）；`walk_target` 命中项补 stat（size/mtime）；单测改真实临时文件沙箱（stat 语义）。
- 复利结论：FFI 输出缓冲区的"前导元数据"与"续批协议"必须逐字节取证（探针 hex dump 是唯一可信依据），"我以为的结构体布局"不算数；性能数字要动手量（6.3s vs walkdir 101s 冷/55s 暖）。

### 白名单对正斜杠路径失明——红线级绕过（2026-09-29，黄金集扩测发现）
- 现象：样本树扩测后 tempItems=17（= temp 子树全部文件，含两个白名单守卫）；修复前旧测试"守卫未被扫"的通过是**假阴性**（测试比较键与引擎产物分隔符不一致，永远查不到）。
- 根因：`is_whitelisted` 对候选路径只做小写化、未做 `/`→`\` 归一；规则 XML 以正斜杠声明 target 时，walkdir 产出混合分隔符路径，段级前缀匹配按 `\` 切段 → 整体失配 → 白名单（含常量根 C:\Windows 等）被整体绕过。
- 修复：[whitelist.rs](../../src-tauri/src/safety/whitelist.rs) `is_whitelisted` 候选侧改走 `normalize_lower`（两侧同规）；补回归单测 `forward_slash_target_paths_do_not_bypass_whitelist`；`golden-misdelete-test` 加 `norm_key` 键归一。
- 复利结论：①跨层路径比较必须双侧同规（分隔符+大小写），"看起来差不多"不够；②测试自身的比较逻辑也要被检验——对拍真实产物键，否则门禁是空转。

### Win32 GetTokenInformation：TOKEN_ELEVATION 缓冲区必须恰好 4 字节（2026-09-29）
- 现象：管理员会话中 `is_elevated_ffi()` 恒返回 false，MFT 直读永远不启用，bench 只能走慢速 walkdir。
- 根因：令牌信息类误用 `TOKEN_ELEVATION_CLASS`(22) 且传 8 字节缓冲区 → `ERROR_BAD_LENGTH`(24)。`TOKEN_ELEVATION`(20) 只需要 4 字节 DWORD。
- 修复：[mft.rs](../../src-tauri/src/scanner/mft.rs) `is_elevated_ffi()` 改用 `TOKEN_ELEVATION` + `u32` 缓冲区。
- 验证：探针 `probe-mft` 输出 `ELEV len=4 ok=1 elev=1`，`INTEGRITY=12288`（高完整性）。
- 复利结论：`GetTokenInformation` 各 class 缓冲区长度要求严格，不确定时先传小缓冲区看 `ret_len`；成功调用后 `GetLastError` 可能残留旧值（如 6），判定以返回值为准。

### PowerShell 5.1 跑 .ps1：emoji 必炸 + 中文必须带 BOM（2026-08-26）
- 现象：`tools/*.ps1` 含 🟢🟡🔴 emoji 直接解析错误；无 BOM 的 UTF-8 中文注释乱码并引发语法错误。
- 修复：脚本内不用 emoji（用文字"绿/黄/红"）；所有含中文的 .ps1 写入时带 UTF-8 BOM（`0xEF,0xBB,0xBF`）。

### Start-Process -Verb RunAs 提权进程不继承环境变量（2026-08-26）
- 现象：提权 runner 里读不到父进程设置的 `PURESLATE_REPO`。
- 修复：提权脚本内用字面路径写死 `$repo`；输出写回 repo 下临时文件而非依赖控制台回传。
- 复用模式：管理员排障探针 = ASCII runner 脚本 + 直接运行已编译二进制（避免提权窗口内重建）+ 输出落盘文件。runner 统一放 `tools/diag/`，输出 `*-out.txt` 已 gitignore。

### Tauri async command 的返回类型约束（2026-08-25）
- 现象：`scan_start` 返回复杂类型时编译/运行报错。
- 根因：Tauri 异步命令 + `&State` 参数时返回类型受限。
- 修复：`scan_start` 返回 `Result<String, String>`（会话 id），复杂数据经 `scan_get_items` 拉取（[commands.rs](../../src-tauri/src/commands.rs)）。

### loader.rs 把 whitelist.xml 误当 ruleset 解析（2026-08-25）
- 现象：`load_dir` 遍历规则目录时把 `whitelist.xml` 也按规则集加载。
- 修复：增加 `is_ruleset_xml` 检查，仅加载根元素为 `ruleset` 的文件（[loader.rs](../../src-tauri/src/rules/loader.rs)）。

### ScanDimension 缺 Hash derive 导致 cargo check 失败（2026-08-25）
- 现象：作为 HashMap key 使用时编译错误。
- 修复：`ScanDimension` 补 `#[derive(Hash)]`（contract.rs）。

## ② 决策记录

### 方案审计结论与对策（2026-09-29）
- 结论：设计 A（安全闭环与工程纪律超配），落地 B+（单机验证、性能攻坚中）。依据 SPEC / SAFETY / TASKS 全文对照。
- 对策（按优先级）：① 性能攻坚设止损线——MFT 若持续受阻，Plan B = walkdir 多线程并行化（不裁门禁）；② 黄金集样本树扩至 36 项并预置极端用例（中文/emoji/空格命名、只读、0 字节、长路径、junction 不跟随）——本次已落地；③ 规则包治理（版本号 + 一键回滚）须在 R22 上线前定义（待办）；④ kill 进程 → 重启 → journal 对账复验用例待补（待办）；⑤ 小白用户隔离区语义困惑率实验已预置于 P5-03。

### 扫描引擎双路径：walkdir 兜底 + MFT 直读加速（2026-08-25）
- 背景：DG-1 门禁全维度 ≤120s，walkdir 逐目录 + 逐文件 stat 实测 188.75s 不达标。
- 决策：管理员 + NTFS 时走 `FSCTL_ENUM_USN_DATA` 流式读 MFT 记录、内存拼路径；任一前置不满足自动回退 walkdir。统一入口 `MftSession::walk_target`，幂等于 `walk::walk_target`（白名单/include/exclude/min_size/取消/进度语义一致）。
- 理由：MFT 直读只读不写（扫描红线天然满足）；MFT 记录不跟随 reparse point，行为比目录遍历更安全。

### MFT 模块零第三方依赖（2026-08-25）
- 直接 FFI 声明所需 Win32 API（OpenProcessToken/CreateFileW/DeviceIoControl 等），不引入 windows-rs。
- 理由：依赖准入红线（AGENTS §4.7）——包体 <20MB；所需 API 面小，FFI 成本可控。

### 无效/缺失 risk 默认 Red（2026-08-25，P1-04）
- SAFETY §1 一致性：规则缺失/非法 risk 一律按 🔴 处理（宁误报不漏报），同时记 `undetermined_count` 供观察。

### IPC 契约冻结（M0，2026-08-25）
- `src/types/ipc.ts`（TS 唯一事实源）与 `contract.rs`（serde 镜像）两端同步改；变更记 CHANGELOG；新增字段必须 optional。详见 AGENTS §8。

## ③ 本机环境结论

- 开发机用户 `17599`，Windows，系统盘 C: 为 NTFS。
- Shell 为 Windows PowerShell 5.1（非 pwsh）：编码两坑见 §①。
- 提权排障既定模式：`tools/diag/` 下 ASCII runner 脚本（`.probe-elevate.ps1` / `.fsctl-elevate.ps1` / `.bench-elevate.ps1`）+ 输出落盘 `*-out.txt`（已 gitignore）。
- 探针工具：`src-tauri/src/bin/probe-mft.rs`（提权/完整性/MFT 打开诊断）、`probe-fsctl.rs`（卷打开方式矩阵测试）。
- 本地裸 debug exe 8.36MB；安装包体积门禁由 CI 产物断言 <20MB（WiX 下载受限，不走本地打包验证）。

## ④ 门禁/基准趋势

| 日期 | 指标 | 数值 | 门禁 | 状态 |
|---|---|---|---|---|
| 2026-08-25 | M0 契约冻结 + 包体基线 | exe 8.36MB | <20MB | ✅ |
| 2026-08-26 | 黄金集误删率（绿/黄/红） | 0 / 0 / 0 | <0.5% / <2% / =0 | ✅ |
| 2026-08-26 | 隔离区还原率 | 100%（100 次循环） | 100% | ✅ |
| 2026-08-26 | bench-scan 全维度（walkdir 模式） | 188.75s | ≤120s | ❌ 待 MFT 通路 |
| 2026-09-29 | MFT 提权判定 | elev=1，INTEGRITY=12288 | 管理员可用 | ✅ 已修复 |
| 进行中 | MFT USN 枚举 | os error 5（ACCESS_DENIED） | — | ❌ 排查中（probe-fsctl） |
| 2026-09-29 | 黄金集扩测复验（36 项 + 极端用例） | 绿/黄/红 = 0/0/0；junction 零跟随；极端命名/只读/0 字节零漏扫 | <0.5% / <2% / =0 | ✅ 守卫检查真实生效（修复白名单绕过后） |
| 2026-09-29 | MFT USN 枚举（三连坑修复后） | 整卷 1,503,710 记录（112.9 万文件）/ **6.3s** | — | ✅ 引擎可用（walkdir 对照：冷 101s / 暖 55s，仅见 87 万文件） |
| 2026-09-29 | 并行 walk 分片粒度（二层 → 五层） | 112s → **23.5s**（874,854 文件，冷缓存） | — | ✅ 4.8× 加速（二层分片伪并行教训见 §①） |
| 2026-09-29 | **bench 门禁终测（MFT + 五层分片并行 walk）** | **totalMs = 46.5s**（MFT 12.6 + temp 10.3 + walk 23.5 + 桶 0） | ≤120s | ✅ **PASS**（余量 61%；dup sha256 哈希未含，偏差见 TASKS P2-07 备注） |
