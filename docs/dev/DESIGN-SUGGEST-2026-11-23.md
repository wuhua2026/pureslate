# 设计提案：扫描结果「清理建议」能力

> 状态：**待拍板（未实现）**　起草：2026-11-23　范围：公开主线 PureSlate
> 相关：SPEC §4.1（规则 schema）§4.2（扫描产物）、SAFETY §1/§2/§5、前端 `reportModel.ts` / `Report.vue`

## 1. 问题

扫描后用户面对几十项候选，核心疑问只有一句：**"这些我都该清吗？"**
现状只能靠三档颜色（🟢🟡🔴）+ 去向（直清/回收站/隔离区）+ 一段类目描述自行推断，
缺少"这一项对我意味着什么"的判断辅助，导致两种极端：不敢清（工具白装）或全清（误删风险）。

本提案只解决**解释与建议**，不改变任何删除能力、分级或去向。

---

## 2. 现状盘点：现有数据能支撑什么

### 2.1 规则层（`Category`）

字段（[model.rs](../../src-tauri/src/rules/model.rs) L149-L175）：

| 字段 | 是否可用于建议 | 说明 |
|---|---|---|
| `id` / `label` | ✅ | 类目身份与展示名 |
| `risk`（green/yellow/red） | ✅ 主信号 | 分级即"安全档位" |
| `disposition`（direct/recycle/quarantine） | ✅ 主信号 | 决定"清了能不能回来" |
| `description` | ⚠️ 弱信号 | **唯一**可读文案，纯自然语言，无结构化语义 |
| `targets` / `includes` / `excludes` | ❌ 不直接展示 | 实现细节，不宜给用户看 |
| `guard_processes` | ⚠️ 未进契约 | 仅在**清理期**用于整类跳过，扫描产物里没有 |
| `verified` | ❌ **不存在** | 公开 schema 无此属性；仅私有 fork 的 AGENTS.md §3 约定 |

XML 现状（`resources/rules/*.xml`，共 **9** 个清理类目；另有代码内审阅型 `large.file` 不注册规则）：
- 文案已具备"可重建/可还原/影响面"的**雏形**，但埋在句子里，例如
  `temp.user`："程序临时文件，删除后自动重建"；
  `cache.wechat`："聊天内图片/视频/文件缓存；清理后需重新下载，14 天内可从隔离区还原"；
  `privacy.edge-history`："…执行前需退出 Edge"。
- loader 按属性名逐个取值（[loader.rs](../../src-tauri/src/rules/loader.rs) L309-L330），
  **未知属性会被静默忽略** → 后续加性加属性不会破坏旧规则包。

### 2.2 扫描产物

`ScanItem`（[ipc.ts](../../src/types/ipc.ts) L30-L42 / [contract.rs](../../src-tauri/src/contract.rs) L88-L105）：
`id / categoryId / label / path / sizeBytes / grade / disposition / reason / mtime? / atime? / dupGroup?`

关键事实：
- **逐项 `reason` 是常量** `"匹配规则"`（[walk.rs](../../src-tauri/src/scanner/walk.rs) L119），
  只有重复文件（"与『X』内容相同…"）与大文件（审阅型说明）是差异化文案 → **逐项级别没有可用解释**。
- `mtime`/`atime` 两条扫描路径均已采集（walk `metadata()`、MFT 同样取 `modified()/accessed()`，
  [mft.rs](../../src-tauri/src/scanner/mft.rs) L161-L162）。
- 无"文件类型/是否可重建/影响哪些软件"等结构化字段。

`CategoryAggregate`（[aggregate.rs](../../src-tauri/src/scanner/aggregate.rs) L36-L59）：
`reason = category.description`（引擎 L160-L167），即类目级文案是**唯一**可复用的解释来源。

### 2.3 报告模型与方案 A/B

[reportModel.ts](../../src/pages/reportModel.ts)：
- `cleanableItems()` 排除审阅型 `large.file`（大文件永不进方案）；
- `safeCleanItems()` = 仅 `grade === "green"`；
- 方案 A「推荐清理（只清安全项）」= 🟢；方案 B「深度清理（含需确认项）」= 🟢+🟡；
- `immediateBytes` 只算 🟢 且 `direct`；`confirmItems/Bytes` = 🟡+🔴。

页面结构（[Report.vue](../../src/pages/Report.vue) L181-L347）：默认「一键清理简化卡」→ 展开后
左侧**类目行**（徽标 + 体积/项数 + 去向 + reason），右侧**方案 A/B 侧栏**。**没有逐项明细列表**。

### 2.4 安全分级与白名单降级

- 白名单在**扫描期过滤**（不入候选）+ **执行期 preflight 复核**（不信任扫描结论）；
- `whitelistOk === false` 时报告页顶部已有降级警示条（Report.vue L170+）；
- 进程守卫只在清理期生效：**扫描时应用在跑，其缓存仍会显示为候选**（这是"为何不该无脑清"的真实场景之一）；
- 🔴 默认灰禁 + 专家模式 + 一次性 token，任何建议都不能绕开。

### 2.5 缺口清单

| 缺口 | 影响 | 结论 |
|---|---|---|
| 类目无结构化"可重建/影响面"字段 | 只能靠中文句子正则，脆弱 | **需要规则层加性字段**（V1） |
| 逐项 `reason` 恒为"匹配规则" | 无法给出逐项差异化建议 | 建议**不做逐项建议**，聚合到类目级 |
| 无 `verified` 概念（公开版） | 无法表达"作者已实测可通过" | 保持私有 fork 概念，不引入公开 schema |
| 扫描期无进程占用信号 | 无法提示"该应用正在使用中" | V2 再做（需新只读信号） |
| `atime` 可靠性 | Windows 默认常关闭/延迟更新最后访问时间 | 仅作**辅助**信号，不得单独触发"建议保留" |
| 无"建议保留"类目先例 | 现有规则全部是"可清理" | 语义新增，须谨慎措辞 |

---

## 3. 判定依据分层：能算的 vs 必须写进规则的

### A. 规则内置静态属性（能直接算，零成本）

- `risk` + `disposition` → 派生三态建议（主判定）；
- `categoryId` 前缀（`temp.` / `cache.` / `privacy.` / `dup.`）→ 粗粒度性质（临时/缓存/痕迹/重复）；
- `description` → 展示文案（现状已是唯一解释来源）。

**结论：仅靠这些就能做出可信的最小版本**，因为三档分级本身就是"安全判定"的结论。

### B. 运行时信号（可计算，但有条件）

| 信号 | 现有 | 可用性 | 用途 |
|---|---|---|---|
| 体积占比（类目 / 可清理总量） | ✅ `sizeBytes` | 高 | 大额类目置顶展示，不是"更危险" |
| 文件年龄（mtime） | ✅（多数路径） | 高 | "全部早于 7 天"可作为安抚依据 |
| 最后访问（atime） | ✅ 采集 | **低**（Win 默认不更新） | 仅辅助，不单独输出"建议保留" |
| 应用是否运行中（guard 命中） | ❌ 扫描期无 | 中 | V2：可提示"正在使用，建议稍后" |
| 是否活跃配置目录 | ❌ | 低 | 不做（无法可靠判定） |
| 隔离区剩余容量/保留期 | ✅（设置/隔离区命令） | 高 | 🟡 建议文案可带"可还原 N 天" |

### C. 只能人工标注的

- "这类缓存清了是否要重新登录"（微信/浏览器登录态）——**无法从路径推断**，只能规则里写；
- "这类文件是否可重建"（缩略图=可重建；浏览历史=不可重建）——同上；
- "该软件的特殊行为"（如某些程序缓存目录混放配置）——只能人工确认 + 规则收窄。

**因此必须写进规则的新属性（建议，加性可选）**：

```xml
<category id="cache.wechat" ... rebuildable="true" impacts="chat-media">
<category id="privacy.edge-history" ... rebuildable="false" impacts="browsing-history">
```

- `rebuildable`：`true/false`，缺省不推断（缺失 = 未知，不渲染相关文案）；
- `impacts`（枚举，先定 4 值）：`none | login-state | chat-history | browsing-history | user-docs`；
- 未知值 → 视为缺失（fail-safe，不阻断加载，不编文案）。

> 不建议把 `verified` 升进公开 schema：它是"作者实测台账"，属私有 fork 的验证流程产物。

---

## 4. 表达形式：三种方案评估

| 方案 | 形态 | 优点 | 缺点 |
|---|---|---|---|
| S1 逐项一句话 | 每个 `ScanItem` 一句 | 最细 | 项数常达数千，逐项文案只能是模板复读；且逐项 reason 现为常量，需后端改造 |
| S2 类目级徽标 | 每个类目行一个"建议清理/建议保留"标签 | 与现有类目行天然契合，零布局改动 | 缺少"为什么"的总览 |
| **S3 顶部总结 + 类目徽标 + 例外项置顶**（推荐） | 顶部一句"可放心清理 X 项 · 建议保留 Y 项 · 需你判断 Z 项"，类目行加徽标，例外项排前 | 一眼可读、不制造逐项噪音、可解释 | 需新增一个总结条 |

### 对现有 UI 的影响（S3）

- **一键清理简化卡**：把"共 N 项安全项（系统临时文件 / 可重建缓存）"升级为
  "建议清理 N 项（可重建，删除后自动生成）"；按钮集合与提交内容**不变**（仍仅 🟢）。
- **完整报告类目行**：在 `cat-disp` 旁增加建议徽标；`cat-reason` 文案改为
  "依据：可重建 / 可还原 / 影响聊天记录"这样的结构化短句（V0 仍复用 description）。
- **方案 A/B**：语义不动。注意命名冲突——方案 A 已叫「**推荐**清理」，建议徽标应叫「**建议**清理」，
  并在文案上避免两者混谈（见 Q1）。
- **不新增页面、不改双栏栅格**。

### 三态词表（固定，避免自由发挥）

| 三态 | 触发条件 | 展示 |
|---|---|---|
| 建议清理 | 🟢 green + (direct 或 recycle) | 「建议清理」+ 依据（可重建 / 进回收站不立即释放） |
| 可以清理，但… | 🟡 yellow（必 quarantine） | 「可以清理，但…」+ 具体代价（重登/重下载/历史记录丢失）+「保留 14–30 天可还原」 |
| 建议保留 / 需你判断 | 🔴 red，或信号冲突（见 §5），或 `whitelistOk=false` 降级 | 「需要你判断（默认禁用）」；🔴 **禁止**出现"建议清理" |

---

## 5. 判定逻辑（V0 伪逻辑，纯前端）

```
buildSuggestions(items, categories, whitelistOk, now):
  for each category:
    if categoryId == "large.file": skip            # 审阅型不出现
    if whitelistOk === false: state = "unsure"      # 安全过滤降级 → 一律不建议
    elif grade == red:        state = "unsure"      # 永不建议清理
    elif grade == yellow:     state = "with-caveat"
    elif disposition == direct:  state = "suggest-clean"
    else:                     state = "suggest-clean"  # green + recycle
    # 例外（仅可把 suggest-clean 降级为 with-caveat / unsure）
    if state == "suggest-clean":
       if atime 最近 3 天 且 atime 可信（同一文件 mtime <= atime 且 atime > now-7d）:
            state = "with-caveat"，理由"近期访问过"
       if sizeBytes 占可清理总量 >= 60% 且 项数 >= 1000:
            state = "with-caveat"，理由"本次清理的主要体量，建议确认路径"
```

要点：
- 例外规则**只降级、不升级**（安全方向单向）；
- `atime` 不可靠，因此该条件只在"可信度自检"（mtime≤atime）通过时生效；
- 不输出任何"绝对安全/100%"措辞；所有"建议清理"必须带一个依据短语；
- 建议**不参与勾选**：默认勾选逻辑仍由 `grade` 与方案 A/B 决定，建议仅影响文案与排序。

---

## 6. 风险与红线

1. **建议不得改变可选集合**：`safeCleanItems()` / 方案 A/B / 🔴 灰禁 / token 流程一律不动；
   建议模块对 `clean_execute` 零输入（纯展示）。
2. **🔴 永不建议清理**：只允许"需要你判断（默认禁用）"，且必须与"专家模式 + token"说明同屏。
3. **白名单项不可能被建议清理**：白名单在扫描期已过滤，建议层再断言 `isExcluded` 无候选（防御性校验）。
4. **措辞红线**：禁用"绝对安全""100% 无风险""放心全清"；必须条件化表达（"可重建"/"可还原"）。
5. **降级不可静默**：`whitelistOk === false` 时建议整体降为"需你判断"，并复用现有降级警示条。
6. **可一键忽略**：设置项 `showSuggestions`（默认开）+ 报告页"隐藏建议"链接；
   关闭后页面行为与当前版本**逐像素一致**。
7. **不引入遥测**：不接受率上报；若做本地统计，仅存本机且默认关闭（列入未决）。

---

## 7. 契约影响

| 路线 | 契约变更 | 工作量 |
|---|---|---|
| **V0（推荐先行）** | **零**。全部由前端 `suggestModel.ts` 用现有 `ScanItem/CategoryAggregate` 计算 | 小 |
| V1 结构化依据 | 规则 XML 加性可选属性 `rebuildable` / `impacts`；`CategoryAggregate` 加性 `Option<>` 字段（双端同步 + CHANGELOG）；`ScanItem` **不加** | 中 |
| V2 运行中提示 | 需新的只读信号（扫描期进程快照或独立命令），属新契约面 | 中大，独立提案 |

明确**不新增 IPC 命令**：V0/V1 都可由 `scan` / `scan_items` 现有数据完成。

---

## 8. 分批落地

| 批次 | 内容 | 规则标注量 | 依赖 |
|---|---|---|---|
| **V0 最小可用** | `src/pages/suggestModel.ts` 纯函数（三态 + 顶部总结 + 类目徽标 + 例外置顶）+ 设置开关 + 单测 | **0 条新规则**；仅把 9 个类目的 `description` 润色为"依据式短句"（9 处文案） | 无 |
| V1 | 规则 schema 加 `rebuildable`/`impacts`，loader 解析 → 聚合透传 → 前端改用结构化依据 | **9 类目 × 2 属性 = 18 处** | V0 + 契约加性变更 |
| V2 | 扫描期"应用运行中"信号 → "建议稍后清理" | 0 | 独立只读信号设计 |
| V3（暂不做） | 本地接受率学习 / 个性化排序 | 0 | 需先拍板 Q6 |

V0 交付即覆盖 100% 现有类目（9 个），因为三态完全由 `risk`+`disposition` 推导，
文案来自既有 `description`——不需要新增任何规则数据。

---

## 9. 未决问题（请拍板）

- **Q1 命名**：方案 A 已叫「推荐清理」，建议徽标用「建议清理」是否易混？是否把方案 A 改名
  （如「安全清理」），或建议统一用「可放心清理」？
- **Q2 默认位置**：建议摘要只放一键清理简化卡，还是简化卡 + 完整报告都放？（推荐后者）
- **Q3 🟢 类目能否出现"建议保留"**：推荐允许，但仅限有具体依据（近期访问/体量集中）；
  是否接受它削弱"一键清理"的说服力？
- **Q4 V1 是否值得动规则 schema**：若近期还要发版，建议随下次规则包变更一起加
  `rebuildable`/`impacts`，避免两次契约变更。
- **Q5 `verified`**：确认不进入公开 schema（仍归私有 fork 台账）？
- **Q6 本地统计**：是否允许"本地记录用户忽略/采纳建议的计数"用于排序？（默认不做，涉及隐私观感）
- **Q7 多语言**：建议文案是否要进 i18n 体系？（当前 UI 全中文，NSIS 多语言仅安装器）

---

## 10. 证据索引

- 规则 schema：`docs/dev/SPEC.md` §4.1；实现 `src-tauri/src/rules/model.rs` L149-L175
- 扫描聚合：`src-tauri/src/scanner/engine.rs` L155-L170、L303-L316
- 逐项 reason 常量：`src-tauri/src/scanner/walk.rs` L111-L123
- 报告模型：`src/pages/reportModel.ts` L12-L23、L122-L146
- 报告页面：`src/pages/Report.vue` L170-L347
- 白名单降级：`src-tauri/src/ipc/commands.rs` L253-L270；`ScanResult.whitelistOk`
- 规则文案：`src-tauri/resources/rules/{system-temp,cache-apps,privacy-traces,dup-files}.xml`
