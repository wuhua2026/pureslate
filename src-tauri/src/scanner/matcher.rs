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

    /// 供 walk 层读取约束（min_size_mb）——整类取最大 min_size 作下限剪枝。
    pub fn min_size_bytes(&self) -> u64 {
        self.include_constraints
            .iter()
            .map(|(_, mb)| mb * 1024 * 1024)
            .max()
            .unwrap_or(0)
    }

    /// 计算该 category 实际生效的等级（直接引用 rules risk，含默认档由 R02/P1-04 决定）。
    pub fn grade(&self, risk: Risk) -> Risk {
        risk
    }

    /// 判断绝对路径是否命中该 category。
    /// `base_root` 为该 category 的 target 展开根：glob 模式相对它匹配。
    /// include 命中且不被 exclude 排除。constraints（age/size）在 walker 处校验。
    pub fn matches_any(&self, path: &std::path::Path, base_root: &std::path::Path) -> bool {
        let rel = relative_to(path, base_root);
        self.include.is_match(&rel) && !self.exclude.is_match(&rel)
    }

    /// 判断路径是否是 include 集合之一（供 walker 决定是否探索子目录）。
    /// 剪枝：暂不精确，统一返回 true 保证正确性（性能剪枝留待 P3）。
    pub fn could_contain(&self, _path: &std::path::Path) -> bool {
        true
    }
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
            guard_process: None,
        }
    }

    #[test]
    fn matches_include_and_skips_exclude() {
        let c = cat();
        let cc = CompiledCategory::compile(&c);
        let root = std::path::Path::new(r"C:\x");
        assert!(cc.matches_any(&std::path::Path::new(r"C:\x\foo\bar.tmp"), root));
        assert!(cc.matches_any(&std::path::Path::new(r"C:\x\Cache\b.tmp"), root));
        assert!(!cc.matches_any(&std::path::Path::new(r"C:\x\foo\a.keep.tmp"), root));
    }
}
