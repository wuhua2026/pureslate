//! 启动项影响估算（R07 启发式：位置 + 命令特征）。纯函数，便于单测。

use crate::contract::{Impact, StartupSource};

/// 常驻类特征：命中升一档（后台代理/同步，持续占用资源）。
const RESIDENT_KEYWORDS: [&str; 8] = [
    "--background",
    "autostart",
    "agent",
    "cloud",
    "sync",
    "helper",
    "daemon",
    "service",
];

/// 瞬时类特征：命中降一档（更新检查/安装器，跑完即退）。
const TRANSIENT_KEYWORDS: [&str; 6] =
    ["update", "updater", "installer", "setup", "install", "/run"];

/// 基准档位：机器级 > 用户级；登录触发的计划任务多为一次性动作，基准最低。
fn base_level(source: StartupSource) -> Impact {
    match source {
        StartupSource::HklmRun => Impact::High,
        StartupSource::HkcuRun | StartupSource::StartupFolder => Impact::Medium,
        StartupSource::TaskScheduler => Impact::Low,
    }
}

fn step_up(i: Impact) -> Impact {
    match i {
        Impact::Low => Impact::Medium,
        _ => Impact::High,
    }
}

fn step_down(i: Impact) -> Impact {
    match i {
        Impact::High => Impact::Medium,
        _ => Impact::Low,
    }
}

/// 估算启动影响：基准（来源位置）+ 命令/名称特征修正。
pub fn estimate_impact(source: StartupSource, command: &str, name: &str) -> Impact {
    let text = format!("{name} {command}").to_lowercase();
    let mut level = base_level(source);
    if RESIDENT_KEYWORDS.iter().any(|k| text.contains(k)) {
        level = step_up(level);
    }
    if TRANSIENT_KEYWORDS.iter().any(|k| text.contains(k)) {
        level = step_down(level);
    }
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hklm_base_high() {
        assert_eq!(
            estimate_impact(StartupSource::HklmRun, "C:\\app\\tool.exe", "Tool"),
            Impact::High
        );
    }

    #[test]
    fn hkcu_base_medium() {
        assert_eq!(
            estimate_impact(StartupSource::HkcuRun, "C:\\app\\tool.exe", "Tool"),
            Impact::Medium
        );
    }

    #[test]
    fn resident_keyword_bumps_up() {
        assert_eq!(
            estimate_impact(
                StartupSource::HkcuRun,
                "C:\\x\\sync.exe --background",
                "CloudSync"
            ),
            Impact::High
        );
    }

    #[test]
    fn transient_keyword_steps_down() {
        assert_eq!(
            estimate_impact(
                StartupSource::HkcuRun,
                "C:\\x\\GoogleUpdate.exe",
                "GoogleUpdate"
            ),
            Impact::Low
        );
        assert_eq!(
            estimate_impact(StartupSource::HklmRun, "C:\\x\\Update.exe", "Vendor Update"),
            Impact::Medium
        );
    }

    #[test]
    fn task_scheduler_base_low() {
        assert_eq!(
            estimate_impact(StartupSource::TaskScheduler, "C:\\x\\log.bat", "LogTask"),
            Impact::Low
        );
    }

    #[test]
    fn both_keywords_cancel_out() {
        // 常驻 + 瞬时特征并存：先升后降，回到基准。
        assert_eq!(
            estimate_impact(StartupSource::HkcuRun, "C:\\x\\agent_update.exe", "X"),
            Impact::Medium
        );
    }
}
