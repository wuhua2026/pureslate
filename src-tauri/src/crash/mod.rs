//! 崩溃安全（R24 · SPEC §6.6）。
//!
//! - `dump`：SEH 未处理异常过滤器 + panic hook → `MiniDumpWriteDump` 本地落盘
//!   （`<data_root>\crash\`，dbghelp `#[link]` 直调，零第三方依赖守包体红线）；
//! - `parse`：minidump 只读解析（模块列表摘要 + 异常代码），供 opt-in 上传前预览；
//! - `recover`：启动孤儿 journal 恢复（SAFETY §3 规则 2，T-6 协议）。

pub mod dump;
pub mod parse;
pub mod recover;

pub use dump::{install_handlers, write_dump};

use std::path::PathBuf;

use crate::contract::CrashDumpInfo;
use crate::storage::data_root;

/// dump 目录：`<data_root>\crash`。
pub fn crash_dir() -> PathBuf {
    data_root().join("crash")
}

/// 保留最近 N 份 dump（启动时清理更旧的；MiniDumpNormal 每份约百 KB~几 MB）。
pub const KEEP_DUMPS: usize = 5;

/// 上传体积上限（防御异常巨大的 dump；MiniDumpNormal 正常远小于此）。
pub const MAX_UPLOAD_BYTES: u64 = 50 * 1024 * 1024;

/// 崩溃转储上传端点。**2026-10-04 决策：不接入上传后端**（R24 裁剪预案"上传
/// 降级仅本地"）——`.invalid` 顶级域保证不误触真实主机，`crash_upload` 命令
/// 保留完整链路（opt-in 门禁/体积限制/审计）但必然优雅失败；本地 dump 保存
/// 与预览不受影响。未来若接入后端，改此常量即可。
pub const CRASH_UPLOAD_URL: &str = "https://pureslate.invalid/crash-upload";

/// 生产安装：未处理异常 / panic 双通道写 dump 到数据根 crash 目录。
pub fn install() {
    dump::install_handlers(crash_dir());
}

/// 校验前端传入的 dump 文件名并拼出安全路径（防路径穿越，口径对齐 P4-01 规则包校验）。
pub fn validated_dump_path(file_name: &str) -> Result<PathBuf, String> {
    let ok = file_name.len() <= 120
        && file_name.ends_with(".dmp")
        && !file_name.contains(&['\\', '/', ':'][..])
        && file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !ok {
        return Err(format!("非法转储文件名: {file_name}"));
    }
    Ok(crash_dir().join(file_name))
}

/// 列出 crash 目录全部 dump（时间倒序）。只列不删——清理统一在启动时做。
pub fn list_dumps() -> Vec<CrashDumpInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(crash_dir()) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".dmp") {
            continue;
        }
        let Ok(meta) = e.metadata() else {
            continue;
        };
        let ts = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(CrashDumpInfo {
            file_name: name,
            size_bytes: meta.len(),
            ts,
        });
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.file_name.cmp(&b.file_name)));
    out
}

/// 清理旧 dump，仅保留最近 `keep` 份（sidecar JSON 一并清理）。返回删除数（启动钩子调用）。
pub fn prune_old_dumps(keep: usize) -> usize {
    let mut removed = 0;
    for d in list_dumps().into_iter().skip(keep) {
        let p = crash_dir().join(&d.file_name);
        if std::fs::remove_file(&p).is_ok() {
            removed += 1;
            let mut s = p.as_os_str().to_os_string();
            s.push(".json");
            let _ = std::fs::remove_file(PathBuf::from(s)); // sidecar 尽力而为
        }
    }
    removed
}

/// 读取 dump 的 sidecar JSON 异常代码（`<dump>.json` 的 exceptionCode 字段）。
/// MiniDumpWriteDump 异常流在本机环境不可用（恒 998，LESSONS §①），
/// SEH 通道的异常代码经过滤器快照写入 sidecar 携带；缺失/损坏返回 None。
pub fn read_sidecar_code(dump_path: &std::path::Path) -> Option<u32> {
    let mut s = dump_path.as_os_str().to_os_string();
    s.push(".json");
    let text = std::fs::read_to_string(PathBuf::from(s)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("exceptionCode")?.as_u64().map(|n| n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> std::sync::MutexGuard<'static, ()> {
        // 抗中毒（LESSONS ②）：单测首败不级联污染同批用例。
        let g = crate::storage::TEST_DATA_ROOT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!(
            "pureslate-crashmod-{tag}-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        crate::storage::set_data_root_override(Some(d));
        g
    }

    #[test]
    fn filename_validation_rejects_traversal() {
        assert!(validated_dump_path("pureslate-crash-1-2.dmp").is_ok());
        for bad in [
            "../evil.dmp",
            "..\\evil.dmp",
            "a/b.dmp",
            "C:\\x.dmp",
            "no-ext.txt",
            "",
            "naïve.dmp",
        ] {
            assert!(validated_dump_path(bad).is_err(), "应拒绝: {bad}");
        }
    }

    #[test]
    fn prune_keeps_newest() {
        let _g = sandbox("prune");
        for i in 0..7 {
            let p = crash_dir().join(format!("pureslate-crash-2026010{i}-000000-{i}.dmp"));
            std::fs::create_dir_all(crash_dir()).unwrap();
            std::fs::write(&p, b"x").unwrap();
            // mtime 拉开差异（列目录按 mtime 排序）
            let ft = std::fs::FileTimes::new().set_modified(
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000 + i * 10),
            );
            std::fs::OpenOptions::new()
                .write(true)
                .open(&p)
                .unwrap()
                .set_times(ft)
                .unwrap();
        }
        assert_eq!(list_dumps().len(), 7);
        let removed = prune_old_dumps(KEEP_DUMPS);
        assert_eq!(removed, 2);
        assert_eq!(list_dumps().len(), KEEP_DUMPS);
        // 保留的是 mtime 最新的：最旧两份（...000000-0/1）应已删除
        let names: Vec<String> = list_dumps().into_iter().map(|d| d.file_name).collect();
        assert!(!names.iter().any(|n| n.ends_with("-000000-0.dmp")));
        assert!(!names.iter().any(|n| n.ends_with("-000000-1.dmp")));
        crate::storage::set_data_root_override(None);
    }
}
