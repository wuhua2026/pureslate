//! 重复文件检测（R06 · SPEC §6.3）。
//!
//! 三级过滤：
//!   ① size 分组（HashMap<size, idx[]）——首轮廉价剔除；
//!   ② 前 64KB 采样 SHA-256 分组——大文件只读 64KB 即可互斥；
//!   ③ 全量 SHA-256 确认——仅对 "采样命中过" 的文件做全量读取。
//!
//! 保留语义：同内容组内保留 **mtime 最早** 者（keeper），其余为候选副本。
//! I/O 优先级：`find_duplicates` 入口尽力将当前线程置 BelowNormal，避免拖垮交互
//!（M3 门禁一部分；Windows 专用，失败静默）。
//!
//! 安全偏差（记 TASKS）：SPEC 原文"仅对 ≥1MB 做全量 hash"。为消除 64KB~1MB 文件
//! 的采样碰撞误判（两个不同文件前 64KB 相同但尾部不同会被误判为重复），本实现对
//! **采样命中的分组**一律做全量确认，不论文件大小（<1MB 全量读取成本可忽略）。
//! 大文件仍受益：采样唯一者不被全量读取。

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::scanner::walk::CancelToken;

/// 采样长度：前 64KB。
pub const SAMPLE_LEN: usize = 64 * 1024;
/// 全量 hash 触发阈值（SPEC §6.3，≥1MB 记大文件档；此处用于语义标注）。
/// 实际实现见文件头"安全偏差"：采样命中分组全量确认。
pub const FULL_HASH_MIN: u64 = 1024 * 1024;

/// 一个待判重候选文件。
#[derive(Debug, Clone)]
pub struct DupCandidate {
    pub path: PathBuf,
    pub size: u64,
    /// 修改时间戳（epoch ms），用于"保留最早"。
    pub mtime_ms: i64,
}

/// 一组内容相同的文件。
#[derive(Debug, Clone)]
pub struct DupGroup {
    /// 组标识：内容全量 sha256 hex（同组共享）。
    pub id: String,
    /// 单文件大小（同内容 ⇒ 同尺寸）。
    pub size_bytes: u64,
    /// 保留者（mtime 最早），不作为清理候选。
    pub keeper: PathBuf,
    /// 候选副本路径（可回收）。
    pub candidates: Vec<PathBuf>,
}

impl DupGroup {
    /// 可释放字节 = 候选数 × 单文件大小（保留的一份不计）。
    pub fn reclaimable_bytes(&self) -> u64 {
        self.candidates.len() as u64 * self.size_bytes
    }
}

/// 遍历 `root` 收集全部候选文件（只读），白名单过滤（SAFETY §2）、取消贯穿。
/// H1（v0.1.4）：dup 类目为 🟡/quarantine——§2.4 用户核心目录不再整树剪除
/// （原实现 Documents/Pictures/Desktop/Videos 四个 target 被自家白名单废掉，
/// 只剩 Downloads）；候选仍需用户逐项确认且入隔离区可还原。
/// 空文件（size==0）跳过——它们全空哈希会对海量空文件误判成一组，且"保留最早"无意义。
pub fn collect_candidates(root: &Path, cancel: &CancelToken) -> Vec<DupCandidate> {
    use crate::contract::Disposition;
    let mut out = Vec::new();
    if crate::safety::whitelist::is_excluded_from_scan(root, Disposition::Quarantine) {
        return out;
    }
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !crate::safety::whitelist::is_excluded_from_scan(e.path(), Disposition::Quarantine)
        });
    for entry in walker {
        if cancel.is_cancelled() {
            break;
        }
        let e = match entry {
            Ok(e) => e,
            Err(_) => continue, // 权限/IO 单点跳过
        };
        if !e.file_type().is_file() {
            continue;
        }
        if crate::safety::whitelist::is_excluded_from_scan(e.path(), Disposition::Quarantine) {
            continue;
        }
        let meta = match e.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len();
        // M5（SAFETY §2.7）：云盘占位文件跳过——dup 的全量哈希会触发按需下载。
        if crate::scanner::is_cloud_placeholder(&meta) {
            continue;
        }
        if size == 0 {
            continue;
        }
        out.push(DupCandidate {
            path: e.into_path(),
            size,
            mtime_ms: mtime_ms(meta.modified()),
        });
    }
    out
}

/// 判重主入口。返回若干 DupGroup（每组至少 2 个同内容文件）。
pub fn find_duplicates(cands: &[DupCandidate], cancel: &CancelToken) -> Vec<DupGroup> {
    set_below_normal_priority();

    // ① size 分组（仅需 ≥2 的组继续）。
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, c) in cands.iter().enumerate() {
        by_size.entry(c.size).or_default().push(i);
    }

    let mut groups: Vec<DupGroup> = Vec::new();
    for cands_idx in by_size.values().filter(|v| v.len() >= 2) {
        if cancel.is_cancelled() {
            break;
        }
        // ② 采样 hash 分组。
        let mut by_sample: HashMap<String, Vec<usize>> = HashMap::new();
        for &i in cands_idx {
            let c = &cands[i];
            let Ok(h) = hash_first_n(&c.path, SAMPLE_LEN) else {
                continue;
            };
            by_sample.entry(h).or_default().push(i);
        }
        for sample_idx in by_sample.values().filter(|v| v.len() >= 2) {
            if cancel.is_cancelled() {
                break;
            }
            // ③ 全量 hash 确认（大文件仅在采样命中后读取）。
            let mut by_full: HashMap<String, Vec<usize>> = HashMap::new();
            for &i in sample_idx {
                let c = &cands[i];
                let Ok(h) = hash_full(&c.path) else {
                    continue;
                };
                by_full.entry(h).or_default().push(i);
            }
            for (full_hash, full_idx) in &by_full {
                if full_idx.len() < 2 {
                    continue;
                }
                // 组内按 mtime 升序 → 最早者为 keeper。
                let mut members = full_idx.clone();
                members.sort_by_key(|&i| cands[i].mtime_ms);
                let keeper_i = members[0];
                let candidates: Vec<PathBuf> = members[1..]
                    .iter()
                    .map(|&i| cands[i].path.clone())
                    .collect();
                if candidates.is_empty() {
                    continue;
                }
                groups.push(DupGroup {
                    id: full_hash.clone(), // 组标识 = 内容全量 sha256（同组共享）
                    size_bytes: cands[full_idx[0]].size,
                    keeper: cands[keeper_i].path.clone(),
                    candidates,
                });
            }
        }
    }
    groups
}

/// 读取文件前 `n` 字节做 SHA-256（流式、有界内存）。文件小于 n 则读全文件。
fn hash_first_n(path: &Path, n: usize) -> io::Result<String> {
    let f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f.take(n as u64), &mut h)?;
    Ok(to_hex(h.finalize().as_slice()))
}

/// 全量读取文件做 SHA-256（流式、有界内存）。
fn hash_full(path: &Path) -> io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(to_hex(h.finalize().as_slice()))
}

fn to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn mtime_ms(t: io::Result<std::time::SystemTime>) -> i64 {
    t.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 尽力将当前线程置为 BelowNormal 优先级（Windows）。失败静默，不影响正确性。
fn set_below_normal_priority() {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn SetThreadPriority(hThread: *mut std::ffi::c_void, nPriority: i32) -> i32;
        }
        const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
        unsafe {
            let h = GetCurrentThread();
            let _ = SetThreadPriority(h, THREAD_PRIORITY_BELOW_NORMAL);
        }
    }
    #[cfg(not(windows))]
    {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pureslate-dup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, content: &[u8], mtime_ms: i64) -> DupCandidate {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        // 判重"保留最早"依据候选里携带的 mtime_ms，不依赖文件系统真实 mtime（粒度可能到秒）。
        DupCandidate {
            path: p.clone(),
            size: content.len() as u64,
            mtime_ms,
        }
    }

    #[test]
    fn duplicate_group_keeps_earliest_and_one_candidate() {
        let dir = sandbox();
        let cands = [
            write(&dir, "a.bin", b"hello world", 1000),
            write(&dir, "b.bin", b"hello world", 2000),
        ];
        let cancel = CancelToken::new();
        let groups = find_duplicates(&cands, &cancel);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.keeper, cands[0].path); // 最早者保留
        assert_eq!(g.candidates, vec![cands[1].path.clone()]);
        assert_eq!(g.size_bytes, 11);
        assert_eq!(g.reclaimable_bytes(), 11);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn three_identical_keep_one_emit_two() {
        let dir = sandbox();
        let cands = [
            write(&dir, "x1", b"abcdef", 500),
            write(&dir, "x2", b"abcdef", 900),
            write(&dir, "x3", b"abcdef", 300),
        ];
        let cancel = CancelToken::new();
        let groups = find_duplicates(&cands, &cancel);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].keeper, cands[2].path); // mtime 300 最早
        assert_eq!(groups[0].candidates.len(), 2);
        assert_eq!(groups[0].reclaimable_bytes(), 12);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_content_same_size_not_grouped() {
        let dir = sandbox();
        let content_a = vec![0x41u8; 1024 * 1024 + 7];
        let mut content_b = vec![0x41u8; 1024 * 1024 + 7];
        let last = content_b.len() - 1;
        content_b[last] = 0x42; // 尾部不同
        let cands = [
            write(&dir, "a.dat", &content_a, 100),
            write(&dir, "b.dat", &content_b, 200),
        ];
        let cancel = CancelToken::new();
        let groups = find_duplicates(&cands, &cancel);
        assert!(groups.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_groups_separate_ids() {
        let dir = sandbox();
        let cands = [
            write(&dir, "a1", b"AAA", 100),
            write(&dir, "a2", b"AAA", 200),
            write(&dir, "b1", b"BBB", 300),
            write(&dir, "b2", b"BBB", 400),
        ];
        let cancel = CancelToken::new();
        let groups = find_duplicates(&cands, &cancel);
        assert_eq!(groups.len(), 2);
        // 两组 id 不同
        assert_ne!(groups[0].id, groups[1].id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sample_collision_but_tail_differs_not_grouped() {
        // 前 64KB 相同、64KB..1MB 间尾部不同 → 全量确认应排除误判。
        let dir = sandbox();
        // 构造两个 >64KB 且前 64KB 相同、尾部不同的文件。
        let mut a = vec![0u8; 70 * 1024];
        let mut b = vec![0u8; 70 * 1024];
        a.iter_mut().skip(65 * 1024).for_each(|x| *x = 0x11);
        b.iter_mut().skip(65 * 1024).for_each(|x| *x = 0x22);
        let cands = [
            write(&dir, "sa.bin", &a, 100),
            write(&dir, "sb.bin", &b, 200),
        ];
        let cancel = CancelToken::new();
        let groups = find_duplicates(&cands, &cancel);
        assert!(groups.is_empty()); // 前 64KB 相同，但全量后确认不同
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_and_unique_produce_no_groups() {
        let dir = sandbox();
        let empty = std::fs::File::create(dir.join("e.bin")).unwrap();
        drop(empty);
        // collect_candidates 跳过空文件
        let cancel = CancelToken::new();
        let cands = collect_candidates(&dir, &cancel);
        assert!(cands.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancellation_yields_empty() {
        // 预先取消 → 不产出。
        let cancel = CancelToken::new();
        cancel.cancel();
        let dir = sandbox();
        let cands = [
            write(&dir, "a", b"data", 100),
            write(&dir, "b", b"data", 200),
        ];
        let groups = find_duplicates(&cands, &cancel);
        assert!(groups.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
