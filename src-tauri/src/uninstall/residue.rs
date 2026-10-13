//! R27-3 卸载残留枚举（只读）与清理编排（SPEC §6.7 / SAFETY §2.2 R27-3 条款）。
//!
//! 定位：仅在**一次卸载刚完成**（后端留存 ResidueContext，Uninstall 键已消失）后，
//! 对三类高置信残留做枚举——①登记的安装目录；②开始菜单快捷方式/程序组；
//! ③AppData(Local/Roaming/LocalLow) 配置目录。产出文件级 `ScanItem`
//! （category=`residue.uninstall`，默认 🟡/quarantine），复用 preflight + journal +
//! 隔离区 + 审计；用户逐项确认，去向可选隔离区（默认）或直接删除（🔴 专家 token）。
//!
//! 不做无上下文的全局孤儿扫描（同名误报不可控）；名称匹配只接受精确/去版本后缀，
//! 不做模糊包含；系统共享名/通用目录名有拒绝名单；候选根安全由 `root_safety` 强制。

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::cleaner::execute::CleanTarget;
use crate::cleaner::RESIDUE_CATEGORY_ID;
use crate::contract::{Disposition, Grade, ScanItem};
use crate::safety::whitelist;

// ===== 上限（防 InstallLocation 被篡改成卷根/巨型树导致的候选爆炸）=====

/// 单个候选根内文件数上限（超出整组放弃，不产候选）。
pub const MAX_FILES_PER_CANDIDATE: usize = 20_000;
/// 单个候选根内总字节上限（10 GiB；超出整组放弃）。
pub const MAX_BYTES_PER_CANDIDATE: u64 = 10 * 1024 * 1024 * 1024;
/// 递归深度上限（相对候选根）。
pub const MAX_DEPTH: usize = 20;
/// 开始菜单程序组枚举深度（相对 Programs 根）。
const STARTMENU_DEPTH: usize = 2;

/// 卸载残留上下文（R27-2 完成 removed/reboot_pending 后由 AppState 留存）。
#[derive(Debug, Clone)]
pub struct ResidueContext {
    pub app_id: String,
    pub name: String,
    pub publisher: Option<String>,
    /// R27-2 事后读取的 InstallLocation（注册表原值，可能含环境变量/尾斜杠）。
    pub install_location: Option<String>,
    pub uninstalled_at: i64,
}

/// 残留来源类型（契约镜像，snake_case 序列化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidueKind {
    InstallDir,
    StartMenu,
    AppData,
}

impl ResidueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResidueKind::InstallDir => "install_dir",
            ResidueKind::StartMenu => "start_menu",
            ResidueKind::AppData => "app_data",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ResidueKind::InstallDir => "安装目录",
            ResidueKind::StartMenu => "开始菜单",
            ResidueKind::AppData => "AppData 配置",
        }
    }
}

/// 一个残留候选组（授权根 = root_path；items 为展开后的文件级条目）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidueCandidate {
    pub id: String,
    pub kind: ResidueKind,
    pub app_name: String,
    /// 授权根：目录（安装/AppData/程序组）或单个快捷方式文件。
    pub root_path: String,
    /// 判定理由（UI 必须原样明示，DoD）。
    pub reason: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub items: Vec<ScanItem>,
}

/// 枚举用环境根（测试注入假根，生产从环境变量取）。
#[derive(Debug, Clone)]
pub struct ResidueRoots {
    pub program_data: PathBuf,
    pub appdata_roaming: PathBuf,
    pub local_appdata: PathBuf,
    pub user_profile: PathBuf,
}

impl ResidueRoots {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            program_data: std::env::var_os("ProgramData").map(PathBuf::from)?,
            appdata_roaming: std::env::var_os("APPDATA").map(PathBuf::from)?,
            local_appdata: std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?,
            user_profile: std::env::var_os("USERPROFILE").map(PathBuf::from)?,
        })
    }

    /// LocalLow（无独立环境变量，按固定相对路径推）。
    pub fn local_low(&self) -> PathBuf {
        self.user_profile.join("AppData").join("LocalLow")
    }

    /// 开始菜单两 Programs 根（全体 + 当前用户）。
    pub fn start_menu_roots(&self) -> Vec<PathBuf> {
        let common = self
            .program_data
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        let user = self
            .appdata_roaming
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        vec![common, user]
    }

    /// AppData 三个根及其展示名。
    pub fn appdata_roots(&self) -> Vec<(PathBuf, &'static str)> {
        vec![
            (self.local_appdata.clone(), "Local"),
            (self.appdata_roaming.clone(), "Roaming"),
            (self.local_low(), "LocalLow"),
        ]
    }
}

// ===== 名称归一与匹配（纯函数，误报防护核心）=====

/// 产品名归一：小写、去快捷方式/可执行扩展、去尾部架构括号与版本号、折叠空白。
/// 用于"产品名 ↔ 目录名/lnk 基名"精确比对。
pub fn normalize_product_name(s: &str) -> String {
    let mut t = s.trim().to_lowercase();
    for ext in [".lnk", ".url", ".exe"] {
        if t.ends_with(ext) {
            t.truncate(t.len() - ext.len());
            t = t.trim_end().to_string();
        }
    }
    // 交替剥尾部后缀：架构/版本括号 → 裸版本段，直到两者都剥不动为止
    //（顺序无关："App (64-bit) 2.0" 与 "App 2.0 (x64)" 都收敛）。
    loop {
        let trimmed = t.trim_end();
        if trimmed.ends_with(')') {
            if let Some(open) = trimmed.rfind('(') {
                let inside = trimmed[open + 1..trimmed.len() - 1].trim().to_lowercase();
                let is_arch = matches!(
                    inside.as_str(),
                    "x64"
                        | "x86"
                        | "32"
                        | "64"
                        | "32-bit"
                        | "64-bit"
                        | "32bit"
                        | "64bit"
                        | "x86_64"
                        | "amd64"
                        | "ia64"
                        | "arm64"
                        | "win32"
                        | "win64"
                );
                if is_arch || is_versionish(&inside) {
                    t = trimmed[..open].trim_end().to_string();
                    continue;
                }
            }
        }
        let cut = trailing_version(trimmed);
        if cut != trimmed.len() {
            t = trimmed[..cut].trim_end().to_string();
            continue;
        }
        break;
    }
    collapse_spaces(&t)
}

/// 发布者归一：在产品归一基础上额外去尾部法人/商业后缀（仅限固定小名单）。
pub fn normalize_publisher(s: &str) -> String {
    let mut t = normalize_product_name(s);
    const SUFFIXES: &[&str] = &[
        "co.,ltd.",
        "co., ltd.",
        "co.,ltd",
        "co., ltd",
        "corporation",
        "incorporated",
        "technology",
        "technologies",
        "publishing",
        "limited",
        "company",
        "holdings",
        "software",
        "studios",
        "studio",
        "s.a.s.",
        "s.r.l.",
        "gmbh",
        "corp.",
        "corp",
        "ltd.",
        "ltd",
        "llc",
        "inc.",
        "inc",
        "pty",
        "co.",
        "s.a",
        "sa",
        "co",
    ];
    loop {
        let trimmed = t.trim_end();
        let changed = SUFFIXES.iter().find_map(|suf| {
            trimmed
                .ends_with(suf)
                .then(|| trimmed[..trimmed.len() - suf.len()].trim_end().to_string())
        });
        match changed {
            Some(next) if next != trimmed && !next.is_empty() => t = next,
            _ => break,
        }
    }
    collapse_spaces(&t)
}

/// 段是否"版本样"：全由数字/点/连字符/v 组成且至少含一个数字。
fn is_versionish(s: &str) -> bool {
    let t = s
        .strip_prefix('v')
        .unwrap_or(s)
        .strip_prefix('V')
        .unwrap_or(s);
    let t = t.trim();
    !t.is_empty()
        && t.chars().any(|c| c.is_ascii_digit())
        && t.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '_' | ' ' | 'v' | 'V'))
}

/// 返回截掉尾部版本段后的字节位置（无可截则返回原长度）。
fn trailing_version(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    // 从末尾吞版本字符。
    while i > 0 && matches!(bytes[i - 1] as char, '0'..='9' | '.' | '-' | '_') {
        i -= 1;
    }
    // 可选前导 v/V。
    if i > 0 && (bytes[i - 1] as char == 'v' || bytes[i - 1] as char == 'V') {
        i -= 1;
    }
    // 版本段前必须是空白边界（"7zip24" 不截），且截下的内容含数字。
    if i < bytes.len() && i > 0 && (bytes[i - 1] as char).is_whitespace() {
        let cut = s[i..].chars().any(|c| c.is_ascii_digit());
        if cut {
            return i - 1;
        }
    }
    bytes.len()
}

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// 名称是否与产品名精确匹配（归一后相等）。
pub fn name_matches_product(holder: &str, product_norm: &str) -> bool {
    let h = normalize_product_name(holder);
    !h.is_empty() && h == product_norm
}

// ===== 候选根安全（两层闸）=====
//
// 注册表 InstallLocation 是**外部值**（软件可写任意路径），走严格的
// `install_root_safety`；开始菜单/AppData 的候选根由本枚举器从系统已知位置
// 自行拼出，只过防御性的 `asserted_root_safety`（硬保护 + reparse + 末段通用名）。

/// 任意位置出现即拒的绝对段名（小写精确；系统关键/卷信息目录）。
const ABSOLUTE_DENY_SEGMENTS: &[&str] = &[
    "windows",
    "$recycle.bin",
    "system volume information",
    "boot",
    "efi",
    "recovery",
    "perflogs",
    "system32",
    "syswow64",
    "windowsapps",
    "driverstore",
    "servicing",
    "assembly",
];

/// 候选根**末段**拒绝名单（共享/通用目录名，名称匹配不足以授权整目录删除）。
const DENY_FINAL_SEGMENTS: &[&str] = &[
    "common files",
    "internet explorer",
    "windows defender",
    "windows nt",
    "windows kits",
    "windows mail",
    "windows media player",
    "windows photo viewer",
    "windows sidebar",
    "microsoft",
    "programs",
    "start menu",
    "startup",
    "fonts",
    "tasks",
    "common",
    "shared",
    "resources",
    "runtime",
    "runtimes",
    "drivers",
    "installer",
    "temp",
    "tmp",
    "cache",
    "caches",
    "update",
    "updater",
    "logs",
    "data",
    "bin",
    "lib",
    "libs",
    "etc",
    "include",
    "share",
];

/// 通用产品名拒绝名单（产品名归一后命中 → 不做 AppData/开始菜单匹配；
/// 安装目录以注册表登记值为准不受此限）。
const DENY_PRODUCT_NAMES: &[&str] = &[
    "update",
    "updater",
    "setup",
    "installer",
    "install",
    "uninstall",
    "temp",
    "tmp",
    "cache",
    "caches",
    "logs",
    "log",
    "data",
    "config",
    "configs",
    "settings",
    "plugins",
    "plugin",
    "runtime",
    "runtimes",
    "common",
    "shared",
    "resources",
    "bin",
    "lib",
    "libs",
    "drivers",
    "server",
    "client",
    "tool",
    "tools",
    "helper",
    "service",
    "services",
    "node_modules",
    "crashpad",
    "crashdumps",
];

/// 注册表 InstallLocation 候选根的严格安全闸。
pub fn install_root_safety(root: &Path, roots: &ResidueRoots) -> Result<(), String> {
    let s = root.to_string_lossy();
    if s.trim().is_empty() || !root.is_absolute() {
        return Err("安装位置不是绝对路径".into());
    }
    if s.len() > 260 {
        return Err("路径过长（>260），保守跳过".into());
    }
    // 硬保护（SystemRoot/引导/卷信息/隔离区/XML 声明根）：任何授权不可开口。
    if whitelist::is_hard_protected(root) {
        return Err("位于系统硬保护目录内".into());
    }
    // 程序目录软根（PF*/ProgramData\Microsoft）本身不可作为目标。
    if whitelist::is_soft_program_root(root) {
        return Err("目标等于程序目录白名单根本身".into());
    }
    // reparse：根自身或祖先为 junction/symlink → 拒绝。
    if let Some(seg) = crate::cleaner::preflight::first_reparse_component(root) {
        return Err(format!(
            "路径含 junction/symlink，已拒绝（{}）",
            seg.to_string_lossy()
        ));
    }

    let segs = whitelist::normalize_segments(root);
    if segs.len() < 2 {
        return Err("目标过于接近卷根".into());
    }
    for seg in &segs[1..] {
        if ABSOLUTE_DENY_SEGMENTS.contains(&seg.as_str()) {
            return Err(format!("路径含系统关键目录段「{seg}」，保守拒绝"));
        }
    }
    let last = segs.last().map(String::as_str).unwrap_or("");
    if DENY_FINAL_SEGMENTS.contains(&last) {
        return Err(format!("目标末段「{last}」为系统共享/通用目录名，保守拒绝"));
    }

    // Users 树：只允许到 Users\<who>\<x>\<y> 以下（≥4 段），防指到用户主目录
    // 或其直接通用目录；AppData 已知结构（6 段）天然满足。
    let profile_segs = whitelist::normalize_segments(&roots.user_profile);
    let under_profile =
        segs.iter().zip(&profile_segs).all(|(a, b)| a == b) && segs.len() >= profile_segs.len();
    if under_profile && segs.len() < profile_segs.len() + 2 {
        return Err("目标仅位于用户主目录一/两层之下，保守拒绝".into());
    }
    // 任意 Users\<who> 直接子目录（含 Public）：要求 ≥4 段。
    if segs.len() >= 2 && segs[1] == "users" && segs.len() < 4 {
        return Err("用户目录层级过浅，保守拒绝".into());
    }
    // ProgramData 直接子目录（非 Microsoft 软根）：要求 ≥4 段（共享缓存多在浅层）。
    if segs.len() >= 2 && segs[1] == "programdata" && segs.len() < 4 {
        return Err("ProgramData 浅层目录为共享数据常见位置，保守拒绝".into());
    }
    Ok(())
}

/// 枚举器自构候选根（开始菜单程序组/AppData 产品目录）的防御性安全闸。
pub fn asserted_root_safety(root: &Path) -> Result<(), String> {
    if whitelist::is_hard_protected(root) {
        return Err("位于系统硬保护目录内".into());
    }
    if let Some(seg) = crate::cleaner::preflight::first_reparse_component(root) {
        return Err(format!(
            "路径含 junction/symlink，已拒绝（{}）",
            seg.to_string_lossy()
        ));
    }
    let segs = whitelist::normalize_segments(root);
    let last = segs.last().map(String::as_str).unwrap_or("");
    if DENY_FINAL_SEGMENTS.contains(&last) || ABSOLUTE_DENY_SEGMENTS.contains(&last) {
        return Err(format!("目标末段「{last}」为系统共享/通用目录名，保守拒绝"));
    }
    Ok(())
}

/// 产品名是否可用于 AppData/开始菜单匹配（通用名/过短名拒绝）。
pub fn product_name_usable(product_norm: &str) -> bool {
    product_norm.chars().count() >= 2 && !DENY_PRODUCT_NAMES.contains(&product_norm)
}

// ===== 文件收集（只读递归）=====

struct Collected {
    files: Vec<PathBuf>,
    total_bytes: u64,
    /// 遍历时跳过的 reparse 点数（0 才产候选——含链接子树不可整组信任）。
    reparse_skipped: u64,
    /// 遍历中遇到错误的条目数（权限等）；>0 仍可出候选（文件级失败本就逐文件记录），
    /// 但理由中不含失败明细，v1 保守：有错误也允许，删除阶段逐文件失败可见。
    #[allow(dead_code)]
    errors: u64,
}

/// 递归收集根内普通文件（不跟随 reparse；超上限报错）。
fn collect_files(root: &Path, max_depth: usize) -> Result<Collected, String> {
    let mut out = Collected {
        files: Vec::new(),
        total_bytes: 0,
        reparse_skipped: 0,
        errors: 0,
    };
    for entry in walkdir::WalkDir::new(root)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        let ft = entry.file_type();
        if ft.is_symlink() {
            out.reparse_skipped += 1;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        if out.files.len() >= MAX_FILES_PER_CANDIDATE {
            return Err(format!(
                "文件数超过上限 {MAX_FILES_PER_CANDIDATE}，整组跳过"
            ));
        }
        if out.total_bytes.saturating_add(meta.len()) > MAX_BYTES_PER_CANDIDATE {
            return Err("总大小超过 10 GiB 上限，整组跳过".into());
        }
        out.total_bytes += meta.len();
        out.files.push(entry.into_path());
    }
    Ok(out)
}

/// 由 category + path 生成稳定 ScanItem id（与 scanner/walk 同款 DefaultHasher 16hex）。
fn item_id(path: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    RESIDUE_CATEGORY_ID.hash(&mut h);
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn epoch_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// 把根内文件列表构造为 ScanItem（默认 🟡/quarantine；direct 由执行层按用户选择覆盖）。
fn build_items(files: Vec<PathBuf>, reason: &str) -> Vec<ScanItem> {
    files
        .into_iter()
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let path_str = p.to_string_lossy().into_owned();
            Some(ScanItem {
                id: item_id(&path_str),
                category_id: RESIDUE_CATEGORY_ID.into(),
                label: p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path_str.clone()),
                path: path_str,
                size_bytes: meta.len(),
                grade: Grade::Yellow,
                disposition: Disposition::Quarantine,
                reason: reason.into(),
                mtime: epoch_ms(meta.modified().ok()?),
                atime: epoch_ms(meta.accessed().ok()?),
                dup_group: None,
            })
        })
        .collect()
}

// ===== 三源枚举 =====

/// ① 安装目录残留：注册表 InstallLocation 仍存在且通过根安全闸。
pub fn enum_install_dir(ctx: &ResidueContext, roots: &ResidueRoots) -> Option<ResidueCandidate> {
    let raw = ctx.install_location.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = crate::uninstall::runner::expand_env(raw);
    let root = PathBuf::from(expanded.trim_end_matches(['\\', '/']));
    let meta = std::fs::symlink_metadata(&root).ok()?;
    if !meta.is_dir() {
        return None;
    }
    if let Err(reason) = install_root_safety(&root, roots) {
        eprintln!("[residue] 安装目录候选拒绝（{}）: {reason}", root.display());
        return None;
    }
    let collected = match collect_files(&root, MAX_DEPTH) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[residue] 安装目录枚举放弃（{}）: {e}", root.display());
            return None;
        }
    };
    if collected.reparse_skipped > 0 {
        eprintln!(
            "[residue] 安装目录含 junction/symlink 子树，整组不列为候选: {}",
            root.display()
        );
        return None;
    }
    let count = collected.files.len() as u64;
    let size = collected.total_bytes;
    let reason = format!(
        "官方卸载器已运行且卸载条目已消失，但登记的安装目录仍存在（{count} 个文件{}）",
        if count == 0 { "，目录为空" } else { "" }
    );
    let root_str = root.to_string_lossy().into_owned();
    Some(ResidueCandidate {
        id: item_id(&format!("install|{root_str}")),
        kind: ResidueKind::InstallDir,
        app_name: ctx.name.clone(),
        reason,
        items: build_items(collected.files, "卸载后安装目录残留"),
        size_bytes: size,
        file_count: count,
        root_path: root_str,
    })
}

/// ② 开始菜单：两 Programs 根下按目录/快捷方式名精确匹配产品名。
pub fn enum_start_menu(ctx: &ResidueContext, roots: &ResidueRoots) -> Vec<ResidueCandidate> {
    let product = normalize_product_name(&ctx.name);
    if !product_name_usable(&product) {
        return Vec::new();
    }
    let publisher = ctx.publisher.as_deref().map(normalize_publisher);
    let mut out: Vec<ResidueCandidate> = Vec::new();

    for programs in roots.start_menu_roots() {
        if !programs.is_dir() {
            continue;
        }
        // 程序组目录：深度 1（厂商/产品目录）~2（厂商\产品）。
        let group_dirs = match collect_group_dirs(&programs, &product, publisher.as_deref()) {
            Some(dirs) => dirs,
            None => continue,
        };
        for dir in group_dirs {
            if let Some(c) = startmenu_candidate(&ctx.name, &programs, &dir, &product) {
                out.push(c);
            }
        }
        // 散落快捷方式：Programs 根两层内的 .lnk/.url 按所在目录聚合。
        let loose = collect_loose_links(&programs, &product);
        for (dir, files) in loose {
            // 与已收程序组同目录则并入（去重）。
            if out.iter().any(|c| Path::new(&c.root_path) == dir.as_path()) {
                continue;
            }
            let reason = format!(
                "开始菜单目录「{}」中仍存在与「{}」名称一致的快捷方式",
                programs.display(),
                ctx.name
            );
            let root_str = dir.to_string_lossy().into_owned();
            let items = build_items(files, "卸载后开始菜单残留");
            out.push(ResidueCandidate {
                id: item_id(&format!("startmenu|{root_str}")),
                kind: ResidueKind::StartMenu,
                app_name: ctx.name.clone(),
                reason,
                size_bytes: items.iter().map(|i| i.size_bytes).sum(),
                file_count: items.len() as u64,
                root_path: root_str,
                items,
            });
        }
    }
    dedupe_candidates(out)
}

/// 找程序组目录：名≈产品名（1 层）；或名≈厂商且其下有名≈产品名的子目录（2 层）。
fn collect_group_dirs(
    programs: &Path,
    product: &str,
    publisher_norm: Option<&str>,
) -> Option<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(programs).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name_matches_product(&name, product) {
            dirs.push(path);
            continue;
        }
        // 厂商程序组：目录名≈发布者，且内部一层存在产品名子目录或产品名 lnk。
        if let Some(pub_name) = publisher_norm {
            if !pub_name.is_empty() && normalize_product_name(&name) == pub_name {
                if let Ok(children) = std::fs::read_dir(&path) {
                    for child in children.flatten() {
                        let cp = child.path();
                        let cname = child.file_name().to_string_lossy().into_owned();
                        let dir_match = cp.is_dir() && name_matches_product(&cname, product);
                        let link_match = cp.is_file()
                            && matches_link_ext(&cname)
                            && name_matches_product(&cname, product);
                        if dir_match || link_match {
                            dirs.push(path.clone());
                            break;
                        }
                    }
                }
            }
        }
    }
    Some(dirs)
}

/// 程序组候选：收集组内 .lnk/.url（两层内）；组内必须至少有一个名称匹配的快捷方式。
fn startmenu_candidate(
    app_name: &str,
    programs: &Path,
    group: &Path,
    product: &str,
) -> Option<ResidueCandidate> {
    let mut links: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(group)
        .max_depth(STARTMENU_DEPTH)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches_link_ext(&name) {
            links.push(entry.into_path());
        }
    }
    // 高置信闸：组内必须确有产品名快捷方式（防止厂商目录多产品误收整个厂商组）。
    if !links.iter().any(|p| {
        p.file_name()
            .is_some_and(|n| name_matches_product(&n.to_string_lossy(), product))
    }) {
        return None;
    }
    let root_str = group.to_string_lossy().into_owned();
    let reason = format!(
        "开始菜单目录「{}」中仍存在「{}」的程序组/快捷方式",
        programs.display(),
        app_name
    );
    let items = build_items(links, "卸载后开始菜单残留");
    Some(ResidueCandidate {
        id: item_id(&format!("startmenu|{root_str}")),
        kind: ResidueKind::StartMenu,
        app_name: app_name.to_string(),
        reason,
        size_bytes: items.iter().map(|i| i.size_bytes).sum(),
        file_count: items.len() as u64,
        root_path: root_str,
        items,
    })
}

/// 收集 Programs 两层内基名匹配产品名、但所在目录不是已识别程序组的散落快捷方式。
fn collect_loose_links(programs: &Path, product: &str) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut map: std::collections::BTreeMap<String, (PathBuf, Vec<PathBuf>)> =
        std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(programs)
        .max_depth(STARTMENU_DEPTH)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches_link_ext(&name) && name_matches_product(&name, product) {
            let dir = entry
                .path()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(programs.to_path_buf());
            let key = dir.to_string_lossy().into_owned();
            map.entry(key)
                .or_insert_with(|| (dir.clone(), Vec::new()))
                .1
                .push(entry.into_path());
        }
    }
    map.into_values().collect()
}

fn matches_link_ext(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".lnk") || l.ends_with(".url")
}

/// ③ AppData：Local/Roaming/LocalLow 下精确产品名目录；厂商\产品 两层结构。
pub fn enum_appdata(ctx: &ResidueContext, roots: &ResidueRoots) -> Vec<ResidueCandidate> {
    let product = normalize_product_name(&ctx.name);
    if !product_name_usable(&product) {
        return Vec::new();
    }
    let publisher = ctx.publisher.as_deref().map(normalize_publisher);
    let mut out = Vec::new();

    for (base, label) in roots.appdata_roots() {
        if !base.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            // a) 直接产品目录。
            if name_matches_product(&name, &product) {
                if let Some(c) = appdata_candidate(&ctx.name, &path, label, &product) {
                    out.push(c);
                }
                continue;
            }
            // b) 厂商目录：只收其下**名称匹配产品名**的子目录（绝不整收厂商目录——
            //    多产品共享配置是常态，如厂商目录下并存多款软件）。
            if let Some(pub_name) = publisher.as_deref() {
                if !pub_name.is_empty()
                    && normalize_product_name(&name) == pub_name
                    && product_name_usable(pub_name)
                {
                    if let Ok(children) = std::fs::read_dir(&path) {
                        for child in children.flatten() {
                            let cp = child.path();
                            let cname = child.file_name().to_string_lossy().into_owned();
                            if cp.is_dir() && name_matches_product(&cname, &product) {
                                if let Some(c) = appdata_candidate(&ctx.name, &cp, label, &product)
                                {
                                    out.push(c);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    dedupe_candidates(out)
}

fn appdata_candidate(
    app_name: &str,
    dir: &Path,
    base_label: &str,
    product: &str,
) -> Option<ResidueCandidate> {
    if let Err(r) = asserted_root_safety(dir) {
        eprintln!("[residue] AppData 候选拒绝（{}）: {r}", dir.display());
        return None;
    }
    let collected = collect_files(dir, MAX_DEPTH).ok()?;
    if collected.reparse_skipped > 0 {
        return None;
    }
    // 高置信：目录名必须精确匹配（root_safety 已过通用名名单，此处双保险）。
    let dir_name = dir.file_name()?.to_string_lossy().into_owned();
    if !name_matches_product(&dir_name, product) {
        return None;
    }
    let count = collected.files.len() as u64;
    let reason = format!(
        "AppData\\{base_label} 下仍存在与「{app_name}」名称一致的配置目录（{count} 个文件）"
    );
    let root_str = dir.to_string_lossy().into_owned();
    let items = build_items(collected.files, "卸载后 AppData 配置残留");
    Some(ResidueCandidate {
        id: item_id(&format!("appdata|{root_str}")),
        kind: ResidueKind::AppData,
        app_name: app_name.to_string(),
        reason,
        size_bytes: items.iter().map(|i| i.size_bytes).sum(),
        file_count: count,
        root_path: root_str,
        items,
    })
}

fn dedupe_candidates(v: Vec<ResidueCandidate>) -> Vec<ResidueCandidate> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter()
        .filter(|c| seen.insert(c.root_path.to_lowercase()))
        .collect()
}

/// 三源合扫（生产入口）。无环境根返回错误。
pub fn scan_residue(ctx: &ResidueContext) -> Result<Vec<ResidueCandidate>, String> {
    let roots = ResidueRoots::from_env().ok_or("无法解析系统目录环境变量")?;
    let mut out = Vec::new();
    if let Some(c) = enum_install_dir(ctx, &roots) {
        out.push(c);
    }
    out.extend(enum_start_menu(ctx, &roots));
    out.extend(enum_appdata(ctx, &roots));
    Ok(dedupe_candidates(out))
}

// ===== 清理编排（复用 cleaner::execute 的 journal/审计/隔离区/取消）=====

/// 一次残留扫描快照（state 留存；clean 只接受快照内候选根，前端无法伪造路径）。
#[derive(Debug, Clone)]
pub struct ResidueSnapshot {
    pub context: ResidueContext,
    pub candidates: Vec<ResidueCandidate>,
    pub scanned_at: i64,
}

/// 用户对一个候选组的去向选择（R27-3：隔离区默认 / direct 直删需专家 token）。
#[derive(Debug, Clone)]
pub struct ResidueSelection {
    pub candidate_id: String,
    pub disposition: Disposition,
}

/// prepare_targets 的产物。
pub struct PreparedClean {
    /// 文件级目标（已带 authorized_root；direct/quarantine 按用户选择）。
    pub targets: Vec<CleanTarget>,
    /// 进程守卫整组跳过的候选（candidate_id, 原因）。
    pub skipped: Vec<(String, String)>,
    /// 被守卫跳过的文件项总数（并入事务报告 skip）。
    pub skipped_items: u64,
    /// 选中的候选根（事后空目录收尾用，已去重）。
    pub roots: Vec<PathBuf>,
}

/// 把快照 + 用户选择解析为清理目标。
/// - 候选 id 必须存在于后端快照（防前端伪造路径/扩大授权根）；
/// - 去向仅接受 quarantine/direct（残留清理不提供 recycle 档）；
/// - 组内含运行中 .exe → 整组跳过（沿用 guard"请先退出"语义，不做半清）。
pub fn prepare_targets(
    snapshot: &ResidueSnapshot,
    selections: &[ResidueSelection],
) -> Result<PreparedClean, String> {
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    let mut skipped_items = 0u64;
    let mut roots = Vec::new();
    let mut chosen = std::collections::HashSet::new();

    for sel in selections {
        if !chosen.insert(sel.candidate_id.to_lowercase()) {
            continue; // 重复选择幂等忽略
        }
        let Some(cand) = snapshot
            .candidates
            .iter()
            .find(|c| c.id == sel.candidate_id)
        else {
            return Err(format!(
                "候选 {} 不在最近扫描结果中（拒绝执行）",
                sel.candidate_id
            ));
        };
        if !matches!(
            sel.disposition,
            Disposition::Quarantine | Disposition::Direct
        ) {
            return Err("残留清理仅支持隔离区或直接删除两种去向".into());
        }

        // 进程守卫：组内登记的 .exe 任一在运行 → 整组跳过。
        let exes: Vec<String> = cand
            .items
            .iter()
            .filter_map(|i| {
                let p = Path::new(&i.path);
                let name = p.file_name()?.to_string_lossy().into_owned();
                name.to_lowercase().ends_with(".exe").then_some(name)
            })
            .collect();
        if !exes.is_empty() && crate::guard::any_running(&exes) {
            skipped_items += cand.items.len() as u64;
            skipped.push((
                cand.id.clone(),
                "残留目录中的程序仍在运行，请先退出后再清理".into(),
            ));
            continue;
        }

        let root = PathBuf::from(&cand.root_path);
        for item in &cand.items {
            targets.push(CleanTarget {
                path: PathBuf::from(&item.path),
                grade: Grade::Yellow,
                disposition: sel.disposition,
                category_id: RESIDUE_CATEGORY_ID.into(),
                size_bytes: item.size_bytes,
                guard_processes: Vec::new(), // 守卫已在组级统一处理
                mtime_ms: item.mtime,
                authorized_root: Some(root.clone()),
            });
        }
        roots.push(root);
    }
    Ok(PreparedClean {
        targets,
        skipped,
        skipped_items,
        roots,
    })
}

/// 候选根空目录收尾：文件移走/删除后，自底向上 best-effort 移除变空的目录
/// （含候选根本身；空目录候选由此删除）。`RemoveDirectoryW` 对非空目录必然失败，
/// 不递归删内容——绝不构成第二条删除路径。reparse 点跳过。
pub fn prune_empty_dirs(roots: &[PathBuf]) {
    for root in roots {
        // 先收子孙目录（contents_first = 子在前），再处理根本身。
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in walkdir::WalkDir::new(root)
            .max_depth(MAX_DEPTH)
            .follow_links(false)
            .contents_first(true)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_dir() && !entry.file_type().is_symlink() {
                dirs.push(entry.into_path());
            }
        }
        for d in &dirs {
            remove_empty_dir(d);
        }
        remove_empty_dir(root);
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn RemoveDirectoryW(path: *const u16) -> i32;
}

fn remove_empty_dir(p: &Path) {
    let wide: Vec<u16> = p
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // 安全性：RemoveDirectoryW 仅在目录为空时成功；非空/有句柄占用静默失败
    //（best-effort 收尾，失败原因对用户无行动意义）。
    unsafe {
        RemoveDirectoryW(wide.as_ptr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_roots(tag: &str) -> (PathBuf, ResidueRoots) {
        let base = std::env::temp_dir().join(format!(
            "pureslate-residue-{tag}-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        let pd = base.join("ProgramData");
        let up = base.join("Users").join("tester");
        let roaming = up.join("AppData").join("Roaming");
        let local = up.join("AppData").join("Local");
        for p in [&pd, &roaming, &local] {
            std::fs::create_dir_all(p).unwrap();
        }
        let roots = ResidueRoots {
            program_data: pd,
            appdata_roaming: roaming.clone(),
            local_appdata: local.clone(),
            user_profile: up.clone(),
        };
        (base, roots)
    }

    // ---------- 名称归一 ----------

    #[test]
    fn product_name_normalization() {
        assert_eq!(normalize_product_name("7-Zip 24.08 (x64)"), "7-zip");
        assert_eq!(
            normalize_product_name("Google Chrome 120.0.6099.71"),
            "google chrome"
        );
        assert_eq!(normalize_product_name("Python 3.12"), "python");
        assert_eq!(normalize_product_name("My App v1.2.3"), "my app");
        assert_eq!(normalize_product_name("CoolTool (64-bit) 2.0"), "cooltool");
        assert_eq!(normalize_product_name("  空格  程序 "), "空格 程序");
        // 无版本尾巴不变。
        assert_eq!(normalize_product_name("WeChat"), "wechat");
        // 不含空格的数字尾巴不截（防误伤）。
        assert_eq!(normalize_product_name("7zip24"), "7zip24");
    }

    #[test]
    fn publisher_strips_legal_suffixes() {
        assert_eq!(normalize_publisher("Google LLC"), "google");
        assert_eq!(normalize_publisher("Mozilla Corporation"), "mozilla");
        assert_eq!(normalize_publisher("Acme Software, Inc."), "acme software,");
        // 不在名单内不动。
        assert_eq!(normalize_publisher("某软件公司"), "某软件公司");
    }

    #[test]
    fn exact_name_matching_no_fuzzy() {
        let p = normalize_product_name("7-Zip 24.08");
        assert!(name_matches_product("7-Zip 24.08.lnk", &p));
        assert!(name_matches_product("7-zip", &p));
        // 模糊包含一律不认。
        assert!(!name_matches_product("7-Zip Context Menu", &p));
        assert!(!name_matches_product("Uninstall 7-Zip", &p));
    }

    #[test]
    fn generic_product_names_rejected() {
        for n in ["Update", "Setup", "cache", "Tool", "CrashDumps"] {
            assert!(!product_name_usable(&normalize_product_name(n)), "{n}");
        }
        assert!(!product_name_usable("a"));
        assert!(product_name_usable("7-zip"));
    }

    // ---------- 候选根安全 ----------

    #[test]
    fn install_root_safety_allows_product_dirs() {
        let (base, roots) = tmp_roots("safety1");
        // 其他卷根下一层产品目录（模拟 D:\Game）。
        let game = base.join("Game");
        std::fs::create_dir_all(&game).unwrap();
        assert!(install_root_safety(&game, &roots).is_ok());
        // Users\tester\AppData\Local\Programs\App 深位安装。
        let deep = roots.local_appdata.join("Programs").join("SomeApp");
        std::fs::create_dir_all(&deep).unwrap();
        assert!(install_root_safety(&deep, &roots).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn install_root_safety_rejects_dangerous_locations() {
        let (base, roots) = tmp_roots("safety2");
        // 伪造 Windows 目录 → 绝对段拒绝。
        let fake_windows = base.join("Windows");
        std::fs::create_dir_all(&fake_windows).unwrap();
        assert!(install_root_safety(&fake_windows, &roots).is_err());
        // Common Files 共享目录末段拒绝。
        let cf = base.join("PF").join("Common Files");
        std::fs::create_dir_all(&cf).unwrap();
        assert!(install_root_safety(&cf, &roots).is_err());
        // 用户主目录一层之下拒。
        let under_home = roots.user_profile.join("Stuff");
        std::fs::create_dir_all(&under_home).unwrap();
        assert!(install_root_safety(&under_home, &roots).is_err());
        // 相对路径拒。
        assert!(install_root_safety(Path::new("rel\\path"), &roots).is_err());
        // ProgramData 深层产品目录（假根中末段/绝对段均不触名单）允许。
        let pd_deep = roots.program_data.join("Vendor").join("Product");
        std::fs::create_dir_all(&pd_deep).unwrap();
        assert!(install_root_safety(&pd_deep, &roots).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    // ---------- 三源枚举（沙箱假根）----------

    fn ctx(name: &str, publisher: Option<&str>, install: Option<&str>) -> ResidueContext {
        ResidueContext {
            app_id: "x".into(),
            name: name.into(),
            publisher: publisher.map(str::to_string),
            install_location: install.map(str::to_string),
            uninstalled_at: 0,
        }
    }

    fn write_file(p: &Path, content: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn enum_install_dir_lists_files_with_limits() {
        let (base, roots) = tmp_roots("install");
        let loc = base.join("AppRoot").join("SevenZip");
        write_file(&loc.join("7z.exe"), "x");
        write_file(&loc.join("lang").join("zh.txt"), "y");
        let c = {
            let c = ctx("7-Zip 24.08", None, Some(loc.to_str().unwrap()));
            enum_install_dir(&c, &roots)
        }
        .expect("应产出安装目录候选");
        assert_eq!(c.kind, ResidueKind::InstallDir);
        assert_eq!(c.file_count, 2);
        assert!(c.items.iter().all(|i| i.category_id == RESIDUE_CATEGORY_ID));
        assert!(c.reason.contains("安装目录"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enum_install_dir_empty_dir_still_candidate() {
        let (base, roots) = tmp_roots("emptydir");
        let loc = base.join("EmptyApp");
        std::fs::create_dir_all(&loc).unwrap();
        let c = enum_install_dir(&ctx("EmptyApp", None, Some(loc.to_str().unwrap())), &roots)
            .expect("空目录也应成候选（清理时仅删空目录）");
        assert_eq!(c.file_count, 0);
        assert!(c.items.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enum_install_dir_rejects_missing_and_denylisted() {
        let (base, roots) = tmp_roots("installrej");
        // 不存在。
        assert!(enum_install_dir(
            &ctx("X", None, Some(base.join("nope").to_str().unwrap())),
            &roots
        )
        .is_none());
        // 指向伪造 Windows 目录 → 拒绝。
        let fake = base.join("Windows");
        std::fs::create_dir_all(&fake).unwrap();
        write_file(&fake.join("a.dll"), "x");
        assert!(enum_install_dir(&ctx("X", None, Some(fake.to_str().unwrap())), &roots).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enum_start_menu_matches_groups_and_loose_links() {
        let (base, roots) = tmp_roots("sm");
        let programs = roots.start_menu_roots()[0].clone(); // common
                                                            // 产品程序组（名称带版本，目录名精确产品名）。
        write_file(&programs.join("7-Zip").join("7-Zip 24.08.lnk"), "x");
        write_file(&programs.join("7-Zip").join("Help.lnk"), "x");
        // 散落快捷方式在根上。
        write_file(&programs.join("Standalone 3.0.lnk"), "x");
        // 厂商\产品 两层结构（Google\Google Chrome.lnk）。
        write_file(&programs.join("Google").join("Google Chrome 120.lnk"), "x");
        // 干扰：其他软件同名架构不同——"7-Zip Context Menu" 不应产生新组，
        // 也不应让无关程序组被收。
        write_file(&programs.join("Other App").join("Other.lnk"), "x");

        let c7 = ctx("7-Zip 24.08", Some("Igor Pavlov"), None);
        let groups = enum_start_menu(&c7, &roots);
        // 仅一个 7-Zip 程序组（Help.lnk 随组，上下文菜单不单独成组）。
        let z = groups
            .iter()
            .find(|g| g.root_path.to_lowercase().ends_with("7-zip"))
            .expect("应有 7-Zip 程序组");
        assert_eq!(z.file_count, 2);

        let c_standalone = ctx("Standalone 3.0", None, None);
        let loose = enum_start_menu(&c_standalone, &roots);
        assert!(loose.iter().any(|g| g.file_count == 1
            && g.items
                .iter()
                .any(|i| i.path.to_lowercase().ends_with("standalone 3.0.lnk"))));

        let c_chrome = ctx("Google Chrome 120", Some("Google LLC"), None);
        let chrome = enum_start_menu(&c_chrome, &roots);
        assert!(
            chrome
                .iter()
                .any(|g| g.root_path.to_lowercase().ends_with("google")),
            "厂商组内产品快捷方式应使厂商组成候选"
        );

        // 无关软件不得被产品名匹配。
        let c_other = ctx("Other App", None, None);
        // 其他组存在且精确匹配，应能找到自己的组；但 7-Zip 不得出现在其结果里。
        let other = enum_start_menu(&c_other, &roots);
        assert!(other
            .iter()
            .all(|g| !g.root_path.to_lowercase().contains("7-zip")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn vendor_group_without_product_link_is_not_candidate() {
        let (base, roots) = tmp_roots("smvendor");
        let programs = roots.start_menu_roots()[0].clone();
        write_file(&programs.join("BigVendor").join("Other Product.lnk"), "x");
        let c = ctx("Target App 1.0", Some("BigVendor Inc"), None);
        assert!(enum_start_menu(&c, &roots).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn enum_appdata_product_and_vendor_subdir() {
        let (base, roots) = tmp_roots("ad");
        let local = roots.local_appdata.clone();
        // 直接产品目录。
        write_file(&local.join("MyApp").join("config.json"), "{}");
        write_file(&local.join("MyApp").join("Cache").join("x.bin"), "x");
        // 厂商\产品 两层。
        let roaming = roots.appdata_roaming.clone();
        write_file(&roaming.join("Acme").join("MyApp").join("prefs.ini"), "y");
        // 干扰：厂商目录下其他产品。
        write_file(&roaming.join("Acme").join("OtherSuite").join("z.ini"), "z");

        let c = ctx("MyApp 2.0", Some("Acme Inc"), None);
        let found = enum_appdata(&c, &roots);
        assert_eq!(found.len(), 2, "Local 直目录 + Roaming 厂商子目录");
        let local_c = found
            .iter()
            .find(|f| f.root_path.to_lowercase().ends_with(r"local\myapp"))
            .expect("Local 产品目录");
        assert_eq!(local_c.file_count, 2);
        let roam_c = found
            .iter()
            .find(|f| f.root_path.to_lowercase().ends_with(r"roaming\acme\myapp"))
            .expect("Roaming 厂商\\产品");
        assert_eq!(roam_c.file_count, 1);
        // 厂商目录本身绝不成候选（OtherSuite 未被卷入）。
        assert!(found
            .iter()
            .all(|f| !f.root_path.to_lowercase().ends_with(r"roaming\acme")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reparse_subtree_makes_install_dir_not_a_candidate() {
        // 候选根内若存在 junction 子树，整组放弃（防链接指向外部目录被一起移动）。
        let (base, roots) = tmp_roots("reparse");
        let loc = base.join("LinkedApp");
        let real = base.join("realstore");
        std::fs::create_dir_all(&loc).unwrap();
        std::fs::create_dir_all(&real).unwrap();
        write_file(&loc.join("app.exe"), "x");
        let link = loc.join("pointer");
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .expect("mklink");
        if !out.status.success() {
            // CI 环境 mklink 不可用时跳过本用例（不伪造结论）。
            let _ = std::fs::remove_dir_all(&base);
            return;
        }
        assert!(
            enum_install_dir(&ctx("LinkedApp", None, Some(loc.to_str().unwrap())), &roots)
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // ---------- 清理准备（快照信任/去向/幂等）----------

    fn snapshot_with(cands: Vec<ResidueCandidate>) -> ResidueSnapshot {
        ResidueSnapshot {
            context: ctx("X", None, None),
            candidates: cands,
            scanned_at: 0,
        }
    }

    fn file_candidate(base: &Path, id: &str, file: &str) -> ResidueCandidate {
        let p = base.join(file);
        write_file(&p, "x");
        let root_str = base.to_string_lossy().into_owned();
        let items = build_items(vec![p], "test");
        ResidueCandidate {
            id: id.into(),
            kind: ResidueKind::InstallDir,
            app_name: "X".into(),
            root_path: root_str,
            reason: "test".into(),
            size_bytes: items.iter().map(|i| i.size_bytes).sum(),
            file_count: items.len() as u64,
            items,
        }
    }

    #[test]
    fn prepare_rejects_id_not_in_snapshot_and_bad_disposition() {
        let (base, _roots) = tmp_roots("prep");
        let snap = snapshot_with(vec![file_candidate(&base, "c1", "a.txt")]);
        // 伪造候选 id → 拒绝。
        let r = prepare_targets(
            &snap,
            &[ResidueSelection {
                candidate_id: "forged".into(),
                disposition: Disposition::Quarantine,
            }],
        );
        assert!(r.is_err());
        // 非法去向（recycle 不提供给残留）→ 拒绝。
        let r2 = prepare_targets(
            &snap,
            &[ResidueSelection {
                candidate_id: "c1".into(),
                disposition: Disposition::Recycle,
            }],
        );
        assert!(r2.is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn prepare_builds_authorized_targets_and_dedupes() {
        let (base, _roots) = tmp_roots("prep2");
        let c1 = file_candidate(&base, "c1", "a.txt");
        let snap = snapshot_with(vec![c1]);
        let prepped = prepare_targets(
            &snap,
            &[
                ResidueSelection {
                    candidate_id: "c1".into(),
                    disposition: Disposition::Direct,
                },
                // 重复选择幂等。
                ResidueSelection {
                    candidate_id: "c1".into(),
                    disposition: Disposition::Direct,
                },
            ],
        )
        .expect("应解析成功");
        assert_eq!(prepped.targets.len(), 1);
        assert_eq!(prepped.targets[0].disposition, Disposition::Direct);
        assert_eq!(prepped.targets[0].category_id, RESIDUE_CATEGORY_ID);
        assert_eq!(
            prepped.targets[0].authorized_root.as_deref(),
            Some(base.as_path())
        );
        assert_eq!(prepped.roots.len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn prune_empty_dirs_removes_only_empty_tree() {
        let base = std::env::temp_dir().join(format!(
            "pureslate-prune-{}-{}",
            std::process::id(),
            crate::logging::audit::now_ms()
        ));
        let keep = base.join("keep");
        write_file(&keep.join("survive.txt"), "x");
        let empty = base.join("empty").join("deep");
        std::fs::create_dir_all(&empty).unwrap();
        prune_empty_dirs(std::slice::from_ref(&base));
        // 空树自底向上删净。
        assert!(!base.join("empty").exists());
        // 非空目录与文件保留（RemoveDirectoryW 对非空必然失败）。
        assert!(keep.join("survive.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
