//! 规则引擎 R01：模型 + 加载 + 匹配 + 缓存。
//! P1-01 交付 model/loader/cache；P1-02 交付 matcher（walk 遍历）。

pub mod loader;
pub mod model;

pub use loader::{parse_ruleset, RuleLoader};
pub use model::{Category, Disposition, GlobRule, Risk, RuleSetTable, Ruleset, Target, TargetType};
