//! 应用级共享状态（Tauri State）。

use crate::contract::AppSettings;

pub struct AppState {
    pub version: String,
    pub rules_version: String,
    pub settings: AppSettings,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            rules_version: "0".to_string(),
            settings: AppSettings::default(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
