//! 两阶段事务 journal（R04 · SAFETY §3）。
//!
//! 落盘 `%LOCALAPPDATA%\PureSlate\journal\<txId>.jsonl`，逐文件：
//! `{"seq":N,"phase":"intent","op":"delete","path":"...","disposition":"...","ts":...}`
//! `{"seq":N,"phase":"result","op":"delete","path":"...","ok":true,"ts":...}`
//!
//! 规则：
//! 1. 操作前写 intent，操作后写 result（O_APPEND 只追加、禁重写整文件）；
//! 2. 崩溃后启动扫描：无对应 result 的 intent = orphan，按去向执行恢复
//!    （quarantine 未完成→还原；recycle/direct→记 orphan，损害可控 + UI 提示）；
//! 3. 单文件失败不中断事务：记 `result.ok=false`，汇总报告。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::contract::Disposition;
use crate::storage::data_root;

/// journal 根：`<data_root>\journal`。
pub fn journal_dir() -> PathBuf {
    data_root().join("journal")
}

/// 单事务 journal 文件路径。
pub fn journal_path(tx_id: &str) -> PathBuf {
    journal_dir().join(format!("{tx_id}.jsonl"))
}

/// 追加写一行（O_APPEND）。调用前由调用方保证目录已建。
fn append_line(file: &mut fs::File, line: &str) -> std::io::Result<()> {
    writeln!(file, "{line}")?;
    file.flush()
}

/// 事务写入器（持开着的目标文件，自增 seq，只追加）。
pub struct Journal {
    tx_id: String,
    file: fs::File,
    seq: u64,
}

impl Journal {
    /// 打开（创建）单事务 journal 文件。目录缺失则自动创建。
    /// R24 启动恢复会向**既有**事务文件追加 result 行闭环——此时从既有最大 seq
    /// 续编（保持事务内 seq 单调；新事务文件从 0 起，生产行为不变）。
    pub fn open(tx_id: &str) -> std::io::Result<Self> {
        let dir = journal_dir();
        fs::create_dir_all(&dir)?;
        let p = journal_path(tx_id);
        let mut seq = 0u64;
        if p.exists() {
            if let Ok(text) = fs::read_to_string(&p) {
                for line in text.lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(s) = v["seq"].as_u64() {
                            seq = seq.max(s);
                        }
                    }
                }
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(p)?;
        Ok(Self {
            tx_id: tx_id.to_string(),
            file,
            seq,
        })
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// 写 intent 行（操作前）。
    pub fn intent(&mut self, path: &Path, disposition: Disposition) -> std::io::Result<()> {
        let seq = self.next_seq();
        let line = serde_json::json!({
            "seq": seq,
            "phase": "intent",
            "op": "delete",
            "path": path.to_string_lossy(),
            "disposition": disposition,
            "ts": crate::logging::audit::now_ms(),
        });
        append_line(&mut self.file, &line.to_string())
    }

    /// 写 result 行（操作后）。`ok=true` 成功，否则 `ok=false` 且 detail 记原因。
    pub fn result(&mut self, path: &Path, ok: bool, detail: Option<&str>) -> std::io::Result<()> {
        let seq = self.next_seq();
        let mut obj = serde_json::json!({
            "seq": seq,
            "phase": "result",
            "op": "delete",
            "path": path.to_string_lossy(),
            "ok": ok,
            "ts": crate::logging::audit::now_ms(),
        });
        if let Some(d) = detail {
            obj["detail"] = serde_json::Value::String(d.to_string());
        }
        append_line(&mut self.file, &obj.to_string())
    }

    /// 当前事务 id。
    pub fn tx_id(&self) -> &str {
        &self.tx_id
    }
}

/// 一个孤儿 intent：有 intent 无对应 result。崩溃恢复用。
#[derive(Debug, Clone)]
pub struct OrphanIntent {
    pub tx_id: String,
    pub path: String,
    pub disposition: Disposition,
}

/// 扫描全部 journal：发现无 result 的 intent，归为孤儿。
/// 以「同 tx 内 intent 后无任何该 path 的 result 行」判定。
pub fn detect_orphans() -> Vec<OrphanIntent> {
    let dir = journal_dir();
    let Ok(rd) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut orphans = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !name.ends_with(".jsonl") {
            continue;
        }
        let tx_id = name.trim_end_matches(".jsonl").to_string();
        let Ok(lines) = fs::read_to_string(&path) else {
            continue;
        };
        // 记录每个 path 出现的 intent 位置集合与其后是否有 result。
        use std::collections::HashMap;
        #[derive(Default)]
        struct Track {
            had_intent: bool,
            had_result: bool,
            disposition: Option<Disposition>,
        }
        let mut map: HashMap<String, Track> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for line in lines.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let phase = v["phase"].as_str().unwrap_or_default();
            let p = v["path"].as_str().unwrap_or_default();
            let disp = v["disposition"].as_str().and_then(|s| match s {
                "direct" => Some(Disposition::Direct),
                "recycle" => Some(Disposition::Recycle),
                "quarantine" => Some(Disposition::Quarantine),
                _ => None,
            });
            let t = map.entry(p.to_string()).or_default();
            if !t.had_intent {
                order.push(p.to_string());
            }
            match phase {
                "intent" => {
                    t.had_intent = true;
                    t.disposition = disp;
                }
                "result" => t.had_result = true,
                _ => {}
            }
        }
        for p in order {
            let t = &map[&p];
            if t.had_intent && !t.had_result {
                if let Some(d) = t.disposition {
                    orphans.push(OrphanIntent {
                        tx_id: tx_id.clone(),
                        path: p,
                        disposition: d,
                    });
                }
            }
        }
    }
    orphans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> std::sync::MutexGuard<'static, ()> {
        let g = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
        let d = std::env::temp_dir().join(format!(
            "pureslate-journal-{tag}-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        crate::storage::set_data_root_override(Some(d.join("data-root")));
        g
    }

    #[allow(dead_code)] // 测试工具人，保留给后续孤儿恢复/整合测试复用
    fn disp(s: &str) -> Disposition {
        match s {
            "quarantine" => Disposition::Quarantine,
            "recycle" => Disposition::Recycle,
            _ => Disposition::Direct,
        }
    }

    #[test]
    fn intent_only_is_orphan() {
        let _g = sandbox("orphan");
        let mut j = Journal::open("t1").unwrap();
        j.intent(Path::new("C:\\a\\x.tmp"), Disposition::Quarantine)
            .unwrap();
        drop(j);
        let o = detect_orphans();
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].tx_id, "t1");
        assert_eq!(o[0].disposition, Disposition::Quarantine);
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn intent_with_result_is_not_orphan() {
        let _g = sandbox("ok");
        let mut j = Journal::open("t2").unwrap();
        let p = Path::new("C:\\a\\y.tmp");
        j.intent(p, Disposition::Direct).unwrap();
        j.result(p, true, None).unwrap();
        drop(j);
        assert!(detect_orphans().is_empty());
        crate::storage::set_data_root_override(None);
    }

    #[test]
    fn failed_result_is_not_orphan() {
        let _g = sandbox("fail");
        let mut j = Journal::open("t3").unwrap();
        let p = Path::new("C:\\a\\z.tmp");
        j.intent(p, Disposition::Recycle).unwrap();
        j.result(p, false, Some("权限不足")).unwrap();
        drop(j);
        assert!(detect_orphans().is_empty());
        crate::storage::set_data_root_override(None);
    }
}
