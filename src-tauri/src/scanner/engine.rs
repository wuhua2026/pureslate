//! 扫描编排（R03 engine）。SPEC §6.1：
//! profile（维度开关）→ 选择可扫 category → 逐类目展开 target → walk/mft 遍历匹配
//! → 聚合 CategoryAggregate + FoundBytes → 进度回调（供事件节流推送）。
//!
//! 全程只读（红线 #1：扫描路径下零写盘）；白名单过滤在 walk/mft 内部强制生效。
//! startup 维度（R07 注册表枚举）与 dup/large（R06/R05）属后续 Phase，本任务
//! 仅处理基于规则文件的文件类目目扫描；未注册的维度自动落空但不报错。

use std::path::{Path, PathBuf};

use crate::contract::{
    CategoryAggregate, Disposition, FoundBytes, Grade, ScanDimension, ScanItem, ScanPhase,
};
use crate::rules::model::RuleSetTable;
use crate::safety::grade;

use super::aggregate::{category_aggregate, found_bytes};
use super::expand::{expand_target, volume_of};
use super::matcher::CompiledCategory;
use super::walk::{parallel_walk_stats, CancelToken};
use super::walk_matching;
use crate::cleaner::dup::{collect_candidates, find_duplicates, DupCandidate};

/// R05 大文件阈值（SPEC §3：large = ≥500MB）。
pub const LARGE_MIN_BYTES: u64 = 500 * 1024 * 1024;

/// R30 大文件类目 id（**审阅型，永不进入清理方案/清理执行**）。
///
/// 真机反馈（v0.2.0，165.8GB/121 项个人文件被整卷扫出）：≥500MB 的全盘条目
/// 绝大多数是用户主动存放的资料（视频/数据集/虚拟机镜像），不是垃圾；
/// 一键批量清理个人大文件违背"宁漏勿错"。故 large 只负责**发现**：
/// - items 保留（大文件页浏览，支持"在资源管理器中显示"）；
/// - 不生成 aggregate、不进报告 A/B 方案；
/// - 该 id 永不注册进规则表，resolve_targets 的 fail-closed 即后端兜底，
///   即使前端陈旧/有 bug 也无法经本应用删除个人大文件。
pub const LARGE_REVIEW_CATEGORY: &str = "large.file";

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
            let mut starts: Vec<PathBuf> = Vec::new();
            for target in &cat.targets {
                if let Some(start) = expand_target(target) {
                    starts.push(start);
                }
            }
            // v0.1.5 反馈 1b：temp.user 追加非系统固定盘根下的垃圾目录（代码级枚举——
            // 静态 XML 无法表达"每个固定盘根"，且会产生大量无效 walkdir 错误流）。
            if cat.id == "temp.user" {
                starts.extend(super::expand::multi_drive_temp_roots());
            }
            for start in &starts {
                if cancel.is_cancelled() {
                    break;
                }
                if out.volume.is_empty() {
                    if let Some(v) = volume_of(start) {
                        out.volume = v;
                    }
                }
                current_path = start.to_string_lossy().into_owned();
                cat_items.extend(walk_matching(
                    start,
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

    // v0.1.5 反馈 1a：volume 语义扩为"覆盖卷串"——全部固定盘（如 "C:+D:"）；
    // 单盘机器与旧行为一致（"C:"）。消费面已核：仅前端展示/断言非空，后端不解析该串。
    out.volume = {
        let vols: Vec<String> = crate::quarantine::fixed_drive_roots()
            .iter()
            .map(|r| {
                r.to_string_lossy()
                    .trim_end_matches([':', '\\'])
                    .to_string()
            })
            .collect();
        if vols.is_empty() {
            "C:".to_string()
        } else {
            vols.join("+")
        }
    };

    // large 维度（R05/R30）：**全部固定盘**逐卷并行遍历（v0.1.5 反馈 1a：原仅扫首个
    // target 所在卷，其他盘大文件完全不进结果）。逐卷推进 progress（currentPath=卷根）。
    // R30：large 为**审阅型**类目——items 保留供大文件页浏览，但不生成 aggregate、
    // 不进报告 A/B 清理方案（个人大文件不可一键清理），且不计入"可清理发现量"。
    if dims.contains(&ScanDimension::Large) && !cancel.is_cancelled() {
        for vol_root in crate::quarantine::fixed_drive_roots() {
            if cancel.is_cancelled() {
                break;
            }
            scan_large_items(&vol_root, LARGE_MIN_BYTES, cancel)
                .into_iter()
                .for_each(|it| out.items.push(it));
            progress(
                ScanPhase::Walking,
                done,
                total,
                vol_root.to_string_lossy().into_owned(),
                found_bytes(&out.items),
            );
        }
    }

    out.found = found_bytes(&out.items);
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
                id: stable_item_id(&cat.id, cpath),
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

/// large 维度扫描（R05/R30）：对卷根做一次并行全盘遍历，产出 ≥large_min 的条目。
///
/// R30 定位（SAFETY §1 宁漏勿错）：大文件是**审阅型**条目——多为用户主动存放的
/// 个人资料，不是垃圾。只供大文件页发现/浏览（可"在资源管理器中显示"），
/// **不提供应用内删除出口**：grade 仅作风险提示，永不进入清理方案；
/// disposition 字段保留 quarantine 仅为契约兼容，实际无执行路径
/// （id 不注册规则表，resolve_targets fail-closed）。
/// 顺序：大小降序、同大小按路径字典序（确定性，与 Files 页默认排序一致）。
pub fn scan_large_items(vol_root: &Path, large_min: u64, cancel: &CancelToken) -> Vec<ScanItem> {
    let stats = parallel_walk_stats(vol_root, large_min, true, cancel);
    let mut items: Vec<ScanItem> = stats
        .large
        .into_iter()
        .map(|f| ScanItem {
            id: stable_item_id(LARGE_REVIEW_CATEGORY, &f.path),
            category_id: LARGE_REVIEW_CATEGORY.into(),
            label: "大文件".into(),
            path: f.path.to_string_lossy().into_owned(),
            size_bytes: f.size,
            grade: Grade::Yellow,
            disposition: Disposition::Quarantine,
            reason: format!(
                "≥{} 的个人大文件，仅供查阅；PureSlate 不提供大文件删除，请在资源管理器中自行确认处理",
                human_bytes(large_min)
            ),
            mtime: f.mtime_ms,
            atime: f.atime_ms,
            dup_group: None,
        })
        .collect();
    items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then(a.path.cmp(&b.path)));
    items
}

/// 字节数人类可读（reason 文案用）。
fn human_bytes(n: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    if n >= GB {
        format!("{:.1}GB", n as f64 / GB as f64)
    } else {
        format!("{}MB", n / MB)
    }
}

/// 由 category_id + path 生成稳定条目 id（16 位十六进制，对齐 `walk::stable_id` 语义）。
fn stable_item_id(category_id: &str, path: &std::path::Path) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_large_items_marks_size_path_atime() {
        // 沙箱 + 小阈值：≥large_min 的文件被标记（大小/路径/atime/分级/去向/降序）。
        let root = std::env::temp_dir().join(format!(
            "pureslate-large-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("d1")).unwrap();
        std::fs::write(root.join("small.txt"), "abc").unwrap(); // 3B < 4
        std::fs::write(root.join("d1/mid.bin"), "abcd").unwrap(); // 4B ≥ 4
        std::fs::write(root.join("d1/big.bin"), "abcdef").unwrap(); // 6B ≥ 4

        let cancel = CancelToken::new();
        let items = scan_large_items(&root, 4, &cancel);
        assert_eq!(items.len(), 2, "仅 ≥large_min 的文件入列");
        assert!(items[0].path.ends_with("big.bin"), "大小降序");
        assert_eq!(items[0].size_bytes, 6);
        assert_eq!(items[1].size_bytes, 4);
        assert!(items.iter().all(|i| i.category_id == "large.file"));
        assert!(items.iter().all(|i| i.grade == Grade::Yellow));
        assert!(items
            .iter()
            .all(|i| i.disposition == Disposition::Quarantine));
        assert!(items.iter().all(|i| i.mtime.is_some() && i.atime.is_some()));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    #[ignore = "真机抽查（P3-01 验证列）：扫真实系统盘 C:，人工核对排序与标记"]
    fn real_machine_large_spot_check() {
        let cancel = CancelToken::new();
        let items = scan_large_items(Path::new(r"C:\"), LARGE_MIN_BYTES, &cancel);
        eprintln!("large files on C: {}", items.len());
        for it in items.iter().take(10) {
            eprintln!("  {}  {}", human_bytes(it.size_bytes), it.path);
        }
        let sorted = items.windows(2).all(|w| w[0].size_bytes >= w[1].size_bytes);
        assert!(sorted, "必须按大小降序");
        assert!(items
            .iter()
            .all(|i| i.size_bytes >= LARGE_MIN_BYTES && i.atime.is_some()));
    }
}
