//! P3-04 隐私清理（R08）集成测试。
//!
//! 验证点（[DESTRUCTIVE] 评审关卡证据，SAFETY §6.3 相关边界）：
//! 1. privacy 维度端到端：规则（History* / *.lnk）→ ScanItem（🟡/quarantine），
//!    含中文+空格文件名（§6.3-1）、0 字节文件（§6.3-8）、非递归目录不误入；
//! 2. 扫描零写盘（红线 #1）；
//! 3. 进程守卫：guard 进程运行中（用本测试进程自身）→ 整类阻止、文件原样（§5.1 不做半清）；
//! 4. 隐私项移入隔离区（沙箱根）：manifest 记 category/grade → 可还原（SAFETY §4.2/4.3）。

use pureslate_lib::cleaner::execute::{execute, CleanTarget};
use pureslate_lib::contract::{Disposition, Grade, ScanDimension, ScanProfile};
use pureslate_lib::quarantine::{move_into_quarantine, restore_one, QuarantineInput};
use pureslate_lib::rules::loader::RuleLoader;
use pureslate_lib::scanner::engine::run_scan;
use pureslate_lib::scanner::walk::CancelToken;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 全程唯一序号，避免并发/重复运行互相干扰。
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 本测试文件内的 DATA_ROOT_OVERRIDE 串行锁（execute/journal 依赖 data_root；
/// 集成测试与 lib 单测分属不同进程，只需在本文件内串行）。
static DATA_ROOT_LOCK: Mutex<()> = Mutex::new(());

fn fresh_base(tag: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ps-privacy-{tag}-{nano:x}-{seq}"));
    fs::create_dir_all(&base).expect("create base dir");
    base
}

/// 写 privacy fixture 规则：两个沙箱 target（模拟 Edge User Data 与 RecentDocs）。
fn write_fixture_rules(rules_dir: &Path, edge_root: &str, recent_root: &str) {
    let esc_edge = edge_root.replace('\\', "\\\\");
    let esc_recent = recent_root.replace('\\', "\\\\");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="privacy-itest" version="1" lang="zh-CN">
  <category id="privacy.edge-history" label="Edge 浏览历史" risk="yellow" disposition="quarantine"
            description="集成测试：历史记录数据库">
    <target type="path" value="{esc_edge}"/>
    <include pattern="**/History*" recursive="true"/>
    <guard process="msedge.exe"/>
  </category>
  <category id="privacy.recent-docs" label="最近文档记录" risk="yellow" disposition="quarantine"
            description="集成测试：最近文档">
    <target type="path" value="{esc_recent}"/>
    <include pattern="*.lnk" recursive="false"/>
    <guard process=""/>
  </category>
</ruleset>
"#
    );
    fs::write(rules_dir.join("privacy-itest.xml"), xml).expect("write fixture rules");
    fs::write(
        rules_dir.join("whitelist.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<whitelist></whitelist>
"#,
    )
    .expect("write fixture whitelist");
}

fn privacy_profile() -> ScanProfile {
    let mut dims = std::collections::HashMap::new();
    dims.insert(ScanDimension::Privacy, true);
    ScanProfile { dimensions: dims }
}

/// 递归快照：相对路径 → 文件字节（零写盘审计用）。
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let entry = entry.expect("dir entry");
            let rel = if prefix.is_empty() {
                entry.file_name().to_string_lossy().into_owned()
            } else {
                format!("{prefix}\\{}", entry.file_name().to_string_lossy())
            };
            let p = entry.path();
            if p.is_dir() {
                stack.push((p, rel));
            } else {
                out.insert(rel, fs::read(&p).expect("read file"));
            }
        }
    }
    out
}

/// 造沙箱样本：Edge 形态目录 + RecentDocs 形态目录。
/// 覆盖 SAFETY §6.3 边界：中文+空格文件名（1）、0 字节文件（8）、子目录不误入（非递归）。
fn make_samples(edge_root: &Path, recent_root: &Path) {
    fs::create_dir_all(edge_root.join("Default")).unwrap();
    fs::create_dir_all(edge_root.join("Profile 1")).unwrap();
    fs::create_dir_all(edge_root.join("Other")).unwrap();
    fs::write(edge_root.join("Default\\History"), b"sqlite-db-main").unwrap();
    fs::write(edge_root.join("Default\\History-journal"), b"wal-bytes").unwrap();
    // 0 字节 History（另一 Profile）+ 空格目录名。
    fs::write(edge_root.join("Profile 1\\History"), b"").unwrap();
    // 非命中文件：缓存类命名不得入候选。
    fs::write(edge_root.join("Other\\Cache_003"), b"cache-data").unwrap();
    fs::write(edge_root.join("Other\\Preferences"), b"{}").unwrap();

    fs::create_dir_all(recent_root.join("CustomDestinations")).unwrap();
    // 中文+点号文件名（RecentDocs 真实形态）。
    fs::write(recent_root.join("机密.公司财务报表.xlsx.lnk"), b"lnk-bytes").unwrap();
    // emoji 文件名（SAFETY §6.3-1：中文/emoji/空格用户名与文件名）。
    fs::write(recent_root.join("🎮游戏存档.lnk"), b"lnk-emoji").unwrap();
    fs::write(recent_root.join("notes.txt"), b"not a trace").unwrap();
    // 子目录 jumplist 不在 *.lnk 非递归范围。
    fs::write(recent_root.join("CustomDestinations\\abc.dest"), b"jump").unwrap();
}

/// privacy 维度端到端：命中 History*（含 0 字节/WAL 伴随）+ 顶层 .lnk（含中文名），
/// 缓存类文件与子目录不误入；全部 🟡/quarantine。
#[test]
fn privacy_scan_finds_history_and_recentdocs() {
    let base = fresh_base("scan");
    let rules_dir = base.join("rules");
    let edge_root = base.join("edge-user-data");
    let recent_root = base.join("recent");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::create_dir_all(&edge_root).unwrap();
    fs::create_dir_all(&recent_root).unwrap();
    write_fixture_rules(
        &rules_dir,
        &edge_root.to_string_lossy(),
        &recent_root.to_string_lossy(),
    );
    make_samples(&edge_root, &recent_root);

    let before_edge = snapshot(&edge_root);
    let before_recent = snapshot(&recent_root);

    let mut loader = RuleLoader::new();
    let table = loader
        .load_dir(&rules_dir)
        .expect("load privacy fixture rules");
    let cancel = CancelToken::new();
    let mut progress = |_phase, _done, _total, _path, _found| {};
    let outcome = run_scan(&table, &privacy_profile(), &cancel, &mut progress);

    // 5 项：Edge 主库 + WAL 伴随 + 0 字节 History + 中文 .lnk + emoji .lnk。
    assert_eq!(outcome.items.len(), 5, "实得: {:#?}", outcome.items);
    let by_cat: std::collections::HashMap<String, usize> =
        outcome
            .items
            .iter()
            .fold(std::collections::HashMap::new(), |mut m, it| {
                *m.entry(it.category_id.clone()).or_insert(0) += 1;
                m
            });
    assert_eq!(by_cat.get("privacy.edge-history"), Some(&3));
    assert_eq!(by_cat.get("privacy.recent-docs"), Some(&2));

    for it in &outcome.items {
        assert_eq!(it.grade, Grade::Yellow, "path={}", it.path);
        assert_eq!(it.disposition, Disposition::Quarantine, "path={}", it.path);
    }
    // 非命中样本不入候选。
    let paths: Vec<String> = outcome
        .items
        .iter()
        .map(|i| i.path.to_lowercase())
        .collect();
    assert!(!paths.iter().any(|p| p.contains("cache_003")));
    assert!(!paths.iter().any(|p| p.contains("preferences")));
    assert!(!paths.iter().any(|p| p.contains("notes.txt")));
    assert!(!paths.iter().any(|p| p.contains("customdestinations")));
    // 中文/emoji .lnk 命中。
    assert!(
        paths
            .iter()
            .any(|p| p.contains("机密.公司财务报表.xlsx.lnk")),
        "中文名 .lnk 必须命中: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.contains("🎮游戏存档.lnk")),
        "emoji .lnk 必须命中: {paths:?}"
    );

    // 红线 #1：扫描零写盘。
    assert_eq!(before_edge, snapshot(&edge_root), "Edge 沙箱零写盘");
    assert_eq!(before_recent, snapshot(&recent_root), "Recent 沙箱零写盘");

    let _ = fs::remove_dir_all(&base);
}

/// 进程守卫（SAFETY §5.1）：guard 进程运行中 → 整类阻止（不做半清），文件原样。
/// 用本测试进程自身作"运行中的浏览器"，保证判定确定命中。
#[test]
fn guard_running_blocks_whole_privacy_category() {
    let _g = DATA_ROOT_LOCK.lock().unwrap();
    let base = fresh_base("guard");
    let data_root = base.join("data-root");
    fs::create_dir_all(&data_root).unwrap();
    pureslate_lib::storage::set_data_root_override(Some(data_root));

    let edge_root = base.join("edge");
    fs::create_dir_all(edge_root.join("Default")).unwrap();
    let h1 = edge_root.join("Default\\History");
    let h2 = edge_root.join("Default\\History-journal");
    fs::write(&h1, b"sqlite-main").unwrap();
    fs::write(&h2, b"wal").unwrap();

    // guard = 当前测试进程 exe 基名（必然在运行，等价"浏览器运行中"）。
    let own_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .expect("current exe");
    let targets = vec![
        CleanTarget {
            path: h1.clone(),
            grade: Grade::Yellow,
            disposition: Disposition::Quarantine,
            category_id: "privacy.edge-history".into(),
            size_bytes: 11,
            guard_process: Some(own_exe),
            mtime_ms: None,
        },
        CleanTarget {
            path: h2.clone(),
            grade: Grade::Yellow,
            disposition: Disposition::Quarantine,
            category_id: "privacy.edge-history".into(),
            size_bytes: 3,
            guard_process: Some("msedge.exe".into()),
            mtime_ms: None,
        },
    ];

    let cancel = CancelToken::new();
    let report = execute(
        "tx-privacy-guard",
        &targets,
        &cancel,
        14,
        &mut |_, _, _, _, _| {},
    );

    // 整类 2 项全部 skip，无一项被移动/删除（不做半清）。
    assert_eq!(report.total, 2);
    assert_eq!(report.skip, 2, "运行中守卫应整类 skip");
    assert_eq!(report.ok, 0);
    assert!(h1.exists(), "守卫命中时文件必须原样保留");
    assert!(h2.exists(), "守卫命中时伴随文件必须原样保留");
    assert!(
        report
            .failures
            .iter()
            .all(|f| f.reason.contains("进程守卫")),
        "失败原因应提示进程守卫: {:?}",
        report.failures
    );

    pureslate_lib::storage::set_data_root_override(None);
    let _ = fs::remove_dir_all(&base);
}

/// 出厂规则包回归：真实 resources/rules 必须全部加载成功（被丢弃的 category 不会
/// 出现在 by_id），隐私三类目在列且 risk/disposition/guard 符合 SAFETY §1/§5.1 设计。
#[test]
fn shipped_privacy_rules_load_clean() {
    let rules_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("rules");
    let mut loader = RuleLoader::new();
    let table = loader.load_dir(&rules_dir).expect("出厂规则包必须可加载");

    let expect = [
        ("privacy.edge-history", "msedge.exe"),
        ("privacy.chrome-history", "chrome.exe"),
        ("privacy.recent-docs", ""),
    ];
    for (id, guard) in expect {
        let (_, cat) = table
            .by_id
            .get(id)
            .unwrap_or_else(|| panic!("出厂规则缺少 {id}（可能被 loader 丢弃）"));
        assert_eq!(cat.risk, pureslate_lib::rules::model::Risk::Yellow, "{id}");
        assert_eq!(
            cat.disposition,
            pureslate_lib::rules::model::Disposition::Quarantine,
            "{id}"
        );
        assert_eq!(cat.guard_process.as_deref().unwrap_or(""), guard, "{id}");
    }
}

/// 隐私项入隔离区（沙箱根，避免触碰真实卷根）→ manifest 记 privacy 类目 → 可还原。
#[test]
fn privacy_item_quarantines_and_restores() {
    let _g = DATA_ROOT_LOCK.lock().unwrap();
    let base = fresh_base("quar");
    let data_root = base.join("data-root");
    fs::create_dir_all(&data_root).unwrap();
    pureslate_lib::storage::set_data_root_override(Some(data_root));

    let recent_root = base.join("recent");
    fs::create_dir_all(&recent_root).unwrap();
    let lnk = recent_root.join("机密.公司财务报表.xlsx.lnk");
    fs::write(&lnk, b"lnk-bytes").unwrap();

    // 沙箱隔离区根（生产为 quarantine_root_of(path)，此处注入避免写真实盘根）。
    let q_root = base.join("qroot");
    let entry = move_into_quarantine(
        &q_root,
        QuarantineInput {
            original_path: lnk.clone(),
            grade: Grade::Yellow,
            category_id: "privacy.recent-docs".into(),
            retention_days: 14,
        },
    )
    .expect("move into quarantine");

    assert!(!lnk.exists(), "移入后原路径不得残留");
    assert_eq!(entry.category_id, "privacy.recent-docs");
    assert_eq!(entry.grade, Grade::Yellow);
    assert_eq!(entry.original_path, lnk.to_string_lossy().as_ref());
    // expires_at = 移入时间 + 14 天。
    assert!(entry.expires_at > entry.moved_at);
    // 沙箱隔离区根与卷根计算函数一致性（根命名）。
    assert!(pureslate_lib::quarantine::quarantine_root_of(&lnk)
        .to_string_lossy()
        .ends_with(".pureslate-quarantine"));

    // 还原：回原路径，逐字节一致。
    let outcome = restore_one(&q_root, &entry);
    assert!(
        matches!(outcome, pureslate_lib::quarantine::RestoreOutcome::Restored),
        "应成功还原，实得 {outcome:?}"
    );
    assert_eq!(fs::read(&lnk).unwrap(), b"lnk-bytes");

    pureslate_lib::storage::set_data_root_override(None);
    let _ = fs::remove_dir_all(&base);
}
