//! 临时排障探针 v3：决定性地定位提权判定异常。
//! 1) 用多档缓冲区长度测试 TokenElevation(20)，打印每次的返回码/GetLastError/elev 值；
//! 2) 直接打印进程完整性级别 SID（S-1-16-12288=高/提权，S-1-16-8192=中/未提权）作为权威基准；
//! 3) 尝试 MftSession::try_open。
use std::ffi::c_void;
use std::path::PathBuf;

use pureslate_lib::scanner::mft::MftSession;

const TOKEN_QUERY: u32 = 0x0000_0008;
const TOKEN_ELEVATION: i32 = 20;
const TOKEN_INTEGRITY_LEVEL: i32 = 25;

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
    fn GetSidSubAuthority(sid: *const c_void, index: u32) -> *mut u32;
    fn GetSidSubAuthorityCount(sid: *const c_void) -> *mut u8;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

fn main() {
    let proc_handle = unsafe { GetCurrentProcess() };
    println!("PROC_HANDLE={proc_handle:p}");

    let mut token: *mut c_void = std::ptr::null_mut();
    // SAFETY: OpenProcessToken 取本进程 token。
    let open_ok = unsafe { OpenProcessToken(proc_handle, TOKEN_QUERY, &mut token) };
    println!("OPEN_TOKEN_OK={open_ok} token={token:p}");

    for len in [4u32, 8, 64] {
        let mut buf = vec![0u8; len as usize];
        let mut ret = 0u32;
        // SAFETY: GetTokenInformation(TokenElevation) 试探各缓冲区长度。
        let gi_ok = unsafe {
            GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                buf.as_mut_ptr() as *mut c_void,
                len,
                &mut ret,
            )
        };
        let gi_err = unsafe { GetLastError() };
        let elev = buf[0];
        println!("ELEV len={len} ok={gi_ok} err={gi_err} elev={elev} ret={ret}");
    }

    // 完整性级别（权威基准）。
    let il = integrity_level(token);
    println!("INTEGRITY={il}");

    // SAFETY: 关闭句柄。
    if !token.is_null() {
        unsafe {
            let _ = CloseHandle(token);
        }
    }

    let id = windows_identity_name();
    println!("WHOAMI={id}");

    // MFT 打开结果 + 解析诊断（v4：定位真实 USN 记录解析为何几乎无产出）。
    let root = PathBuf::from(r"C:\");
    let t0 = std::time::Instant::now();
    match MftSession::try_open(&root) {
        Ok(s) => {
            println!("MFT_OK open_ms={}", t0.elapsed().as_millis());
            let (total, dirs, files, samples) = s.diag(5);
            println!("DIAG total={total} dirs={dirs} files={files}");
            for e in &samples {
                println!(
                    "SAMPLE name={:?} is_dir={} len={} ref={:#x} parent={:#x}",
                    e.name, e.is_dir, e.file_len, e.reference, e.parent
                );
            }
            // temp 维度定位诊断：MFT 拼装路径 vs %TEMP% 展开路径的前缀命中差异。
            let temp_env = std::env::var("TEMP").unwrap_or_default();
            println!("TEMP_ENV={temp_env}");
            let (cs, ci, tsamples) = s.diag_target(&PathBuf::from(&temp_env), 3);
            println!("TEMP_HITS case_sensitive={cs} case_insensitive={ci}");
            for p in &tsamples {
                println!("TEMP_SAMPLE={p}");
            }
        }
        Err(e) => println!("MFT_ERR={e}"),
    }

    // 首批原始字节（hex，前 128）+ 前 3 批返回大小：判定记录布局与续批推进。
    match pureslate_lib::scanner::mft::debug_first_records(&root, 3) {
        Some((bytes, sizes)) => {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("FIRST128={}", hex.join(""));
            println!("BATCH_SIZES={sizes:?}");
        }
        None => println!("FIRST128=NONE BATCH_SIZES=NONE"),
    }
}

/// 读取进程令牌完整性级别 SID 的最后一个子权威值（12288=高/提权，8192=中/未提权）。
fn integrity_level(token: *mut c_void) -> u32 {
    let mut buf = [0u8; 64];
    let mut ret = 0u32;
    // SAFETY: GetTokenInformation(TokenIntegrityLevel)。
    let ok = unsafe {
        GetTokenInformation(
            token,
            TOKEN_INTEGRITY_LEVEL,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() as u32,
            &mut ret,
        )
    };
    if ok == 0 {
        let _e = unsafe { GetLastError() };
        return u32::MAX; // 失败哨兵，避免与真实值混淆
    }
    // buf 首 4 字节是 TOKEN_MANDATORY_LABEL.LabelSid（指针）。
    let sid_ptr = unsafe { std::ptr::read(buf.as_ptr() as *const *const c_void) };
    if sid_ptr.is_null() {
        return u32::MAX;
    }
    // SAFETY: 末尾子权威即完整性级别。
    let count = unsafe { *GetSidSubAuthorityCount(sid_ptr) };
    unsafe { *GetSidSubAuthority(sid_ptr, (count as u32).saturating_sub(1)) }
}

fn windows_identity_name() -> String {
    // 用 Win32 GetUserNameW 简单返回当前用户名（仅排障展示）。
    #[link(name = "advapi32")]
    extern "system" {
        fn GetUserNameW(name: *mut u16, len: *mut u32) -> i32;
    }
    let mut buf = [0u16; 128];
    let mut len = buf.len() as u32;
    // SAFETY: GetUserNameW 写入限定长度缓冲。
    let ok = unsafe { GetUserNameW(buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return format!(
            "getusername_failed({})",
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
        );
    }
    String::from_utf16_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}
