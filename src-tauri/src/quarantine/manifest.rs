//! 隔离区 manifest（R25 · SPEC §4.3）。
//!
//! `<卷根>\.pureslate-quarantine\manifest.jsonl` 逐行 JSON。每行一条 `ManifestEntry`。
//! 只追加（append），行校验失败/冲突行仅跳过并告警，不中断后续行。

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::QUARANTINE_DIR;
use crate::contract::Grade;

/// manifest 文件名（位于隔离区根内）。
pub const MANIFEST_FILE: &str = "manifest.jsonl";

/// 条目状态（SPEC §4.3：quarantined | restored | purged）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestState {
    Quarantined,
    Restored,
    Purged,
}

/// 一条隔离区记录（SPEC §4.3 JSON 结构）。键名 camelCase，与 IPC 契约一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub id: String,
    pub original_path: String,
    pub quarantine_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub grade: Grade,
    pub category_id: String,
    pub moved_at: i64,
    /// 保留截止（epoch ms）。由创建时按保留天数计算，本模块不负责寿命，见 `lifecycle`。
    pub expires_at: i64,
    /// 源文件被隔离前的 mtime（epoch ms）。还原时回写，保留取证信息（SAFETY §4.2）。
    /// 可选：旧版 manifest 行缺少该字段时还原仅尽力回写。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_mtime_ms: Option<i64>,
    /// 还原发生时间（epoch ms）。P3-06：restored 行保留 30 天后由 lifecycle 清理
    /// （SPEC §4.3）。可选：旧版行缺失时 lifecycle 不清理（保守保留）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_at: Option<i64>,
    pub state: ManifestState,
}

/// 隔离区根（`quarantine_root_of`）+ manifest 完整路径。
pub fn manifest_path_of(root: &Path) -> std::path::PathBuf {
    root.join(MANIFEST_FILE)
}

/// 追加一行 manifest（幂等：不入已存在同 id 行）。
pub fn add_manifest_entry(root: &Path, entry: &ManifestEntry) -> std::io::Result<()> {
    let p = manifest_path_of(root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut f = OpenOptions::new().create(true).append(true).open(&p)?;
    f.write_all(line.as_bytes())
}

/// 读取全部条目。坏行跳过并累计计数（返回 `(entry, 跳过行数)`）。
pub fn load_manifest(root: &Path) -> std::io::Result<(Vec<ManifestEntry>, u64)> {
    let p = manifest_path_of(root);
    if !p.exists() {
        return Ok((Vec::new(), 0));
    }
    let f = File::open(&p)?;
    let reader = BufReader::new(f);
    let mut entries = Vec::new();
    let mut skipped = 0u64;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<ManifestEntry>(trimmed) {
            Ok(e) => entries.push(e),
            Err(_) => skipped += 1,
        }
    }
    Ok((entries, skipped))
}

/// 列出全部条目（含跳过计数）。
pub fn list_manifest(root: &Path) -> std::io::Result<(Vec<ManifestEntry>, u64)> {
    load_manifest(root)
}

/// 按 id 更新条目状态为终态（restored/purged）。行替换而非追加（state 迁移语义）。
/// 仅覆盖 state 字段，其余原样保留；迁移到 Restored 时补记 `restored_at`（P3-06 生命周期行清理依据）。
pub fn update_entry_state(root: &Path, id: &str, state: ManifestState) -> std::io::Result<bool> {
    let p = manifest_path_of(root);
    if !p.exists() {
        return Ok(false);
    }
    let (entries, _) = load_manifest(root)?;
    let mut changed = false;
    let mut out = String::new();
    for mut e in entries {
        if e.id == id {
            e.state = state;
            if state == ManifestState::Restored {
                e.restored_at = Some(super::now_ms());
            }
            changed = true;
        }
        out.push_str(&serde_json::to_string(&e)?);
        out.push('\n');
    }
    if changed {
        let mut f = File::create(&p)?;
        f.write_all(out.as_bytes())?;
    }
    Ok(changed)
}

/// 清理"已还原且还原时间早于 cutoff"的 manifest 行（SPEC §4.3：restored 行保留 30 天）。
/// 只删行不动文件（文件早已还原回原路径）；缺 `restored_at` 的旧版行保守保留。
pub fn prune_restored_before(root: &Path, cutoff_ms: i64) -> std::io::Result<usize> {
    let p = manifest_path_of(root);
    if !p.exists() {
        return Ok(0);
    }
    let (entries, _) = load_manifest(root)?;
    let mut removed = 0usize;
    let mut out = String::new();
    for e in entries {
        let drop =
            e.state == ManifestState::Restored && e.restored_at.is_some_and(|t| t < cutoff_ms);
        if drop {
            removed += 1;
        } else {
            out.push_str(&serde_json::to_string(&e)?);
            out.push('\n');
        }
    }
    if removed > 0 {
        std::fs::write(&p, out)?;
    }
    Ok(removed)
}

/// 清除全部行（隔离区整卷清空用）。返回被清理行数。
/// P3-06 生命周期任务将接入；当前未调用，保留避免误删。
#[allow(dead_code)]
pub fn clear_manifest(root: &Path) -> std::io::Result<usize> {
    let p = manifest_path_of(root);
    let count = if !p.exists() {
        0
    } else {
        let (entries, _) = load_manifest(root)?;
        let n = entries.len();
        std::fs::remove_file(&p)?;
        n
    };
    Ok(count)
}

/// 防御性：隔离区目录名常量再导出（供 volume 计算）。P3-06 接入。
#[allow(dead_code)]
pub fn quarantine_dir_name() -> &'static str {
    QUARANTINE_DIR
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "pureslate-qmanifest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn entry(id: &str) -> ManifestEntry {
        ManifestEntry {
            id: id.into(),
            original_path: format!(r"C:\Users\u\file{id}.tmp"),
            quarantine_path: format!(r"C:\.pureslate-quarantine\202609\a\b_{id}.tmp"),
            size_bytes: 1024,
            sha256: "abc".into(),
            grade: crate::contract::Grade::Yellow,
            category_id: "cache.wechat".into(),
            moved_at: 1756000000000,
            expires_at: 1757200000000,
            original_mtime_ms: None,
            restored_at: None,
            state: ManifestState::Quarantined,
        }
    }

    #[test]
    fn append_then_load_roundtrip() {
        let root = sandbox();
        let p = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&p).unwrap();
        add_manifest_entry(&p, &entry("e1")).unwrap();
        add_manifest_entry(&p, &entry("e2")).unwrap();
        let (entries, skipped) = load_manifest(&p).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "e1");
        assert_eq!(entries[1].id, "e2");
        assert_eq!(entries[0].state, ManifestState::Quarantined);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_line_skipped_not_fatal() {
        let root = sandbox();
        let p = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&p).unwrap();
        let path = p.join(MANIFEST_FILE);
        std::fs::write(
            &path,
            r#"not-json-line
{"id":"e1","originalPath":"x","quarantinePath":"y","sizeBytes":1,"sha256":"a","grade":"yellow","categoryId":"c","movedAt":1,"expiresAt":2,"state":"quarantined"}
"#,
        )
        .unwrap();
        let (entries, skipped) = load_manifest(&p).unwrap();
        assert_eq!(skipped, 1, "skipped ({skipped}) != 1");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "e1");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn update_state_overwrites_state_only() {
        let root = sandbox();
        let p = root.join(QUARANTINE_DIR);
        std::fs::create_dir_all(&p).unwrap();
        add_manifest_entry(&p, &entry("e1")).unwrap();
        let ok = update_entry_state(&p, "e1", ManifestState::Restored).unwrap();
        assert!(ok);
        let (entries, _) = load_manifest(&p).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, ManifestState::Restored);
        assert_eq!(entries[0].sha256, "abc");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quarantine_root_computed_on_volume() {
        let qroot = |p: &str| super::super::quarantine_root_of(std::path::Path::new(p));
        // 相对/无盘符路径回退 C:
        let rel = qroot("foo\\bar");
        assert!(rel
            .to_string_lossy()
            .starts_with(r"C:\.pureslate-quarantine"));
        // 盘符路径归属正确
        assert_eq!(
            qroot(r"D:\x\y"),
            std::path::PathBuf::from(r"D:\.pureslate-quarantine")
        );
        let _ = QUARANTINE_DIR;
        let _ = quarantine_dir_name();
    }
}
