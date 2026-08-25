//! 规则加载器（R01 loader/cache）。
//!
//! 职责：
//! 1. 解析 `resources/rules/*.xml`（SPEC §4.1 schema）；
//! 2. 校验失败/字段非法的 category 整体丢弃并记录（宁缺勿错）；
//! 3. 合并为 `RuleSetTable`，多 ruleset 同 id category 后加载覆盖先加载；
//! 4. 缓存按来源文件 mtime 失效。

use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::model::{
    Category, Disposition, GlobRule, Risk, RuleSetTable, Ruleset, RulesetSource, Target, TargetType,
};

/// 规则加载错误。
#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML 解析失败: {0}")]
    Xml(String),
    #[error("非法规则包: {0}")]
    Invalid(String),
    /// 类别级软失败：整个 category 丢弃。与文件级错误区分，不中断文件内其他 category。
    #[error("跳过非法 category `{category}`: {reason}")]
    CategoryDropped { category: String, reason: String },
}

/// 规则加载/缓存结构。
#[derive(Default)]
pub struct RuleLoader {
    /// 已装载来源 -> 元数据缓存（含 mtime）。
    cache: std::collections::HashMap<String, RulesetSource>,
}

impl RuleLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// 加载目录下全部 `*.xml` 规则包，合并为可见表。
    pub fn load_dir(&mut self, rules_dir: &Path) -> Result<RuleSetTable, RulesError> {
        let mut table = RuleSetTable::default();
        let mut sorted = Vec::new();
        for entry in std::fs::read_dir(rules_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("xml") {
                sorted.push(path);
            }
        }
        // 确定性加载顺序：按文件名字典序，保证覆盖语义可复现（后加载覆盖先加载）。
        sorted.sort();
        for path in sorted {
            let source = self.load_file(&path)?;
            let rs_id = source.ruleset.id.clone();
            for cat in &source.ruleset.categories {
                table
                    .by_id
                    .insert(cat.id.clone(), (rs_id.clone(), cat.clone()));
            }
        }
        Ok(table)
    }

    /// 加载单个文件，带 mtime 缓存（缓存失效判定）。
    /// 返回 owned `RulesetSource`；命中缓存时返回其克隆，未命中则重载并更新缓存。
    pub fn load_file(&mut self, path: &Path) -> Result<RulesetSource, RulesError> {
        let path_str = path.to_string_lossy().into_owned();
        let current_mtime = file_mtime_secs(path)?;

        if let Some(cached) = self.cache.get(&path_str) {
            if cached.mtime_secs == current_mtime {
                // 命中缓存，返回克隆（结束缓存借用后再 clone 内容）。
                let hit = cached.clone();
                return Ok(hit);
            }
        }

        let xml = std::fs::read_to_string(path)?;
        let ruleset = parse_ruleset(&xml)?;
        let source = RulesetSource {
            ruleset,
            source_path: path_str.clone(),
            mtime_secs: current_mtime,
        };
        self.cache.insert(path_str.clone(), source.clone());
        Ok(source)
    }

    /// 清除全部缓存（供测试/热重载）。
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// 加载给定规则包的某一路径并校验目标合法（env 展开等由 P1-02 处理）。
    #[allow(dead_code)]
    pub fn ruleset_count(&self) -> usize {
        self.cache.len()
    }
}

/// 解析单个规则包 XML（SPEC §4.1）。非法 ruleset 级字段 → 硬错；非法 category → 丢弃并记录。
pub fn parse_ruleset(xml: &str) -> Result<Ruleset, RulesError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut id = None;
    let mut version = 0u32;
    let mut lang = String::new();
    let mut categories: Vec<Category> = Vec::new();

    // 当前 category 的累积状态。
    let mut cur: Option<PartialCategory> = None;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            // 元素起始（非自闭合）。
            Ok(Event::Start(e)) => {
                text_buf.clear();
                let name = e.local_name();
                let name = String::from_utf8_lossy(name.as_ref()).into_owned();
                match name.as_str() {
                    "ruleset" => parse_ruleset_attrs(&e, &mut id, &mut version, &mut lang)?,
                    "category" => {
                        cur = Some(PartialCategory::default());
                        parse_category_attrs(&e, cur.as_mut().expect("just set"))?;
                    }
                    // 空壳元素虽不符合 schema，但以常规元素处理（其 Empty 分支在下方统一结算）。
                    _ => {}
                }
            }
            // 自闭合元素：<target/> <include/> <exclude/> <guard/>，以及可能特例的自闭合 <category/>。
            Ok(Event::Empty(e)) => {
                text_buf.clear();
                let name = e.local_name();
                let name = String::from_utf8_lossy(name.as_ref()).into_owned();
                match name.as_str() {
                    "target" => {
                        let (ty, value) = parse_target_attrs(&e)?;
                        if let Some(c) = cur.as_mut() {
                            c.targets.push(Target { ty, value });
                        }
                    }
                    "include" | "exclude" => {
                        let rule = parse_glob_attrs(&e);
                        if let Some(c) = cur.as_mut() {
                            if name == "include" {
                                c.includes.push(rule);
                            } else {
                                c.excludes.push(rule);
                            }
                        }
                    }
                    "guard" => {
                        if let Some(c) = cur.as_mut() {
                            for a in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                                if key == "process" {
                                    let val = a.unescape_value().unwrap_or_default().into_owned();
                                    c.guard_process = if val.is_empty() { None } else { Some(val) };
                                }
                            }
                        }
                    }
                    "category" => {
                        // 自闭合 category 无子元素，直接结算。
                        if let Some(pc) = cur.take() {
                            push_or_drop(pc, &mut categories);
                        }
                    }
                    "ruleset" => parse_ruleset_attrs(&e, &mut id, &mut version, &mut lang)?,
                    _ => {}
                }
            }
            // 元素结束：category 在此结算（子元素已全部收集）。
            Ok(Event::End(e)) => {
                text_buf.clear();
                let name = e.local_name();
                let name = String::from_utf8_lossy(name.as_ref()).into_owned();
                if name == "category" {
                    if let Some(pc) = cur.take() {
                        push_or_drop(pc, &mut categories);
                    }
                }
            }
            Ok(Event::Text(_t)) => {
                // 文本内容在本 schema 中无语义（类目信息全在属性），置空即可。
                text_buf.clear();
            }
            Ok(Event::CData(_t)) => {
                text_buf.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(RulesError::Xml(format!(
                    "行 {}: {e}",
                    reader.buffer_position()
                )))
            }
            _ => {}
        }
    }

    let id = id.ok_or_else(|| RulesError::Invalid("ruleset 缺少 id".into()))?;
    if lang.is_empty() {
        lang = "zh-CN".into();
    }
    Ok(Ruleset {
        id,
        version,
        lang,
        categories,
    })
}

/// 结算一个部分解析的 category：合法则入列，非法则软丢弃并记录。
fn push_or_drop(pc: PartialCategory, categories: &mut Vec<Category>) {
    match pc.finish() {
        Ok(cat) => categories.push(cat),
        Err((reason, cid)) => records::dropped_category(&cid, &reason),
    }
}

/// 解析 `<ruleset>` 顶层属性。
fn parse_ruleset_attrs(
    e: &quick_xml::events::BytesStart<'_>,
    id: &mut Option<String>,
    version: &mut u32,
    lang: &mut String,
) -> Result<(), RulesError> {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "id" => *id = Some(val),
            "version" => *version = val.parse().unwrap_or(0),
            "lang" => *lang = val,
            _ => {}
        }
    }
    Ok(())
}

/// 解析 `<category>` 起始属性（id/label/risk/disposition/description）。
fn parse_category_attrs(
    e: &quick_xml::events::BytesStart<'_>,
    c: &mut PartialCategory,
) -> Result<(), RulesError> {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "id" => c.id = Some(val),
            "label" => c.label = Some(val),
            "risk" => c.risk = parse_risk(&val),
            "disposition" => c.disposition = parse_disposition(&val),
            "description" => c.description = Some(val),
            _ => {}
        }
    }
    Ok(())
}

/// 从 target 元素的属性解析 TargetType + value。
fn parse_target_attrs(
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<(TargetType, String), RulesError> {
    let mut ty = None;
    let mut value = None;
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "type" => ty = Some(val),
            "value" => value = Some(val),
            _ => {}
        }
    }
    let ty = ty.ok_or(RulesError::Invalid("target 缺少 type".into()))?;
    let value = value.ok_or(RulesError::Invalid("target 缺少 value".into()))?;
    let ty = match ty.as_str() {
        "env" => TargetType::Env,
        "path" => TargetType::Path,
        "knownFolder" => TargetType::KnownFolder,
        other => return Err(RulesError::Invalid(format!("target type `{other}` 不合法"))),
    };
    Ok((ty, value))
}

/// glob 元素属性解析。
fn parse_glob_attrs(e: &quick_xml::events::BytesStart<'_>) -> GlobRule {
    let mut pattern = None;
    let mut recursive = true;
    let mut max_age_days = 0u32;
    let mut min_size_mb = 0u64;
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "pattern" => pattern = Some(val),
            "recursive" => recursive = val != "false",
            "maxAgeDays" => max_age_days = val.parse().unwrap_or(0),
            "minSizeMB" => min_size_mb = val.parse().unwrap_or(0),
            _ => {}
        }
    }
    GlobRule {
        pattern: pattern.unwrap_or_default(),
        recursive,
        max_age_days,
        min_size_mb,
    }
}

fn parse_risk(s: &str) -> Risk {
    match s {
        "red" => Risk::Red,
        "yellow" => Risk::Yellow,
        _ => Risk::Green,
    }
}

fn parse_disposition(s: &str) -> Disposition {
    match s {
        "recycle" => Disposition::Recycle,
        "quarantine" => Disposition::Quarantine,
        _ => Disposition::Direct,
    }
}

fn file_mtime_secs(path: &Path) -> Result<i64, RulesError> {
    // CREATED/UNIX 精度：目标平台为 Windows，modification time 秒级够用（缓存失效粒度）。
    // 仅读取 mtime，不做任何写操作（符合扫描只读红线）。
    let meta = std::fs::metadata(path)?;
    use std::time::UNIX_EPOCH;
    Ok(meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0))
}

/// 部分解析出的 category（finish 时校验并转正式 Category）。
#[derive(Default)]
struct PartialCategory {
    id: Option<String>,
    label: Option<String>,
    risk: Risk,
    disposition: Disposition,
    description: Option<String>,
    targets: Vec<Target>,
    includes: Vec<GlobRule>,
    excludes: Vec<GlobRule>,
    guard_process: Option<String>,
}

impl PartialCategory {
    /// 完成并校验（宁缺勿错）。
    fn finish(self) -> Result<Category, (String, String)> {
        let id = match self.id {
            Some(v) if !v.is_empty() => v,
            _ => return Err(("category 缺少 id".into(), "<unknown>".into())),
        };
        let label = self.label.unwrap_or_else(|| id.clone());
        if self.targets.is_empty() {
            return Err(("category 无任何 target".into(), id.clone()));
        }
        if self.includes.is_empty() {
            return Err(("category 无任何 include 规则".into(), id.clone()));
        }
        let cat = Category {
            id,
            label,
            risk: self.risk,
            disposition: self.disposition,
            description: self.description,
            targets: self.targets,
            includes: self.includes,
            excludes: self.excludes,
            guard_process: self.guard_process,
        };
        if !cat.is_valid_config() {
            let cid = cat.id.clone();
            return Err(("green 类目只允许 direct/recycle 去向".into(), cid));
        }
        Ok(cat)
    }
}

/// 丢弃类目的软日志（临时使用 stderr；正式日志管线在 Phase 2 R09 接入）。
mod records {
    pub fn dropped_category(id: &str, reason: &str) {
        // TODO(R09): 接入审计日志后替换为结构化记录。
        eprintln!("[rules] drop category `{id}`: {reason}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="system-temp" version="1" lang="zh-CN">
  <category id="temp.user" label="临时文件" risk="green" disposition="direct" description="临时文件">
    <target type="env" value="%TEMP%"/>
    <include pattern="*" recursive="true" maxAgeDays="7"/>
    <exclude pattern="*.lock"/>
    <guard process=""/>
  </category>
  <category id="cache.wechat" label="微信缓存" risk="yellow" disposition="quarantine" description="清理后需重新登录">
    <target type="knownFolder" value="WeChat Files"/>
    <include pattern="FileStorage\Cache\**" recursive="true"/>
    <guard process="WeChat.exe"/>
  </category>
</ruleset>"#;

    #[test]
    fn parses_valid_ruleset() {
        let rs = parse_ruleset(VALID_XML).expect("should parse");
        assert_eq!(rs.id, "system-temp");
        assert_eq!(rs.version, 1);
        assert_eq!(rs.lang, "zh-CN");
        assert_eq!(rs.categories.len(), 2);

        let temp = rs.categories.iter().find(|c| c.id == "temp.user").unwrap();
        assert_eq!(temp.risk, Risk::Green);
        assert_eq!(temp.disposition, Disposition::Direct);
        assert_eq!(temp.targets.len(), 1);
        assert_eq!(temp.targets[0].ty, TargetType::Env);
        assert_eq!(temp.targets[0].value, "%TEMP%");
        assert_eq!(temp.includes.len(), 1);
        assert_eq!(temp.includes[0].pattern, "*");
        assert!(temp.includes[0].recursive);
        assert_eq!(temp.includes[0].max_age_days, 7);
        assert_eq!(temp.excludes[0].pattern, "*.lock");
        assert!(temp.guard_process.is_none());

        let wx = rs
            .categories
            .iter()
            .find(|c| c.id == "cache.wechat")
            .unwrap();
        assert_eq!(wx.risk, Risk::Yellow);
        assert_eq!(wx.disposition, Disposition::Quarantine);
        assert_eq!(wx.guard_process.as_deref(), Some("WeChat.exe"));
    }

    #[test]
    fn drops_invalid_category_keeps_valid() {
        let xml = r#"<ruleset id="r" version="1">
  <category id="bad.no-target" label="x" risk="green" disposition="direct">
    <include pattern="*"/>
  </category>
  <category id="good" label="ok" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\tmp"/>
    <include pattern="*"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("file-level ok, only category dropped");
        assert_eq!(rs.categories.len(), 1);
        assert_eq!(rs.categories[0].id, "good");
    }

    #[test]
    fn invalid_ruleset_missing_id_fails() {
        let xml = r#"<ruleset version="1"><category id="c" label="l" risk="green" disposition="direct"><target type="path" value="C:\"/><include pattern="*"/></category></ruleset>"#;
        assert!(parse_ruleset(xml).is_err());
    }

    #[test]
    fn green_category_with_quarantine_rejected() {
        let xml = r#"<ruleset id="r" version="1">
  <category id="bad.green.quarantine" label="l" risk="green" disposition="quarantine">
    <target type="path" value="C:\tmp"/>
    <include pattern="*"/>
  </category>
  <category id="ok" label="l" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\tmp"/>
    <include pattern="*"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("should parse");
        assert_eq!(rs.categories.len(), 1);
        assert_eq!(rs.categories[0].id, "ok");
    }

    #[test]
    fn load_dir_merges_and_overrides() {
        let dir = std::env::temp_dir().join(format!("pureslate-rules-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.xml"), VALID_XML).unwrap();
        // 覆盖规则包：同 id category 后加载覆盖先加载。
        let override_xml = r#"<ruleset id="override" version="2">
  <category id="temp.user" label="临时文件(改)" risk="green" disposition="direct" description="覆盖版">
    <target type="path" value="C:\SOMETHING"/>
    <include pattern="*.tmp" recursive="false"/>
  </category>
</ruleset>"#;
        std::fs::write(dir.join("b-override.xml"), override_xml).unwrap();

        let mut loader = RuleLoader::new();
        let tbl = loader.load_dir(&dir).expect("load");
        // temp.user 应被 override 覆盖。
        let (rsid, cat) = tbl.by_id.get("temp.user").expect("has temp.user");
        assert_eq!(rsid, "override");
        assert_eq!(cat.includes[0].recursive, false);
        assert_eq!(cat.description.as_deref(), Some("覆盖版"));
        // cache.wechat 仍在（来自 a.xml）。
        assert!(tbl.by_id.contains_key("cache.wechat"));

        // 缓存命中：再次加载不重建（mtime 一致）。
        let before = loader.ruleset_count();
        loader.load_dir(&dir).unwrap();
        assert_eq!(loader.ruleset_count(), before);

        std::fs::remove_dir_all(&dir).ok();
    }
}
