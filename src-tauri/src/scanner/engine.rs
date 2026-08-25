//! 扫描编排（R03 engine）。SPEC §6.1：
//! profile（维度开关）→ 选择可扫 category → 逐类目展开 target → walk/mft 遍历匹配
//! → 聚合 CategoryAggregate + FoundBytes → 进度回调（供事件节流推送）。
//!
//! 全程只读（红线 #1：扫描路径下零写盘）；白名单过滤在 walk/mft 内部强制生效。
//! startup 维度（R07 注册表枚举）与 dup/large（R06/R05）属后续 Phase，本任务
//! 仅处理基于规则文件的文件类目目扫描；未注册的维度自动落空但不报错。

use crate::contract::{
    CategoryAggregate, Disposition, FoundBytes, ScanDimension, ScanItem, ScanPhase,
};
use crate::rules::model::RuleSetTable;
use crate::safety::grade;

use super::aggregate::{category_aggregate, found_bytes};
use super::expand::{expand_target, volume_of};
use super::matcher::CompiledCategory;
use super::walk::CancelToken;
use super::walk_matching;

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
        let compiled = CompiledCategory::compile(cat);

        let mut cat_items: Vec<ScanItem> = Vec::new();
        let mut current_path = String::new();
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

/// 规则去向 → 对外契约去向。green 仅 direct/recycle（loader 已校验），unreachable 兜底。
fn contract_disposition(d: crate::rules::model::Disposition) -> Disposition {
    match d {
        crate::rules::model::Disposition::Direct => Disposition::Direct,
        crate::rules::model::Disposition::Recycle => Disposition::Recycle,
        crate::rules::model::Disposition::Quarantine => Disposition::Quarantine,
    }
}
