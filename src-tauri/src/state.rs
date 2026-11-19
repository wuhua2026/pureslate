//! 应用级共享状态（Tauri State）。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;

use crate::contract::{AppSettings, CrashRecoveryReport, ScanItem, ScanResult};
use crate::scanner::walk::CancelToken;

/// 最近保留的完成扫描数（超出按完成时间丢弃最旧）。
const RETAIN_FINISHED: usize = 8;

pub struct AppState {
    pub version: String,
    pub rules_version: String,
    /// 应用设置（Mutex 以支持跨命令原地更新）。
    pub settings: Mutex<AppSettings>,
    /// 扫描会话存储（current：进行中；finished：已完成的滚动历史）。
    pub scans: Mutex<ScanStore>,
    /// R24 崩溃恢复进行中（true 时 `clean_execute` 拒绝新事务，防恢复移动文件与
    /// 新清理竞态）。
    pub recovery_pending: AtomicBool,
    /// 最近一次启动恢复结果（`crash_recovery` 拉取；None/ranAt=0 = 尚未跑）。
    pub recovery_report: Mutex<Option<CrashRecoveryReport>>,
    /// I-2（P4-06）：后端签发的一次性 🔴 二次确认令牌（`confirm_token_issue`
    /// 写入，`clean_execute`/`quarantine_purge` 取出消费）。None = 无待消费令牌。
    pub pending_confirm: Mutex<Option<String>>,
    /// R27-2：进行中的卸载事务（txId → 取消令牌）。完成后移除。
    pub uninstalls: Mutex<HashMap<String, Arc<CancelToken>>>,
    /// R27-3：最近一次成功卸载的上下文（removed/reboot_pending 后写入；
    /// 残留扫描只消费该上下文，不做无上下文孤儿扫描）。
    pub last_uninstall: Mutex<Option<crate::uninstall::residue::ResidueContext>>,
    /// R27-3：最近一次残留扫描快照（clean 时校验候选根必须出自该快照，一次性）。
    pub residue_snapshot: Mutex<Option<crate::uninstall::residue::ResidueSnapshot>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            rules_version: "0".to_string(),
            // P2-01：启动从 settings.json 恢复设置（信任底座持久化）。
            settings: Mutex::new(crate::storage::load_settings()),
            scans: Mutex::new(ScanStore::default()),
            recovery_pending: AtomicBool::new(false),
            recovery_report: Mutex::new(None),
            pending_confirm: Mutex::new(None),
            uninstalls: Mutex::new(HashMap::new()),
            last_uninstall: Mutex::new(None),
            residue_snapshot: Mutex::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// I-2（P4-06）：校验并消费一次性 🔴 二次确认令牌（忽略大小写与首尾空白）。
    /// **取出即失效**——无论匹配与否旧令牌都作废（防暴力重试；重试须重新签发）。
    pub fn consume_pending_confirm(&self, input: &str) -> bool {
        let Ok(mut g) = self.pending_confirm.lock() else {
            return false;
        };
        let Some(stored) = g.take() else {
            return false; // 无待消费令牌（未签发/已消费）
        };
        let norm = |s: &str| s.trim().to_uppercase();
        norm(input) == norm(&stored)
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
    /// 登记一个新的进行中扫描（R32-1 single-flight：同刻只允许一个扫描）。
    /// 已有进行中会话时拒绝——否则两个扫描并行推进，`finish` 会把结果错挂到
    /// 另一会话（AUDIT §3 #1）。取消后仍需等待后台线程收尾（`finish`）才能再开。
    pub fn begin(&mut self, ctx: ScanContext) -> Result<(), String> {
        if self.current.is_some() {
            return Err("已有扫描进行中，请等待其完成或取消后再试".into());
        }
        self.current = Some(ctx);
        Ok(())
    }

    /// 将指定 scan_id 的进行中扫描转入完成历史（裁剪超量），并回填 result/items。
    /// R32-1：按 id 匹配才摘取 current；id 不符一律不动（防迟到的收尾线程
    /// 把另一会话的上下文摘走）。返回是否实际完成。
    pub fn finish(&mut self, scan_id: &str, result: ScanResult, items: Vec<ScanItem>) -> bool {
        let matches = self
            .current
            .as_ref()
            .is_some_and(|cur| cur.scan_id == scan_id);
        if !matches {
            return false;
        }
        if let Some(mut cur) = self.current.take() {
            cur.items = items;
            cur.result = Some(result);
            self.finished.push(cur);
        }
        while self.finished.len() > RETAIN_FINISHED {
            self.finished.remove(0);
        }
        true
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
    fn consume_pending_confirm_is_one_shot() {
        // I-2（P4-06）：令牌一次性消费——命中后立即失效；未命中同样作废
        // （防暴力重试，重试须重新签发）。
        let state = AppState::new();
        *state.pending_confirm.lock().unwrap() = Some("PS-ABCD-EFGH".into());
        assert!(state.consume_pending_confirm(" ps-abcd-efgh "));
        assert!(
            !state.consume_pending_confirm("PS-ABCD-EFGH"),
            "已消费不得复用"
        );

        *state.pending_confirm.lock().unwrap() = Some("PS-AAAA-BBBB".into());
        assert!(
            !state.consume_pending_confirm("PS-WRNG-TOKN"),
            "未命中返回 false"
        );
        assert!(
            !state.consume_pending_confirm("PS-AAAA-BBBB"),
            "未命中的令牌也已作废"
        );
    }

    #[test]
    fn begin_then_finish_moves_to_history() {
        let mut store = ScanStore::default();
        store.begin(ctx("s1")).unwrap();
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
            whitelist_ok: None,
        };
        assert!(store.finish("s1", result, vec![]));
        assert!(store.current.is_none());
        assert_eq!(store.finished.len(), 1);
        assert!(store.by_id("s1").is_some());
    }

    #[test]
    fn finished_history_trims() {
        let mut store = ScanStore::default();
        for i in 0..(RETAIN_FINISHED + 3) {
            store.begin(ctx(&format!("s{i}"))).unwrap();
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
                whitelist_ok: None,
            };
            store.finish(&format!("s{i}"), result, vec![]);
        }
        assert_eq!(store.finished.len(), RETAIN_FINISHED);
        // 最旧 "s0" 被裁掉，最近 "sN" 仍在。
        assert!(store.by_id("s0").is_none());
        assert!(store.by_id(&format!("s{}", RETAIN_FINISHED + 2)).is_some());
        // finished 里最新在当前的最前? 倒序检索：最新在前。
    }

    #[test]
    fn begin_rejects_second_scan_until_finished() {
        // R32-1 single-flight：进行中不得再开第二个扫描。
        let mut store = ScanStore::default();
        store.begin(ctx("s1")).unwrap();
        assert!(store.begin(ctx("s2")).is_err(), "已有 current 时必须拒绝");
        assert_eq!(
            store.current.as_ref().map(|c| c.scan_id.as_str()),
            Some("s1")
        );
        // 完成 s1 后可再开。
        assert!(store.finish("s1", test_result("s1"), vec![]));
        store.begin(ctx("s2")).unwrap();
    }

    #[test]
    fn finish_with_mismatched_id_leaves_current_untouched() {
        // R32-1：迟到的收尾线程不得摘走另一会话。
        let mut store = ScanStore::default();
        store.begin(ctx("s2")).unwrap();
        assert!(!store.finish("s1", test_result("s1"), vec![]));
        assert_eq!(
            store.current.as_ref().map(|c| c.scan_id.as_str()),
            Some("s2")
        );
        assert!(store.finished.is_empty());
    }

    fn test_result(scan_id: &str) -> ScanResult {
        ScanResult {
            scan_id: scan_id.into(),
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
            whitelist_ok: None,
        }
    }
}
