//! 规则路径匹配（R01 matcher）。把 category 的 include/exclude glob 编译为
//! `globset::GlobSet`，判断命中与约束过滤。

use globset::{Glob, GlobSet};
use regex_lite::Regex;

use crate::rules::model::{compile_segment_regex, Category, GlobRule, Risk};

/// 编译后的规则匹配器。一次编译复用多次匹配。
pub struct CompiledCategory {
    /// category id（用于产出 ScanItem）。
    pub category_id: String,
    /// include 译成 GlobSet（`**`/`*` 由 globset 支持）。
    include: GlobSet,
    /// exclude 译成 GlobSet。
    exclude: GlobSet,
    /// include 的 maxAgeDays / minSizeMB（与成功入集的 include 一一对应，供按序核验）。
    include_constraints: Vec<(u32, u64)>, // (max_age_days, min_size_mb)
    /// include 的段收紧（R28-2），与 include_constraints 平行对齐；None=无段约束。
    include_segments: Vec<Option<(u32, Regex)>>,
    /// include 的深度封顶（R28-3），与 include_constraints 平行对齐；None=不限。
    include_depths: Vec<Option<u16>>,
    /// exclude 的段收紧，与 exclude GlobSet 入集顺序平行对齐。
    exclude_segments: Vec<Option<(u32, Regex)>>,
    /// exclude 的深度封顶，与 exclude 入集顺序平行对齐。
    exclude_depths: Vec<Option<u16>>,
}

/// 单条规则编译后的附属守卫（段收紧 + 深度）。
struct CompiledGuard {
    segment: Option<(u32, Regex)>,
    depth: Option<u16>,
}

impl CompiledCategory {
    /// 编译 category。非法 glob/段正则：该条规则忽略（宁缺勿错，记日志；
    /// loader 加载期已做同样校验，这里是第二道 fail-closed）。
    pub fn compile(cat: &Category) -> Self {
        let mut inc = globset::GlobSetBuilder::new();
        let mut exc = globset::GlobSetBuilder::new();
        let mut constraints = Vec::new();
        let mut include_segments = Vec::new();
        let mut include_depths = Vec::new();
        let mut exclude_segments = Vec::new();
        let mut exclude_depths = Vec::new();
        for g in &cat.includes {
            let Some(guard) = append(&mut inc, g) else {
                continue;
            };
            constraints.push((g.max_age_days, g.min_size_mb));
            include_segments.push(guard.segment);
            include_depths.push(guard.depth);
        }
        for g in &cat.excludes {
            let Some(guard) = append(&mut exc, g) else {
                continue;
            };
            exclude_segments.push(guard.segment);
            exclude_depths.push(guard.depth);
        }
        Self {
            category_id: cat.id.clone(),
            include: inc.build().unwrap_or_default(),
            exclude: exc.build().unwrap_or_default(),
            include_constraints: constraints,
            include_segments,
            include_depths,
            exclude_segments,
            exclude_depths,
        }
    }

    /// R28-3：walk 遍历深度上限（相对 target 根，根直属文件=1）。
    /// 任一 include 不限深度则整体不限（None）；否则取最宽者（最大值），
    /// per-include 的精确裁尾在 `match_include` 内进行。
    pub fn traversal_max_depth(&self) -> Option<usize> {
        self.include_depths
            .iter()
            .map(|d| d.map(|n| n as usize))
            .try_fold(1usize, |acc, d| match d {
                None => Err(()),
                Some(n) => Ok(acc.max(n)),
            })
            .ok()
    }

    /// 供 walk 层读取剪枝下界——取各 include 的**最小** min_size（最宽松者）。
    /// H3（v0.1.4）：原实现取最大值会剪掉匹配较小 include 的文件；类目级剪枝
    /// 只做性能下界，per-include 精确校验在命中后由 `constraint_allows` 进行。
    pub fn min_size_bytes(&self) -> u64 {
        self.include_constraints
            .iter()
            .map(|(_, mb)| mb * 1024 * 1024)
            .min()
            .unwrap_or(0)
    }

    /// 计算该 category 实际生效的等级（直接引用 rules risk，含默认档由 R02/P1-04 决定）。
    pub fn grade(&self, risk: Risk) -> Risk {
        risk
    }

    /// 判断绝对路径是否命中该 category，并返回**命中的 include 约束**（H3 · v0.1.4）。
    /// `base_root` 为该 category 的 target 展开根：glob 模式相对它匹配。
    /// 返回 None = 不命中（或被 exclude 排除）；命中约束由 walker 按
    /// `constraint_allows` 做 per-include 校验（age/size）。
    pub fn match_include(
        &self,
        path: &std::path::Path,
        base_root: &std::path::Path,
    ) -> Option<IncludeConstraint> {
        let rel = relative_to(path, base_root);
        // exclude 同样支持段收紧与深度封顶：glob+段+深度都满足才算排除。
        let excluded = self.exclude.matches(&rel).into_iter().any(|i| {
            self.exclude_segments
                .get(i)
                .is_some_and(|s| segment_allows(&rel, s))
                && self
                    .exclude_depths
                    .get(i)
                    .copied()
                    .is_some_and(|d| depth_allows(&rel, d))
        });
        if excluded {
            return None;
        }
        // 多个 include 可能同时 glob 命中；取第一个段约束与深度都通过的（各数组与入集索引对齐）。
        for idx in self.include.matches(&rel) {
            let Some(segment) = self.include_segments.get(idx) else {
                continue;
            };
            if !segment_allows(&rel, segment) {
                continue;
            }
            let Some(depth) = self.include_depths.get(idx).copied() else {
                continue;
            };
            if !depth_allows(&rel, depth) {
                continue;
            }
            let (age_days, min_mb) = self.include_constraints.get(idx)?;
            return Some(IncludeConstraint {
                max_age_days: *age_days,
                min_size_bytes: *min_mb * 1024 * 1024,
            });
        }
        None
    }

    /// 判断绝对路径是否命中该 category（不取约束的旧语义包装）。
    pub fn matches_any(&self, path: &std::path::Path, base_root: &std::path::Path) -> bool {
        self.match_include(path, base_root).is_some()
    }

    /// 判断路径是否是 include 集合之一（供 walker 决定是否探索子目录）。
    /// 剪枝：暂不精确，统一返回 true 保证正确性（性能剪枝留待 P3）。
    pub fn could_contain(&self, _path: &std::path::Path) -> bool {
        true
    }
}

/// 命中的 include 约束（per-include，v0.1.4 H3）。
#[derive(Debug, Clone, Copy)]
pub struct IncludeConstraint {
    /// maxAgeDays（0 = 不限）：文件 mtime 距今须 ≥ N 天——老文件才入清理候选，
    /// 保护最近写入/修改的文件。
    pub max_age_days: u32,
    /// minSizeMB（0 = 不限）：文件大小下限。
    pub min_size_bytes: u64,
}

/// per-include 约束校验（H3 · v0.1.4）。`now_ms` 注入供测试。
/// 返回 false = 不满足清理条件（宁缺勿错：mtime 异常/不可得一律排除——
/// 无法证明"足够老"就不该进直清候选）。
pub fn constraint_allows(
    c: &IncludeConstraint,
    size_bytes: u64,
    mtime_ms: i64,
    now_ms: i64,
) -> bool {
    if c.min_size_bytes > 0 && size_bytes < c.min_size_bytes {
        return false;
    }
    if c.max_age_days > 0 {
        // mtime 在未来或 epoch 前等异常形态：年龄按 0 处理 → maxAge>0 时排除。
        let age_ms = if mtime_ms <= 0 { 0 } else { now_ms - mtime_ms };
        if age_ms < (c.max_age_days as i64) * 86_400_000 {
            return false;
        }
    }
    true
}

/// 追加一个 glob 规则到 builder；成功入集返回其守卫（段收紧+深度）。
/// glob 或段正则编译失败 → 不入集返回 None（调用方约束数组与其对齐，
/// 修复了此前坏 glob 静默跳过导致约束索引错位的潜在问题）。
fn append(b: &mut globset::GlobSetBuilder, g: &GlobRule) -> Option<CompiledGuard> {
    let glob = Glob::new(&g.pattern).ok()?;
    let segment = match (g.segment_regex.as_ref(), g.segment_index) {
        (Some(src), Some(idx)) => match compile_segment_regex(src) {
            Ok(re) => Some((idx, re)),
            Err(e) => {
                eprintln!("[matcher] 丢弃段正则编译失败的规则 `{}`: {e}", g.pattern);
                return None;
            }
        },
        _ => None,
    };
    b.add(glob);
    Some(CompiledGuard {
        segment,
        depth: g.max_depth,
    })
}

/// R28-2：段收紧判定。段不存在（index 越界）= 不命中（fail-closed）。
fn segment_allows(rel: &str, segment: &Option<(u32, Regex)>) -> bool {
    match segment {
        None => true,
        Some((idx, re)) => rel
            .split('/')
            .nth(*idx as usize)
            .is_some_and(|seg| re.is_match(seg)),
    }
}

/// R28-3：深度判定。rel 的路径段数（文件直属根=1）不得超过 depth。
fn depth_allows(rel: &str, depth: Option<u16>) -> bool {
    match depth {
        None => true,
        Some(d) => rel.split('/').count() <= d as usize,
    }
}

/// 计算 `path` 相对 `base_root` 的路径，统一 `/` 分隔符（globset 匹配用）。
/// 若 path 不在 base_root 下（理论上不会发生），退化为 path 文件名。
fn relative_to(path: &std::path::Path, base_root: &std::path::Path) -> String {
    // 统一大小写无关：Windows 路径不敏感，glob 匹配时二者都低比较即可（globset 默认逐字符）。
    match path.strip_prefix(base_root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::model::{Disposition, GlobRule, Target, TargetType};

    #[test]
    fn constraint_allows_age_semantics() {
        // H3（v0.1.4）：maxAgeDays = 文件至少 N 天未修改（老文件才入清理候选）。
        let now = 1_800_000_000_000i64;
        let day = 86_400_000i64;
        let c = IncludeConstraint {
            max_age_days: 7,
            min_size_bytes: 0,
        };
        // 刚写入（3 天前）→ 排除（保护最近文件，审查 H3 的核心诉求）。
        assert!(!constraint_allows(&c, 100, now - 3 * day, now));
        // 8 天前 → 命中。
        assert!(constraint_allows(&c, 100, now - 8 * day, now));
        // 恰好 7 天 → 命中（≥ 语义）。
        assert!(constraint_allows(&c, 100, now - 7 * day, now));
        // mtime 异常（未来/epoch 前）→ 排除（宁缺勿错）。
        assert!(!constraint_allows(&c, 100, now + day, now));
        assert!(!constraint_allows(&c, 100, -1, now));
        // maxAgeDays=0 → 不限（任何 mtime 命中）。
        let c0 = IncludeConstraint {
            max_age_days: 0,
            min_size_bytes: 0,
        };
        assert!(constraint_allows(&c0, 100, now, now));
    }

    #[test]
    fn constraint_allows_min_size() {
        let c = IncludeConstraint {
            max_age_days: 0,
            min_size_bytes: 1024,
        };
        assert!(!constraint_allows(&c, 1023, 100, 100));
        assert!(constraint_allows(&c, 1024, 100, 100));
    }

    #[test]
    fn min_size_prune_takes_min_not_max() {
        // H3 回归：类目级剪枝下界取最小者——原取最大值会剪掉匹配较小 include 的文件。
        let cat = Category {
            includes: vec![
                GlobRule {
                    min_size_mb: 10,
                    ..GlobRule::new("a/*")
                },
                GlobRule::new("b/*"),
            ],
            ..cat()
        };
        let compiled = CompiledCategory::compile(&cat);
        assert_eq!(compiled.min_size_bytes(), 0, "剪枝下界应取最小（最宽松）");
    }

    #[test]
    fn match_include_returns_per_include_constraint() {
        let cat = Category {
            includes: vec![
                GlobRule {
                    max_age_days: 7,
                    min_size_mb: 1,
                    ..GlobRule::new("a/*")
                },
                GlobRule::new("b/*"),
            ],
            ..cat()
        };
        let compiled = CompiledCategory::compile(&cat);
        let base = std::path::Path::new("C:\\t");
        let ca = compiled
            .match_include(std::path::Path::new("C:\\t\\a\\x.tmp"), base)
            .unwrap();
        assert_eq!(ca.max_age_days, 7);
        assert_eq!(ca.min_size_bytes, 1024 * 1024);
        let cb = compiled
            .match_include(std::path::Path::new("C:\\t\\b\\y.tmp"), base)
            .unwrap();
        assert_eq!(cb.max_age_days, 0);
        // exclude 优先。
        let c2 = Category {
            excludes: vec![cat.includes[1].clone()],
            ..cat
        };
        let compiled2 = CompiledCategory::compile(&c2);
        assert!(compiled2
            .match_include(std::path::Path::new("C:\\t\\b\\y.tmp"), base)
            .is_none());
    }

    fn cat() -> Category {
        Category {
            id: "t".into(),
            label: "t".into(),
            risk: Risk::Green,
            disposition: Disposition::Direct,
            description: None,
            targets: vec![Target::simple(TargetType::Path, r"C:\x")],
            includes: vec![
                GlobRule {
                    max_age_days: 7,
                    ..GlobRule::new("**/*.tmp")
                },
                GlobRule {
                    min_size_mb: 5,
                    ..GlobRule::new("Cache/**")
                },
            ],
            excludes: vec![GlobRule::new("**/*.keep.tmp")],
            guard_processes: Vec::new(),
        }
    }

    #[test]
    fn matches_include_and_skips_exclude() {
        let c = cat();
        let cc = CompiledCategory::compile(&c);
        let root = std::path::Path::new(r"C:\x");
        assert!(cc.matches_any(std::path::Path::new(r"C:\x\foo\bar.tmp"), root));
        assert!(cc.matches_any(std::path::Path::new(r"C:\x\Cache\b.tmp"), root));
        assert!(!cc.matches_any(std::path::Path::new(r"C:\x\foo\a.keep.tmp"), root));
    }

    // ---- R28-2 segment-regex ----

    fn account_category(pattern: &str, regex: &str, index: u32) -> Category {
        Category {
            includes: vec![GlobRule {
                segment_index: Some(index),
                segment_regex: Some(regex.into()),
                ..GlobRule::new(pattern)
            }],
            ..cat()
        }
    }

    #[test]
    fn segment_regex_passes_only_well_formed_segment() {
        // rel: accounts/<id>/cache/<file>，收紧第 1 段为 6-12 位纯数字。
        let cc =
            CompiledCategory::compile(&account_category("accounts/*/cache/*", "[0-9]{6,12}", 1));
        let root = std::path::Path::new(r"C:\x");
        assert!(
            cc.matches_any(std::path::Path::new(r"C:\x\accounts\123456\cache\f1"), root),
            "形态正确的账号目录应命中"
        );
        assert!(
            cc.matches_any(
                std::path::Path::new(r"C:\x\accounts\123456789012\cache\f2"),
                root
            ),
            "12 位数字仍在区间内"
        );
        assert!(
            !cc.matches_any(std::path::Path::new(r"C:\x\accounts\evil\cache\f3"), root),
            "非数字账号目录必须零命中"
        );
        assert!(
            !cc.matches_any(std::path::Path::new(r"C:\x\accounts\12345\cache\f4"), root),
            "5 位（少于 6）不命中"
        );
        assert!(
            !cc.matches_any(
                std::path::Path::new(r"C:\x\accounts\a123456\cache\f5"),
                root
            ),
            "自动首尾锚定：夹带字母不命中"
        );
    }

    #[test]
    fn segment_index_out_of_range_never_matches() {
        let cc = CompiledCategory::compile(&account_category("*/*", "[0-9]+", 5));
        let root = std::path::Path::new(r"C:\x");
        assert!(
            !cc.matches_any(std::path::Path::new(r"C:\x\a\123"), root),
            "段越界 = 不命中（fail-closed）"
        );
    }

    #[test]
    fn segment_regex_on_exclude_must_also_match() {
        // exclude 的段约束不满足时，不应排除该文件。
        let c = Category {
            includes: vec![GlobRule::new("accounts/*/cache/*")],
            excludes: vec![GlobRule {
                segment_index: Some(1),
                segment_regex: Some("[0-9]{6,12}".into()),
                ..GlobRule::new("accounts/*/cache/secret.tmp")
            }],
            ..cat()
        };
        let cc = CompiledCategory::compile(&c);
        let root = std::path::Path::new(r"C:\x");
        assert!(
            cc.matches_any(
                std::path::Path::new(r"C:\x\accounts\evil\cache\secret.tmp"),
                root
            ),
            "账号段不符排除正则 → 该文件不被排除"
        );
        assert!(
            !cc.matches_any(
                std::path::Path::new(r"C:\x\accounts\123456\cache\secret.tmp"),
                root
            ),
            "账号段命中排除正则 → 排除生效"
        );
    }

    // ---- R28-3 max-depth ----

    #[test]
    fn max_depth_trims_include_match() {
        let c = Category {
            includes: vec![GlobRule {
                max_depth: Some(2),
                ..GlobRule::new("deep/**")
            }],
            ..cat()
        };
        let cc = CompiledCategory::compile(&c);
        let root = std::path::Path::new(r"C:\t");
        assert!(
            cc.matches_any(std::path::Path::new(r"C:\t\deep\f1"), root),
            "2 段（=depth 2）应命中"
        );
        assert!(
            !cc.matches_any(std::path::Path::new(r"C:\t\deep\a\f2"), root),
            "3 段（>depth 2）不命中"
        );
        assert_eq!(cc.traversal_max_depth(), Some(2));
    }

    #[test]
    fn traversal_depth_none_when_any_include_unbounded() {
        let c = Category {
            includes: vec![
                GlobRule {
                    max_depth: Some(2),
                    ..GlobRule::new("deep/**")
                },
                GlobRule::new("other/**"),
            ],
            ..cat()
        };
        let cc = CompiledCategory::compile(&c);
        assert_eq!(
            cc.traversal_max_depth(),
            None,
            "存在不限深度的 include → 不剪"
        );
    }

    #[test]
    fn traversal_depth_takes_max_when_all_bounded() {
        let c = Category {
            includes: vec![
                GlobRule {
                    max_depth: Some(1),
                    ..GlobRule::new("a/*")
                },
                GlobRule {
                    max_depth: Some(3),
                    ..GlobRule::new("b/**")
                },
            ],
            ..cat()
        };
        let cc = CompiledCategory::compile(&c);
        assert_eq!(cc.traversal_max_depth(), Some(3), "遍历深度取最宽者");
    }
}
