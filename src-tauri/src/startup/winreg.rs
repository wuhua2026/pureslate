//! 注册表薄封装（R07 启动项）。
//!
//! 沿用 P1-02b 依赖决策：`#[link]` 直调 advapi32，零第三方 crate。
//! 仅覆盖本模块所需操作：枚举值 / 写值 / 删值 / 删测试键；UTF-16 安全
//! （值名与数据一律经 UTF-16 转换，禁 String 拼宽字符，AGENTS §5）。

use std::ffi::c_void;
use std::io;

/// 预定义根键句柄（伪句柄，整数值）。
pub type HKey = *mut c_void;

/// HKCU 预定义句柄（0x80000001）。
pub const HKEY_CURRENT_USER: HKey = 0x8000_0001usize as *mut c_void;
/// HKLM 预定义句柄（0x80000002）。
pub const HKEY_LOCAL_MACHINE: HKey = 0x8000_0002usize as *mut c_void;

/// 只读访问掩码（winnt.h KEY_READ 原值 = STANDARD_RIGHTS_READ | QUERY_VALUE |
/// ENUMERATE_SUB_KEYS | NOTIFY = 0x20019）。
/// **不得含 SET_VALUE/CREATE_SUB_KEY 写位**——标准用户对 HKLM 请求写位会被 ACL
/// 直接拒绝（code 5），曾误用 0x2001F 致非提权形态 HKLM 枚举整源静默丢失。
pub const KEY_READ: u32 = 0x0002_0019;
/// WOW64 视图标志（R27 卸载枚举：x64 进程显式访问 64/32 位视图）。
/// 取自 winnt.h 原值：64KEY=0x0100、32KEY=0x0200（注意不是 0x10000/0x20000）。
pub const KEY_WOW64_64KEY: u32 = 0x0100;
pub const KEY_WOW64_32KEY: u32 = 0x0200;
const KEY_SET_VALUE: u32 = 0x0002;
const ERROR_SUCCESS: i32 = 0;
const ERROR_MORE_DATA: i32 = 234;
const ERROR_NO_MORE_ITEMS: i32 = 259;

/// 值类型（本模块只完整支持 SZ / EXPAND_SZ，枚举时原样返回 kind 供调用方过滤）。
pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;

/// 根键选择（含 `.reg` 备份文件用的路径前缀）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    /// 预定义句柄。
    pub fn raw(self) -> HKey {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    /// `.reg` 文件里的键路径前缀。
    pub fn key_prefix(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
        }
    }

    /// 由键路径前缀反解根键。
    pub fn from_key_prefix(prefix: &str) -> Option<Hive> {
        match prefix {
            "HKEY_CURRENT_USER" => Some(Hive::CurrentUser),
            "HKEY_LOCAL_MACHINE" => Some(Hive::LocalMachine),
            _ => None,
        }
    }
}

/// 一条注册表值（原始字节，调用方按 kind 解释）。
#[derive(Debug, Clone)]
pub struct RegValue {
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hkey: HKey,
        lpsubkey: *const u16,
        reserved: u32,
        access: u32,
        result: *mut HKey,
    ) -> i32;
    fn RegEnumKeyExW(
        hkey: HKey,
        index: u32,
        lpname: *mut u16,
        lpcchname: *mut u32,
        reserved: *mut u32,
        lpclass: *const u16,
        lpcchclass: *const u32,
        lpftlastwritetime: *mut u64,
    ) -> i32;
    fn RegCreateKeyExW(
        hkey: HKey,
        lpsubkey: *const u16,
        reserved: u32,
        lpclass: *const u16,
        options: u32,
        access: u32,
        sa: *const c_void,
        result: *mut HKey,
        disposition: *mut u32,
    ) -> i32;
    fn RegCloseKey(hkey: HKey) -> i32;
    fn RegEnumValueW(
        hkey: HKey,
        index: u32,
        lpvaluename: *mut u16,
        lpcchvaluename: *mut u32,
        reserved: *mut u32,
        lpkind: *mut u32,
        lpdata: *mut u8,
        lpcbdata: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        hkey: HKey,
        lpvaluename: *const u16,
        reserved: u32,
        kind: u32,
        lpdata: *const u8,
        cbdata: u32,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: HKey,
        lpvaluename: *const u16,
        lpreserved: *mut u32,
        lptype: *mut u32,
        lpdata: *mut u8,
        lpcbdata: *mut u32,
    ) -> i32;
    fn RegDeleteValueW(hkey: HKey, lpvaluename: *const u16) -> i32;
    fn RegDeleteKeyW(hkey: HKey, lpsubkey: *const u16) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, dst_chars: u32) -> u32;
}

/// ERROR_FILE_NOT_FOUND：注册表值/键不存在（与 IO 层 NotFound 同源语义）。
const ERROR_FILE_NOT_FOUND: i32 = 2;

/// 打开的键句柄（RAII 关闭）。
pub struct OpenKey(HKey);

impl OpenKey {
    /// 打开既有键。
    pub fn open(hive: Hive, subkey: &str, access: u32) -> io::Result<Self> {
        let sub = wide_null(subkey);
        let mut h: HKey = std::ptr::null_mut();
        // 安全性：参数均为有效指针/预定义句柄，返回码统一转 io::Error 传播。
        let code = unsafe { RegOpenKeyExW(hive.raw(), sub.as_ptr(), 0, access, &mut h) };
        if code == ERROR_SUCCESS {
            Ok(Self(h))
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }

    /// 创建或打开键（restore 场景：键被整体删除后也能重建）。
    pub fn create(hive: Hive, subkey: &str, access: u32) -> io::Result<Self> {
        let sub = wide_null(subkey);
        let mut h: HKey = std::ptr::null_mut();
        // 安全性：参数均为有效指针/预定义句柄，返回码统一转 io::Error 传播。
        let code = unsafe {
            RegCreateKeyExW(
                hive.raw(),
                sub.as_ptr(),
                0,
                std::ptr::null(),
                0, // REG_OPTION_NON_VOLATILE
                access,
                std::ptr::null(),
                &mut h,
                std::ptr::null_mut(),
            )
        };
        if code == ERROR_SUCCESS {
            Ok(Self(h))
        } else {
            Err(io::Error::from_raw_os_error(code))
        }
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        // 安全性：句柄必来自 RegOpenKeyExW/RegCreateKeyExW 成功路径。
        unsafe { RegCloseKey(self.0) };
    }
}

/// 枚举某键下全部子键名（v0.1.5 反馈 4：Active Setup 枚举用）。
pub fn enum_subkeys(hive: Hive, subkey: &str) -> io::Result<Vec<String>> {
    enum_subkeys_view(hive, subkey, 0)
}

/// 枚举子键（显式 WOW64 视图标志，R27：0=进程默认 / KEY_WOW64_64KEY / KEY_WOW64_32KEY）。
pub fn enum_subkeys_view(hive: Hive, subkey: &str, wow64_flags: u32) -> io::Result<Vec<String>> {
    let key = OpenKey::open(hive, subkey, KEY_READ | wow64_flags)?;
    let mut out = Vec::new();
    let mut index: u32 = 0;
    loop {
        let mut name_buf = vec![0u16; 512];
        let mut name_len = name_buf.len() as u32;
        // 安全性：句柄有效；缓冲区长度与指针配对传入，API 不越界写。
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name_buf.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        match code {
            ERROR_SUCCESS => {
                out.push(String::from_utf16_lossy(&name_buf[..name_len as usize]));
                index += 1;
            }
            ERROR_MORE_DATA => {
                name_buf.resize(name_len as usize + 2, 0);
            }
            ERROR_NO_MORE_ITEMS => break,
            e => return Err(io::Error::from_raw_os_error(e)),
        }
    }
    Ok(out)
}

/// UTF-8 → UTF-16 NUL 结尾（路径/值名一律走此转换，UTF-16 安全）。
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 枚举某键下全部值（原始字节）。键不可访问（如无权限）→ Err 传播，调用方跳过该源。
pub fn enum_values(hive: Hive, subkey: &str) -> io::Result<Vec<RegValue>> {
    enum_values_view(hive, subkey, 0)
}

/// 枚举值（显式 WOW64 视图标志，R27）。
pub fn enum_values_view(hive: Hive, subkey: &str, wow64_flags: u32) -> io::Result<Vec<RegValue>> {
    let key = OpenKey::open(hive, subkey, KEY_READ | wow64_flags)?;
    let mut out = Vec::new();
    let mut index: u32 = 0;
    'outer: loop {
        let mut name_buf = vec![0u16; 512];
        let mut data_buf = vec![0u8; 4096];
        loop {
            let mut name_len = name_buf.len() as u32;
            let mut data_len = data_buf.len() as u32;
            let mut kind: u32 = 0;
            // 安全性：句柄有效；缓冲区长度与指针配对传入，API 不越界写。
            let code = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    &mut kind,
                    data_buf.as_mut_ptr(),
                    &mut data_len,
                )
            };
            match code {
                ERROR_SUCCESS => {
                    let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                    data_buf.truncate(data_len as usize);
                    out.push(RegValue {
                        name,
                        kind,
                        data: data_buf.clone(),
                    });
                    index += 1;
                    continue 'outer;
                }
                ERROR_MORE_DATA => {
                    // 缓冲不足：扩容后同 index 重试。
                    if name_len as usize + 2 > name_buf.len() {
                        name_buf.resize(name_len as usize + 2, 0);
                    }
                    if data_len as usize + 16 > data_buf.len() {
                        data_buf.resize(data_len as usize + 16, 0);
                    }
                }
                ERROR_NO_MORE_ITEMS => break 'outer,
                e => return Err(io::Error::from_raw_os_error(e)),
            }
        }
    }
    Ok(out)
}

/// 写入（或覆盖）一个值；键不存在则创建（备份还原用）。
pub fn set_value(hive: Hive, subkey: &str, name: &str, kind: u32, data: &[u8]) -> io::Result<()> {
    let key = OpenKey::create(hive, subkey, KEY_SET_VALUE)?;
    let wide_name = wide_null(name);
    // 安全性：句柄有效；数据指针与长度配对。
    let code = unsafe {
        RegSetValueExW(
            key.0,
            wide_name.as_ptr(),
            0,
            kind,
            data.as_ptr(),
            data.len() as u32,
        )
    };
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// 删除一个值。
pub fn delete_value(hive: Hive, subkey: &str, name: &str) -> io::Result<()> {
    let key = OpenKey::open(hive, subkey, KEY_SET_VALUE)?;
    let wide_name = wide_null(name);
    // 安全性：句柄有效；值为 NUL 结尾 UTF-16。
    let code = unsafe { RegDeleteValueW(key.0, wide_name.as_ptr()) };
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// 删除子键（须为叶子键；仅测试清理自建测试键使用）。
pub fn delete_key(hive: Hive, subkey: &str) -> io::Result<()> {
    let sub = wide_null(subkey);
    // 安全性：参数均为有效指针/预定义句柄。
    let code = unsafe { RegDeleteKeyW(hive.raw(), sub.as_ptr()) };
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// REG_SZ / REG_EXPAND_SZ 原始字节 → 字符串（去尾部 NUL）。
pub fn reg_data_to_string(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut s = String::from_utf16_lossy(&units);
    while s.ends_with('\0') {
        s.pop();
    }
    s
}

/// 字符串 → REG_SZ 原始字节（UTF-16LE + NUL）。
pub fn string_to_reg_data(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|u| u.to_le_bytes())
        .collect()
}

/// 读取单个字符串值（进程默认注册表视图）。见 [`query_string_value_view`]。
pub fn query_string_value(hive: Hive, subkey: &str, name: &str) -> io::Result<Option<String>> {
    query_string_value_view(hive, subkey, name, 0)
}

/// 读取单个字符串值（显式 WOW64 视图标志，R28）。
///
/// 仅接受 REG_SZ / REG_EXPAND_SZ（后者自动展开 `%VAR%` 环境变量）。
/// - 键/值不存在、值类型不是字符串 → `Ok(None)`：机器相关软失败，
///   调用方按"本机未安装或不在默认位置"跳过 target，不报错；
/// - 权限/其它 API 错误 → `Err` 传播。
///
/// 只读纪律：以 KEY_READ=0x20019（不含写位）打开；UTF-16 安全。
pub fn query_string_value_view(
    hive: Hive,
    subkey: &str,
    name: &str,
    wow64_flags: u32,
) -> io::Result<Option<String>> {
    let key = OpenKey::open(hive, subkey, KEY_READ | wow64_flags)?;
    let wide_name = wide_null(name);
    let mut kind: u32 = 0;
    let mut byte_len: u32 = 0;
    // 第一次查询取字节长度（lpdata 传空指针，仅探尺寸）。
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            wide_name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    match code {
        ERROR_FILE_NOT_FOUND => return Ok(None),
        ERROR_SUCCESS | ERROR_MORE_DATA => {}
        other => return Err(io::Error::from_raw_os_error(other)),
    }
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Ok(None);
    }
    let mut buf = vec![0u8; byte_len as usize];
    let mut actual_len = byte_len;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            wide_name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            buf.as_mut_ptr(),
            &mut actual_len,
        )
    };
    match code {
        ERROR_FILE_NOT_FOUND => return Ok(None),
        ERROR_SUCCESS | ERROR_MORE_DATA => {}
        other => return Err(io::Error::from_raw_os_error(other)),
    }
    buf.truncate(actual_len as usize);
    let raw = reg_data_to_string(&buf);
    if kind == REG_EXPAND_SZ {
        Ok(Some(expand_environment_strings(&raw)?))
    } else {
        Ok(Some(raw))
    }
}

/// ExpandEnvironmentStringsW 薄封装：展开字符串中的 `%VAR%` 环境变量（R28）。
fn expand_environment_strings(input: &str) -> io::Result<String> {
    let src: Vec<u16> = input.encode_utf16().chain(std::iter::once(0)).collect();
    // 返回值含结尾 NUL；0 = 失败。
    let needed = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut dst = vec![0u16; needed as usize];
    let ret =
        unsafe { ExpandEnvironmentStringsW(src.as_ptr(), dst.as_mut_ptr(), dst.len() as u32) };
    if ret == 0 {
        return Err(io::Error::last_os_error());
    }
    // ret 含结尾 NUL，截断时去掉。
    let len = (ret as usize).saturating_sub(1);
    dst.truncate(len);
    Ok(String::from_utf16_lossy(&dst))
}
