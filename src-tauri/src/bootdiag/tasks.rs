//! 开机/登录触发计划任务（只读）：直接读 `%SystemRoot%\System32\Tasks` 下的任务 XML。
//!
//! 只读纪律：只 `fs::read`，不调用 `schtasks /change`、不写任务存储。
//!
//! 与既有 `startup/tasks.rs` 的差异（设计稿 §3.4）：
//! - 不沿用"读不到就 `continue`"：目录为空/遍历出错/解析失败都计入 `failed_sources`；
//! - 同时识别 `BootTrigger` 与 `LogonTrigger`；
//! - 编码按 BOM 判定（UTF-16LE/BE、UTF-8），无 BOM 时用零字节分布启发式，
//!   不依赖 `schtasks` 的本地化文本输出（实测为 CP936）。
//! - 任务 XML 不携带可靠启用状态 → `enabled = None`，不臆造。

use std::fs;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::Reader;
use walkdir::WalkDir;

use crate::contract::{BootStartupEntry, BootStartupKind, BootStartupScope};

use super::startup_sources::entry_id;

const MAX_FILES: usize = 2_000;

/// 解析结果（纯结构，便于单测）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TaskInfo {
    pub has_boot_trigger: bool,
    pub has_logon_trigger: bool,
    pub command: String,
}

impl TaskInfo {
    fn trigger_kind(&self) -> Option<(BootStartupKind, &'static str)> {
        match (self.has_boot_trigger, self.has_logon_trigger) {
            (true, true) => Some((BootStartupKind::ScheduledTaskBoot, "boot+logon")),
            (true, false) => Some((BootStartupKind::ScheduledTaskBoot, "boot")),
            (false, true) => Some((BootStartupKind::ScheduledTaskLogon, "logon")),
            (false, false) => None,
        }
    }
}

/// 任务 XML 解码：BOM 优先；无 BOM 时按零字节比例判断 UTF-16LE。
pub(crate) fn decode_task_xml(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(utf16le(&bytes[2..]));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (units, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = units.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec()).ok();
    }
    // 必须先于 `from_utf8`：无 BOM 的 UTF-16LE 纯 ASCII 文本也是合法 UTF-8
    // （NUL 是合法 UTF-8 字节），先试 UTF-8 会把 `TimeTrigger` 解成 `T\0i\0m\0…`。
    if looks_utf16le(bytes) {
        return Some(utf16le(bytes));
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some(s.to_string());
    }
    // 非法 UTF-8 且不像 UTF-16：按损失转换，至少保留结构信息。
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn utf16le(bytes: &[u8]) -> String {
    let (units, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = units.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&units)
}

/// 零字节启发式：UTF-16LE ASCII 文本每隔一字节为 0。
fn looks_utf16le(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(512)];
    let zeros = sample.iter().filter(|b| **b == 0).count();
    sample.len() > 8 && zeros * 3 > sample.len()
}

/// 解析任务 XML：是否含 Boot/Logon 触发器 + Exec 动作命令行。
pub(crate) fn parse_task_xml(xml: &str) -> TaskInfo {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut info = TaskInfo::default();
    let mut in_trigger = 0usize;
    let mut capture: Option<&'static str> = None;
    let mut command = String::new();
    let mut arguments = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "BootTrigger" => {
                    info.has_boot_trigger = true;
                    in_trigger += 1;
                }
                "LogonTrigger" => {
                    info.has_logon_trigger = true;
                    in_trigger += 1;
                }
                "Command" if in_trigger == 0 => capture = Some("command"),
                "Arguments" if in_trigger == 0 => capture = Some("arguments"),
                _ => {}
            },
            Ok(Event::Empty(e)) => match local_name(e.name().as_ref()) {
                // 无子元素的空触发器同样算命中。
                "BootTrigger" => info.has_boot_trigger = true,
                "LogonTrigger" => info.has_logon_trigger = true,
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "BootTrigger" | "LogonTrigger" if in_trigger > 0 => in_trigger -= 1,
                "Command" | "Arguments" => capture = None,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some(which) = capture {
                    let text = t.unescape().unwrap_or_default();
                    match which {
                        "command" => command.push_str(&text),
                        "arguments" => arguments.push_str(&text),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let mut full = command;
    if !arguments.is_empty() {
        if !full.is_empty() {
            full.push(' ');
        }
        full.push_str(&arguments);
    }
    info.command = full;
    info
}

fn local_name(qname: &[u8]) -> &str {
    let s = std::str::from_utf8(qname).unwrap_or("");
    s.rsplit(':').next().unwrap_or("")
}

fn system_tasks_root() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("WINDIR").map(PathBuf::from))?;
    let p = root.join("System32").join("Tasks");
    p.is_dir().then_some(p)
}

/// 组装任务条目（纯函数）。
pub(crate) fn task_entry(
    name: &str,
    command: &str,
    kind: BootStartupKind,
    detail: &str,
) -> BootStartupEntry {
    BootStartupEntry {
        id: entry_id(kind, BootStartupScope::Machine, name, command),
        name: name.to_string(),
        command: command.to_string(),
        kind,
        scope: BootStartupScope::Machine,
        enabled: None,
        approved_state: None,
        delayed_auto_start: None,
        detail: Some(detail.to_string()),
    }
}

/// 从任务存储根枚举（沙箱可注入根目录）。
pub(crate) fn collect_from_root(root: &Path) -> (Vec<BootStartupEntry>, Vec<String>) {
    let mut out = Vec::new();
    let mut failed = Vec::new();
    let mut xml_files = 0usize;
    let mut walk_errors = 0usize;
    let mut parse_failed = 0usize;

    for entry in WalkDir::new(root).max_depth(6) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                walk_errors += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        {
            continue;
        }
        xml_files += 1;
        if xml_files > MAX_FILES {
            failed.push(format!("tasks_truncated_{MAX_FILES}"));
            break;
        }
        let Ok(bytes) = fs::read(path) else {
            parse_failed += 1;
            continue;
        };
        let Some(xml) = decode_task_xml(&bytes) else {
            parse_failed += 1;
            continue;
        };
        let info = parse_task_xml(&xml);
        let Some((kind, detail)) = info.trigger_kind() else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('/', "\\");
        let name = rel
            .strip_suffix(".xml")
            .or_else(|| rel.strip_suffix(".XML"))
            .unwrap_or(&rel)
            .to_string();
        out.push(task_entry(&name, &info.command, kind, detail));
    }

    if xml_files == 0 {
        // 非提权实测：目录可 stat 但枚举为空 —— 必须与"确实没有任务"区分。
        failed.push("tasks_dir_empty_or_denied".to_string());
    }
    if walk_errors > 0 {
        failed.push(format!("tasks_walk_errors_{walk_errors}"));
    }
    if parse_failed > 0 {
        failed.push(format!("tasks_parse_failed_{parse_failed}"));
    }
    (out, failed)
}

/// 枚举开机/登录触发任务（含 `\Microsoft\` 系统命名空间，UI 默认折叠）。
pub fn collect() -> (Vec<BootStartupEntry>, Vec<String>) {
    match system_tasks_root() {
        Some(root) => collect_from_root(&root),
        None => (Vec::new(), vec!["tasks_root_missing".to_string()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTF16_BOOT: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers><BootTrigger/></Triggers>
  <Actions><Exec><Command>C:\Vendor\boot.exe</Command><Arguments>/run</Arguments></Exec></Actions>
</Task>"#;

    const LOGON_AND_BOOT: &str = r#"<t:Task xmlns:t="urn:x">
  <t:Triggers><t:LogonTrigger/><t:BootTrigger/></t:Triggers>
  <t:Actions><t:Exec><t:Command>D:\svc\run.cmd</t:Command></t:Exec></t:Actions>
</t:Task>"#;

    const TIME_ONLY: &str = r#"<Task><Triggers><TimeTrigger><StartBoundary>2026-01-01T00:00:00</StartBoundary></TimeTrigger></Triggers>
<Actions><Exec><Command>C:\x\a.exe</Command></Exec></Actions></Task>"#;

    fn utf16le_with_bom(s: &str) -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    }

    #[test]
    fn decodes_utf16le_bom() {
        let bytes = utf16le_with_bom(UTF16_BOOT);
        let xml = decode_task_xml(&bytes).unwrap();
        assert!(xml.contains("BootTrigger"));
        assert!(parse_task_xml(&xml).has_boot_trigger);
    }

    #[test]
    fn decodes_utf16be_bom() {
        let mut v = vec![0xFE, 0xFF];
        for u in TIME_ONLY.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        let xml = decode_task_xml(&v).unwrap();
        assert!(xml.contains("TimeTrigger"));
    }

    #[test]
    fn decodes_utf8_and_bomless_utf16() {
        assert!(decode_task_xml(UTF16_BOOT.as_bytes()).is_some());
        let mut raw = Vec::new();
        for u in TIME_ONLY.encode_utf16() {
            raw.extend_from_slice(&u.to_le_bytes());
        }
        assert!(looks_utf16le(&raw));
        assert!(decode_task_xml(&raw).unwrap().contains("TimeTrigger"));
    }

    #[test]
    fn parses_boot_and_logon_triggers() {
        let info = parse_task_xml(LOGON_AND_BOOT);
        assert!(info.has_boot_trigger);
        assert!(info.has_logon_trigger);
        assert_eq!(info.command, "D:\\svc\\run.cmd");
        assert_eq!(
            info.trigger_kind(),
            Some((BootStartupKind::ScheduledTaskBoot, "boot+logon"))
        );
    }

    #[test]
    fn time_only_task_is_excluded() {
        let info = parse_task_xml(TIME_ONLY);
        assert_eq!(info.trigger_kind(), None);
    }

    #[test]
    fn collect_from_root_reports_empty_dir() {
        let dir = std::env::temp_dir().join(format!(
            "pureslate-bootdiag-tasks-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("Microsoft\\Windows")).unwrap();
        fs::create_dir_all(dir.join("Vendor")).unwrap();
        fs::write(
            dir.join("Microsoft\\Windows\\SysBoot.xml"),
            utf16le_with_bom(UTF16_BOOT),
        )
        .unwrap();
        fs::write(dir.join("Vendor\\TimeOnly.xml"), TIME_ONLY).unwrap();
        fs::write(dir.join("Vendor\\Login.xml"), utf16le_with_bom(UTF16_BOOT)).unwrap();

        let (entries, failed) = collect_from_root(&dir);
        assert!(failed.is_empty(), "failed = {failed:?}");
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|e| e.name == "Microsoft\\Windows\\SysBoot"));
        assert!(entries
            .iter()
            .all(|e| e.kind == BootStartupKind::ScheduledTaskBoot));

        let empty = dir.join("Vendor\\Empty");
        fs::create_dir_all(&empty).unwrap();
        let (entries, failed) = collect_from_root(&empty);
        assert!(entries.is_empty());
        assert_eq!(failed, vec!["tasks_dir_empty_or_denied"]);

        let _ = fs::remove_dir_all(&dir);
    }

    /// 真机手工验证（B1-09）：提权应接近 `Get-ScheduledTask` 的 Boot/Logon 计数；
    /// 非提权应为 0 条 + `tasks_dir_empty_or_denied`（正是既有缺口）。
    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_tasks_prints_count() {
        let (entries, failed) = collect();
        println!("tasks = {}, failed = {:?}", entries.len(), failed);
        for e in entries.iter().take(8) {
            println!("{} -> {}", e.name, e.command);
        }
    }
}
