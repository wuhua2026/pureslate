//! 规则路径匹配（R01 matcher）。把 category 的 include/exclude glob 编译为
//! `globset::GlobSet`，判断命中与约束过滤。

use globset::{Glob, GlobSet};

use crate::rules::model::{Category, GlobRule, Risk};

/// 编译后的规则匹配器。一次编译复用多次匹配。
pub struct CompiledCategory {
    /// category id（用于产出 ScanItem）。
    pub category_id: String,
    /// include 译成 GlobSet（`**`/`*` 由 globset 支持）。
    include: GlobSet,
    /// exclude 译成 GlobSet。
    exclude: GlobSet,
    /// include 的 maxAgeDays / minSizeMB（一一对应 include，供按序核验）。
    include_constraints: Vec<(u32, u64)>, // (max_age_days, min_size_mb)
}

impl CompiledCategory {
    /// 编译 category。非法 glob 模式：编译失败时该条忽略（宁缺勿错，记日志）。
    pub fn compile(cat: &Category) -> Self {
        let mut inc = globset::GlobSetBuilder::new();
        let mut exc = globset::GlobSetBuilder::new();
        let mut constraints = Vec::new();
        for g in &cat.includes {
            append(&mut inc, g);
            constraints.push((g.max_age_days, g.min_size_mb));
        }
        for g in &cat.excludes {
            append(&mut exc, g);
        }
        Self {
            category_id: cat.id.clone(),
            include: inc.build().unwrap_or_default(),
            exclude: exc.build().unwrap_or_default(),
            include_constraints: constraints,
        }
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
        if self.exclude.is_match(&rel) {
            return None;
        }
        let idx = self.include.matches(&rel).into_iter().next()?;
        let (age_days, min_mb) = self.include_constraints.get(idx)?;
        Some(IncludeConstraint {
            max_age_days: *age_days,
            min_size_bytes: *min_mb * 1024 * 1024,
        })
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

/// 追加一个 glob 规则到 builder；非法模式忽略。
fn append(b: &mut globset::GlobSetBuilder, g: &GlobRule) {
    if let Ok(glob) = Glob::new(&g.pattern) {
        b.add(glob);
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
                    pattern: "a/*".into(),
                    recursive: true,
                    max_age_days: 0,
                    min_size_mb: 10,
                },
                GlobRule {
                    pattern: "b/*".into(),
                    recursive: true,
                    max_age_days: 0,
                    min_size_mb: 0,
                },
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
                    pattern: "a/*".into(),
                    recursive: true,
                    max_age_days: 7,
                    min_size_mb: 1,
                },
                GlobRule {
                    pattern: "b/*".into(),
                    recursive: true,
                    max_age_days: 0,
                    min_size_mb: 0,
                },
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
            targets: vec![Target {
                ty: TargetType::Path,
                value: "C:\\x".into(),
            }],
            includes: vec![
                GlobRule {
                    pattern: "**/*.tmp".into(),
                    recursive: true,
                    max_age_days: 7,
                    min_size_mb: 0,
                },
                GlobRule {
                    pattern: "Cache/**".into(),
                    recursive: true,
                    max_age_days: 0,
                    min_size_mb: 5,
                },
            ],
            excludes: vec![GlobRule {
                pattern: "**/*.keep.tmp".into(),
                recursive: true,
                max_age_days: 0,
                min_size_mb: 0,
            }],
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
}
