//! 白名单（SAFETY §2.1–4、§2.6 路径类强制排除）。
//!
//! P1-02 交付路径级最小集：系统目录、程序目录、引导目录、用户核心数据目录、自身设施。
//! §2.5(进程句柄)、whitelist.xml 加载在 P1-04 (R02) 落地。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 白名单根路径集合。由系统相关环境变量在首次使用时展开一次并缓存。
fn roots() -> &'static Vec<PathBuf> {
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

/// 判断路径是否命中白名单。
///
/// 含系统/引导卷信息目录的逐盘根判断（§2.3）——该类的盘根本身不是白名单根，
/// 但 `C:\Boot`、`C:\EFI`、`C:\Recovery`、`System Volume Information`、
/// `$Recycle.Bin` 均不可触碰。
pub fn is_whitelisted(path: &Path) -> bool {
    let p = path.to_string_lossy();
    let p_lower = p.to_lowercase();

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

    // 其余白名单根：段级前缀匹配（避免 `C:\a` 误配 `C:\ab`）。
    roots()
        .iter()
        .any(|root| prefix_matches(&p_lower, &normalize_lower(root)))
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
    fn case_insensitive_prefix() {
        assert!(is_whitelisted(Path::new(r"c:\windows\system32")));
    }
}
