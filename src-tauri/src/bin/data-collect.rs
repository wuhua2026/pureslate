//! P1-09 数据采集二进制（tools/data-baseline）。
//!
//! 只读采集：枚举**所有固定盘**，逐盘产出——文件数、大文件 size-bucket 直方图、
//! 分类占用（复用真实规则引擎 + MFT/walk 两种遍历）、dup 候选、扫描耗时。
//! **全程匿名**：stdout 仅输出聚合数值 + 类别 label + 脱敏 tag，绝不落任何真实路径/文件名。
//!
//! 数据去向：由 tools/data-baseline.ps1 调用并补充各盘容量/盘型，最终渲染
//! `docs/verify/data-baseline.md`（多机聚合模板）。
//!
//! 只读红线（AGENTS §4.1）：本二进制不做任何删除/移动/写入用户数据；仅读目标盘 + stdout。
//! 生产路径禁 `unwrap()/expect()`，单点 IO 错误跳过不中断，错误经 `Result<String>` 传播。
//! 不引入任何 crate（容量/盘型由 PowerShell `Get-CimInstance` 补齐，守 AGENTS §4.7）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pureslate_lib::contract::{Disposition, Grade};
use pureslate_lib::rules::model::{Risk, Target, TargetType};
use pureslate_lib::rules::{RuleLoader, RuleSetTable};
use pureslate_lib::scanner::matcher::CompiledCategory;
use pureslate_lib::scanner::mft::MftSession;
use pureslate_lib::scanner::walk::{walk_target, CancelToken, ProgressFn};

use serde::Serialize;

// ---- 命令行参数 ----
struct Args {
    rules_dir: PathBuf,
    tag: String,
}

fn parse_args() -> Result<Args, String> {
    let mut rules_dir: Option<PathBuf> = None;
    let mut tag: Option<String> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rules-dir" => {
                rules_dir = Some(PathBuf::from(it.next().ok_or("--rules-dir 缺少值".to_owned())?));
            }
            "--tag" => {
                tag = Some(it.next().ok_or("--tag 缺少值".to_owned())?);
            }
            "--help" | "-h" => {
                eprintln!(
                    "用法: data-collect --rules-dir <规则目录> [--tag <机台标识>]\n\
                     只读枚举全部固定盘，stdout 输出匿名聚合 JSON（不落任何真实路径）。"
                );
                std::process::exit(0);
            }
            other => return Err(format!("未知参数: {other}")),
        }
    }
    Ok(Args {
        rules_dir: rules_dir.ok_or("缺少 --rules-dir".to_owned())?,
        tag: tag.unwrap_or_else(default_tag),
    })
}

/// 缺省机台标识：hostname 脱敏（仅保留小写字母/数字/`-`/`_`，其余替换为 `-`）。
fn default_tag() -> String {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unknown".into());
    host.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

// ---- 输出结构（匿名）----
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DriveReport {
    tag: String,
    drive: String,
    engine: String, // "mft" | "walk"
    ms: u64,
    files: u64,
    size_buckets: SizeBuckets,
    categories: Vec<CategoryAgg>,
    dup_candidates: u64,
    dup_candidate_bytes: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct SizeBuckets {
    lt100mb: u64,
    x100m_500mb: u64,
    x500m_1gb: u64,
    x1gb_10gb: u64,
    x10gb_up: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryAgg {
    id: String,
    label: String,
    grade: String, // "green" | "yellow" | "red"
    items: u64,
    bytes: u64,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(reports) => match serde_json::to_string_pretty(&reports) {
            Ok(s) => {
                println!("{s}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("JSON 序列化失败: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("data-collect 失败: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Vec<DriveReport>, String> {
    let args = parse_args()?;
    let mut table = RuleLoader::new();
    let table = table.load_dir(&args.rules_dir).map_err(|e| format!("规则加载失败: {e}"))?;

    let drives = fixed_drives();
    if drives.is_empty() {
        return Err("未发现任何固定盘".to_owned());
    }

    let mut reports = Vec::new();
    for drive in drives {
        let root = PathBuf::from(format!("{drive}\\"));

        // 单卷扫描：优先 MFT（管理员 + NTFS），否则回退 walkdir。
        let started = Instant::now();
        let mft = MftSession::try_open(&root).ok();
        let engine = if mft.is_some() { "mft" } else { "walk" };

        let (files, buckets) = match &mft {
            Some(s) => {
                let stats = s.full_disk_stats(0, false);
                (stats.files, stats.buckets)
            }
            None => full_disk_walk(&root)?,
        };
        let (dup_candidates, dup_candidate_bytes) = bucket_stat(&buckets);
        let size_buckets = to_size_buckets(&buckets);
        let categories = collect_categories(&table, &root, mft.as_ref(), &drive)?;
        let ms = started.elapsed().as_millis() as u64;

        reports.push(DriveReport {
            tag: args.tag.clone(),
            drive: drive.clone(),
            engine: engine.to_string(),
            ms,
            files,
            size_buckets,
            categories,
            dup_candidates,
            dup_candidate_bytes,
        });
    }
    Ok(reports)
}

// ---- 固定盘枚举（kernel32,GetDriveTypeW）----
/// 仅返回 Windows `DRIVE_FIXED(3)` 盘（跳过软驱/光驱/U盘/网络盘）。非 Windows 返回空。
fn fixed_drives() -> Vec<String> {
    cfg_if_windows_fixed_drives()
}

#[cfg(windows)]
fn cfg_if_windows_fixed_drives() -> Vec<String> {
    // DRIVE_UNKNOWN=0, DRIVE_NO_ROOT_DIR=1, DRIVE_REMOVABLE=2, DRIVE_FIXED=3,
    // DRIVE_REMOTE=4, DRIVE_CDROM=5, DRIVE_RAMDISK=6。
    const DRIVE_FIXED: u32 = 3;
    let mut out = Vec::new();
    for c in 'A'..='Z' {
        let path: Vec<u16> = format!("{c}:\\").encode_utf16().collect();
        // unsafe: kernel32,GetDriveTypeW 查询盘类型。
        let drv = unsafe { imp::GetDriveTypeW(path.as_ptr()) };
        if drv == DRIVE_FIXED {
            out.push(format!("{c}:"));
        }
    }
    out
}

#[cfg(not(windows))]
fn cfg_if_windows_fixed_drives() -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
mod imp {
    #[link(name = "kernel32")]
    extern "system" {
        pub(super) fn GetDriveTypeW(lp_root_path_name: *const u16) -> u32;
    }
}

// ---- walk 全盘枚举（回落引擎，MFT 不可用时）----
/// 全盘枚举计数（匿名）：files 总数 + size→count 直方图。
/// 语义与 `MftSession::full_disk_stats` 对齐（**磁盘普查**，不做白名单过滤），保证两引擎可比。
#[cfg_attr(not(windows), allow(dead_code))]
fn full_disk_walk(root: &Path) -> Result<(u64, HashMap<u64, u64>), String> {
    let mut files = 0u64;
    let mut buckets: HashMap<u64, u64> = HashMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // 权限/IO 错误：单点跳过
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        files += 1;
        *buckets.entry(size).or_insert(0) += 1;
    }
    Ok((files, buckets))
}

/// size 组内 ≥2 份的文件数与字节（R06 一级过滤，dup 候选，不哈希）。
fn bucket_stat(buckets: &HashMap<u64, u64>) -> (u64, u64) {
    let mut candidates = 0u64;
    let mut bytes = 0u64;
    for (&size, &count) in buckets {
        if count >= 2 {
            candidates += count;
            bytes = bytes.saturating_add(size.saturating_mul(count));
        }
    }
    (candidates, bytes)
}

/// size→count 直方图折叠为固定 5 档 bucket。
const M_100: u64 = 100 * 1024 * 1024;
const M_500: u64 = 500 * 1024 * 1024;
const G_1: u64 = 1024 * 1024 * 1024;
const G_10: u64 = 10 * 1024 * 1024 * 1024;

fn to_size_buckets(buckets: &HashMap<u64, u64>) -> SizeBuckets {
    let mut b = SizeBuckets::default();
    for (&size, &count) in buckets {
        let slot = match size {
            ..M_100 => &mut b.lt100mb,
            M_100..M_500 => &mut b.x100m_500mb,
            M_500..G_1 => &mut b.x500m_1gb,
            G_1..G_10 => &mut b.x1gb_10gb,
            _ => &mut b.x10gb_up,
        };
        *slot += count;
    }
    b
}

// ---- 分类占用（复用真实规则引擎，仅 temp 维度类目）----
/// 对表内 id 前缀为 `temp.` 的类目，展开各 target；凡落在本盘根下的 target，
/// 用 MFT（有会话时）或 walkdir 做 `walk_target`，聚合 items/bytes。
fn collect_categories(
    table: &RuleSetTable,
    drive_root: &Path,
    mft: Option<&MftSession>,
    drive: &str,
) -> Result<Vec<CategoryAgg>, String> {
    let mut out = Vec::new();
    let cancel = CancelToken::new();

    for (id, (_, cat)) in &table.by_id {
        if !id.starts_with("temp.") {
            continue;
        }
        let compiled = CompiledCategory::compile(cat);
        let gg = to_grade(cat.risk);
        let dd = to_disposition(cat.disposition);

        let mut items = 0u64;
        let mut bytes = 0u64;
        for target in &cat.targets {
            let Some(start) = expand_target(target) else {
                continue;
            };
            if !start.starts_with(drive_root) {
                continue; // 该 target 不在本盘，跳过（保持 per-drive 口径）
            }
            // 单点进度节流打印；闭包按值持有 drive/id（均转自有，满足 'static）。
            let progress: Box<ProgressFn> = Box::new({
                let id = id.clone();
                let drive = drive.to_owned();
                move |done, byt| {
                    if done % 4096 == 0 {
                        eprintln!("[{drive}/{id}] 已处理 {done} 文件 / {byt} B");
                    }
                }
            });
            let more = match mft {
                Some(s) => s.walk_target(
                    &start, &compiled, gg, dd, &cancel, Some(progress.as_ref()),
                ),
                None => walk_target(
                    &start, &compiled, gg, dd, &cancel, Some(progress.as_ref()),
                ),
            };
            items += more.len() as u64;
            bytes += more.iter().map(|i| i.size_bytes).sum::<u64>();
        }

        out.push(CategoryAgg {
            id: id.clone(),
            label: cat.label.clone(),
            grade: risk_label(cat.risk).to_string(),
            items,
            bytes,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn risk_label(risk: Risk) -> &'static str {
    match risk {
        Risk::Green => "green",
        Risk::Yellow => "yellow",
        Risk::Red => "red",
    }
}

/// rules::model::Risk → contract::Grade（walk_target 用契约类型）。
fn to_grade(risk: Risk) -> Grade {
    match risk {
        Risk::Green => Grade::Green,
        Risk::Yellow => Grade::Yellow,
        Risk::Red => Grade::Red,
    }
}

/// rules::model::Disposition → contract::Disposition。
fn to_disposition(d: pureslate_lib::rules::model::Disposition) -> Disposition {
    use pureslate_lib::rules::model::Disposition as R;
    match d {
        R::Direct => Disposition::Direct,
        R::Recycle => Disposition::Recycle,
        R::Quarantine => Disposition::Quarantine,
    }
}

// ---- target 展开（本二进制局部实现，镜像 scanner::expand，避免改公开面）----
fn expand_target(target: &Target) -> Option<PathBuf> {
    let p = match target.ty {
        TargetType::Env => {
            let var = target.value.trim().trim_start_matches('%').trim_end_matches('%');
            if var.is_empty() {
                return None;
            }
            std::env::var(var).ok().filter(|s| !s.is_empty()).map(PathBuf::from)
        }
        TargetType::Path => {
            if target.value.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(target.value.as_str()))
            }
        }
        TargetType::KnownFolder => expand_known_folder(&target.value),
    };
    match p {
        Some(p) if p.as_os_str().is_empty() => None,
        other => other,
    }
}

/// Known Folder 映射（镜像 scanner::expand::expand_known_folder 的自研映射）。
fn expand_known_folder(value: &str) -> Option<PathBuf> {
    let user = std::env::var("USERPROFILE").ok().map(PathBuf::from)?;
    let local_app = std::env::var("LOCALAPPDATA")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(user.join("AppData\\Local")));
    match value.trim() {
        "Local AppData" => local_app,
        "Temp" => std::env::var("TEMP").ok().map(PathBuf::from),
        "WeChat Files" => {
            let docs = std::env::var("DOCUMENTS")
                .ok()
                .map(PathBuf::from)
                .or_else(|| Some(user.join("Documents")));
            let a = docs.map(|d| d.join("WeChat Files"));
            if a.as_ref().is_some_and(|p| p.exists()) {
                a
            } else {
                Some(user.join("WeChat Files"))
            }
        }
        "Thumbnail Cache" => local_app.map(|p| p.join("Microsoft\\Windows\\Explorer")),
        _ => {
            eprintln!("[expand] 未知 knownFolder `{value}`，跳过该 target");
            None
        }
    }
}