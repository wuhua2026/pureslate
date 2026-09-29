//! NTFS MFT 直读枚举（scanner/mft）。SPEC §6.1：当 DG-1 全维度基准 >120s 时启用。
//!
//! 原理：通过 `FSCTL_ENUM_USN_DATA` 一次流式读出整卷 MFT 记录（文件引用号/父引用号/
//! 文件名/属性/V3 长度），内存中拼装全路径，替代 walkdir 逐目录 + 每文件 stat() 的慢路径。
//!
//! 安全红线（SAFETY §2 / AGENTS §4.1）：本模块**只读不写**，不触发任何删除/移动；
//! 白名单过滤照旧在匹配阶段强制生效；MFT 记录不含软链目标内容，天然不跟随 reparse point。
//!
//! 权限与降级：打开卷句柄需管理员（或 SeBackupPrivilege）。统一入口 `walk_matching`：
//! 非管理员或目标不在 NTFS/USN 可用卷上时，自动回退 `super::walk::walk_target`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::contract::{Disposition, Grade, ScanItem};
use crate::safety::whitelist::is_whitelisted;

use super::matcher::CompiledCategory;
use super::walk::{walk_target, CancelToken, ProgressFn};

/// NTFS 卷根目录的固定 MFT 记录引用号。
const ROOT_REF: u64 = 5;
/// `FILE_ATTRIBUTE_DIRECTORY`。
const DIR_ATTR: u32 = 0x10;

/// 单条 MFT 记录（解析自 USN 枚举缓冲区）。
#[derive(Debug, Clone)]
pub struct MftEntry {
    /// 文件/目录引用号（Map key）。
    pub reference: u64,
    /// 父目录引用号。
    pub parent: u64,
    /// 文件名（UTF-8，来自 UTF-16LE）。
    pub name: String,
    /// 是否为目录。
    pub is_dir: bool,
    /// 文件长度（字节）。V2 记录无长度时为 0（调用方可按需补 stat）。
    pub file_len: u64,
}

/// MFT 枚举/使用过程中的错误。生产路径错误一律经此传播（AGENTS §4.6）。
#[derive(Debug, thiserror::Error)]
pub enum MftError {
    #[error("缺少管理员权限，需以管理员身份运行以启用 MFT 直读枚举")]
    NotAdmin,
    #[error("打开卷句柄失败 (os error {0})")]
    OpenVolume(i32),
    #[error("USN 枚举失败 (os error {0})")]
    Enum(i32),
    #[error("卷不可用（非 NTFS 或无 USN 日志）: {0}")]
    Unavailable(String),
}

/// 一次 MFT 会话：持有已枚举的整卷记录，供多次 target 匹配复用以避免重复遍历。
pub struct MftSession {
    volume_root: PathBuf,
    entries: Vec<MftEntry>,
    index: HashMap<u64, MftEntry>,
}

impl MftSession {
    /// 尝试打开卷并一次性枚举整卷 MFT 记录。
    ///
    /// 任一前置不满足（非管理员 / 打开卷失败 / 非 NTFS）→ 返回 Err，调用方回退 walkdir。
    pub fn try_open(volume_root: &Path) -> Result<Self, MftError> {
        if !is_admin() {
            return Err(MftError::NotAdmin);
        }
        let entries = enumerate_volume(volume_root)?;
        let index = entries.iter().map(|e| (e.reference, e.clone())).collect();
        Ok(Self {
            volume_root: volume_root.to_path_buf(),
            entries,
            index,
        })
    }

    /// 用 MFT 记录替代 walkdir，对单一 target 起点做匹配，产出 ScanItem[]。
    ///
    /// 幂等于 `walk_target`：白名单过滤、include/exclude + 相对 target 根匹配、
    /// min_size 剪枝、取消令牌、进度回调；仅路径来自拼装而非逐级目录遍历。
    pub fn walk_target(
        &self,
        target_start: &Path,
        compiled: &CompiledCategory,
        grade: Grade,
        disposition: Disposition,
        cancel: &CancelToken,
        progress: Option<&ProgressFn>,
    ) -> Vec<ScanItem> {
        // 根判定在 resolve_path 内按"低 48 位记录号 == 5"处理（序列号任意），
        // 无需预置 cache 种子。
        let mut cache: HashMap<u64, PathBuf> = HashMap::new();

        let min_size = compiled.min_size_bytes();
        let mut out = Vec::new();
        let mut done = 0u64;
        let mut bytes = 0u64;

        for e in &self.entries {
            if cancel.is_cancelled() {
                break;
            }
            if e.is_dir {
                continue;
            }
            let Some(path) =
                resolve_path(&self.index, &mut cache, &self.volume_root, e.reference, 64)
            else {
                continue;
            };
            // 仅处理 target 前缀下的项（MFT 全卷枚举的剪枝）。
            if path.strip_prefix(target_start).is_err() {
                continue;
            }
            if is_whitelisted(&path) {
                continue;
            }
            if !compiled.matches_any(&path, target_start) {
                continue;
            }
            // USN 记录无长度/mtime：命中项补一次 stat（命中集远小于全盘，代价可控；
            // stat 失败=文件已消失/不可达，软跳过）。
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let size = meta.len();
            if size < min_size {
                continue;
            }
            out.push(ScanItem {
                id: stable_id(&compiled.category_id, &path),
                category_id: compiled.category_id.clone(),
                label: e.name.clone(),
                path: path.to_string_lossy().into_owned(),
                size_bytes: size,
                grade,
                disposition,
                reason: "匹配规则".into(),
                mtime: to_epoch_ms(meta.modified().ok()),
                atime: to_epoch_ms(meta.accessed().ok()),
                dup_group: None,
            });
            done += 1;
            bytes += size;
            if let Some(p) = progress {
                p(done, bytes);
            }
        }
        if let Some(p) = progress {
            p(done, bytes);
        }
        out
    }

    /// 全盘文件统计（large/dup 时常用）：files 总数、≥large_min 的文件数、size→count 分组。
    pub fn full_disk_stats(&self, large_min: u64, want_large: bool) -> MftScanStats {
        let mut stats = MftScanStats {
            files: 0,
            large_files: 0,
            buckets: HashMap::new(),
        };
        for e in &self.entries {
            if e.is_dir {
                continue;
            }
            let size = e.file_len;
            stats.files += 1;
            if want_large && size >= large_min {
                stats.large_files += 1;
            }
            *stats.buckets.entry(size).or_insert(0u64) += 1;
        }
        stats
    }

    /// 排障诊断（探针专用）：(总记录, 目录数, 文件数, 前 n 条样本)。
    pub fn diag(&self, n: usize) -> (usize, usize, usize, Vec<MftEntry>) {
        let dirs = self.entries.iter().filter(|e| e.is_dir).count();
        let samples = self.entries.iter().take(n).cloned().collect();
        (self.entries.len(), dirs, self.entries.len() - dirs, samples)
    }

    /// 排障（探针专用）：target 前缀下的文件记录统计。
    /// 返回 (大小写敏感 strip_prefix 命中数, 不敏感前缀命中数, 不敏感样本路径)。
    /// 用于诊断 MFT 拼装路径与目标展开路径的组件差异（大小写/拼写）。
    pub fn diag_target(&self, target: &Path, n: usize) -> (usize, usize, Vec<String>) {
        let mut cache: HashMap<u64, PathBuf> = HashMap::new();
        let t_norm = target.to_string_lossy().replace('\\', "/").to_lowercase();
        let t_norm = t_norm.trim_end_matches('/').to_owned();
        let mut cs = 0usize;
        let mut ci = 0usize;
        let mut samples = Vec::new();
        for e in &self.entries {
            if e.is_dir {
                continue;
            }
            let Some(p) = resolve_path(&self.index, &mut cache, &self.volume_root, e.reference, 64)
            else {
                continue;
            };
            if p.strip_prefix(target).is_ok() {
                cs += 1;
            }
            let pl = p.to_string_lossy().replace('\\', "/").to_lowercase();
            if pl == t_norm || pl.starts_with(&format!("{t_norm}/")) {
                ci += 1;
                if samples.len() < n {
                    samples.push(p.to_string_lossy().into_owned());
                }
            }
        }
        (cs, ci, samples)
    }
}

/// `full_disk_stats` 的结果。
#[derive(Debug, Default)]
pub struct MftScanStats {
    pub files: u64,
    pub large_files: u64,
    pub buckets: HashMap<u64, u64>,
}

/// 统一入口：优先 MFT（管理员 + NTFS），否则回退 walkdir。
pub fn walk_matching(
    target_start: &Path,
    compiled: &CompiledCategory,
    grade: Grade,
    disposition: Disposition,
    cancel: &CancelToken,
    progress: Option<&ProgressFn>,
) -> Vec<ScanItem> {
    if let Some(vol) = volume_root_of(target_start) {
        if let Ok(session) = MftSession::try_open(&vol) {
            return session.walk_target(
                target_start,
                compiled,
                grade,
                disposition,
                cancel,
                progress,
            );
        }
    }
    walk_target(target_start, compiled, grade, disposition, cancel, progress)
}

/// 当前进程是否以管理员（提权）身份运行。
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        imp::is_elevated_ffi()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 枚举卷根下的全部 MFT 记录（文件+目录）。
fn enumerate_volume(volume_root: &Path) -> Result<Vec<MftEntry>, MftError> {
    #[cfg(windows)]
    {
        imp::enumerate_ffi(volume_root)
    }
    #[cfg(not(windows))]
    {
        let _ = volume_root;
        Err(MftError::Unavailable("仅支持 Windows/NTFS".into()))
    }
}

/// 排障探针：首批记录原始字节（前 128）+ 前 n 批的返回大小（沿用现有续批策略）。
/// 非生产路径，仅供 probe-mft 诊断真实 USN 记录解析。
pub fn debug_first_records(volume_root: &Path, n_batches: usize) -> Option<(Vec<u8>, Vec<u32>)> {
    #[cfg(windows)]
    {
        imp::debug_first_batches(volume_root, n_batches)
    }
    #[cfg(not(windows))]
    {
        let _ = (volume_root, n_batches);
        None
    }
}

/// 从绝对路径提取卷根（`C:\`）。非绝对路径返回 None。
fn volume_root_of(path: &Path) -> Option<PathBuf> {
    let s = path.to_string_lossy();
    let mut chars = s.chars();
    let first = chars.next()?;
    let second = chars.next()?;
    if first.is_ascii_alphabetic() && second == ':' {
        Some(PathBuf::from(format!("{first}:\\")))
    } else {
        None
    }
}

/// 拼装引用号对应全路径，逐层沿父引用上溯，结果缓存在 `cache`。
/// `depth` 为递归深度上限（防御异常父环导致栈溢出；正常 NTFS 层级远小于此）。
fn resolve_path(
    index: &HashMap<u64, MftEntry>,
    cache: &mut HashMap<u64, PathBuf>,
    volume_root: &Path,
    reference: u64,
    depth: u32,
) -> Option<PathBuf> {
    if let Some(p) = cache.get(&reference) {
        return Some(p.clone());
    }
    // MFT 引用 = (序列号<<48)|记录号；根目录记录号固定 5、序列号任意——必须按
    // 低 48 位判根：根记录本身可能不在枚举结果里，且子记录存的父引用带序列号
    // （2026-09-29 probe 取证：parent=0x0005_0000_0000_0005，裸比较 ==5 永不命中，
    // 导致所有路径解析在最后一跳断裂、MFT 模式全维度 0 项）。
    if (reference & 0x0000_FFFF_FFFF_FFFF) == ROOT_REF {
        let vr = volume_root.to_path_buf();
        cache.insert(reference, vr.clone());
        return Some(vr);
    }
    if depth == 0 {
        return None; // 异常父环防御
    }
    let entry = index.get(&reference)?;
    let parent_p = resolve_path(index, cache, volume_root, entry.parent, depth - 1)?;
    let p = parent_p.join(&entry.name);
    cache.insert(reference, p.clone());
    Some(p)
}

/// 是否为 MFT 内部元数据名（`$` 开头，如 $MFT/$LogFile）。这些永不落入删除候选。
fn is_meta_name(name: &str) -> bool {
    name.starts_with('$')
}

/// 同 walk.rs 的稳定条目 id 生成（category_id + path 散列）。
fn stable_id(category_id: &str, path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    category_id.hash(&mut h);
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 同 walk.rs：SystemTime → epoch 毫秒。
fn to_epoch_ms(t: Option<std::time::SystemTime>) -> Option<i64> {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

/// Windows 平台 FFI 实现。
///
/// 仅依赖 `kernel32`/`advapi32` 的少量原生导出，手写 `extern "system"` 声明，
/// 不引入任何 crate（维持 AGENTS §4.7 依赖纪律、安装包 <20MB 门禁）。
/// 每个 `unsafe` 块均附一行理由注释。
#[cfg(windows)]
mod imp {
    use super::{MftEntry, MftError, DIR_ATTR};
    use std::ffi::c_void;
    use std::path::Path;

    // 常量（Win32 头文件取值）。
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_SHARE_DELETE: u32 = 4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    // CTL_CODE(FILE_DEVICE_FILE_SYSTEM(9), 44, METHOD_NEITHER(3), FILE_ANY_ACCESS(0))。
    const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00B3;
    const ERROR_HANDLE_EOF: i32 = 38;
    const INVALID_HANDLE: isize = -1;
    // Token access / token information class。
    const TOKEN_QUERY: u32 = 0x0000_0008;
    // TokenInformationClass::TokenElevation = 20（判定是否提权）。
    // 注意：22 是 TokenElevationClass/LinkedToken，会查错对象，必须用 20。
    const TOKEN_ELEVATION: i32 = 20;

    /// `MFT_ENUM_DATA_V0`：FSCTL_ENUM_USN_DATA 的输入结构。
    #[repr(C)]
    struct MftEnumData {
        start_file_reference: u64,
        low_usn: i64,
        high_usn: i64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        // 返回卷句柄（HANDLE，失败为 INVALID_HANDLE_VALUE）。
        fn CreateFileW(
            file: *const u16,
            desired_access: u32,
            share_mode: u32,
            security: *const c_void,
            creation: u32,
            flags: u32,
            template: *const c_void,
        ) -> *mut c_void;
        // 关闭句柄。
        fn CloseHandle(handle: *mut c_void) -> i32;
        // 卷 USN 枚举；成功返回非 0。
        fn DeviceIoControl(
            device: *mut c_void,
            code: u32,
            in_buf: *const c_void,
            in_size: u32,
            out_buf: *mut c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *const c_void,
        ) -> i32;
        // 当前进程伪句柄。
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
        fn GetTokenInformation(
            token: *mut c_void,
            class: i32,
            info: *mut c_void,
            info_len: u32,
            ret_len: *mut u32,
        ) -> i32;
    }

    /// 从一批 USN 记录缓冲区解析 MftEntry 追加到 `out`，并收集末尾引用号列表
    /// （供下一轮 `FSCTL_ENUM_USN_DATA` 作为 StartFileReferenceNumber）。
    fn collect_records(
        data: &[u8],
        out: &mut Vec<MftEntry>,
        refs: &mut Vec<u64>,
    ) -> Result<(), MftError> {
        let mut off = 0usize;
        while off + 4 <= data.len() {
            let rec_len = le_u32(data, off)? as usize;
            if rec_len < 60 {
                break;
            }
            let end = off + rec_len;
            if end > data.len() {
                break;
            }
            let file_ref = le_u64(data, off + 8)?;
            let parent = le_u64(data, off + 16)?;
            let attr = le_u32(data, off + 52)?;
            let name_len = le_u16(data, off + 56)? as usize;
            let name_off = le_u16(data, off + 58)? as usize;
            let is_dir = attr & DIR_ATTR != 0;
            // USN 记录（V2/V3）不含文件长度：V3 是 ReFS 128 位引用号变体，同样无
            // size 字段。长度统一 0，由调用方按需 stat（walk_target 对命中项已补）。
            let file_len = 0u64;
            if let Some(nbytes) = data.get(off + name_off..off + name_off + name_len) {
                let mut u16v = Vec::with_capacity(name_len / 2);
                let mut i = 0;
                while i + 1 < nbytes.len() {
                    u16v.push(u16::from_le_bytes([nbytes[i], nbytes[i + 1]]));
                    i += 2;
                }
                let name = String::from_utf16_lossy(&u16v);
                if !super::is_meta_name(&name) {
                    out.push(MftEntry {
                        reference: file_ref,
                        parent,
                        name,
                        is_dir,
                        file_len,
                    });
                }
                refs.push(file_ref);
            }
            off = end;
        }
        Ok(())
    }

    fn le_u16(buf: &[u8], off: usize) -> Result<u16, MftError> {
        let a = buf.get(off..off + 2).ok_or(MftError::Enum(-2))?;
        Ok(u16::from_le_bytes([a[0], a[1]]))
    }
    fn le_u32(buf: &[u8], off: usize) -> Result<u32, MftError> {
        let a = buf.get(off..off + 4).ok_or(MftError::Enum(-2))?;
        Ok(u32::from_le_bytes([a[0], a[1], a[2], a[3]]))
    }
    fn le_u64(buf: &[u8], off: usize) -> Result<u64, MftError> {
        let a = buf.get(off..off + 8).ok_or(MftError::Enum(-2))?;
        Ok(u64::from_le_bytes(a.try_into().unwrap()))
    }

    fn last_os_error() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
    }

    pub(super) fn enumerate_ffi(volume_root: &Path) -> Result<Vec<MftEntry>, MftError> {
        // 卷设备路径必须用 `\\.\C:` 形式（无尾反斜杠）：带尾 `\` 会打开卷根目录
        // 而非卷设备——句柄能开但 FSCTL_ENUM_USN_DATA 报 os error 5（2026-09-29
        // probe-fsctl 矩阵取证：无尾斜杠形式下五种访问掩码组合全部枚举成功）。
        let vol = format!(
            r"\\.\{}",
            volume_root.to_string_lossy().trim_end_matches('\\')
        );
        let wide: Vec<u16> = vol.encode_utf16().chain(std::iter::once(0)).collect();

        // unsafe：调用 kernel32,CreateFileW 打开卷句柄（需管理员/SeBackupPrivilege）。
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null(),
            )
        };
        if handle as isize == INVALID_HANDLE {
            return Err(MftError::OpenVolume(last_os_error()));
        }

        let mut out: Vec<MftEntry> = Vec::new();
        let mut start_ref: u64 = 0;
        const BUF: usize = 256 * 1024;
        let mut buf = vec![0u8; BUF];

        let result = (|| -> Result<(), MftError> {
            loop {
                let mft_enum = MftEnumData {
                    start_file_reference: start_ref,
                    low_usn: i64::MIN,
                    high_usn: i64::MAX,
                };
                let mut returned = 0u32;
                // unsafe：kernel32,DeviceIoControl(FSCTL_ENUM_USN_DATA) 流式读卷记录。
                let ok = unsafe {
                    DeviceIoControl(
                        handle,
                        FSCTL_ENUM_USN_DATA,
                        &mft_enum as *const _ as *const c_void,
                        std::mem::size_of::<MftEnumData>() as u32,
                        buf.as_mut_ptr() as *mut c_void,
                        BUF as u32,
                        &mut returned,
                        std::ptr::null(),
                    )
                } != 0;
                if !ok {
                    let code = last_os_error();
                    if code == ERROR_HANDLE_EOF {
                        break; // 记录枚举完毕
                    }
                    return Err(MftError::Enum(code));
                }
                if returned == 0 {
                    break;
                }
                // 输出布局 = [8 字节 NextFileReferenceNumber][USN 记录...]：
                // 前 8 字节是下一批的 StartFileReferenceNumber，记录从 +8 起。
                // （2026-09-29 probe 取证修正：旧实现从偏移 0 解析，首条"伪记录"
                // rec_len=631 吞掉整批真实记录；且用末记录 ref 续批，一轮即断。）
                if (returned as usize) < 8 {
                    break;
                }
                let next_ref = le_u64(&buf, 0)?;
                let mut refs = Vec::new();
                collect_records(&buf[8..returned as usize], &mut out, &mut refs)?;
                if next_ref == 0 || next_ref <= start_ref {
                    break; // 防御：续批引用号未前进即终止，防死循环
                }
                start_ref = next_ref;
            }
            Ok(())
        })();

        // unsafe：kernel32,CloseHandle 释放卷句柄。
        unsafe {
            let _ = CloseHandle(handle);
        }
        result?;
        Ok(out)
    }

    /// 排障探针：打开卷设备，取首批记录原始字节（前 128）+ 前 n 批返回大小，
    /// 续批策略与 `enumerate_ffi` 一致（末记录引用号），用于诊断真实记录解析。
    pub(super) fn debug_first_batches(
        volume_root: &Path,
        n_batches: usize,
    ) -> Option<(Vec<u8>, Vec<u32>)> {
        let vol = format!(
            r"\\.\{}",
            volume_root.to_string_lossy().trim_end_matches('\\')
        );
        let wide: Vec<u16> = vol.encode_utf16().chain(std::iter::once(0)).collect();
        // unsafe：kernel32,CreateFileW 打开卷设备句柄。
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null(),
            )
        };
        if handle as isize == INVALID_HANDLE {
            return None;
        }
        let mut buf = vec![0u8; 64 * 1024];
        let mut sizes: Vec<u32> = Vec::new();
        let mut first_bytes: Vec<u8> = Vec::new();
        let mut start_ref: u64 = 0;
        for _ in 0..n_batches {
            let mft_enum = MftEnumData {
                start_file_reference: start_ref,
                low_usn: i64::MIN,
                high_usn: i64::MAX,
            };
            let mut returned = 0u32;
            // unsafe：kernel32,DeviceIoControl(FSCTL_ENUM_USN_DATA) 取一批记录。
            let ok = unsafe {
                DeviceIoControl(
                    handle,
                    FSCTL_ENUM_USN_DATA,
                    &mft_enum as *const _ as *const c_void,
                    std::mem::size_of::<MftEnumData>() as u32,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len() as u32,
                    &mut returned,
                    std::ptr::null(),
                )
            } != 0;
            if !ok || returned == 0 {
                break;
            }
            sizes.push(returned);
            if first_bytes.is_empty() {
                first_bytes.extend_from_slice(&buf[..(returned as usize).min(128)]);
            }
            if (returned as usize) < 8 {
                break;
            }
            // 与 enumerate_ffi 同规：前 8 字节为续批引用号，记录从 +8 起。
            let next_ref = le_u64(&buf, 0).ok()?;
            let mut entries = Vec::new();
            let mut refs = Vec::new();
            let _ = collect_records(&buf[8..returned as usize], &mut entries, &mut refs);
            if next_ref == 0 || next_ref <= start_ref {
                break;
            }
            start_ref = next_ref;
        }
        // unsafe：kernel32,CloseHandle 释放卷句柄。
        unsafe {
            let _ = CloseHandle(handle);
        }
        if sizes.is_empty() {
            None
        } else {
            Some((first_bytes, sizes))
        }
    }

    /// 提权判定：TokenElevation。
    pub(super) fn is_elevated_ffi() -> bool {
        let mut token: *mut c_void = std::ptr::null_mut();
        // unsafe：advapi32,OpenProcessToken 取当前进程 token。
        let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0;
        if !ok {
            return false;
        }
        // TokenElevation 查询须恰好传 DWORD(4 字节) 缓冲区；传 8+ 字节会触发
        // ERROR_BAD_LENGTH。取偏移 0 的 DWORD 即 TokenIsElevated。
        let buf = 0u32;
        let mut ret = 0u32;
        // unsafe：advapi32,GetTokenInformation(TokenElevation) 判定是否提权。
        let elevated = unsafe {
            GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                &buf as *const u32 as *mut c_void,
                std::mem::size_of::<u32>() as u32,
                &mut ret,
            )
        } != 0
            && buf != 0;
        // unsafe：kernel32,CloseHandle 释放 token 句柄。
        unsafe {
            let _ = CloseHandle(token);
        }
        elevated
    }

    /// 测试辅助：解析一段合成记录缓冲区。
    #[cfg(test)]
    pub(super) fn parse_records_test(data: &[u8]) -> Result<Vec<MftEntry>, MftError> {
        let mut out = Vec::new();
        let mut refs = Vec::new();
        collect_records(data, &mut out, &mut refs)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::Category;
    use crate::rules::model::{Disposition as RuleDisposition, GlobRule, Risk, Target, TargetType};

    fn test_category() -> Category {
        Category {
            id: "temp.user".into(),
            label: "t".into(),
            risk: Risk::Green,
            disposition: RuleDisposition::Direct,
            description: None,
            targets: vec![Target {
                ty: TargetType::Path,
                value: "C:\\alice".into(),
            }],
            includes: vec![GlobRule {
                pattern: "**/*.tmp".into(),
                recursive: true,
                max_age_days: 0,
                min_size_mb: 0,
            }],
            excludes: vec![],
            guard_process: None,
        }
    }

    fn entry(reference: u64, parent: u64, name: &str, is_dir: bool, len: u64) -> MftEntry {
        MftEntry {
            reference,
            parent,
            name: name.into(),
            is_dir,
            file_len: len,
        }
    }

    #[test]
    fn parse_v2_record_extracts_name_no_size() {
        // 真实 V2 记录布局（2026-09-29 probe 取证）：NameOffset=60，USN 记录
        // 不含文件长度（file_len 恒 0，长度由调用方 stat 补）。
        let mut buf = vec![0u8; 72];
        buf[0..4].copy_from_slice(&72u32.to_le_bytes());
        buf[4..6].copy_from_slice(&2u16.to_le_bytes()); // MajorVersion = 2
        buf[8..16].copy_from_slice(&0x100u64.to_le_bytes());
        buf[16..24].copy_from_slice(&5u64.to_le_bytes()); // parent = root
        buf[52..56].copy_from_slice(&0u32.to_le_bytes()); // 普通文件
        buf[56..58].copy_from_slice(&10u16.to_le_bytes()); // NameLength = 10 字节
        buf[58..60].copy_from_slice(&60u16.to_le_bytes()); // NameOffset = 60
        let name: Vec<u16> = "a.tmp".encode_utf16().collect();
        for (i, ch) in name.iter().enumerate() {
            let o = 60 + i * 2;
            buf[o..o + 2].copy_from_slice(&ch.to_le_bytes());
        }
        #[cfg(windows)]
        {
            let parsed = imp::parse_records_test(&buf).unwrap();
            assert_eq!(parsed.len(), 1);
            assert_eq!(parsed[0].reference, 0x100);
            assert_eq!(parsed[0].file_len, 0, "USN 记录无长度字段");
            assert_eq!(parsed[0].name, "a.tmp");
            assert!(!parsed[0].is_dir);
        }
    }

    #[test]
    fn resolve_path_builds_full_path() {
        // 父引用带序列号（(seq<<48)|ref）：根目录须按低 48 位记录号识别——
        // 2026-09-29 修复的回归锁定（裸比较 ==5 会让所有路径解析在根处断裂）。
        let seq_root = 0x0005_0000_0000_0005u64; // 序列号 5 的根引用
        let index: HashMap<u64, MftEntry> = HashMap::from([
            (0x10, entry(0x10, seq_root, "Users", true, 0)),
            (0x20, entry(0x20, 0x10, "alice", true, 0)),
            (0x30, entry(0x30, 0x20, "a.tmp", false, 9)),
        ]);
        let vol = PathBuf::from(r"C:\");
        let mut cache = HashMap::new();
        let p = resolve_path(&index, &mut cache, &vol, 0x30, 64).unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Users\alice\a.tmp"));
    }

    #[test]
    fn walk_target_filters_and_matches() {
        // USN 记录无长度：walk_target 对命中项补 stat——用真实临时文件构造索引，
        // 同时验证 size_bytes 来自真实 stat 而非（恒 0 的）记录长度。
        let dir = std::env::temp_dir().join(format!(
            "pureslate-mft-walk-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.tmp"), "0123456789abcde").unwrap(); // 15 字节
        std::fs::write(dir.join("b.log"), "blog").unwrap();
        let dir_name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let vol = std::env::temp_dir();

        let entries = vec![
            entry(0x20, 5, &dir_name, true, 0),
            entry(0x30, 0x20, "a.tmp", false, 0),
            entry(0x31, 0x20, "b.log", false, 0),
        ];
        let index: HashMap<u64, MftEntry> =
            entries.iter().map(|e| (e.reference, e.clone())).collect();
        let session = MftSession {
            volume_root: vol,
            entries,
            index,
        };
        let cc = CompiledCategory::compile(&test_category());
        let cancel = CancelToken::new();
        let items =
            session.walk_target(&dir, &cc, Grade::Green, Disposition::Direct, &cancel, None);
        let paths: Vec<String> = items.iter().map(|i| i.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("a.tmp")),
            "a.tmp 应命中: {paths:?}"
        );
        assert!(!paths.iter().any(|p| p.ends_with("b.log")));
        assert_eq!(
            items
                .iter()
                .find(|i| i.path.ends_with("a.tmp"))
                .map(|i| i.size_bytes),
            Some(15),
            "size 应来自 stat 补齐"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
