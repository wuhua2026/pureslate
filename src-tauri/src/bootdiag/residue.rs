//! 卸载残留「登记一致性」检查（只读，设计稿 §2.4）。
//!
//! 只回答一个问题：Uninstall 注册表条目所登记的安装目录/卸载程序是否还站得住。
//!
//! 纪律：
//! - **不**扫描安装目录内容、**不**访问 UNC/相对路径、**不**按名称/发布者模糊匹配；
//! - 普通程序走**双条件**（目录缺失 ∧ 卸载程序缺失）——本机 210/276 条无
//!   `InstallLocation`，单条件会误报约 76%；
//! - MSI 走产品码登记（`MsiEnumProductsExW`），安装位置未知只计入 `unknown_count`；
//! - 卸载命令只用于判定，原样返回给 UI 但不展示（评审决定：避免误点执行）。

use std::collections::HashMap;
use std::path::Path;

use crate::contract::{
    BootResidueData, BootResidueEntry, BootSection, BootSectionStatus, BootStartupScope,
};
use crate::startup::winreg::{enum_values_view, reg_data_to_string, Hive};
use crate::uninstall::runner::{expand_env, split_exe_args};
use crate::uninstall::{is_msi_guid_key, located_by_ids};

/// `MSIINSTALLCONTEXT` 取值（msi.h）。
const MSIINSTALLCONTEXT_USERMANAGED: u32 = 1;
const MSIINSTALLCONTEXT_USERUNMANAGED: u32 = 2;
const MSIINSTALLCONTEXT_MACHINE: u32 = 4;
/// `INSTALLPROPERTY_INSTALLLOCATION`（msi.h）。
const INSTALLPROPERTY_INSTALLLOCATION: &str = "InstallLocation";

const ERROR_SUCCESS: u32 = 0;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_NO_MORE_ITEMS: u32 = 259;

/// GUID 38 字符 + NUL。
const PRODUCT_CODE_BUF: usize = 39;

#[link(name = "msi")]
extern "system" {
    fn MsiEnumProductsExW(
        sz_product_code: *const u16,
        sz_user_sid: *const u16,
        dw_context: u32,
        dw_index: u32,
        sz_installed_product_code: *mut u16,
        pdw_installed_context: *mut u32,
        sz_sid: *mut u16,
        pcch_sid: *mut u32,
    ) -> u32;
    fn MsiGetProductInfoExW(
        sz_product_code: *const u16,
        sz_user_sid: *const u16,
        dw_context: u32,
        sz_property: *const u16,
        sz_value: *mut u16,
        pcch_value: *mut u32,
    ) -> u32;
}

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// MSI 产品在系统中的登记（本轮只取机器/当前用户三个上下文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MsiProduct {
    pub context: u32,
    pub install_location: Option<String>,
}

/// 枚举本机 MSI 产品码 → 登记信息。
///
/// 任一上下文枚举成功即视为整体可用；全部失败才返回首个错误码。
pub(crate) fn msi_products() -> Result<HashMap<String, MsiProduct>, u32> {
    let mut out = HashMap::new();
    let mut any_context_ok = false;
    let mut first_err: Option<u32> = None;

    for context in [
        MSIINSTALLCONTEXT_MACHINE,
        MSIINSTALLCONTEXT_USERUNMANAGED,
        MSIINSTALLCONTEXT_USERMANAGED,
    ] {
        let mut index: u32 = 0;
        let mut retried = false;
        loop {
            let mut code_buf = vec![0u16; PRODUCT_CODE_BUF];
            let mut sid_buf = vec![0u16; 256];
            let mut sid_len = sid_buf.len() as u32;
            let mut installed_context: u32 = 0;
            // 安全性：缓冲区长度与指针配对传入；szUserSid=NULL 表示当前用户。
            let rc = unsafe {
                MsiEnumProductsExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    context,
                    index,
                    code_buf.as_mut_ptr(),
                    &mut installed_context,
                    sid_buf.as_mut_ptr(),
                    &mut sid_len,
                )
            };
            match rc {
                ERROR_SUCCESS => {
                    retried = false;
                    let code = String::from_utf16_lossy(&code_buf[..PRODUCT_CODE_BUF - 1]);
                    let code = code.trim_end_matches('\0').to_ascii_uppercase();
                    if !code.is_empty() {
                        out.insert(
                            code.clone(),
                            MsiProduct {
                                context,
                                install_location: msi_install_location(&code, context),
                            },
                        );
                    }
                    index += 1;
                }
                ERROR_MORE_DATA if !retried => {
                    // SID 缓冲不足：扩容后重试同一 index（仅一次，防死循环）。
                    retried = true;
                    sid_buf.resize(sid_len as usize + 2, 0);
                }
                ERROR_NO_MORE_ITEMS => {
                    any_context_ok = true;
                    break;
                }
                e => {
                    first_err.get_or_insert(e);
                    break;
                }
            }
        }
    }

    if any_context_ok {
        Ok(out)
    } else {
        Err(first_err.unwrap_or(ERROR_SUCCESS))
    }
}

/// MSI 登记的安装位置（空串视同未知）。
fn msi_install_location(product_code: &str, context: u32) -> Option<String> {
    let code = wide_null(product_code);
    let prop = wide_null(INSTALLPROPERTY_INSTALLLOCATION);
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // 安全性：缓冲区长度与指针配对；len 成功时为字符数（不含 NUL）。
    let rc = unsafe {
        MsiGetProductInfoExW(
            code.as_ptr(),
            std::ptr::null(),
            context,
            prop.as_ptr(),
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// 安装目录判定结果（`Unknown` = 不猜、不计残留）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirCheck {
    Missing,
    Present,
    Unknown,
}

/// 判定登记中的安装目录：空/UNC/相对 → `Unknown`（不触碰、不猜测）。
pub(crate) fn check_install_dir(loc: Option<&str>, exists: impl Fn(&Path) -> bool) -> DirCheck {
    let Some(raw) = loc else {
        return DirCheck::Unknown;
    };
    let expanded = expand_env(raw.trim().trim_matches('"'));
    let s = expanded.trim();
    if s.is_empty() {
        return DirCheck::Unknown;
    }
    // UNC（含 `\\?\` 设备路径）一律不访问。
    if s.starts_with("\\\\") || s.starts_with("//") {
        return DirCheck::Unknown;
    }
    let p = Path::new(s);
    if !p.is_absolute() {
        return DirCheck::Unknown;
    }
    if exists(p) {
        DirCheck::Present
    } else {
        DirCheck::Missing
    }
}

/// 卸载程序判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UninstallerCheck {
    Missing,
    Present,
    /// `msiexec.exe`：系统卸载器必然存在，永不因"卸载程序缺失"判残留。
    SystemMsi,
    /// 裸文件名/相对路径/无法解析：无法可靠判定。
    Unknown,
}

/// 判定卸载命令指向的可执行文件是否存在。
pub(crate) fn check_uninstaller(command: &str, exists: impl Fn(&Path) -> bool) -> UninstallerCheck {
    let Some((exe, _args)) = split_exe_args(command) else {
        return UninstallerCheck::Unknown;
    };
    let base = exe.rsplit(['\\', '/']).next().unwrap_or(&exe);
    if base.eq_ignore_ascii_case("msiexec.exe") || base.eq_ignore_ascii_case("msiexec") {
        return UninstallerCheck::SystemMsi;
    }
    let p = Path::new(&exe);
    if !p.is_absolute() {
        return UninstallerCheck::Unknown;
    }
    if exists(p) {
        UninstallerCheck::Present
    } else {
        UninstallerCheck::Missing
    }
}

/// 单条判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Residue {
        missing_dir: bool,
        missing_uninstaller: bool,
        reason: &'static str,
    },
    Unknown,
    NotResidue,
}

/// 普通（非 MSI）条目：**双条件**才判残留（设计稿 §2.4）。
pub(crate) fn judge_plain(
    install_location: Option<&str>,
    uninstall_command: &str,
    exists: impl Fn(&Path) -> bool,
) -> Verdict {
    let dir = check_install_dir(install_location, &exists);
    let unl = check_uninstaller(uninstall_command, &exists);
    match (dir, unl) {
        (DirCheck::Missing, UninstallerCheck::Missing) => Verdict::Residue {
            missing_dir: true,
            missing_uninstaller: true,
            reason: "安装目录与卸载程序均不存在",
        },
        (DirCheck::Unknown, _) | (_, UninstallerCheck::Unknown) => Verdict::Unknown,
        // 单条件缺失不判残留；msiexec 视为系统卸载器存在。
        _ => Verdict::NotResidue,
    }
}

/// MSI 条目：产品码不再登记 → 残留；安装位置未知 → `Unknown`，不冒充残留。
pub(crate) fn judge_msi(
    product_code: Option<&str>,
    registry_install_location: Option<&str>,
    product: Option<&MsiProduct>,
    exists: impl Fn(&Path) -> bool,
) -> Verdict {
    if product_code.is_none() {
        // 标了 MSI 但拿不到产品码（非 GUID 子键），无从核对。
        return Verdict::Unknown;
    }
    let Some(product) = product else {
        return Verdict::Residue {
            missing_dir: false,
            missing_uninstaller: false,
            reason: "MSI 产品码已不在系统登记中",
        };
    };
    // 优先用 MSI 自己的安装位置，缺失才退回 Uninstall 键的值。
    let loc = product
        .install_location
        .as_deref()
        .or(registry_install_location);
    match check_install_dir(loc, &exists) {
        DirCheck::Missing => Verdict::Residue {
            missing_dir: true,
            missing_uninstaller: false,
            reason: "MSI 登记中的安装位置已不存在",
        },
        DirCheck::Present => Verdict::NotResidue,
        DirCheck::Unknown => Verdict::Unknown,
    }
}

/// 卸载命令是否指向**非 msiexec** 的外部卸载器。
///
/// 用于识别「GUID 形态子键但其实不是 MSI 产品」的注册（WiX Burn bundle 等：
/// 子键是 GUID，`WindowsInstaller` 缺失，卸载串指向 Package Cache 里的 EXE）。
fn is_external_uninstaller(command: &str) -> bool {
    let Some((exe, _args)) = split_exe_args(command) else {
        return false;
    };
    let base = exe.rsplit(['\\', '/']).next().unwrap_or(&exe);
    !(base.eq_ignore_ascii_case("msiexec.exe") || base.eq_ignore_ascii_case("msiexec"))
}

/// 有产品码不代表一定是 MSI 产品（既有 `uninstall::is_msi` 对 GUID 子键是启发式判定，
/// Burn bundle 会被误判）。这里以「MSI 登记里能否查到」为准，查不到时：
/// - 带外部卸载器 → 按普通条目判（否则 21 条 bundle 会全部误报）；
/// - 不带 → 产品码确已注销。
pub(crate) fn judge_msi_or_plain(
    product_code: Option<&str>,
    product: Option<&MsiProduct>,
    registry_install_location: Option<&str>,
    uninstall_command: &str,
    exists: impl Fn(&Path) -> bool,
) -> Verdict {
    if let (Some(code), Some(product)) = (product_code, product) {
        return judge_msi(
            Some(code),
            registry_install_location,
            Some(product),
            &exists,
        );
    }
    if product_code.is_some() && !is_external_uninstaller(uninstall_command) {
        return Verdict::Residue {
            missing_dir: false,
            missing_uninstaller: false,
            reason: "MSI 产品码已不在系统登记中",
        };
    }
    judge_plain(registry_install_location, uninstall_command, exists)
}

/// 补丁类条目（ReleaseType=Update/Rollback）不算残留；读不到则不排除。
fn is_patch_entry(hive: Hive, full_key: &str, wow64: u32) -> bool {
    let Ok(values) = enum_values_view(hive, full_key, wow64) else {
        return false;
    };
    values
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case("ReleaseType"))
        .map(|v| reg_data_to_string(&v.data))
        .is_some_and(|s| {
            s.trim().eq_ignore_ascii_case("Update") || s.trim().eq_ignore_ascii_case("Rollback")
        })
}

fn scope_of(app_scope: &str) -> BootStartupScope {
    if app_scope.eq_ignore_ascii_case("user") {
        BootStartupScope::User
    } else {
        BootStartupScope::Machine
    }
}

/// 采集卸载登记残留（只读）。
pub fn collect() -> BootSection<BootResidueData> {
    let apps = crate::uninstall::list_apps();
    let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
    let located = located_by_ids(&ids);

    let mut failed: Vec<String> = Vec::new();
    let msi_index = match msi_products() {
        Ok(map) => Some(map),
        Err(code) => {
            failed.push(format!("msi_enum_code_{code}"));
            None
        }
    };

    let mut entries = Vec::new();
    let mut unknown_count: u32 = 0;

    for app in &apps {
        let meta = located.get(&app.id);
        let key_name = meta.and_then(|(_, full, _, _)| full.rsplit('\\').next());
        let product_code = key_name.filter(|k| is_msi_guid_key(k));

        let verdict = match &msi_index {
            Some(map) => {
                let product = product_code
                    .map(|c| c.to_ascii_uppercase())
                    .and_then(|c| map.get(&c).cloned());
                judge_msi_or_plain(
                    product_code,
                    product.as_ref(),
                    app.install_location.as_deref(),
                    &app.uninstall_command,
                    |p| p.exists(),
                )
            }
            // MSI 枚举整体失败：不能核对产品码，退回普通双条件判定（不误报）。
            None => judge_plain(
                app.install_location.as_deref(),
                &app.uninstall_command,
                |p| p.exists(),
            ),
        };

        match verdict {
            Verdict::Residue {
                missing_dir,
                missing_uninstaller,
                reason,
            } => {
                // 补丁/更新条目即使双条件成立也不当残留（设计稿 §2.4 排除项）。
                if let Some((hive, full, wow64, _)) = meta {
                    if is_patch_entry(*hive, full, *wow64) {
                        continue;
                    }
                }
                entries.push(BootResidueEntry {
                    id: app.id.clone(),
                    name: app.name.clone(),
                    publisher: app.publisher.clone(),
                    version: app.version.clone(),
                    scope: scope_of(&app.scope),
                    arch: app.arch.clone(),
                    is_msi: app.is_msi,
                    msi_product_code: product_code.map(|c| c.to_ascii_uppercase()),
                    install_location: app.install_location.clone(),
                    uninstall_command: non_empty(&app.uninstall_command),
                    missing_install_dir: missing_dir,
                    missing_uninstaller,
                    reason: reason.to_string(),
                });
            }
            Verdict::Unknown => unknown_count += 1,
            Verdict::NotResidue => {}
        }
    }

    entries.sort_by_key(|e| e.name.to_lowercase());

    BootSection::with_reason(
        if failed.is_empty() {
            BootSectionStatus::Ok
        } else {
            BootSectionStatus::Partial
        },
        super::failed_reason(&failed),
        Some(BootResidueData {
            entries,
            unknown_count,
        }),
    )
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never(_: &Path) -> bool {
        false
    }

    fn always(_: &Path) -> bool {
        true
    }

    #[test]
    fn install_dir_needs_absolute_local_path() {
        assert_eq!(check_install_dir(None, never), DirCheck::Unknown);
        assert_eq!(check_install_dir(Some("   "), never), DirCheck::Unknown);
        assert_eq!(
            check_install_dir(Some(r"\\server\share\app"), never),
            DirCheck::Unknown,
            "UNC 必须不访问"
        );
        assert_eq!(
            check_install_dir(Some(r"relative\dir"), never),
            DirCheck::Unknown
        );
        assert_eq!(
            check_install_dir(Some(r"C:\Program Files\Gone"), never),
            DirCheck::Missing
        );
        assert_eq!(
            check_install_dir(Some(r"C:\Program Files\Here"), always),
            DirCheck::Present
        );
        assert_eq!(
            check_install_dir(Some(r#""C:\Quoted\Dir""#), always),
            DirCheck::Present,
            "注册表值常见带引号"
        );
    }

    #[test]
    fn uninstaller_classification() {
        assert_eq!(check_uninstaller("", never), UninstallerCheck::Unknown);
        assert_eq!(
            check_uninstaller(r"C:\Program Files\App\unins000.exe", never),
            UninstallerCheck::Missing
        );
        assert_eq!(
            check_uninstaller(
                r"msiexec.exe /X{11111111-1111-1111-1111-111111111111}",
                never
            ),
            UninstallerCheck::SystemMsi
        );
        assert_eq!(
            check_uninstaller("unins000.exe", never),
            UninstallerCheck::Unknown,
            "裸文件名可能来自 PATH，不得判缺失"
        );
        assert_eq!(
            check_uninstaller(r"C:\App\unins000.exe /S", always),
            UninstallerCheck::Present
        );
    }

    #[test]
    fn plain_requires_both_conditions() {
        // 双缺失 → 残留。
        assert!(matches!(
            judge_plain(Some(r"C:\Gone\App"), r"C:\Gone\App\unins000.exe", never),
            Verdict::Residue {
                missing_dir: true,
                missing_uninstaller: true,
                ..
            }
        ));
        // 只有目录缺失 → 不判。
        assert_eq!(
            judge_plain(Some(r"C:\Gone\App"), r"C:\App\unins000.exe", always),
            Verdict::NotResidue
        );
        // 只有卸载程序缺失 → 不判。
        assert_eq!(
            judge_plain(Some(r"C:\App"), r"C:\App\unins000.exe", always),
            Verdict::NotResidue
        );
        // 卸载器是 msiexec → 永不因卸载程序缺失判残留。
        assert_eq!(
            judge_plain(Some(r"C:\Gone\App"), "msiexec.exe /X{A}", never),
            Verdict::NotResidue
        );
        // 目录信息缺失 → 未知，不冒充残留。
        assert_eq!(judge_plain(None, "", never), Verdict::Unknown);
        // UNC → 未知。
        assert_eq!(
            judge_plain(Some(r"\\nas\apps\x"), r"C:\x\unins.exe", never),
            Verdict::Unknown
        );
    }

    #[test]
    fn msi_requires_product_registration() {
        let product = MsiProduct {
            context: MSIINSTALLCONTEXT_MACHINE,
            install_location: Some(r"C:\Program Files\Gone".into()),
        };
        assert_eq!(
            judge_msi(
                Some("{11111111-1111-1111-1111-111111111111}"),
                None,
                Some(&product),
                never
            ),
            Verdict::Residue {
                missing_dir: true,
                missing_uninstaller: false,
                reason: "MSI 登记中的安装位置已不存在"
            }
        );
        // 产品码不在枚举结果中 → 登记残留。
        assert!(matches!(
            judge_msi(
                Some("{22222222-2222-2222-2222-222222222222}"),
                None,
                None,
                never
            ),
            Verdict::Residue {
                missing_dir: false,
                missing_uninstaller: false,
                ..
            }
        ));
        // 安装位置未知 → 未知，不判残留。
        let unknown_loc = MsiProduct {
            context: MSIINSTALLCONTEXT_MACHINE,
            install_location: None,
        };
        assert_eq!(
            judge_msi(
                Some("{33333333-3333-3333-3333-333333333333}"),
                None,
                Some(&unknown_loc),
                never
            ),
            Verdict::Unknown
        );
        // 非 GUID 子键但标了 MSI → 未知。
        assert_eq!(
            judge_msi(None, Some(r"C:\X"), None, never),
            Verdict::Unknown
        );
        // 位置存在 → 不是残留。
        assert_eq!(
            judge_msi(
                Some("{44444444-4444-4444-4444-444444444444}"),
                None,
                Some(&product),
                always
            ),
            Verdict::NotResidue
        );
    }

    #[test]
    fn bundle_with_external_uninstaller_is_not_msi_residue() {
        // 真机取证：VC++ 2012 是 WiX Burn bundle——GUID 子键、无 WindowsInstaller、
        // 卸载串指向 Package Cache 里的 EXE。若只按"GUID 子键 ⇒ MSI"会误报 21 条。
        let bundle = r#""C:\ProgramData\Package Cache\{33d1fd90}\vcredist_x86.exe"  /uninstall"#;
        assert_eq!(
            judge_msi_or_plain(
                Some("{33D1FD90-4274-48A1-9BC1-97E33D9C2D6F}"),
                None,
                None,
                bundle,
                never
            ),
            Verdict::Unknown
        );
        // 无外部卸载器（真 MSI 残留）→ 残留。
        assert!(matches!(
            judge_msi_or_plain(
                Some("{11111111-1111-1111-1111-111111111111}"),
                None,
                None,
                "MsiExec.exe /X{11111111-1111-1111-1111-111111111111}",
                never
            ),
            Verdict::Residue {
                missing_dir: false,
                missing_uninstaller: false,
                ..
            }
        ));
        // 空卸载串同样按产品码注销处理。
        assert!(matches!(
            judge_msi_or_plain(
                Some("{22222222-2222-2222-2222-222222222222}"),
                None,
                None,
                "",
                never
            ),
            Verdict::Residue { .. }
        ));
        // 产品已登记 → 走 MSI 判定。
        let product = MsiProduct {
            context: MSIINSTALLCONTEXT_MACHINE,
            install_location: Some(r"C:\Gone".into()),
        };
        assert!(matches!(
            judge_msi_or_plain(
                Some("{33333333-3333-3333-3333-333333333333}"),
                Some(&product),
                None,
                "",
                never
            ),
            Verdict::Residue {
                missing_dir: true,
                missing_uninstaller: false,
                ..
            }
        ));
    }

    /// 真机手工验证（B1-09）：对照「设置 → 应用」逐条复核前 20 条。
    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_residue_prints_section() {
        let t0 = std::time::Instant::now();
        let section = collect();
        println!("apps = {}", crate::uninstall::list_apps().len());
        println!("elapsed_ms = {}", t0.elapsed().as_millis());
        println!(
            "status = {:?}, reason = {:?}",
            section.status, section.reason
        );
        if let Some(d) = &section.data {
            println!(
                "residue = {}, unknown = {}",
                d.entries.len(),
                d.unknown_count
            );
            for e in d.entries.iter().take(20) {
                println!(
                    "{} | {} | {}",
                    e.name,
                    e.reason,
                    e.install_location.clone().unwrap_or_default()
                );
            }
        }
    }

    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_msi_products_prints_count() {
        match msi_products() {
            Ok(map) => println!("msi products = {}", map.len()),
            Err(code) => println!("msi enum failed: {code}"),
        }
    }
}
