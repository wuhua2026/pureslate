//! 安全分级（R02 · SAFETY §1）。
//!
//! 分级以规则 XML 的 `risk` 声明为准；**无法判定时保守默认 🔴** 并记日志。
//! 规则模型侧的 `Risk` 由 R01 loader 保证为合法枚举，但调用方仍可能拿到"未声明/非法"
//! 的来源（例如后续动态规则、占位数据），因此统一收敛到 `from_declaration`。
//! 去向约束（green 仅 direct/recycle）在 `rules::Category::is_valid_config` 校验，此处不重复。

use crate::contract::Grade;
use crate::rules::model::Risk;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 无法判定分级的累计次数（供测试断言与后续统计）。
static UNDETERMINED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// 将规则声明的风险映射为对外暴露的清理分级。
pub fn from_risk(risk: Risk) -> Grade {
    match risk {
        Risk::Green => Grade::Green,
        Risk::Yellow => Grade::Yellow,
        Risk::Red => Grade::Red,
    }
}

/// 依据规则声明判定分级。`risk` 为 `None`（未声明/非法/来源缺失）时保守默认 🔴 并记日志。
pub fn from_declaration(risk: Option<Risk>) -> Grade {
    match risk {
        Some(r) => from_risk(r),
        None => {
            UNDETERMINED_COUNT.fetch_add(1, Ordering::Relaxed);
            // TODO(R09): 接入审计日志后替换为结构化记录。
            eprintln!("[grade] risk 无法判定，保守默认 🔴");
            Grade::Red
        }
    }
}

/// 无法判定分级的累计次数（供测试断言与后续统计）。
pub fn undetermined_count() -> usize {
    UNDETERMINED_COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_three_risk_levels() {
        assert_eq!(from_risk(Risk::Green), Grade::Green);
        assert_eq!(from_risk(Risk::Yellow), Grade::Yellow);
        assert_eq!(from_risk(Risk::Red), Grade::Red);
    }

    #[test]
    fn declaration_none_defaults_to_red() {
        // 三档来源照传。
        assert_eq!(from_declaration(Some(Risk::Yellow)), Grade::Yellow);
        // 无法判定 → 默认 🔴。
        assert_eq!(from_declaration(None), Grade::Red);
    }

    #[test]
    fn undetermined_is_counted() {
        let before = undetermined_count();
        let _ = from_declaration(None);
        assert!(undetermined_count() > before);
    }
}
