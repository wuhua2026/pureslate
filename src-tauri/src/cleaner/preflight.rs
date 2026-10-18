//! 执行前最终复核（T-1 · P4-02 [DESTRUCTIVE]，安全审计 IPC/TOCTOU 缺口修复）。
//!
//! 扫描与执行之间存在时间窗（TOCTOU）：提权进程/用户操作可在窗口内把目标的
//! 中间目录替换为 junction/symlink、改写文件内容、或把目标移入白名单禁区。
//! 本模块在 `apply_one` 动手前做最后一道复核，任一不过即拒绝该目标：
//! 1. **白名单复核**：重跑 `is_whitelisted`（不信任扫描期结论）；
//! 2. **逐级 reparse 复核**：从根到叶逐级 `symlink_metadata`，任一祖先或目标本身
//!    是 junction/symlink（Rust std 将二者均映射为 `is_symlink`）即拒绝——
//!    "扫描后替换中间目录"攻击在此被拦截；
//! 3. **一致性复核**：size 与扫描记录一致；mtime 有记录时也须一致（内容被改写即拒绝）。
//!
//! 复核失败按单目标失败处理（journal result ok=false + 审计），不中断事务其余目标。

use std::path::{Path, PathBuf};

use super::execute::CleanTarget;
use crate::contract::Disposition;
use crate::safety::whitelist::{
    is_hard_protected, is_soft_program_root, is_user_core_data, is_within,
};

/// 复核一个待清理目标。`Ok(())` 放行；`Err(reason)` 拒绝（原因入 journal/审计）。
pub fn verify(t: &CleanTarget) -> Result<(), String> {
    let p = &t.path;

    // 1) 白名单最终复核（分层语义）：
    //    a) 硬保护（系统/引导卷目录、SystemRoot、卷根关键文件、隔离区哨兵、自身设施、
    //       XML 附加白名单）——**任何授权都不可豁免**，永远拒绝；
    //    b) §2.2 程序目录软根（ProgramFiles*/ProgramData\Microsoft）——普通清理
    //       一律拦截；R27-3 后端签发的精确残留授权根可在根内豁免（SAFETY §2.2 条款）；
    //    c) §2.4 用户核心目录——direct/recycle 整树拦；quarantine 按 H1 放行；
    //       R27-3 精确授权根内任何去向放行（残留可能位于 Documents 下的异常安装位，
    //       已由枚举器高置信 + 用户逐项确认兜底，硬保护 a) 仍优先）。
    // R31-4：SAFETY §2.1 唯一例外——`%SystemRoot%\Temp` 仅对 🟢 direct/recycle
    // 放行（temp.system 类目，与扫描侧 is_excluded_from_scan 同口径）。
    let safe_temp_exception = t.disposition != Disposition::Quarantine
        && crate::safety::whitelist::is_safe_temp_exception(p);
    if is_hard_protected(p) && !safe_temp_exception {
        return Err(format!(
            "执行前复核：目标命中硬保护白名单（系统/引导/隔离区/规则声明），已拒绝 ({})",
            p.to_string_lossy()
        ));
    }
    let in_authorized_root = t
        .authorized_root
        .as_ref()
        .is_some_and(|root| is_within(p, root));
    if is_soft_program_root(p) && !in_authorized_root {
        return Err(format!(
            "执行前复核：目标位于程序目录白名单且无残留授权，已拒绝 ({})",
            p.to_string_lossy()
        ));
    }
    if t.disposition != Disposition::Quarantine && is_user_core_data(p) && !in_authorized_root {
        return Err(format!(
            "执行前复核：目标命中安全白名单，已拒绝 ({})",
            p.to_string_lossy()
        ));
    }

    // 2) 逐级 reparse 复核（含目标自身）。
    if let Some(seg) = first_reparse_component(p) {
        return Err(format!(
            "执行前复核：路径组件含 junction/symlink，已拒绝（{}）",
            seg.to_string_lossy()
        ));
    }

    // 3) 与扫描记录的一致性（size / mtime）。
    let meta = std::fs::metadata(p).map_err(|e| format!("执行前复核：读取目标元数据失败: {e}"))?;
    if meta.len() != t.size_bytes {
        return Err(format!(
            "执行前复核：目标大小与扫描记录不符（现 {} / 记录 {}），可能已被修改",
            meta.len(),
            t.size_bytes
        ));
    }
    if let Some(expected) = t.mtime_ms {
        let actual = modified_ms(&meta);
        if actual != expected {
            return Err(format!(
                "执行前复核：目标修改时间与扫描记录不符（现 {actual} / 记录 {expected}），可能已被修改"
            ));
        }
    }
    Ok(())
}

/// 从根到叶逐级检查，返回第一个命中 reparse（junction/symlink）的路径组件。
/// `symlink_metadata` 不跟随链接；std 在 Windows 上把 junction 与 symlink 都映射为
/// `is_symlink()`（IO_REPARSE_TAG_MOUNT_POINT / SYMLINK）。
pub fn first_reparse_component(p: &Path) -> Option<PathBuf> {
    // ancestors() 由叶到根；反转为根到叶（先查上层，尽早拦截"中间目录被替换"）。
    let mut chain: Vec<&Path> = p.ancestors().collect();
    chain.reverse();
    for seg in chain {
        if let Ok(meta) = std::fs::symlink_metadata(seg) {
            if meta.file_type().is_symlink() {
                return Some(seg.to_path_buf());
            }
        }
    }
    None
}

/// metadata → epoch 毫秒（i64；不可得时 -1，与扫描侧语义一致即视为一致失败面）。
fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Disposition, Grade};

    fn target(path: &Path, size: u64, mtime: Option<i64>) -> CleanTarget {
        CleanTarget {
            path: path.to_path_buf(),
            grade: Grade::Green,
            disposition: Disposition::Direct,
            category_id: "temp.user".into(),
            size_bytes: size,
            guard_processes: Vec::new(),
            mtime_ms: mtime,
            authorized_root: None,
        }
    }

    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "pureslate-preflight-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn normal_target_passes() {
        let d = sandbox("ok");
        let f = d.join("a.tmp");
        std::fs::write(&f, b"hello").unwrap();
        let mtime = {
            let meta = std::fs::metadata(&f).unwrap();
            modified_ms(&meta)
        };
        assert!(verify(&target(&f, 5, Some(mtime))).is_ok());
        // 无 mtime 记录时只比 size。
        assert!(verify(&target(&f, 5, None)).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn size_mismatch_rejected() {
        let d = sandbox("size");
        let f = d.join("b.tmp");
        std::fs::write(&f, b"hello").unwrap();
        let r = verify(&target(&f, 999, None));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("大小与扫描记录不符"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mtime_mismatch_rejected() {
        let d = sandbox("mtime");
        let f = d.join("c.tmp");
        std::fs::write(&f, b"hello").unwrap();
        let r = verify(&target(&f, 5, Some(12345)));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("修改时间与扫描记录不符"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn whitelisted_target_rejected_at_execute_time() {
        // T-1 核心场景：扫描期白名单未命中 ≠ 执行期安全；执行前重跑拦截。
        if let Ok(root) = std::env::var("SystemRoot") {
            let f = Path::new(&root).join("notepad.exe");
            let meta = std::fs::metadata(&f).ok();
            let (size, mtime) = match &meta {
                Some(m) => (m.len(), Some(modified_ms(m))),
                None => (0, None),
            };
            let r = verify(&target(&f, size, mtime));
            assert!(r.is_err(), "白名单路径必须在执行期被复核拦截");
            assert!(r.unwrap_err().contains("白名单"));
        }
    }

    /// R27-3 授权分层：硬根授权也不豁免；程序目录软根凭精确授权放行到下一闸。
    #[test]
    fn r273_authorization_layers() {
        // ① 硬根（SystemRoot\notepad.exe）：即便带授权根也必须被拦。
        if let Ok(win) = std::env::var("SystemRoot") {
            let f = Path::new(&win).join("notepad.exe");
            if let Ok(meta) = std::fs::metadata(&f) {
                let mut t = target(&f, meta.len(), Some(modified_ms(&meta)));
                t.authorized_root = Some(PathBuf::from(&win));
                let r = verify(&t);
                assert!(r.is_err(), "硬保护不得被残留授权豁免");
                assert!(r.unwrap_err().contains("硬保护"));
            }
        }
        // ② 程序目录软根：无授权 → 程序目录白名单拦截；授权根精确覆盖 →
        //    越过白名单闸（走到元数据/reparse 等后续检查，错误文案不再含白名单）。
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            let Ok(pf) = std::env::var(var) else { continue };
            // 动态找一个真实存在的文件作为路径样本（不修改/删除它）。
            let Some(sample) = find_any_file(Path::new(&pf), 3) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&sample) else {
                continue;
            };
            let mut t = target(&sample, meta.len(), Some(modified_ms(&meta)));
            let no_auth = verify(&t);
            assert!(
                no_auth
                    .as_ref()
                    .is_err_and(|e| e.contains("程序目录白名单")),
                "{var} 下无授权应被软根拦截: {sample:?} => {no_auth:?}"
            );
            // 授权根 = 文件所在产品目录（其某个祖先，软根之下一层及更深均可）。
            let product_root = sample
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(&pf));
            t.authorized_root = Some(product_root);
            // size/mtime 均真实一致 → 应整体 Ok（reparse 也必须无；有 reparse 的罕见
            // 样本会报 junction 错，同样证明已越过白名单闸）。
            match verify(&t) {
                Ok(()) => {}
                Err(e) => assert!(!e.contains("白名单"), "精确授权后不得再被白名单拦截: {e}"),
            }
            // 授权根不覆盖（伪造为无关目录）→ 仍拦。
            t.authorized_root = Some(PathBuf::from("Z:\\__nonexistent_root__"));
            assert!(verify(&t)
                .as_ref()
                .is_err_and(|e| e.contains("程序目录白名单")));
            break; // 一个真实软根验证足够
        }
    }

    /// 在 root 下至多 max_depth 层找第一个普通文件（避开 reparse）。
    fn find_any_file(root: &Path, max_depth: usize) -> Option<PathBuf> {
        for entry in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() && !entry.file_type().is_symlink() {
                return Some(entry.into_path());
            }
        }
        None
    }

    #[test]
    fn symlink_ancestor_rejected() {
        // junction/symlink 祖先（扫描后替换中间目录攻击）。
        let d = sandbox("reparse");
        let real = d.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("a.tmp"), b"x").unwrap();
        let link = d.join("link");
        #[cfg(windows)]
        {
            // symlink 目录用 mklink /D 需要开发者模式/特权；junction（/J）不需要——
            // 用 junction 构造（std 将 junction 映射为 is_symlink）。
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&real)
                .output()
                .expect("mklink");
            assert!(
                out.status.success(),
                "mklink /J 应可用: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let f = link.join("a.tmp");
            let meta = std::fs::metadata(&f).unwrap();
            let r = verify(&target(&f, meta.len(), Some(modified_ms(&meta))));
            assert!(r.is_err(), "junction 路径下的目标必须被拒绝");
            assert!(r.unwrap_err().contains("junction/symlink"));
            assert!(first_reparse_component(&f).is_some());
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let f = link.join("a.tmp");
            let meta = std::fs::metadata(&f).unwrap();
            assert!(verify(&target(&f, meta.len(), None)).is_err());
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
