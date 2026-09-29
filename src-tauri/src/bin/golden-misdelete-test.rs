//! P2-07 黄金文件集误删率自测（tools/test-misdelete.ps1 调用）。
//!
//! 目标：验收安全分级/规则匹配不误删。（M2 门禁，对应 SPEC 误删率分档）
//!   🟢 必须保留样本误删率 < 0.5%；🟡 < 2%；🔴 = 0。
//!
//! 做法：
//!   1. 在系统临时目录下构造 "黄金样本树"（36 项，2026-09-29 扩测）：
//!        - `safe/`：珍贵/有值文件十三项（十常规 + 三极端命名：中文/emoji/空格），
//!          不在任何规则 target 内 → 必须零被扫；
//!        - `temp/`：八常规 + 三极端命名 + 两只读属性 + 一 0 字节 + guard.keep + 空目录一个；
//!        - `temp/guard.keep`：**白名单守卫样本**，位于会被扫的 target 内但被 whitelist 强制排除
//!          → 必须零被扫（验证白名单过滤真的拦截，而非"不巧没扫到"）；
//!        - `temp/longpath/...`：长路径深层样本（`\\?\` 前缀创建 >260 字符，SAFETY §6.3.2），
//!          内含白名单守卫 guard_deep.keep——不可达须软跳过不误报，可达须白名单生效；
//!        - `temp/junction_to_safe`：指向 safe/ 的 junction（SAFETY §6.3.3）——
//!          reparse point 不跟随是红线，若跟随则 safe 文件经 junction 路径被扫即失败；
//!        - `dup/`：三对内容相同的重复文件 → 判重引擎须产出 🟡 冗余副本、且每组保留 keeper 一份。
//!   2. 写 golden 规则集 + 白名单（含守卫样本），经 `RuleLoader`/whitelist 真实装载。
//!   3. `run_scan`（temp+dup）全程只读扫描沙盒。
//!   4. 统计"必须保留样本"被各档扫到的假阳性数 → 算分档误删率 → 断言门禁；
//!      另断言极端命名/只读/0 字节样本被完整扫到（防漏扫假阴性）、junction 零跟随。
//!
//! 纯沙箱运行：只操作自建临时目录与被注入的 golden 规则集，绝不触碰真实用户数据/系统盘。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pureslate_lib::contract::{Grade, ScanDimension, ScanProfile};
use pureslate_lib::rules::RuleLoader;
use pureslate_lib::scanner::engine::run_scan;
use pureslate_lib::scanner::walk::CancelToken;

use serde::Serialize;

/// 门禁：各档误删率上限（百分比）。SAFETY §2 分档要求 🟢<0.5% / 🟡<2% / 🔴=0。
const THRESHOLDS: [(Grade, f64); 3] = [
    (Grade::Green, 0.005),
    (Grade::Yellow, 0.02),
    (Grade::Red, 0.0),
];

fn main() -> ExitCode {
    match run() {
        Ok(summary) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
            if summary.pass {
                eprintln!("[golden-misdelete-test] 通过：误删率分档达标。");
                ExitCode::SUCCESS
            } else {
                eprintln!("[golden-misdelete-test] 未达标：存在误删假阳性（分档越界）。");
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
struct Summary {
    ts: String,
    golden_total: u64,                              // 样本树全部文件数
    must_keep_total: u64,                           // 必须保留样本数
    fp: std::collections::BTreeMap<String, u64>,    // 各档误删数（绿/黄/红）
    rates: std::collections::BTreeMap<String, f64>, // 各档误删率
    thresholds: std::collections::BTreeMap<String, f64>,
    temp_items: u64,            // 命中 temp 的合法清理项（sanity，>0）
    dup_groups: u64,            // 判重组数（sanity，>0）
    guard_swept: bool,          // 白名单守卫样本是否被扫（必须 false）
    unicode_temp_swept: u64,    // 极端命名 temp 样本被扫数（期望 = 样本数，防漏扫）
    readonly_swept: u64,        // 只读属性 temp 样本被扫数（期望 = 样本数，防漏扫）
    zero_byte_swept: u64,       // 0 字节样本被扫数（期望 1，防漏扫）
    longpath_supported: bool,   // 长路径深层样本是否创建成功（\\?\ 前缀）
    longpath_guard_swept: bool, // 长路径深层白名单守卫被扫（必须 false）
    junction_created: bool,     // junction 是否创建成功
    junction_followed: bool,    // junction 内出现任何被扫项（必须 false）
    pass: bool,
}

fn run() -> Result<Summary, String> {
    let sandbox =
        std::env::temp_dir().join(format!("pureslate-golden-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).map_err(|e| format!("建沙箱失败: {e}"))?;

    // ---- 1) 构造黄金样本树 ----
    let edge = build_golden_tree(&sandbox)?;

    // 必须保留样本 = safe/*（含极端命名）+ temp/guard.keep + 长路径深层守卫。
    let mut must_keep: Vec<PathBuf> = Vec::new();
    for i in 0..10 {
        let p = sandbox.join(format!("safe/must_keep_{:02}.txt", i + 1));
        must_keep.push(p);
    }
    must_keep.extend(edge.extreme_safe.iter().cloned());
    let guard_path = sandbox.join("temp/guard.keep");
    must_keep.push(guard_path.clone());
    if let Some(gd) = &edge.guard_deep {
        must_keep.push(gd.clone());
    }

    // ---- 2) 写 golden 规则集 + 白名单（含守卫 root） ----
    let rules_dir = sandbox.join("rules");
    std::fs::create_dir_all(&rules_dir).map_err(|e| format!("建规则目录失败: {e}"))?;
    std::fs::write(
        rules_dir.join("golden-scan.xml"),
        golden_rules_xml(&sandbox),
    )
    .map_err(|e| format!("写 golden 规则失败: {e}"))?;
    let mut guard_paths = vec![guard_path.to_string_lossy().replace('\\', "/")];
    if let Some(gd) = &edge.guard_deep {
        guard_paths.push(gd.to_string_lossy().replace('\\', "/"));
    }
    let whitelist_xml = format!(
        "<whitelist>\n{}\n</whitelist>\n",
        guard_paths
            .iter()
            .map(|g| format!("  <path>{g}</path>"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(rules_dir.join("whitelist.xml"), whitelist_xml)
        .map_err(|e| format!("写白名单失败: {e}"))?;

    pureslate_lib::safety::whitelist::load_from_dir(&rules_dir)
        .map_err(|e| format!("加载白名单失败: {e}"))?;

    let table = RuleLoader::new()
        .load_dir(&rules_dir)
        .map_err(|e| format!("加载规则失败: {e}"))?;

    // ---- 3) 只读扫描（temp + dup） ----
    let mut profile = ScanProfile::default();
    profile.dimensions.insert(ScanDimension::Temp, true);
    profile.dimensions.insert(ScanDimension::Dup, true);
    let cancel = CancelToken::new();
    let mut progress = |_: pureslate_lib::contract::ScanPhase,
                        _: usize,
                        _: usize,
                        _: String,
                        _: pureslate_lib::contract::FoundBytes| {};
    let outcome = run_scan(&table, &profile, &cancel, &mut progress);

    // ---- 4) 统计 ----
    let mut swept: HashMap<String, Grade> = HashMap::new();
    let mut temp_items: u64 = 0;
    let mut dup_groups: u64 = 0;
    for it in &outcome.items {
        swept.insert(norm_key(Path::new(&it.path)), it.grade);
        if it.category_id.starts_with("temp.") {
            temp_items += 1;
        }
        if it.category_id.starts_with("dup.") {
            dup_groups += 1;
        }
    }
    let dup_groups = dup_groups; // 复用：候选副本数 >0 即判重生效

    let mut fp: HashMap<String, u64> = HashMap::new();
    for mp in &must_keep {
        if let Some(g) = swept.get(&norm_key(mp)) {
            let key = match g {
                Grade::Green => "green",
                Grade::Yellow => "yellow",
                Grade::Red => "red",
            }
            .to_string();
            *fp.entry(key).or_insert(0) += 1;
        }
    }

    let must_keep_total = must_keep.len() as u64;
    let denom = must_keep_total.max(1) as f64;
    let rate = |k: &str| *fp.get(k).unwrap_or(&0) as f64 / denom;

    let mut rates_map = std::collections::BTreeMap::new();
    let mut thresholds_map = std::collections::BTreeMap::new();
    for (grade, th) in THRESHOLDS {
        let key = match grade {
            Grade::Green => "green",
            Grade::Yellow => "yellow",
            Grade::Red => "red",
        }
        .to_string();
        rates_map.insert(key.clone(), rate(&key));
        thresholds_map.insert(key, th);
    }

    let path_of = |p: &Path| norm_key(p);
    let guard_swept = swept.contains_key(&path_of(&guard_path));
    let unicode_temp_swept = edge
        .extreme_temp
        .iter()
        .filter(|p| swept.contains_key(&path_of(p)))
        .count() as u64;
    let readonly_swept = edge
        .readonly_temp
        .iter()
        .filter(|p| swept.contains_key(&path_of(p)))
        .count() as u64;
    let zero_byte_swept = u64::from(swept.contains_key(&path_of(&edge.zero_byte)));
    let longpath_guard_swept = edge
        .guard_deep
        .as_ref()
        .map(|p| swept.contains_key(&path_of(p)))
        .unwrap_or(false);
    // junction 跟随检测：被扫项若经 junction 前缀路径出现，即 reparse point 被跟随。
    let junction_followed = outcome
        .items
        .iter()
        .any(|it| it.path.contains("junction_to_safe"));
    let temp_items_gt0 = temp_items > 0;
    let dup_gt0 = outcome
        .items
        .iter()
        .any(|it| it.category_id.starts_with("dup.") && it.grade == Grade::Yellow);

    // 门禁断言
    let mut pass = true;
    for (grade, th) in THRESHOLDS {
        let key = match grade {
            Grade::Green => "green",
            Grade::Yellow => "yellow",
            Grade::Red => "red",
        };
        if rate(key) >= th + 1e-9 {
            eprintln!("[golden] 误删率越界: {key}={:.4} >= {th}", rate(key));
            pass = false;
        }
    }
    if !temp_items_gt0 {
        eprintln!("[golden] sanity 失败：temp 未产出任何合法清理项");
        pass = false;
    }
    if !dup_gt0 {
        eprintln!("[golden] sanity 失败：dup 未产出黄色副本候选");
        pass = false;
    }
    if guard_swept {
        eprintln!("[golden] 白名单守卫样本被扫（必须零被扫）");
        pass = false;
    }
    // 漏扫假阴性断言：极端命名/只读/0 字节样本必须被完整扫到。
    if unicode_temp_swept != edge.extreme_temp.len() as u64 {
        eprintln!("[golden] 极端命名（中文/emoji/空格）temp 样本存在漏扫");
        pass = false;
    }
    if readonly_swept != edge.readonly_temp.len() as u64 {
        eprintln!("[golden] 只读属性 temp 样本存在漏扫");
        pass = false;
    }
    if zero_byte_swept != 1 {
        eprintln!("[golden] 0 字节 temp 样本未被扫描（漏扫）");
        pass = false;
    }
    // 长路径：深层守卫不可达须软跳过（不扫即通过），可达则白名单必须生效。
    if edge.longpath_supported && longpath_guard_swept {
        eprintln!("[golden] 长路径深层白名单守卫被扫（白名单在长路径下失效）");
        pass = false;
    }
    // junction：reparse point 不跟随是红线。
    if edge.junction_created && junction_followed {
        eprintln!("[golden] junction 被跟随（safe 文件经 junction 路径被扫）");
        pass = false;
    }

    let summary = Summary {
        ts: iso_now(),
        golden_total: count_tree_files(&sandbox),
        must_keep_total,
        fp: fp.into_iter().collect(),
        rates: rates_map,
        thresholds: thresholds_map,
        temp_items,
        dup_groups,
        guard_swept,
        unicode_temp_swept,
        readonly_swept,
        zero_byte_swept,
        longpath_supported: edge.longpath_supported,
        longpath_guard_swept,
        junction_created: edge.junction_created,
        junction_followed,
        pass,
    };

    // 只读样本须先摘除只读位，否则 Windows 下 remove_dir_all 会失败残留沙箱。
    for p in &edge.readonly_temp {
        set_readonly(p, false);
    }
    let _ = std::fs::remove_dir_all(&sandbox);
    if sandbox.exists() {
        // 长路径子树超出 MAX_PATH 时须用 \\?\ 前缀再清一次。
        let _ = std::fs::remove_dir_all(format!(r"\\?\{}", sandbox.to_string_lossy()));
    }
    Ok(summary)
}

fn count_tree_files(root: &Path) -> u64 {
    let mut n = 0u64;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            if e.file_name() == "rules" || e.file_name() == "whitelist.xml" {
                continue; // 注入的规则/白名单不计入样本
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

/// 边缘样本组构造结果（供统计与断言）。
struct EdgeSetup {
    /// safe/ 下极端命名必须保留样本（中文/emoji/空格文件名）。
    extreme_safe: Vec<PathBuf>,
    /// temp/ 下极端命名合法清理样本（应被完整扫到，防漏扫假阴性）。
    extreme_temp: Vec<PathBuf>,
    /// temp/ 下只读属性合法清理样本。
    readonly_temp: Vec<PathBuf>,
    /// temp/ 下 0 字节样本。
    zero_byte: PathBuf,
    /// 长路径深层白名单守卫（\\?\ 创建成功才有）。
    guard_deep: Option<PathBuf>,
    /// 长路径深层样本是否创建成功。
    longpath_supported: bool,
    /// junction 是否创建成功。
    junction_created: bool,
}

/// 构造黄金样本树（36 项文件：safe 13 + temp 17 + dup 6；junction 为目录不计文件数）。
fn build_golden_tree(sandbox: &Path) -> Result<EdgeSetup, String> {
    // safe/：十项常规 + 三项极端命名（中文/emoji/空格），均不在 target 内，必须零被扫。
    let safe = sandbox.join("safe");
    std::fs::create_dir_all(&safe).map_err(|e| e.to_string())?;
    for i in 0..10 {
        std::fs::write(
            safe.join(format!("must_keep_{:02}.txt", i + 1)),
            format!("珍贵样本 {i}: PureSlate 黄金集——该文件应有值且不得被误删。\n"),
        )
        .map_err(|e| e.to_string())?;
    }
    let mut extreme_safe = Vec::new();
    for name in [
        "年度 报告 (最终版).docx",
        "游戏存档🎮备份.sav",
        "带 空 格 文 件 名.txt",
    ] {
        let p = safe.join(name);
        std::fs::write(&p, "极端命名必须保留样本：不得因命名特殊被误扫。\n")
            .map_err(|e| e.to_string())?;
        extreme_safe.push(p);
    }

    // temp/：八常规 + 三极端命名 + 两只读 + 一 0 字节 + 空目录 + 白名单守卫。
    let temp = sandbox.join("temp");
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    for i in 0..8 {
        std::fs::write(
            temp.join(format!("scratch_{i:02}.tmp")),
            format!("临时残留 {i}\n"),
        )
        .map_err(|e| e.to_string())?;
    }
    let mut extreme_temp = Vec::new();
    for name in [
        "临时文件_中文命名.tmp",
        "缓存🎮残留.tmp",
        "带 空格 的临时.tmp",
    ] {
        let p = temp.join(name);
        std::fs::write(&p, "极端命名合法清理样本。\n").map_err(|e| e.to_string())?;
        extreme_temp.push(p);
    }
    let mut readonly_temp = Vec::new();
    for name in ["只读临时_one.tmp", "只读临时_two.tmp"] {
        let p = temp.join(name);
        std::fs::write(&p, "只读属性合法清理样本。\n").map_err(|e| e.to_string())?;
        set_readonly(&p, true);
        readonly_temp.push(p);
    }
    let zero_byte = temp.join("空文件_zero.tmp");
    std::fs::write(&zero_byte, "").map_err(|e| e.to_string())?;
    // 空目录（SAFETY §6.3.8 边界：遍历须容忍，不产出项）。
    std::fs::create_dir_all(temp.join("空目录_emptydir")).map_err(|e| e.to_string())?;
    // temp/guard.keep：白名单守卫样本（位于会被扫的 target 内 → 若白名单失效会被扫）。
    std::fs::write(temp.join("guard.keep"), "guard: 白名单守卫，必须不被扫。\n")
        .map_err(|e| e.to_string())?;

    // 长路径（SAFETY §6.3.2）：用 \\?\ 前缀创建 >260 字符深层物理路径，引擎仍以普通
    // 路径遍历——不可达时须软跳过（不崩不误报），可达时白名单须对深层守卫生效。
    let mut deep = temp.join("longpath");
    for i in 0..4 {
        deep = deep.join(format!("深层目录{:02}_{}", i, "L".repeat(45)));
    }
    let mut guard_deep: Option<PathBuf> = None;
    let longpath_supported = {
        let vp = |p: &Path| format!(r"\\?\{}", p.to_string_lossy());
        if std::fs::create_dir_all(vp(&deep)).is_err() {
            false
        } else {
            let ok = std::fs::write(vp(&deep.join("guard_deep.keep")), "深层白名单守卫。\n")
                .is_ok()
                && std::fs::write(vp(&deep.join("scratch_deep.tmp")), "深层合法清理样本。\n")
                    .is_ok();
            if ok {
                guard_deep = Some(deep.join("guard_deep.keep"));
            }
            ok
        }
    };

    // junction（SAFETY §6.3.3）：temp 内建指向 safe/ 的 junction。
    // 红线：reparse point 不跟随——若跟随，safe 文件会以 junction 前缀路径被扫。
    let junction_created = create_junction(&temp.join("junction_to_safe"), &safe);

    // dup/：三对内容相同（每对同内容 → 判重引擎产出冗余副本 + 保留 keeper）。
    let dup = sandbox.join("dup");
    std::fs::create_dir_all(&dup).map_err(|e| e.to_string())?;
    for g in 0..3 {
        let content = format!("重复样本组 {g}: {} bytes\n", "x".repeat(4096 + g));
        for name in ["a", "b"] {
            std::fs::write(dup.join(format!("g{g}_{name}.bin")), &content)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(EdgeSetup {
        extreme_safe,
        extreme_temp,
        readonly_temp,
        zero_byte,
        guard_deep,
        longpath_supported,
        junction_created,
    })
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
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// golden 规则集：temp 扫 `<sandbox>\temp`（绿/direct），dup 扫 `<sandbox>\dup`（黄/quarantine）。
fn golden_rules_xml(sandbox: &Path) -> String {
    let temp = sandbox.join("temp").to_string_lossy().replace('\\', "/");
    let dup = sandbox.join("dup").to_string_lossy().replace('\\', "/");
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
</ruleset>
"#
    )
}
