//! 应用级共享状态（Tauri State）。

use std::sync::Mutex;

use crate::contract::{AppSettings, ScanItem, ScanResult};
use crate::scanner::walk::CancelToken;

/// 最近保留的完成扫描数（超出按完成时间丢弃最旧）。
const RETAIN_FINISHED: usize = 8;

pub struct AppState {
    pub version: String,
    pub rules_version: String,
    pub settings: AppSettings,
    /// 扫描会话存储（current：进行中；finished：已完成的滚动历史）。
    pub scans: Mutex<ScanStore>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            rules_version: "0".to_string(),
            settings: AppSettings::default(),
            scans: Mutex::new(ScanStore::default()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// 一次扫描会话的运行时上下文。
#[derive(Default)]
pub struct ScanContext {
    pub scan_id: String,
    pub started_at: i64,
    /// 取消令牌：`scan_cancel` 置位，遍历循环读取。
    pub cancel: CancelToken,
    /// 扫描完成后的条目全量（经 `scan_get_items` 分页拉取）。
    pub items: Vec<ScanItem>,
    /// 扫描完成的最终结果（`scan_done` 事件 payload 同款）。
    pub result: Option<ScanResult>,
}

/// 扫描会话集合。全部经外层 `Mutex` 串行访问。
#[derive(Default)]
pub struct ScanStore {
    /// 进行中的扫描（同刻至多一个）。
    pub current: Option<ScanContext>,
    /// 已完成的滚动历史（最近 `RETAIN_FINISHED` 个）。
    pub finished: Vec<ScanContext>,
}

impl ScanStore {
    /// 登记一个新的进行中扫描。
    pub fn begin(&mut self, ctx: ScanContext) {
        self.current = Some(ctx);
    }

    /// 将进行中扫描转入完成历史（裁剪超量），并回填 result/items。
    pub fn finish(&mut self, result: ScanResult, items: Vec<ScanItem>) {
        if let Some(mut cur) = self.current.take() {
            cur.items = items;
            cur.result = Some(result);
            self.finished.push(cur);
        }
        while self.finished.len() > RETAIN_FINISHED {
            self.finished.remove(0);
        }
    }

    /// 按 scan_id 定位扫描上下文：先在 current，再在 finished（倒序最近优先）。
    pub fn by_id(&self, scan_id: &str) -> Option<&ScanContext> {
        if let Some(cur) = &self.current {
            if cur.scan_id == scan_id {
                return Some(cur);
            }
        }
        self.finished.iter().rev().find(|c| c.scan_id == scan_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(id: &str) -> ScanContext {
        ScanContext {
            scan_id: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn begin_then_finish_moves_to_history() {
        let mut store = ScanStore::default();
        store.begin(ctx("s1"));
        assert!(store.current.is_some());
        let result = ScanResult {
            scan_id: "s1".into(),
            started_at: 0,
            finished_at: 1,
            volume: "C:".into(),
            aggregates: vec![],
            item_count: 0,
            total_bytes: crate::contract::FoundBytes {
                green: 0,
                yellow: 0,
                red: 0,
            },
        };
        store.finish(result, vec![]);
        assert!(store.current.is_none());
        assert_eq!(store.finished.len(), 1);
        assert!(store.by_id("s1").is_some());
    }

    #[test]
    fn finished_history_trims() {
        let mut store = ScanStore::default();
        for i in 0..(RETAIN_FINISHED + 3) {
            store.begin(ctx(&format!("s{i}")));
            let result = ScanResult {
                scan_id: format!("s{i}"),
                started_at: i as i64,
                finished_at: i as i64,
                volume: "C:".into(),
                aggregates: vec![],
                item_count: 0,
                total_bytes: crate::contract::FoundBytes {
                    green: 0,
                    yellow: 0,
                    red: 0,
                },
            };
            store.finish(result, vec![]);
        }
        assert_eq!(store.finished.len(), RETAIN_FINISHED);
        // 最旧 "s0" 被裁掉，最近 "sN" 仍在。
        assert!(store.by_id("s0").is_none());
        assert!(store.by_id(&format!("s{}", RETAIN_FINISHED + 2)).is_some());
        // finished 里最新在当前的最前? 倒序检索：最新在前。
    }
}
