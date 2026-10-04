//! minidump 只读解析（R24 · 上传前预览，SPEC §6.6「模块列表摘要」）。
//!
//! 只解析稳定的最小面：文件头/目录 → ModuleListStream(4) 模块名与数量 +
//! ExceptionStream(6) 异常代码。不引入 minidump crate（依赖准入红线）；
//! 结构布局依据 MSDN（MINIDUMP_HEADER / MINIDUMP_DIRECTORY / MINIDUMP_MODULE /
//! MINIDUMP_STRING / MINIDUMP_EXCEPTION_STREAM，dbgeng.h）。
//! 全部读取走边界检查 + checked 偏移，畸形文件返回 Err 而非 panic（红线 #6）。

/// 解析体积上限（防御畸形/超大文件；MiniDumpNormal 正常在几 MB 内）。
pub const MAX_PARSE_BYTES: u64 = 64 * 1024 * 1024;
/// 预览最多列出的模块名数（防超大模块表撑爆 UI/契约）。
pub const PREVIEW_MODULE_CAP: usize = 48;

const DIR_ENTRY_SIZE: usize = 12; // StreamType u32 + Location{DataSize u32, Rva u32}
const MODULE_SIZE: usize = 108;
const MODULE_NAME_RVA_OFF: usize = 20;
const STREAM_MODULE_LIST: u32 = 4;
const STREAM_EXCEPTION: u32 = 6;
const SIG_MDMP: u32 = 0x504D_444D; // 'MDMP' LE

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDumpSummary {
    pub module_count: u32,
    /// 模块基名（仅取文件名部分；上限 `PREVIEW_MODULE_CAP` 个）。
    pub modules: Vec<String>,
    /// 异常代码（无 ExceptionStream 的 panic 通道 dump 为 None）。
    pub exception_code: Option<u32>,
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    if end > b.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
    ]))
}

/// 解析 dump 字节流为预览摘要。
pub fn parse_summary(bytes: &[u8]) -> Result<CrashDumpSummary, String> {
    if u32_at(bytes, 0) != Some(SIG_MDMP) {
        return Err("不是有效的 minidump 文件（签名不符）".into());
    }
    let n_streams = u32_at(bytes, 8).ok_or("文件头截断")? as usize;
    let dir_rva = u32_at(bytes, 12).ok_or("文件头截断")? as usize;

    let mut modules = Vec::new();
    let mut module_count = 0u32;
    let mut exception_code = None;

    for i in 0..n_streams {
        let entry = dir_rva
            .checked_add(i.checked_mul(DIR_ENTRY_SIZE).ok_or("目录偏移溢出")?)
            .ok_or("目录偏移溢出")?;
        let stream_type = u32_at(bytes, entry).ok_or("目录截断")?;
        let rva =
            u32_at(bytes, entry.checked_add(8).ok_or("目录截断")?).ok_or("目录截断")? as usize;

        match stream_type {
            STREAM_MODULE_LIST => {
                module_count = u32_at(bytes, rva).ok_or("模块流截断")?;
                let base = rva.checked_add(4).ok_or("模块流偏移溢出")?;
                for k in 0..module_count as usize {
                    let m = base
                        .checked_add(k.checked_mul(MODULE_SIZE).ok_or("模块表偏移溢出")?)
                        .ok_or("模块表偏移溢出")?;
                    let name_rva = u32_at(
                        bytes,
                        m.checked_add(MODULE_NAME_RVA_OFF)
                            .ok_or("模块条目偏移溢出")?,
                    )
                    .ok_or("模块条目截断")? as usize;
                    if modules.len() < PREVIEW_MODULE_CAP {
                        if let Some(name) = read_minidump_string(bytes, name_rva) {
                            // 只取基名（完整路径无预览价值且更长）
                            let base = name.rsplit(['\\', '/']).next().unwrap_or(&name).to_string();
                            modules.push(base);
                        }
                    }
                }
            }
            // MINIDUMP_EXCEPTION_STREAM：ThreadId u32 / 对齐 u32 / ExceptionCode u32
            STREAM_EXCEPTION => {
                exception_code = Some(
                    u32_at(bytes, rva.checked_add(8).ok_or("异常流截断")?).ok_or("异常流截断")?,
                );
            }
            _ => {}
        }
    }

    Ok(CrashDumpSummary {
        module_count,
        modules,
        exception_code,
    })
}

/// 读 MINIDUMP_STRING（u32 字节长度 + UTF-16LE）。畸形返回 None（跳过该模块名）。
fn read_minidump_string(bytes: &[u8], rva: usize) -> Option<String> {
    let len = u32_at(bytes, rva)? as usize;
    let start = rva.checked_add(4)?;
    let end = start.checked_add(len)?;
    if !len.is_multiple_of(2) || end > bytes.len() {
        return None;
    }
    let units: Vec<u16> = bytes[start..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一份最小合法 minidump：头 + 目录（模块流+异常流）+ 模块表 + 名字串。
    fn synthetic_dump(module_names: &[&str], with_exception: bool) -> Vec<u8> {
        // 布局：header(32) | dir entries | strings | module table | exception stream
        let n_streams = if with_exception { 2 } else { 1 };
        let dir_at = 32usize;
        let dir_len = n_streams * DIR_ENTRY_SIZE;
        let mut strings = Vec::new();
        let mut string_rvas = Vec::new();
        let mut cursor = dir_at + dir_len;
        for name in module_names {
            string_rvas.push(cursor as u32);
            let units: Vec<u16> = name.encode_utf16().collect();
            strings.extend_from_slice(&(units.len() as u32 * 2).to_le_bytes());
            for u in &units {
                strings.extend_from_slice(&u.to_le_bytes());
            }
            cursor += 4 + units.len() * 2;
        }
        let module_table_at = cursor;
        // NumberOfModules + N*108（每模块只填 BaseOfImage=0 与 NameRva，其余置零）
        let mut table = Vec::new();
        table.extend_from_slice(&(module_names.len() as u32).to_le_bytes());
        for rva in &string_rvas {
            let mut m = [0u8; MODULE_SIZE];
            m[MODULE_NAME_RVA_OFF..MODULE_NAME_RVA_OFF + 4].copy_from_slice(&rva.to_le_bytes());
            table.extend_from_slice(&m);
        }
        cursor += table.len();
        let exception_at = cursor;
        let exception = if with_exception {
            let mut s = Vec::new();
            s.extend_from_slice(&7u32.to_le_bytes()); // ThreadId
            s.extend_from_slice(&0u32.to_le_bytes()); // alignment
            s.extend_from_slice(&0xC000_0005u32.to_le_bytes()); // ExceptionCode
            s
        } else {
            Vec::new()
        };

        let total = cursor + exception.len();
        let mut out = vec![0u8; total];
        // header
        out[0..4].copy_from_slice(&SIG_MDMP.to_le_bytes());
        out[4..8].copy_from_slice(&979017u32.to_le_bytes()); // version（任意）
        out[8..12].copy_from_slice(&(n_streams as u32).to_le_bytes());
        out[12..16].copy_from_slice(&(dir_at as u32).to_le_bytes());
        // strings
        out[dir_at + dir_len..dir_at + dir_len + strings.len()].copy_from_slice(&strings);
        // module table
        let mt_off = module_table_at;
        out[mt_off..mt_off + table.len()].copy_from_slice(&table);
        // directory
        let mut d = Vec::new();
        d.extend_from_slice(&STREAM_MODULE_LIST.to_le_bytes());
        d.extend_from_slice(&(table.len() as u32).to_le_bytes());
        d.extend_from_slice(&(module_table_at as u32).to_le_bytes());
        if with_exception {
            d.extend_from_slice(&STREAM_EXCEPTION.to_le_bytes());
            d.extend_from_slice(&(exception.len() as u32).to_le_bytes());
            d.extend_from_slice(&(exception_at as u32).to_le_bytes());
        }
        out[dir_at..dir_at + d.len()].copy_from_slice(&d);
        // exception stream
        if with_exception {
            let e_off = exception_at;
            out[e_off..e_off + exception.len()].copy_from_slice(&exception);
        }
        out
    }

    #[test]
    fn parses_modules_and_exception() {
        let bytes = synthetic_dump(
            &[r"C:\Windows\System32\ntdll.dll", r"E:\app\pureslate.exe"],
            true,
        );
        let s = parse_summary(&bytes).unwrap();
        assert_eq!(s.module_count, 2);
        assert_eq!(s.modules, vec!["ntdll.dll", "pureslate.exe"]);
        assert_eq!(s.exception_code, Some(0xC000_0005));
    }

    #[test]
    fn panic_dump_without_exception_stream() {
        let bytes = synthetic_dump(&["kernel32.dll"], false);
        let s = parse_summary(&bytes).unwrap();
        assert_eq!(s.module_count, 1);
        assert_eq!(s.exception_code, None);
    }

    #[test]
    fn rejects_bad_signature_and_truncated() {
        assert!(parse_summary(b"notadump").is_err());
        let bytes = synthetic_dump(&["a.dll"], true);
        assert!(parse_summary(&bytes[..20]).is_err()); // 截断
    }

    #[test]
    fn module_cap_applies_but_count_keeps_full() {
        let owned: Vec<String> = (0..60).map(|i| format!("m{i}.dll")).collect();
        let names: Vec<&str> = owned.iter().map(String::as_str).collect();
        let bytes = synthetic_dump(&names, false);
        let s = parse_summary(&bytes).unwrap();
        assert_eq!(s.module_count, 60);
        assert_eq!(s.modules.len(), PREVIEW_MODULE_CAP);
    }
}
