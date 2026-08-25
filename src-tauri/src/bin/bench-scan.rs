//! DG-1 性能决策门基准二进制（tools/bench-scan）。
//!
//! 独立运行，不经 Tauri IPC。对 `temp+large+dup` 三维度做**只读**遍历计时，
//! 依 SPEC §8 口径：先冷缓存（写读删 ~5GB 干扰文件）再分维度计时，stdout 输出 JSON。
//!
//! - temp：真实规则引擎（rules loader + CompiledCategory + walk_target）；
//! - large/dup：全盘（系统盘根）枚举成本度量（large/dup 的实现分别在 P3-01/P2-05，
//!   此处只量其主导成本——目录遍历枚举；dup 的 sha256 哈希延迟到 P2-07）。
//!
//! 全程只读（冷缓存文件仅操作 bench 自建临时目录，随用随删）；单点 IO 错误跳过不中断；
//! 生产路径禁止 unwrap()/expect()，错误经 Result 传播。

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pureslate_lib::contract::{Disposition, Grade};
use pureslate_lib::rules::model::TargetType;
use pureslate_lib::rules::{RuleLoader, RuleSetTable};
use pureslate_lib::safety::is_whitelisted;
use pureslate_lib::scanner::matcher::CompiledCategory;
use pureslate_lib::scanner::walk::{walk_target, CancelToken};

use serde::Serialize;

// ---- 命令行参数 ----
struct Args {
    rules_dir: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut rules_dir: Option<PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--rules-dir" => {
                let v = it.next().ok_or("--rules-dir 缺少值".to_owned())?;
                rules_dir = Some(PathBuf::from(v));
            }
            "--help" | "-h" => {
                eprintln!(
                    "用法: bench-scan --rules-dir <rules 目录>\n\
                     对 temp+large+dup 三维度只读遍历计时，stdout 输出 JSON。"
                );
                std::process::exit(0);
            }
            other => return Err(format!("未知参数: {other}")),
        }
    }
    Ok(Args {
        rules_dir: rules_dir.ok_or("缺少 --rules-dir".to_owned())?,
    })
}

// ---- 输出结构（SPEC §8）----
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchReport {
    ts: String,
    app_version: String,
    drive: String,
    cold_cache_bytes: u64,
    dims: Dims,
    total_ms: u64,
    threshold_ms: u64,
    pass: bool,
    note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Dims {
    temp: TempDim,
    large: LargeDim,
    dup: DupDim,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TempDim {
    ms: u64,
    items: u64,
    bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LargeDim {
    ms: u64,
    files: u64,
    large_files: u64, // >=500MB
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DupDim {
    ms: u64,
    candidates: u64, // 同 size 组内 >=2 份的文件数
    candidate_bytes: u64,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(report) => match serde_json::to_string_pretty(&report) {
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
            eprintln!("bench-scan 失败: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<BenchReport, String> {
    let args = parse_args()?;
    let drive = system_drive();

    // 1) 冷缓存（SPEC §8：先读写释放 5GB 干扰文件）。
    let cold_cache_bytes = cold_cache();

    // 2) temp 维度：真实规则引擎。
    let temp = bench_temp(&args.rules_dir)?;

    // 3) large 维度：全盘枚举（找 >=500MB）。
    let large = bench_large(&drive)?;

    // 4) dup 维度：全盘枚举 + size 预分组（不做哈希）。
    let dup = bench_dup(&drive)?;

    let total_ms = temp.ms + large.ms + dup.ms;
    let pass = total_ms <= 120_000;

    Ok(BenchReport {
        ts: iso_now(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        drive,
        cold_cache_bytes,
        dims: Dims { temp, large, dup },
        total_ms,
        threshold_ms: 120_000,
        pass,
        note: "三维度独立遍历(保守上界)；large/dup 同一次全盘枚举在 P1-05 可复用；dup 哈希延迟到 P2-07；磁盘总字节未查询(未引 Windows crate)".into(),
    })
}

fn system_drive() -> String {
    // 优先 %SystemDrive%（如 C:），兜底取当前 exe 所在盘。
    std::env::var("SystemDrive")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().chars().take(2).collect())
                .unwrap_or_else(|| "C:".into())
        })
}

fn drive_root(drive: &str) -> PathBuf {
    let d = drive.trim_end_matches('\\');
    PathBuf::from(format!("{d}\\"))
}

// ---- 冷缓存 ----
fn cold_cache() -> u64 {
    let target_gb: u64 = 5;
    let chunk = 1024 * 1024; // 1MiB 写块
    let buf = vec![0u8; 1024 * 1024];
    let path = std::env::temp_dir().join(format!(
        "pureslate-bench-cold-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let mut written: u64 = 0;
    let mut err: Option<String> = None;

    if let Ok(mut f) = std::fs::File::create(&path) {
        // 写 ~target_gb GiB。
        for _ in 0..(target_gb * 1024) {
            if f.write_all(&buf).is_err() {
                err = Some("写入冷缓存失败".into());
                break;
            }
            written += chunk;
        }
        drop(f);
        // 读一遍以驱逐文件缓存。
        if let Ok(rd) = std::fs::File::open(&path) {
            use std::io::Read as _;
            let _ = rd.take(written).read_to_end(&mut Vec::new());
        }
    } else {
        err = Some("创建冷缓存文件失败".into());
    }

    // 随用随删（只读红线：仅操作 bench 自建临时文件）。
    let _ = std::fs::remove_file(&path);

    eprintln!(
        "[cold-cache] 写入并回读 {:.2} GiB 干扰文件{}",
        written as f64 / (1024f64 * 1024f64 * 1024f64),
        err.map(|e| format!(" ({e})")).unwrap_or_default()
    );
    written
}

// ---- 目标展开（bench 内临最小实现，真实展开在 P1-05）----
fn expand_target(ty: TargetType, value: &str) -> Option<PathBuf> {
    match ty {
        TargetType::Path => Some(PathBuf::from(value)),
        TargetType::Env => {
            let trimmed = value.trim_start_matches('%').trim_end_matches('%');
            if trimmed.is_empty() {
                return std::env::temp_dir().into();
            }
            std::env::var(trimmed)
                .ok()
                .map(PathBuf::from)
                .or_else(|| std::env::temp_dir().into())
        }
        TargetType::KnownFolder => None, // 本环节用不到，不展开
    }
}

fn load_table(rules_dir: &Path) -> Result<RuleSetTable, String> {
    let mut loader = RuleLoader::new();
    loader
        .load_dir(rules_dir)
        .map_err(|e| format!("规则加载失败: {e}"))
}

// ---- temp 维度（真实引擎）----
fn bench_temp(rules_dir: &Path) -> Result<TempDim, String> {
    let table = load_table(rules_dir)?;
    let mut items = 0u64;
    let mut bytes = 0u64;

    // 仅测 system-temp 的两个目录类目标：temp.user(%TEMP%) 与 temp.system(C:\Windows\Temp)。
    let ids = ["temp.user", "temp.system"];
    let started = Instant::now();

    for id in ids {
        let Some((_, cat)) = table.by_id.get(id) else {
            eprintln!("[temp] 缺规则 category `{id}`，跳过");
            continue;
        };
        let compiled = CompiledCategory::compile(cat);
        let cancel = CancelToken::new();
        for t in &cat.targets {
            let Some(start) = expand_target(t.ty, &t.value) else {
                continue;
            };
            if is_whitelisted(&start) {
                eprintln!("[temp] 目标被白名单拦截: {start:?}，跳过");
                continue;
            }
            let more = walk_target(
                &start,
                &compiled,
                Grade::Green,
                Disposition::Direct,
                &cancel,
                Some(&move |done, byt| {
                    if done % 4096 == 0 {
                        eprintln!("[temp/{id}] 已处理 {done} 文件 / {byt} B");
                    }
                }),
            );
            items += more.len() as u64;
            bytes += more.iter().map(|i| i.size_bytes).sum::<u64>();
        }
    }

    Ok(TempDim {
        ms: started.elapsed().as_millis() as u64,
        items,
        bytes,
    })
}

// ---- 全盘枚举（large 与 dup 共用 walkdir 逻辑）----
struct DiskWalk {
    files: u64,
    garbage: u64,               // >=500MB
    buckets: HashMap<u64, u64>, // size -> count（dup 预分组）
}

fn full_disk_walk(root: &Path, want_large: bool) -> Result<DiskWalk, String> {
    let mut out = DiskWalk {
        files: 0,
        garbage: 0,
        buckets: HashMap::new(),
    };

    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_whitelisted(e.path()))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // 权限/IO 错误单点跳过
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if is_whitelisted(entry.path()) {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        out.files += 1;
        if want_large && size >= 500 * 1024 * 1024 {
            out.garbage += 1;
        }
        *out.buckets.entry(size).or_insert(0) += 1;
        if out.files.is_multiple_of(65536) {
            eprintln!("[walk] 已枚举 {files} 文件", files = out.files);
        }
    }
    Ok(out)
}

fn bucket_stat(w: &DiskWalk) -> (u64, u64) {
    // R06 一级过滤：size 组内 >=2 份才可能是重复候选。
    let mut candidates = 0u64;
    let mut cand_bytes = 0u64;
    for (&size, &count) in &w.buckets {
        if count >= 2 {
            candidates += count;
            cand_bytes = cand_bytes.saturating_add(size.saturating_mul(count));
        }
    }
    (candidates, cand_bytes)
}

fn bench_large(drive: &str) -> Result<LargeDim, String> {
    let root = drive_root(drive);
    let started = Instant::now();
    let w = full_disk_walk(&root, true)?;
    Ok(LargeDim {
        ms: started.elapsed().as_millis() as u64,
        files: w.files,
        large_files: w.garbage,
    })
}

fn bench_dup(drive: &str) -> Result<DupDim, String> {
    let root = drive_root(drive);
    let started = Instant::now();
    let w = full_disk_walk(&root, false)?;
    let (candidates, cand_bytes) = bucket_stat(&w);
    Ok(DupDim {
        ms: started.elapsed().as_millis() as u64,
        candidates,
        candidate_bytes: cand_bytes,
    })
}

// ---- 工具 ----
fn iso_now() -> String {
    // 简单 ISO8601（UTC，够基准报告核对用；无需 chrono 依赖）。
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs_in_day = n % 86400;
    let (y, mo, d) = civil_from_days((n / 86400) as i64);
    let hour = secs_in_day / 3600;
    let min = (secs_in_day % 3600) / 60;
    let sec = secs_in_day % 60;
    format!("{y:04}-{mo:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// 从 1970-01-01 起的天数转 (年,月,日)。Howard Hinnant 的 civil_from_days 简版。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0,399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1,12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
