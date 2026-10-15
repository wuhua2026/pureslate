//! 安全卸载辅助（R27 · SPEC §6.7）。

pub mod icons;
pub mod residue;
pub mod runner;

use sha2::{Digest, Sha256};

use crate::contract::InstalledApp;
use crate::logging::audit;
use crate::scanner::walk::CancelToken;
use crate::startup::winreg::{
    enum_subkeys_view, enum_values_view, reg_display_value, DisplayField, Hive, RegValue,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SZ,
};
use runner::{classify, launch, pending_covers, pending_reboot_entries, split_exe_args, wait_tree};

pub use runner::{OutcomeKind, WaitConclusion};

/// Uninstall 根键。
const UNINSTALL_SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const REG_DWORD: u32 = 4;

/// R30：DisplayName 值存在但编码损坏到不可辨识时的占位名——条目仍保留
/// （用户可能正想卸载这类老程序），名称不伪装成可识别内容。
const FALLBACK_DISPLAY_NAME: &str = "异常名称的程序";

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
    // R30：展示类字段走防御解码（老中文安装器坏 REG_SZ → 乱码/占位）；
    // 值根本不存在才跳过条目，值存在但不可辨识则保留并给占位名。
    let name = match reg_display_value(values, "DisplayName", DisplayField::Name) {
        Some(n) => n,
        None => {
            if has_sz_value(values, "DisplayName") {
                FALLBACK_DISPLAY_NAME.to_string()
            } else {
                return None;
            }
        }
    };

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
        // R30：展示字段防御解码（乱码 → None，UI 显示"未知发布者/—"）；
        // InstallLocation 是路径语义，必须与原值一致，不走猜测性解码。
        publisher: reg_display_value(values, "Publisher", DisplayField::Publisher),
        version: reg_display_value(values, "DisplayVersion", DisplayField::Version),
        install_date: reg_display_value(values, "InstallDate", DisplayField::Date),
        install_location: string_value(values, "InstallLocation"),
        uninstall_command,
        is_msi,
        arch,
        scope,
        estimated_size_bytes,
        priority,
    })
}

/// 值集合中是否存在指定名称的 SZ/EXPAND_SZ 值（不论内容能否解码）。
fn has_sz_value(values: &[RegValue], name: &str) -> bool {
    values
        .iter()
        .any(|v| v.name.eq_ignore_ascii_case(name) && (v.kind == REG_SZ || v.kind == REG_EXPAND_SZ))
}

/// 跨视图去重键（name+publisher+version，全部小写；None 按空串）。
fn dedupe_key(app: &RawApp) -> (String, String, String) {
    (
        app.name.to_lowercase(),
        app.publisher.as_deref().unwrap_or("").to_lowercase(),
        app.version.as_deref().unwrap_or("").to_lowercase(),
    )
}

/// 跨视图去重（纯函数，泛型单份）：同键保留首次出现者；
/// 输入须按 VIEWS 顺序（priority 升序）枚举，首次即最高优先级。
fn dedupe_by<T>(items: Vec<T>, key_of: impl Fn(&T) -> (String, String, String)) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        if seen.insert(key_of(&item)) {
            out.push(item);
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

/// RawApp + 其注册表物理位置（图标提取等内部用途：避免按 id 回查时重复枚举）。
/// 模块私有：仅 list_apps/located_by_ids 在本模块内消费，不泄露 RawApp 类型。
struct LocatedApp {
    id: String,
    app: RawApp,
    hive: Hive,
    full_key: String,
    wow64: u32,
}

/// 枚举全部已安装程序（只读）。任一视图/子键失败均跳过（宁缺勿错，不中断整体）。
fn enumerate_located() -> Vec<LocatedApp> {
    let mut located: Vec<(RawApp, Hive, String, u32)> = Vec::new();
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
                located.push((app, view.hive, full, view.wow64));
            }
        }
    }
    // 去重键与列表口径完全一致，保证视图选择不漂移。
    let located: Vec<LocatedApp> = located
        .into_iter()
        .map(|(app, hive, full_key, wow64)| LocatedApp {
            id: stable_id(app.scope, app.arch, &app.key_name),
            app,
            hive,
            full_key,
            wow64,
        })
        .collect();
    dedupe_by(located, |l| dedupe_key(&l.app))
}

/// 测试辅助：暴露物理位置/命令/名称（仅 cfg(test)；供图标模块视觉转储测试）。
#[cfg(test)]
pub(crate) fn enumerate_located_test() -> Vec<(
    Hive,
    String,
    u32,
    String, /*uninstall_command*/
    String, /*name*/
)> {
    enumerate_located()
        .into_iter()
        .map(|l| {
            (
                l.hive,
                l.full_key,
                l.wow64,
                l.app.uninstall_command,
                l.app.name,
            )
        })
        .collect()
}

/// 枚举全部已安装程序（只读）。任一视图/子键失败均跳过（宁缺勿错，不中断整体）。
pub fn list_apps() -> Vec<InstalledApp> {
    let mut apps: Vec<RawApp> = enumerate_located().into_iter().map(|l| l.app).collect();
    // 名称不区分大小写排序（跨语种稳定）。
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.iter().map(RawApp::to_contract).collect()
}

/// R30：按 id 批量取条目的注册表物理位置（图标 IPC 一次枚举、批量消费）。
pub(crate) fn located_by_ids(
    ids: &[String],
) -> std::collections::HashMap<String, (Hive, String, u32, String)> {
    let want: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    enumerate_located()
        .into_iter()
        .filter(|l| want.contains(l.id.as_str()))
        .map(|l| (l.id, (l.hive, l.full_key, l.wow64, l.app.uninstall_command)))
        .collect()
}

/// 卸载执行结果（R27-2）。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub app_id: String,
    pub name: String,
    /// R27-3：发布者（残留快照名称匹配需要）。
    pub publisher: Option<String>,
    pub kind: OutcomeKind,
    /// 卸载器主进程退出码（非 0 仅作警告；成功判据以事后重扫为准）。
    pub exit_code: Option<u32>,
    /// Uninstall 键事后是否仍在。
    pub key_still_present: bool,
    /// 安装目录事后是否仍在（None=注册表里没有 InstallLocation）。
    pub install_location_still_present: Option<bool>,
    /// R27-3：登记的安装目录原值（供残留扫描；None=未登记）。
    pub install_location: Option<String>,
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
            publisher: app.publisher.clone(),
            kind: OutcomeKind::Cancelled,
            exit_code,
            key_still_present: true,
            install_location_still_present: None,
            install_location: app.install_location.clone(),
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
        publisher: app.publisher,
        kind,
        exit_code,
        key_still_present,
        install_location_still_present,
        install_location: app.install_location,
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
        let out = dedupe_by(
            vec![
                mk(0, "machine", "x64", "P"),
                mk(1, "machine", "x86", "P"),
                mk(2, "user", "x64", "P"),
            ],
            dedupe_key,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].scope, "machine");
        assert_eq!(out[0].arch, "x64");

        // publisher 不同 → 视为不同程序，均保留。
        let out2 = dedupe_by(
            vec![mk(0, "machine", "x64", "A"), mk(1, "machine", "x86", "B")],
            dedupe_key,
        );
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
    ///
    /// 错误语义区分（2026-10-13 CI 修正）：
    /// - `ERROR_FILE_NOT_FOUND/ERROR_PATH_NOT_FOUND(2/3)`：该视图键在本用户配置单元
    ///   不存在（GitHub Actions 干净 runner 的 HKCU 即如此）——生产枚举本就容错跳过，
    ///   合法；
    /// - `ERROR_ACCESS_DENIED(5)`：提权进程仍被拒 = KEY_READ/WOW64 标志错误的信号
    ///   （正是 R27-1 事故），必须失败；其他错误同样报出。
    #[test]
    fn all_four_views_open() {
        for view in &VIEWS {
            let keys = enum_subkeys_view(view.hive, UNINSTALL_SUBKEY, view.wow64);
            if let Err(e) = &keys {
                let code = e.raw_os_error().unwrap_or(0);
                assert!(
                    matches!(code, 2 | 3),
                    "{}/{} 视图 Uninstall 键打开失败（非 NotFound，疑 WOW64/权限标志错误 code={code}）: {e:?}",
                    view.scope,
                    view.arch
                );
            }
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

    // ---- R30：坏编码防御解码（真机：ANSI 时代中文安装器的乱码 Uninstall 项） ----

    /// 极域电子教室 的 GBK(936) 字节（老中文安装器实际写入形态之一）。
    const GBK_JIYU_DIANZI_JIAOSHI: &[u8] = &[
        0xBC, 0xAB, // 极
        0xD3, 0xF2, // 域
        0xB5, 0xE7, // 电
        0xD7, 0xD3, // 子
        0xBD, 0xCC, // 教
        0xCA, 0xD2, // 室
    ];
    /// 极域 二字 GBK 字节。
    const GBK_JIYU: &[u8] = &[0xBC, 0xAB, 0xD3, 0xF2];

    /// 直接用原始字节构造 REG_SZ（不经 UTF-16，模拟 ANSI 安装器直写）。
    fn raw_sz(name: &str, data: &[u8]) -> RegValue {
        RegValue {
            name: name.into(),
            kind: REG_SZ,
            data: data.to_vec(),
        }
    }

    /// 构造真机典型坏数据：UTF-16 英文头 + NUL + 原始 GBK 尾。
    fn mixed_head_gbk(head: &str, tail: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = head.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(tail);
        bytes
    }

    #[test]
    fn r30_pure_gbk_display_name_decodes() {
        // 整段 GBK 字节：UTF-16 直解必然乱码，936 候选应还原中文。
        let vals = vec![
            raw_sz("DisplayName", GBK_JIYU_DIANZI_JIAOSHI),
            sz("UninstallString", "x.exe"),
        ];
        let app = parse(&vals, "machine", "x64", 0).expect("应解析");
        eprintln!("pure GBK decode = {}", app.name);
        assert_eq!(app.name, "极域电子教室");
    }

    #[test]
    fn r30_mixed_utf16_head_gbk_tail_repairs() {
        // 真机形态：UTF-16 "Minisoft Toolbox" + GBK 尾。
        let data = mixed_head_gbk("Minisoft Toolbox", GBK_JIYU);
        let vals = vec![raw_sz("DisplayName", &data), sz("UninstallString", "x.exe")];
        let app = parse(&vals, "machine", "x64", 0).expect("应解析");
        eprintln!("mixed decode = {}", app.name);
        assert_eq!(app.name, "Minisoft Toolbox极域");
    }

    #[test]
    fn r30_valid_utf16_chinese_unchanged() {
        // 正常现代中文条目不得被防御逻辑改动（腾讯科技(深圳)有限公司）。
        let vals = vec![
            sz("DisplayName", "腾讯科技(深圳)有限公司"),
            sz("Publisher", "腾讯科技(深圳)有限公司"),
            sz("DisplayVersion", "9.9.28"),
        ];
        let app = parse(&vals, "machine", "x64", 0).expect("应解析");
        assert_eq!(app.name, "腾讯科技(深圳)有限公司");
        assert_eq!(app.publisher.as_deref(), Some("腾讯科技(深圳)有限公司"));
        assert_eq!(app.version.as_deref(), Some("9.9.28"));
    }

    #[test]
    fn r30_unrecoverable_garbage_falls_back_and_aux_fields_hidden() {
        // 含控制符/随机字节，三种候选都不可信 → 占位名；发布者/版本隐藏。
        let garbage = [0x01u8, 0x02, 0xE0, 0xAA, 0x55, 0x00, 0x7F, 0x80];
        let vals = vec![
            raw_sz("DisplayName", &garbage),
            raw_sz("Publisher", &garbage),
            raw_sz("DisplayVersion", &garbage),
            sz("UninstallString", "x.exe"),
        ];
        let app = parse(&vals, "machine", "x64", 0).expect("值存在就应保留条目");
        assert_eq!(app.name, "异常名称的程序");
        assert_eq!(app.publisher, None);
        assert_eq!(app.version, None);
    }

    #[test]
    fn r30_garbled_version_and_bad_date_hidden() {
        // 截图同款：ASCII 版本号头 + GBK 乱码尾 → Version 形态校验拒绝。
        let ver_data = mixed_head_gbk("1.0.0.93", GBK_JIYU);
        let vals = vec![
            sz("DisplayName", "某程序"),
            raw_sz("DisplayVersion", &ver_data),
            sz("InstallDate", "2026093x"),
        ];
        let app = parse(&vals, "machine", "x64", 0).unwrap();
        assert_eq!(app.version, None);
        assert_eq!(app.install_date, None);

        // 合法日期保留。
        let vals2 = vec![sz("DisplayName", "某程序"), sz("InstallDate", "20260930")];
        let app2 = parse(&vals2, "machine", "x64", 0).unwrap();
        assert_eq!(app2.install_date.as_deref(), Some("20260930"));
    }

    #[test]
    fn r30_missing_display_name_still_skipped() {
        // 值不存在（区别于"值存在但坏"）→ 条目跳过，不给占位。
        assert!(parse(&[sz("Publisher", "x")], "machine", "x64", 0).is_none());
    }
}
