//! 开机诊断（只读）：开机耗时 / 启动项来源 / 卸载残留 / 快速启动与休眠。
//!
//! 设计稿：`docs/dev/DESIGN-BOOTDIAG-2026-11-10.md`。
//! 红线（AGENTS §4）：
//! - **只读**：不写注册表、不删文件、不改电源配置、不启动/停止服务；
//! - 唯一"想写"的场景（开启事件日志通道）本轮**不实现**，非提权一律分段 `denied`；
//! - 错误一律降级为 `BootSection` 状态，不让单个子源失败拖垮整份报告。

use std::time::{SystemTime, UNIX_EPOCH};

use crate::contract::{BootDiagnosisReport, BootSection, BootSectionStatus, BootStartupData};

mod eventlog;
mod power;
mod residue;
mod services;
mod startup_sources;
mod tasks;

/// 采集整份只读诊断报告。
pub fn diagnose() -> BootDiagnosisReport {
    BootDiagnosisReport {
        collected_at: now_rfc3339(),
        is_elevated: crate::scanner::mft::is_admin(),
        // 四段各自带 status，读不到即显式降级，不把空数据当事实。
        timing: eventlog::collect(),
        startup: assemble_startup(),
        residue: residue::collect(),
        power: power::collect(),
    }
}

/// 启动项：注册表类（B1-02）+ 自动服务（B1-04）+ 计划任务（B1-05）。
fn assemble_startup() -> BootSection<BootStartupData> {
    let (mut entries, mut failed) = startup_sources::collect();
    let (service_entries, service_failures) = services::collect();
    entries.extend(service_entries);
    failed.extend(service_failures);
    let (task_entries, task_failures) = tasks::collect();
    entries.extend(task_entries);
    failed.extend(task_failures);
    BootSection::with_reason(
        if failed.is_empty() {
            BootSectionStatus::Ok
        } else {
            BootSectionStatus::Partial
        },
        failed_reason(&failed),
        Some(BootStartupData {
            entries,
            failed_sources: failed,
        }),
    )
}

/// 失败子源 → 人类可读原因（空集合时返回空串，调用方只在 partial 时使用）。
pub(crate) fn failed_reason(failed: &[String]) -> String {
    if failed.is_empty() {
        String::new()
    } else {
        format!("{} 个子源读取失败：{}", failed.len(), failed.join(", "))
    }
}

/// 当前时间 → UTC RFC3339（秒精度），不引入时间库。
fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_rfc3339(secs)
}

/// Unix 秒 → `YYYY-MM-DDTHH:MM:SSZ`（Howard Hinnant civil_from_days）。
pub(crate) fn format_rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)。纯函数，便于单测。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_known_epochs() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(format_rfc3339(1_767_225_600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn diagnose_returns_all_sections() {
        let r = diagnose();
        assert!(!r.collected_at.is_empty());
        // 真机状态取决于提权与系统配置，只断言"有明确结论"，不假设具体状态。
        assert!(matches!(
            r.startup.status,
            BootSectionStatus::Ok | BootSectionStatus::Partial
        ));
        assert!(r.startup.data.is_some());
        assert!(matches!(
            r.residue.status,
            BootSectionStatus::Ok | BootSectionStatus::Partial
        ));
        assert!(r.residue.data.is_some());
        assert!(matches!(
            r.power.status,
            BootSectionStatus::Ok | BootSectionStatus::Partial
        ));
        assert!(r.power.data.is_some());
    }
}
