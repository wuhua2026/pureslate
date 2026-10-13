//! R27-3 卸载残留清理端到端（[DESTRUCTIVE] 评审关卡证据，SAFETY §6.3）。
//!
//! 验证点：
//! 1. 沙箱"安装目录"经 scan_residue → prepare_targets → execute(quarantine)
//!    全链路：文件移入沙箱隔离区、manifest 记 category=residue.uninstall、
//!    journal 无孤儿、空目录收尾删净候选目录；
//! 2. 隔离区还原：文件回原路径且父目录链重建（目录被删后仍可还原）；
//! 3. direct 去向：文件永久删除 + journal/审计，空目录收尾；
//! 4. 空目录候选（0 文件）：不经 execute，prune 直接删目录；
//! 5. 唯一产品名避免在真实开始菜单/AppData 根产生噪声候选。
//!
//! 隔离区根与数据根均注入沙箱（不写真实盘根）。

use pureslate_lib::cleaner::execute::execute;
use pureslate_lib::cleaner::RESIDUE_CATEGORY_ID;
use pureslate_lib::contract::Disposition;
use pureslate_lib::quarantine::{
    load_manifest, restore_one, set_quarantine_root_override, ManifestState, RestoreOutcome,
};
use pureslate_lib::scanner::walk::CancelToken;
use pureslate_lib::uninstall::residue::{
    prepare_targets, prune_empty_dirs, scan_residue, ResidueContext, ResidueSelection,
};

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 本文件共享全局（data root / quarantine root 覆盖）串行锁。
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Sandbox {
    base: PathBuf,
    qroot: PathBuf,
    _g: MutexGuard<'static>,
}

fn lock() -> MutexGuard<'static> {
    GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// 引用标准库守卫类型别名。
type MutexGuard<'a> = std::sync::MutexGuard<'a, ()>;

impl Sandbox {
    fn new(tag: &str) -> Self {
        let g = lock();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let nano = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ps-residue-{tag}-{nano:x}-{seq}"));
        let data_root = base.join("data-root");
        let qroot = base.join("qroot");
        fs::create_dir_all(&data_root).unwrap();
        fs::create_dir_all(&qroot).unwrap();
        pureslate_lib::storage::set_data_root_override(Some(data_root));
        set_quarantine_root_override(Some(qroot.clone()));
        Sandbox { base, qroot, _g: g }
    }

    fn install_dir(&self, name: &str) -> PathBuf {
        self.base.join("installs").join(name)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        pureslate_lib::storage::set_data_root_override(None);
        set_quarantine_root_override(None);
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// 唯一产品名（避免真实开始菜单/AppData 根里同名噪声；同时测中文/空格路径）。
fn unique_product(tag: &str) -> String {
    format!("Ps残留测试 {tag} {}", now_ms())
}

fn write(p: &std::path::Path, body: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn run_execute(
    targets: &[pureslate_lib::cleaner::CleanTarget],
) -> pureslate_lib::cleaner::CleanReport {
    let cancel = CancelToken::new();
    let tx = format!("residue-it-{}", now_ms());
    execute(&tx, targets, &cancel, 14, &mut |_, _, _, _, _| {})
}

/// ① quarantine 全链路：移入隔离区 → manifest/journal 齐备 → 目录收尾 → 可还原。
#[test]
fn quarantine_roundtrip_with_dir_prune_and_restore() {
    let sb = Sandbox::new("q");
    let product = unique_product("q");
    let loc = sb.install_dir("LegacyProductQ");
    let f1 = loc.join("legacyapp.exe");
    let f2 = loc.join("lang").join("zh-cn.dll");
    let empty_sub = loc.join("empty-sub");
    write(&f1, "exe-body");
    write(&f2, "dll-body");
    fs::create_dir_all(&empty_sub).unwrap();

    // ① 枚举（install_location 是注册表外部值，走严格安全闸；沙箱路径合规）。
    let ctx = ResidueContext {
        app_id: "app-x".into(),
        name: product.clone(),
        publisher: Some("PsTestVendor".into()),
        install_location: Some(loc.to_string_lossy().into_owned()),
        uninstalled_at: now_ms(),
    };
    let cands = scan_residue(&ctx).expect("枚举成功");
    let c = cands
        .iter()
        .find(|c| c.kind.as_str() == "install_dir")
        .expect("应有安装目录候选");
    assert_eq!(c.file_count, 2, "空目录不计文件，实得: {cands:?}");
    assert!(c.reason.contains("安装目录"));
    // 唯一产品名不应在真实开始菜单/AppData 产生噪声候选。
    assert!(
        cands
            .iter()
            .all(|c| c.kind.as_str() == "install_dir" || c.file_count == 0),
        "真实系统根不应有同名噪声: {cands:?}"
    );

    // ② 隔离区去向执行。
    let prepped = prepare_targets(
        &snapshot(&ctx, cands.clone()),
        &[ResidueSelection {
            candidate_id: c.id.clone(),
            disposition: Disposition::Quarantine,
        }],
    )
    .expect("prepare");
    assert_eq!(prepped.targets.len(), 2);
    assert!(prepped
        .targets
        .iter()
        .all(|t| t.category_id == RESIDUE_CATEGORY_ID && t.authorized_root.is_some()));

    let report = run_execute(&prepped.targets);
    assert_eq!(report.ok, 2, "应全部成功: {report:?}");
    assert_eq!(report.fail, 0);
    assert!(!f1.exists() && !f2.exists(), "源文件应已移入隔离区");

    // manifest 两行，类目均为 residue.uninstall。
    let (entries, _) = load_manifest(&sb.qroot).expect("manifest");
    let moved: Vec<_> = entries
        .iter()
        .filter(|e| e.state == ManifestState::Quarantined)
        .collect();
    assert_eq!(moved.len(), 2);
    assert!(moved.iter().all(|e| e.category_id == RESIDUE_CATEGORY_ID));

    // journal 无孤儿。
    assert!(
        pureslate_lib::cleaner::journal::detect_orphans().is_empty(),
        "两阶段 journal 必须闭环"
    );

    // ③ 空目录收尾：含空的 empty-sub 与候选根本身全部删净。
    prune_empty_dirs(&prepped.roots);
    assert!(!loc.exists(), "文件清空后候选目录应被收尾删除");

    // ④ 还原：文件回原路径，父目录链自动重建（目录已被删）。
    for e in &moved {
        let out = restore_one(&sb.qroot, e);
        assert!(
            matches!(out, RestoreOutcome::Restored),
            "应成功还原: {out:?}"
        );
    }
    assert_eq!(fs::read(&f1).unwrap(), b"exe-body");
    assert_eq!(fs::read(&f2).unwrap(), b"dll-body");
}

/// ② direct 去向：永久删除 + journal/审计 + 空目录收尾。
#[test]
fn direct_disposition_deletes_permanently() {
    let sb = Sandbox::new("d");
    let product = unique_product("d");
    let loc = sb.install_dir("LegacyProductD");
    let f = loc.join("legacyapp-d.exe");
    write(&f, "bye");

    let ctx = ResidueContext {
        app_id: "app-d".into(),
        name: product,
        publisher: None,
        install_location: Some(loc.to_string_lossy().into_owned()),
        uninstalled_at: now_ms(),
    };
    let cands = scan_residue(&ctx).expect("枚举成功");
    let c = cands
        .iter()
        .find(|c| c.kind.as_str() == "install_dir")
        .expect("安装目录候选");
    let prepped = prepare_targets(
        &snapshot(&ctx, cands.clone()),
        &[ResidueSelection {
            candidate_id: c.id.clone(),
            disposition: Disposition::Direct,
        }],
    )
    .expect("prepare");
    let report = run_execute(&prepped.targets);
    assert_eq!(report.ok, 1, "{report:?}");
    assert!(!f.exists(), "direct 应永久删除");
    prune_empty_dirs(&prepped.roots);
    assert!(!loc.exists());
    assert!(pureslate_lib::cleaner::journal::detect_orphans().is_empty());
    // 审计覆盖（op=clean，residue 类目）。
    let logs =
        pureslate_lib::logging::audit::query(now_ms() - 60_000, now_ms() + 1_000, Some("clean"));
    assert!(
        logs.iter().any(|l| l
            .category_id
            .as_deref()
            .is_some_and(|c| c == RESIDUE_CATEGORY_ID)),
        "direct 残留删除必须有审计记录"
    );
}

/// ③ 空目录候选（0 文件）：无 execute 目标，空目录收尾直接删除候选根。
#[test]
fn empty_dir_candidate_pruned_without_execute() {
    let sb = Sandbox::new("empty");
    let product = unique_product("empty");
    let loc = sb.install_dir("EmptyLegacy");
    fs::create_dir_all(&loc).unwrap();

    let ctx = ResidueContext {
        app_id: "app-e".into(),
        name: product,
        publisher: None,
        install_location: Some(loc.to_string_lossy().into_owned()),
        uninstalled_at: now_ms(),
    };
    let cands = scan_residue(&ctx).expect("枚举成功");
    let c = cands
        .iter()
        .find(|c| c.kind.as_str() == "install_dir")
        .expect("空目录也应成候选");
    assert_eq!(c.file_count, 0);
    let prepped = prepare_targets(
        &snapshot(&ctx, cands.clone()),
        &[ResidueSelection {
            candidate_id: c.id.clone(),
            disposition: Disposition::Quarantine,
        }],
    )
    .expect("prepare");
    assert!(prepped.targets.is_empty());
    assert_eq!(prepped.roots.len(), 1);
    // 与 IPC 一致：targets 空则不跑 execute，直接空目录收尾。
    prune_empty_dirs(&prepped.roots);
    assert!(!loc.exists(), "空安装目录应被删除");
}

/// 构造快照（与 state::residue_snapshot 同构）。
fn snapshot(
    ctx: &ResidueContext,
    candidates: Vec<pureslate_lib::uninstall::residue::ResidueCandidate>,
) -> pureslate_lib::uninstall::residue::ResidueSnapshot {
    pureslate_lib::uninstall::residue::ResidueSnapshot {
        context: ctx.clone(),
        candidates,
        scanned_at: now_ms(),
    }
}
