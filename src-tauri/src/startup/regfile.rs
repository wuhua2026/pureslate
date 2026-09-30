//! `.reg` 备份文件生成与解析（R07）。
//!
//! 只写/只读本模块自己生成的单一值子集，但同时保持 regedit v5 可导入格式
//! （UTF-16LE + BOM）——用户即使脱离 PureSlate 也能双击导入恢复。

use std::fs;
use std::io;
use std::path::Path;

use super::winreg::{reg_data_to_string, string_to_reg_data};

/// 一条被备份的注册表值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegBackupValue {
    /// 完整键路径，如 `HKEY_CURRENT_USER\Software\...\Run`。
    pub key_path: String,
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}

/// 写出 `.reg`（UTF-16LE + BOM，regedit v5 格式）。
pub fn write_reg_file(path: &Path, v: &RegBackupValue) -> io::Result<()> {
    let value_line = match v.kind {
        1 => format!(
            "\"{}\"=\"{}\"",
            escape(&v.name),
            escape(&reg_data_to_string(&v.data))
        ),
        2 => format!("\"{}\"=hex(2):{}", escape(&v.name), hex_bytes(&v.data)),
        4 => format!("\"{}\"=dword:{:08x}", escape(&v.name), le_u32(&v.data)),
        _ => format!("\"{}\"=hex:{}", escape(&v.name), hex_bytes(&v.data)),
    };
    let text = format!(
        "Windows Registry Editor Version 5.00\r\n\r\n[{}]\r\n{}\r\n",
        v.key_path, value_line
    );
    let mut bytes: Vec<u8> = vec![0xFF, 0xFE]; // UTF-16LE BOM
    for u in text.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    fs::write(path, bytes)
}

/// 解析本模块生成的 `.reg`（单键单值子集）。格式不符 → Err。
pub fn parse_reg_file(path: &Path) -> io::Result<RegBackupValue> {
    let raw = fs::read(path)?;
    if raw.len() < 2 || raw[0] != 0xFF || raw[1] != 0xFE {
        return Err(invalid("缺少 UTF-16LE BOM"));
    }
    let units: Vec<u16> = raw[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let text = String::from_utf16_lossy(&units);

    let mut key_path: Option<String> = None;
    let mut value: Option<RegBackupValue> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
            key_path = Some(line[1..line.len() - 1].to_string());
        } else if line.starts_with('"') {
            value = parse_value_line(line);
        }
    }
    match (key_path, value) {
        (Some(key_path), Some(mut v)) => {
            v.key_path = key_path;
            Ok(v)
        }
        _ => Err(invalid("缺少键路径或值行")),
    }
}

/// 解析 `"name"=data` 行。
fn parse_value_line(line: &str) -> Option<RegBackupValue> {
    // 名字段：到未转义的闭引号为止。
    let mut name_end = None;
    let bytes = line.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            name_end = Some(i);
            break;
        }
        i += 1;
    }
    let name_end = name_end?;
    let name = unescape(&line[1..name_end]);
    let rest = line.get(name_end + 1..)?.strip_prefix('=')?;

    let (kind, data) = if let Some(s) = rest.strip_prefix("hex(2):") {
        (2, parse_hex(s)?)
    } else if let Some(s) = rest.strip_prefix("hex:") {
        (3, parse_hex(s)?)
    } else if let Some(s) = rest.strip_prefix("dword:") {
        let n = u32::from_str_radix(s.trim(), 16).ok()?;
        (4, n.to_le_bytes().to_vec())
    } else if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
        (1, string_to_reg_data(&unescape(&rest[1..rest.len() - 1])))
    } else {
        return None;
    };
    Some(RegBackupValue {
        key_path: String::new(),
        name,
        kind,
        data,
    })
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn hex_bytes(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    s.split(',')
        .filter(|p| !p.trim().is_empty())
        .map(|p| u8::from_str_radix(p.trim(), 16).ok())
        .collect()
}

fn le_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_file_roundtrip_sz_with_escapes() {
        let dir = std::env::temp_dir().join(format!(
            "pureslate-regfile-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.reg");
        let v = RegBackupValue {
            key_path: "HKEY_CURRENT_USER\\Software\\PureSlateTest\\Run".into(),
            name: "My\"App".into(),
            kind: 1,
            data: string_to_reg_data("C:\\Program Files\\app.exe\" -bg"),
        };
        write_reg_file(&p, &v).unwrap();
        let back = parse_reg_file(&p).unwrap();
        assert_eq!(back, v);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reg_file_roundtrip_expand_sz() {
        let dir = std::env::temp_dir().join(format!(
            "pureslate-regfile2-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t2.reg");
        let v = RegBackupValue {
            key_path:
                "HKEY_LOCAL_MACHINE\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run"
                    .into(),
            name: "Updater".into(),
            kind: 2,
            data: string_to_reg_data("%ProgramFiles(x86)%\\upd\\update.exe /run"),
        };
        write_reg_file(&p, &v).unwrap();
        let back = parse_reg_file(&p).unwrap();
        assert_eq!(back, v);
        let _ = fs::remove_dir_all(&dir);
    }
}
