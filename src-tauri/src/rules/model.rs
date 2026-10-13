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
    RegValue,    // 注册表字符串值动态路径（R28，细节见 RegTargetSpec）
}

/// reg-value target 的注册表根键（R28，SPEC §4.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegHive {
    CurrentUser,  // HKCU
    LocalMachine, // HKLM
}

/// reg-value target 的 WOW64 视图（R28）。x64 进程显式选择，避免注册表重定向歧义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegView {
    #[default]
    Native, // 64 位视图（KEY_WOW64_64KEY）
    Wow64, // 32 位视图（KEY_WOW64_32KEY）
}

/// reg-value target 如何解释注册表值（R28）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegBase {
    #[default]
    Path, // 值本身即目标目录
    Parent, // 值是文件路径，取其父目录作为目标目录
}

/// reg-value target 的注册表定位信息（R28）。仅 `Target.ty == RegValue` 时存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegTargetSpec {
    pub hive: RegHive,
    /// 子键路径（反斜杠分隔）。
    pub key: String,
    /// 注册表值名。
    pub value_name: String,
    pub view: RegView,
    pub base: RegBase,
}

impl RegTargetSpec {
    /// 日志可读的根键名。
    pub fn hive_label(&self) -> &'static str {
        match self.hive {
            RegHive::CurrentUser => "HKCU",
            RegHive::LocalMachine => "HKLM",
        }
    }
}

/// 单个匹配目标集合起点。
#[derive(Debug, Clone)]
pub struct Target {
    pub ty: TargetType,
    pub value: String,
    /// 仅 ty == RegValue 时 Some；其它 target 类型恒为 None。
    pub reg: Option<RegTargetSpec>,
}

impl Target {
    /// env/path/knownFolder 三类传统 target 的构造器。
    pub fn simple(ty: TargetType, value: impl Into<String>) -> Self {
        Self {
            ty,
            value: value.into(),
            reg: None,
        }
    }

    /// reg-value target 构造器（路径不放在 value，全部来自注册表动态解析）。
    pub fn reg_value(spec: RegTargetSpec) -> Self {
        Self {
            ty: TargetType::RegValue,
            value: String::new(),
            reg: Some(spec),
        }
    }
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
    /// R28-2：路径段收紧——相对 target 根的段序号（0 起，含文件名段），
    /// 与 `segment_regex` 成对出现；glob 命中后该段必须整段匹配正则。
    pub segment_index: Option<u32>,
    /// R28-2：段正则源串（自动首尾锚定）；编译失败的规则在加载期被丢弃（fail-closed）。
    pub segment_regex: Option<String>,
}

impl GlobRule {
    /// 测试/构造快捷方式：无任何可选约束的 glob 规则（递归）。
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            recursive: true,
            max_age_days: 0,
            min_size_mb: 0,
            segment_index: None,
            segment_regex: None,
        }
    }
}

/// R28-2：编译路径段收紧正则并自动首尾锚定（规则里不用写 `^$`）。
/// 编译失败由调用方按 fail-closed 丢弃该条规则（loader/matcher 两处共用）。
pub fn compile_segment_regex(src: &str) -> Result<regex_lite::Regex, regex_lite::Error> {
    regex_lite::Regex::new(&format!("^(?:{src})$"))
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
