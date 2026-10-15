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
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        multi_byte_str: *const u8,
        multi_byte_len: i32,
        wide_char_str: *mut u16,
        wide_char_len: i32,
    ) -> i32;
}

/// 系统 ANSI 代码页（R30 坏编码防御解码候选之一）。
const CP_ACP: u32 = 0;
/// 简体中文 GBK 代码页（中文 ANSI 时代安装器的 REG_SZ 实际编码）。
const CP_GBK: u32 = 936;

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
///
/// 精确路径/命令用此函数（必须与注册表明义一致，不做猜测性转换）。
/// **展示类字段**（名称/发布者/版本）应走 [`reg_data_decode_display`]——
/// 老安装器会把坏编码写进 REG_SZ，直解会得到满屏乱码（R30）。
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

/// 代码页（CP_ACP/CP936）字节 → 字符串（两段式探测长度；NUL 字节须已剔除，
/// 否则 API 遇到 0 会提前截断）。失败/空结果返回 None。
fn codepage_to_string(code_page: u32, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    // flags=0：非法字节以 U+FFFD 兜底（不报错丢串）；显式长度，不依赖结尾 NUL。
    let needed = unsafe {
        MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return None;
    }
    let mut buf = vec![0u16; needed as usize];
    let got = unsafe {
        MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            buf.as_mut_ptr(),
            needed,
        )
    };
    if got <= 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..got as usize]);
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// 剔除全部 NUL 字节后按代码页解码（R30）。
///
/// 关键观察：真机坏数据的典型形态是"UTF-16 英文头（X 00 交错）+ 原始 GBK 尾"。
/// 去掉 0x00 后，英文头自然塌缩为 ASCII、GBK 尾可由 936 正确解码——
/// 一个候选同时覆盖"整段 ANSI"与"半段拼接"两种损坏。
fn ansi_candidate(raw: &[u8], code_page: u32) -> Option<String> {
    let bytes: Vec<u8> = raw.iter().copied().filter(|&b| b != 0).collect();
    codepage_to_string(code_page, &bytes)
}

/// 字符是否可能出现在正常程序名称/发布者文本里。
///
/// 收录：ASCII、拉丁扩展、常规标点/货币/箭头/带圈数字、CJK 标点/假名/汉字、
/// 全角字符、韩文、西里尔文。其余一律视为可疑（控制符、PUA、U+FFFD、
/// 制表符/框线/几何符号、缅甸文等被坏字节对随机命中的区段）。
fn is_plausible_display_char(c: char) -> bool {
    match c {
        '\t' => true,
        c if c.is_control() => false,
        // ASCII 可打印
        '\u{20}'..='\u{7E}' => true,
        // Latin-1 + Latin Extended（® © ² ³ 及欧洲重音字母）
        '\u{A0}'..='\u{024F}' => true,
        // 通用标点（– — ‘ ’ “ ” … ‰）与货币/箭头/商标/带圈数字
        '\u{2000}'..='\u{206F}' => true,
        '\u{20A0}'..='\u{20CF}' => true,
        '\u{2120}'..='\u{2123}' => true,
        '\u{2190}'..='\u{21FF}' => true,
        '\u{2460}'..='\u{24FF}' => true,
        // CJK 标点 / 假名 / 扩展 A / 汉字主体 / 全角
        '\u{3000}'..='\u{303F}' => true,
        '\u{3040}'..='\u{30FF}' => true,
        '\u{3400}'..='\u{4DBF}' => true,
        '\u{4E00}'..='\u{9FFF}' => true,
        '\u{FF00}'..='\u{FF5E}' => true,
        // 韩文音节 / 西里尔文（本机可能装韩/俄软件，合法）
        '\u{AC00}'..='\u{D7AF}' => true,
        '\u{0400}'..='\u{04FF}' => true,
        _ => false,
    }
}

/// 候选文本的可疑度评分（越低越好；0 = 完全干净）。
/// = 坏字符占比×100 + 异文字系存在惩罚。
/// 异文惩罚用于打散"GBK 尾被当 UTF-16 对"随机命中韩文/缅甸文/希腊文区段的情形。
fn display_score(s: &str) -> f32 {
    let total = s.chars().count().max(1) as f32;
    let mut bad = 0u32;
    let mut greek = false;
    let mut cyrillic = false;
    let mut kana = false;
    let mut hangul = false;
    let mut other_exotic = false;
    for c in s.chars() {
        if !is_plausible_display_char(c) {
            bad += 1;
        }
        match c {
            '\u{0370}'..='\u{03FF}' => greek = true,
            '\u{0400}'..='\u{04FF}' => cyrillic = true,
            '\u{0590}'..='\u{10FF}' => other_exotic = true, // 希伯来/阿拉伯/婆罗米系/缅甸文等
            '\u{3040}'..='\u{30FF}' => kana = true,
            '\u{AC00}'..='\u{D7AF}' => hangul = true,
            _ => {}
        }
    }
    let exotic = greek as u32 + cyrillic as u32 + kana as u32 + hangul as u32 + other_exotic as u32;
    (bad as f32 / total) * 100.0 + exotic as f32 * 1.5
}

/// 展示类 REG_SZ 防御解码（R30）。
///
/// 候选：① UTF-16LE 直解；② 去 NUL 后按系统 ANSI 代码页；③ 同前按 GBK(936)。
/// 取可疑度最低者；最低仍超过阈值（损坏到不可辨识）时返回 None，
/// 由调用方决定降级展示（名称给固定占位，发布者/版本直接隐藏）。
/// **不得用于路径/命令字段**——那里错误"修复"反而会破坏真实值。
pub fn reg_data_decode_display(raw: &[u8]) -> Option<String> {
    let mut utf16 = reg_data_to_string(raw);
    utf16.retain(|c| c != '\0');
    let utf16 = utf16.trim().to_string();

    let mut best: Option<(f32, u8, String)> = None;
    let candidates = [
        Some(utf16).filter(|s| !s.is_empty()).map(|s| (0u8, s)),
        ansi_candidate(raw, CP_ACP).map(|s| (1u8, s)),
        ansi_candidate(raw, CP_GBK).map(|s| (2u8, s)),
    ];
    for cand in candidates.into_iter().flatten() {
        let score = display_score(&cand.1);
        let better = best
            .as_ref()
            .map(|(bs, bp, _)| score < *bs || (score == *bs && cand.0 < *bp))
            .unwrap_or(true);
        if better {
            best = Some((score, cand.0, cand.1));
        }
    }

    const SCORE_FLOOR: f32 = 8.0;
    best.filter(|(score, _, s)| *score < SCORE_FLOOR && !s.trim().is_empty())
        .map(|(_, _, s)| s)
}

/// 展示类字段策略（R30）：控制哪些字符形态合法。
#[derive(Clone, Copy)]
pub enum DisplayField {
    /// 程序名：允许正常 Unicode 文本，损坏时由调用方给占位名。
    Name,
    /// 发布者：允许正常 Unicode 文本，损坏时隐藏（None）。
    Publisher,
    /// DisplayVersion：实践中恒为 ASCII 可打印（含点/连字符），其余一律隐藏。
    Version,
    /// InstallDate：仅接受 YYYYMMDD 八位数字。
    Date,
}

/// 取展示类值并做防御解码与字段形态校验；不可信/不合法 → None。
pub fn reg_display_value(values: &[RegValue], name: &str, field: DisplayField) -> Option<String> {
    let raw = values.iter().find_map(|v| {
        if v.name.eq_ignore_ascii_case(name) && (v.kind == REG_SZ || v.kind == REG_EXPAND_SZ) {
            Some(&v.data)
        } else {
            None
        }
    })?;
    let s = reg_data_decode_display(raw)?;
    let ok = match field {
        DisplayField::Name | DisplayField::Publisher => true,
        DisplayField::Version => !s.is_empty() && s.chars().all(|c| (' '..='~').contains(&c)),
        DisplayField::Date => s.chars().count() == 8 && s.bytes().all(|b| b.is_ascii_digit()),
    };
    ok.then_some(s)
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
