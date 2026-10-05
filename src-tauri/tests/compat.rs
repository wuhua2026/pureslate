//! P4-02 兼容加固（R23）集成测试——SAFETY §6.3 八项边界全链路（扫描→清理→还原）。
//!
//! 1. 中文/emoji/空格用户名与路径；2. >260 长路径（`\\?\` 扩展前缀）；3. junction 环 +
//!    扫描后替换（双用例，T-1）；4. 占用文件；5. 只读属性；6. 盘满等价（隔离区根不可用）；
//! 7. 中途 kill 等价（journal 孤儿检测）；8. 空文件/空目录/255 字节文件名。
//!
//! 另含 T-2 还原信任链负例与 F-1 fail-closed。
//! 跑法：`cargo test --test compat`。

use pureslate_lib::cleaner::execute::{execute, resolve_targets, CleanTarget};
use pureslate_lib::contract::{Disposition, Grade, ScanDimension, ScanProfile};
use pureslate_lib::quarantine::restore_one;
use pureslate_lib::quarantine::RestoreOutcome;
use pureslate_lib::rules::loader::RuleLoader;
use pureslate_lib::rules::model::RuleSetTable;
use pureslate_lib::scanner::engine::run_scan;
use pureslate_lib::scanner::walk::CancelToken;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 本文件内 DATA_ROOT_OVERRIDE 串行锁（journal/审计落 data_root）。
/// 抗中毒：某用例失败持锁 panic 不应级联废掉其余用例。
static DATA_ROOT_LOCK: Mutex<()> = Mutex::new(());

fn lock_data_root() -> std::sync::MutexGuard<'static, ()> {
    DATA_ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn fresh_base(tag: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ps-compat-{tag}-{nano:x}-{seq}"));
    fs::create_dir_all(&base).expect("create base");
    base
}

/// data_root 沙箱（全程持锁）。返回 (base, data_root)。
fn sandbox(tag: &str) -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
    let guard = lock_data_root();
    let base = fresh_base(tag);
    let data_root = base.join("data-root");
    fs::create_dir_all(&data_root).unwrap();
    pureslate_lib::storage::set_data_root_override(Some(data_root));
    (base, guard)
}

fn write_fixture_rules(rules_dir: &Path, target_dir: &str) {
    // 注意：XML 属性中反斜杠无须转义；原样写入（双写会破坏 `\\?\` 扩展前缀——
    // Win32 不归一化扩展长度路径，boundary_2 曾因此 0 命中）。
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="compat" version="1" lang="zh-CN">
  <category id="temp.compat" label="兼容测试" risk="green" disposition="direct"
            description="兼容边界测试">
    <target type="path" value="{target_dir}"/>
    <include pattern="**\*.tmp" recursive="true"/>
    <guard process=""/>
  </category>
</ruleset>
"#
    );
    fs::write(rules_dir.join("compat.xml"), xml).expect("write rules");
    fs::write(
        rules_dir.join("whitelist.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<whitelist></whitelist>
"#,
    )
    .expect("write whitelist");
}

fn temp_profile() -> ScanProfile {
    let mut dims = std::collections::HashMap::new();
    dims.insert(ScanDimension::Temp, true);
    ScanProfile { dimensions: dims }
}

/// 扫描 fixture 目标目录，返回 (规则表, items)。
fn scan_target(target_dir: &Path) -> (RuleSetTable, Vec<pureslate_lib::contract::ScanItem>) {
    let rules_dir = target_dir.parent().unwrap().join("rules");
    fs::create_dir_all(&rules_dir).unwrap();
    write_fixture_rules(&rules_dir, &target_dir.to_string_lossy());
    let mut loader = RuleLoader::new();
    let table = loader
        .load_dirs(std::slice::from_ref(&rules_dir))
        .expect("load rules");
    let cancel = CancelToken::new();
    let mut progress = |_phase, _done, _total, _path, _found| {};
    let outcome = run_scan(&table, &temp_profile(), &cancel, &mut progress);
    (table, outcome.items)
}

/// 按扫描产物执行清理（真实规则表 → guard/类目信息齐全）。
fn clean_scanned(
    tx: &str,
    table: &RuleSetTable,
    items: &[pureslate_lib::contract::ScanItem],
    ids: &[String],
) -> pureslate_lib::cleaner::CleanReport {
    let targets = resolve_targets(items, ids, table).expect("resolve targets");
    let cancel = CancelToken::new();
    execute(tx, &targets, &cancel, 14, &mut |_, _, _, _, _| {})
}

fn modified_ms(p: &Path) -> i64 {
    fs::metadata(p)
        .unwrap()
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(-1)
}

fn direct_target(path: &Path) -> CleanTarget {
    CleanTarget {
        path: path.to_path_buf(),
        grade: Grade::Green,
        disposition: Disposition::Direct,
        category_id: "temp.compat".into(),
        size_bytes: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        guard_processes: Vec::new(),
        mtime_ms: Some(modified_ms(path)),
    }
}

/// §6.3-1：中文/emoji/空格用户名与路径全链路（扫描→清理）。
#[test]
fn boundary_1_chinese_emoji_space_paths() {
    let (base, _g) = sandbox("b1");
    let target = base.join("用户 小明 🎮\\AppData\\Local\\Temp");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("缓存 残留.tmp"), b"x").unwrap();
    fs::write(target.join("日本語テスト.tmp"), b"y").unwrap();

    let (table, items) = scan_target(&target);
    assert_eq!(items.len(), 2, "中文名文件必须命中: {:#?}", items);

    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let report = clean_scanned("tx-b1", &table, &items, &ids);
    assert_eq!(report.ok, 2, "全链路清理应成功: {:?}", report.failures);
    assert!(!target.join("缓存 残留.tmp").exists());

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-2：>260 长路径（\\?\ 扩展长度前缀）扫描与清理。
#[test]
fn boundary_2_long_paths_over_260() {
    let (base, _g) = sandbox("b2");
    // 每级 ~90 字符 × 4 级 + base ≈ 420+ 字符。统一用 \\?\ 扩展前缀创建
    // （系统未启用 LongPathsEnabled 时普通形态创建会失败）。
    let seg = "很长的目录名".repeat(12); // 72 字符
    let mut deep = base.clone();
    for _ in 0..4 {
        deep = deep.join(&seg);
    }
    let file = deep.join("长路径文件.tmp");
    assert!(
        file.to_string_lossy().len() > 260,
        "用例前提：路径须超过 260 字符（实际 {}）",
        file.to_string_lossy().len()
    );
    let ext_of = |p: &Path| PathBuf::from(format!(r"\\?\{}", p.to_string_lossy()));
    fs::create_dir_all(ext_of(&deep)).unwrap();
    fs::write(ext_of(&file), b"long").unwrap();

    // 用 \\?\ 扩展前缀作为 target（绕过 MAX_PATH 限制的正规形态）。
    let ext_path = ext_of(deep.as_path());
    let (table, items) = scan_target(&ext_path);
    assert_eq!(items.len(), 1, "长路径文件必须命中: {:#?}", items);

    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let report = clean_scanned("tx-b2", &table, &items, &ids);
    assert_eq!(report.ok, 1, "长路径清理应成功: {:?}", report.failures);
    assert!(!file.exists());

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-3a：junction 环——walk 不跟随，环内文件不进候选。
#[test]
fn boundary_3a_junction_loop_not_followed() {
    let (base, _g) = sandbox("b3a");
    let target = base.join("target");
    fs::create_dir_all(target.join("inner")).unwrap();
    fs::write(target.join("inner\\normal.tmp"), b"n").unwrap();
    // junction 环：target\loop → target。
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(target.join("loop"))
        .arg(&target)
        .output()
        .expect("mklink");
    assert!(out.status.success(), "mklink /J 应成功");

    let (_table, items) = scan_target(&target);
    // 只命中 inner\normal.tmp；环内（loop\...）路径不出现，也无死循环。
    assert_eq!(items.len(), 1, "junction 环不被跟随: {:#?}", items);
    assert!(items[0].path.ends_with("normal.tmp"));

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-3b（T-1 核心双用例之二）：扫描后替换——扫描时正常、执行前父目录被换成
/// junction（目标文件 size/mtime 保持一致以越过一致性检查）→ 执行前复核必须拒绝。
#[test]
fn boundary_3b_post_scan_junction_swap_blocked() {
    let (base, _g) = sandbox("b3b");
    let target = base.join("target");
    let decoy = base.join("decoy");
    fs::create_dir_all(target.join("d")).unwrap();
    fs::create_dir_all(decoy.join("d")).unwrap();
    // 同内容同 mtime（攻击者可伪造一致性，靠 reparse 复核拦截）。
    fs::write(target.join("d\\victim.tmp"), b"payload").unwrap();
    fs::write(decoy.join("d\\victim.tmp"), b"payload").unwrap();
    let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    for p in [target.join("d\\victim.tmp"), decoy.join("d\\victim.tmp")] {
        fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    let (table, items) = scan_target(&target);
    assert_eq!(items.len(), 1);
    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();

    // 扫描后替换：target → target_real，再建 junction target → decoy。
    let real = base.join("target_real");
    fs::rename(&target, &real).unwrap();
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&target)
        .arg(&decoy)
        .output()
        .expect("mklink");
    assert!(out.status.success());

    let report = clean_scanned("tx-b3b", &table, &items, &ids);
    assert_eq!(report.fail, 1, "扫描后替换必须被拒绝: {:#?}", report);
    assert!(
        report.failures[0].reason.contains("junction/symlink"),
        "拒绝原因: {}",
        report.failures[0].reason
    );
    // 两侧文件都原样（既没删 real 里的，也没动 decoy 里的）。
    assert!(real.join("d\\victim.tmp").exists());
    assert!(decoy.join("d\\victim.tmp").exists());

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-4：占用文件（独占打开）——direct 删除失败，journal 记 fail，文件原样，不 panic。
#[test]
fn boundary_4_locked_file_fails_gracefully() {
    let (base, _g) = sandbox("b4");
    let f = base.join("locked.tmp");
    fs::write(&f, b"held").unwrap();
    // 独占打开（share_mode=0）：删除将得到共享冲突（Windows 专属 API）。
    #[cfg(windows)]
    let _hold = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&f)
            .expect("open exclusive")
    };

    let cancel = CancelToken::new();
    let targets = vec![direct_target(&f)];
    let report = execute("tx-b4", &targets, &cancel, 14, &mut |_, _, _, _, _| {});
    assert_eq!(report.fail, 1, "占用文件应失败: {:#?}", report);
    assert!(f.exists(), "占用文件必须原样保留");
    // 孤儿为空（intent+result 成对）。
    assert!(pureslate_lib::cleaner::journal::detect_orphans().is_empty());

    #[cfg(windows)]
    drop(_hold);
    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-5：只读属性文件——删除失败传播（journal fail），不 panic，可恢复清理。
#[test]
fn boundary_5_readonly_file_fails_gracefully() {
    let (base, _g) = sandbox("b5");
    let f = base.join("readonly.tmp");
    fs::write(&f, b"ro").unwrap();
    let mut perm = fs::metadata(&f).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(&f, perm).unwrap();

    let cancel = CancelToken::new();
    let targets = vec![direct_target(&f)];
    let report = execute("tx-b5", &targets, &cancel, 14, &mut |_, _, _, _, _| {});
    assert_eq!(report.fail, 1, "只读文件删除应失败: {:#?}", report);
    assert!(f.exists(), "只读文件必须原样保留");
    assert!(pureslate_lib::cleaner::journal::detect_orphans().is_empty());

    // 恢复可写后清理沙箱（Windows 上 set_readonly(false) = 清 FILE_ATTRIBUTE_READONLY）。
    #[allow(clippy::permissions_set_readonly_false)] // 本仓库仅面向 Windows
    {
        let mut perm = fs::metadata(&f).unwrap().permissions();
        perm.set_readonly(false);
        fs::set_permissions(&f, perm).unwrap();
    }
    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-6（盘满等价）：隔离区根被普通文件占用（IO 失败注入等价）——
/// move_into_quarantine 报错传播，不 panic、不留半成品。
#[test]
fn boundary_6_quarantine_io_failure_propagates() {
    let (base, _g) = sandbox("b6");
    // "隔离区根"被一个普通文件占位 → ensure_quarantine_root 拒绝（T-3 顺带覆盖"非目录"）。
    let qroot = base.join("qroot");
    fs::write(&qroot, b"not a dir").unwrap();

    let src = base.join("src.tmp");
    fs::write(&src, b"data").unwrap();
    let r = pureslate_lib::quarantine::move_into_quarantine(
        &qroot,
        pureslate_lib::quarantine::QuarantineInput {
            original_path: src.clone(),
            grade: Grade::Yellow,
            category_id: "temp.compat".into(),
            retention_days: 14,
        },
    );
    assert!(r.is_err(), "隔离区根不可用必须报错");
    assert!(src.exists(), "失败时源文件必须原样（不删除）");

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-7（中途 kill 等价）：journal intent 无 result → detect_orphans 检出。
/// （真实 kill 进程测试属 P4-03 崩溃安全 DoD；此处验证协议判定。）
#[test]
fn boundary_7_orphan_journal_detected() {
    let (_base, _g) = sandbox("b7");
    // 写一条 intent 后不写 result（模拟崩溃）。
    let mut j = pureslate_lib::cleaner::journal::Journal::open("tx-orphan-b7").expect("journal");
    let victim = PathBuf::from(r"C:\Users\nobody\crash-victim.tmp");
    j.intent(&victim, Disposition::Direct).expect("intent");
    drop(j);

    let orphans = pureslate_lib::cleaner::journal::detect_orphans();
    assert!(
        orphans.iter().any(|o| o.path == victim.to_string_lossy()),
        "孤儿 intent 必须被检出: {orphans:?}"
    );
    pureslate_lib::storage::set_data_root_override(None);
}

/// §6.3-8：空文件 / 空目录 / 255 字节文件名。
#[test]
fn boundary_8_empty_and_long_names() {
    let (base, _g) = sandbox("b8");
    let target = base.join("target");
    fs::create_dir_all(target.join("empty-dir")).unwrap();
    fs::write(target.join("zero.tmp"), b"").unwrap(); // 0 字节
                                                      // 253 字节文件名（UTF-8 每字 3 字节 × 83 + ".tmp"）——贴住 255 上限。
    let long_name = format!("{}.tmp", "漢".repeat(83));
    fs::write(target.join(&long_name), b"L").unwrap();

    let (table, items) = scan_target(&target);
    assert_eq!(
        items.len(),
        2,
        "0 字节与超长名都命中，空目录不产项: {:#?}",
        items
    );
    // 0 字节项的 size 为 0 且可清理。
    assert!(items.iter().any(|i| i.size_bytes == 0));

    let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let report = clean_scanned("tx-b8", &table, &items, &ids);
    assert_eq!(report.ok, 2, "边界形态清理应成功: {:?}", report.failures);
    assert!(!target.join("zero.tmp").exists());
    assert!(!target.join(&long_name).exists());

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// T-2（还原信任链）：伪造 manifest 的三种攻击形态全部被拒。
#[test]
fn t2_restore_trust_chain_rejects_forged_manifests() {
    let (base, _g) = sandbox("t2");
    let qroot = base.join("qroot");
    let outside = base.join("outside");
    fs::create_dir_all(&qroot).unwrap();
    fs::create_dir_all(&outside).unwrap();

    // 正常移入一条（拿真实 entry）。
    let orig = base.join("orig.tmp");
    fs::write(&orig, b"real").unwrap();
    let entry = pureslate_lib::quarantine::move_into_quarantine(
        &qroot,
        pureslate_lib::quarantine::QuarantineInput {
            original_path: orig.clone(),
            grade: Grade::Yellow,
            category_id: "temp.compat".into(),
            retention_days: 14,
        },
    )
    .expect("move");

    // ① quarantine_path 越出隔离区根（伪造指向外部文件）。
    let mut forged = entry.clone();
    fs::write(outside.join("evil.tmp"), b"evil").unwrap();
    forged.quarantine_path = outside.join("evil.tmp").to_string_lossy().into_owned();
    assert!(matches!(
        restore_one(&qroot, &forged),
        RestoreOutcome::Failed(_)
    ));

    // ② sha256 不符（内容被替换）。
    let qfile = PathBuf::from(&entry.quarantine_path);
    fs::write(&qfile, b"tampered").unwrap();
    assert!(matches!(
        restore_one(&qroot, &entry),
        RestoreOutcome::Failed(_)
    ));
    // 恢复内容使 sha 一致，供 ③ 继续。
    fs::write(&qfile, b"real").unwrap();

    // ③ original_path 指向白名单禁区（系统目录）。
    let mut forged2 = entry.clone();
    forged2.original_path = format!(
        r"{}\System32\drivers\etc\hosts",
        std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
    );
    let r = restore_one(&qroot, &forged2);
    assert!(
        matches!(r, RestoreOutcome::Failed(_)),
        "白名单禁区必须拒绝: {r:?}"
    );

    // 正常路径仍可还原（回归）。
    assert!(matches!(
        restore_one(&qroot, &entry),
        RestoreOutcome::Restored
    ));
    assert_eq!(fs::read(&orig).unwrap(), b"real");

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// F-1（fail-closed）：规则表缺失类目 → resolve_targets 拒绝（不得守卫退化）。
#[test]
fn f1_missing_category_fails_closed() {
    let (base, _g) = sandbox("f1");
    let target = base.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("a.tmp"), b"a").unwrap();
    let (_table, items) = scan_target(&target);
    assert_eq!(items.len(), 1);

    // 空规则表（等价"规则包损坏/被清空"）→ 拒绝。
    let empty_table = RuleSetTable::default();
    let r = resolve_targets(&items, &[items[0].id.clone()], &empty_table);
    assert!(r.is_err(), "类目缺失必须 fail-closed");
    assert!(r.unwrap_err().contains("拒绝"));

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}

/// T-3（隔离区根 reparse）：根为 junction → ensure_quarantine_root 拒绝。
#[test]
fn t3_quarantine_root_junction_rejected() {
    let (base, _g) = sandbox("t3");
    let real = base.join("real-root");
    fs::create_dir_all(&real).unwrap();
    let link = base.join("link-root");
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .output()
        .expect("mklink");
    assert!(out.status.success());

    let r = pureslate_lib::quarantine::ensure_quarantine_root(&link);
    assert!(r.is_err(), "junction 作为隔离区根必须被拒绝");
    // 正常根仍可用。
    assert!(pureslate_lib::quarantine::ensure_quarantine_root(&real).is_ok());

    let _ = fs::remove_dir_all(&base);
    pureslate_lib::storage::set_data_root_override(None);
}
