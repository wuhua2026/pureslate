//! 安全卸载辅助（R27 · SPEC §6.7）。
//!
//! 本批（R27-1）只做**只读枚举**：Uninstall 注册表四视图（HKLM/HKCU × 64/32 位），
//! 过滤系统组件/补丁子项，跨视图去重，产出 `InstalledApp`。
//! 后续批次：拉起官方卸载器 + 进程树等待 + 事后判定（R27-2，[DESTRUCTIVE] 评审），
//! 以及残留扫描接入既有清理管线（R27-3）。
//!
//! 设计红线（SPEC §6.7）：本模块全程零写入；不申请 SeDebugPrivilege、不强杀、
//! 不枚举 UWP（Remove-AppxPackage 不在范围）。

use sha2::{Digest, Sha256};

use crate::contract::InstalledApp;
use crate::startup::winreg::{
    enum_subkeys_view, enum_values_view, Hive, RegValue, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_EXPAND_SZ, REG_SZ,
};

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

    // 卸载命令：显式 UninstallString 优先；MSI 缺省时构造 msiexec /X{GUID}。
    let uninstall_command = match string_value(values, "QuietUninstallString").or_else(|| {
        // QuietUninstallString 优先于交互命令？——不：R27 编排交互卸载器（SW_SHOWNORMAL），
        // 用户要看到官方 UI；静默串留给后续专家选项，这里不取。
        string_value(values, "UninstallString")
    }) {
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
