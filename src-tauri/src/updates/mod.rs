//! 更新机制（R22 · SPEC §6.5，P4-01）。
//!
//! - **通道回退链**：`mirrorFirst=true` → jsDelivr → ghproxy → GitHub（raw）直连；
//!   `false` → 直连优先、镜像兜底；任一成功即停；
//! - **sha256 校验**：规则包下载后先校验，失败 → 丢弃 + `rulesPackHashOk=false` + 审计；
//!   **校验通过才落盘**（规则 XML 是可执行内容，红线 #5/#4）；
//! - **安装位置**：`<data_root>\rules\`（安装目录对 asInvoker 只读）；加载时与资源目录
//!   叠加，用户目录 category 覆盖资源目录（loader `load_dirs`，规则包更新语义）；
//! - **opt-in 周查**：7 天窗口（`should_auto_check`），manual 无视 optIn；
//!   周查请求仅含路径参数，无任何标识（红线 #4）；
//! - 网络访问集中在 `http::http_get`（WinHTTP）；回退/校验/周查判定均为注入式纯函数，可单测。

pub mod http;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contract::{Channel, UpdateStatus};
use crate::storage::data_root;

/// 仓库坐标（P4-06 发布流水线定稿时统一调整）。
pub const REPO_SLUG: &str = "pureslate/pureslate";
/// 发布通道 manifest 文件名（GitHub Releases latest 附件 / 仓库内同步文件）。
pub const MANIFEST_PATH: &str = "update-manifest.json";
/// 主分支名（jsDelivr/raw 直连用）。
pub const MAIN_BRANCH: &str = "main";
/// 周查窗口。
pub const WEEKLY_MS: i64 = 7 * 86_400_000;

/// 上游 update-manifest.json 结构（SPEC §6.5；字段宽松解析，缺失按空串）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub app_url: String,
    #[serde(default)]
    pub app_sha256: String,
    #[serde(default)]
    pub rules_version: String,
    #[serde(default)]
    pub rules_url: String,
    #[serde(default)]
    pub rules_sha256: String,
}

/// 规则包结构（自研格式）：sha256 对**整包字节**校验，通过后逐文件落 `<data_root>\rules\`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesPack {
    pub version: String,
    pub files: Vec<RulesPackFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesPackFile {
    pub name: String,
    pub content: String,
}

/// 周查判定（纯函数）：optIn 关 → 否；距上次检查不足 7 天 → 否。
pub fn should_auto_check(opt_in: bool, last_check_at: i64, now: i64) -> bool {
    opt_in && (last_check_at == 0 || now - last_check_at >= WEEKLY_MS)
}

/// 通道 URL 链（纯函数）：按 mirrorFirst 排序，任一成功即停。
/// jsDelivr/ghproxy 记为 mirror 通道，GitHub raw 直连记为 github 通道。
pub fn channel_urls(mirror_first: bool, slug: &str, manifest_path: &str) -> Vec<(Channel, String)> {
    let raw = format!("https://raw.githubusercontent.com/{slug}/{MAIN_BRANCH}/{manifest_path}");
    let jsdelivr = format!("https://cdn.jsdelivr.net/gh/{slug}@{MAIN_BRANCH}/{manifest_path}");
    let ghproxy = format!("https://ghproxy.net/{raw}");
    if mirror_first {
        vec![
            (Channel::Mirror, jsdelivr),
            (Channel::Mirror, ghproxy),
            (Channel::Github, raw),
        ]
    } else {
        vec![
            (Channel::Github, raw),
            (Channel::Mirror, jsdelivr),
            (Channel::Mirror, ghproxy),
        ]
    }
}

/// 通道回退核心（fetch 注入）：依次尝试，任一**取回且可解析**即停；全失败 → None。
/// 解析失败（如 CDN 缓存损坏）同样回退下一通道。
pub fn fetch_manifest_with(
    fetch: &dyn Fn(&str) -> Result<String, String>,
    urls: &[(Channel, String)],
) -> Option<(UpdateManifest, Channel)> {
    for (channel, url) in urls {
        if let Ok(body) = fetch(url) {
            if let Ok(m) = serde_json::from_str::<UpdateManifest>(&body) {
                return Some((m, *channel));
            }
        }
    }
    None
}

/// 版本比较（纯函数）：按 `.` 分段**数值**比较，段数少者不足部分按 0。
/// 任一段非数值（如预发布后缀）→ `None`（无法判定，保守判无更新）。
fn version_cmp(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let pa: Vec<&str> = a.split('.').collect();
    let pb: Vec<&str> = b.split('.').collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let sa = pa.get(i).copied().unwrap_or("0");
        let sb = pb.get(i).copied().unwrap_or("0");
        let (na, nb) = (sa.parse::<u64>().ok()?, sb.parse::<u64>().ok()?);
        let ord = na.cmp(&nb);
        if ord != std::cmp::Ordering::Equal {
            return Some(ord);
        }
    }
    Some(std::cmp::Ordering::Equal)
}

/// 是否存在应用新版本（纯函数；畸形版本号保守判无更新）。
pub fn has_newer_version(current: &str, latest: &str) -> bool {
    !latest.is_empty()
        && version_cmp(latest, current).is_some_and(|o| o == std::cmp::Ordering::Greater)
}

/// 规则包文件名合法性：仅 `[A-Za-z0-9._-]`，防路径穿越（规则包是远端内容）。
fn safe_pack_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with('.')
}

/// 用户规则目录（规则包安装目标；加载时覆盖资源目录同 id category）。
pub fn user_rules_dir() -> PathBuf {
    data_root().join("rules")
}

/// 规则包校验+安装核心（下载注入，测试可控）。
/// 返回 `Ok(true)`=校验通过并已安装；`Ok(false)`=**sha256 不符已丢弃**（未写任何文件）；
/// `Err`=下载/包结构/文件名非法（未安装）。
pub fn verify_and_install_pack_with(
    download: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    pack_url: &str,
    expected_sha256: &str,
) -> Result<bool, String> {
    let bytes = download(pack_url)?;
    let actual = {
        let mut h = Sha256::new();
        h.update(&bytes);
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    // 期望 hash 缺失也按校验失败处理（写入前必须校验通过，SPEC §6.5）。
    if expected_sha256.trim().is_empty() || !actual.eq_ignore_ascii_case(expected_sha256.trim()) {
        audit_update("fail", Some("规则包 sha256 校验失败，已丢弃".into()));
        return Ok(false);
    }

    let pack: RulesPack =
        serde_json::from_slice(&bytes).map_err(|e| format!("规则包结构非法: {e}"))?;
    if pack.files.is_empty() {
        return Err("规则包为空".into());
    }
    for f in &pack.files {
        if !safe_pack_name(&f.name) {
            return Err(format!("规则包文件名非法: {}", f.name));
        }
    }
    // 校验全通过才落盘（部分写入后中断仅留下完整文件，下次扫描可正常加载）。
    let dir = user_rules_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建规则目录失败: {e}"))?;
    for f in &pack.files {
        std::fs::write(dir.join(&f.name), &f.content)
            .map_err(|e| format!("写规则文件失败: {e}"))?;
    }
    audit_update(
        "ok",
        Some(format!(
            "规则包 {} 已安装（{} 个文件）",
            pack.version,
            pack.files.len()
        )),
    );
    Ok(true)
}

/// 上次检查时间持久化：`<data_root>\updates\last-check.json`。
fn last_check_path() -> PathBuf {
    data_root().join("updates").join("last-check.json")
}

#[derive(Serialize, Deserialize)]
struct LastCheck {
    ts: i64,
}

pub fn read_last_check() -> i64 {
    std::fs::read(last_check_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<LastCheck>(&b).ok())
        .map(|c| c.ts)
        .unwrap_or(0)
}

pub fn record_last_check(now: i64) {
    let p = last_check_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec(&LastCheck { ts: now }) {
        let _ = std::fs::write(p, json);
    }
}

/// 完整检查（生产路径，WinHTTP）：manifest 回退链 → 版本比较 → 规则包下载校验安装。
pub fn check(
    current_version: &str,
    current_rules_version: &str,
    mirror_first: bool,
) -> UpdateStatus {
    let now = crate::logging::audit::now_ms();
    let urls = channel_urls(mirror_first, REPO_SLUG, MANIFEST_PATH);
    let fetch = |url: &str| -> Result<String, String> {
        http::http_get(url).map(|b| String::from_utf8_lossy(&b).into_owned())
    };

    let Some((m, channel)) = fetch_manifest_with(&fetch, &urls) else {
        return UpdateStatus {
            current_version: current_version.to_string(),
            latest_version: None,
            has_update: false,
            rules_pack_hash_ok: None,
            channel: Channel::Github,
            checked_at: now,
        };
    };

    let has_update = has_newer_version(current_version, &m.app_version);

    // 规则包：版本不同且给了 URL 才下载（校验失败 → 丢弃 + rulesPackHashOk=false）。
    let mut rules_ok: Option<bool> = None;
    if !m.rules_url.is_empty() && m.rules_version != current_rules_version {
        match verify_and_install_pack_with(&fetch_bytes, &m.rules_url, &m.rules_sha256) {
            Ok(true) => rules_ok = Some(true),
            Ok(false) => rules_ok = Some(false),
            Err(_) => rules_ok = Some(false),
        }
    }

    UpdateStatus {
        current_version: current_version.to_string(),
        latest_version: if m.app_version.is_empty() {
            None
        } else {
            Some(m.app_version)
        },
        has_update,
        rules_pack_hash_ok: rules_ok,
        channel,
        checked_at: now,
    }
}

/// 字节下载适配（复用 http_get；独立函数便于未来对包走不同通道）。
fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    http::http_get(url)
}

/// 审计一条更新操作（op="update"，加性字符串字段，SPEC §4.4 op 集之外的既定扩展）。
fn audit_update(result: &str, detail: Option<String>) {
    let entry = crate::contract::LogEntry {
        ts: crate::logging::audit::now_ms(),
        op: "update".into(),
        tx_id: None,
        category_id: None,
        path: Some(MANIFEST_PATH.into()),
        size_bytes: None,
        disposition: None,
        result: Some(result.into()),
        detail,
    };
    if let Err(e) = crate::logging::audit::record(&entry) {
        eprintln!("[updates] 审计写入失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 沙箱 data_root + 全程持锁（audit/规则包写入落 data_root）。
    struct Sandbox {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Sandbox {
        fn new(tag: &str) -> Self {
            let guard = crate::storage::TEST_DATA_ROOT_LOCK.lock().unwrap();
            let base = std::env::temp_dir().join(format!(
                "pureslate-updates-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&base).unwrap();
            crate::storage::set_data_root_override(Some(base));
            Sandbox { _guard: guard }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let root = data_root();
            crate::storage::set_data_root_override(None);
            // 沙箱根的父目录是临时 base。
            let _ = std::fs::remove_dir_all(root.parent().unwrap_or(&root));
        }
    }

    #[test]
    fn channel_urls_order_by_mirror_first() {
        let mirror = channel_urls(true, "a/b", "m.json");
        assert_eq!(mirror.len(), 3);
        assert_eq!(mirror[0].0, Channel::Mirror);
        assert!(mirror[0].1.contains("cdn.jsdelivr.net"));
        assert!(mirror[1].1.contains("ghproxy.net"));
        assert_eq!(mirror[2].0, Channel::Github);
        assert!(mirror[2].1.contains("raw.githubusercontent.com"));

        let direct = channel_urls(false, "a/b", "m.json");
        assert_eq!(direct[0].0, Channel::Github);
        assert_eq!(direct[1].0, Channel::Mirror);
    }

    #[test]
    fn fetch_falls_through_errors_and_bad_bodies() {
        // 首通道网络失败、次通道返回坏 JSON → 第三通道成功。
        let urls = vec![
            (Channel::Mirror, "u1".into()),
            (Channel::Mirror, "u2".into()),
            (Channel::Github, "u3".into()),
        ];
        let fetch = |url: &str| -> Result<String, String> {
            match url {
                "u1" => Err("网络不可达".into()),
                "u2" => Ok("<html>坏内容</html>".into()),
                _ => Ok(r#"{"appVersion":"0.2.0"}"#.into()),
            }
        };
        let (m, ch) = fetch_manifest_with(&fetch, &urls).expect("第三通道应成功");
        assert_eq!(m.app_version, "0.2.0");
        assert_eq!(ch, Channel::Github);
    }

    #[test]
    fn fetch_all_failed_returns_none() {
        let urls = vec![(Channel::Mirror, "u1".into())];
        let fetch = |_url: &str| -> Result<String, String> { Err("失败".into()) };
        assert!(fetch_manifest_with(&fetch, &urls).is_none());
    }

    #[test]
    fn auto_check_weekly_gate() {
        let now = 1_800_000_000_000;
        assert!(!should_auto_check(false, 0, now), "optIn 关闭不查");
        assert!(should_auto_check(true, 0, now), "从未查过 → 查");
        assert!(
            !should_auto_check(true, now - WEEKLY_MS / 2, now),
            "不足 7 天不查"
        );
        assert!(
            should_auto_check(true, now - WEEKLY_MS, now),
            "满 7 天 → 查"
        );
    }

    #[test]
    fn version_compare() {
        assert!(has_newer_version("0.1.0", "0.2.0"));
        assert!(has_newer_version("0.1.9", "0.1.10"));
        assert!(!has_newer_version("0.2.0", "0.1.9"));
        assert!(!has_newer_version("1.0.0", "1.0.0"));
        assert!(!has_newer_version("1.0.0", ""));
        assert!(!has_newer_version("1.0.0", "畸形"));
    }

    #[test]
    fn pack_hash_mismatch_discards_without_write() {
        let _sb = Sandbox::new("mismatch");
        let pack_json = r#"{"version":"2","files":[{"name":"x.xml","content":"<ruleset/>"}]}"#;
        let download = |_u: &str| -> Result<Vec<u8>, String> { Ok(pack_json.as_bytes().to_vec()) };
        // 错误 hash → Ok(false)（丢弃），且规则目录不得有任何文件。
        let r = verify_and_install_pack_with(&download, "https://x/pack.json", "deadbeef");
        assert_eq!(r, Ok(false));
        assert!(
            !user_rules_dir().exists() || std::fs::read_dir(user_rules_dir()).unwrap().count() == 0
        );
        // 审计有失败记录。
        let now = crate::logging::audit::now_ms();
        let logs = crate::logging::audit::query(now - 60_000, now + 60_000, Some("update"));
        assert!(logs.iter().any(|l| l.result.as_deref() == Some("fail")));
    }

    #[test]
    fn pack_hash_ok_installs_files() {
        let _sb = Sandbox::new("install");
        let pack_json = r#"{"version":"3","files":[{"name":"privacy-traces.xml","content":"<ruleset id=\"p\"></ruleset>"}]}"#;
        // 先算真实 sha256。
        let mut h = Sha256::new();
        h.update(pack_json.as_bytes());
        let sha: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let download =
            move |_u: &str| -> Result<Vec<u8>, String> { Ok(pack_json.as_bytes().to_vec()) };
        let r = verify_and_install_pack_with(&download, "https://x/pack.json", &sha);
        assert_eq!(r, Ok(true));
        let written = user_rules_dir().join("privacy-traces.xml");
        assert!(written.is_file(), "校验通过后须落盘");
        assert!(std::fs::read_to_string(&written)
            .unwrap()
            .contains("ruleset"));
    }

    #[test]
    fn pack_rejects_path_traversal_names() {
        let _sb = Sandbox::new("evil");
        let pack_json = r#"{"version":"4","files":[{"name":"../evil.xml","content":"x"}]}"#;
        let mut h = Sha256::new();
        h.update(pack_json.as_bytes());
        let sha: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let download =
            move |_u: &str| -> Result<Vec<u8>, String> { Ok(pack_json.as_bytes().to_vec()) };
        assert!(verify_and_install_pack_with(&download, "https://x/p.json", &sha).is_err());
        // data_root 之外不得出现 evil.xml。
        assert!(!data_root().parent().unwrap().join("evil.xml").exists());
    }

    #[test]
    fn last_check_roundtrip() {
        let _sb = Sandbox::new("lastcheck");
        assert_eq!(read_last_check(), 0, "未记录过 → 0");
        record_last_check(123456);
        assert_eq!(read_last_check(), 123456);
    }
}
