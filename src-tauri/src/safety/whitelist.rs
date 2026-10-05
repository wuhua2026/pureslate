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

use crate::contract::Disposition;

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
    // §2.4 用户核心数据目录**不再入通用白名单根**（H1 · v0.1.4）：
    // 它们对 quarantine 去向的扫描/清理放行（见 `is_excluded_from_scan` /
    // `is_user_core_data`），对 direct/recycle 去向仍整树拦截（preflight）。
    // 原实现把五目录混入通用根，导致 dup 维度 4/5 的 target 被自家白名单
    // 整树剪除（只读扫描被"禁删"语义误伤）。
    v
}

/// §2.4 用户核心数据目录的段级根（相对用户主目录，小写）。
/// 纯函数（home 注入）供测试与 `is_user_core_data` 复用。
fn user_core_data_seg_roots(home: &Path) -> Vec<Vec<String>> {
    let home_segs = normalize_segments(home);
    ["documents", "desktop", "pictures", "videos", "music"]
        .iter()
        .map(|sub| {
            let mut segs = home_segs.clone();
            segs.push((*sub).to_string());
            segs
        })
        .collect()
}

/// 路径是否位于 §2.4 用户核心数据目录（Documents/Desktop/Pictures/Videos/Music）。
/// H1（v0.1.4）语义：这些目录对 **quarantine 去向**（隔离区可还原 + 用户逐项确认）
/// 不再构成扫描/清理禁区；对 direct/recycle 去向仍整树拦截（preflight 强制）。
pub fn is_user_core_data(path: &Path) -> bool {
    if let Some(home) = env("USERPROFILE") {
        let p_segs = normalize_segments(path);
        return user_core_data_seg_roots(&home)
            .iter()
            .any(|root| prefix_matches(&p_segs, root));
    }
    false
}

/// 扫描期保护判定（H1 · v0.1.4）：白名单根全类目拦截；§2.4 用户核心目录仅对
/// **非 quarantine** 去向整树拦截。walk/mft/dup 的过滤统一走本函数。
pub fn is_excluded_from_scan(path: &Path, disposition: Disposition) -> bool {
    if is_whitelisted(path) {
        return true;
    }
    disposition != Disposition::Quarantine && is_user_core_data(path)
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
///
/// **T-4 声明侧加固（P4-04，安全审计「白名单字符串旁路形态」）**：声明若以 8.3
/// 短名书写（`PROGRA~1`/`GUARD~1.KEE` 等），长名候选永不匹配 → 保护静默丢失。
/// 加载时对每条根做 `GetLongPathNameW` 展开（best-effort：文件不存在/卷禁用 8.3
/// 时保留原形式），长短两种形式并存——匹配只会更保守，不会更宽松。
pub fn set_xml_roots(roots: Vec<PathBuf>) {
    let mut norm: Vec<PathBuf> = Vec::with_capacity(roots.len() * 2);
    for r in roots {
        if let Some(long) = expand_short_name(&r) {
            if long != r && !norm.contains(&long) {
                norm.push(long);
            }
        }
        if !norm.contains(&r) {
            norm.push(r);
        }
    }
    // M6（v0.1.4 · 红线 #6）：锁中毒时取守卫继续（into_inner）——白名单是保护
    // 机构，panic 比"读到一个被污染的旧值"更不可接受；与全库抗中毒模式一致。
    let mut guard = XML_ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(norm);
}

/// 短名路径 best-effort 归一为长名（失败返回原路径，调用方语义不变）。
/// 供两处使用：①`set_xml_roots` 白名单根展开；②scanner target 归一——提权时
/// MFT 引擎产物为长名路径（MFT 存长名），短名 target 会使前缀匹配整体失配
/// （CI 提权环境实测：扫描 0 项）。
pub(crate) fn normalize_to_long_path(p: &Path) -> PathBuf {
    expand_short_name(p).unwrap_or_else(|| p.to_path_buf())
}

/// 8.3 短名 → 长名（Windows；失败返回 None）。非 Windows 恒 None。
#[cfg(windows)]
fn expand_short_name(p: &Path) -> Option<PathBuf> {
    // unsafe：GetLongPathNameW 单次调用（固定足量缓冲）。kernel32 默认链接。
    // 注意：不用 NULL/0 two-call 惯用法——本机实测该 API 对 NULL 缓冲不可靠
    // （CI 上返回 0 导致展开静默失败；LESSONS ① GetShortPathNameW 同族教训）。
    unsafe extern "system" {
        fn GetLongPathNameW(
            lpsz_short_path: *const u16,
            lpsz_long_path: *mut u16,
            cch_buffer: u32,
        ) -> u32;
    }
    let wide: Vec<u16> = p
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    const CAP: usize = 4096;
    let mut buf = vec![0u16; CAP];
    let written = unsafe { GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), CAP as u32) };
    if written == 0 || written as usize >= CAP {
        return None; // 失败或缓冲不足（极端长路径保守放弃，退回原形式）
    }
    buf.truncate(written as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buf)))
}

#[cfg(not(windows))]
fn expand_short_name(_p: &Path) -> Option<PathBuf> {
    None
}

/// 当前生效的 XML 附加根（尚未加载时为默认空集）。
fn xml_roots() -> Vec<PathBuf> {
    // M6（v0.1.4 · 红线 #6）：同 set_xml_roots——中毒取守卫而非 panic。
    XML_ROOTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
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
/// **候选侧短名展开（T-4 镜像形态，P4-06 CI 实测补全）**：环境变量来源的路径
/// （如 CI 的 `%TEMP%` = `C:\Users\RUNNER~1\...`）可能天然含 8.3 短名段，而
/// 白名单根已展开为长名——段级匹配失配即保护缺口。仅当路径含 `~N` 短名段时
/// 才做 `GetLongPathNameW` 展开（成功时长/短两形态都判定），常规长名路径
/// **零额外系统调用**（扫描热路径保障）。
///
/// 含系统/引导卷信息目录的逐盘根判断（§2.3）——该类的盘根本身不是白名单根，
/// 但 `C:\Boot`、`C:\EFI`、`C:\Recovery`、`System Volume Information`、
/// `$Recycle.Bin` 均不可触碰。
pub fn is_whitelisted(path: &Path) -> bool {
    if path_has_short_component(path) {
        if let Some(long) = expand_short_name(path) {
            let hit = whitelisted_once(path) || whitelisted_once(&long);
            return hit;
        }
    }
    whitelisted_once(path)
}

/// 8.3 短名段特征：任一路径段含 `~` 且其后紧跟 ASCII 数字（`RUNNER~1`）。
/// 宽松检测（真名含 `~1` 的用户文件会多一次失败的展开调用并退回原形，无碍）。
fn path_has_short_component(p: &Path) -> bool {
    p.to_string_lossy().split(['\\', '/']).any(|seg| {
        seg.find('~').is_some_and(|i| {
            seg[i + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
    })
}

/// 单形态白名单判定（盘根特例 + 常量根 + XML 附加根）。
fn whitelisted_once(path: &Path) -> bool {
    // 候选路径与白名单根两侧共用段级归一（T-4，P4-04）：`/`→`\` + 小写 +
    // `\\?\` 剥离 + `..`/`.` 段文本化解析 + 逐段去尾随点/空格——任何一侧的
    // 书写形式差异都不再造成保护缺口（此前仅分隔符+小写归一，2026-09-29 曾
    // 因正斜杠失配整体绕过；同族形态见安全审计 T-4）。
    let p_segs = normalize_segments(path);
    let p_lower = p_segs.join("\\");

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

    // §2.1 卷根系统关键文件（pagefile/hiberfil/swapfile/dumpstack 等）：
    // 任何盘根下这些名字永不入候选——常量判定，不依赖 whitelist.xml 的加载时序
    // （2026-09-30 真机抽查发现：XML 未加载时 pagefile.sys 曾漏进 large 候选）。
    // 仅匹配卷根一层，子目录同名用户文件不受影响。
    if let Some(drive) = drive_letter(&p_lower) {
        let root_file = p_lower
            .strip_prefix(&drive)
            .unwrap_or("")
            .trim_start_matches('\\');
        if !root_file.contains('\\')
            && matches!(
                root_file,
                "pagefile.sys"
                    | "hiberfil.sys"
                    | "swapfile.sys"
                    | "dumpstack.log"
                    | "dumpstack.log.tmp"
            )
        {
            return true;
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
    if any_root_matches(&p_segs, const_roots()) {
        return true;
    }
    any_root_matches(&p_segs, &xml_roots())
}

/// 段级前缀匹配任一白名单根（根侧同样走共用归一，T-4）。
fn any_root_matches(path_segs: &[String], roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| prefix_matches(path_segs, &normalize_segments(root)))
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

/// 段级归一（白名单根与候选路径**两侧共用**，T-4 · P4-04）：
/// `\\?\` 前缀剥离、`/`→`\`、小写、跳过空段与 `.` 段、`..` 段文本化弹栈、
/// 逐段剥 Windows 创建时会被 Win32 吃掉的尾随点/空格（`guard.keep.` 与
/// `guard.keep` 在 NTFS 语义上是同一个名字）。
/// 保护方向单调：归一只会让两侧**更容易对上**（更保守），不会扩大扫除面。
fn normalize_segments(p: &Path) -> Vec<String> {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let s = s.replace('/', "\\");
    let mut out: Vec<String> = Vec::new();
    for raw in s.split('\\') {
        if raw == ".." {
            out.pop();
            continue;
        }
        let seg = raw.trim_end_matches(['.', ' ']);
        if seg.is_empty() || seg == "." {
            continue;
        }
        out.push(seg.to_lowercase());
    }
    out
}

/// 段级前缀匹配：`base_segs` 的每一段都必须与 `path_segs` 对应段一致。
fn prefix_matches(path_segs: &[String], base_segs: &[String]) -> bool {
    base_segs.len() <= path_segs.len() && base_segs.iter().zip(path_segs).all(|(b, p)| b == p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XML_ROOTS 全局测试锁：触碰 set_xml_roots 的用例必须持锁整个测试体
    /// （并行竞态会互相清根，LESSONS ① 模式）。
    static XML_ROOTS_LOCK: Mutex<()> = Mutex::new(());

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
        let segs = |p: &str| normalize_segments(Path::new(p));
        assert!(prefix_matches(
            &segs(r"c:\windows\system32"),
            &segs(r"c:\windows")
        ));
        assert!(!prefix_matches(
            &segs(r"c:\windows32\system32"),
            &segs(r"c:\windows")
        ));
        assert!(!prefix_matches(&segs(r"c:\windo"), &segs(r"c:\windows")));
        assert!(prefix_matches(&segs(r"c:\windows"), &segs(r"c:\windows")));
    }

    #[test]
    fn t4_dot_and_dotdot_segments_resolve() {
        // T-4（P4-04）：声明含 `.`/`..` 段时必须文本化解析，否则保护静默丢失
        // （旧归一只处理分隔符+小写，`..` 段使段级前缀永不匹配）。
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_xml_roots(vec![PathBuf::from(
            r"C:\Users\u\AppData\Local\Temp\keep\sub\.\guard.dat",
        )]);
        assert!(
            is_whitelisted(Path::new(
                r"C:\Users\u\AppData\Local\Temp\keep\sub\guard.dat"
            )),
            "`.` 段声明必须解析后命中"
        );
        set_xml_roots(vec![PathBuf::from(
            r"C:\Users\u\AppData\Local\Temp\keep\sub\..\guard2.dat",
        )]);
        assert!(
            is_whitelisted(Path::new(r"C:\Users\u\AppData\Local\Temp\keep\guard2.dat")),
            "`..` 段声明必须弹栈后命中"
        );
        set_xml_roots(vec![]);
    }

    #[test]
    fn t4_trailing_dot_or_space_segments_normalize() {
        // T-4（P4-04）：Win32 创建路径会吃掉段尾点/空格——`guard.dat.` 与
        // `guard.dat` 在 NTFS 语义上同名；声明或候选任一侧带尾随点时必须命中。
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_xml_roots(vec![PathBuf::from(
            r"C:\Users\u\AppData\Local\Temp\keep\guard3.dat.",
        )]);
        assert!(
            is_whitelisted(Path::new(r"C:\Users\u\AppData\Local\Temp\keep\guard3.dat")),
            "声明尾随点必须归一后命中"
        );
        // 反向：候选带字面尾随点（经 \\?\ 创建的罕见形态）同样命中同一声明。
        assert!(
            is_whitelisted(Path::new(r"C:\Users\u\AppData\Local\Temp\keep\guard3.dat.")),
            "候选字面尾随点必须归一后命中"
        );
        set_xml_roots(vec![PathBuf::from(
            r"C:\Users\u\AppData\Local\Temp\keep\guard4 .dat",
        )]);
        assert!(
            is_whitelisted(Path::new(r"C:\Users\u\AppData\Local\Temp\keep\guard4 .dat")),
            "段内空格不受影响（只剥段尾）"
        );
        set_xml_roots(vec![]);
    }

    #[test]
    fn t4_short_name_declaration_expands_to_long() {
        // T-4（P4-04）：8.3 短名声明的白名单根必须经 GetLongPathNameW 展开为
        // 长名并存，否则长名候选永不匹配（保护静默丢失）。卷禁用 8.3 时跳过。
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "pureslate-wl-short-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let long = dir.join("ОченьДлинноеИмяФайла_样本.dat");
        std::fs::write(&long, b"x").unwrap();

        // unsafe：GetShortPathNameW 查询真实 8.3 短名（仅读取）；kernel32 默认链接。
        // 判定法：足量缓冲单次调用，返回串与原串相同 = 无短形态（原样返回）。
        unsafe extern "system" {
            fn GetShortPathNameW(
                lpsz_long_path: *const u16,
                lpsz_short_path: *mut u16,
                cch_buffer: u32,
            ) -> u32;
        }
        let original = long.to_string_lossy().into_owned();
        let wide: Vec<u16> = original.encode_utf16().chain(std::iter::once(0)).collect();
        let cap = wide.len() as u32 + 32;
        let mut buf = vec![0u16; cap as usize];
        let short: Option<PathBuf> = unsafe {
            let written = GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), cap);
            if written == 0 || written as usize >= buf.len() {
                None
            } else {
                buf.truncate(buf.iter().position(|&c| c == 0).unwrap_or(buf.len()));
                let s = String::from_utf16_lossy(&buf);
                if s == original {
                    None // 无短形态
                } else {
                    Some(PathBuf::from(s))
                }
            }
        };
        let Some(short) = short else {
            let _ = std::fs::remove_dir_all(&dir);
            return; // 该卷无 8.3：本用例不适用
        };
        // ① 声明侧：白名单根以短名声明（P4-04 T-4），展开后须命中长名候选。
        set_xml_roots(vec![short.clone()]);
        assert!(
            is_whitelisted(&long),
            "短名声明的白名单根必须展开为长名后命中"
        );
        // ② 镜像形态（P4-06 CI 实测补全）：候选本身带短名段（如 CI 的 %TEMP%）时
        //    候选侧也要展开——短名候选 vs 长名 root 必须命中。
        assert!(
            is_whitelisted(&short),
            "短名候选（展开后与长名 root 同指）必须命中"
        );
        set_xml_roots(vec![]);
        let _ = std::fs::remove_dir_all(&dir);
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
    fn volume_root_critical_files_whitelisted_without_xml() {
        // 卷根关键系统文件走常量判定（不依赖 whitelist.xml 加载）：
        // 2026-09-30 真机抽查发现 XML 未加载时 pagefile.sys 漏进 large 候选。
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_xml_roots(vec![]);
        for f in [
            "pagefile.sys",
            "hiberfil.sys",
            "swapfile.sys",
            "dumpstack.log",
        ] {
            assert!(is_whitelisted(Path::new(&format!(r"C:\{f}"))), "{f}");
            assert!(is_whitelisted(Path::new(&format!(r"D:\{f}"))), "D:\\{f}");
        }
        // 子目录同名用户文件不受影响。
        assert!(!is_whitelisted(Path::new(
            r"C:\Users\u\Documents\pagefile.sys"
        )));
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
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        // （样本避开卷根系统文件名——pagefile.sys 等 2026-09-30 起为常量拦截。）
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let target = Path::new(r"C:\Users\u\AppData\Local\Temp\keepme.dat");
        set_xml_roots(vec![]);
        assert!(!is_whitelisted(target), "未注入前不应被拦截");
        set_xml_roots(vec![PathBuf::from(
            r"C:\Users\u\AppData\Local\Temp\keepme.dat",
        )]);
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

    #[test]
    fn user_core_data_layers_for_quarantine_scan() {
        // H1（v0.1.4）三层语义验证：
        // ① §2.4 五目录段级判定（纯函数，fake home）；
        // ② is_whitelisted 不再含 §2.4（dup 维度 4/5 target 被自家白名单废掉的根因）；
        // ③ is_excluded_from_scan：direct/recycle 仍拦、quarantine 放行、系统根全拦。
        let home = Path::new(r"C:\Users\testuser");
        let roots = user_core_data_seg_roots(home);
        assert_eq!(roots.len(), 5);
        let doc = normalize_segments(Path::new(r"C:\Users\testuser\Documents\a\b.txt"));
        assert!(roots.iter().any(|r| prefix_matches(&doc, r)));
        let tmp = normalize_segments(Path::new(r"C:\Users\testuser\AppData\Local\Temp\x"));
        assert!(
            !roots.iter().any(|r| prefix_matches(&tmp, r)),
            "非 §2.4 目录不误判"
        );

        // ②③ 用真实 USERPROFILE 动态构造（is_user_core_data 读环境变量；
        // 不硬编码本机用户名）。
        let Ok(real_home) = std::env::var("USERPROFILE") else {
            set_xml_roots(vec![]);
            return;
        };
        let _g = XML_ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_xml_roots(vec![]);
        let doc_path = Path::new(&real_home)
            .join("Documents")
            .join("dups")
            .join("a.bin");
        assert!(
            !is_whitelisted(&doc_path),
            "§2.4 已从通用白名单拆出（H1 根因修复）"
        );
        assert!(is_user_core_data(&doc_path));
        assert!(
            is_excluded_from_scan(&doc_path, Disposition::Direct),
            "direct/recycle 去向仍整树拦（用户文档绝不进直清候选）"
        );
        assert!(
            !is_excluded_from_scan(&doc_path, Disposition::Quarantine),
            "quarantine 去向放行（隔离可还原 + 逐项确认）"
        );
        // 系统根对所有类目仍拦（§2.4 例外不外溢）。
        if let Ok(win) = std::env::var("SystemRoot") {
            assert!(is_excluded_from_scan(
                Path::new(&format!("{win}\\x.dll")),
                Disposition::Quarantine
            ));
        }
        set_xml_roots(vec![]);
    }
}
