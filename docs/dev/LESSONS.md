# PureSlate 踩坑复利日志（LESSONS.md）

> 定位：跨会话复利的经验沉淀，四个小节：① 踩坑记录（现象→根因→修复）② 决策记录（选型理由）③ 本机环境结论 ④ 门禁/基准趋势。
> 维护规则（AGENTS.md §9）：任务完成时 agent 自动追加；新条目置于小节**顶部**；只增不删；单条须给出可复现的代码位置/命令，假设读者是下次会话的自己。
> 本初版由历史会话记忆回溯整理（截至 2026-09-29），细节以 git 历史与代码为准。

## ① 踩坑记录

### 本地 NSIS 打包：tauri 首次打包下载工具链走 GitHub 超时（2026-10-04，P4-06）
- 现象：本地 `pnpm tauri build --bundles nsis` 在 "Verifying NSIS package" 处下载 `nsis-3.11.zip` 超时（`timeout: global`），与 P0-06 的 WiX 下载受限同因（网络环境）。
- 边界澄清：CI windows-latest 上同一命令一直绿（NSIS 下载无碍）——**"本地不能打包"≠"流水线不能打包"**，安装包产物验证以 CI 为准（LESSONS §③ 既有结论的 NSIS 版）。
- 对策：本地脚本自测用占位安装包（按命名约定放入 bundle/nsis/ 后跑 publish-manifest.ps1，验证清单字段/sha256 逻辑/产物格式），跑完清理；真实端到端等 CI tag 触发。
- 复利结论：凡"构建期下载外部工具链"的步骤，本地开发环境（网络受限）与 CI（通畅）能力不同，设计脚本时留占位自测路径。

### MiniDumpWriteDump 异常流路径在本机环境不可用（恒 998）——异常代码走 sidecar（2026-10-04，P4-03）
- 现象：SEH 顶级过滤器里带 `MINIDUMP_EXCEPTION_INFORMATION` 调 `MiniDumpWriteDump` 稳定失败，`GetLastError=2147943398`（= HRESULT 0x800703E6 → Win32 **998 ERROR_NOACCESS**）；换成干净辅助线程 + 静态堆快照（EXCEPTION_RECORD/CONTEXT 深拷贝，排除"OS 指针在故障线程栈上"的地址空间语义）**仍然 998**。不带异常参数（`exception_param=NULL`）则成功（panic 通道 56KB dump 可解析）。
- 结论：本机（Win32 build 26300）dbghelp 的异常流写入路径不可用，与调用线程/指针位置无关；不要赌 dbghelp 版本行为。
- 修复：dump 保证落盘（异常参数失败 → 无异常流兜底重写一次）；异常代码在过滤器内从深拷贝的 EXCEPTION_RECORD 首 u32（ExceptionCode，MSVC/x64 布局首字段稳定）取出，写 sidecar `<dump>.json`（`{"exceptionCode":..., "ts":...}`），`crash_preview` 解析 dump 后合并 sidecar；`prune_old_dumps` 连 sidecar 一起清。
- 复利结论：①崩溃路径上的 FFI 失败必须打 GetLastError（本次靠它锁定 998）；②对 OS 级行为做三层降级设计（带异常流 → 无异常流 → 放弃），每层都要真实可测——SEH 真实崩溃测试（volatile 空指针写触发硬件 AV，勿用 `std::process::abort`，fastfail 绕过 SEH）是验证唯一手段；③`GetLastError` 返回值 ≥0x80070000 时减 0x80070000 得 Win32 码。

### cargo 集成测试进程看不到 lib 的 #[cfg(test)] 项——共享锁须本文件自建（2026-10-04，P4-03）
- 现象：`tests/crash_safety.rs` 引用 `pureslate_lib::storage::TEST_DATA_ROOT_LOCK` 编译错 cannot find value。
- 根因：`#[cfg(test)]` 只在编译 lib 自身单测目标时存在；集成测试是独立 crate，看到的 lib 没有 cfg(test) 项（compat.rs 早就用了本文件私有锁，口径一致）。
- 复利结论：集成测试需要"串行化共享全局"时，在测试文件内 `static TEST_LOCK: Mutex<()> = Mutex::new(())` 自建；子进程数据根注入用 `PURESLATE_DATA_ROOT` 环境变量（storage::data_root 第三优先级）。

### 测试锁守卫在构造函数末尾即释放——共享全局依旧被并行污染（2026-09-30，P3-02）
- 现象：startup 三个沙箱测试随机失败（备份文件"不存在"、manifest 多出别人的记录）。
- 根因：`Sandbox::new()` 里 `let _g = LOCK.lock()` 的守卫在**函数返回时即 drop**，锁只护住了 setup 阶段；测试体执行期间其他并行测试照样换掉共享 `DATA_ROOT_OVERRIDE`。
- 修复：把 `MutexGuard<'static, ()>` 存进 Sandbox 结构体字段（`_guard`），随沙箱一起活到测试结束；Drop 里先清 override 再释放锁。
- 复利结论：`TEST_DATA_ROOT_LOCK` 的语义是"持锁整个测试体"，任何只想借一下 data_root 沙箱的新测试都必须用结构体持有守卫，不能在辅助函数里 lock。

### 任务 XML 不含启用状态——计划任务 enabled 只能靠自家 manifest（2026-09-30，P3-02）
- 现象：`%SystemRoot%\System32\Tasks\*.xml` 全文无 Enabled 字段，schtasks 文本输出又本地化（中英系统字段名不同），无 locale 无依赖手段拿到任务启停状态。
- 对策：枚举只认 LogonTrigger（quick-xml 本地名匹配）；未经本应用禁用的任务一律按 enabled=true 展示，本应用禁用项由 startup-backup manifest 标记 false（`startup/tasks.rs` 头注释）。
- 复利结论：Windows 存储双轨制（XML 定义 vs COM 运行态）是常态；避 COM 又要 locale 无关时，先确认目标属性到底存在哪一轨，别假设导出 XML 是全量。

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

## ① 踩坑记录

### GetShortPathNameW / GetLongPathNameW：NULL/0 查询惯用法不可靠 + 必须先有文件再查询（2026-10-04，P4-04/P4-06）
- 现象：golden 短名守卫 `shortNameAvailable` 恒 false，但 FSO 探针证明本卷 8.3 是启用的（`PURESL~1.TXT` 存在）。
- 根因一：照搬 GetLongPathNameW 的 two-call 惯用法（NULL 缓冲 + 0 取所需尺寸）对 GetShortPathNameW 行为微妙不可靠；且我写的启发式 `need <= wide.len()` 方向反了——**短名必然短于长名**，该条件恰好把成功情形拒掉。
- 修复：分配足量缓冲单次调用，返回串**与原串比较**——相同 = 无短形态（该 API 对无短名的路径原样返回），不同 = 短形态。根因二：查询发生在守卫文件创建之前（查询不存在的路径恒失败）——先创建再查询。
- CI 实锤（P4-06）：同族的 **GetLongPathNameW** 用 NULL/0 two-call 在本地碰巧成功、CI 上返回 0 → 8.3 展开静默失败 → t4_short 测试挂——同一坑同族 API 再踩一次后彻底改为固定足量缓冲单次调用。
- 复利结论：①Win32 "query size then call" 两段式惯用法不是普适模式，逐 API 核实语义；②"返回串与输入比较"是判有无短形态的唯一稳法；③FSO（Scripting.FileSystemObject）的 ShortPath 是免管理员验证 8.3 是否启用的探针（fsutil 8dot3name query 需要提权）。

### 共享全局的单测并行竞态会潜伏到扰动增加才爆发（2026-10-04，P4-04）
- 现象：whitelist 单测在新增 T-4 用例后随机失败（3 例，单线程全绿）。
- 根因：多个用例共享全局 `XML_ROOTS` 却各自 set/clear，旧用例集时序侥幸不撞；新增用例改变并发分布后暴露。与 LESSONS ①「测试锁守卫」（2026-09-30）同型第 2 例——那次是 DATA_ROOT_OVERRIDE，这次是白名单根。
- 修复：`XML_ROOTS_LOCK` 全局测试锁，凡触碰 set_xml_roots 的用例持锁整个测试体。
- 复利结论：新增触碰既有共享全局的测试时，先给该全局补锁，别赌时序；"单线程全绿 + 并行随机挂"就是共享全局竞态的指纹。

## ② 决策记录

### 安全审计 T-4 定级"低（声明健壮性）"并以匹配加固收口（2026-10-04，P4-04）
- 定级依据：白名单候选侧来自 walkdir 产物（长名、无 `..`/尾随点），攻击者无法注入旁路形态候选；缺口在**声明侧**——白名单/规则包作者写出短名、尾随点、`..` 段时旧归一化导致保护静默丢失（同族第 3 例，前有正斜杠绕过）。可达性 = 需控制声明内容，故"低"。
- 处置：①匹配逻辑加固（段级归一双侧共用 + 8.3 展开，**只收紧不放宽**，逐条论证见评审文档 §2）；②golden 四形态守卫进门禁——后续规则包/白名单更新若引入旁路声明，CI 直接拦截；③匹配逻辑变更按 SAFETY §2 白名单维护纪律出评审关卡（虽非 [DESTRUCTIVE]）。
- 复利结论：审计"待验证"项的闭环形态 = 黄金集样本定性 + 定级 + 最小加固 + 门禁断言固化的完整链条，而不是改完就忘。

### golden 误删率测试升级为扫描+清理两阶段（2026-10-04，P4-04）
- 背景：原"必须保留样本被扫到"只是误删的**代理**指标；SAFETY §6.1 原文是"跑扫描+清理 → 对比预期清单"。
- 落地：绿/direct 在沙箱内可真实执行（journal/preflight/审计经 `PURESLATE_DATA_ROOT` 注入），盘面核对 must_keep 缺失 = 真误删；只读/长路径 DeleteFileW 失败是 Win32 硬边界 → 从"应删"清单排除并单独断言（记录真实行为而非掩盖）。
- 边界：黄/红去向隔离区，`quarantine_root_of` 硬编码真实卷根不可注入 → golden 仅扫描期度量（集成测试的隔离区行为由 quarantine 模块单测/compat 覆盖）。头注释声明，避免后来者误以为覆盖了黄/红执行路径。

### 孤儿 journal 恢复协议（T-6）：manifest 唯一信任锚，journal 只定位不动作（2026-10-04，P4-03）
- 背景：安全审计 T-6——journal 位于用户可写目录，启动恢复若信任 journal 声称的 path 执行操作即 confused deputy（伪造 journal 让应用删/移任意文件）。
- 协议（先定死后实现，recover.rs 头注释为权威表述）：①journal 内容只用于决定"查哪条 manifest 记录"，绝不直接对 journal path 执行删除/移动；direct/recycle 不可逆去向一律只标记 orphan+审计；②quarantine 孤儿的文件移动 100% 复用 restore_one（T-2 四重校验）；③「move 已发生、manifest 未写」窗口按 store 命名约定（`<sha256 前 8>_<原名>`，深度≤3）反查**未被任何 manifest 行引用**的唯一候选——**不唯一即拒绝**、补登记进 manifest（🟡/crash.recovered）而**不自动还原**（内容身份无法再对已不存在的原文件证明，交用户在隔离区页操作）；④恢复动作逐条审计 op=recover，完成后向原 tx journal **追加** result 行闭环（只追加，seq 续编）。
- 复利结论：恢复协议的本质是把"信任根"从可写文件（journal）收敛到已有校验链（manifest→restore_one），新破坏面评审即可缩到一条；任何"补全数据"类恢复都应默认降级为登记而非动作。

### 隐私清理走规则驱动而非独立 privacy/ 模块（2026-09-30，P3-04）
- 背景：SPEC §2 列有 `privacy/` 模块（R08 枚举），但隐私项本质是文件类目（History 数据库/.lnk），guard/quarantine/journal/审计全在既有管线里。
- 决策：`resources/rules/privacy-traces.xml` 三类目（edge-history/chrome-history/recent-docs）+ `expand.rs` knownFolder 增 3 个源映射；不建 Rust 模块、不加 IPC、零新破坏性代码路径——[DESTRUCTIVE] 评审面缩到"新规则类目使 clean_execute 可作用于新文件集合"一条。
- 关键参数：`**/History*` 连 WAL/journal 伴随文件一并移入（只移主库会留残留 WAL，SQLite 打开新库时可能从 WAL 恢复历史，等于白清）；RecentDocs 仅顶层 `*.lnk`（`*` globset 不跨 `/`，非递归由模式语义保证）。
- 复利结论：能被规则引擎表达的新类目就别写代码——规则包可随发版更新（SPEC §1"改 XML 即改行为"），且引擎/守卫/事务的评审成本一次摊销。

### 启动项计划任务源：读 Tasks\*.xml + schtasks 启停，不引入 COM（2026-09-30，P3-02）
- 背景：SPEC §6.4 要求枚举登录触发计划任务；ITaskService COM 链路（CoInitialize→GetFolder→GetTasks→GetState）大量 unsafe vtable 调用，且计划任务备份无法用 `.reg`/`.lnk` 形态。
- 决策：枚举直接读 `%SystemRoot%\System32\Tasks\**\*.xml`（quick-xml 解析 LogonTrigger/Exec/Command，locale 无关、只读、零依赖沿用 P1-02b）；启停走 `schtasks /change /tn <name> /enable|/disable`（CREATE_NO_WINDOW，任务本体保留在系统=天然"不删源"）；备份记录用 startup-backup manifest.jsonl（registry 项另存 regedit v5 可导入的 `<id>.reg`，文件夹项移入 `<id>.<ext>`，满足 SPEC 路径约定）。排除 `\Microsoft\` 系统命名空间任务（几十条系统维护任务，非用户可控启动项）。
- 理由：HKLM Run 写入需管理员而应用 asInvoker——HKLM 项 toggle 会失败返回 false（契约返回 bool），可接受；WOW6432Node Run 展示归入 hklm_run（契约 source 无独立枚举值，M0 冻结不加）。

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
