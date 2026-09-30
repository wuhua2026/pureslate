//! 扫描遍历引擎（R03 walk）。SPEC §6.1：
//! 对 target 展开 → 白名单过滤（SAFETY §2）→ 规则匹配 → 产出 ScanItem。
//! reparse point 不跟随；取消令牌随时生效；进度节流回调。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::contract::{Disposition, Grade, ScanItem};
use crate::safety::whitelist::is_whitelisted;

use super::matcher::CompiledCategory;

/// 取消令牌：`cancel()` 置 true，遍历循环读取判断。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// 供 walk/mft 两个引擎读取取消状态。
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 进度回调：`(已处理文件数, 累计命中字节)`。
pub type ProgressFn = dyn Fn(u64, u64) + Send + Sync;

/// 遍历并匹配单个 target 起点，产出 ScanItem[]。
///
/// 行为（对齐 SPEC §6.1 + SAFETY §2）：
/// - `follow_links(false)` → junction/symlink/reparse point 不跟随（§2.8）；
/// - 白名单路径不下行、不产出（§2.1–4/6，含 boot/system volume/$Recycle.Bin）；
/// - include 命中且未被 exclude 排除，且 size ≥ 规则 min_size；
/// - 取消令牌随时失效（未取消时应尽早退出循环并停止下行）；
/// - 权限/IO 错误不中断整体（单点跳过）。
pub fn walk_target(
    target_start: &Path,
    compiled: &CompiledCategory,
    grade: Grade,
    disposition: Disposition,
    cancel: &CancelToken,
    progress: Option<&ProgressFn>,
) -> Vec<ScanItem> {
    let mut out = Vec::new();
    if is_whitelisted(target_start) {
        return out;
    }

    let min_size = compiled.min_size_bytes();
    let walker = walkdir::WalkDir::new(target_start)
        .follow_links(false)
        .into_iter();

    let mut done = 0u64;
    let mut bytes = 0u64;

    for entry in walker.filter_entry(|e| !is_whitelisted(e.path())) {
        if cancel.is_cancelled() {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // 权限/IO 错误：单点跳过
        };
        if !entry.file_type().is_file() {
            continue;
        }
        // 二次白名单（filter_entry 对目录排除了，这里对文件再兜底一次）。
        if is_whitelisted(entry.path()) {
            continue;
        }
        if !compiled.matches_any(entry.path(), target_start) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len();
        if size < min_size {
            continue;
        }
        out.push(ScanItem {
            id: stable_id(&compiled.category_id, entry.path()),
            category_id: compiled.category_id.clone(),
            label: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().to_string_lossy().into_owned(),
            size_bytes: size,
            grade,
            disposition,
            reason: "匹配规则".into(),
            mtime: to_epoch_ms(meta.modified().ok()),
            atime: to_epoch_ms(meta.accessed().ok()),
            dup_group: None,
        });
        done += 1;
        bytes += size;
        if let Some(p) = progress {
            p(done, bytes);
        }
    }
    if let Some(p) = progress {
        p(done, bytes);
    }
    out
}

/// 大文件记录（large 维度产出用，R05）。
#[derive(Debug, Clone)]
pub struct LargeFile {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: Option<i64>,
    pub atime_ms: Option<i64>,
}

/// 全盘遍历统计（large/dup 维度共用）。
#[derive(Debug, Default)]
pub struct WalkStats {
    pub files: u64,
    pub large_files: u64,
    /// size → count（dup 一级分组）。
    pub buckets: HashMap<u64, u64>,
    /// ≥ large_min 的文件明细（want_large 时收集，large 维度产出用）。
    pub large: Vec<LargeFile>,
}

/// 并行全盘遍历统计（DG-1 门禁 Plan B，2026-09-29 决策）。
///
/// 分片策略：主线程 BFS 展开至**第 5 层**目录作为分片（如
/// `C:\Users\<u>\AppData\Local\<app>` 粒度），沿途直属文件就地统计；白名单子树
/// 展开期即剪除（如 `C:\Windows`）。分片粒度是并行度的命门——浅层分片会让
/// AppData 这类巨型子树独占单线程、并行度归零（2026-09-29 实测：二层分片
/// 112s ≈ 串行 101s）。线程数 = min(CPU, 8)，与串行 walkdir 结果等价（单测对拍）。
///
/// 只读红线：仅遍历与 metadata 读取，无任何写操作（AGENTS §4.1）。
pub fn parallel_walk_stats(
    root: &Path,
    large_min: u64,
    want_large: bool,
    cancel: &CancelToken,
) -> WalkStats {
    let mut total = WalkStats::default();

    // ---- 主线程：BFS 展开 5 层收集分片，沿途直属文件就地统计 ----
    let mut shards: Vec<PathBuf> = Vec::new();
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    for depth in 0..5 {
        if pending.is_empty() || cancel.is_cancelled() {
            break;
        }
        let mut next: Vec<PathBuf> = Vec::new();
        for dir in pending.drain(..) {
            match std::fs::read_dir(&dir) {
                Ok(rd) => {
                    for e in rd.flatten() {
                        let p = e.path();
                        if is_whitelisted(&p) {
                            continue;
                        }
                        let Ok(ft) = e.file_type() else { continue };
                        if ft.is_file() {
                            add_file_stat(&mut total, &p, large_min, want_large);
                        } else if ft.is_dir() && !ft.is_symlink() {
                            next.push(p);
                        }
                    }
                }
                Err(_) => shards.push(dir), // 展不开（权限）：整目录成片交给 walkdir 软失败
            }
        }
        // 第 5 层目录不再展开 → 分片；巨扇出提前收口（≥2048 片足够均衡）。
        if depth == 4 || next.len() >= 2048 {
            shards.extend(next);
            break;
        }
        pending = next;
    }

    // ---- 工作线程：抢分片统计，本地聚合后合并 ----
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    if shards.is_empty() {
        return total;
    }
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..n_threads)
            .map(|_| {
                s.spawn(|| {
                    let mut local = WalkStats::default();
                    loop {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        let Some(shard) = shards.get(i) else {
                            break;
                        };
                        walk_shard(shard, large_min, want_large, cancel, &mut local);
                    }
                    local
                })
            })
            .collect();
        for h in handles {
            if let Ok(l) = h.join() {
                total.files += l.files;
                total.large_files += l.large_files;
                for (k, v) in l.buckets {
                    *total.buckets.entry(k).or_insert(0) += v;
                }
                total.large.extend(l.large);
            }
        }
    });
    total
}

/// 单分片递归统计（walkdir；白名单过滤 + 软失败跳过，与串行语义一致）。
fn walk_shard(
    shard: &Path,
    large_min: u64,
    want_large: bool,
    cancel: &CancelToken,
    local: &mut WalkStats,
) {
    if is_whitelisted(shard) {
        return;
    }
    for entry in walkdir::WalkDir::new(shard)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_whitelisted(e.path()))
    {
        if cancel.is_cancelled() {
            return;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        if is_whitelisted(entry.path()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let size = meta.len();
        add_stat(local, size, large_min, want_large);
        if want_large && size >= large_min {
            add_large(local, entry.path(), &meta, size);
        }
    }
}

/// 单文件就地统计（主线程直属文件用）。
fn add_file_stat(total: &mut WalkStats, p: &Path, large_min: u64, want_large: bool) {
    let Ok(md) = std::fs::metadata(p) else { return };
    let size = md.len();
    add_stat(total, size, large_min, want_large);
    if want_large && size >= large_min {
        add_large(total, p, &md, size);
    }
}

/// 大文件明细收集（调用方已确认 want_large && size >= large_min，R05）。
fn add_large(s: &mut WalkStats, path: &Path, meta: &std::fs::Metadata, size: u64) {
    s.large.push(LargeFile {
        path: path.to_path_buf(),
        size,
        mtime_ms: to_epoch_ms(meta.modified().ok()),
        atime_ms: to_epoch_ms(meta.accessed().ok()),
    });
}

fn add_stat(s: &mut WalkStats, size: u64, large_min: u64, want_large: bool) {
    s.files += 1;
    if want_large && size >= large_min {
        s.large_files += 1;
    }
    *s.buckets.entry(size).or_insert(0) += 1;
}

/// 由 category_id + path 生成稳定条目 id（16 位十六进制）。
/// 用 DefaultHasher（快速稳定，非加密）；路径含中文也安全（按字节散列）。
fn stable_id(category_id: &str, path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    category_id.hash(&mut h);
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn to_epoch_ms(t: Option<std::time::SystemTime>) -> Option<i64> {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::Category;
    use crate::rules::model::{Disposition as RuleDisposition, GlobRule, Risk, Target, TargetType};

    fn make_sandbox() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pureslate-walk-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.tmp"), "hello").unwrap();
        std::fs::write(d.join("keep.tmp"), "keep").unwrap(); // 会被 exclude 排除
        std::fs::write(d.join("sub").join("b.tmp"), "world").unwrap();
        std::fs::write(d.join("sub").join("c.txt"), "nocache").unwrap();
        d
    }

    fn compiled() -> CompiledCategory {
        let cat = Category {
            id: "temp.user".into(),
            label: "t".into(),
            risk: Risk::Green,
            disposition: RuleDisposition::Direct,
            description: None,
            targets: vec![Target {
                ty: TargetType::Path,
                value: String::new(),
            }],
            includes: vec![GlobRule {
                pattern: "**/*.tmp".into(),
                recursive: true,
                max_age_days: 0,
                min_size_mb: 0,
            }],
            excludes: vec![GlobRule {
                pattern: "**/keep.tmp".into(),
                recursive: true,
                max_age_days: 0,
                min_size_mb: 0,
            }],
            guard_process: None,
        };
        CompiledCategory::compile(&cat)
    }

    #[test]
    fn matches_excludes_and_size() {
        let dir = make_sandbox();
        let cc = compiled();
        let cancel = CancelToken::new();
        let items = walk_target(&dir, &cc, Grade::Green, Disposition::Direct, &cancel, None);
        // 命中 a.tmp 与 sub/b.tmp；排除 keep.tmp；排除 c.txt。
        let paths: Vec<String> = items.iter().map(|i| i.path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a.tmp")));
        assert!(paths.iter().any(|p| p.ends_with("b.tmp")));
        assert!(!paths.iter().any(|p| p.ends_with("keep.tmp")));
        assert!(!paths.iter().any(|p| p.ends_with("c.txt")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancellation_stops_early() {
        let dir = make_sandbox();
        let cc = compiled();
        let cancel = CancelToken::new();
        // 预先取消 → 不应产出任何条目。
        cancel.cancel();
        let items = walk_target(&dir, &cc, Grade::Green, Disposition::Direct, &cancel, None);
        assert!(items.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sub_dir_not_emitted_as_item() {
        // 目录不应作为条目产出；只有其中的文件 b.tmp 会产出。
        let dir = make_sandbox();
        let cc = compiled();
        let cancel = CancelToken::new();
        let items = walk_target(&dir, &cc, Grade::Green, Disposition::Direct, &cancel, None);
        // `sub` 是目录：不应出现名为 "sub" 的条目。
        assert!(!items.iter().any(|i| i.label == "sub"));
        // b.tmp 在 sub 内被匹配到。
        assert!(items.iter().any(|i| i.label == "b.tmp"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parallel_walk_stats_counts_match_tree() {
        // 分片语义对拍：根直属 + 一级直属就地统计、二级目录分片，总数/大文件/桶一致；
        // 预取消应产出空结果。
        let root = std::env::temp_dir().join(format!(
            "pureslate-parwalk-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("a/sub")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(root.join("f0"), "12").unwrap(); // 2B 根直属
        std::fs::write(root.join("a/f1"), "123").unwrap(); // 3B 一级直属
        std::fs::write(root.join("a/f2"), "123").unwrap(); // 3B 一级直属
        std::fs::write(root.join("a/sub/f3"), "12345").unwrap(); // 5B 二级分片
        std::fs::write(root.join("b/f4"), "1234567").unwrap(); // 7B 一级直属（b 无二级）

        let cancel = CancelToken::new();
        let st = parallel_walk_stats(&root, 5, true, &cancel);
        assert_eq!(st.files, 5, "全部分片+直属文件各计一次");
        assert_eq!(st.large_files, 2, "5B 与 7B >= large_min(5)");
        assert_eq!(st.buckets.get(&3), Some(&2));
        assert_eq!(st.buckets.len(), 4);
        // 大文件明细（R05）：≥large_min 的路径/大小/时间被收集。
        assert_eq!(st.large.len(), 2);
        assert!(st
            .large
            .iter()
            .any(|f| f.path.ends_with("f3") && f.size == 5));
        assert!(st
            .large
            .iter()
            .any(|f| f.path.ends_with("f4") && f.size == 7));
        assert!(st
            .large
            .iter()
            .all(|f| f.mtime_ms.is_some() && f.atime_ms.is_some()));

        let c2 = CancelToken::new();
        c2.cancel();
        let st2 = parallel_walk_stats(&root, 5, true, &c2);
        assert_eq!(st2.files, 0, "预取消应产出空结果");
        std::fs::remove_dir_all(&root).ok();
    }
}
