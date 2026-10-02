//! 单实例自守卫（R23 第二语义 · SAFETY §5.2，P4-02）。
//!
//! 命名互斥量 `Global\PureSlateSingleInstance`：第二实例检测到已存在 →
//! 激活首实例主窗口（按窗口标题查找，尽力而为）后退出进程。
//! 零第三方依赖：`#[link]` 直调 kernel32/user32。

/// 单实例互斥量名（Global 命名空间，跨会话）。
pub const MUTEX_NAME: &str = r"Global\PureSlateSingleInstance";
/// 主窗口标题（P0-02：MainWindowTitle=PureSlate）。
pub const MAIN_WINDOW_TITLE: &str = "PureSlate";

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    const ERROR_ALREADY_EXISTS: i32 = 183;
    const SW_RESTORE: i32 = 9;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attrs: *const c_void, initial_owner: i32, name: *const u16) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> *mut c_void;
        fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
        fn SetForegroundWindow(hwnd: *mut c_void) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 尝试成为唯一实例。返回 true = 本进程是首实例（互斥量已持有，进程存活期间不释放）。
    /// 返回 false = 已有实例在运行。
    pub fn is_single_instance(mutex_name: &str) -> bool {
        let name = wide(mutex_name);
        // 安全性：CreateMutexW 参数为合法宽字符串；返回句柄为 NULL 时按"已有实例"保守处理。
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        // GetLastError 必须紧跟调用；通过 std io 取原始错误码。
        let already = std::io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS);
        if already {
            // 安全性：CloseHandle 关闭本进程持有的重复句柄（不影响首实例的互斥量）。
            unsafe { CloseHandle(handle) };
            false
        } else {
            // 首实例：句柄有意持有到进程结束（互斥量随进程销毁自动释放）。
            true
        }
    }

    /// 激活首实例主窗口（按标题查找 + 恢复 + 前置）。尽力而为，失败静默。
    pub fn activate_main_window(title: &str) {
        let t = wide(title);
        // 安全性：FindWindowW 仅按标题查找顶层窗口，无副作用。
        let hwnd = unsafe { FindWindowW(std::ptr::null(), t.as_ptr()) };
        if hwnd.is_null() {
            return;
        }
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_single_instance(_mutex_name: &str) -> bool {
        true
    }
    pub fn activate_main_window(_title: &str) {}
}

/// 尝试成为唯一实例（true=首实例）。非 Windows 恒 true。
pub fn is_single_instance(mutex_name: &str) -> bool {
    imp::is_single_instance(mutex_name)
}

/// 激活首实例主窗口（尽力而为）。
pub fn activate_main_window(title: &str) {
    imp::activate_main_window(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_reports_existing() {
        // 同进程内两次获取同名互斥量：第二次 must 报 ERROR_ALREADY_EXISTS。
        // 用例专用名字（避免与生产互斥量互扰），句柄由 is_single_instance 语义持有。
        let name = format!("Local\\PureSlateTest-{}-{}", std::process::id(), 4242);
        assert!(is_single_instance(&name), "首次获取应为首实例");
        assert!(!is_single_instance(&name), "再次获取应判定已有实例");
        // 不同名字互不影响。
        assert!(is_single_instance(&format!("{name}-other")));
    }

    #[test]
    fn activate_window_is_best_effort() {
        // 无同名窗口时静默返回（不 panic）。
        activate_main_window("__pureslate_no_such_window__");
    }
}
