//! 临时排障探针：自包含测试 FSCTL_ENUM_USN_DATA 在不同卷打开方式下的结果。
//! 一次提权测多种访问掩码，定位哪位能成功枚举 MFT（解决 os error 5 权限问题）。
use std::ffi::c_void;

const INVALID_HANDLE: isize = -1;
const OPEN_EXISTING: u32 = 3;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const FILE_SHARE_DELETE: u32 = 4;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00B3;
const ERROR_HANDLE_EOF: i32 = 38;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_READ_DATA: u32 = 0x0001;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;

#[repr(C)]
struct MftEnumData {
    start_file_reference: u64,
    low_usn: i64,
    high_usn: i64,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        file: *const u16,
        desired_access: u32,
        share_mode: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: *const c_void,
    ) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
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
    fn GetLastError() -> u32;
}

fn open_and_enum(label: &str, access: u32, flags: u32, low_usn: i64) {
    let vol: Vec<u16> = r"\\.\C:".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: CreateFileW 打开卷句柄。
    let h = unsafe {
        CreateFileW(
            vol.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            flags,
            std::ptr::null(),
        )
    };
    let h_isize = h as isize;
    if h_isize == INVALID_HANDLE {
        let e = unsafe { GetLastError() };
        println!("[{label}] OPEN_FAIL err={e}");
        return;
    }
    let mut buf = vec![0u8; 256 * 1024];
    let mut start_ref: u64 = 0;
    let mut got = 0usize;
    let mut first_err: Option<u32> = None;
    loop {
        let mft_enum = MftEnumData {
            start_file_reference: start_ref,
            low_usn,
            high_usn: i64::MAX,
        };
        let mut returned = 0u32;
        // SAFETY: FSCTL_ENUM_USN_DATA。
        let ok = unsafe {
            DeviceIoControl(
                h,
                FSCTL_ENUM_USN_DATA,
                &mft_enum as *const _ as *const c_void,
                std::mem::size_of::<MftEnumData>() as u32,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut returned,
                std::ptr::null(),
            )
        } != 0;
        if !ok {
            let e = unsafe { GetLastError() };
            if e == ERROR_HANDLE_EOF as u32 {
                break;
            }
            first_err = Some(e);
            break;
        }
        if returned == 0 {
            break;
        }
        start_ref = start_ref.wrapping_add(1);
        got += returned as usize;
        // 只读到 1MB 就视为能枚举（证明访问掩码正确），不必枚举全卷。
        if got >= 1024 * 1024 {
            break;
        }
    }
    // SAFETY: 关闭句柄。
    unsafe {
        let _ = CloseHandle(h);
    }
    match first_err {
        Some(e) => println!("[{label}] ENUM_FAIL err={e} (open=ok, got={got}B)"),
        None => println!("[{label}] ENUM_OK got={got}B"),
    }
}

fn main() {
    open_and_enum("A", GENERIC_READ, FILE_FLAG_BACKUP_SEMANTICS, 0);
    open_and_enum("B", FILE_READ_DATA | FILE_READ_ATTRIBUTES, 0, 0);
    open_and_enum(
        "C",
        GENERIC_READ | GENERIC_WRITE,
        FILE_FLAG_BACKUP_SEMANTICS,
        0,
    );
    open_and_enum("D", GENERIC_READ, FILE_FLAG_BACKUP_SEMANTICS, i64::MIN);
    open_and_enum("E", GENERIC_READ | GENERIC_WRITE, 0, 0);
}
