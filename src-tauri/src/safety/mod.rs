//! 安全模块：分级判定 + 白名单强制排除（SAFETY §1、§2）。
//! R02 本阶段（P1-04）交付分级判定与"常量根 + whitelist.xml"路径级白名单；
//! §2.5 运行中进程句柄判定推迟（见 whitelist.rs 头注释）。

pub mod grade;
pub mod whitelist;

pub use grade::{from_declaration, from_risk, undetermined_count};
pub use whitelist::{
    is_whitelisted, load_from_dir, parse_whitelist_xml, set_xml_roots, WhitelistError,
};
