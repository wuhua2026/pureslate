//! 开机耗时（只读）：`Microsoft-Windows-Diagnostics-Performance/Operational`。
//!
//! 调用链（全部只读，无写操作）：
//! `EvtOpenChannelConfig` → `EvtGetChannelConfigProperty(Enabled)` → `EvtClose`
//! `EvtQuery` → `EvtNext` → `EvtRender(EvtRenderEventXml)` → `EvtClose`。
//!
//! 边界（设计稿 §3.1）：非提权 ACL 不含交互用户 → `denied`；通道未启用/无事件 →
//! `unavailable`；能读到但通道已关 → `partial` + 备注。**不开启通道、不弹 UAC**。

use std::ffi::c_void;
use std::io;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::contract::{
    BootIssue, BootIssueKind, BootPhaseBreakdown, BootSection, BootSectionStatus, BootTimingData,
    BootTimingSample,
};

const CHANNEL: &str = "Microsoft-Windows-Diagnostics-Performance/Operational";
const XPATH_FILTERED: &str = "*[System[(EventID >= 100) and (EventID <= 110)]]";
const XPATH_ALL: &str = "*";
const MAX_EVENTS: usize = 200;
const MAX_SAMPLES: usize = 10;
const WINDOW_SECS: i64 = 7 * 24 * 3600;
const BATCH: usize = 8;

// ---- wevtapi FFI（零 crate；句柄为 HANDLE）----

type EvtHandle = isize;

const EVT_OPEN_CHANNEL_PATH: u32 = 1;
const EVT_QUERY_CHANNEL_PATH: u32 = 0x1;
const EVT_QUERY_REVERSE_DIRECTION: u32 = 0x200;
const EVT_RENDER_EVENT_XML: u32 = 1;
const EVT_CHANNEL_CONFIG_ENABLED: u32 = 0;
const EVT_VARIANT_TYPE_BOOLEAN: u32 = 1;

const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const ERROR_NO_MORE_ITEMS: i32 = 259;
const ERROR_EVT_INVALID_QUERY: i32 = 15001;
const ERROR_EVT_CHANNEL_NOT_FOUND: i32 = 15007;

#[link(name = "wevtapi")]
extern "system" {
    fn EvtOpenChannelConfig(session: EvtHandle, path: *const u16, flags: u32) -> EvtHandle;
    fn EvtGetChannelConfigProperty(
        config: EvtHandle,
        property_id: u32,
        flags: u32,
        buffer_size: u32,
        buffer: *mut c_void,
        used: *mut u32,
    ) -> i32;
    fn EvtQuery(session: EvtHandle, path: *const u16, query: *const u16, flags: u32) -> EvtHandle;
    fn EvtNext(
        result_set: EvtHandle,
        events_size: u32,
        events: *mut EvtHandle,
        timeout: u32,
        flags: u32,
        returned: *mut u32,
    ) -> i32;
    fn EvtRender(
        context: EvtHandle,
        fragment: EvtHandle,
        flags: u32,
        buffer_size: u32,
        buffer: *mut c_void,
        used: *mut u32,
        property_count: *mut u32,
    ) -> i32;
    fn EvtClose(object: EvtHandle) -> i32;
}

/// EVT_VARIANT：8 字节 union + Count + Type（x86/x64 均为 16 字节）。
#[repr(C)]
#[derive(Default)]
struct EvtVariant {
    value: u64,
    count: u32,
    type_: u32,
}

struct Handle(EvtHandle);

impl Handle {
    fn new(h: EvtHandle) -> Option<Self> {
        (h != 0).then_some(Self(h))
    }
    fn raw(&self) -> EvtHandle {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: 句柄由 wevtapi 返回且非 0；Drop 后不再使用。
        unsafe {
            EvtClose(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// 读取通道"是否继续收集"标志；失败返回 OS 错误码。
fn channel_enabled() -> Result<bool, i32> {
    let path = wide(CHANNEL);
    // SAFETY: 传入合法 UTF-16 路径；session=NULL 表示本机。
    let cfg = unsafe { EvtOpenChannelConfig(0, path.as_ptr(), EVT_OPEN_CHANNEL_PATH) };
    let Some(cfg) = Handle::new(cfg) else {
        return Err(last_error());
    };
    let mut variant = EvtVariant::default();
    let mut used = 0u32;
    // SAFETY: 缓冲区为合法的 EVT_VARIANT，大小与之匹配。
    let ok = unsafe {
        EvtGetChannelConfigProperty(
            cfg.raw(),
            EVT_CHANNEL_CONFIG_ENABLED,
            0,
            std::mem::size_of::<EvtVariant>() as u32,
            &mut variant as *mut EvtVariant as *mut c_void,
            &mut used,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(variant.type_ == EVT_VARIANT_TYPE_BOOLEAN && variant.value as u32 != 0)
}

/// 查询结果集；指定 XPath 无效时降级为 `*`（调用方按 EventId 过滤）。
fn open_query() -> Result<(Handle, bool), i32> {
    let path = wide(CHANNEL);
    let flags = EVT_QUERY_CHANNEL_PATH | EVT_QUERY_REVERSE_DIRECTION;
    let filtered = wide(XPATH_FILTERED);
    // SAFETY: 路径/查询均为合法 UTF-16；返回句柄由 Handle 负责关闭。
    let h = unsafe { EvtQuery(0, path.as_ptr(), filtered.as_ptr(), flags) };
    if h != 0 {
        return Ok((Handle(h), false));
    }
    let err = last_error();
    if err != ERROR_EVT_INVALID_QUERY {
        return Err(err);
    }
    let all = wide(XPATH_ALL);
    // SAFETY: 同上；降级查询 `*`。
    let h = unsafe { EvtQuery(0, path.as_ptr(), all.as_ptr(), flags) };
    match Handle::new(h) {
        Some(h) => Ok((h, true)),
        None => Err(last_error()),
    }
}

/// 渲染一条事件为 XML 字符串（两步法：先探尺寸再取内容）。
fn render_xml(event: EvtHandle) -> Result<String, i32> {
    let mut used = 0u32;
    let mut count = 0u32;
    // SAFETY: 缓冲区传空、大小 0，仅探所需字节数。
    let ok = unsafe {
        EvtRender(
            0,
            event,
            EVT_RENDER_EVENT_XML,
            0,
            std::ptr::null_mut(),
            &mut used,
            &mut count,
        )
    };
    if ok != 0 || last_error() != ERROR_INSUFFICIENT_BUFFER {
        return Err(last_error());
    }
    if used == 0 {
        return Ok(String::new());
    }
    let units = (used as usize).div_ceil(2);
    let mut buf = vec![0u16; units + 1];
    let mut used2 = 0u32;
    // SAFETY: 缓冲区按探得的字节数分配（额外 1 个 u16 兜底），长度以字节传入。
    let ok = unsafe {
        EvtRender(
            0,
            event,
            EVT_RENDER_EVENT_XML,
            (buf.len() * 2) as u32,
            buf.as_mut_ptr() as *mut c_void,
            &mut used2,
            &mut count,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    let n = (used2 as usize / 2).min(buf.len());
    Ok(String::from_utf16_lossy(&buf[..n])
        .trim_end_matches('\0')
        .to_string())
}

/// 一条原始事件的解析结果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RawEvent {
    pub event_id: u16,
    pub time_created: Option<String>,
    pub data: Vec<(String, String)>,
}

impl RawEvent {
    fn field(&self, names: &[&str]) -> Option<&str> {
        for n in names {
            if let Some((_, v)) = self.data.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)) {
                if !v.trim().is_empty() {
                    return Some(v);
                }
            }
        }
        None
    }

    fn field_u64(&self, names: &[&str]) -> Option<u64> {
        self.field(names).and_then(|v| v.trim().parse::<u64>().ok())
    }

    fn field_bool(&self, names: &[&str]) -> Option<bool> {
        self.field(names).map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("true") || v == "1"
        })
    }
}

/// 解析事件 XML：不依赖字段顺序，按 `Data@Name` 建映射；缺失即 None。
pub(crate) fn parse_event_xml(xml: &str) -> Option<RawEvent> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out = RawEvent::default();
    let mut in_system = 0usize;
    let mut capture_id = false;
    let mut current_data_name: Option<String> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "System" => in_system += 1,
                "EventID" if in_system > 0 => {
                    capture_id = true;
                    text.clear();
                }
                "TimeCreated" => {
                    if let Some(v) = attr_value(&e, b"SystemTime") {
                        out.time_created = Some(v);
                    }
                }
                "Data" => {
                    current_data_name = attr_value(&e, b"Name");
                    text.clear();
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if local_name(e.name().as_ref()) == "TimeCreated" {
                    if let Some(v) = attr_value(&e, b"SystemTime") {
                        out.time_created = Some(v);
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if capture_id || current_data_name.is_some() {
                    text.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::CData(t)) => {
                if current_data_name.is_some() {
                    text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "System" if in_system > 0 => in_system -= 1,
                "EventID" if capture_id => {
                    out.event_id = text.trim().parse::<u16>().unwrap_or(0);
                    capture_id = false;
                    text.clear();
                }
                "Data" => {
                    if let Some(name) = current_data_name.take() {
                        if !name.is_empty() {
                            out.data.push((name, text.trim().to_string()));
                        }
                    }
                    text.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (out.event_id > 0).then_some(out)
}

fn attr_value(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.as_ref().rsplit(|b| *b == b':').next() == Some(name) {
            return a
                .unescape_value()
                .ok()
                .map(|v| v.into_owned())
                .or_else(|| Some(String::from_utf8_lossy(&a.value).to_string()));
        }
    }
    None
}

fn local_name(qname: &[u8]) -> &str {
    let s = std::str::from_utf8(qname).unwrap_or("");
    s.rsplit(':').next().unwrap_or("")
}

/// 事件号 → 类别（101–110 映射见设计稿 §2.2）。
pub(crate) fn issue_kind(event_id: u16) -> BootIssueKind {
    match event_id {
        101 => BootIssueKind::Application,
        102 => BootIssueKind::Driver,
        103 => BootIssueKind::Service,
        106 => BootIssueKind::BackgroundOptimization,
        107 => BootIssueKind::MachinePolicy,
        108 => BootIssueKind::UserPolicy,
        109 => BootIssueKind::Device,
        110 => BootIssueKind::SessionManager,
        _ => BootIssueKind::Unknown,
    }
}

/// 事件 100 → 开机耗时样本。
pub(crate) fn parse_timing_sample(ev: &RawEvent) -> BootTimingSample {
    let phases = BootPhaseBreakdown {
        kernel_init_ms: ev.field_u64(&["BootKernelInitTime"]),
        driver_init_ms: ev.field_u64(&["BootDriverInitTime"]),
        devices_init_ms: ev.field_u64(&["BootDevicesInitTime"]),
        prefetch_init_ms: ev.field_u64(&["BootPrefetchInitTime"]),
        smss_init_ms: ev.field_u64(&["BootSmssInitTime"]),
        critical_services_init_ms: ev.field_u64(&["BootCriticalServicesInitTime"]),
        user_profile_ms: ev.field_u64(&["BootUserProfileProcessingTime"]),
        machine_profile_ms: ev.field_u64(&["BootMachineProfileProcessingTime"]),
        explorer_init_ms: ev.field_u64(&["BootExplorerInitTime"]),
    };
    BootTimingSample {
        booted_at: ev.time_created.clone(),
        boot_time_ms: ev.field_u64(&["BootTime"]),
        main_path_boot_time_ms: ev.field_u64(&["MainPathBootTime"]),
        post_boot_time_ms: ev.field_u64(&["BootPostBootTime"]),
        degradation_delta_ms: ev.field_u64(&["BootDegradationDelta"]),
        is_degradation: ev.field_bool(&["BootIsDegradation"]),
        startup_app_count: ev
            .field_u64(&["BootNumStartupApps"])
            .and_then(|v| u32::try_from(v).ok()),
        phases: Some(phases),
    }
}

/// 事件 101–110 → 拖慢明细。
pub(crate) fn parse_issue(ev: &RawEvent) -> BootIssue {
    BootIssue {
        event_id: ev.event_id,
        kind: issue_kind(ev.event_id),
        name: ev
            .field(&["Name", "File Name", "Service Name", "Device Name"])
            .map(str::to_string),
        friendly_name: ev.field(&["Friendly Name"]).map(str::to_string),
        version: ev.field(&["Version"]).map(str::to_string),
        total_time_ms: ev.field_u64(&["Total Time", "TotalTime"]),
        degradation_time_ms: ev.field_u64(&["Degradation Time", "DegradationTime"]),
        at: ev.time_created.clone(),
    }
}

/// 采集开机耗时。`now_secs` 由调用方注入，便于单测窗口逻辑。
pub fn collect() -> BootSection<BootTimingData> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    collect_with_now(now)
}

fn collect_with_now(now_secs: i64) -> BootSection<BootTimingData> {
    let enabled = channel_enabled();
    let (enabled_flag, enabled_err) = match enabled {
        Ok(v) => (v, None),
        Err(code) => (false, Some(code)),
    };

    let (query, fallback) = match open_query() {
        Ok(v) => v,
        Err(code) => {
            let data = BootTimingData {
                channel_enabled: enabled_flag,
                samples: vec![],
                issues: vec![],
                truncated: false,
            };
            return match code {
                // 通道配置本身也可能读不到；不携带 data，避免把"未知"报成 enabled=false。
                5 => BootSection::with_reason(
                    BootSectionStatus::Denied,
                    "事件日志访问被拒绝：需要管理员权限（不会自动提权）",
                    None,
                ),
                ERROR_EVT_CHANNEL_NOT_FOUND => BootSection::with_reason(
                    BootSectionStatus::Unavailable,
                    "事件通道不存在（系统版本可能不支持开机性能诊断）",
                    Some(data),
                ),
                _ => BootSection::with_reason(
                    BootSectionStatus::Unavailable,
                    format!("事件查询失败（OS 错误码 {code}）"),
                    Some(data),
                ),
            };
        }
    };

    let mut samples = Vec::new();
    let mut issues = Vec::new();
    let mut read = 0usize;
    let mut truncated = false;
    let mut error: Option<i32> = None;

    'outer: loop {
        let mut handles = [0 as EvtHandle; BATCH];
        let mut returned = 0u32;
        // SAFETY: 数组长度与 events_size 一致；返回句柄逐个交给 Handle 关闭。
        let ok = unsafe {
            EvtNext(
                query.raw(),
                BATCH as u32,
                handles.as_mut_ptr(),
                u32::MAX, // INFINITE：函数内部立即返回，不会真等满
                0,
                &mut returned,
            )
        };
        if ok == 0 {
            let code = last_error();
            if code != ERROR_NO_MORE_ITEMS {
                error = Some(code);
            }
            break;
        }
        if returned == 0 {
            break;
        }
        for &raw in handles.iter().take(returned as usize) {
            let Some(h) = Handle::new(raw) else { continue };
            match render_xml(h.raw()) {
                Ok(xml) => {
                    if let Some(ev) = parse_event_xml(&xml) {
                        // 降级查询时客户端按事件号过滤。
                        if fallback && !(100..=110).contains(&ev.event_id) {
                            continue;
                        }
                        // 窗口：只保留最近 7 天；解析不了时间就保留。
                        if let Some(ts) = ev.time_created.as_deref() {
                            if let Some(secs) = parse_rfc3339_secs(ts) {
                                if now_secs.saturating_sub(secs) > WINDOW_SECS {
                                    break 'outer;
                                }
                            }
                        }
                        match ev.event_id {
                            100 if samples.len() < MAX_SAMPLES => {
                                samples.push(parse_timing_sample(&ev));
                            }
                            100 => truncated = true,
                            101..=110 => issues.push(parse_issue(&ev)),
                            _ => {}
                        }
                        read += 1;
                        if read >= MAX_EVENTS {
                            truncated = true;
                            break 'outer;
                        }
                    }
                }
                Err(_) => {
                    // 单条渲染失败不拖垮整批；计数体现在最终 partial 备注里。
                    error.get_or_insert(-1);
                }
            }
        }
    }

    let data = BootTimingData {
        channel_enabled: enabled_flag,
        samples,
        issues,
        truncated,
    };

    // 状态判定（设计稿 §3.1）。
    if data.samples.is_empty() && data.issues.is_empty() {
        if let Some(code) = enabled_err {
            return BootSection::with_reason(
                BootSectionStatus::Unavailable,
                format!("无法读取事件通道配置（OS 错误码 {code}）"),
                Some(data),
            );
        }
        if !enabled_flag {
            return BootSection::with_reason(
                BootSectionStatus::Unavailable,
                "开机性能事件通道未启用：可在事件查看器中手动启用后重新诊断",
                Some(data),
            );
        }
        return BootSection::with_reason(
            BootSectionStatus::Unavailable,
            "未找到开机性能事件：可能尚未记录、日志已滚动，或被组策略关闭",
            Some(data),
        );
    }

    let mut reasons = Vec::new();
    if !enabled_flag {
        reasons.push("事件通道当前已关闭，数据截至最后一条事件".to_string());
    }
    if let Some(code) = error {
        reasons.push(if code == -1 {
            "部分事件渲染失败".to_string()
        } else {
            format!("部分事件读取失败（OS 错误码 {code}）")
        });
    }
    if truncated {
        reasons.push(format!(
            "仅保留最近 {} 条事件 / {} 次开机",
            MAX_EVENTS, MAX_SAMPLES
        ));
    }
    if reasons.is_empty() {
        BootSection::ok(data)
    } else {
        BootSection::with_reason(BootSectionStatus::Partial, reasons.join("；"), Some(data))
    }
}

/// RFC3339 → Unix 秒（容忍小数秒与 `Z`）；解析失败返回 None。
pub(crate) fn parse_rfc3339_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let se: i64 = s.get(17..19)?.parse().ok()?;
    let days = days_from_civil(y, mo as u32, d as u32);
    Some(days * 86_400 + h * 3_600 + mi * 60 + se)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const EV_100: &str = r#"<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><Provider Name="Microsoft-Windows-Diagnostics-Performance"/><EventID Qualifiers="16384">100</EventID>
<TimeCreated SystemTime="2026-11-08T23:12:05.1234567Z"/></System>
<EventData>
<Data Name="BootTime">34857</Data><Data Name="MainPathBootTime">12657</Data>
<Data Name="BootPostBootTime">22200</Data><Data Name="BootDegradationDelta">0</Data>
<Data Name="BootIsDegradation">false</Data><Data Name="BootNumStartupApps">18</Data>
<Data Name="BootKernelInitTime">4210</Data><Data Name="BootDriverInitTime">5120</Data>
<Data Name="BootDevicesInitTime">1880</Data><Data Name="BootPrefetchInitTime">940</Data>
<Data Name="BootSmssInitTime">210</Data><Data Name="BootCriticalServicesInitTime">2600</Data>
<Data Name="BootUserProfileProcessingTime">3400</Data>
<Data Name="BootMachineProfileProcessingTime">1200</Data>
<Data Name="BootExplorerInitTime">1700</Data>
</EventData></Event>"#;

    const EV_102: &str = r#"<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><EventID>102</EventID><TimeCreated SystemTime="2026-11-09T10:02:41.0000000Z"/></System>
<EventData><Data Name="File Name">\SystemRoot\System32\drivers\x.sys</Data>
<Data Name="Friendly Name">Test Driver</Data><Data Name="Version">1.2.3</Data>
<Data Name="Total Time">2600</Data><Data Name="Degradation Time">900</Data></EventData></Event>"#;

    const EV_109: &str = r#"<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><EventID>109</EventID></System>
<EventData><Data Name="TotalTime">300</Data></EventData></Event>"#;

    #[test]
    fn parses_event_100_fields() {
        let ev = parse_event_xml(EV_100).expect("must parse");
        assert_eq!(ev.event_id, 100);
        assert_eq!(
            ev.time_created.as_deref(),
            Some("2026-11-08T23:12:05.1234567Z")
        );
        let s = parse_timing_sample(&ev);
        assert_eq!(s.boot_time_ms, Some(34857));
        assert_eq!(s.main_path_boot_time_ms, Some(12657));
        assert_eq!(s.post_boot_time_ms, Some(22200));
        assert_eq!(s.degradation_delta_ms, Some(0));
        assert_eq!(s.is_degradation, Some(false));
        assert_eq!(s.startup_app_count, Some(18));
        let phases = s.phases.unwrap();
        assert_eq!(phases.kernel_init_ms, Some(4210));
        assert_eq!(phases.explorer_init_ms, Some(1700));
    }

    #[test]
    fn boot_time_equals_main_plus_post() {
        let ev = parse_event_xml(EV_100).unwrap();
        let s = parse_timing_sample(&ev);
        assert_eq!(
            s.boot_time_ms,
            Some(s.main_path_boot_time_ms.unwrap() + s.post_boot_time_ms.unwrap())
        );
    }

    #[test]
    fn parses_driver_issue_aliases() {
        let ev = parse_event_xml(EV_102).unwrap();
        let issue = parse_issue(&ev);
        assert_eq!(issue.kind, BootIssueKind::Driver);
        assert_eq!(
            issue.name.as_deref(),
            Some("\\SystemRoot\\System32\\drivers\\x.sys")
        );
        assert_eq!(issue.friendly_name.as_deref(), Some("Test Driver"));
        assert_eq!(issue.version.as_deref(), Some("1.2.3"));
        assert_eq!(issue.total_time_ms, Some(2600));
        assert_eq!(issue.degradation_time_ms, Some(900));
    }

    #[test]
    fn missing_fields_stay_none() {
        let ev = parse_event_xml(EV_109).unwrap();
        let issue = parse_issue(&ev);
        assert_eq!(issue.kind, BootIssueKind::Device);
        assert_eq!(issue.name, None);
        assert_eq!(issue.total_time_ms, Some(300));
    }

    #[test]
    fn parses_time_with_fractional_seconds() {
        assert_eq!(parse_rfc3339_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_secs("2026-11-08T23:12:05.1234567Z"),
            Some(1_794_179_525)
        );
    }

    #[test]
    fn rfc3339_roundtrip_against_formatter() {
        for secs in [0i64, 1_000_000_000, 1_767_225_600, 1_794_179_525] {
            let s = crate::bootdiag::format_rfc3339(secs as u64);
            assert_eq!(parse_rfc3339_secs(&s), Some(secs));
        }
    }

    #[test]
    fn event_id_filter_range() {
        assert_eq!(issue_kind(100), BootIssueKind::Unknown);
        assert_eq!(issue_kind(101), BootIssueKind::Application);
        assert_eq!(issue_kind(110), BootIssueKind::SessionManager);
    }

    /// 真机手工验证（B1-09）：非提权应 `denied`，提权应 `ok/partial`。
    /// 运行：`cargo test --lib live_collect_prints_section -- --ignored --nocapture`
    #[test]
    #[ignore = "真机手工验证；CI/无桌面环境不跑"]
    fn live_collect_prints_section() {
        let s = collect();
        println!("status={:?} reason={:?}", s.status, s.reason);
        if let Some(d) = &s.data {
            println!(
                "enabled={} samples={} issues={} truncated={}",
                d.channel_enabled,
                d.samples.len(),
                d.issues.len(),
                d.truncated
            );
            for x in d.samples.iter().take(3) {
                println!("sample: {x:?}");
            }
            for x in d.issues.iter().take(5) {
                println!("issue: {x:?}");
            }
        }
    }
}
