//! 规则模型（R01）。映射 SPEC §4.1 的 rules XML schema。

use serde::Serialize;

/// 风险分级。与 `contract::Grade` 语义一致；此处为规则侧元数据独立定义。
///
/// `Default` 为 `Red`（SAFETY §1：无法判定时默认 🔴，保守原则）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Green,
    Yellow,
    #[default]
    Red,
}

/// 去向预定义。green 只允许 direct/recycle，黄色本阶段属 quarantine。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    #[default]
    Direct,
    Recycle,
    Quarantine,
}

/// 目标类型：如何把 target 展开为实际路径集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Env,         // 环境变量，value 形如 %TEMP%
    Path,        // 绝对路径
    KnownFolder, // Known Folder API（P1-02 展开）
}

/// 单个匹配目标集合起点。
#[derive(Debug, Clone)]
pub struct Target {
    pub ty: TargetType,
    pub value: String,
}

/// 包含/排除 glob 规则。
#[derive(Debug, Clone)]
pub struct GlobRule {
    pub pattern: String,
    pub recursive: bool,
    /// 天数；0 表示不限。仅对 include 生效。
    pub max_age_days: u32,
    /// 可选最小大小 (MB)；0 表示不限。
    pub min_size_mb: u64,
}

/// 一条清理类目。解析失败/字段非法 → 整类丢弃并记日志。
#[derive(Debug, Clone)]
pub struct Category {
    pub id: String,
    pub label: String,
    pub risk: Risk,
    pub disposition: Disposition,
    pub description: Option<String>,
    pub targets: Vec<Target>,
    pub includes: Vec<GlobRule>,
    pub excludes: Vec<GlobRule>,
    /// 进程守卫列表（v0.1.4 修复 H2：多 `<guard>` 声明全部生效，此前后者覆盖前者）；
    /// 空 = 无守卫。
    pub guard_processes: Vec<String>,
}

impl Category {
    /// green 只允许 direct/recycle；否则非法。
    pub fn is_valid_config(&self) -> bool {
        matches!(
            (self.risk, self.disposition),
            (Risk::Green, Disposition::Direct) | (Risk::Green, Disposition::Recycle)
        ) || self.risk != Risk::Green
    }
}

/// 一个规则包（对应一个 XML 文件）。
#[derive(Debug, Clone)]
pub struct Ruleset {
    pub id: String,
    pub version: u32,
    pub lang: String,
    pub categories: Vec<Category>,
}

/// 来源文件元信息（装载/缓存判定用，非序列化到产物）。
#[derive(Debug, Clone)]
pub struct RulesetSource {
    pub ruleset: Ruleset,
    /// 来源文件绝对路径。
    pub source_path: String,
    /// 加载时刻的 mtime（可秒级精度；用于缓存失效判定）。
    pub mtime_secs: i64,
}

/// 合并后的可见规则集合。多 ruleset 同 id category 后加载覆盖先加载。
#[derive(Debug, Clone, Default)]
pub struct RuleSetTable {
    /// category_id -> (ruleset_id, category)
    pub by_id: std::collections::HashMap<String, (String, Category)>,
}
