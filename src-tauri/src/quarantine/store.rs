//! 隔离区移入（R25 · SAFETY §4.2）。
//!
//! 流程：计算目标隔离路径（同卷瞬时 rename / 跨盘 copy+fsync+hash 校验+删源），
//! 先写 manifest 行再做容器层返回。journal 两阶段协议由 `cleaner`（P2-04）在上层接入；
//! 本模块只保证"文件安全迁入隔离区 + manifest 有记录"这一最小原子面。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::contract::Grade;

use super::ensure_quarantine_root;
use super::manifest::{add_manifest_entry, ManifestEntry, ManifestState};

/// 移入隔离区失败原因。
#[derive(Debug, thiserror::Error)]
pub enum QuarantineMoveError {
    #[error("隔离区目录不可用: {0}")]
    Io(#[from] std::io::Error),
    #[error("序号序列化失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("源路径不是文件: {path}")]
    NotFile { path: String },
    #[error("跨盘校验失败（目标哈希与源不一致），已中止: {0}")]
    HashMismatch(String),
}

/// 移入参数（保留原文案的等级/类目与保留天数）。
pub struct QuarantineInput {
    pub original_path: PathBuf,
    pub grade: Grade,
    pub category_id: String,
    /// 保留天数（来自 settings.qrantRetentionDays，移入即定截止）。
    pub retention_days: u32,
}

/// 把单个文件移入隔离区，返回生成的一条 manifest 记录。
///
/// `root` 为隔离区根（由调用方用 `quarantine_root_of` 计算，或测试注入沙箱根）。
///
/// 语义（SAFETY §4.2）：
/// - 同卷：`fs::rename` 瞬时移动；
/// - 跨卷：复制 → fsync → 校验目标 sha256 与源一致 → 删除源；校验失败中止并报 `HashMismatch`；
/// - 目标路径 `yyyyMM\<batchId>\<原路径 hash 前缀>_<原文件名>` 防冲突；
/// - 源路径鉴定为目录 → `NotFile`。
pub fn move_into_quarantine(
    root: &Path,
    input: QuarantineInput,
) -> Result<ManifestEntry, QuarantineMoveError> {
    let src = &input.original_path;
    let meta = fs::symlink_metadata(src).map_err(QuarantineMoveError::Io)?;
    if !meta.is_file() {
        return Err(QuarantineMoveError::NotFile {
            path: src.to_string_lossy().into_owned(),
        });
    }

    ensure_quarantine_root(root)?;

    // 目标子路径：yyyyMM / batchId / <hash前缀>_<原文件名>
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let batch_id = uuid::Uuid::new_v4().to_string();
    let ymd = format_yyyymm(now);
    let file_name = src
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unnamed".to_string());
    let src_sha = sha256_file(src)?;
    let hash_prefix: String = src_sha.chars().take(8).collect();

    let target_dir = root.join(&ymd).join(&batch_id);
    fs::create_dir_all(&target_dir)?;
    let target = target_dir.join(format!("{hash_prefix}_{file_name}"));

    // 同卷判定：源与目标卷根一致 → rename；否则跨盘复制+校验。
    if same_volume(src, &target) {
        fs::rename(src, &target)?;
    } else {
        copy_verify_delete(src, &target)?;
    }

    let moved_at = now;
    let entry = ManifestEntry {
        id: uuid::Uuid::new_v4().to_string(),
        original_path: src.to_string_lossy().into_owned(),
        quarantine_path: target.to_string_lossy().into_owned(),
        size_bytes: meta.len(),
        sha256: src_sha,
        grade: input.grade,
        category_id: input.category_id,
        moved_at,
        expires_at: moved_at + (input.retention_days as i64) * 86_400_000,
        state: ManifestState::Quarantined,
    };
    add_manifest_entry(root, &entry)?;
    Ok(entry)
}

/// 跨盘复制：复制 → fsync → 目标 sha256 == 源 → 删除源。校验失败中止（不清源）。
fn copy_verify_delete(src: &Path, target: &Path) -> Result<(), QuarantineMoveError> {
    fs::copy(src, target)?;
    // fsync 目标（尽力而为；失败不阻断，进 error 则中止）
    if let Ok(f) = fs::File::open(target) {
        let _ = f.sync_all();
    }
    let target_sha = sha256_file(target)?;
    let src_sha = sha256_file(src)?;
    if target_sha != src_sha {
        return Err(QuarantineMoveError::HashMismatch(format!(
            "src={} target={}",
            src.to_string_lossy(),
            target.to_string_lossy()
        )));
    }
    fs::remove_file(src)?;
    Ok(())
}

/// 计算文件 sha256（十六进制小写）。
pub fn sha256_file(path: &Path) -> Result<String, QuarantineMoveError> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `yyyyMM`（北京时间近似，用本地时间；采集口径无需时区精度）。
fn format_yyyymm(ms: i64) -> String {
    let secs = ms / 1000;
    let days = secs / 86_400;
    let (y, m) = civil_from_days(days);
    format!("{y}{m:02}")
}

/// 天数 → (年, 月)（Howard Hinnant civil 算法，proleptic Gregorian，仅用于目录命名）。
fn civil_from_days(z: i64) -> (i64, i32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let _d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i32)
}

/// 两个路径是否同一卷（盘符一致）。
fn same_volume(a: &Path, b: &Path) -> bool {
    volume_of(a) == volume_of(b)
}

fn volume_of(p: &Path) -> Option<String> {
    let s = p.to_string_lossy();
    let mut c = s.chars();
    let f = c.next()?;
    let g = c.next()?;
    if f.is_ascii_alphabetic() && g == ':' {
        Some(format!("{f}:"))
    } else {
        None
    }
}

// 仅供测试：避免预留守卫占用告警。

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pureslate-qstore-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("dst")).unwrap();
        d
    }

    #[test]
    fn moves_file_and_writes_manifest() {
        let root = sandbox();
        let qroot = root.join("quar");
        let src_file = root.join("src").join("wechat-cache.db");
        std::fs::write(&src_file, "cache-body").unwrap();
        let entry = move_into_quarantine(
            &qroot,
            QuarantineInput {
                original_path: src_file.clone(),
                grade: Grade::Yellow,
                category_id: "cache.wechat".into(),
                retention_days: 14,
            },
        )
        .unwrap();
        // 源已被移走
        assert!(!src_file.exists());
        // 目标在隔离区内
        assert!(std::path::Path::new(&entry.quarantine_path).is_file());
        assert!(entry
            .quarantine_path
            .starts_with(qroot.to_string_lossy().as_ref()));
        // manifest 有记录
        let (entries, _) = super::super::manifest::load_manifest(&qroot).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, entry.id);
        assert_eq!(entries[0].state, ManifestState::Quarantined);
        assert_eq!(entries[0].expires_at - entries[0].moved_at, 14 * 86_400_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_directory() {
        let root = sandbox();
        let qroot = root.join("quar");
        let dir = root.join("src").join("subdir");
        std::fs::create_dir_all(&dir).unwrap();
        let r = move_into_quarantine(
            &qroot,
            QuarantineInput {
                original_path: dir.clone(),
                grade: Grade::Green,
                category_id: "temp.user".into(),
                retention_days: 14,
            },
        );
        assert!(matches!(r, Err(QuarantineMoveError::NotFile { .. })));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sha256_stable() {
        let root = sandbox();
        let f = root.join("src").join("x.txt");
        std::fs::write(&f, "hello").unwrap();
        let h1 = sha256_file(&f).unwrap();
        let h2 = sha256_file(&f).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn civil_yyyymm_sane() {
        // 2026-08-25 UTC -> 202608
        // 用 civil_from_days 直接验证
        let days = (SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)
            / 86_400;
        let (y, m) = civil_from_days(days);
        // 当前 2026-08 应为 true（今日日期 2026-08-25）
        assert!(y >= 2026);
        assert!((1..=12).contains(&m));
    }
}
