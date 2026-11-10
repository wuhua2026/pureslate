//! 登录触发计划任务（R07 第 5 源）。
//!
//! 枚举：直接读 `%SystemRoot%\System32\Tasks\` 下的任务 XML（Task Scheduler 存储），
//! quick-xml 解析、只保留含 LogonTrigger 的任务；排除 `\Microsoft\` 系统命名空间
//! （系统维护任务，非用户可控启动项，避免几十条噪音与误操作面）。
//! 启停：`schtasks /change /tn <name> /enable|/disable`——任务本体保留在系统中，
//! 满足"禁用不删源"。
//!
//! 已知局限（LESSONS 记录）：任务 XML 不含启用状态 → 未经本应用禁用的任务一律按
//! "启用"显示；本应用自己禁用过的任务由 startup-backup manifest 标记 enabled=false。
//!
//! 失败边界（v0.2.2）：目录不可枚举 / 遍历出错 / XML 解析失败不再静默吞掉，
//! 而是与 `bootdiag/tasks.rs` 同一套 `failed_sources` 语义返回给调用方——
//! 非提权下 0 条必须能被区分成"读不到"，而不是"没有任务"。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use quick_xml::events::Event;
use quick_xml::Reader;

const MAX_FILES: usize = 2_000;

/// 一个登录触发任务。
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// 任务相对路径（如 `Vendor\Update`），即 schtasks /tn 的名字。
    pub name: String,
    pub command: String,
}

/// 任务 XML 解析结果（纯函数，便于单测）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TaskInfo {
    pub has_logon_trigger: bool,
    pub command: String,
}

/// 解析任务 XML：是否含 LogonTrigger + Exec 动作命令行。
/// 解析中途出错：按已收集内容收场（宁缺勿错，不 panic）。
pub fn parse_task_xml(xml: &str) -> TaskInfo {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut info = TaskInfo::default();
    let mut in_logon = 0usize;
    let mut capture: Option<&str> = None;
    let mut command = String::new();
    let mut arguments = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "LogonTrigger" => {
                    info.has_logon_trigger = true;
                    in_logon += 1;
                }
                "Command" if in_logon == 0 => capture = Some("Command"),
                "Arguments" if in_logon == 0 => capture = Some("Arguments"),
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                // <LogonTrigger/>（无子元素的空触发器）同样算登录触发。
                if local_name(e.name().as_ref()) == "LogonTrigger" {
                    info.has_logon_trigger = true;
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "LogonTrigger" if in_logon > 0 => in_logon -= 1,
                "Command" | "Arguments" => capture = None,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some(which) = capture {
                    let text = t.unescape().unwrap_or_default();
                    match which {
                        "Command" => command.push_str(&text),
                        "Arguments" => arguments.push_str(&text),
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

/// 任务存储根：`%SystemRoot%\System32\Tasks`。
fn system_tasks_root() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("WINDIR").map(PathBuf::from))?;
    let p = root.join("System32").join("Tasks");
    p.is_dir().then_some(p)
}

/// 枚举登录触发的第三方计划任务（排除 \Microsoft 系统命名空间）。
///
/// 返回 `(entries, failed_sources)`；失败码与 `bootdiag/tasks.rs` 完全一致：
/// `tasks_root_missing` / `tasks_dir_empty_or_denied` / `tasks_walk_errors_N` /
/// `tasks_parse_failed_N` / `tasks_truncated_2000`。
pub fn collect() -> (Vec<TaskEntry>, Vec<String>) {
    match system_tasks_root() {
        Some(root) => collect_from_root(&root),
        None => (Vec::new(), vec!["tasks_root_missing".to_string()]),
    }
}

/// 从任务存储根枚举（沙箱可注入根目录）。
pub(crate) fn collect_from_root(root: &Path) -> (Vec<TaskEntry>, Vec<String>) {
    let mut out = Vec::new();
    let mut failed = Vec::new();
    let mut xml_files = 0usize;
    let mut walk_errors = 0usize;
    let mut parse_failed = 0usize;

    for entry in walkdir::WalkDir::new(root).max_depth(6) {
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
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        xml_files += 1;
        if xml_files > MAX_FILES {
            failed.push(format!("tasks_truncated_{MAX_FILES}"));
            break;
        }
        let rel_str = rel.to_string_lossy().replace('/', "\\");
        let name = rel_str
            .strip_suffix(".xml")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                rel_str
                    .strip_suffix(".XML")
                    .map(str::to_owned)
                    .unwrap_or(rel_str.clone())
            });
        if name.starts_with("Microsoft\\") {
            continue; // 系统命名空间：不进用户启动项列表
        }
        let Ok(bytes) = fs::read(path) else {
            parse_failed += 1;
            continue;
        };
        let Some(xml) = crate::bootdiag::tasks::decode_task_xml(&bytes) else {
            parse_failed += 1;
            continue;
        };
        let info = parse_task_xml(&xml);
        if !info.has_logon_trigger {
            continue;
        }
        out.push(TaskEntry {
            name,
            command: info.command,
        });
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

/// 启/停计划任务（schtasks，系统工具；无窗口闪烁）。
pub fn set_task_enabled(task_name: &str, enabled: bool) -> bool {
    let flag = if enabled { "/enable" } else { "/disable" };
    let mut cmd = Command::new("schtasks");
    cmd.arg("/change")
        .arg("/tn")
        .arg(task_name)
        .arg(flag)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // CREATE_NO_WINDOW：GUI 进程派生控制台程序时避免闪黑框。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Actions>
    <Exec>
      <Command>C:\Program Files\Vendor\tool.exe</Command>
      <Arguments>--silent --run</Arguments>
    </Exec>
  </Actions>
</Task>"#;

    const NO_LOGON: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <TimeTrigger>
      <StartBoundary>2026-01-01T00:00:00</StartBoundary>
    </TimeTrigger>
  </Triggers>
  <Actions>
    <Exec>
      <Command>C:\x\a.exe</Command>
    </Exec>
  </Actions>
</Task>"#;

    #[test]
    fn parse_logon_trigger_with_exec() {
        let info = parse_task_xml(SAMPLE);
        assert!(info.has_logon_trigger);
        assert_eq!(
            info.command,
            "C:\\Program Files\\Vendor\\tool.exe --silent --run"
        );
    }

    #[test]
    fn parse_time_trigger_ignored() {
        let info = parse_task_xml(NO_LOGON);
        assert!(!info.has_logon_trigger);
        assert_eq!(info.command, "C:\\x\\a.exe");
    }

    #[test]
    fn parse_namespaced_and_empty_trigger() {
        let xml = r#"<t:Task xmlns:t="urn:x">
          <Triggers><t:LogonTrigger/></Triggers>
          <Actions><t:Exec><t:Command>D:\svc\run.cmd</t:Command></t:Exec></Actions>
        </t:Task>"#;
        let info = parse_task_xml(xml);
        assert!(info.has_logon_trigger);
        assert_eq!(info.command, "D:\\svc\\run.cmd");
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pureslate-tasks-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn utf16le_with_bom(s: &str) -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    }

    #[test]
    fn collect_from_root_filters_microsoft_and_non_logon() {
        let dir = temp_dir("filter");
        fs::create_dir_all(dir.join("Microsoft\\Windows")).unwrap();
        fs::create_dir_all(dir.join("Vendor")).unwrap();
        fs::write(dir.join("Microsoft\\Windows\\SysTask.xml"), SAMPLE).unwrap();
        fs::write(dir.join("Vendor\\TimeOnly.xml"), NO_LOGON).unwrap();
        fs::write(dir.join("Vendor\\Logon.xml"), SAMPLE).unwrap();

        let (out, failed) = collect_from_root(&dir);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Vendor\\Logon");
        assert!(failed.is_empty(), "正常目录不应产生失败子源: {failed:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_from_root_reads_utf16_task_xml() {
        let dir = temp_dir("utf16");
        fs::create_dir_all(dir.join("Vendor")).unwrap();
        fs::write(dir.join("Vendor\\Utf16.xml"), utf16le_with_bom(SAMPLE)).unwrap();

        let (out, failed) = collect_from_root(&dir);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Vendor\\Utf16");
        assert!(failed.is_empty(), "{failed:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_dir_is_reported_as_unreadable_not_absent() {
        let dir = temp_dir("empty");

        let (out, failed) = collect_from_root(&dir);
        assert!(out.is_empty());
        assert_eq!(failed, vec!["tasks_dir_empty_or_denied"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonexistent_root_surfaces_walk_error() {
        let dir = std::env::temp_dir().join("pureslate-tasks-does-not-exist");
        let (out, failed) = collect_from_root(&dir);
        assert!(out.is_empty());
        assert!(
            failed.contains(&"tasks_dir_empty_or_denied".to_string()),
            "{failed:?}"
        );
        assert!(
            failed.iter().any(|f| f.starts_with("tasks_walk_errors_")),
            "{failed:?}"
        );
    }

    #[test]
    fn unreadable_xml_counted_as_parse_failure() {
        let dir = temp_dir("badxml");
        fs::create_dir_all(dir.join("Vendor")).unwrap();
        fs::write(dir.join("Vendor\\Broken.xml"), [0xEF, 0xBB, 0xBF, 0xFF]).unwrap();

        let (out, failed) = collect_from_root(&dir);
        assert!(out.is_empty());
        assert!(
            failed.contains(&"tasks_parse_failed_1".to_string()),
            "{failed:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "真机验证：打印非提权下的来源失败边界"]
    fn live_collect_prints_sources() {
        let (out, failed) = collect();
        println!("LIVE_TASKS entries={} failed={failed:?}", out.len());
    }
}
