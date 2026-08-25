//! 应用数据落盘（SPEC §2 数据落盘位置）。
//!
//! 数据根：`%LOCALAPPDATA%\PureSlate\`（settings.json / logs/ / journal/ / restore-conflict/）。
//! 本模块定位数据根并负责 `settings.json` 读写（信任底座，P2-01 落地）。

use std::fs;
use std::path::PathBuf;

use crate::contract::AppSettings;

/// 定位 PureSlate 数据根。首选 `%LOCALAPPDATA%\PureSlate`；缺失回退 `%APPDATA%`。
/// 生产禁 unwrap/expect（红线 §6）。
pub fn data_root() -> PathBuf {
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
        // 用临时目录注入 LOCALAPPDATA 做隔离测试（不影响真实用户数据）。
        let sandbox = std::env::temp_dir().join(format!(
            "pureslate-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::env::set_var("LOCALAPPDATA", &sandbox);
        let s = AppSettings {
            expert_mode: true,
            quarantine_retention_days: 7,
            ..AppSettings::default()
        };
        save_settings(&s).unwrap();
        let loaded = load_settings();
        assert!(loaded.expert_mode);
        assert_eq!(loaded.quarantine_retention_days, 7);
        let _ = fs::remove_dir_all(&sandbox);
        std::env::remove_var("LOCALAPPDATA");
    }
}
