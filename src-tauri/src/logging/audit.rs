//! 审计日志（SPEC §4.4，R09 / M8）。
//!
//! 落盘：`%LOCALAPPDATA%\PureSlate\logs\audit-YYYY-MM-DD.jsonl`，只追加、按天滚动。
//! 每条是一行 JSON（camelCase，结构见 `contract::LogEntry`）。
//! 覆盖全部破坏性操作（clean/restore/purge/auto_purge/disable_startup）与 scan（M8=100%）。
//!
//! 只追加红线（AGENTS §4）：绝不回写/截断历史行；单行损坏跳过不致命（同 manifest）。
//! 生产路径禁 unwrap/expect；错误走 `io::Result` 传播，写入失败不 panic。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::contract::LogEntry;
use crate::storage::data_root;

/// 当前时间 epoch ms。
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 日志根目录：`<data_root>\logs`。
pub fn log_dir() -> PathBuf {
    data_root().join("logs")
}

/// 由时间戳(epoch ms) → 当日文件名 `audit-YYYY-MM-DD.jsonl`（UTC 语义，够日志归档用）。
fn file_for_ts(ts_ms: i64) -> String {
    format!("audit-{}.jsonl", date_str(ts_ms))
}

/// 时间戳(epoch ms) → `YYYY-MM-DD`（proleptic Gregorian，Howard Hinnant civil 算法）。
fn date_str(ts_ms: i64) -> String {
    let days = ts_ms.div_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 天数 → (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// 追加一条审计记录。按 `entry.ts` 的日期落到对应当日文件（只追加，不覆盖）。
pub fn record(entry: &LogEntry) -> std::io::Result<()> {
    let dir = log_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(file_for_ts(entry.ts));
    let line = serde_json::to_string(entry)?;
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")
}

/// 按时间范围 + 操作过滤，跨天合并、按 ts 升序返回。
/// 单行损坏跳过不致命。
pub fn query(from: i64, to: i64, op: Option<&str>) -> Vec<LogEntry> {
    let mut out: Vec<LogEntry> = Vec::new();
    // 轮询范围内涉及的全部日期文件即可：覆盖 [from,to] 的时间 span。
    let start_day = from.div_euclid(86_400_000);
    let end_day = to.div_euclid(86_400_000);
    for day in start_day..=end_day.max(start_day) {
        let name = format!("audit-{}.jsonl", date_str(day * 86_400_000));
        let path = log_dir().join(name);
        if !path.is_file() {
            continue;
        }
        if let Ok(lines) = read_lines(&path) {
            for line in lines {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<LogEntry>(&line) {
                    Ok(e) => {
                        if e.ts >= from && e.ts <= to && op.is_none_or(|o| e.op == o) {
                            out.push(e);
                        }
                    }
                    // 单行损坏跳过（不致命，审计尽量完整）
                    Err(_) => continue,
                }
            }
        }
    }
    out.sort_by_key(|e| e.ts);
    out
}

/// 导出全量审计日志到指定文件（JSONL）。覆盖整个历史，不做范围裁剪（契约签名仅 `path`）。
pub fn export_all(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let dir = log_dir();
    let mut f = File::create(path)?;
    if let Ok(rd) = fs::read_dir(&dir) {
        // 收集并排序日志文件，保证按日期顺序输出。
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("audit-"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        for fp in files {
            if let Ok(lines) = read_lines(&fp) {
                for line in lines {
                    if !line.trim().is_empty() {
                        let _ = writeln!(f, "{line}");
                    }
                }
            }
        }
    }
    f.flush()
}

fn read_lines(path: &Path) -> std::io::Result<Vec<String>> {
    let f = OpenOptions::new().read(true).open(path)?;
    let r = BufReader::new(f);
    r.lines().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Disposition, LogEntry};

    /// 造一条日志。ts 可注入跨天测试。
    fn entry(ts: i64, op: &str, result: &str) -> LogEntry {
        LogEntry {
            ts,
            op: op.to_string(),
            tx_id: Some("tx-1".into()),
            category_id: None,
            path: Some("C:\\tmp\\junk.tmp".into()),
            size_bytes: Some(456),
            disposition: Some(Disposition::Direct),
            result: Some(result.to_string()),
            detail: None,
        }
    }

    /// 独立沙箱：持共享锁并覆盖 data_root，避免污染真实 `%LOCALAPPDATA%`/并行竞态。
    fn sandbox(tag: &str) -> std::sync::MutexGuard<'static, ()> {
        let g = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
        let sandbox = std::env::temp_dir().join(format!(
            "pureslate-log-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        crate::storage::set_data_root_override(Some(sandbox.join("data-root")));
        g
    }

    #[test]
    fn append_then_read_back_same_day() {
        let _g = sandbox("r1");
        let ts = now_ms();
        let e = entry(ts, "restore", "ok");
        record(&e).unwrap();
        let got = query(ts - 1000, ts + 1000, None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].op, "restore");
        assert_eq!(got[0].result.as_deref(), Some("ok"));
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn only_append_never_rewrites() {
        let _g = sandbox("append");
        let ts = now_ms();
        record(&entry(ts, "scan", "ok")).unwrap();
        record(&entry(ts + 1, "clean", "ok")).unwrap();
        // 文件逐条增长，行数 == 记录数
        let lines = fs::read_to_string(log_dir().join(file_for_ts(ts))).unwrap();
        assert_eq!(lines.lines().count(), 2);
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn daily_rotation_splits_by_day() {
        let _g = sandbox("rotate");
        let day1 = now_ms();
        let day2 = day1 + 86_400_000; // 次日
        record(&entry(day1, "clean", "ok")).unwrap();
        record(&entry(day2, "restore", "ok")).unwrap();
        assert!(log_dir().join(file_for_ts(day1)).is_file());
        assert!(log_dir().join(file_for_ts(day2)).is_file());
        assert_ne!(file_for_ts(day1), file_for_ts(day2));
        // 跨天 query 合并两行
        let all = query(day1 - 1, day2 + 1, None);
        assert_eq!(all.len(), 2);
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn query_filters_by_op_and_range() {
        let _g = sandbox("filter");
        let ts = now_ms();
        record(&entry(ts, "clean", "ok")).unwrap();
        record(&entry(ts + 1000, "restore", "ok")).unwrap();
        let only_restore = query(ts - 1, ts + 99_999, Some("restore"));
        assert_eq!(only_restore.len(), 1);
        assert_eq!(only_restore[0].op, "restore");
        // 范围外裁剪
        let ranged = query(ts + 500, ts + 99_999, None);
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].op, "restore");
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn bad_line_skipped_not_fatal() {
        let _g = sandbox("bad");
        let ts = now_ms();
        record(&entry(ts, "clean", "ok")).unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_dir().join(file_for_ts(ts)))
            .unwrap();
        writeln!(f, "{{not-json}}").unwrap();
        let got = query(ts - 1, ts + 1, None);
        assert_eq!(got.len(), 1);
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn export_writes_all_entries() {
        let _g = sandbox("export");
        let ts = now_ms();
        record(&entry(ts, "clean", "ok")).unwrap();
        record(&entry(ts + 86_400_000, "restore", "ok")).unwrap();
        let out = std::env::temp_dir().join(format!("pureslate-export-{}.jsonl", now_ms()));
        export_all(&out).unwrap();
        let content = fs::read_to_string(&out).unwrap();
        assert_eq!(content.lines().count(), 2);
        let _ = fs::remove_file(&out);
        crate::storage::set_data_root_override(None);
    }
}
