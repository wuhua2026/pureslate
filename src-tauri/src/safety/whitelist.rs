//! 白名单（SAFETY §2 路径类强制排除）。
//!
//! 白名单根分两部分，合并后用于 `is_whitelisted` 判定：
//! - **常量根**（P1-02 交付）：系统目录、程序目录、引导目录、用户核心数据目录、自身设施；
//! - **XML 附加根**（P1-04 交付）：`resources/rules/whitelist.xml`，可随规则包更新。
//!
//! §2.5(运行中进程句柄判定) 不属路径级白名单，为独立的一整套句柄快照 ffI 逻辑，
//! 推迟到后续任务处理，避免在未做边界验证前引入脆弱的删除前判定。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use quick_xml::events::Event;
use quick_xml::Reader;

/// 白名单根路径集合（常量部分）。由系统相关环境变量在首次使用时展开一次并缓存。
fn const_roots() -> &'static Vec<PathBuf> {
    static INSTANCE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    INSTANCE.get_or_init(collect_roots)
}

fn env(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// 收集路径级白名单根。
fn collect_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    // §2.1 系统目录（C:\Windows）及其子目录。
    if let Some(root) = env("SystemRoot") {
        v.push(root);
    }
    // §2.2 程序目录。
    if let Some(pf) = env("ProgramFiles") {
        v.push(pf);
    }
    if let Some(pf86) = env("ProgramFiles(x86)") {
        v.push(pf86);
    }
    if let Some(pd) = env("ProgramData") {
        v.push(pd.join("Microsoft"));
    }
    // §2.6 自身设施：%LOCALAPPDATA%\PureSlate（journal/日志/隔离区相关）。
    if let Some(la) = env("LOCALAPPDATA") {
        v.push(la.join("PureSlate"));
    }
    // §2.4 用户核心数据目录（默认 🔴 且不建议清理）。
    if let Some(home) = env("USERPROFILE") {
        for sub in ["Documents", "Desktop", "Pictures", "Videos", "Music"] {
            v.push(home.join(sub));
        }
    }
    v
}

// ===== XML 附加白名单根（resources/rules/whitelist.xml，SAFETY §2 白名单维护）=====

/// 白名单 XML 加载错误。
#[derive(Debug, thiserror::Error)]
pub enum WhitelistError {
    #[error("读取白名单文件失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML 解析失败: {0}")]
    Xml(String),
}

/// 已加载的 XML 附加白名单根。未加载时为 `None`（仅常量根生效）。
static XML_ROOTS: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);

/// 替换/清空 XML 附加白名单根（幂等；调用方在启动/扫描时加载一次）。
/// 传空切片等价于清空附加根，仅保留常量根。
pub fn set_xml_roots(roots: Vec<PathBuf>) {
    let mut guard = XML_ROOTS.lock().expect("whitelist xml roots lock poisoned");
    *guard = Some(roots);
}

/// 当前生效的 XML 附加根（尚未加载时为默认空集）。
fn xml_roots() -> Vec<PathBuf> {
    XML_ROOTS
        .lock()
        .expect("whitelist xml roots lock poisoned")
        .as_ref()
        .cloned()
        .unwrap_or_default()
}

/// 解析 `<whitelist><path>…</path>…</whitelist>` 内容，返回声明的路径根。
/// 空文本忽略；解析失败返回错供调用方记录（不破坏既定常量根）。
pub fn parse_whitelist_xml(xml: &str) -> Result<Vec<PathBuf>, WhitelistError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut roots: Vec<PathBuf> = Vec::new();
    let mut in_path = false;
    let mut buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"path" && !in_path {
                    in_path = true;
                    buf.clear();
                }
            }
            Ok(Event::Empty(_)) | Ok(Event::Decl(_)) => {}
            Ok(Event::Text(ref t)) => {
                if in_path {
                    buf.push_str(
                        &t.unescape()
                            .map_err(|e| WhitelistError::Xml(e.to_string()))?,
                    );
                }
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref() == b"path" && in_path {
                    in_path = false;
                    let v = buf.trim();
                    if !v.is_empty() {
                        roots.push(PathBuf::from(v));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(WhitelistError::Xml(format!(
                    "行 {}: {e}",
                    reader.buffer_position()
                )))
            }
            _ => {}
        }
    }
    Ok(roots)
}

/// 从规则目录加载 `whitelist.xml` 并把声明的路径根合并进全局白名单。
/// 文件不存在视为合法（仅保留常量根，返回 Ok）；解析失败返回错供调用方记录。
pub fn load_from_dir(rules_dir: &Path) -> Result<(), WhitelistError> {
    let path = rules_dir.join("whitelist.xml");
    if !path.exists() {
        return Ok(());
    }
    let xml = std::fs::read_to_string(&path)?;
    let roots = parse_whitelist_xml(&xml)?;
    set_xml_roots(roots);
    Ok(())
}

/// 判断路径是否命中白名单。
///
/// 含系统/引导卷信息目录的逐盘根判断（§2.3）——该类的盘根本身不是白名单根，
/// 但 `C:\Boot`、`C:\EFI`、`C:\Recovery`、`System Volume Information`、
/// `$Recycle.Bin` 均不可触碰。
pub fn is_whitelisted(path: &Path) -> bool {
    // 候选路径与白名单根两侧必须同规（/ → \ + 小写）：规则 XML 常以正斜杠声明
    // target，walkdir 产物为混合分隔符路径；此前候选侧只小写不归一，会导致
    // 段级前缀匹配整体失配 → 白名单被绕过（2026-09-29 黄金集扩测发现）。
    let p_lower = normalize_lower(path);

    // §2.3 盘根下的系统/引导卷信息目录（必须最优先判断，早于 roots 前缀）。
    if let Some(drive) = drive_letter(&p_lower) {
        for child in [
            "boot",
            "efi",
            "recovery",
            "system volume information",
            "$recycle.bin",
        ] {
            let candidate = format!(r"{drive}\{child}");
            if p_lower.trim_end_matches('\\') == candidate
                || p_lower.starts_with(&format!(r"{candidate}\"))
            {
                return true;
            }
        }
    }

    // §2.6 隔离区哨兵：任何卷根下的 `<drive>:\.pureslate-quarantine` 整棵子树
    // 一律白名单（隔离区文件绝不经通用清理路径；SAFETY §2.6）。
    if let Some(drive) = drive_letter(&p_lower) {
        let sentinel = format!(r"{drive}\.pureslate-quarantine");
        if p_lower.trim_end_matches('\\') == sentinel
            || p_lower.starts_with(&format!(r"{sentinel}\"))
        {
            return true;
        }
    }

    // 其余白名单根：常量根 + XML 附加根，段级前缀匹配（避免 `C:\a` 误配 `C:\ab`）。
    if any_root_matches(&p_lower, const_roots()) {
        return true;
    }
    any_root_matches(&p_lower, &xml_roots())
}

/// 段级前缀匹配任一白名单根。
fn any_root_matches(p_lower: &str, roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| prefix_matches(p_lower, &normalize_lower(root)))
}

/// 提取路径的盘符部分（小写），无盘符返回 None。
fn drive_letter(p_lower: &str) -> Option<String> {
    if p_lower.len() >= 2 {
        let b = p_lower.as_bytes();
        let second = b.get(1).copied()?;
        if b[0].is_ascii_alphabetic() && second == b':' {
            return Some(p_lower[..2].to_owned());
        }
    }
    None
}

/// 将路径规范化为小写、`/`→`\`、去掉末尾反斜杠。
fn normalize_lower(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.replace('/', "\\");
    s.trim_end_matches('\\').to_lowercase()
}

/// 段级前缀匹配：`base_lower` 的每一个 `\` 段都必须与 `path_lower` 对应段一致。
fn prefix_matches(path_lower: &str, base_lower: &str) -> bool {
    let path_segs: Vec<&str> = path_lower.split('\\').filter(|s| !s.is_empty()).collect();
    let base_segs: Vec<&str> = base_lower.split('\\').filter(|s| !s.is_empty()).collect();
    if base_segs.len() > path_segs.len() {
        return false;
    }
    base_segs.iter().zip(&path_segs).all(|(b, p)| b == p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelists_system_root() {
        // 用运行环境的真实 SystemRoot 验证自身子树被白名单拦截。
        if let Ok(root) = std::env::var("SystemRoot") {
            let sub = std::path::Path::new(&root).join("System32\\config");
            assert!(is_whitelisted(&sub), "SystemRoot 子树应被白名单挡下");
            assert!(is_whitelisted(std::path::Path::new(&root)));
        }
    }

    #[test]
    fn does_not_whitelist_sibling() {
        // 段级前缀：`C:\Windows32` 不应被 `C:\Windows` 白名单根误配。
        // 直接测段级前缀函数，避免依赖运行环境的具体变量。
        assert!(prefix_matches(r"c:\windows\system32", r"c:\windows"));
        assert!(!prefix_matches(r"c:\windows32\system32", r"c:\windows"));
        assert!(!prefix_matches(r"c:\windo", r"c:\windows"));
        assert!(prefix_matches(r"c:\windows", r"c:\windows"));
    }

    #[test]
    fn boot_volume_dir_whitelisted() {
        assert!(is_whitelisted(Path::new(r"C:\Boot")));
        assert!(is_whitelisted(Path::new(
            r"C:\System Volume Information\cat"
        )));
        assert!(is_whitelisted(Path::new(r"C:\$Recycle.Bin\S-1-5-21")));
    }

    #[test]
    fn quarantine_sentinel_whitelisted() {
        // §2.6 隔离区哨兵：任意盘根的 `.pureslate-quarantine` 子树整棵白名单。
        assert!(is_whitelisted(Path::new(r"C:\.pureslate-quarantine")));
        assert!(is_whitelisted(Path::new(
            r"D:\.pureslate-quarantine\202608\abc\def.tmp"
        )));
        // 相邻名字不应被哨兵误配。
        assert!(!is_whitelisted(Path::new(r"C:\.pureslate-quarantine2")));
    }

    #[test]
    fn case_insensitive_prefix() {
        assert!(is_whitelisted(Path::new(r"c:\windows\system32")));
    }

    #[test]
    fn forward_slash_target_paths_do_not_bypass_whitelist() {
        // 2026-09-29 黄金集扩测发现：规则 XML 以正斜杠声明 target 时，walkdir 产物
        // 为混合分隔符路径，候选侧若不做分隔符归一，段级前缀匹配整体失配 → 白名单
        // 被绕过。修复后正斜杠路径必须同样命中白名单根。
        set_xml_roots(vec![PathBuf::from(r"C:\Users\u\AppData\Local\Temp\keep")]);
        assert!(
            is_whitelisted(Path::new(r"C:/Users/u/AppData/Local/Temp/keep/guard.keep")),
            "正斜杠路径必须命中 XML 白名单根"
        );
        set_xml_roots(vec![]);
        // 常量根同理：SystemRoot 以正斜杠形式出现也不得绕过。
        if let Ok(root) = std::env::var("SystemRoot") {
            let fwd = format!("{}/System32/drivers", root.replace('\\', "/"));
            assert!(
                is_whitelisted(Path::new(&fwd)),
                "正斜杠系统目录不得绕过常量根"
            );
        }
    }

    #[test]
    fn parses_xml_path_roots() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<whitelist>
  <path>C:\pagefile.sys</path>
  <path> D:\safe\stuff </path>
  <path></path>
</whitelist>"#;
        let roots = parse_whitelist_xml(xml).expect("parse ok");
        assert_eq!(
            roots,
            vec![
                PathBuf::from("C:\\pagefile.sys"),
                PathBuf::from("D:\\safe\\stuff")
            ]
        );
    }

    #[test]
    fn malformed_xml_is_error() {
        assert!(parse_whitelist_xml("<whitelist><path></whitelist>").is_err());
    }

    #[test]
    fn xml_added_root_blocks_path() {
        // 一处常量根未见过的路径：注入 XML 根后应被白名单拦截。
        let target = Path::new(r"C:\pagefile.sys");
        set_xml_roots(vec![]);
        assert!(!is_whitelisted(target), "未注入前不应被拦截");
        set_xml_roots(vec![PathBuf::from(r"C:\pagefile.sys")]);
        assert!(is_whitelisted(target), "注入 XML 根后应被拦截");
        // 子树也应被拦截（段级前缀）。
        set_xml_roots(vec![PathBuf::from(r"D:\safe\stuff")]);
        assert!(is_whitelisted(Path::new(r"D:\safe\stuff\sub\1.bin")));
        // 清空后恢复默认（仅常量根）。
        set_xml_roots(vec![]);
        assert!(!is_whitelisted(target));
    }

    #[test]
    fn load_from_dir_missing_file_is_ok() {
        let dir =
            std::env::temp_dir().join(format!("pureslate-whitelist-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 无 whitelist.xml 视为合法。
        assert!(load_from_dir(&dir).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}
