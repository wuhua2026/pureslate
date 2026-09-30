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

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use quick_xml::events::Event;
use quick_xml::Reader;

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
pub fn enumerate_logon_tasks() -> Vec<TaskEntry> {
    let Some(root) = system_tasks_root() else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_xml = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xml"));
        if !is_xml {
            continue;
        }
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
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
        let Ok(xml) = fs::read_to_string(path) else {
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
    out
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

/// 测试辅助：由任务存储根路径枚举（沙箱可注入）。
#[cfg(test)]
pub(crate) fn enumerate_from_root(root: &std::path::Path) -> Vec<TaskEntry> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
        {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let name = rel.to_string_lossy().replace('/', "\\");
        let name = name.strip_suffix(".xml").unwrap_or(&name).to_string();
        let Ok(xml) = fs::read_to_string(path) else {
            continue;
        };
        let info = parse_task_xml(&xml);
        if !info.has_logon_trigger || name.starts_with("Microsoft\\") {
            continue;
        }
        out.push(TaskEntry {
            name,
            command: info.command,
        });
    }
    out
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

    #[test]
    fn enumerate_from_root_filters_microsoft_and_non_logon() {
        let dir = std::env::temp_dir().join(format!(
            "pureslate-tasks-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("Microsoft\\Windows")).unwrap();
        fs::create_dir_all(dir.join("Vendor")).unwrap();
        fs::write(dir.join("Microsoft\\Windows\\SysTask.xml"), SAMPLE).unwrap();
        fs::write(dir.join("Vendor\\TimeOnly.xml"), NO_LOGON).unwrap();
        fs::write(dir.join("Vendor\\Logon.xml"), SAMPLE).unwrap();

        let out = enumerate_from_root(&dir);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Vendor\\Logon");

        let _ = fs::remove_dir_all(&dir);
    }
}
