//! 进程守卫（R23 双语义 · SAFETY §5）。
//!
//! - `process`（第一语义）：清理某应用类目缓存/数据前，检测其主进程是否运行；
//!   运行中 → 该类目整体阻止 + UI 提示，**不做半清**（SAFETY §5.1）；
//! - `instance`（第二语义，P4-02）：命名互斥量单实例——第二实例激活首实例窗口后退出（§5.2）。

pub mod instance;
pub mod process;

pub use instance::{activate_main_window, is_single_instance};
pub use process::running_matching;

/// 判定给定进程名（exe 基名，忽略大小写）中是否有正在运行的。
/// 命中任一 → 该类目应整体阻止。
pub fn any_running(exe_base_names: &[String]) -> bool {
    !running_matching(exe_base_names).is_empty()
}
