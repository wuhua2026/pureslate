//! 启动项注册表来源（只读）：Run / RunOnce / 启动文件夹 / StartupApproved 状态。
//!
//! 只读纪律：一律 `KEY_READ`（0x20019，不含写位）；不拼 `Wow6432Node` 物理路径，
//! 32 位视图用 `KEY_WOW64_32KEY` 逻辑视图访问（见设计稿 §2.3 纠正 1）。
//!
//! StartupApproved 是**未文档化**存储：首字节 02/03=禁用、06=启用，仅作展示状态，
//! 不作安全判定。按评审决定，本轮只读 HKCU 三键；机器级条目 `approvedState` 缺省。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::contract::{BootApprovedState, BootStartupEntry, BootStartupKind, BootStartupScope};
use crate::startup::winreg::{self, Hive};

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_ONCE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
const APPROVED_RUN: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const APPROVED_RUN32: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run32";
const APPROVED_FOLDER: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder";

/// StartupApproved 首字节判定（纯函数）：02/03 = 禁用。
pub(crate) fn is_disabled_byte(data: &[u8]) -> bool {
    matches!(data.first(), Some(2 | 3))
}

/// 解析 StartupApproved 值集合 → (出现的名字, 被判禁用的名字)，统一小写。
pub(crate) fn parse_approved(
    values: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> (HashSet<String>, HashSet<String>) {
    let mut present = HashSet::new();
    let mut disabled = HashSet::new();
    for (name, data) in values {
        let key = name.to_lowercase();
        present.insert(key.clone());
        if is_disabled_byte(&data) {
            disabled.insert(key);
        }
    }
    (present, disabled)
}

/// HKCU StartupApproved 状态表：值名（小写）→ 是否禁用。
fn approved_map() -> (HashMap<String, bool>, Option<String>) {
    let mut map = HashMap::new();
    let mut failed = None;
    for (subkey, tag) in [
        (APPROVED_RUN, "startup_approved_run"),
        (APPROVED_RUN32, "startup_approved_run32"),
        (APPROVED_FOLDER, "startup_approved_folder"),
    ] {
        match winreg::enum_values(Hive::CurrentUser, subkey) {
            Ok(values) => {
                let (present, disabled) =
                    parse_approved(values.into_iter().map(|v| (v.name, v.data)));
                for name in present {
                    map.insert(name.clone(), disabled.contains(&name));
                }
            }
            // 键不存在是正常形态（从未被任务管理器写过），不算失败。
            Err(e) if is_not_found(&e) => {}
            Err(e) => {
                failed.get_or_insert_with(|| {
                    format!("{tag}_code_{}", e.raw_os_error().unwrap_or(-1))
                });
            }
        }
    }
    (map, failed)
}

fn is_not_found(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(2) | Some(3))
}

/// 注册表 Run/RunOnce 枚举（含 HKLM 64/32 两个视图）。
fn collect_run_values(
    approved: &HashMap<String, bool>,
    out: &mut Vec<BootStartupEntry>,
    failed: &mut Vec<String>,
) {
    // (hive, subkey, wow64_flags, kind, scope, tag)
    let sources: [(Hive, &str, u32, BootStartupKind, BootStartupScope, &str); 6] = [
        (
            Hive::CurrentUser,
            RUN,
            0,
            BootStartupKind::Run,
            BootStartupScope::User,
            "hkcu_run",
        ),
        (
            Hive::CurrentUser,
            RUN_ONCE,
            0,
            BootStartupKind::RunOnce,
            BootStartupScope::User,
            "hkcu_runonce",
        ),
        (
            Hive::LocalMachine,
            RUN,
            winreg::KEY_WOW64_64KEY,
            BootStartupKind::Run,
            BootStartupScope::Machine,
            "hklm_run_64",
        ),
        (
            Hive::LocalMachine,
            RUN,
            winreg::KEY_WOW64_32KEY,
            BootStartupKind::Run,
            BootStartupScope::Machine,
            "hklm_run_32",
        ),
        (
            Hive::LocalMachine,
            RUN_ONCE,
            winreg::KEY_WOW64_64KEY,
            BootStartupKind::RunOnce,
            BootStartupScope::Machine,
            "hklm_runonce_64",
        ),
        (
            Hive::LocalMachine,
            RUN_ONCE,
            winreg::KEY_WOW64_32KEY,
            BootStartupKind::RunOnce,
            BootStartupScope::Machine,
            "hklm_runonce_32",
        ),
    ];
    for (hive, subkey, flags, kind, scope, tag) in sources {
        let values = match winreg::enum_values_view(hive, subkey, flags) {
            Ok(v) => v,
            Err(e) if is_not_found(&e) => continue,
            Err(e) => {
                failed.push(format!("{tag}_code_{}", e.raw_os_error().unwrap_or(-1)));
                continue;
            }
        };
        for v in values {
            if v.kind != winreg::REG_SZ && v.kind != winreg::REG_EXPAND_SZ {
                continue;
            }
            let command = winreg::reg_data_to_string(&v.data);
            let command = if command.trim().is_empty() {
                continue;
            } else if v.kind == winreg::REG_EXPAND_SZ {
                winreg::expand_environment(&command).unwrap_or(command)
            } else {
                command
            };
            out.push(make_entry(
                &v.name,
                &command,
                kind,
                scope,
                approved,
                Some(format!("{}\\{}", hive.key_prefix(), subkey)),
            ));
        }
    }
}

/// 启动文件夹枚举（用户 / 全体用户）。
fn collect_startup_folders(
    approved: &HashMap<String, bool>,
    out: &mut Vec<BootStartupEntry>,
    failed: &mut Vec<String>,
) {
    let user = std::env::var_os("APPDATA").map(|p| {
        PathBuf::from(p)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
    });
    let machine = std::env::var_os("ProgramData").map(|p| {
        PathBuf::from(p)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("StartUp")
    });
    for (dir, scope, tag) in [
        (user, BootStartupScope::User, "startup_folder_user"),
        (machine, BootStartupScope::Machine, "startup_folder_machine"),
    ] {
        let Some(dir) = dir else {
            failed.push(format!("{tag}_env_missing"));
            continue;
        };
        if let Err(e) = fs::read_dir(&dir) {
            if e.kind() != io::ErrorKind::NotFound {
                failed.push(format!("{tag}_code_{}", e.raw_os_error().unwrap_or(-1)));
            }
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            out.push(make_entry(
                &name,
                &entry.path().to_string_lossy(),
                BootStartupKind::StartupFolder,
                scope,
                approved,
                Some(dir.to_string_lossy().to_string()),
            ));
        }
    }
}

/// 组装一条启动项（纯函数）：ID 稳定、状态只对 HKCU 有值。
pub(crate) fn make_entry(
    name: &str,
    command: &str,
    kind: BootStartupKind,
    scope: BootStartupScope,
    approved: &HashMap<String, bool>,
    detail: Option<String>,
) -> BootStartupEntry {
    let (enabled, approved_state) = match scope {
        BootStartupScope::User => match approved.get(&name.to_lowercase()) {
            Some(disabled) => (
                Some(!*disabled),
                Some(if *disabled {
                    BootApprovedState::Disabled
                } else {
                    BootApprovedState::Enabled
                }),
            ),
            // 任务管理器口径：无记录视作启用，但状态未知。
            None => (Some(true), Some(BootApprovedState::Unknown)),
        },
        // 评审决定：本轮不读 HKLM StartupApproved。
        BootStartupScope::Machine => (Some(true), None),
    };
    BootStartupEntry {
        id: entry_id(kind, scope, name, command),
        name: name.to_string(),
        command: command.to_string(),
        kind,
        scope,
        enabled,
        approved_state,
        delayed_auto_start: None,
        detail,
    }
}

/// 稳定 ID：kind + scope + name + command 的 SHA-256 前 12 个十六进制字符。
/// 服务/计划任务复用同一算法，保证跨来源不撞车。
pub(crate) fn entry_id(
    kind: BootStartupKind,
    scope: BootStartupScope,
    name: &str,
    command: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(format!("{kind:?}|{scope:?}|").as_bytes());
    h.update(name.as_bytes());
    h.update(b"|");
    h.update(command.as_bytes());
    let digest = h.finalize();
    format!("bs-{}", hex12(&digest))
}

fn hex12(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(12);
    for b in bytes.iter().take(6) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 枚举注册表类启动项（Run/RunOnce/启动文件夹），返回条目与失败子源。
pub fn collect() -> (Vec<BootStartupEntry>, Vec<String>) {
    let (approved, approved_failure) = approved_map();
    let mut out = Vec::new();
    let mut failed = Vec::new();
    if let Some(f) = approved_failure {
        failed.push(f);
    }
    collect_run_values(&approved, &mut out, &mut failed);
    collect_startup_folders(&approved, &mut out, &mut failed);
    (out, failed)
}

#[cfg(test)]
fn startup_folder(base: &std::path::Path) -> PathBuf {
    base.join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_byte_semantics() {
        assert!(is_disabled_byte(&[2]));
        assert!(is_disabled_byte(&[3, 0, 0]));
        assert!(!is_disabled_byte(&[6]));
        assert!(!is_disabled_byte(&[]));
    }

    #[test]
    fn parse_approved_splits_present_and_disabled() {
        let values = vec![
            ("OneDrive".to_string(), vec![6u8]),
            ("Steam".to_string(), vec![2u8, 0, 0]),
        ];
        let (present, disabled) = parse_approved(values);
        assert_eq!(present.len(), 2);
        assert!(present.contains("onedrive"));
        assert!(disabled.contains("steam"));
        assert!(!disabled.contains("onedrive"));
    }

    #[test]
    fn make_entry_marks_user_approved_states() {
        let mut approved = HashMap::new();
        approved.insert("steam".to_string(), true);
        approved.insert("onedrive".to_string(), false);

        let disabled = make_entry(
            "Steam",
            "C:\\x\\steam.exe",
            BootStartupKind::Run,
            BootStartupScope::User,
            &approved,
            None,
        );
        assert_eq!(disabled.enabled, Some(false));
        assert_eq!(disabled.approved_state, Some(BootApprovedState::Disabled));

        let enabled = make_entry(
            "OneDrive",
            "C:\\x\\od.exe",
            BootStartupKind::Run,
            BootStartupScope::User,
            &approved,
            None,
        );
        assert_eq!(enabled.enabled, Some(true));
        assert_eq!(enabled.approved_state, Some(BootApprovedState::Enabled));

        let unknown = make_entry(
            "Other",
            "C:\\x\\o.exe",
            BootStartupKind::Run,
            BootStartupScope::User,
            &approved,
            None,
        );
        assert_eq!(unknown.approved_state, Some(BootApprovedState::Unknown));
    }

    #[test]
    fn machine_scope_never_claims_approved_state() {
        let e = make_entry(
            "Svc",
            "C:\\x\\s.exe",
            BootStartupKind::Run,
            BootStartupScope::Machine,
            &HashMap::new(),
            None,
        );
        assert_eq!(e.approved_state, None);
        assert_eq!(e.enabled, Some(true));
    }

    #[test]
    fn entry_id_is_stable_and_distinct() {
        let a = entry_id(
            BootStartupKind::Run,
            BootStartupScope::User,
            "A",
            "C:\\a.exe",
        );
        let b = entry_id(
            BootStartupKind::Run,
            BootStartupScope::User,
            "A",
            "C:\\a.exe",
        );
        let c = entry_id(
            BootStartupKind::Run,
            BootStartupScope::User,
            "A",
            "C:\\b.exe",
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 15);
    }

    #[test]
    fn startup_folder_path_shape() {
        let p = startup_folder(std::path::Path::new("C:\\Users\\u\\AppData\\Roaming"));
        assert!(p.ends_with("Startup"));
    }
}
