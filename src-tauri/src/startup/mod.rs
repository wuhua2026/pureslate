//! 启动项（R07，SPEC §6.4；v0.1.5 反馈 4 扩至 7 源）。
//!
//! 枚举源：HKCU Run、HKLM Run、HKLM WOW6432Node Run（展示归入 hklm_run）、
//! 用户启动文件夹（shell:startup）、**全体用户启动文件夹**（ProgramData）、
//! **Active Setup**（HKLM/HKCU Installed Components 的 StubPath）、登录触发计划任务。
//! 禁用 = 备份后移除（备份即事务记录，先备份后动手）、不删源程序；`toggle(id, true)` 还原。
//! 全程审计（disable_startup / enable_startup）。
//! Run/文件夹类条目附带 **StartupApproved** 系统禁用状态（只读展示，不写系统批准状态）。

pub mod backup;
pub mod impact;
pub mod regfile;
pub mod tasks;
pub mod version;
pub mod winreg;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::contract::{Impact, LogEntry, StartupEntry, StartupSource};
use crate::logging::audit;
use winreg::Hive;

/// HKCU/HKLM Run 子键。
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// HKLM 32 位视图 Run 子键。
const RUN_SUBKEY_WOW64: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run";
/// Active Setup 自启位根（v0.1.5 反馈 4）。
const ACTIVE_SETUP_ROOT: &str = "Software\\Microsoft\\Active Setup\\Installed Components";
/// StartupApproved 系统批准状态（v0.1.5：只读判定"任务管理器口径的已禁用"）。
const STARTUP_APPROVED_RUN: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const STARTUP_APPROVED_RUN32: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run32";
const STARTUP_APPROVED_FOLDER: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder";

/// 一个活动启动项（含 toggle 定位信息）。
#[derive(Debug, Clone)]
pub struct RawItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: StartupSource,
    pub kind: ItemKind,
    /// 系统级已禁用（StartupApproved 判定）：展示 enabled=false + systemDisabled 标注，
    /// 但**不是**本应用备份所致——不提供恢复入口（不写系统批准状态）。
    pub system_disabled: bool,
}

/// 启动项的定位与备份/还原方式。
#[derive(Debug, Clone)]
pub enum ItemKind {
    /// 注册表 Run 值。
    RunValue {
        hive: Hive,
        subkey: String,
        value_name: String,
        value_kind: u32,
        data: Vec<u8>,
    },
    /// 启动文件夹中的文件（.lnk 等）。
    FolderFile { path: PathBuf },
    /// 计划任务（登录触发）。
    Task { task_name: String },
}

/// 稳定 ID：sha256(定位串) 前 16 位 hex（与 ScanItem 风格一致）。
fn stable_id(tag: &str) -> String {
    let mut h = Sha256::new();
    h.update(tag.as_bytes());
    let digest = h.finalize();
    digest[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// 用户启动文件夹（shell:startup）。环境缺失 → None（跳过该源）。
fn startup_folder() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    let p = appdata
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup");
    p.is_dir().then_some(p)
}

/// 全体用户启动文件夹（ProgramData，v0.1.5 反馈 4）。
fn startup_folder_all_users() -> Option<PathBuf> {
    let pd = std::env::var_os("ProgramData").map(PathBuf::from)?;
    let p = pd
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("StartUp");
    p.is_dir().then_some(p)
}

/// StartupApproved 二进制判定（纯函数）：首字节 02/03 = 系统级已禁用（06=启用）。
fn is_approved_disabled(data: &[u8]) -> bool {
    !data.is_empty() && (data[0] == 2 || data[0] == 3)
}

/// 系统"已禁用"条目名集合（StartupApproved 三子键，只读；键缺失=空集）。
/// 值名统一小写化（注册表值名大小写不敏感，字符串比对敏感）。
fn approved_disabled_names() -> HashSet<String> {
    let mut out = HashSet::new();
    for subkey in [
        STARTUP_APPROVED_RUN,
        STARTUP_APPROVED_RUN32,
        STARTUP_APPROVED_FOLDER,
    ] {
        if let Ok(values) = winreg::enum_values(Hive::CurrentUser, subkey) {
            for v in values {
                if is_approved_disabled(&v.data) {
                    out.insert(v.name.to_lowercase());
                }
            }
        }
    }
    out
}

/// 启动文件夹枚举（用户/全体用户共用；approved 键 = 文件名含扩展名）。
fn folder_items(
    dir: &Path,
    source: StartupSource,
    approved: &HashSet<String>,
    out: &mut Vec<RawItem>,
) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tag = format!("folder|{}", path.to_string_lossy());
        out.push(RawItem {
            id: stable_id(&tag),
            name,
            command: path.to_string_lossy().into_owned(),
            source,
            kind: ItemKind::FolderFile { path },
            system_disabled: approved.contains(&file_name.to_lowercase()),
        });
    }
}

/// Active Setup 自启位枚举（v0.1.5 反馈 4）。参数化子键根（测试可注入）；
/// HKLM 与 HKCU 同名组件取 HKLM 优先；StubPath 为字符串类才入选。
fn active_setup_items(root_subkey: &str, out: &mut Vec<RawItem>) {
    let mut seen: HashSet<String> = HashSet::new();
    for hive in [Hive::LocalMachine, Hive::CurrentUser] {
        let Ok(subkeys) = winreg::enum_subkeys(hive, root_subkey) else {
            continue;
        };
        for sk in subkeys {
            if !seen.insert(sk.clone()) {
                continue; // HKLM 优先（先遍历）
            }
            let full = format!("{root_subkey}\\{sk}");
            let Ok(values) = winreg::enum_values(hive, &full) else {
                continue;
            };
            let Some(v) = values.iter().find(|v| {
                v.name == "StubPath"
                    && (v.kind == winreg::REG_SZ || v.kind == winreg::REG_EXPAND_SZ)
            }) else {
                continue;
            };
            let command = winreg::reg_data_to_string(&v.data);
            if command.is_empty() {
                continue;
            }
            let tag = format!("activesetup|{full}");
            out.push(RawItem {
                id: stable_id(&tag),
                name: sk,
                command,
                source: StartupSource::ActiveSetup,
                kind: ItemKind::RunValue {
                    hive,
                    subkey: full,
                    value_name: "StubPath".into(),
                    value_kind: v.kind,
                    data: v.data.clone(),
                },
                system_disabled: false,
            });
        }
    }
}

/// 枚举全部活动启动项（只读）。
pub fn live_items() -> Vec<RawItem> {
    let mut out = Vec::new();
    let approved = approved_disabled_names();

    // 注册表 3 键（wow64 展示归入 hklm_run，契约 source 无独立 wow64 值）。
    let run_keys: [(Hive, &str, StartupSource); 3] = [
        (Hive::CurrentUser, RUN_SUBKEY, StartupSource::HkcuRun),
        (Hive::LocalMachine, RUN_SUBKEY, StartupSource::HklmRun),
        (Hive::LocalMachine, RUN_SUBKEY_WOW64, StartupSource::HklmRun),
    ];
    for (hive, subkey, source) in run_keys {
        let Ok(values) = winreg::enum_values(hive, subkey) else {
            continue; // 无权限/键缺失：跳过该源（宁缺勿错）
        };
        for v in values {
            // 只展示字符串类值（Run 键 99% 为 SZ/EXPAND_SZ；其他类型禁用/还原语义不完整）。
            if v.kind != winreg::REG_SZ && v.kind != winreg::REG_EXPAND_SZ {
                continue;
            }
            let command = winreg::reg_data_to_string(&v.data);
            let tag = format!("run|{subkey}|{}", v.name);
            let system_disabled = approved.contains(&v.name.to_lowercase());
            out.push(RawItem {
                id: stable_id(&tag),
                name: v.name.clone(),
                command,
                source,
                kind: ItemKind::RunValue {
                    hive,
                    subkey: subkey.to_string(),
                    value_name: v.name,
                    value_kind: v.kind,
                    data: v.data,
                },
                system_disabled,
            });
        }
    }

    // 用户启动文件夹 + 全体用户启动文件夹（v0.1.5 反馈 4）。
    if let Some(dir) = startup_folder() {
        folder_items(&dir, StartupSource::StartupFolder, &approved, &mut out);
    }
    if let Some(dir) = startup_folder_all_users() {
        folder_items(
            &dir,
            StartupSource::StartupFolderAllUsers,
            &approved,
            &mut out,
        );
    }

    // Active Setup（v0.1.5 反馈 4）。
    active_setup_items(ACTIVE_SETUP_ROOT, &mut out);

    // 登录触发计划任务。
    for t in tasks::enumerate_logon_tasks() {
        let tag = format!("task|{}", t.name);
        out.push(RawItem {
            id: stable_id(&tag),
            name: t.name.clone(),
            command: t.command,
            source: StartupSource::TaskScheduler,
            kind: ItemKind::Task { task_name: t.name },
            system_disabled: false,
        });
    }

    out
}

/// 组装对前端的 StartupEntry（发布者 + 影响估算）。
fn to_entry(item: &RawItem) -> StartupEntry {
    let publisher = version::exe_path_from_command(&item.command)
        .filter(|p| p.is_file())
        .and_then(|p| version::file_publisher(&p));
    StartupEntry {
        id: item.id.clone(),
        name: item.name.clone(),
        publisher,
        command: item.command.clone(),
        source: item.source,
        impact: impact::estimate_impact(item.source, &item.command, &item.name),
        enabled: !item.system_disabled,
        system_disabled: item.system_disabled.then_some(true),
    }
}

/// 由备份记录重建（已禁用项）。
fn entry_from_record(rec: &backup::BackupRecord) -> StartupEntry {
    StartupEntry {
        id: rec.id.clone(),
        name: rec.name.clone(),
        publisher: None,
        command: rec.command.clone(),
        source: rec.source,
        impact: impact::estimate_impact(rec.source, &rec.command, &rec.name),
        enabled: false,
        system_disabled: None,
    }
}

/// 启动项列表：活动项（enabled=true）+ 已禁用备份项（enabled=false），按影响降序。
pub fn list() -> Vec<StartupEntry> {
    let live = live_items();
    let live_ids: HashSet<String> = live.iter().map(|it| it.id.clone()).collect();

    let mut entries: Vec<StartupEntry> = live.iter().map(to_entry).collect();
    for rec in backup::load_manifest() {
        // 已被用户手动恢复的陈旧记录：忽略（活动项优先）。
        if live_ids.contains(&rec.id) {
            continue;
        }
        entries.push(entry_from_record(&rec));
    }

    let rank = |i: Impact| match i {
        Impact::High => 0,
        Impact::Medium => 1,
        Impact::Low => 2,
    };
    entries.sort_by(|a, b| {
        rank(a.impact)
            .cmp(&rank(b.impact))
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// 禁用单项：备份先行（红线"先日志后动手"），成功后落 manifest。
fn disable_item(item: &RawItem) -> Result<(), String> {
    let dir = backup::backup_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {e}"))?;

    match &item.kind {
        ItemKind::RunValue {
            hive,
            subkey,
            value_name,
            value_kind,
            data,
        } => {
            // 1) 写 .reg 备份；2) 删值；失败即回滚备份文件。
            let bv = regfile::RegBackupValue {
                key_path: format!("{}\\{}", hive.key_prefix(), subkey),
                name: value_name.clone(),
                kind: *value_kind,
                data: data.clone(),
            };
            let reg_path = dir.join(format!("{}.reg", item.id));
            regfile::write_reg_file(&reg_path, &bv)
                .map_err(|e| format!("写注册表备份失败: {e}"))?;
            if let Err(e) = winreg::delete_value(*hive, subkey, value_name) {
                let _ = fs::remove_file(&reg_path);
                return Err(format!("移除注册表值失败: {e}"));
            }
            backup::append_record(&backup::BackupRecord {
                id: item.id.clone(),
                source: item.source,
                name: item.name.clone(),
                command: item.command.clone(),
                disabled_at: audit::now_ms(),
                reg_file: Some(format!("{}.reg", item.id)),
                file_backup: None,
                orig_path: None,
                task_name: None,
            })
            .map_err(|e| format!("写备份记录失败: {e}"))
        }
        ItemKind::FolderFile { path } => {
            // 备份先行：复制+长度校验后才移除原文件（崩溃窗口最坏双份而非丢失）。
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|| "lnk".into());
            let backup_name = format!("{}.{}", item.id, ext);
            let dst = dir.join(&backup_name);
            let copied = fs::copy(path, &dst).map_err(|e| format!("备份启动文件失败: {e}"))?;
            let orig_len = fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
            if copied != orig_len {
                let _ = fs::remove_file(&dst);
                return Err("备份文件校验失败（长度不一致）".into());
            }
            if let Err(e) = fs::remove_file(path) {
                let _ = fs::remove_file(&dst);
                return Err(format!("移除启动文件失败: {e}"));
            }
            backup::append_record(&backup::BackupRecord {
                id: item.id.clone(),
                source: item.source,
                name: item.name.clone(),
                command: item.command.clone(),
                disabled_at: audit::now_ms(),
                reg_file: None,
                file_backup: Some(backup_name),
                orig_path: Some(path.to_string_lossy().into_owned()),
                task_name: None,
            })
            .map_err(|e| format!("写备份记录失败: {e}"))
        }
        ItemKind::Task { task_name } => {
            // schtasks 禁用：任务本体保留在系统，天然"不删源"。
            if !tasks::set_task_enabled(task_name, false) {
                return Err(format!("禁用计划任务失败: {task_name}"));
            }
            backup::append_record(&backup::BackupRecord {
                id: item.id.clone(),
                source: item.source,
                name: item.name.clone(),
                command: item.command.clone(),
                disabled_at: audit::now_ms(),
                reg_file: None,
                file_backup: None,
                orig_path: None,
                task_name: Some(task_name.clone()),
            })
            .map_err(|e| format!("写备份记录失败: {e}"))
        }
    }
}

/// 按备份记录还原单项。
fn enable_record(rec: &backup::BackupRecord) -> Result<(), String> {
    if let Some(task_name) = rec.task_name.as_deref() {
        if !tasks::set_task_enabled(task_name, true) {
            return Err(format!("启用计划任务失败: {task_name}"));
        }
    } else if let (Some(backup_name), Some(orig)) =
        (rec.file_backup.as_deref(), rec.orig_path.as_deref())
    {
        let src = backup::backup_dir().join(backup_name);
        let orig_path = PathBuf::from(orig);
        if !src.is_file() {
            return Err("备份文件缺失，无法还原".into());
        }
        if orig_path.exists() {
            return Err("原路径已被占用，还原中止（备份保留）".into());
        }
        if let Some(parent) = orig_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("重建启动文件夹失败: {e}"))?;
        }
        // rename 优先（原子）；跨盘兜底 copy+remove。
        if fs::rename(&src, &orig_path).is_err() {
            fs::copy(&src, &orig_path)
                .and_then(|_| fs::remove_file(&src))
                .map_err(|e| {
                    let _ = fs::remove_file(&orig_path);
                    format!("还原启动文件失败: {e}")
                })?;
        }
    } else if let Some(reg_name) = rec.reg_file.as_deref() {
        let path = backup::backup_dir().join(reg_name);
        let bv = regfile::parse_reg_file(&path).map_err(|e| format!("读取注册表备份失败: {e}"))?;
        let (hive, subkey) =
            split_key_path(&bv.key_path).ok_or_else(|| "备份键路径无法识别".to_string())?;
        winreg::set_value(hive, &subkey, &bv.name, bv.kind, &bv.data)
            .map_err(|e| format!("写回注册表值失败: {e}"))?;
        let _ = fs::remove_file(&path);
    } else {
        return Err("备份记录不完整".into());
    }

    backup::remove_record(&rec.id).map_err(|e| format!("清理备份记录失败: {e}"))
}

/// `HKEY_CURRENT_USER\Software\...` → (Hive, "Software\\...")。
fn split_key_path(key_path: &str) -> Option<(Hive, String)> {
    let (prefix, rest) = key_path.split_once('\\')?;
    let hive = Hive::from_key_prefix(prefix)?;
    Some((hive, rest.to_string()))
}

/// 启动项启停（IPC 入口）。禁用=备份后移除；启用=按备份还原。
/// 返回是否成功；全程审计（失败含 detail）。
pub fn toggle(id: &str, enabled: bool) -> bool {
    if enabled {
        let rec = backup::load_manifest().into_iter().find(|r| r.id == id);
        match rec {
            Some(rec) => match enable_record(&rec) {
                Ok(()) => {
                    audit_startup("enable_startup", &rec.name, "ok", None);
                    true
                }
                Err(e) => {
                    audit_startup("enable_startup", &rec.name, "fail", Some(e));
                    false
                }
            },
            // 无备份记录：项本身在位视为幂等成功，否则失败。
            None => live_items().iter().any(|it| it.id == id),
        }
    } else {
        // 已禁用：幂等成功。
        if backup::load_manifest().iter().any(|r| r.id == id) {
            return true;
        }
        match live_items().into_iter().find(|it| it.id == id) {
            Some(item) => match disable_item(&item) {
                Ok(()) => {
                    audit_startup("disable_startup", &item.name, "ok", None);
                    true
                }
                Err(e) => {
                    audit_startup("disable_startup", &item.name, "fail", Some(e));
                    false
                }
            },
            None => {
                audit_startup("disable_startup", id, "fail", Some("未找到该启动项".into()));
                false
            }
        }
    }
}

/// 审计一条启动项操作（op 语义见 SPEC §4.4；enable_startup 为加性补充，op 为字符串字段）。
fn audit_startup(op: &str, subject: &str, result: &str, detail: Option<String>) {
    let entry = LogEntry {
        ts: audit::now_ms(),
        op: op.to_string(),
        tx_id: None,
        category_id: None,
        path: Some(subject.to_string()),
        size_bytes: None,
        disposition: None,
        result: Some(result.to_string()),
        detail,
    };
    if let Err(e) = audit::record(&entry) {
        eprintln!("[startup] 审计写入失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 沙箱 data_root + 串行锁（同 storage 测试模式，避免共享全局并行污染）。
    /// 锁须持满整个测试体：否则并行测试会中途换掉共享 DATA_ROOT_OVERRIDE。
    struct Sandbox {
        root: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Sandbox {
        fn new(tag: &str) -> Self {
            // 抗中毒（LESSONS：某用例失败持锁 panic 不应级联废掉其余用例）。
            let guard = crate::storage::TEST_DATA_ROOT_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let root = std::env::temp_dir().join(format!(
                "pureslate-startup-{tag}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).unwrap();
            crate::storage::set_data_root_override(Some(root.clone()));
            Sandbox {
                root,
                _guard: guard,
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            crate::storage::set_data_root_override(None);
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn registry_disable_enable_roundtrip_on_test_key() {
        let _sb = Sandbox::new("reg");
        let subkey = format!("Software\\PureSlateSelfTest\\Run-{}", std::process::id());
        let data = winreg::string_to_reg_data("C:\\PureSlate Test\\app.exe /bg");
        winreg::set_value(Hive::CurrentUser, &subkey, "TestApp", winreg::REG_SZ, &data).unwrap();

        // 枚举能读到
        let values = winreg::enum_values(Hive::CurrentUser, &subkey).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].name, "TestApp");

        let item = RawItem {
            id: stable_id("run|test"),
            name: "TestApp".into(),
            command: "C:\\PureSlate Test\\app.exe /bg".into(),
            source: StartupSource::HkcuRun,
            kind: ItemKind::RunValue {
                hive: Hive::CurrentUser,
                subkey: subkey.clone(),
                value_name: "TestApp".into(),
                value_kind: winreg::REG_SZ,
                data: values[0].data.clone(),
            },
            system_disabled: false,
        };

        // 禁用：备份文件存在 + 值被移除 + manifest 有记录
        disable_item(&item).unwrap();
        let reg_backup = backup::backup_dir().join(format!("{}.reg", item.id));
        assert!(reg_backup.is_file(), ".reg 备份文件须存在（任务 DoD）");
        assert!(winreg::enum_values(Hive::CurrentUser, &subkey)
            .unwrap()
            .is_empty());
        let recs = backup::load_manifest();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].id, item.id);
        assert_eq!(
            recs[0].reg_file.as_deref(),
            Some(format!("{}.reg", item.id).as_str())
        );

        // 还原：值回到原样（含中文路径空格转义），manifest 清空
        enable_record(&recs[0]).unwrap();
        let after = winreg::enum_values(Hive::CurrentUser, &subkey).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(
            winreg::reg_data_to_string(&after[0].data),
            "C:\\PureSlate Test\\app.exe /bg"
        );
        assert!(backup::load_manifest().is_empty());
        assert!(!reg_backup.exists());

        // 清理测试键
        winreg::delete_value(Hive::CurrentUser, &subkey, "TestApp").unwrap();
        winreg::delete_key(Hive::CurrentUser, &subkey).unwrap();
    }

    #[test]
    fn folder_disable_enable_roundtrip() {
        let _sb = Sandbox::new("folder");
        let startup_dir = std::env::temp_dir().join(format!(
            "pureslate-startup-folder-src-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&startup_dir).unwrap();
        let orig = startup_dir.join("My App.lnk");
        fs::write(&orig, b"fake-lnk-content").unwrap();

        let item = RawItem {
            id: stable_id("folder|test"),
            name: "My App".into(),
            command: orig.to_string_lossy().into_owned(),
            source: StartupSource::StartupFolder,
            kind: ItemKind::FolderFile { path: orig.clone() },
            system_disabled: false,
        };

        disable_item(&item).unwrap();
        assert!(!orig.exists(), "禁用后原文件须移除");
        let recs = backup::load_manifest();
        assert_eq!(recs.len(), 1);
        let backup_file = backup::backup_dir().join(recs[0].file_backup.clone().unwrap());
        assert!(backup_file.is_file(), "启动文件备份须存在（任务 DoD）");
        assert_eq!(fs::read(&backup_file).unwrap(), b"fake-lnk-content");

        enable_record(&recs[0]).unwrap();
        assert!(orig.exists(), "还原后原文件须回到原路径");
        assert_eq!(fs::read(&orig).unwrap(), b"fake-lnk-content");
        assert!(backup::load_manifest().is_empty());

        let _ = fs::remove_dir_all(&startup_dir);
    }

    #[test]
    fn toggle_disable_missing_id_fails() {
        let _sb = Sandbox::new("missing");
        assert!(!toggle("no-such-id-1234", false));
    }

    /// v0.1.5 反馈 4：Active Setup 枚举 + 禁用/还原 roundtrip（HKCU 自建测试键）。
    #[test]
    fn active_setup_disable_enable_roundtrip() {
        let _sb = Sandbox::new("activesetup");
        let root = format!(
            "Software\\PureSlateSelfTest\\ActiveSetup-{}",
            std::process::id()
        );
        let comp = format!("{root}\\{{TEST-GUID}}");
        let data = winreg::string_to_reg_data("rundll32.exe setup.dll,Setup");
        winreg::set_value(Hive::CurrentUser, &comp, "StubPath", winreg::REG_SZ, &data).unwrap();
        // 无 StubPath 的兄弟组件：不入选。
        winreg::set_value(
            Hive::CurrentUser,
            &format!("{root}\\{{NO-STUB}}"),
            "Version",
            winreg::REG_SZ,
            &winreg::string_to_reg_data("1.0"),
        )
        .unwrap();

        let mut items = Vec::new();
        active_setup_items(&root, &mut items);
        assert_eq!(items.len(), 1, "只有含 StubPath 的组件入选");
        let item = &items[0];
        assert_eq!(item.name, "{TEST-GUID}");
        assert_eq!(item.source, StartupSource::ActiveSetup);
        assert!(item.command.contains("rundll32"));

        // 禁用（备份 .reg + 删 StubPath）→ 还原（写回）。
        disable_item(item).unwrap();
        assert!(
            winreg::enum_values(Hive::CurrentUser, &comp)
                .unwrap()
                .is_empty(),
            "StubPath 须被移除"
        );
        let recs = backup::load_manifest();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].source, StartupSource::ActiveSetup);
        assert!(recs[0].reg_file.is_some(), "Active Setup 须 .reg 备份");

        enable_record(&recs[0]).unwrap();
        let after = winreg::enum_values(Hive::CurrentUser, &comp).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].name, "StubPath");
        assert!(backup::load_manifest().is_empty());

        // 清理测试键。
        winreg::delete_key(Hive::CurrentUser, &comp).unwrap();
        winreg::delete_key(Hive::CurrentUser, &format!("{root}\\{{NO-STUB}}")).unwrap();
        winreg::delete_key(Hive::CurrentUser, &root).unwrap();
    }

    /// v0.1.5 反馈 4：全体用户启动文件夹 = FolderFile 同款禁用/还原语义（注入目录）。
    #[test]
    fn folder_all_users_roundtrip_via_injected_dir() {
        let _sb = Sandbox::new("folderall");
        let dir = std::env::temp_dir().join(format!(
            "pureslate-startup-folderall-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let orig = dir.join("Team Agent.lnk");
        fs::write(&orig, b"lnk").unwrap();

        let mut items = Vec::new();
        let approved = HashSet::new();
        folder_items(
            &dir,
            StartupSource::StartupFolderAllUsers,
            &approved,
            &mut items,
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, StartupSource::StartupFolderAllUsers);
        assert!(!items[0].system_disabled);

        disable_item(&items[0]).unwrap();
        assert!(!orig.exists());
        let recs = backup::load_manifest();
        assert_eq!(recs[0].source, StartupSource::StartupFolderAllUsers);
        enable_record(&recs[0]).unwrap();
        assert!(orig.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    /// StartupApproved 判定：首字节 02/03 = 禁用；06/空 = 未禁用。
    #[test]
    fn startup_approved_byte_semantics() {
        assert!(is_approved_disabled(&[
            0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]));
        assert!(is_approved_disabled(&[0x03, 0, 0, 0]));
        assert!(!is_approved_disabled(&[0x06, 0, 0, 0]));
        assert!(!is_approved_disabled(&[]));
    }

    #[test]
    fn toggle_enable_live_item_is_idempotent() {
        let _sb = Sandbox::new("idem");
        // 取一个真实活动项（如 HKCU Run 里的项或至少枚举不 panic）；没有则跳过断言。
        let live = live_items();
        if let Some(it) = live.first() {
            assert!(toggle(&it.id, true), "已启用项再启用应幂等成功");
        }
    }

    #[test]
    fn list_returns_live_and_disabled() {
        let _sb = Sandbox::new("list");
        // 沙箱内放一条陈旧备份记录：list 应产出 enabled=false 的条目。
        backup::append_record(&backup::BackupRecord {
            id: "zz-disabled-item".into(),
            source: StartupSource::HkcuRun,
            name: "已禁用项".into(),
            command: "C:\\x\\a.exe".into(),
            disabled_at: 0,
            reg_file: None,
            file_backup: None,
            orig_path: None,
            task_name: None,
        })
        .unwrap();
        let entries = list();
        let e = entries
            .iter()
            .find(|e| e.id == "zz-disabled-item")
            .expect("已禁用项应出现在列表");
        assert!(!e.enabled);
        // enabled=false 只允许两类：本应用备份项（zz-disabled-item）或系统级禁用
        // （StartupApproved 判定，v0.1.5 起真实机器上可能存在）。
        assert!(entries.iter().all(|e| {
            e.enabled || e.id == "zz-disabled-item" || e.system_disabled == Some(true)
        }));
    }
}
