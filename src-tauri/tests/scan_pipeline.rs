//! P1-05 扫描编排集成测试 + 零写盘快照对比。
//!
//! 验证点：
//! 1. 依据规则文件（fixture XML）与 ScanProfile 能端到端产出 ScanItem / CategoryAggregate；
//! 2. 全程零写盘：对扫描目标目录做扫描前后快照对比（路径集合 + 逐字节内容），必须完全一致。
//!
//! 快照对比是红线 #1（扫描只读）的审计证据，跑法：`cargo test --test scan_pipeline`。

use pureslate_lib::contract::{Grade, ScanDimension, ScanProfile};
use pureslate_lib::rules::loader::RuleLoader;
use pureslate_lib::scanner::engine::run_scan;
use pureslate_lib::scanner::walk::CancelToken;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// 全程唯一的测试根目录序号，避免并发测试/重复运行互相干扰。
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 返回唯一的一组临时目录：`(rules_dir, target_dir)`（均已创建）。
fn fresh_roots() -> (PathBuf, PathBuf) {
    let seq = SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = std::env::temp_dir().join(format!("ps-itest-{nano:x}-{seq}"));
    let rules = base.join("rules");
    let target = base.join("target");
    fs::create_dir_all(&rules).expect("create rules dir");
    fs::create_dir_all(&target).expect("create target dir");
    (rules, target)
}

/// 写依赖规则：一条 `temp.integration`，target 指向扫描目录，匹配 `**\*.log`（递归）。
fn write_fixture_rules(rules_dir: &Path, target_dir: &str) {
    let esc = target_dir.replace('\\', "\\\\");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="itest" version="1" lang="zh-CN">
  <category id="temp.integration" label="集成测试临时" risk="green" disposition="direct"
            description="集成测试构造的临时日志">
    <target type="path" value="{esc}"/>
    <include pattern="**\*.log" recursive="true"/>
    <guard process=""/>
  </category>
</ruleset>
"#
    );
    fs::write(rules_dir.join("itest.xml"), xml).expect("write fixture rules");
    // 空白名单：避免加载器缺文件报错（本测试不依赖白名单过滤）。
    fs::write(
        rules_dir.join("whitelist.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<whitelist></whitelist>
"#,
    )
    .expect("write fixture whitelist");
}

/// 递归快照：相对路径 → 文件字节。返回排好序映射以便逐项比对。
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let entry = entry.expect("dir entry");
            let rel = if prefix.is_empty() {
                entry.file_name().to_string_lossy().into_owned()
            } else {
                format!("{prefix}\\{}", entry.file_name().to_string_lossy())
            };
            let p = entry.path();
            if p.is_dir() {
                stack.push((p, rel));
            } else {
                out.insert(rel, fs::read(&p).expect("read file"));
            }
        }
    }
    out
}

/// 构造启用 temp 维度的扫描配置。
fn temp_profile() -> ScanProfile {
    let mut dims = std::collections::HashMap::new();
    dims.insert(ScanDimension::Temp, true);
    ScanProfile { dimensions: dims }
}

#[test]
fn scan_finds_matching_items_and_aggregates() {
    let (rules_dir, target_dir) = fresh_roots();
    write_fixture_rules(&rules_dir, &target_dir.to_string_lossy());

    // 样本：两个命中 .log，一个不命名的 .txt。
    fs::create_dir_all(target_dir.join("sub")).unwrap();
    fs::write(target_dir.join("a.log"), "aaa-log").unwrap();
    fs::write(target_dir.join("sub\\b.log"), "bbb-log").unwrap();
    fs::write(target_dir.join("keep.txt"), "keep").unwrap();

    let mut loader = RuleLoader::new();
    let table = loader.load_dir(&rules_dir).expect("load fixture rules");
    let cancel = CancelToken::new();
    let mut progress_count = 0usize;
    {
        let mut progress = |_phase, _done, _total, _path, _found| {
            progress_count += 1;
        };
        let outcome = run_scan(&table, &temp_profile(), &cancel, &mut progress);

        // 命中：a.log 与 sub\b.log（每文件 1 项），keep.txt 不命中。
        assert_eq!(outcome.items.len(), 2, "应命中两个 .log");
        let mut paths: Vec<String> = outcome
            .items
            .iter()
            .map(|it| it.path.to_lowercase())
            .collect();
        paths.sort_unstable();
        assert!(
            paths.iter().any(|p| p.ends_with("a.log")),
            "应包含 a.log，实际 {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.contains("sub\\b.log")),
            "应包含递归命中的 sub\\b.log，实际 {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("keep.txt")),
            "不应命中 .txt"
        );

        // 分级：规则 risk=green → Grade::Green。
        for it in &outcome.items {
            assert_eq!(it.grade, Grade::Green, "path={}", it.path);
        }

        // 聚合：单类目 2 项，total_bytes 与明细一致。
        assert_eq!(outcome.aggregates.len(), 1);
        let agg = &outcome.aggregates[0];
        assert_eq!(agg.category_id, "temp.integration");
        assert_eq!(agg.item_count, 2);
        let sum: u64 = outcome.items.iter().map(|it| it.size_bytes).sum();
        assert_eq!(agg.total_bytes, sum);

        // 卷归属非空（至少回落到 C:）。
        assert!(!outcome.volume.is_empty());
    }

    // 进度回调至少触发一次。
    assert!(progress_count >= 1, "应至少推送一次进度回调");
}

#[test]
fn scan_is_zero_write_on_target_dir() {
    let (rules_dir, target_dir) = fresh_roots();
    write_fixture_rules(&rules_dir, &target_dir.to_string_lossy());

    fs::create_dir_all(target_dir.join("sub")).unwrap();
    fs::write(target_dir.join("a.log"), "aaa-log").unwrap();
    fs::write(target_dir.join("sub\\b.log"), "bbb-log").unwrap();
    fs::write(target_dir.join("keep.txt"), "keep").unwrap();

    let before = snapshot(&target_dir);

    let mut loader = RuleLoader::new();
    let table = loader.load_dir(&rules_dir).expect("load fixture rules");
    let cancel = CancelToken::new();
    let mut progress = |_phase, _done, _total, _path, _found| {};
    let _outcome = run_scan(&table, &temp_profile(), &cancel, &mut progress);

    let after = snapshot(&target_dir);

    assert_eq!(
        before, after,
        "扫描后目录快照必须与扫描前完全一致（红线#1 扫描只读：路径与内容零改动）"
    );
}
