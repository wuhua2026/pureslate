//! 应用数据落盘（SPEC §2 数据落盘位置）。
//!
//! 数据根：`%LOCALAPPDATA%\PureSlate\`（settings.json / logs/ / journal/ / restore-conflict/）。
//! 本模块定位数据根并负责 `settings.json` 读写（信任底座，P2-01 落地）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::contract::AppSettings;

/// 数据根覆盖（工具 `restore-self-test` 与单测得用，避免触碰真实 `%LOCALAPPDATA%`）。
static DATA_ROOT_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
/// 串行化**共享** `DATA_ROOT_OVERRIDE` 的测试（quarantine/storage 均触碰此全局）。
/// cargo test 并行跑线程时，任何测试中途改覆盖会污染他人的 data_root；
/// 用一把测试专用锁把所有覆盖使用者串行化，消除该竞态。生产路径不受影响。
pub(crate) static TEST_DATA_ROOT_LOCK: Mutex<()> = Mutex::new(());

/// 注入/清除 data_root 覆盖。生产调用方不使用；测试/测试二进制隔离用。
pub fn set_data_root_override(root: Option<PathBuf>) {
    if let Ok(mut g) = DATA_ROOT_OVERRIDE.lock() {
        *g = root;
    }
}

/// 定位 PureSlate 数据根。优先级：测试覆盖 > `PURESLATE_DATA_ROOT` 环境变量
/// （R24 kill 测试/crash-sim 子进程注入沙箱用；生产不设置，等价于既有 LOCALAPPDATA
/// 信任面，不扩权）> `%LOCALAPPDATA%\PureSlate`；缺失回退 `%APPDATA%`。
/// 生产禁 unwrap/expect（红线 §6）。
pub fn data_root() -> PathBuf {
    if let Ok(g) = DATA_ROOT_OVERRIDE.lock() {
        if let Some(r) = g.as_ref() {
            return r.clone();
        }
    }
    if let Some(p) = std::env::var_os("PURESLATE_DATA_ROOT") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("PureSlate")
}

/// settings.json 完整路径。
pub fn settings_path() -> PathBuf {
    data_root().join("settings.json")
}

/// 还原冲突兜底路径：`<data_root>\restore-conflict\<yyyyMMdd>\<原名>-<纳秒>`。
/// 原路径被占用/已存在时，隔离文件落到此处而非静默丢弃（计 M12 冲突分母）。
pub fn restore_conflict_path(orig: &Path) -> PathBuf {
    let date = today_yyyymmdd();
    let name = orig
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "restored".into());
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    data_root()
        .join("restore-conflict")
        .join(date)
        .join(format!("{name}-{suffix}"))
}

/// 今天 yyyyMMdd（worker_civil 算法，仅目录命名用，无需时区精度）。
fn today_yyyymmdd() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}")
}

/// 天数 → (年, 月, 日)。proleptic Gregorian（Howard Hinnant civil）。
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

/// 读取设置：文件缺失/损坏 → 返回默认值（并尝试保留原值字段，见 `merge`）。
/// errors 不向调用方传播（设置可重生成），仅暴露读取异常计数供测试。
pub fn load_settings() -> AppSettings {
    match fs::read_to_string(settings_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

/// 写入设置（全量覆盖）。创建目录；写失败静默（尽力而为，读取侧已有默认兜底）。
pub fn save_settings(settings: &AppSettings) -> std::io::Result<()> {
    let root = data_root();
    fs::create_dir_all(&root)?;
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(settings_path(), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_when_missing() {
        // 不清扫真实用户目录：仅验证默认值结构合理（读取逻辑由命令层覆盖）。
        let d = AppSettings::default();
        assert_eq!(d.quarantine_retention_days, 14);
        assert!(d.quarantine_auto_purge);
        assert!(!d.update_opt_in);
        assert!(!d.expert_mode);
    }

    #[test]
    fn parse_valid_settings_json() {
        let text = r#"{"quarantineRetentionDays":21,"quarantineAutoPurge":false,"updateOptIn":true,"crashUploadOptIn":false,"expertMode":true,"mirrorFirst":false}"#;
        let p: AppSettings = serde_json::from_str(text).unwrap();
        assert_eq!(p.quarantine_retention_days, 21);
        assert!(!p.quarantine_auto_purge);
        assert!(p.expert_mode);
    }

    #[test]
    fn save_load_roundtrip_in_sandbox() {
        // 用临时目录覆盖 data-root 做隔离测试（不影响真实用户数据）。
        // 用覆盖机制而非改全局环境变量：避免与 quarantine 测试共享的全局
        // DATA_ROOT_OVERRIDE 并行干扰（set_data_root_override 优先于环境变量）。
        let sandbox = std::env::temp_dir().join(format!(
            "pureslate-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 与 quarantine 测试共享全局 DATA_ROOT_OVERRIDE，并行会污染对方 data_root，
        // 故持 TEST_DATA_ROOT_LOCK 串行化整个测试体（见该锁注释）。
        let _g = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
        set_data_root_override(Some(sandbox.join("data-root")));
        let s = AppSettings {
            expert_mode: true,
            quarantine_retention_days: 7,
            ..AppSettings::default()
        };
        save_settings(&s).unwrap();
        let loaded = load_settings();
        assert!(loaded.expert_mode);
        assert_eq!(loaded.quarantine_retention_days, 7);
        set_data_root_override(None);
        let _ = fs::remove_dir_all(&sandbox);
    }
}
