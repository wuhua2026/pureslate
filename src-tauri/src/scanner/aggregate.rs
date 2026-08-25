//! 扫描结果聚合（R03 aggregate）。把 `ScanItem[]` 聚合成报告页直用的
//! `FoundBytes` 与单类目 `CategoryAggregate`。纯函数，无 IO、无副作用。

use crate::contract::{CategoryAggregate, FoundBytes, Grade, ScanItem};

/// 全量已发现字节，按三档分级统计。
pub fn found_bytes(items: &[ScanItem]) -> FoundBytes {
    let mut f = FoundBytes {
        green: 0,
        yellow: 0,
        red: 0,
    };
    for it in items {
        add_bytes(&mut f, it.grade, it.size_bytes);
    }
    f
}

/// 累加单个条目字节到对应档位。
fn add_bytes(f: &mut FoundBytes, grade: Grade, n: u64) {
    match grade {
        Grade::Green => f.green += n,
        Grade::Yellow => f.yellow += n,
        Grade::Red => f.red += n,
    }
}

/// 单类目聚合：对该类目下的 items 汇总 total_bytes / item_count。
/// label / grade / disposition / reason 由调用方（引擎持有 category 元数据）提供。
pub fn category_aggregate(
    category_id: &str,
    label: &str,
    grade: Grade,
    disposition: crate::contract::Disposition,
    reason: &str,
    items: &[ScanItem],
) -> CategoryAggregate {
    let mut total = 0u64;
    for it in items {
        total += it.size_bytes;
    }
    CategoryAggregate {
        category_id: category_id.to_string(),
        label: label.to_string(),
        grade,
        disposition,
        total_bytes: total,
        item_count: items.len() as u64,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Disposition;

    fn item(cat: &str, grade: Grade, size: u64) -> ScanItem {
        ScanItem {
            id: format!("{cat}-{size}"),
            category_id: cat.to_string(),
            label: String::new(),
            path: format!("C:\\{cat}\\{size}"),
            size_bytes: size,
            grade,
            disposition: Disposition::Direct,
            reason: String::new(),
            mtime: None,
            atime: None,
            dup_group: None,
        }
    }

    #[test]
    fn found_bytes_sums_per_grade() {
        let items = vec![
            item("a", Grade::Green, 10),
            item("a", Grade::Yellow, 100),
            item("a", Grade::Red, 1000),
            item("b", Grade::Green, 1),
        ];
        let f = found_bytes(&items);
        assert_eq!(f.green, 11);
        assert_eq!(f.yellow, 100);
        assert_eq!(f.red, 1000);
    }

    #[test]
    fn category_aggregate_sums_items() {
        let items = vec![
            item("temp.user", Grade::Green, 5),
            item("temp.user", Grade::Green, 7),
        ];
        let a = category_aggregate(
            "temp.user",
            "临时文件",
            Grade::Green,
            Disposition::Direct,
            "desc",
            &items,
        );
        assert_eq!(a.total_bytes, 12);
        assert_eq!(a.item_count, 2);
        assert_eq!(a.label, "临时文件");
        assert_eq!(a.disposition, Disposition::Direct);
    }
}
