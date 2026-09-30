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

const KEY_READ: u32 = 0x0002_001F;
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
    fn RegDeleteValueW(hkey: HKey, lpvaluename: *const u16) -> i32;
    fn RegDeleteKeyW(hkey: HKey, lpsubkey: *const u16) -> i32;
}

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

/// UTF-8 → UTF-16 NUL 结尾（路径/值名一律走此转换，UTF-16 安全）。
fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 枚举某键下全部值（原始字节）。键不可访问（如无权限）→ Err 传播，调用方跳过该源。
pub fn enum_values(hive: Hive, subkey: &str) -> io::Result<Vec<RegValue>> {
    let key = OpenKey::open(hive, subkey, KEY_READ)?;
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
