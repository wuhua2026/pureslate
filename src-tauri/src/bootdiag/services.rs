//! 自动服务来源（只读）：SCM 枚举 + `QueryServiceConfig` / `QueryServiceConfig2`。
//!
//! 只读纪律：`OpenSCManagerW(SC_MANAGER_ENUMERATE_SERVICE)` + `OpenServiceW(SERVICE_QUERY_CONFIG)`，
//! 不含 SERVICE_START/STOP/CHANGE_CONFIG 位；不调用 `StartService` / `ChangeServiceConfig`。
//!
//! 边界：标准用户可枚举服务；个别受保护服务的 `QueryServiceConfig` 可能被拒，
//! 计入失败数而不是静默丢弃（与既有 `tasks.rs` 的缺陷相反）。

use std::ffi::c_void;
use std::io;

use crate::contract::{BootStartupEntry, BootStartupKind, BootStartupScope};

use super::startup_sources::entry_id;

type ScHandle = isize;

const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
const SERVICE_WIN32: u32 = 0x30;
const SERVICE_STATE_ALL: u32 = 0x3;
const SERVICE_QUERY_CONFIG: u32 = 0x1;
const SC_ENUM_PROCESS_INFO: u32 = 0;
const SERVICE_AUTO_START: u32 = 0x2;
const SERVICE_CONFIG_DELAYED_AUTO_START_INFO: u32 = 3;

const ERROR_MORE_DATA: i32 = 234;
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const ERROR_NO_MORE_ITEMS: i32 = 259;

#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(machine: *const u16, database: *const u16, access: u32) -> ScHandle;
    fn EnumServicesStatusExW(
        sc_manager: ScHandle,
        info_level: u32,
        service_type: u32,
        service_state: u32,
        services: *mut u8,
        buf_size: u32,
        bytes_needed: *mut u32,
        services_returned: *mut u32,
        resume_handle: *mut u32,
        group_name: *const u16,
    ) -> i32;
    fn OpenServiceW(sc_manager: ScHandle, name: *const u16, access: u32) -> ScHandle;
    fn QueryServiceConfigW(
        service: ScHandle,
        config: *mut c_void,
        buf_size: u32,
        bytes_needed: *mut u32,
    ) -> i32;
    fn QueryServiceConfig2W(
        service: ScHandle,
        info_level: u32,
        buffer: *mut u8,
        buf_size: u32,
        bytes_needed: *mut u32,
    ) -> i32;
    fn CloseServiceHandle(handle: ScHandle) -> i32;
}

struct Handle(ScHandle);

impl Handle {
    fn new(h: ScHandle) -> Option<Self> {
        (h != 0).then_some(Self(h))
    }
    fn raw(&self) -> ScHandle {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: 句柄由 SCM 返回且非 0，只关闭一次。
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}

#[repr(C)]
struct ServiceStatusProcess {
    service_type: u32,
    current_state: u32,
    controls_accepted: u32,
    win32_exit_code: u32,
    service_specific_exit_code: u32,
    checkpoint: u32,
    wait_hint: u32,
    process_id: u32,
    service_flags: u32,
}

#[repr(C)]
struct EnumServiceStatusProcessW {
    service_name: *mut u16,
    display_name: *mut u16,
    status: ServiceStatusProcess,
}

#[repr(C)]
struct QueryServiceConfigW {
    service_type: u32,
    start_type: u32,
    error_control: u32,
    binary_path: *mut u16,
    load_order_group: *mut u16,
    tag_id: u32,
    dependencies: *mut u16,
    service_start_name: *mut u16,
    display_name: *mut u16,
}

#[repr(C)]
struct ServiceDelayedAutoStartInfo {
    delayed: i32,
}

fn last_error() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 从 SCM 缓冲区里的宽字符指针读字符串（上限 1024 字符，防坏指针死循环）。
/// 参数必须指向 SCM 返回缓冲区内的 NUL 结尾宽字符串（私有约定）。
fn read_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    // SAFETY: 由调用方保证指针有效；上限兜底。
    while len < 1024 && unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: 前 len 个 u16 已验证可读。
    let slice = unsafe { std::slice::from_raw_parts(p, len) };
    String::from_utf16_lossy(slice)
}

/// 仅自动启动服务进入开机启动面（纯函数，可单测）。
pub(crate) fn is_auto_start(start_type: u32) -> bool {
    start_type == SERVICE_AUTO_START
}

/// 组装服务条目（纯函数）。
pub(crate) fn service_entry(
    service_name: &str,
    display_name: &str,
    binary_path: &str,
    delayed: Option<bool>,
) -> BootStartupEntry {
    let display = if display_name.trim().is_empty() {
        service_name
    } else {
        display_name
    };
    BootStartupEntry {
        id: entry_id(
            BootStartupKind::ServiceAuto,
            BootStartupScope::Machine,
            service_name,
            binary_path,
        ),
        name: display.to_string(),
        command: binary_path.to_string(),
        kind: BootStartupKind::ServiceAuto,
        scope: BootStartupScope::Machine,
        enabled: Some(true),
        approved_state: None,
        delayed_auto_start: delayed,
        detail: Some(service_name.to_string()),
    }
}

/// 枚举自动服务；返回条目与失败子源（失败数聚合为一条）。
pub fn collect() -> (Vec<BootStartupEntry>, Vec<String>) {
    let mut out = Vec::new();
    let mut failures = 0u32;

    let Some(scm) = Handle::new(unsafe {
        // SAFETY: NULL 表示本机；只申请枚举权限。
        OpenSCManagerW(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ENUMERATE_SERVICE,
        )
    }) else {
        let code = last_error();
        return (out, vec![format!("services_scm_code_{code}")]);
    };

    // EnumServicesStatusExW 的缓冲区按字节计；用 u64 分配保证 8 字节对齐。
    let mut buf: Vec<u64> = vec![0; 16 * 1024];
    let mut resume = 0u32;
    let mut names: Vec<String> = Vec::new();
    let mut enum_rounds = 0u32;

    loop {
        enum_rounds += 1;
        if enum_rounds > 1_000 {
            // 防御：SCM 异常时不无限循环（正常只需 2–3 轮）。
            failures += 1;
            break;
        }
        let mut needed = 0u32;
        let mut returned = 0u32;
        let size_bytes = (buf.len() * 8) as u32;
        // SAFETY: 缓冲区大小与指针匹配；resume 指向本栈变量。
        let ok = unsafe {
            EnumServicesStatusExW(
                scm.raw(),
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                buf.as_mut_ptr() as *mut u8,
                size_bytes,
                &mut needed,
                &mut returned,
                &mut resume,
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let code = last_error();
            if code == ERROR_MORE_DATA && needed as usize > size_bytes as usize {
                let units = (needed as usize).div_ceil(8).max(1);
                buf.resize(units, 0);
                continue;
            }
            if code != ERROR_NO_MORE_ITEMS {
                failures += 1;
            }
            break;
        }
        if returned == 0 {
            break;
        }
        let base = buf.as_ptr() as *const u8;
        let stride = std::mem::size_of::<EnumServiceStatusProcessW>();
        for i in 0..returned as usize {
            // SAFETY: 返回条数 <= 缓冲区容量，按固定步长解析。
            let ent = unsafe { &*(base.add(i * stride) as *const EnumServiceStatusProcessW) };
            // SAFETY: 指针指向 SCM 缓冲区内的宽字符串。
            let name = read_wide(ent.service_name);
            if !name.is_empty() {
                names.push(name);
            }
        }
        // 关键：缓冲区没被填满 ⇒ 已取完。若只看 resume（本机成功时为 0），
        // 会无限重复同一批 320 条（实测踩坑，见 LESSONS）。
        let capacity = size_bytes as usize / stride;
        if (returned as usize) < capacity {
            break;
        }
    }

    for name in names {
        let Some(svc) = Handle::new(unsafe {
            // SAFETY: 服务名来自 SCM 枚举结果；仅申请查询权限。
            OpenServiceW(scm.raw(), wide(&name).as_ptr(), SERVICE_QUERY_CONFIG)
        }) else {
            failures += 1;
            continue;
        };
        // 结构化查询：先固定 8 字节对齐的缓冲，再按需扩容。
        let mut cfg_buf: Vec<u64> = vec![0; 1024];
        let mut needed = 0u32;
        let ok = unsafe {
            // SAFETY: 缓冲区与大小匹配；结构体按 repr(C) 解释同一块内存。
            QueryServiceConfigW(
                svc.raw(),
                cfg_buf.as_mut_ptr() as *mut c_void,
                (cfg_buf.len() * 8) as u32,
                &mut needed,
            )
        };
        if ok == 0 && last_error() == ERROR_INSUFFICIENT_BUFFER && needed > 0 {
            let units = (needed as usize).div_ceil(8).max(1);
            cfg_buf.resize(units, 0);
            // SAFETY: 同上，扩容后重试。
            let retry = unsafe {
                QueryServiceConfigW(
                    svc.raw(),
                    cfg_buf.as_mut_ptr() as *mut c_void,
                    (cfg_buf.len() * 8) as u32,
                    &mut needed,
                )
            };
            if retry == 0 {
                failures += 1;
                continue;
            }
        } else if ok == 0 {
            failures += 1;
            continue;
        }

        // SAFETY: 查询成功，缓冲区首部即 QUERY_SERVICE_CONFIGW。
        let cfg = unsafe { &*(cfg_buf.as_ptr() as *const QueryServiceConfigW) };
        if !is_auto_start(cfg.start_type) {
            continue;
        }
        // SAFETY: 字符串指针指向同一缓冲区内部。
        let binary = read_wide(cfg.binary_path);
        let display = read_wide(cfg.display_name);

        let mut delayed_info = ServiceDelayedAutoStartInfo { delayed: 0 };
        let mut needed2 = 0u32;
        // SAFETY: 输出结构体大小与缓冲区一致。
        let delayed_ok = unsafe {
            QueryServiceConfig2W(
                svc.raw(),
                SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                &mut delayed_info as *mut ServiceDelayedAutoStartInfo as *mut u8,
                std::mem::size_of::<ServiceDelayedAutoStartInfo>() as u32,
                &mut needed2,
            )
        };
        let delayed = (delayed_ok != 0).then_some(delayed_info.delayed != 0);
        out.push(service_entry(&name, &display, &binary, delayed));
    }

    let mut failed_sources = Vec::new();
    if failures > 0 {
        failed_sources.push(format!("services_query_failed_{failures}"));
    }
    (out, failed_sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_start_filter() {
        assert!(is_auto_start(2));
        assert!(!is_auto_start(3));
        assert!(!is_auto_start(4));
    }

    #[test]
    fn service_entry_uses_display_name_and_service_name_detail() {
        let e = service_entry(
            "wuauserv",
            "Windows Update",
            "C:\\Windows\\system32\\svchost.exe -k netsvcs",
            Some(false),
        );
        assert_eq!(e.name, "Windows Update");
        assert_eq!(e.detail.as_deref(), Some("wuauserv"));
        assert_eq!(e.kind, BootStartupKind::ServiceAuto);
        assert_eq!(e.scope, BootStartupScope::Machine);
        assert_eq!(e.delayed_auto_start, Some(false));
        assert_eq!(e.enabled, Some(true));
    }

    #[test]
    fn service_entry_falls_back_to_service_name() {
        let e = service_entry("svc", "  ", "C:\\x.exe", None);
        assert_eq!(e.name, "svc");
        assert_eq!(e.delayed_auto_start, None);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn query_service_config_layout_is_msvc_compatible() {
        // 3 个 DWORD 后 4 字节填充，接着是指针。
        assert_eq!(std::mem::offset_of!(QueryServiceConfigW, binary_path), 16);
        assert_eq!(std::mem::size_of::<ServiceStatusProcess>(), 36);
        assert_eq!(std::mem::size_of::<EnumServiceStatusProcessW>(), 56);
    }

    /// 真机手工验证（B1-09）：应得到数十条自动服务（本机基准 111）。
    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_services_prints_count() {
        let (entries, failed) = collect();
        println!("auto services = {}, failed = {:?}", entries.len(), failed);
        for e in entries.iter().take(5) {
            println!("{e:?}");
        }
        assert!(!entries.is_empty(), "真机应有自动启动服务");
    }
}
