//! 安全卸载辅助（R27 · SPEC §6.7）。

pub mod runner;

use sha2::{Digest, Sha256};

use crate::contract::InstalledApp;
use crate::logging::audit;
use crate::scanner::walk::CancelToken;
use crate::startup::winreg::{
    enum_subkeys_view, enum_values_view, Hive, RegValue, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_EXPAND_SZ, REG_SZ,
};
use runner::{classify, launch, pending_covers, pending_reboot_entries, split_exe_args, wait_tree};

pub use runner::{OutcomeKind, WaitConclusion};

/// Uninstall 根键。
const UNINSTALL_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const REG_DWORD: u32 = 4;

/// 一个视图：根键 × WOW64 标志。priority 小者优先（去重保留高优先级条目）。
struct View {
    hive: Hive,
    wow64: u32,
    scope: &'static str,
    arch: &'static str,
    priority: u8,
}

/// HKLM64 > HKLM32 > HKCU64 > HKCU32（机器级、64 位视图优先）。
const VIEWS: [View; 4] = [
    View {
        hive: Hive::LocalMachine,
        wow64: KEY_WOW64_64KEY,
        scope: "machine",
        arch: "x64",
        priority: 0,
    },
    View {
        hive: Hive::LocalMachine,
        wow64: KEY_WOW64_32KEY,
        scope: "machine",
        arch: "x86",
        priority: 1,
    },
    View {
        hive: Hive::CurrentUser,
        wow64: KEY_WOW64_64KEY,
        scope: "user",
        arch: "x64",
        priority: 2,
    },
    View {
        hive: Hive::CurrentUser,
        wow64: KEY_WOW64_32KEY,
        scope: "user",
        arch: "x86",
        priority: 3,
    },
];

/// 枚举中间态（携带去重优先级与注册表子键名）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawApp {
    key_name: String,
    name: String,
    publisher: Option<String>,
    version: Option<String>,
    install_date: Option<String>,
    install_location: Option<String>,
    uninstall_command: String,
    is_msi: bool,
    arch: &'static str,
    scope: &'static str,
    estimated_size_bytes: Option<u64>,
    priority: u8,
}

/// 稳定 ID：sha256(scope|arch|子键名) 前 16 位 hex。
fn stable_id(scope: &str, arch: &str, key_name: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("{scope}|{arch}|{key_name}").as_bytes());
    let d = h.finalize();
    d[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// 取字符串类值（SZ/EXPAND_SZ），空白归一为 None。
fn string_value(values: &[RegValue], name: &str) -> Option<String> {
    values.iter().find_map(|v| {
        if v.name.eq_ignore_ascii_case(name) && (v.kind == REG_SZ || v.kind == REG_EXPAND_SZ) {
            let s = crate::startup::winreg::reg_data_to_string(&v.data);
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        } else {
            None
        }
    })
}

/// 取 DWORD（小端 u32）。
fn dword_value(values: &[RegValue], name: &str) -> Option<u32> {
    values.iter().find_map(|v| {
        if v.name.eq_ignore_ascii_case(name) && v.kind == REG_DWORD && v.data.len() >= 4 {
            Some(u32::from_le_bytes([
                v.data[0], v.data[1], v.data[2], v.data[3],
            ]))
        } else {
            None
        }
    })
}

/// 子键名是否为 MSI 产品 GUID 形态 `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`。
pub fn is_msi_guid_key(key_name: &str) -> bool {
    let s = key_name;
    let b = s.as_bytes();
    if b.len() != 38 || b[0] != b'{' || b[37] != b'}' {
        return false;
    }
    // 8-4-4-4-12 十六进制分段，连字符位置固定。
    let dash = [9, 14, 19, 24];
    if dash.iter().any(|&i| b[i] != b'-') {
        return false;
    }
    s[1..37].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// 从一个 Uninstall 子键的值集合解析条目。
/// 过滤规则（SPEC §6.7）：DisplayName 必填；SystemComponent≠0 跳过；
/// ParentKeyName 非空（补丁/更新子项）跳过。
fn parse_app(
    key_name: &str,
    values: &[RegValue],
    scope: &'static str,
    arch: &'static str,
    priority: u8,
) -> Option<RawApp> {
    let name = string_value(values, "DisplayName")?;

    if dword_value(values, "SystemComponent").is_some_and(|v| v != 0) {
        return None;
    }
    if string_value(values, "ParentKeyName").is_some() {
        return None;
    }

    let is_msi = dword_value(values, "WindowsInstaller").is_some_and(|v| v != 0)
        || is_msi_guid_key(key_name);

    // 卸载命令：**只取交互 UninstallString**（SPEC §6.7：R27 编排交互卸载器
    // SW_SHOWNORMAL，用户必须看到官方 UI）；QuietUninstallString 是静默串，
    // 留给后续专家模式选项，这里永不取用；MSI 缺省时构造 msiexec /X{GUID}。
    let uninstall_command = match string_value(values, "UninstallString") {
        Some(s) => s,
        None if is_msi => format!("msiexec /X{key_name}"),
        None => String::new(),
    };

    // EstimatedSize 注册表单位 KB → 字节。
    let estimated_size_bytes = dword_value(values, "EstimatedSize").map(|kb| u64::from(kb) * 1024);

    Some(RawApp {
        key_name: key_name.to_string(),
        name,
        publisher: string_value(values, "Publisher"),
        version: string_value(values, "DisplayVersion"),
        install_date: string_value(values, "InstallDate"),
        install_location: string_value(values, "InstallLocation"),
        uninstall_command,
        is_msi,
        arch,
        scope,
        estimated_size_bytes,
        priority,
    })
}

/// 跨视图去重（纯函数）：同 name+publisher+version 保留 priority 最小者；
/// 输入为按 VIEWS 顺序（priority 升序）枚举的结果，同键首次出现即保留。
fn dedupe(apps: Vec<RawApp>) -> Vec<RawApp> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for app in apps {
        let key = (
            app.name.to_lowercase(),
            app.publisher.as_deref().unwrap_or("").to_lowercase(),
            app.version.as_deref().unwrap_or("").to_lowercase(),
        );
        if seen.insert(key) {
            out.push(app);
        }
    }
    out
}

impl RawApp {
    fn to_contract(&self) -> InstalledApp {
        InstalledApp {
            id: stable_id(self.scope, self.arch, &self.key_name),
            name: self.name.clone(),
            publisher: self.publisher.clone(),
            version: self.version.clone(),
            install_date: self.install_date.clone(),
            install_location: self.install_location.clone(),
            uninstall_command: self.uninstall_command.clone(),
            is_msi: self.is_msi,
            arch: self.arch.to_string(),
            scope: self.scope.to_string(),
            estimated_size_bytes: self.estimated_size_bytes,
        }
    }
}

/// 枚举全部已安装程序（只读）。任一视图/子键失败均跳过（宁缺勿错，不中断整体）。
pub fn list_apps() -> Vec<InstalledApp> {
    let mut raws: Vec<RawApp> = Vec::new();
    for view in &VIEWS {
        let Ok(keys) = enum_subkeys_view(view.hive, UNINSTALL_SUBKEY, view.wow64) else {
            continue;
        };
        for key in keys {
            let full = format!("{UNINSTALL_SUBKEY}\\{key}");
            let Ok(values) = enum_values_view(view.hive, &full, view.wow64) else {
                continue;
            };
            if let Some(app) = parse_app(&key, &values, view.scope, view.arch, view.priority) {
                raws.push(app);
            }
        }
    }
    let mut apps = dedupe(raws);
    // 名称不区分大小写排序（跨语种稳定）。
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.iter().map(RawApp::to_contract).collect()
}

/// 卸载执行结果（R27-2）。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub app_id: String,
    pub name: String,
    pub kind: OutcomeKind,
    /// 卸载器主进程退出码（非 0 仅作警告；成功判据以事后重扫为准）。
    pub exit_code: Option<u32>,
    /// Uninstall 键事后是否仍在。
    pub key_still_present: bool,
    /// 安装目录事后是否仍在（None=注册表里没有 InstallLocation）。
    pub install_location_still_present: Option<bool>,
    /// 目标路径已进 PendingFileRenameOperations（重启后完成）。
    pub reboot_pending: bool,
    /// 最终展示文案。
    pub detail: String,
}

/// 执行一次卸载编排（SPEC §6.7）。
///
/// 流程：重扫定位条目（防陈旧列表）→ 审计拉起 → 解析/还原命令行 → 拉起官方卸载器
/// → 等待进程树（取消不强杀）→ 事后重扫 Uninstall 键/安装目录 + 待重启检测 → 分类 → 审计。
/// 破坏主体是第三方卸载器：本函数不自删任何文件（红线边界，UI 须如实告知）。
pub fn run_app_uninstall<F: FnMut(&str)>(
    app_id: &str,
    cancel: &CancelToken,
    mut on_phase: F,
) -> Result<RunOutcome, String> {
    // ① 重扫定位（不用调用时的陈旧快照；条目可能已被其他途径卸载）。
    on_phase("正在定位程序…");
    let app = list_apps()
        .into_iter()
        .find(|a| a.id == app_id)
        .ok_or_else(|| "该程序已不在可卸载列表中（可能已卸载）".to_string())?;
    audit_uninstall(&app, "start", Some(&app.uninstall_command), None);

    // ② 解析卸载命令行（环境变量展开/引号/.exe 边界；8.3 还原在 launch 内）。
    let (exe, args) = split_exe_args(&app.uninstall_command)
        .ok_or_else(|| "该程序没有可用的卸载命令".to_string())?;

    // ③ 拉起官方卸载器（其窗口由用户交互；740 时走 runas 标准 UAC）。
    on_phase("已调起官方卸载器，请在弹出的窗口中完成卸载…");
    let launched = match launch(&exe, &args) {
        Ok(l) => l,
        Err(e) => {
            audit_uninstall(&app, "launch_failed", None, None);
            return Err(e);
        }
    };

    // ④ 等待进程树（取消 = 停止等待，不强杀）。
    let wait = wait_tree(&launched, cancel, &mut on_phase);
    let exit_code = runner::exit_code(&launched);
    drop(launched);

    // ⑤ 取消：不做事后判定（卸载器可能仍在运行，结论无意义）。
    if wait == WaitConclusion::Cancelled {
        let out = RunOutcome {
            app_id: app.id.clone(),
            name: app.name.clone(),
            kind: OutcomeKind::Cancelled,
            exit_code,
            key_still_present: true,
            install_location_still_present: None,
            reboot_pending: false,
            detail: "已停止等待；官方卸载器可能仍在运行，请在其窗口内完成或取消".into(),
        };
        audit_uninstall(&app, "cancelled", None, None);
        return Ok(out);
    }

    // ⑥ 事后重扫（SPEC §6.7 三层判据）。
    on_phase("正在核对卸载结果…");
    let key_still_present = list_apps().iter().any(|a| a.id == app_id);
    let install_location_still_present = app
        .install_location
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|p| std::path::Path::new(p).exists());
    let pending = pending_reboot_entries();
    let reboot_pending = app
        .install_location
        .as_deref()
        .map(|loc| pending_covers(&pending, loc))
        .unwrap_or(false)
        || pending
            .iter()
            .any(|e| e.to_lowercase().contains(&app.name.to_lowercase()));
    let kind = classify(wait, key_still_present, reboot_pending);

    let detail = match kind {
        OutcomeKind::Removed => {
            if install_location_still_present == Some(true) {
                "程序已卸载；安装目录仍有残留（残留隔离清理将在后续版本提供）".to_string()
            } else if let Some(c) = exit_code {
                if c != 0 {
                    format!("卸载条目已移除（卸载器退出码 {c}，非 0——建议核对程序是否完整卸载）")
                } else {
                    "程序已卸载".to_string()
                }
            } else {
                "程序已卸载".to_string()
            }
        }
        OutcomeKind::RebootPending => "卸载将在重启电脑后完成（系统已登记待删除文件）".to_string(),
        OutcomeKind::StillPresent => {
            "卸载器已退出，但该程序仍在列表中（可能被取消或卸载失败）".to_string()
        }
        OutcomeKind::Timeout => {
            "卸载器子进程长时间未退出；已停止等待，请检查是否有未完成的卸载窗口".to_string()
        }
        OutcomeKind::Cancelled => String::new(),
    };
    audit_uninstall(
        &app,
        match kind {
            OutcomeKind::Removed => "removed",
            OutcomeKind::RebootPending => "reboot_pending",
            OutcomeKind::StillPresent => "still_present",
            OutcomeKind::Timeout => "timeout",
            OutcomeKind::Cancelled => "cancelled",
        },
        None,
        exit_code,
    );

    Ok(RunOutcome {
        app_id: app.id,
        name: app.name,
        kind,
        exit_code,
        key_still_present,
        install_location_still_present,
        reboot_pending,
        detail,
    })
}

/// 卸载审计（op=uninstall，SPEC §4.4）。
fn audit_uninstall(app: &InstalledApp, phase: &str, path: Option<&str>, exit_code: Option<u32>) {
    let mut detail = format!("{} | phase={}", app.name, phase);
    if let Some(c) = exit_code {
        detail.push_str(&format!(" | exitCode={c}"));
    }
    let _ = audit::record(&crate::contract::LogEntry {
        ts: audit::now_ms(),
        op: "uninstall".into(),
        tx_id: Some(app.id.clone()),
        category_id: None,
        path: path.map(str::to_string),
        size_bytes: None,
        disposition: None,
        result: Some(phase.to_string()),
        detail: Some(detail),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sz(name: &str, data: &str) -> RegValue {
        // UTF-16LE + 结尾 NUL（与 advapi32 REG_SZ 线格式一致）。
        let mut bytes: Vec<u8> = data.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        bytes.extend_from_slice(&[0, 0]);
        RegValue {
            name: name.into(),
            kind: REG_SZ,
            data: bytes,
        }
    }

    fn dw(name: &str, v: u32) -> RegValue {
        RegValue {
            name: name.into(),
            kind: REG_DWORD,
            data: v.to_le_bytes().to_vec(),
        }
    }

    fn parse(
        values: &[RegValue],
        scope: &'static str,
        arch: &'static str,
        prio: u8,
    ) -> Option<RawApp> {
        parse_app("TestKey", values, scope, arch, prio)
    }

    #[test]
    fn parses_full_entry() {
        let vals = vec![
            sz("DisplayName", "示例软件  "),
            sz("Publisher", "某公司"),
            sz("DisplayVersion", "2.1.0"),
            sz("InstallDate", "20260930"),
            sz("InstallLocation", r"C:\Program Files\App"),
            sz("UninstallString", r#""C:\Program Files\App\un.exe" /S"#),
            dw("EstimatedSize", 12_345),
        ];
        let app = parse(&vals, "machine", "x64", 0).expect("应解析");
        assert_eq!(app.name, "示例软件"); // trim 生效
        assert_eq!(app.publisher.as_deref(), Some("某公司"));
        assert_eq!(app.version.as_deref(), Some("2.1.0"));
        assert_eq!(app.install_date.as_deref(), Some("20260930"));
        assert!(app.uninstall_command.contains("un.exe"));
        assert_eq!(app.estimated_size_bytes, Some(12_345 * 1024));
        assert!(!app.is_msi);
    }

    #[test]
    fn skips_without_display_name() {
        assert!(parse(&[sz("Publisher", "x")], "machine", "x64", 0).is_none());
    }

    #[test]
    fn skips_system_component_and_parent_key() {
        let mut v = vec![sz("DisplayName", "系统组件"), dw("SystemComponent", 1)];
        assert!(parse(&v, "machine", "x64", 0).is_none());
        v = vec![
            sz("DisplayName", "某补丁"),
            sz("ParentKeyName", "SomeParent"),
        ];
        assert!(parse(&v, "machine", "x64", 0).is_none());
    }

    #[test]
    fn msi_detection_and_constructed_command() {
        // WindowsInstaller=1 且无 UninstallString → 构造 msiexec /X{GUID}。
        let guid = "{12345678-90AB-CDEF-1234-567890ABCDEF}";
        let vals = vec![sz("DisplayName", "MSI 产品"), dw("WindowsInstaller", 1)];
        let app = parse_app(guid, &vals, "machine", "x64", 0).expect("应解析");
        assert!(app.is_msi);
        assert_eq!(app.uninstall_command, format!("msiexec /X{guid}"));

        // 仅 GUID 键名也判 MSI。
        let app2 = parse_app(guid, &[sz("DisplayName", "X")], "user", "x86", 2).unwrap();
        assert!(app2.is_msi);
        assert!(app2.uninstall_command.contains("/X"));
    }

    #[test]
    fn never_uses_quiet_uninstall_string() {
        // 回归（R27-1 遗留）：代码曾误取 QuietUninstallString——SPEC §6.7 要求
        // 只编排交互卸载器，静默串永不取用。
        let vals = vec![
            sz("DisplayName", "双串程序"),
            sz("UninstallString", r#""C:\App\un.exe" interactive"#),
            sz("QuietUninstallString", r#""C:\App\un.exe" /VERYSILENT"#),
        ];
        let app = parse(&vals, "machine", "x64", 0).expect("应解析");
        assert_eq!(app.uninstall_command, r#""C:\App\un.exe" interactive"#);

        // 只有 QuietUninstallString（无交互串）且非 MSI → 命令留空（拒绝静默拉起）。
        let vals2 = vec![
            sz("DisplayName", "仅静默串"),
            sz("QuietUninstallString", r#""C:\App\un.exe" /S"#),
        ];
        let app2 = parse(&vals2, "machine", "x64", 0).unwrap();
        assert!(app2.uninstall_command.is_empty());
    }

    #[test]
    fn non_msi_without_command_has_empty_command() {
        let app = parse(&[sz("DisplayName", "无卸载串")], "user", "x86", 2).unwrap();
        assert!(!app.is_msi);
        assert!(app.uninstall_command.is_empty());
    }

    #[test]
    fn guid_key_shape_validation() {
        assert!(is_msi_guid_key("{12345678-90AB-CDEF-1234-567890ABCDEF}"));
        assert!(is_msi_guid_key("{12345678-90ab-cdef-1234-567890abcdef}"));
        assert!(!is_msi_guid_key("12345678-90AB-CDEF-1234-567890ABCDEF")); // 无花括号
        assert!(!is_msi_guid_key("{1234567-90AB-CDEF-1234-567890ABCDEF}")); // 段长错
        assert!(!is_msi_guid_key("{GGGG5678-90AB-CDEF-1234-567890ABCDEF}")); // 非十六进制
        assert!(!is_msi_guid_key("NotAGuid"));
    }

    #[test]
    fn dedupe_keeps_higher_priority_view() {
        let mk = |prio: u8, scope: &'static str, arch: &'static str, publisher: &str| RawApp {
            key_name: format!("k{prio}"),
            name: "Same App".into(),
            publisher: Some(publisher.into()),
            version: Some("1.0".into()),
            install_date: None,
            install_location: None,
            uninstall_command: "x".into(),
            is_msi: false,
            arch,
            scope,
            estimated_size_bytes: None,
            priority: prio,
        };
        // 输入按枚举顺序（prio 升序）：HKLM64 先到 → 保留。
        let out = dedupe(vec![
            mk(0, "machine", "x64", "P"),
            mk(1, "machine", "x86", "P"),
            mk(2, "user", "x64", "P"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].scope, "machine");
        assert_eq!(out[0].arch, "x64");

        // publisher 不同 → 视为不同程序，均保留。
        let out2 = dedupe(vec![
            mk(0, "machine", "x64", "A"),
            mk(1, "machine", "x86", "B"),
        ]);
        assert_eq!(out2.len(), 2);
    }

    #[test]
    fn id_is_stable_and_unique_across_locations() {
        let a = RawApp {
            key_name: "K".into(),
            name: "A".into(),
            publisher: None,
            version: None,
            install_date: None,
            install_location: None,
            uninstall_command: String::new(),
            is_msi: false,
            arch: "x64",
            scope: "machine",
            estimated_size_bytes: None,
            priority: 0,
        };
        let c1 = a.to_contract();
        let c2 = a.to_contract();
        assert_eq!(c1.id, c2.id, "同位置重复枚举 ID 须稳定");
        assert_eq!(c1.id.len(), 16);
        // 同名但位于不同视图（HKCU vs HKLM）→ ID 不同。
        let mut b = a.clone();
        b.scope = "user";
        assert_ne!(c1.id, b.to_contract().id);
    }

    /// 回归（覆盖率事故 2026-10-12）：WOW64 标志必须为 winnt.h 原值 0x0100/0x0200——
    /// 曾误写为 0x10000/0x20000 导致 HKLM 两视图打开被静默跳过（只剩 HKCU 十几项）。
    #[test]
    fn all_four_views_open() {
        for view in &VIEWS {
            let keys = enum_subkeys_view(view.hive, UNINSTALL_SUBKEY, view.wow64);
            assert!(
                keys.is_ok(),
                "{}/{} 视图 Uninstall 键必须可打开（WOW64 标志错误？）: {:?}",
                view.scope,
                view.arch,
                keys.err()
            );
        }
    }

    /// 真机只读冒烟：枚举不 panic，条目名非空、ID 唯一、arch/scope 合法。
    #[test]
    fn live_enumeration_smoke() {
        let apps = list_apps();
        let mut ids = std::collections::HashSet::new();
        for app in &apps {
            assert!(!app.name.trim().is_empty());
            assert!(app.arch == "x64" || app.arch == "x86");
            assert!(app.scope == "machine" || app.scope == "user");
            assert!(ids.insert(app.id.clone()), "ID 重复: {}", app.id);
            if app.is_msi && app.uninstall_command.contains("msiexec") {
                assert!(
                    app.uninstall_command.contains("/X"),
                    "MSI 构造命令必须为 /X 卸载形态"
                );
            }
        }
        // 开发机至少装了 PureSlate 自身依赖链上的运行时（或任意程序）；
        // 空列表不致 CI 假失败——仅断言不 panic + 不变量，数量不要求。
        eprintln!("live installed apps: {}", apps.len());
    }
}
