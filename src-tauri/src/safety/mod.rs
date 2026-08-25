//! 安全模块：白名单强制排除（SAFETY §2）。R02 完整锁定在 P1-04。

pub mod whitelist;

pub use whitelist::is_whitelisted;
