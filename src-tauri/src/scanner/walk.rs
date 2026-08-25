//! 扫描遍历引擎（R03 walk）。SPEC §6.1：
//! 对 target 展开 → 白名单过滤（SAFETY §2）→ 规则匹配 → 产出 ScanItem。
//! reparse point 不跟随；取消令牌随时生效；进度节流回调。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
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
}
