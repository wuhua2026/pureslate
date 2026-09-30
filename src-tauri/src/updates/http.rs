//! 极简 HTTPS GET（R22 更新检查）。WinHTTP 直调，零第三方依赖（AGENTS §4.7 依赖准入）。
//!
//! 约束：仅 https；超时（解析 5s/连接 5s/发送 5s/接收 15s）；非 2xx 一律失败；
//! 本模块只服务 update-manifest 与规则包下载（红线 #4：联网仅限 R22/R24 opt-in）。
//! 不做单测（无网络沙箱）；逻辑保持薄、可评审，回退/校验等可测逻辑在 mod.rs 注入层。

use std::ffi::c_void;

/// WinHTTP 句柄（RAII 关闭）。
struct HWin(*mut c_void);

impl Drop for HWin {
    fn drop(&mut self) {
        // 安全性：句柄必来自 WinHttpOpen/Connect/OpenRequest 成功路径（非 NULL）。
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

const WINHTTP_ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access_type: u32,
        proxy: *const u16,
        proxy_bypass: *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpConnect(
        session: *mut c_void,
        host: *const u16,
        port: u16,
        reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        connect: *mut c_void,
        verb: *const u16,
        object: *const u16,
        version: *const u16,
        referer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSetTimeouts(
        request: *mut c_void,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpSendRequest(
        request: *mut c_void,
        headers: *const u16,
        headers_len: u32,
        optional: *const c_void,
        optional_len: u32,
        total_len: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: *mut c_void, reserved: *const c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: *mut c_void,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        buffer_len: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpQueryDataAvailable(request: *mut c_void, available: *mut u32) -> i32;
    fn WinHttpReadData(request: *mut c_void, buffer: *mut u8, to_read: u32, read: *mut u32) -> i32;
    fn WinHttpCloseHandle(handle: *mut c_void) -> i32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 解析出的 URL 组件（仅 https）。
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
}

/// 手工解析 `https://host[:port]/path?query`。非 https / 畸形 → None。
fn parse_https_url(url: &str) -> Option<ParsedUrl> {
    let rest = url.strip_prefix("https://")?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (host_port, 443),
    };
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(ParsedUrl {
        host: host.to_string(),
        port,
        path: path.to_string(),
    })
}

/// HTTPS GET → 响应体字节。任一步失败 → Err（调用方按通道回退处理）。
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let parsed = parse_https_url(url).ok_or_else(|| format!("非法 URL: {url}"))?;
        get_https(&parsed)
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("更新检查仅支持 Windows（WinHTTP）".into())
    }
}

#[cfg(windows)]
fn get_https(p: &ParsedUrl) -> Result<Vec<u8>, String> {
    // 安全性：以下 WinHTTP 调用参数均为有效宽字符串指针/NULL；句柄 RAII 关闭。
    unsafe {
        let agent = wide("PureSlate-Update-Check");
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHttpOpen 失败".into());
        }
        let session = HWin(session);

        let host = wide(&p.host);
        let connect = WinHttpConnect(session.0, host.as_ptr(), p.port, 0);
        if connect.is_null() {
            return Err(format!("连接失败: {}", p.host));
        }
        let connect = HWin(connect);

        let verb = wide("GET");
        let object = wide(&p.path);
        let request = WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            object.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            return Err("创建请求失败".into());
        }
        let request = HWin(request);

        if WinHttpSetTimeouts(request.0, 5000, 5000, 5000, 15_000) != 0 {
            return Err("设置超时失败".into());
        }
        if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 {
            return Err("发送请求失败".into());
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null()) == 0 {
            return Err("接收响应失败".into());
        }

        // 状态码（数值查询）。
        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("查询状态码失败".into());
        }
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }

        // 读体循环。
        let mut body = Vec::new();
        let mut available: u32 = 0;
        loop {
            if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                return Err("查询数据可用性失败".into());
            }
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read: u32 = 0;
            if WinHttpReadData(request.0, chunk.as_mut_ptr(), available, &mut read) == 0 {
                return Err("读取响应体失败".into());
            }
            chunk.truncate(read as usize);
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_variants() {
        let u = parse_https_url("https://cdn.jsdelivr.net/gh/a/b@latest/x.json").unwrap();
        assert_eq!(u.host, "cdn.jsdelivr.net");
        assert_eq!(u.port, 443);
        assert_eq!(u.path, "/gh/a/b@latest/x.json");

        let u = parse_https_url("https://example.com:8443/p?q=1").unwrap();
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 8443);
        assert_eq!(u.path, "/p?q=1");

        let u = parse_https_url("https://example.com").unwrap();
        assert_eq!(u.path, "/");

        assert!(parse_https_url("http://insecure.example/x").is_none());
        assert!(parse_https_url("ftp://x/y").is_none());
        assert!(parse_https_url("https://").is_none());
    }
}
