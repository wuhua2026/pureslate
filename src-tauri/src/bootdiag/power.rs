//! 快速启动与休眠（只读，设计稿 §2.5）。
//!
//! 两个独立来源：
//! - 注册表 `HiberbootEnabled`（配置意图）；
//! - `CallNtPowerInformation(SystemPowerCapabilities)`（硬件/固件能力）。
//!
//! **不要**探测 `C:\hiberfil.sys`：本机实测 `HiberbootEnabled=1` 但该文件不存在，
//! 且非提权 ACL 会让探测失真。以 API 返回为准。

use std::ffi::c_void;

use crate::contract::{BootPowerData, BootSection, BootSectionStatus};
use crate::startup::winreg::{enum_values_view, Hive};

/// `POWER_INFORMATION_LEVEL::SystemPowerCapabilities`（winnt.h）。
const SYSTEM_POWER_CAPABILITIES_LEVEL: u32 = 4;
/// `sizeof(SYSTEM_POWER_CAPABILITIES)` 在各版本最多 ~88 字节，留足余量。
const POWER_CAPS_BUF: usize = 128;

const POWER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power";
const HIBERBOOT_ENABLED: &str = "HiberbootEnabled";
const REG_DWORD: u32 = 4;

// `SYSTEM_POWER_CAPABILITIES` 前 21 个字段全是 `BOOLEAN`（winnt.h），
// 逐字节排列，因此偏移即字段序号。用 `#[repr(C)]` + `offset_of!` 把这份
// 约定钉死在测试里，避免以后有人误以为可以加 `u32` 字段。
#[repr(C)]
#[allow(dead_code)]
struct SystemPowerCapabilitiesHead {
    power_button_present: u8,
    sleep_button_present: u8,
    lid_present: u8,
    system_s1: u8,
    system_s2: u8,
    system_s3: u8,
    system_s4: u8,
    system_s5: u8,
    hiber_file_present: u8,
    full_wake: u8,
    video_dim_present: u8,
    apm_present: u8,
    ups_present: u8,
    thermal_control: u8,
    processor_throttle: u8,
    processor_min_throttle: u8,
    processor_max_throttle: u8,
    fast_system_s4: u8,
    hiberboot: u8,
    wake_alarm_present: u8,
    ao_ac: u8,
}

/// 采集所需的能力位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PowerCaps {
    pub system_s4: bool,
    pub hiber_file_present: bool,
    pub hiberboot: bool,
    pub ao_ac: bool,
}

#[link(name = "powrprof")]
extern "system" {
    fn CallNtPowerInformation(
        information_level: u32,
        input_buffer: *mut c_void,
        input_buffer_length: u32,
        output_buffer: *mut c_void,
        output_buffer_length: u32,
    ) -> i32;
}

/// 读电源能力（非 0 NTSTATUS 原样返回，调用方降级为 `partial`）。
pub(crate) fn power_capabilities() -> Result<PowerCaps, i32> {
    let mut buf = [0u8; POWER_CAPS_BUF];
    // 安全性：输出缓冲区长度与指针配对传入；API 只写该缓冲区。
    let status = unsafe {
        CallNtPowerInformation(
            SYSTEM_POWER_CAPABILITIES_LEVEL,
            std::ptr::null_mut(),
            0,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let at = |offset: usize| buf[offset] != 0;
    Ok(PowerCaps {
        system_s4: at(std::mem::offset_of!(SystemPowerCapabilitiesHead, system_s4)),
        hiber_file_present: at(std::mem::offset_of!(
            SystemPowerCapabilitiesHead,
            hiber_file_present
        )),
        hiberboot: at(std::mem::offset_of!(SystemPowerCapabilitiesHead, hiberboot)),
        ao_ac: at(std::mem::offset_of!(SystemPowerCapabilitiesHead, ao_ac)),
    })
}

/// 读取 `HiberbootEnabled`；返回值 = (`registry_present`, `enabled`)。
///
/// 值存在但类型/长度异常时 `registry_present=true` 且 `enabled=None`，
/// 不把"看不懂"当成"关"。
pub(crate) fn read_hiberboot() -> Result<(bool, Option<bool>), i32> {
    let values = enum_values_view(Hive::LocalMachine, POWER_KEY, 0)
        .map_err(|e| e.raw_os_error().unwrap_or(-1))?;
    Ok(
        match values
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(HIBERBOOT_ENABLED))
        {
            Some(v) => (true, dword_to_bool(v.kind, &v.data)),
            None => (false, None),
        },
    )
}

/// `REG_DWORD` → bool；非 DWORD 或长度不足 → `None`。
pub(crate) fn dword_to_bool(kind: u32, data: &[u8]) -> Option<bool> {
    if kind != REG_DWORD || data.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]) != 0)
}

/// 采集快速启动/休眠状态（只读）。
pub fn collect() -> BootSection<BootPowerData> {
    let mut failed: Vec<String> = Vec::new();

    let (registry_present, fast_startup_enabled) = match read_hiberboot() {
        Ok((present, enabled)) => (present, enabled),
        Err(code) => {
            failed.push(format!("power_registry_code_{code}"));
            (false, None)
        }
    };
    let registry_ok = failed.is_empty();

    let caps = match power_capabilities() {
        Ok(caps) => Some(caps),
        Err(code) => {
            failed.push(format!("power_caps_status_{code}"));
            None
        }
    };

    // 两个来源都读不到才算 unavailable；否则保留能读到的部分。
    if !registry_ok && caps.is_none() {
        return BootSection::with_reason(
            BootSectionStatus::Unavailable,
            super::failed_reason(&failed),
            None,
        );
    }

    let data = BootPowerData {
        fast_startup_enabled,
        fast_startup_registry_present: registry_present,
        hibernate_supported: caps.map(|c| c.system_s4),
        hiber_file_present: caps.map(|c| c.hiber_file_present),
        hiberboot_capability: caps.map(|c| c.hiberboot),
        modern_standby: caps.map(|c| c.ao_ac),
    };

    BootSection::with_reason(
        if failed.is_empty() {
            BootSectionStatus::Ok
        } else {
            BootSectionStatus::Partial
        },
        super::failed_reason(&failed),
        Some(data),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_field_offsets_match_winnt_header() {
        // winnt.h：SystemS4 = 6、HiberFilePresent = 8、Hiberboot = 18、AoAc = 20。
        assert_eq!(
            std::mem::offset_of!(SystemPowerCapabilitiesHead, system_s4),
            6
        );
        assert_eq!(
            std::mem::offset_of!(SystemPowerCapabilitiesHead, hiber_file_present),
            8
        );
        assert_eq!(
            std::mem::offset_of!(SystemPowerCapabilitiesHead, hiberboot),
            18
        );
        assert_eq!(std::mem::offset_of!(SystemPowerCapabilitiesHead, ao_ac), 20);
        assert_eq!(std::mem::size_of::<SystemPowerCapabilitiesHead>(), 21);
    }

    #[test]
    fn dword_to_bool_rejects_wrong_type_and_short_data() {
        assert_eq!(dword_to_bool(REG_DWORD, &[1, 0, 0, 0]), Some(true));
        assert_eq!(dword_to_bool(REG_DWORD, &[0, 0, 0, 0]), Some(false));
        assert_eq!(dword_to_bool(REG_DWORD, &[1, 0]), None);
        assert_eq!(dword_to_bool(1, &[1, 0, 0, 0]), None, "REG_SZ 不是布尔");
    }

    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_power_prints_section() {
        let section = collect();
        println!(
            "status = {:?}, reason = {:?}",
            section.status, section.reason
        );
        if let Some(d) = &section.data {
            println!(
                "fast_startup={:?} present={} s4={:?} hiberfile={:?} hiberboot={:?} aoac={:?}",
                d.fast_startup_enabled,
                d.fast_startup_registry_present,
                d.hibernate_supported,
                d.hiber_file_present,
                d.hiberboot_capability,
                d.modern_standby
            );
        }
    }
}
