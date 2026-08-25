//! 扫描编排（R03 engine）。SPEC §6.1：
//! profile（维度开关）→ 选择可扫 category → 逐类目展开 target → walk/mft 遍历匹配
//! → 聚合 CategoryAggregate + FoundBytes → 进度回调（供事件节流推送）。
//!
//! 全程只读（红线 #1：扫描路径下零写盘）；白名单过滤在 walk/mft 内部强制生效。
//! startup 维度（R07 注册表枚举）与 dup/large（R06/R05）属后续 Phase，本任务
//! 仅处理基于规则文件的文件类目目扫描；未注册的维度自动落空但不报错。

use crate::contract::{
    CategoryAggregate, Disposition, FoundBytes, Grade, ScanDimension, ScanItem, ScanPhase,
};
use crate::rules::model::RuleSetTable;
use crate::safety::grade;

use super::aggregate::{category_aggregate, found_bytes};
use super::expand::{expand_target, volume_of};
use super::matcher::CompiledCategory;
use super::walk::CancelToken;
use super::walk_matching;
use crate::cleaner::dup::{collect_candidates, find_duplicates, DupCandidate};

/// 扫描进度回调：`(阶段, 已完成类目, 类目总数, 当前类目路径, 实时累计字节)`。
pub type ProgressCb<'a> = dyn FnMut(ScanPhase, usize, usize, String, FoundBytes) + 'a;

/// 一次扫描的完整产出。
#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub items: Vec<ScanItem>,
    pub aggregates: Vec<CategoryAggregate>,
    pub found: FoundBytes,
    /// 结果归属卷（如 `C:`）。
    pub volume: String,
}

/// 维度 → 规则类目 id 前缀。规则包按此约定投递白名单之外的类目。
fn dim_prefix(d: ScanDimension) -> &'static str {
    match d {
        ScanDimension::Temp => "temp.",
        ScanDimension::Large => "large.",
        ScanDimension::Dup => "dup.",
        ScanDimension::Cache => "cache.",
        ScanDimension::Startup => "startup.",
        ScanDimension::Privacy => "privacy.",
    }
}

/// profile 中显式启用的维度集合。
fn enabled_dimensions(profile: &crate::contract::ScanProfile) -> Vec<ScanDimension> {
    profile
        .dimensions
        .iter()
        .filter(|(_, &on)| on)
        .map(|(&d, _)| d)
        .collect()
}

/// 执行扫描编排。
///
/// `cancel` 贯穿遍历；`progress` 每处理完一个类目或聚合阶段回调一次，由调用方节流推送。
/// 任意 target/category 的软失败均跳过并继续，不中断整体扫描。
pub fn run_scan(
    table: &RuleSetTable,
    profile: &crate::contract::ScanProfile,
    cancel: &CancelToken,
    progress: &mut ProgressCb,
) -> ScanOutcome {
    let dims = enabled_dimensions(profile);
    let prefixes: Vec<&str> = dims.iter().map(|d| dim_prefix(*d)).collect();

    // 选择启用的类目（按 id 前缀命中任一启用维度）。
    let mut cats: Vec<(String, crate::rules::model::Category)> = table
        .by_id
        .iter()
        .filter(|(id, _)| {
            let prefix = id.split('.').next().unwrap_or("").to_string() + ".";
            prefixes.contains(&prefix.as_str())
        })
        .map(|(id, (_, cat))| (id.clone(), cat.clone()))
        .collect();
    // 确定性顺序：按类目 id 字典序（可复现）。
    cats.sort_by(|a, b| a.0.cmp(&b.0));

    let total = cats.len();
    let mut out = ScanOutcome::default();
    let mut done = 0usize;

    for (_, cat) in &cats {
        if cancel.is_cancelled() {
            break;
        }
        let grade = grade::from_risk(cat.risk);
        let disposition = contract_disposition(cat.disposition);

        // dup 维度走内容判重引擎（不是规则匹配）：按 target 范围收集全部文件 → 三级过滤 → 回填 dupGroup。
        let is_dup = cat.id.starts_with("dup.");
        let mut cat_items: Vec<ScanItem> = Vec::new();
        let mut current_path = String::new();

        if is_dup {
            cat_items = scan_dup_category(cat, grade, disposition, cancel, &mut current_path);
            if out.volume.is_empty() && !current_path.is_empty() {
                if let Some(v) = volume_of(std::path::Path::new(&current_path)) {
                    out.volume = v;
                }
            }
        } else {
            let compiled = CompiledCategory::compile(cat);
            for target in &cat.targets {
                if cancel.is_cancelled() {
                    break;
                }
                let Some(start) = expand_target(target) else {
                    continue;
                };
                if out.volume.is_empty() {
                    if let Some(v) = volume_of(&start) {
                        out.volume = v;
                    }
                }
                current_path = start.to_string_lossy().into_owned();
                cat_items.extend(walk_matching(
                    &start,
                    &compiled,
                    grade,
                    disposition,
                    cancel,
                    None,
                ));
            }
        }

        // 累加进全局结果（按类目边界推进，聚合阶段单次汇总）。
        let cat_count = cat_items.len();
        out.items.append(&mut cat_items);
        if cat_count > 0 {
            let cat_slice = &out.items[out.items.len() - cat_count..];
            let agg = category_aggregate(
                &cat.id,
                &cat.label,
                grade,
                disposition,
                cat.description.as_deref().unwrap_or("匹配规则"),
                cat_slice,
            );
            out.aggregates.push(agg);
        }
        done += 1;
        progress(
            ScanPhase::Walking,
            done,
            total,
            current_path,
            found_bytes(&out.items),
        );
    }

    out.found = found_bytes(&out.items);
    out.volume = if out.volume.is_empty() {
        "C:".into()
    } else {
        out.volume
    };
    // 聚合阶段：一次性收尾，占比体现为总进度。
    if done < total && cancel.is_cancelled() {
        progress(
            ScanPhase::Walking,
            done,
            total,
            String::new(),
            out.found.clone(),
        );
    }
    progress(
        ScanPhase::Aggregating,
        total,
        total,
        String::new(),
        out.found.clone(),
    );
    out
}

/// dup 维度扫描：对类目全部 target 范围收集候选文件 → 三级过滤判重 → 产出带 `dup_group` 的 ScanItem。
///
/// 保留语义：同内容组内 **mtime 最早者（keeper）保留**，仅将冗余副本产出为可清理项，
/// 从根上保证 keeper 永不是清理目标。顺带回填 `current_path`（用于扫描进度展示）。
/// 全程只读（红线 #1），取消贯穿。
fn scan_dup_category(
    cat: &crate::rules::model::Category,
    grade: Grade,
    disposition: Disposition,
    cancel: &CancelToken,
    current_path: &mut String,
) -> Vec<ScanItem> {
    // ① 收集全部 target 下的候选文件（白名单过滤、跳过空文件）。
    let mut cands: Vec<DupCandidate> = Vec::new();
    for target in &cat.targets {
        let Some(start) = expand_target(target) else {
            continue;
        };
        current_path.clone_from(&start.to_string_lossy().into_owned());
        if cancel.is_cancelled() {
            break;
        }
        cands.extend(collect_candidates(&start, cancel));
    }

    // ② 三级过滤判重 → 组。
    let groups = find_duplicates(&cands, cancel);

    // ③ 仅产出每组冗余副本（keeper 不出现），回填 dup_group。
    let mut items: Vec<ScanItem> = Vec::new();
    let cand_by_path: std::collections::HashMap<&std::path::Path, &DupCandidate> =
        cands.iter().map(|c| (c.path.as_path(), c)).collect();
    for g in &groups {
        if cancel.is_cancelled() {
            break;
        }
        let keeper = g.keeper.to_string_lossy().into_owned();
        for cpath in &g.candidates {
            let Some(c) = cand_by_path.get(cpath.as_path()) else {
                continue;
            };
            let label = cpath
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unknown>".into());
            items.push(ScanItem {
                id: dup_stable_id(&cat.id, cpath),
                category_id: cat.id.clone(),
                label,
                path: cpath.to_string_lossy().into_owned(),
                size_bytes: c.size,
                grade,
                disposition,
                reason: format!("与“{keeper}”内容相同，保留最早一份，副本可入隔离区 14 天可还原"),
                mtime: Some(c.mtime_ms),
                atime: None,
                dup_group: Some(g.id.clone()),
            });
        }
    }
    // 确定性顺序：按路径字典序（可复现）。
    items.sort_by(|a, b| a.path.cmp(&b.path));
    items
}

/// 由 category_id + path 生成稳定条目 id（16 位十六进制，对齐 `walk::stable_id` 语义）。
fn dup_stable_id(category_id: &str, path: &std::path::Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    category_id.hash(&mut h);
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 规则去向 → 对外契约去向。green 仅 direct/recycle（loader 已校验），unreachable 兜底。
fn contract_disposition(d: crate::rules::model::Disposition) -> Disposition {
    match d {
        crate::rules::model::Disposition::Direct => Disposition::Direct,
        crate::rules::model::Disposition::Recycle => Disposition::Recycle,
        crate::rules::model::Disposition::Quarantine => Disposition::Quarantine,
    }
}
