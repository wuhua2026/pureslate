//! 进程守卫（R23 第一语义 · SAFETY §5.1）。
//!
//! 目标应用守卫：清理某应用类目缓存/数据前，检测其主进程是否运行；
//! 运行中 → 该类目整体阻止 + UI 提示，**不做半清**。
//! 本阶段实现 `process`（目标进程探测）；`instance`（单实例互斥量）延至 P4-02。

pub mod process;

pub use process::running_matching;

/// 判定给定进程名（exe 基名，忽略大小写）中是否有正在运行的。
/// 命中任一 → 该类目应整体阻止。
pub fn any_running(exe_base_names: &[String]) -> bool {
    !running_matching(exe_base_names).is_empty()
}
