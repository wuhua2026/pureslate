//! 已安装程序图标提取（R30）。
//!
//! 只读：读取注册表 DisplayIcon → `PrivateExtractIconsW` 取 HICON → GetDIBits
//! 转 32bpp BGRA → 手写 PNG（stored deflate + CRC32/Adler32）→ base64 data URL。
//! 零第三方依赖（不引 image/png 等 crate，AGENTS §7 体积准入）。
//! 失败一律返回 None，前端回退默认字形图标——图标永远不影响卸载功能可用性。

use std::ffi::c_void;
use std::path::Path;

use crate::startup::winreg;

type Hicon = *mut c_void;
type Hbitmap = *mut c_void;
type Hdc = *mut c_void;

const BI_RGB: u32 = 0;
const DIB_RGB_COLORS: u32 = 0;

#[repr(C)]
struct IconInfo {
    f_icon: i32,
    x_hotspot: u32,
    y_hotspot: u32,
    hbm_mask: Hbitmap,
    hbm_color: Hbitmap,
}

#[repr(C)]
struct Bitmap {
    bm_type: i32,
    bm_width: i32,
    bm_height: i32,
    bm_width_bytes: i32,
    bm_planes: u16,
    bm_bits_pixel: u16,
    bm_bits: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels: i32,
    bi_y_pels: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RgbQuad {
    rgb_blue: u8,
    rgb_green: u8,
    rgb_red: u8,
    rgb_reserved: u8,
}

#[repr(C)]
struct BitmapInfo1 {
    header: BitmapInfoHeader,
    colors: [RgbQuad; 2],
}

#[link(name = "user32")]
extern "system" {
    fn PrivateExtractIconsW(
        file: *const u16,
        icon_index: i32,
        cx: i32,
        cy: i32,
        hicon_out: *mut Hicon,
        icon_id_out: *mut u32,
        n_icons: u32,
        flags: u32,
    ) -> u32;
    fn GetIconInfo(hicon: Hicon, info: *mut IconInfo) -> i32;
    fn DestroyIcon(hicon: Hicon) -> i32;
    fn GetDC(hwnd: *mut c_void) -> Hdc;
    fn ReleaseDC(hwnd: *mut c_void, hdc: Hdc) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetObjectW(obj: *mut c_void, c: i32, pv: *mut c_void) -> i32;
    fn GetDIBits(
        hdc: Hdc,
        bmp: Hbitmap,
        start: u32,
        lines: u32,
        bits: *mut u8,
        info: *mut BitmapInfoHeader,
        usage: u32,
    ) -> i32;
    fn DeleteObject(obj: *mut c_void) -> i32;
}

struct GdiBitmap {
    handle: Hbitmap,
}
impl Drop for GdiBitmap {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { DeleteObject(self.handle) };
        }
    }
}

/// 解析 DisplayIcon 注册表值（纯函数）：`路径[,索引]`（索引可负=资源序数）。
/// 无逗号/尾段非整数时按索引 0 处理；容忍成对引号与多余空白。
pub fn parse_display_icon(input: &str) -> (String, i32) {
    let s = input.trim();
    let s = s
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(s)
        .trim();
    if let Some(idx) = s.rfind(',') {
        let (path, tail) = s.split_at(idx);
        if let Ok(n) = tail[1..].trim().parse::<i32>() {
            let path = path.trim().trim_matches('"').trim();
            if !path.is_empty() {
                return (path.to_string(), n);
            }
        }
    }
    (s.to_string(), 0)
}

/// 图标提取总入口：DisplayIcon 优先；缺失/失败时回退到卸载命令的 exe（索引 0）。
pub fn data_url(display_icon: Option<&str>, fallback_command: Option<&str>) -> Option<String> {
    let mut candidates: Vec<(String, i32)> = Vec::new();
    if let Some(v) = display_icon {
        let (p, i) = parse_display_icon(v);
        if !p.is_empty() {
            candidates.push((p, i));
        }
    }
    if let Some(cmd) = fallback_command {
        if let Some((exe, _)) = crate::uninstall::runner::split_exe_args(cmd) {
            candidates.push((exe, 0));
        }
    }
    for (raw_path, index) in candidates {
        // 路径可能含 %ProgramFiles% 等环境变量；展开失败就用原值（交给 API 判定）。
        let path = winreg::expand_environment(&raw_path).unwrap_or(raw_path);
        if !Path::new(&path).is_file() {
            continue;
        }
        if let Some(url) = extract_file_icon(&path, index) {
            return Some(url);
        }
    }
    None
}

/// 从文件提取图标：请求索引失败 → 回退索引 0（部分程序只注册默认资源）。
fn extract_file_icon(path: &str, index: i32) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let attempts = if index == 0 { vec![0] } else { vec![index, 0] };
    for idx in attempts {
        let mut hicon: Hicon = std::ptr::null_mut();
        let mut icon_id: u32 = 0;
        let n = unsafe {
            PrivateExtractIconsW(
                wide.as_ptr(),
                idx,
                ICON_SIZE,
                ICON_SIZE,
                &mut hicon,
                &mut icon_id,
                1,
                0,
            )
        };
        if n == 0 || hicon.is_null() {
            continue;
        }
        let png = icon_to_png(hicon);
        unsafe { DestroyIcon(hicon) };
        if let Some(bytes) = png {
            return Some(format!("data:image/png;base64,{}", base64_encode(&bytes)));
        }
    }
    None
}

/// 提取边长（32px：22 CSS px 行内 @1.5 DPI 仍清晰；体积与清晰度折中）。
const ICON_SIZE: i32 = 32;

/// HICON → RGBA 像素 → PNG。任何 GDI 步骤失败返回 None。
fn icon_to_png(hicon: Hicon) -> Option<Vec<u8>> {
    let mut info = IconInfo {
        f_icon: 0,
        x_hotspot: 0,
        y_hotspot: 0,
        hbm_mask: std::ptr::null_mut(),
        hbm_color: std::ptr::null_mut(),
    };
    if unsafe { GetIconInfo(hicon, &mut info) } == 0 {
        return None;
    }
    // RAII：函数退出（含提前返回）时释放两个位图；掩码读取必须在 drop 之前。
    let _mask_guard = GdiBitmap {
        handle: info.hbm_mask,
    };
    let _color_guard = GdiBitmap {
        handle: info.hbm_color,
    };
    if info.hbm_color.is_null() {
        return None;
    }

    let mut bm = Bitmap {
        bm_type: 0,
        bm_width: 0,
        bm_height: 0,
        bm_width_bytes: 0,
        bm_planes: 0,
        bm_bits_pixel: 0,
        bm_bits: std::ptr::null_mut(),
    };
    let got = unsafe {
        GetObjectW(
            info.hbm_color,
            std::mem::size_of::<Bitmap>() as i32,
            &mut bm as *mut Bitmap as *mut c_void,
        )
    };
    if got == 0 || bm.bm_width <= 0 || bm.bm_height <= 0 || bm.bm_width > 512 || bm.bm_height > 512
    {
        return None;
    }
    let (w, h) = (bm.bm_width as usize, bm.bm_height as usize);
    let stride = w.checked_mul(4)?;
    let mut bgra = vec![0u8; stride.checked_mul(h)?];

    let hdc = unsafe { GetDC(std::ptr::null_mut()) };
    if hdc.is_null() {
        return None;
    }
    let mut bi = BitmapInfoHeader {
        bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
        bi_width: bm.bm_width,
        bi_height: -(bm.bm_height), // 负值=top-down，首行即图标顶行
        bi_planes: 1,
        bi_bit_count: 32,
        bi_compression: BI_RGB,
        bi_size_image: 0,
        bi_x_pels: 0,
        bi_y_pels: 0,
        bi_clr_used: 0,
        bi_clr_important: 0,
    };
    let scanned = unsafe {
        GetDIBits(
            hdc,
            info.hbm_color,
            0,
            h as u32,
            bgra.as_mut_ptr(),
            &mut bi,
            DIB_RGB_COLORS,
        )
    };
    // 32bpp 源：直接信 alpha；低位深源：GetDIBits 的 32bpp 转换不携带 alpha，
    // 用 AND 掩码补不透明度（掩码位 1=透明）。须在位图 GDI 对象释放前读取。
    let has_alpha = bm.bm_bits_pixel >= 32;
    let mask_bits = if scanned != 0 && !has_alpha {
        read_and_mask(hdc, info.hbm_mask, w, h)
    } else {
        None
    };
    unsafe { ReleaseDC(std::ptr::null_mut(), hdc) };
    if scanned == 0 {
        return None;
    }

    let mut rgba = Vec::with_capacity(stride * h);
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * 4;
            if i + 3 >= bgra.len() {
                return None;
            }
            let (b, g, r, a) = (bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]);
            let alpha = if has_alpha {
                a
            } else if mask_is_transparent(mask_bits.as_deref(), w, x, y) {
                0
            } else {
                255
            };
            rgba.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    Some(png_encode(w, h, &rgba))
}

/// 读取单色 AND 掩码为 1bpp top-down 字节流；失败 None。
fn read_and_mask(hdc: Hdc, hbm: Hbitmap, w: usize, h: usize) -> Option<Vec<u8>> {
    if hbm.is_null() {
        return None;
    }
    let row_bytes = w.div_ceil(8);
    let row_padded = row_bytes.div_ceil(4) * 4;
    let mut bits = vec![0u8; row_padded.checked_mul(h)?];
    let mut bi = BitmapInfo1 {
        header: BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: w as i32,
            bi_height: -(h as i32),
            bi_planes: 1,
            bi_bit_count: 1,
            bi_compression: BI_RGB,
            bi_size_image: 0,
            bi_x_pels: 0,
            bi_y_pels: 0,
            bi_clr_used: 2,
            bi_clr_important: 0,
        },
        // AND 掩码：0=不透明(黑)，1=透明(白)。
        colors: [
            RgbQuad {
                rgb_blue: 0,
                rgb_green: 0,
                rgb_red: 0,
                rgb_reserved: 0,
            },
            RgbQuad {
                rgb_blue: 255,
                rgb_green: 255,
                rgb_red: 255,
                rgb_reserved: 0,
            },
        ],
    };
    let scanned = unsafe {
        GetDIBits(
            hdc,
            hbm,
            0,
            h as u32,
            bits.as_mut_ptr(),
            &mut bi.header as *mut BitmapInfoHeader,
            DIB_RGB_COLORS,
        )
    };
    (scanned != 0).then_some(bits)
}

/// 掩码位为 1（透明）？读不到掩码按不透明处理。
fn mask_is_transparent(bits: Option<&[u8]>, w: usize, x: usize, y: usize) -> bool {
    let Some(bits) = bits else { return false };
    let row_padded = w.div_ceil(8).div_ceil(4) * 4;
    let Some(row) = bits.get(y * row_padded..) else {
        return false;
    };
    match row.get(x / 8) {
        Some(byte) => (byte & (0x80 >> (x % 8))) != 0,
        None => false,
    }
}

// ===== 手写 PNG（32bpp RGBA，stored deflate，零压缩依赖） =====

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut check_input = Vec::with_capacity(4 + data.len());
    check_input.extend_from_slice(kind);
    check_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&check_input).to_be_bytes());
}

/// RGBA（top-down，逐行连续）→ PNG 字节。zlib 流用 stored 块（不压缩）。
fn png_encode(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    // 原始扫描线：每行前缀 filter=0。
    let row_bytes = w * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * row_bytes..(y + 1) * row_bytes]);
    }

    // zlib 包装：CMF/FLG + stored deflate 块（每块 ≤65535）+ Adler32。
    let mut zlib = Vec::with_capacity(raw.len() + 16);
    zlib.push(0x78);
    zlib.push(0x01);
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        zlib.push(if last { 0x01 } else { 0x00 });
        zlib.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::new();
    png.extend_from_slice(&PNG_SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &zlib);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

// ===== base64（标准字母表 + padding） =====

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_TABLE[((triple >> 18) & 63) as usize] as char);
        out.push(B64_TABLE[((triple >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_TABLE[((triple >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_TABLE[(triple & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_display_icon_cases() {
        assert_eq!(
            parse_display_icon(r"C:\App\app.exe,0"),
            (r"C:\App\app.exe".into(), 0)
        );
        assert_eq!(
            parse_display_icon(r#""C:\Program Files\A\a.dll",-15"#),
            (r"C:\Program Files\A\a.dll".into(), -15)
        );
        assert_eq!(
            parse_display_icon(r"D:\icons\app.ico"),
            (r"D:\icons\app.ico".into(), 0)
        );
        // 尾段不是整数 → 整串当路径，索引 0。
        assert_eq!(
            parse_display_icon(r"C:\foo, bar\app.ico"),
            (r"C:\foo, bar\app.ico".into(), 0)
        );
        // 空白容忍。
        assert_eq!(
            parse_display_icon("  C:\\A\\a.exe , 3 "),
            ("C:\\A\\a.exe".into(), 3)
        );
    }

    #[test]
    fn checksum_known_vectors() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn base64_roundtrip() {
        // 三种长度余数（手工解码验证，避免引入 base64 crate）。
        for (input, expect) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(input.as_bytes()), expect);
        }
        // 任意长度解码还原。
        for len in 1..64 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37) as u8).collect();
            assert_eq!(b64_decode_naive(&base64_encode(&data))[..len], data[..]);
        }
    }

    /// 测试专用：最小标准 base64 解码（仅校验编码器输出用）。
    fn b64_decode_naive(s: &str) -> Vec<u8> {
        let val = |c: u8| -> u8 {
            match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => 0,
            }
        };
        let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
        let mut out = Vec::new();
        for ch in bytes.chunks(4) {
            let mut buf = [0u32; 4];
            for (i, &c) in ch.iter().enumerate() {
                buf[i] = val(c) as u32;
            }
            let triple = (buf[0] << 18) | (buf[1] << 12) | (buf[2] << 6) | buf[3];
            out.push((triple >> 16) as u8);
            if ch.len() > 2 {
                out.push((triple >> 8) as u8);
            }
            if ch.len() > 3 {
                out.push(triple as u8);
            }
        }
        out
    }

    /// 校验 PNG 结构：签名/IHDR 尺寸/色型、各 chunk CRC、IDAT stored 块
    /// 重组后等于"过滤前缀扫描线"且 Adler32 匹配（不依赖外部 inflate）。
    #[test]
    fn png_structure_and_payload() {
        let rgba = [255u8, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 9, 9, 9, 255];
        let png = png_encode(2, 2, &rgba);
        assert_eq!(&png[..8], &PNG_SIGNATURE);

        let mut pos = 8;
        let mut ihdr = None;
        let mut idat = Vec::new();
        let mut saw_iend = false;
        while pos + 8 <= png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let kind = &png[pos + 4..pos + 8];
            let data = &png[pos + 8..pos + 8 + len];
            let crc_stored =
                u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            let mut input = kind.to_vec();
            input.extend_from_slice(data);
            assert_eq!(crc32(&input), crc_stored, "chunk {:?} CRC 不匹配", kind);
            match kind {
                b"IHDR" => ihdr = Some(data.to_vec()),
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => {
                    assert_eq!(len, 0);
                    saw_iend = true;
                }
                _ => {}
            }
            pos += 12 + len;
        }
        assert!(saw_iend);
        let ihdr = ihdr.expect("IHDR");
        assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), 2);
        assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), 2);
        assert_eq!(ihdr[8], 8);
        assert_eq!(ihdr[9], 6);

        // zlib 头 + 尾校验，stored 块重组。
        assert_eq!(&idat[..2], &[0x78, 0x01]);
        let mut p = 2;
        let mut raw = Vec::new();
        loop {
            let bfinal = idat[p];
            assert!(bfinal <= 1, "BTYPE 必须为 stored(00)");
            let len = u16::from_le_bytes(idat[p + 1..p + 3].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes(idat[p + 3..p + 5].try_into().unwrap());
            assert_eq!(nlen, !(len as u16));
            raw.extend_from_slice(&idat[p + 5..p + 5 + len]);
            p += 5 + len;
            if bfinal == 1 {
                break;
            }
        }
        let adler_stored = u32::from_be_bytes(idat[p..p + 4].try_into().unwrap());
        assert_eq!(adler_stored, adler32(&raw));
        // 2×2：filter 0 + 8 像素字节/行 = 9 字节/行；两段像素 == 两行原始 RGBA。
        assert_eq!(raw.len(), 2 * 9);
        assert_eq!(&raw[1..9], &rgba[..8]);
        assert_eq!(&raw[10..18], &rgba[8..16]);
    }

    /// 人工视觉验证（`--ignored` 显式触发）：抽取本机前若干程序图标落盘，
    /// 供浏览器/看图工具核对透明通道与内容。不进常规门禁。
    #[test]
    #[ignore]
    fn dump_real_app_icons_for_visual_check() {
        let dir = std::env::temp_dir().join("pureslate-icon-check");
        let _ = std::fs::create_dir_all(&dir);
        let located = crate::uninstall::enumerate_located_test();
        let mut dumped = 0;
        for loc in located.iter().take(12) {
            let (hive, full_key, wow64, uninstall_command, name) =
                (loc.0, &loc.1, loc.2, &loc.3, &loc.4);
            let display = crate::startup::winreg::enum_values_view(hive, full_key, wow64)
                .ok()
                .and_then(|values| {
                    values.into_iter().find(|v| {
                        v.name.eq_ignore_ascii_case("DisplayIcon")
                            && (v.kind == crate::startup::winreg::REG_SZ
                                || v.kind == crate::startup::winreg::REG_EXPAND_SZ)
                    })
                })
                .map(|v| crate::startup::winreg::reg_data_to_string(&v.data));
            if let Some(url) = data_url(display.as_deref(), Some(uninstall_command)) {
                let prefix = "data:image/png;base64,";
                if let Some(b64) = url.strip_prefix(prefix) {
                    let bytes = decode_b64_for_dump(b64);
                    let safe: String = name
                        .chars()
                        .map(|c| if "\\/:*?\"<>| ".contains(c) { '_' } else { c })
                        .take(40)
                        .collect();
                    let path = dir.join(format!("{dumped:02}-{safe}.png"));
                    if std::fs::write(&path, &bytes).is_ok() {
                        eprintln!("{} ← {}", path.display(), name);
                        dumped += 1;
                    }
                }
            }
        }
        eprintln!("dumped {dumped} icons to {}", dir.display());
    }

    /// dump 测试专用 base64 解码（不参与生产代码）。
    fn decode_b64_for_dump(s: &str) -> Vec<u8> {
        let val = |c: u8| -> u8 {
            match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => 0,
            }
        };
        s.bytes()
            .filter(|&c| c != b'\n' && c != b'\r')
            .collect::<Vec<_>>()
            .chunks(4)
            .flat_map(|ch| {
                let n = ch.iter().take_while(|&&c| c != b'=').count();
                let mut buf = [0u32; 4];
                for (i, &c) in ch.iter().enumerate() {
                    if c != b'=' {
                        buf[i] = val(c) as u32;
                    }
                }
                let triple = (buf[0] << 18) | (buf[1] << 12) | (buf[2] << 6) | buf[3];
                match n {
                    // n=4 无 padding（常态）；3=1 padding→2 字节；2=2 padding→1 字节。
                    4 => vec![(triple >> 16) as u8, (triple >> 8) as u8, triple as u8],
                    3 => vec![(triple >> 16) as u8, (triple >> 8) as u8],
                    _ => vec![(triple >> 16) as u8],
                }
            })
            .collect()
    }

    /// 真机冒烟（CI 可宽容）：从测试可执行文件提取图标；无资源时允许 None。
    #[test]
    fn smoke_icon_from_current_exe() {
        let exe = std::env::current_exe().expect("current_exe");
        let result = extract_file_icon(&exe.to_string_lossy(), 0);
        match result {
            Some(url) => {
                assert!(url.starts_with("data:image/png;base64,"));
                let b64 = url.trim_start_matches("data:image/png;base64,");
                assert!(b64.len() > 64);
            }
            None => eprintln!("current_exe 无可用图标（CI/无资源环境允许）"),
        }
    }
}
