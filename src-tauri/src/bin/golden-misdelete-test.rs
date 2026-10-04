//! P4-04 黄金文件集误删率自测（R21 · tools/test-misdelete.ps1 调用）。
//!
//! 目标：验收安全分级/规则匹配/白名单/清理执行不误删（M2/M3 门禁，SAFETY §6.1）。
//!   🟢 误删率 < 0.5%；🟡 < 2%；🔴 = 0。
//!
//! fixture 生成器（本文件 `build_golden_tree`，声明式样本族，确定性物化）：
//!   132 文件五族（2026-10-04 P4-04 扩测，原 36 项）：
//!   - `safe/`：48 项珍贵样本（40 常规 + 8 极端命名：中文/emoji/空格/俄文/日文/
//!     嵌套目录/长名），不在任何规则 target 内 → 必须零被扫；
//!   - `temp/`（绿/direct 规则 target）：30 常规 + 6 极端命名 + 4 只读 + 2 零字节
//!     + 长路径深层样本（`\\?\` 创建 >260 字符）+ 空目录 ×2；
//!   - 白名单守卫 8 项（位于会被扫的 target 内，白名单强制排除 → 必须零被扫，
//!     验证过滤真实生效而非"不巧没扫到"）：
//!     - `temp/guard.keep` 常规范式；
//!     - 长路径深层 `guard_deep.keep`；
//!     - **T-4 旁路形态四项**（P4-04 安全审计收口）：`.` 段声明、`..` 段声明、
//!       尾随点声明、8.3 短名声明（卷禁用 8.3 时该样本整体跳过）；
//!     - `dup/guard_y.keep`（黄档 target 内）、`red/guard_r.keep`（红档 target 内）
//!       ——三档误删率分母从此非平凡；
//!   - `dup/`（黄/quarantine target）：10 组 × 2 份内容相同 → 判重引擎产出冗余副本；
//!   - `red/`（红/quarantine 规则 target，模拟"用户文档"激进规则）：12 项。
//!
//! 两个阶段（SAFETY §6.1「跑扫描+清理 → 对比预期清单」）：
//!   1. **扫描**：temp+dup+cache 三维度只读扫描沙盒；统计"必须保留样本"被扫的
//!      假阳性 → 分档误删率断言；另断言极端命名/只读/0 字节样本被完整扫到
//!      （防漏扫假阴性）、守卫零被扫、junction 零跟随。
//!   2. **清理**：对绿/direct 项走真实 `clean_execute` 全链路（resolve_targets →
//!      preflight → journal 两阶段 → DeleteFileW → 审计）；盘面核对：
//!      全部必须保留样本仍然存在（**真误删 = 盘上缺失**）、合法清理项已删除
//!      （只读/长路径项按 Windows 语义优雅失败留盘，单独断言）、journal 零孤儿。
//!      黄/红去向为隔离区（真实卷根），不在沙箱可注入范围 → 仅扫描期度量。
//!
//! 纯沙箱运行：只操作自建临时目录与注入的 golden 规则集/白名单/数据根，
//! 绝不触碰真实用户数据/系统盘。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pureslate_lib::contract::{Grade, ScanDimension, ScanProfile};
use pureslate_lib::rules::RuleLoader;
use pureslate_lib::scanner::engine::run_scan;
use pureslate_lib::scanner::walk::CancelToken;

use serde::Serialize;

/// 门禁：各档误删率上限。SAFETY §2 分档要求 🟢<0.5% / 🟡<2% / 🔴=0。
const THRESHOLDS: [(Grade, f64); 3] = [
    (Grade::Green, 0.005),
    (Grade::Yellow, 0.02),
    (Grade::Red, 0.0),
];

/// 样本规模断言：黄金集 ≥100 项（P4-04 DoD）。
const MIN_TOTAL_FILES: u64 = 100;

fn main() -> ExitCode {
    match run() {
        Ok(summary) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
            if summary.pass {
                eprintln!("[golden-misdelete-test] 通过：扫描+清理两阶段门禁全达标。");
                ExitCode::SUCCESS
            } else {
                eprintln!("[golden-misdelete-test] 未达标：存在误删/漏扫/清理异常（详见 JSON）。");
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("[golden-misdelete-test] 失败: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct T4Report {
    /// `.` 段声明守卫受保护。
    dot_seg_protected: bool,
    /// `..` 段声明守卫受保护。
    dot_dot_protected: bool,
    /// 尾随点声明守卫受保护。
    trailing_dot_protected: bool,
    /// 8.3 短名声明守卫受保护（本卷无 8.3 时无意义）。
    short_name_protected: bool,
    /// 本卷是否提供 8.3 短名（false = 短名样本整体跳过）。
    short_name_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanPhaseReport {
    temp_items: u64,
    dup_candidates: u64,
    red_items: u64,
    /// 被扫到的守卫 tag 列表（必须为空）。
    guards_swept: Vec<String>,
    extreme_temp_swept: u64,
    extreme_temp_total: u64,
    readonly_swept: u64,
    readonly_total: u64,
    zero_byte_swept: u64,
    zero_byte_total: u64,
    longpath_supported: bool,
    junction_created: bool,
    junction_followed: bool,
    t4: T4Report,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanPhaseReport {
    /// 绿/direct 实际执行数（resolve_targets 后）。
    executed: u64,
    ok: u64,
    fail: u64,
    skip: u64,
    /// 清理后盘上缺失的必须保留样本（真误删，必须为空）。
    missing_must_keep: Vec<String>,
    /// 合法清理项已删除数（只读/长路径按 Windows 语义优雅失败除外）。
    temp_gone: u64,
    temp_gone_expected: u64,
    /// 只读项清理失败留盘数（预期 = 创建数，记录真实行为）。
    readonly_survived: u64,
    /// 长路径深层项是否留盘（>260 字符超出 DeleteFileW 能力，优雅失败）。
    longpath_survived: bool,
    journal_orphans: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    ts: String,
    golden_total: u64,
    must_keep_total: u64,
    must_sweep_total: u64,
    fp: BTreeMap<String, u64>,
    rates: BTreeMap<String, f64>,
    thresholds: BTreeMap<String, f64>,
    scan: ScanPhaseReport,
    clean: CleanPhaseReport,
    pass: bool,
}

// ============================ fixture 生成器 ============================

/// 黄金样本树物化结果（预期清单 + 结构探测）。
struct GoldenManifest {
    total_files: u64,
    /// 必须保留样本（误删率分母；含 target 外珍贵样本 + 全部白名单守卫）。
    must_keep: Vec<PathBuf>,
    /// 守卫 tag → 路径（tag 被扫即守卫失效）。
    guards: Vec<(String, PathBuf)>,
    /// 绿/direct 合法清理样本（temp target 内非守卫）。
    temp_sweep: Vec<PathBuf>,
    /// 其中只读项（清理阶段预期优雅失败）。
    readonly: Vec<PathBuf>,
    /// 其中 0 字节项。
    zero_byte: Vec<PathBuf>,
    /// 极端命名清理样本（防漏扫断言）。
    extreme_temp: Vec<PathBuf>,
    /// 长路径深层合法清理样本。
    longpath_scratch: Option<PathBuf>,
    /// 黄档冗余副本（判重产出）。
    dup_candidates_expected_groups: u64,
    /// 红档合法清理样本。
    red_sweep: Vec<PathBuf>,
    longpath_supported: bool,
    junction_created: bool,
}

/// 构造黄金样本树（fixture 生成器主体）。声明式逐族物化，确定性命名。
fn build_golden_tree(sandbox: &Path) -> Result<GoldenManifest, String> {
    let mut must_keep = Vec::new();
    let mut guards: Vec<(String, PathBuf)> = Vec::new();
    let w = |p: &Path, body: &str| -> Result<(), String> {
        std::fs::create_dir_all(p.parent().ok_or("无父目录")?)
            .map_err(|e| format!("建目录 {}: {e}", p.to_string_lossy()))?;
        std::fs::write(p, body).map_err(|e| format!("写样本 {}: {e}", p.to_string_lossy()))
    };

    // ---- 族 1：safe/（target 外 · 全部 must_keep）----
    for i in 0..40 {
        let p = sandbox.join(format!("safe/keep_{i:02}.doc"));
        w(
            &p,
            &format!("珍贵样本 {i}：PureSlate 黄金集——该文件有值且不得被误删。\n"),
        )?;
        must_keep.push(p);
    }
    let extreme_safe: [&str; 8] = [
        "年度 报告 (最终版).docx",
        "游戏存档🎮备份.sav",
        "带 空 格 的 名 字.txt",
        "嵌套 中文 目录/重要 文档.pdf",
        "emoji🖼️收集/相片 📷 001.jpg",
        "Русский файл.txt",
        "ファイル.txt",
        "很长的中文文件名_很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很很.txt",
    ];
    for name in extreme_safe {
        let p = sandbox.join("safe").join(name);
        w(&p, "极端命名必须保留样本：不得因命名特殊被误扫。\n")?;
        must_keep.push(p);
    }

    // ---- 族 2：temp/（绿/direct target）----
    let temp = sandbox.join("temp");
    let mut temp_sweep = Vec::new();
    for i in 0..30 {
        let p = temp.join(format!("scratch_{i:02}.tmp"));
        w(&p, &format!("临时残留 {i}\n"))?;
        temp_sweep.push(p);
    }
    let mut extreme_temp = Vec::new();
    for name in [
        "临时_中文命名.tmp",
        "缓存🎮残留.tmp",
        "带 空格 的临时.tmp",
        "临时_Русский.tmp",
        "臨時_日本語.tmp",
        "嵌套 中文/深层 临时.log",
    ] {
        let p = temp.join(name);
        w(&p, "极端命名合法清理样本。\n")?;
        temp_sweep.push(p.clone());
        extreme_temp.push(p);
    }
    let mut readonly = Vec::new();
    for name in [
        "只读临时_one.tmp",
        "只读临时_two.tmp",
        "只读临时_three.tmp",
        "只读临时_four.tmp",
    ] {
        let p = temp.join(name);
        w(&p, "只读属性合法清理样本。\n")?;
        set_readonly(&p, true);
        temp_sweep.push(p.clone());
        readonly.push(p);
    }
    let mut zero_byte = Vec::new();
    for name in ["空文件_zero.tmp", "空文件_zero2.tmp"] {
        let p = temp.join(name);
        w(&p, "")?;
        temp_sweep.push(p.clone());
        zero_byte.push(p);
    }
    // 空目录 ×2（SAFETY §6.3.8：遍历须容忍，不产出项；不计文件数）。
    std::fs::create_dir_all(temp.join("空目录_emptydir")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(temp.join("empty_dir_2")).map_err(|e| e.to_string())?;

    // 长路径深层（SAFETY §6.3.2）：\\?\ 前缀创建 >260 字符物理路径。
    let mut deep = temp.join("longpath");
    for i in 0..4 {
        deep = deep.join(format!("深层目录{i:02}_{}", "L".repeat(45)));
    }
    let vp = |p: &Path| format!(r"\\?\{}", p.to_string_lossy());
    let (longpath_supported, longpath_scratch, deep_guard) = {
        if std::fs::create_dir_all(vp(&deep)).is_err() {
            (false, None, None)
        } else {
            let ok_scratch =
                std::fs::write(vp(&deep.join("scratch_deep.tmp")), "深层合法清理样本。\n").is_ok();
            let ok_guard =
                std::fs::write(vp(&deep.join("guard_deep.keep")), "深层白名单守卫。\n").is_ok();
            (
                ok_scratch && ok_guard,
                ok_scratch.then(|| deep.join("scratch_deep.tmp")),
                ok_guard.then(|| deep.join("guard_deep.keep")),
            )
        }
    };

    // ---- 守卫（含 T-4 旁路形态）----
    let add_guard = |tag: &str,
                     p: PathBuf,
                     must: &mut Vec<PathBuf>,
                     g: &mut Vec<(String, PathBuf)>|
     -> Result<(), String> {
        w(&p, &format!("guard[{tag}]：白名单守卫，必须不被扫。\n"))?;
        must.push(p.clone());
        g.push((tag.to_string(), p));
        Ok(())
    };
    add_guard(
        "canonical",
        temp.join("guard.keep"),
        &mut must_keep,
        &mut guards,
    )?;
    if let Some(gd) = &deep_guard {
        add_guard("deep", gd.clone(), &mut must_keep, &mut guards)?;
    }
    // T-4 四形态守卫（P4-04）：文件本体按常规名创建，白名单声明用旁路形式
    // （见 golden_whitelist_xml）。本卷无 8.3 时短名守卫整体跳过。
    add_guard(
        "t4_dot_seg",
        temp.join("sub_dir/guard_t4_dotseg.keep"),
        &mut must_keep,
        &mut guards,
    )?;
    add_guard(
        "t4_dot_dot",
        temp.join("guard_t4_dotdot.keep"),
        &mut must_keep,
        &mut guards,
    )?;
    add_guard(
        "t4_trailing_dot",
        temp.join("guard_t4_dot.keep"),
        &mut must_keep,
        &mut guards,
    )?;
    // 短名守卫：先创建文件再查询 8.3 形态（查询不存在的路径恒失败）；
    // 本卷禁用 8.3 时删除样本整体跳过。
    let short_file = temp.join("guard_t4_short_ЭтоДлинноеИмя.keep");
    w(&short_file, "guard[t4_short]：白名单守卫，必须不被扫。\n")?;
    if short_path_of(&short_file).is_some() {
        must_keep.push(short_file.clone());
        guards.push(("t4_short".to_string(), short_file));
    } else {
        let _ = std::fs::remove_file(&short_file);
    }

    // ---- 族 3：dup/（黄/quarantine target · 判重）----
    let dup = sandbox.join("dup");
    std::fs::create_dir_all(&dup).map_err(|e| e.to_string())?;
    for g in 0..10 {
        // 体积 4KB..256KB 阶梯（覆盖采样与直读两级）。
        let size = 4096u64 << (g / 2).min(6);
        let content = vec![b'A' + (g as u8 % 26); size as usize];
        for name in ["a", "b"] {
            let p = dup.join(format!("g{g}_{name}.bin"));
            std::fs::write(&p, &content).map_err(|e| e.to_string())?;
        }
    }
    let dup_guard = dup.join("guard_y.keep");
    add_guard("dup_y", dup_guard, &mut must_keep, &mut guards)?;

    // ---- 族 4：red/（红/quarantine target · 模拟"用户文档"激进规则）----
    let red = sandbox.join("red");
    let mut red_sweep = Vec::new();
    for i in 0..12 {
        let p = red.join(format!("important_{i:02}.docx"));
        w(
            &p,
            &format!("红档清理样本 {i}（规则声明为红/quarantine，仅扫描期度量）。\n"),
        )?;
        red_sweep.push(p);
    }
    let red_guard = red.join("guard_r.keep");
    add_guard("red_r", red_guard, &mut must_keep, &mut guards)?;

    // ---- 族 5：junction（SAFETY §6.3.3 · reparse 不跟随红线）----
    let junction_created = create_junction(&temp.join("junction_to_safe"), &sandbox.join("safe"));

    let total_files = count_tree_files(sandbox);

    Ok(GoldenManifest {
        total_files,
        must_keep,
        guards,
        temp_sweep,
        readonly,
        zero_byte,
        extreme_temp,
        longpath_scratch,
        dup_candidates_expected_groups: 10,
        red_sweep,
        longpath_supported,
        junction_created,
    })
}

/// 查询 8.3 短名（无短形态/查询失败返回 None；卷禁用 8.3 的环境守卫样本整体跳过）。
/// 判定法：分配足量缓冲单次调用，返回串与原串相同 = 无短形态（GetShortPathNameW
/// 对无短名的路径原样返回）。
#[cfg(windows)]
fn short_path_of(p: &Path) -> Option<PathBuf> {
    // unsafe：GetShortPathNameW 仅读取路径信息；kernel32 默认链接。
    unsafe extern "system" {
        fn GetShortPathNameW(
            lpsz_long_path: *const u16,
            lpsz_short_path: *mut u16,
            cch_buffer: u32,
        ) -> u32;
    }
    let original = p.to_string_lossy().into_owned();
    let wide: Vec<u16> = original.encode_utf16().chain(std::iter::once(0)).collect();
    let cap = wide.len() as u32 + 32;
    let mut buf = vec![0u16; cap as usize];
    unsafe {
        let written = GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), cap);
        if written == 0 || written as usize >= buf.len() {
            return None;
        }
    }
    buf.truncate(buf.iter().position(|&c| c == 0).unwrap_or(buf.len()));
    let short = String::from_utf16_lossy(&buf);
    if short == original {
        None // 无短形态（原样返回）
    } else {
        Some(PathBuf::from(short))
    }
}

#[cfg(not(windows))]
fn short_path_of(_p: &Path) -> Option<PathBuf> {
    None
}

// ============================ 规则与白名单 ============================

/// golden 规则集：temp（绿/direct）、dup（黄/quarantine）、red 模拟（红/quarantine，
/// 挂 cache. 前缀以被 cache 维度选中）。
fn golden_rules_xml(sandbox: &Path) -> String {
    let temp = sandbox.join("temp").to_string_lossy().replace('\\', "/");
    let dup = sandbox.join("dup").to_string_lossy().replace('\\', "/");
    let red = sandbox.join("red").to_string_lossy().replace('\\', "/");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="golden-scan" version="1" lang="zh-CN">
  <category id="temp.user" label="临时文件" risk="green" disposition="direct"
            description="golden 误删率测试：扫描沙箱 temp 子树">
    <target type="path" value="{temp}"/>
    <include pattern="*" recursive="true" maxAgeDays="365"/>
    <guard process=""/>
  </category>
  <category id="dup.file" label="重复文件" risk="yellow" disposition="quarantine"
            description="golden 误删率测试：扫描沙箱 dup 子树">
    <target type="path" value="{dup}"/>
    <include pattern="*" recursive="true"/>
    <guard process=""/>
  </category>
  <category id="cache.golden-red" label="黄金集红档模拟" risk="red" disposition="quarantine"
            description="golden 误删率测试：模拟激进的"用户文档"规则（仅沙箱）">
    <target type="path" value="{red}"/>
    <include pattern="*" recursive="true"/>
    <guard process=""/>
  </category>
</ruleset>
"#
    )
}

/// golden 白名单：守卫声明 + **T-4 旁路形态声明**（`.` 段 / `..` 段 / 尾随点 /
/// 8.3 短名）。声明形式故意"不规范"——归一化加固后（P4-04）四形态都必须生效。
fn golden_whitelist_xml(sandbox: &Path, m: &GoldenManifest) -> String {
    let fwd = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let temp = sandbox.join("temp");
    let mut entries: Vec<String> = Vec::new();

    let guard_path = |tag: &str| {
        m.guards
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, p)| p.clone())
    };

    // 常规范式（正斜杠）。
    if let Some(p) = guard_path("canonical") {
        entries.push(format!("  <path>{}</path>", fwd(&p)));
    }
    if let Some(p) = guard_path("deep") {
        entries.push(format!("  <path>{}</path>", fwd(&p)));
    }
    // T-4：`.` 段。
    if guard_path("t4_dot_seg").is_some() {
        entries.push(format!(
            "  <path>{}/./sub_dir/guard_t4_dotseg.keep</path>",
            fwd(&temp)
        ));
    }
    // T-4：`..` 段。
    if guard_path("t4_dot_dot").is_some() {
        entries.push(format!(
            "  <path>{}/fake/../guard_t4_dotdot.keep</path>",
            fwd(&temp)
        ));
    }
    // T-4：尾随点（Win32 创建语义下与无点同名）。
    if guard_path("t4_trailing_dot").is_some() {
        entries.push(format!("  <path>{}/guard_t4_dot.keep.</path>", fwd(&temp)));
    }
    // T-4：8.3 短名。
    if let Some(p) = guard_path("t4_short") {
        if let Some(short) = short_path_of(&p) {
            entries.push(format!("  <path>{}</path>", short.to_string_lossy()));
        }
    }
    // 黄/红档 target 内守卫（常规范式）。
    if let Some(p) = guard_path("dup_y") {
        entries.push(format!("  <path>{}</path>", fwd(&p)));
    }
    if let Some(p) = guard_path("red_r") {
        entries.push(format!("  <path>{}</path>", fwd(&p)));
    }

    format!("<whitelist>\n{}\n</whitelist>\n", entries.join("\n"))
}

// ============================ 主流程 ============================

fn run() -> Result<Summary, String> {
    let sandbox =
        std::env::temp_dir().join(format!("pureslate-golden-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    let _ = std::fs::remove_dir_all(format!(r"\\?\{}", sandbox.to_string_lossy()));
    std::fs::create_dir_all(&sandbox).map_err(|e| format!("建沙箱失败: {e}"))?;

    // ---- 1) fixture 物化 ----
    let m = build_golden_tree(&sandbox)?;
    if m.total_files < MIN_TOTAL_FILES {
        return Err(format!(
            "黄金集规模不足：{} < {MIN_TOTAL_FILES}（fixture 生成器缺陷）",
            m.total_files
        ));
    }

    // ---- 2) 规则 + 白名单（真实装载）----
    let rules_dir = sandbox.join("rules");
    std::fs::create_dir_all(&rules_dir).map_err(|e| format!("建规则目录失败: {e}"))?;
    std::fs::write(
        rules_dir.join("golden-scan.xml"),
        golden_rules_xml(&sandbox),
    )
    .map_err(|e| format!("写 golden 规则失败: {e}"))?;
    std::fs::write(
        rules_dir.join("whitelist.xml"),
        golden_whitelist_xml(&sandbox, &m),
    )
    .map_err(|e| format!("写白名单失败: {e}"))?;
    pureslate_lib::safety::whitelist::load_from_dir(&rules_dir)
        .map_err(|e| format!("加载白名单失败: {e}"))?;
    let table = RuleLoader::new()
        .load_dir(&rules_dir)
        .map_err(|e| format!("加载规则失败: {e}"))?;

    // ---- 3) 扫描阶段（temp+dup+cache 三维度只读）----
    let mut profile = ScanProfile::default();
    profile.dimensions.insert(ScanDimension::Temp, true);
    profile.dimensions.insert(ScanDimension::Dup, true);
    profile.dimensions.insert(ScanDimension::Cache, true);
    let cancel = CancelToken::new();
    let mut progress = |_: pureslate_lib::contract::ScanPhase,
                        _: usize,
                        _: usize,
                        _: String,
                        _: pureslate_lib::contract::FoundBytes| {};
    let outcome = run_scan(&table, &profile, &cancel, &mut progress);

    let mut swept: HashMap<String, Grade> = HashMap::new();
    let mut temp_items: u64 = 0;
    let mut dup_candidates: u64 = 0;
    let mut red_items: u64 = 0;
    for it in &outcome.items {
        swept.insert(norm_key(Path::new(&it.path)), it.grade);
        match it.category_id.as_str() {
            "temp.user" => temp_items += 1,
            "dup.file" => dup_candidates += 1,
            "cache.golden-red" => red_items += 1,
            _ => {}
        }
    }

    // 扫描期误删统计（假阳性 = 必须保留样本被扫到）。
    let mut fp: BTreeMap<String, u64> = BTreeMap::new();
    let mut guards_swept: Vec<String> = Vec::new();
    for mp in &m.must_keep {
        if let Some(g) = swept.get(&norm_key(mp)) {
            let key = grade_key(*g).to_string();
            *fp.entry(key).or_insert(0) += 1;
        }
    }
    for (tag, p) in &m.guards {
        if swept.contains_key(&norm_key(p)) {
            guards_swept.push(tag.clone());
        }
    }

    let must_keep_total = m.must_keep.len() as u64;
    let denom = must_keep_total.max(1) as f64;
    let mut rates_map = BTreeMap::new();
    let mut thresholds_map = BTreeMap::new();
    for (grade, th) in THRESHOLDS {
        let key = grade_key(grade);
        let r = *fp.get(key).unwrap_or(&0) as f64 / denom;
        rates_map.insert(key.to_string(), r);
        thresholds_map.insert(key.to_string(), th);
    }

    // T-4 守卫受保护状态。
    let protected = |tag: &str| !guards_swept.iter().any(|t| t == tag);
    let t4 = T4Report {
        dot_seg_protected: protected("t4_dot_seg"),
        dot_dot_protected: protected("t4_dot_dot"),
        trailing_dot_protected: protected("t4_trailing_dot"),
        short_name_protected: protected("t4_short"),
        short_name_available: m.guards.iter().any(|(t, _)| t == "t4_short"),
    };

    let scan_report = ScanPhaseReport {
        temp_items,
        dup_candidates,
        red_items,
        guards_swept: guards_swept.clone(),
        extreme_temp_swept: m
            .extreme_temp
            .iter()
            .filter(|p| swept.contains_key(&norm_key(p)))
            .count() as u64,
        extreme_temp_total: m.extreme_temp.len() as u64,
        readonly_swept: m
            .readonly
            .iter()
            .filter(|p| swept.contains_key(&norm_key(p)))
            .count() as u64,
        readonly_total: m.readonly.len() as u64,
        zero_byte_swept: m
            .zero_byte
            .iter()
            .filter(|p| swept.contains_key(&norm_key(p)))
            .count() as u64,
        zero_byte_total: m.zero_byte.len() as u64,
        longpath_supported: m.longpath_supported,
        junction_created: m.junction_created,
        junction_followed: outcome
            .items
            .iter()
            .any(|it| it.path.contains("junction_to_safe")),
        t4,
    };

    // ---- 4) 清理阶段（绿/direct 真实执行；黄/红去向隔离区仅扫描期度量）----
    // 数据根注入沙箱（journal/审计落沙箱；本进程独占，无需测试锁）。
    let data_root = sandbox.join("data-root");
    std::fs::create_dir_all(&data_root).map_err(|e| e.to_string())?;
    pureslate_lib::storage::set_data_root_override(Some(data_root));

    let green_ids: Vec<String> = outcome
        .items
        .iter()
        .filter(|it| it.category_id == "temp.user")
        .map(|it| it.id.clone())
        .collect();
    let targets = pureslate_lib::cleaner::resolve_targets(&outcome.items, &green_ids, &table)?;
    let mut noop = |_: String,
                    _: pureslate_lib::contract::Disposition,
                    _: pureslate_lib::contract::CleanProgressState,
                    _: u64,
                    _: u64| {};
    let report = pureslate_lib::cleaner::execute("golden-clean", &targets, &cancel, 14, &mut noop);

    // 盘面核对：真误删 = 必须保留样本缺失。
    let missing_must_keep: Vec<String> = m
        .must_keep
        .iter()
        .filter(|p| !p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    // 合法清理项已删除数：只读（DeleteFileW 拒绝只读）与长路径（>260 超出
    // DeleteFileW 能力）按 Windows 语义优雅失败留盘，从"应删"清单排除。
    let longpath_survived = m
        .longpath_scratch
        .as_ref()
        .map(|p| p.exists())
        .unwrap_or(false);
    let expected_gone: Vec<&PathBuf> = m
        .temp_sweep
        .iter()
        .filter(|p| {
            !m.readonly.iter().any(|r| r == *p)
                && !m
                    .longpath_scratch
                    .as_ref()
                    .map(|l| l == *p)
                    .unwrap_or(false)
        })
        .collect();
    let temp_gone = expected_gone.iter().filter(|p| !p.exists()).count() as u64;
    let readonly_survived = m.readonly.iter().filter(|p| p.exists()).count() as u64;
    let journal_orphans = pureslate_lib::cleaner::journal::detect_orphans().len() as u64;

    let clean_report = CleanPhaseReport {
        executed: report.total,
        ok: report.ok,
        fail: report.fail,
        skip: report.skip,
        missing_must_keep,
        temp_gone,
        temp_gone_expected: expected_gone.len() as u64,
        readonly_survived,
        longpath_survived,
        journal_orphans,
    };

    // ---- 5) 门禁断言 ----
    let mut pass = true;
    let mut fail = |msg: String| {
        eprintln!("[golden] {msg}");
        pass = false;
    };
    for (grade, th) in THRESHOLDS {
        let key = grade_key(grade);
        let r = rates_map.get(key).copied().unwrap_or(0.0);
        if r >= th + 1e-9 {
            fail(format!("误删率越界: {key}={r:.4} >= {th}"));
        }
    }
    if scan_report.temp_items == 0 {
        fail("sanity 失败：temp 未产出任何合法清理项".into());
    }
    if scan_report.dup_candidates == 0 {
        fail("sanity 失败：dup 未产出黄色副本候选".into());
    }
    if scan_report.red_items == 0 {
        fail("sanity 失败：红档类目未产出任何项".into());
    }
    if !scan_report.guards_swept.is_empty() {
        fail(format!(
            "白名单守卫被扫（必须零被扫）: {:?}",
            scan_report.guards_swept
        ));
    }
    if scan_report.extreme_temp_swept != scan_report.extreme_temp_total {
        fail("极端命名 temp 样本存在漏扫".into());
    }
    if scan_report.readonly_swept != scan_report.readonly_total {
        fail("只读属性 temp 样本存在漏扫".into());
    }
    if scan_report.zero_byte_swept != scan_report.zero_byte_total {
        fail("0 字节 temp 样本漏扫".into());
    }
    if m.longpath_supported && scan_report.longpath_supported {
        // 深层守卫被扫 = 白名单在长路径下失效（不可达则软跳过即通过）。
        if scan_report.guards_swept.iter().any(|t| t == "deep") {
            fail("长路径深层白名单守卫被扫".into());
        }
    }
    if m.junction_created && scan_report.junction_followed {
        fail("junction 被跟随（safe 文件经 junction 路径被扫）".into());
    }
    // T-4：四种旁路形态守卫必须全部受保护（可用的才算）。
    if !scan_report.t4.dot_seg_protected {
        fail("T-4：`.` 段声明守卫被扫（声明归一失效）".into());
    }
    if !scan_report.t4.dot_dot_protected {
        fail("T-4：`..` 段声明守卫被扫（声明归一失效）".into());
    }
    if !scan_report.t4.trailing_dot_protected {
        fail("T-4：尾随点声明守卫被扫（声明归一失效）".into());
    }
    if scan_report.t4.short_name_available && !scan_report.t4.short_name_protected {
        fail("T-4：8.3 短名声明守卫被扫（短名展开失效）".into());
    }
    // 清理阶段门禁。
    if !clean_report.missing_must_keep.is_empty() {
        fail(format!(
            "清理后必须保留样本缺失 {} 项（真误删）: {:?}",
            clean_report.missing_must_keep.len(),
            clean_report.missing_must_keep
        ));
    }
    if clean_report.temp_gone != clean_report.temp_gone_expected {
        fail(format!(
            "合法清理项残留: {}/{} 已删",
            clean_report.temp_gone, clean_report.temp_gone_expected
        ));
    }
    if clean_report.readonly_survived != m.readonly.len() as u64 {
        fail(format!(
            "只读项应全部优雅失败留盘（预期 {}）",
            m.readonly.len()
        ));
    }
    if clean_report.journal_orphans != 0 {
        fail(format!(
            "journal 孤儿 {} 项（事务未闭环）",
            clean_report.journal_orphans
        ));
    }
    if clean_report.executed == 0 {
        fail("清理阶段未执行任何项".into());
    }

    let summary = Summary {
        ts: iso_now(),
        golden_total: m.total_files,
        must_keep_total,
        must_sweep_total: (m.temp_sweep.len()
            + m.dup_candidates_expected_groups as usize
            + m.red_sweep.len()) as u64,
        fp,
        rates: rates_map,
        thresholds: thresholds_map,
        scan: scan_report,
        clean: clean_report,
        pass,
    };

    // ---- 6) 清理沙箱 ----
    pureslate_lib::storage::set_data_root_override(None);
    for p in &m.readonly {
        set_readonly(p, false);
    }
    let _ = std::fs::remove_dir_all(&sandbox);
    if sandbox.exists() {
        // 长路径子树超出 MAX_PATH 时须用 \\?\ 前缀再清一次。
        let _ = std::fs::remove_dir_all(format!(r"\\?\{}", sandbox.to_string_lossy()));
    }
    Ok(summary)
}

// ============================ 工具函数 ============================

fn grade_key(g: Grade) -> &'static str {
    match g {
        Grade::Green => "green",
        Grade::Yellow => "yellow",
        Grade::Red => "red",
    }
}

/// 路径键归一化：`/`→`\` + 小写。规则 XML 以正斜杠声明 target 时，引擎产物是
/// 混合分隔符路径，与测试侧构造的反斜杠路径比较前必须双侧归一——否则守卫
/// 检查会因字符串不一致而假通过（2026-09-29 修正）。
fn norm_key(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// 设置/清除文件只读属性（失败静默：样本尽力而为，不影响主门禁断言）。
fn set_readonly(p: &Path, on: bool) {
    if let Ok(md) = std::fs::metadata(p) {
        let mut perms = md.permissions();
        perms.set_readonly(on);
        let _ = std::fs::set_permissions(p, perms);
    }
}

/// 用 cmd mklink /J 创建 junction（无需管理员权限）；失败返回 false（该子项检查跳过）。
fn create_junction(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn count_tree_files(root: &Path) -> u64 {
    let mut n = 0u64;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            if e.file_name() == "rules" || e.file_name() == "data-root" {
                continue; // 注入的规则/数据根不计入样本
            }
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                n += count_tree_files(&e.path());
            } else {
                n += 1;
            }
        }
    }
    n
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn iso_now() -> String {
    let n = now_secs();
    let secs_in_day = n % 86400;
    let (y, mo, d) = civil_from_days((n / 86400) as i64);
    let hour = secs_in_day / 3600;
    let min = (secs_in_day % 3600) / 60;
    let sec = secs_in_day % 60;
    format!("{y:04}-{mo:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
