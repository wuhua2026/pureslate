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
    compile_segment_regex, Category, Disposition, GlobRule, RegBase, RegHive, RegTargetSpec,
    RegView, Risk, RuleSetTable, Ruleset, RulesetSource, Target, TargetType,
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

    /// 加载目录下全部规则包（根元素为 `<ruleset>` 的 `*.xml`），合并为可见表。
    /// 白名单等非规则文件（如 `whitelist.xml`，由 safety 模块单独加载）会被跳过。
    pub fn load_dir(&mut self, rules_dir: &Path) -> Result<RuleSetTable, RulesError> {
        let mut table = RuleSetTable::default();
        let mut sorted = Vec::new();
        for entry in std::fs::read_dir(rules_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("xml") && is_ruleset_xml(&path) {
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

    /// 加载多目录并按序叠加（R22 规则包更新语义：**后目录 category 覆盖先目录**）。
    /// 生产顺序 = 资源规则目录 → `<data_root>\rules`（更新包安装目标）。
    pub fn load_dirs(&mut self, dirs: &[std::path::PathBuf]) -> Result<RuleSetTable, RulesError> {
        let mut table = RuleSetTable::default();
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            let t = self.load_dir(dir)?;
            table.by_id.extend(t.by_id);
        }
        Ok(table)
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
                        // 先解析属性再落库，避免对 Option 使用 expect（AGENTS 红线 6）。
                        let mut pc = PartialCategory::default();
                        parse_category_attrs(&e, &mut pc)?;
                        cur = Some(pc);
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
                    "target" => match parse_target(&e) {
                        Ok(t) => {
                            if let Some(c) = cur.as_mut() {
                                c.targets.push(t);
                            }
                        }
                        // R28：target 非法只丢所属 category（软失败），不得让整文件解析失败。
                        Err(reason) => {
                            if let Some(c) = cur.as_mut() {
                                c.fatal.get_or_insert(reason);
                            }
                        }
                    },
                    "include" | "exclude" => match parse_glob_attrs(&e) {
                        Ok(rule) => {
                            if let Some(c) = cur.as_mut() {
                                if name == "include" {
                                    c.includes.push(rule);
                                } else {
                                    c.excludes.push(rule);
                                }
                            }
                        }
                        // R28：include/exclude 属性非法（如段约束不成对）→ 丢整个 category。
                        Err(reason) => {
                            if let Some(c) = cur.as_mut() {
                                c.fatal.get_or_insert(reason);
                            }
                        }
                    },
                    "guard" => {
                        if let Some(c) = cur.as_mut() {
                            for a in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                                if key == "process" {
                                    // H2（v0.1.4）：多 `<guard>` 全部收集——原实现后者覆盖前者，
                                    // cache.browser 的 chrome.exe 守卫曾被 msedge.exe 静默顶掉。
                                    let val = a.unescape_value().unwrap_or_default().into_owned();
                                    if !val.is_empty() {
                                        c.guard_processes.push(val);
                                    }
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

/// 判断某 XML 文件是否为规则包：首元素为 `<ruleset>`。
/// 用于在 rules 目录中跳过白名单等非规则文件（白名单由 safety 模块单独加载），
/// 避免 `parse_ruleset` 对非规则文件报错而中止整目录加载（P1-05 集成暴露）。
fn is_ruleset_xml(path: &Path) -> bool {
    let xml = match std::fs::read_to_string(path) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            // 根元素（Start 或自闭合 Empty）名称即包类型。
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                return e.local_name().as_ref().eq_ignore_ascii_case(b"ruleset");
            }
            // 跳过 XML 声明/注释/文本后仍未遇根元素则视为非法。
            Ok(Event::Eof) => return false,
            Err(_) => return false,
            _ => {}
        }
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
/// R31-2：非法 disposition 记 fatal（整类丢弃），不再静默降级为 direct。
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
            "disposition" => match parse_disposition(&val) {
                Some(d) => c.disposition = Some(d),
                None => {
                    c.fatal.get_or_insert_with(|| {
                        format!("disposition `{val}` 非法（仅 direct/recycle/quarantine）")
                    });
                }
            },
            "description" => c.description = Some(val),
            _ => {}
        }
    }
    Ok(())
}

/// 解析 target 元素为模型（R28 起支持 env/path/knownFolder/reg-value）。
/// 属性非法/缺失返回 Err —— 调用方据此**丢弃整个 category**（不是整个文件）。
fn parse_target(e: &quick_xml::events::BytesStart<'_>) -> Result<Target, String> {
    let mut ttype = None;
    let mut tvalue = None;
    let mut hive_s = None;
    let mut key_s = None;
    let mut view_s = None;
    let mut base_s = None;
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "type" => ttype = Some(val),
            "value" => tvalue = Some(val),
            "hive" => hive_s = Some(val),
            "key" => key_s = Some(val),
            "view" => view_s = Some(val),
            "base" => base_s = Some(val),
            _ => {}
        }
    }
    let ttype = ttype.ok_or("target 缺少 type")?;
    match ttype.as_str() {
        "env" | "path" | "knownFolder" => {
            let value = tvalue.ok_or_else(|| format!("target({ttype}) 缺少 value"))?;
            if value.trim().is_empty() {
                return Err(format!("target({ttype}) value 为空"));
            }
            let ty = match ttype.as_str() {
                "env" => TargetType::Env,
                "path" => TargetType::Path,
                _ => TargetType::KnownFolder,
            };
            Ok(Target::simple(ty, value))
        }
        "reg-value" => {
            // value 属性复用为注册表值名（SPEC §4.1：hive/key/value/view/base）。
            let value_name = tvalue.ok_or("reg-value target 缺少 value（注册表值名）")?;
            let key = key_s.ok_or("reg-value target 缺少 key")?;
            if value_name.trim().is_empty() || key.trim().is_empty() {
                return Err("reg-value target 的 key/value 不得为空".into());
            }
            let hive = match hive_s.as_deref() {
                Some("HKCU") => RegHive::CurrentUser,
                Some("HKLM") => RegHive::LocalMachine,
                Some(other) => {
                    return Err(format!("reg-value hive `{other}` 非法（仅 HKCU/HKLM）"))
                }
                None => return Err("reg-value target 缺少 hive".into()),
            };
            let view = match view_s.as_deref() {
                None | Some("native") => RegView::Native,
                Some("wow64") => RegView::Wow64,
                Some(other) => {
                    return Err(format!("reg-value view `{other}` 非法（仅 native/wow64）"))
                }
            };
            let base = match base_s.as_deref() {
                None | Some("path") => RegBase::Path,
                Some("parent") => RegBase::Parent,
                Some(other) => {
                    return Err(format!("reg-value base `{other}` 非法（仅 path/parent）"))
                }
            };
            Ok(Target::reg_value(RegTargetSpec {
                hive,
                key,
                value_name,
                view,
                base,
            }))
        }
        other => Err(format!("target type `{other}` 不合法")),
    }
}

/// glob 元素属性解析。R28-2：segment-regex/segment-index 必须成对、index 可解析。
/// R28-3：max-depth（≥1）；recursive="false" 归一为 max-depth=1，与 >1 显式值矛盾则报错。
fn parse_glob_attrs(e: &quick_xml::events::BytesStart<'_>) -> Result<GlobRule, String> {
    let mut pattern = None;
    let mut recursive = true;
    let mut max_age_days = 0u32;
    let mut min_size_mb = 0u64;
    let mut segment_regex = None;
    let mut segment_index: Option<u32> = None;
    let mut max_depth_attr: Option<u16> = None;
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        match key.as_str() {
            "pattern" => pattern = Some(val),
            // R31-2：recursive 只接受严格布尔字面量，拼写错误一律 fail-closed
            // （此前 `recursive="yes"` 会被当成 true，等于静默放开递归面）。
            "recursive" => {
                recursive = match val.as_str() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!("recursive `{other}` 非法（仅 true/false）"));
                    }
                }
            }
            // R31-2：数值属性解析失败 → 丢类目（此前 unwrap_or(0) 会把非法值静默变成"不限"）。
            "maxAgeDays" => {
                max_age_days = val
                    .parse()
                    .map_err(|_| format!("maxAgeDays `{val}` 不是合法 u32"))?;
            }
            "minSizeMB" => {
                min_size_mb = val
                    .parse()
                    .map_err(|_| format!("minSizeMB `{val}` 不是合法 u64"))?;
            }
            "segment-regex" => segment_regex = Some(val),
            "segment-index" => {
                segment_index = Some(
                    val.parse()
                        .map_err(|_| format!("segment-index `{val}` 不是合法 u32"))?,
                );
            }
            "max-depth" => {
                max_depth_attr = Some(
                    val.parse()
                        .map_err(|_| format!("max-depth `{val}` 不是合法 u16"))?,
                );
            }
            _ => {}
        }
    }
    match (segment_regex.is_some(), segment_index.is_some()) {
        (true, false) => {
            return Err("include/exclude 的 segment-regex 必须与 segment-index 成对出现".into())
        }
        (false, true) => {
            return Err("include/exclude 的 segment-index 必须与 segment-regex 成对出现".into())
        }
        _ => {}
    }
    if let Some(0) = max_depth_attr {
        return Err("max-depth 必须 ≥1（根直属文件=1；缺省该属性才表示不限）".into());
    }
    // recursive=false ⇔ 深度 1；显式矛盾的配置按非法 category 处理（宁缺勿错）。
    let max_depth = match (max_depth_attr, recursive) {
        (Some(n), false) if n > 1 => {
            return Err(format!(
                "recursive=false 与 max-depth={n} 矛盾（false 等价深度 1）"
            ))
        }
        (Some(n), _) => Some(n),
        (None, false) => Some(1),
        (None, true) => None,
    };
    Ok(GlobRule {
        pattern: pattern.unwrap_or_default(),
        recursive,
        max_age_days,
        min_size_mb,
        segment_index,
        segment_regex,
        max_depth,
    })
}

fn parse_risk(s: &str) -> Risk {
    // SAFETY §1：无法判定默认 🔴。非法/缺失的 risk 一律保守映射为 Red。
    match s {
        "red" => Risk::Red,
        "yellow" => Risk::Yellow,
        "green" => Risk::Green,
        _ => Risk::Red,
    }
}

/// R31-2：disposition 严格解析。未知/缺失字符串 → None，由调用方按 fail-closed 处理
/// （非法属性 → 整类丢弃），绝不静默降级为 direct。
fn parse_disposition(s: &str) -> Option<Disposition> {
    match s {
        "direct" => Some(Disposition::Direct),
        "recycle" => Some(Disposition::Recycle),
        "quarantine" => Some(Disposition::Quarantine),
        _ => None,
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
    /// R31-2：None = 属性缺失或非法；finish 时按 fail-closed 丢弃类目。
    disposition: Option<Disposition>,
    description: Option<String>,
    targets: Vec<Target>,
    includes: Vec<GlobRule>,
    excludes: Vec<GlobRule>,
    guard_processes: Vec<String>,
    /// 子元素解析期的致命原因（如非法 target）；finish 时据此丢弃整个 category。
    fatal: Option<String>,
}

impl PartialCategory {
    /// 完成并校验（宁缺勿错）。
    fn finish(self) -> Result<Category, (String, String)> {
        let mut this = self;
        let id = match this.id.take() {
            Some(v) if !v.is_empty() => v,
            _ => return Err(("category 缺少 id".into(), "<unknown>".into())),
        };
        if let Some(reason) = this.fatal.take() {
            return Err((reason, id));
        }
        // R31-2：disposition 属性缺失 → 丢类目（非法值已在解析期记 fatal，不在此处静默降级）。
        let Some(disposition) = this.disposition else {
            return Err(("category 缺少 disposition".into(), id.clone()));
        };
        let label = this.label.take().unwrap_or_else(|| id.clone());
        if this.targets.is_empty() {
            return Err(("category 无任何 target".into(), id.clone()));
        }
        if this.includes.is_empty() {
            return Err(("category 无任何 include 规则".into(), id.clone()));
        }
        // R28-2：段正则编译失败的 include/exclude 在加载期丢弃（fail-closed：
        // 收紧约束失效绝不退化为无约束裸 glob）。include 全被丢 → 类目丢弃。
        drop_bad_segment_rules(&mut this.includes, &id, true);
        drop_bad_segment_rules(&mut this.excludes, &id, false);
        if this.includes.is_empty() {
            return Err(("include 段正则全部编译失败".into(), id.clone()));
        }
        // R28-1 裸根闸：reg-value 路径来自注册表外部数据，禁止配裸根全收 include，
        // 防止异常注册表值（如卷根）导致整盘入候选（SPEC §4.1）。
        if this.targets.iter().any(|t| t.ty == TargetType::RegValue) {
            if let Some(pattern) = this
                .includes
                .iter()
                .map(|g| g.pattern.as_str())
                .find(|p| reg_include_is_bare_root(p))
            {
                return Err((
                    format!(
                        "含 reg-value target 的 category 禁止裸根 include `{pattern}`（须含子目录段或纯扩展名 *.ext）"
                    ),
                    id,
                ));
            }
        }
        let cat = Category {
            id,
            label,
            risk: this.risk,
            disposition,
            description: this.description,
            targets: this.targets,
            includes: this.includes,
            excludes: this.excludes,
            guard_processes: this.guard_processes,
        };
        if !cat.is_valid_config() {
            let cid = cat.id.clone();
            return Err((
                "disposition 与 risk 不匹配（green 只允许 direct/recycle；yellow/red 只允许 quarantine）"
                    .into(),
                cid,
            ));
        }
        Ok(cat)
    }
}

/// R28：判断 include 模式对 reg-value target 是否为"裸根全收"。
/// 规则（SPEC §4.1）：
/// - 无路径分隔符时，仅允许纯扩展名形态 `*.ext`（ext 非空且不含通配符），
///   `*`/`**`/`*.*`/`data_*` 等一律判裸根；
/// - 含分隔符时，要求至少一个路径段含字面字符——`**/*`、`**/**` 这类全通配
///   虽然带分隔符，仍是裸根；`cache\\**`、`**/*.dmp` 含具体段/扩展名，放行。
fn reg_include_is_bare_root(pattern: &str) -> bool {
    let normalized = pattern.trim().replace('/', "\\");
    if !normalized.contains('\\') {
        let is_pure_ext = normalized
            .strip_prefix("*.")
            .is_some_and(|ext| !ext.is_empty() && !ext.contains(['*', '?']));
        return !is_pure_ext;
    }
    let has_literal_segment = normalized
        .split('\\')
        .filter(|s| !s.is_empty())
        .any(|s| s.chars().any(|c| !matches!(c, '*' | '?' | '[' | ']' | '!')));
    !has_literal_segment
}

/// R28-2：丢弃段正则编译失败的 include/exclude（fail-closed）。
/// 编译失败的收紧约束绝不能退化成无约束 glob——直接移除该条规则并记日志。
fn drop_bad_segment_rules(rules: &mut Vec<GlobRule>, category_id: &str, is_include: bool) {
    let kind = if is_include { "include" } else { "exclude" };
    rules.retain(|g| match g.segment_regex.as_ref() {
        Some(src) => match compile_segment_regex(src) {
            Ok(_) => true,
            Err(e) => {
                records::dropped_rule(
                    category_id,
                    kind,
                    &g.pattern,
                    &format!("段正则 `{src}` 编译失败: {e}"),
                );
                false
            }
        },
        None => true,
    });
}

/// 丢弃类目的软日志（临时使用 stderr；正式日志管线在 Phase 2 R09 接入）。
mod records {
    pub fn dropped_category(id: &str, reason: &str) {
        // TODO(R09): 接入审计日志后替换为结构化记录。
        eprintln!("[rules] drop category `{id}`: {reason}");
    }

    /// R28-2：单条 include/exclude 被丢弃（如段正则编译失败）。
    pub fn dropped_rule(category_id: &str, kind: &str, pattern: &str, reason: &str) {
        eprintln!("[rules] drop {kind} `{pattern}` in category `{category_id}`: {reason}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R22 规则包叠加：`load_dirs` 后目录（更新包安装目标）的同 id category 覆盖前目录。
    #[test]
    fn load_dirs_overlay_overrides_base() {
        let base = std::env::temp_dir().join(format!(
            "ps-loader-base-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let overlay = base.join("overlay");
        std::fs::create_dir_all(&overlay).unwrap();
        std::fs::write(
            base.join("base.xml"),
            r#"<ruleset id="base" version="1"><category id="temp.user" label="旧" risk="green" disposition="direct" description="d"><target type="path" value="C:\x"/><include pattern="*" recursive="true"/></category></ruleset>"#,
        )
        .unwrap();
        std::fs::write(
            overlay.join("update.xml"),
            r#"<ruleset id="update" version="2"><category id="temp.user" label="新" risk="yellow" disposition="quarantine" description="d"><target type="path" value="C:\x"/><include pattern="*" recursive="true"/></category></ruleset>"#,
        )
        .unwrap();

        let mut loader = RuleLoader::new();
        let table = loader.load_dirs(&[base.clone(), overlay.clone()]).unwrap();
        let (_, cat) = table.by_id.get("temp.user").unwrap();
        assert_eq!(cat.label, "新", "overlay 目录须覆盖 base 同 id category");
        assert_eq!(cat.risk, crate::rules::model::Risk::Yellow);
        // 仅 base 目录时仍是旧定义。
        let mut loader2 = RuleLoader::new();
        let t2 = loader2.load_dirs(std::slice::from_ref(&base)).unwrap();
        assert_eq!(t2.by_id.get("temp.user").unwrap().1.label, "旧");

        let _ = std::fs::remove_dir_all(&base);
    }

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
        assert!(temp.guard_processes.is_empty());

        let wx = rs
            .categories
            .iter()
            .find(|c| c.id == "cache.wechat")
            .unwrap();
        assert_eq!(wx.risk, Risk::Yellow);
        assert_eq!(wx.disposition, Disposition::Quarantine);
        assert!(wx.guard_processes == vec!["WeChat.exe".to_string()]);
    }

    #[test]
    fn parses_multiple_guards_without_folding() {
        // H2（v0.1.4）回归：多 `<guard>` 声明必须全部生效——原实现后者覆盖前者，
        // cache.browser 的 chrome.exe 守卫曾被 msedge.exe 静默顶掉（SAFETY §5.1 缺口）。
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ruleset id="t" version="1" lang="zh-CN">
  <category id="cache.browser" label="浏览器缓存" risk="yellow" disposition="quarantine" description="x">
    <target type="knownFolder" value="Local AppData"/>
    <include pattern="**" recursive="true"/>
    <guard process="chrome.exe"/>
    <guard process="msedge.exe"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("parse ok");
        let cat = rs
            .categories
            .iter()
            .find(|c| c.id == "cache.browser")
            .unwrap();
        assert_eq!(
            cat.guard_processes,
            vec!["chrome.exe".to_string(), "msedge.exe".to_string()],
            "两个守卫都必须保留"
        );
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

    // ---- R31-2 fail-closed：disposition / 数值 / recursive ----

    #[test]
    fn invalid_or_missing_disposition_drops_category() {
        // 非法 disposition 不得静默降级为 direct；缺失 disposition 不得默认放行。
        let xml = r#"<ruleset id="r" version="1">
  <category id="bad.invalid-disp" label="l" risk="yellow" disposition="delete">
    <target type="path" value="C:\a"/><include pattern="*"/>
  </category>
  <category id="bad.missing-disp" label="l" risk="yellow">
    <target type="path" value="C:\b"/><include pattern="*"/>
  </category>
  <category id="good" label="l" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\c"/><include pattern="*"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("文件级解析应成功（非法只丢 category）");
        assert_eq!(rs.categories.len(), 1);
        assert_eq!(rs.categories[0].id, "good");
    }

    #[test]
    fn disposition_risk_mismatch_drops_category() {
        // R31-2：green 只允许 direct/recycle；yellow/red 只允许 quarantine。
        let xml = r#"<ruleset id="r" version="1">
  <category id="bad.yellow-direct" label="l" risk="yellow" disposition="direct">
    <target type="path" value="C:\a"/><include pattern="*"/>
  </category>
  <category id="bad.red-direct" label="l" risk="red" disposition="direct">
    <target type="path" value="C:\b"/><include pattern="*"/>
  </category>
  <category id="bad.red-recycle" label="l" risk="red" disposition="recycle">
    <target type="path" value="C:\c"/><include pattern="*"/>
  </category>
  <category id="ok.green-direct" label="l" risk="green" disposition="direct">
    <target type="path" value="C:\d"/><include pattern="*"/>
  </category>
  <category id="ok.green-recycle" label="l" risk="green" disposition="recycle">
    <target type="path" value="C:\e"/><include pattern="*"/>
  </category>
  <category id="ok.yellow-quarantine" label="l" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\f"/><include pattern="*"/>
  </category>
  <category id="ok.red-quarantine" label="l" risk="red" disposition="quarantine">
    <target type="path" value="C:\g"/><include pattern="*"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("文件级解析应成功");
        let ids: Vec<&str> = rs.categories.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "ok.green-direct",
                "ok.green-recycle",
                "ok.yellow-quarantine",
                "ok.red-quarantine"
            ],
            "risk/disposition 不匹配的类目必须丢弃，合法的三类去向保留"
        );
    }

    #[test]
    fn invalid_glob_numeric_or_recursive_attrs_drop_category() {
        // R31-2：非法数值/布尔属性不得被 unwrap_or(0)/!= "false" 静默兜底。
        let cases = [
            r#"<include pattern="*" maxAgeDays="abc"/>"#,
            r#"<include pattern="*" maxAgeDays="-1"/>"#,
            r#"<include pattern="*" minSizeMB="1.5"/>"#,
            r#"<include pattern="*" recursive="yes"/>"#,
            r#"<include pattern="*" recursive="0"/>"#,
            r#"<include pattern="*" recursive=""/>"#,
        ];
        for inc in cases {
            let frag = format!(
                r#"<category id="bad" label="l" risk="green" disposition="direct"><target type="path" value="C:\tmp"/>{inc}</category>
  <category id="good" label="ok" risk="green" disposition="direct"><target type="path" value="C:\tmp"/><include pattern="*"/></category>"#
            );
            let cats = parse_one_category(&frag);
            assert_eq!(cats.len(), 1, "非法 glob 属性应只丢 bad：{inc}");
            assert_eq!(cats[0].id, "good");
        }
    }

    #[test]
    fn valid_glob_attrs_still_parse_after_strictening() {
        // 合法配置行为不变：严格化只拒非法字面量，不改变合法值与缺省语义。
        let g = parse_one_include(
            r#"<include pattern="*" recursive="true" maxAgeDays="7" minSizeMB="12"/>"#,
        );
        assert!(g.recursive);
        assert_eq!(g.max_age_days, 7);
        assert_eq!(g.min_size_mb, 12);

        let g = parse_one_include(r#"<include pattern="*.lnk" recursive="false"/>"#);
        assert!(!g.recursive);
        assert_eq!(g.max_age_days, 0, "缺省 maxAgeDays = 不限");
        assert_eq!(g.min_size_mb, 0, "缺省 minSizeMB = 不限");
    }

    #[test]
    fn invalid_or_missing_risk_defaults_to_red() {
        let xml = r#"<ruleset id="r" version="1">
  <category id="c.bad-risk" label="l" risk="aggressive" disposition="quarantine">
    <target type="path" value="C:\a"/>
    <include pattern="*"/>
  </category>
  <category id="c.no-risk" label="l" disposition="quarantine" description="缺 risk 属性">
    <target type="path" value="C:\b"/>
    <include pattern="*"/>
  </category>
  <category id="c.green" label="l" risk="green" disposition="direct">
    <target type="path" value="C:\c"/>
    <include pattern="*"/>
  </category>
</ruleset>"#;
        let rs = parse_ruleset(xml).expect("should parse");
        assert_eq!(
            rs.categories
                .iter()
                .find(|c| c.id == "c.bad-risk")
                .unwrap()
                .risk,
            Risk::Red,
            "非法 risk → 默认红"
        );
        assert_eq!(
            rs.categories
                .iter()
                .find(|c| c.id == "c.no-risk")
                .unwrap()
                .risk,
            Risk::Red,
            "缺失 risk → 默认红"
        );
        assert_eq!(
            rs.categories
                .iter()
                .find(|c| c.id == "c.green")
                .unwrap()
                .risk,
            Risk::Green
        );
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
        assert!(!cat.includes[0].recursive);
        assert_eq!(cat.description.as_deref(), Some("覆盖版"));
        // cache.wechat 仍在（来自 a.xml）。
        assert!(tbl.by_id.contains_key("cache.wechat"));

        // 缓存命中：再次加载不重建（mtime 一致）。
        let before = loader.ruleset_count();
        loader.load_dir(&dir).unwrap();
        assert_eq!(loader.ruleset_count(), before);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- R28 reg-value target ----

    const REG_VALID_CATEGORY: &str = r#"<category id="cache.reg" label="r" risk="yellow" disposition="quarantine">
    <target type="reg-value" hive="HKCU" key="Software\ExampleVendor\ExampleApp" value="InstallDir" view="native" base="path"/>
    <include pattern="data\accounts\*\cache\**"/>
  </category>"#;

    fn parse_one_category(xml_fragment: &str) -> Vec<Category> {
        let xml = format!(r#"<ruleset id="r" version="1">{xml_fragment}</ruleset>"#);
        parse_ruleset(&xml)
            .expect("文件级解析应成功（非法只丢 category）")
            .categories
    }

    #[test]
    fn reg_value_target_parses_with_all_attrs() {
        let cats = parse_one_category(REG_VALID_CATEGORY);
        assert_eq!(cats.len(), 1);
        let t = &cats[0].targets[0];
        assert_eq!(t.ty, TargetType::RegValue);
        let spec = t.reg.as_ref().expect("reg spec");
        assert_eq!(spec.hive, RegHive::CurrentUser);
        assert_eq!(spec.key, r"Software\ExampleVendor\ExampleApp");
        assert_eq!(spec.value_name, "InstallDir");
        assert_eq!(spec.view, RegView::Native);
        assert_eq!(spec.base, RegBase::Path);
    }

    #[test]
    fn reg_value_defaults_view_and_base() {
        let xml = r#"<category id="c" label="l" risk="yellow" disposition="quarantine">
    <target type="reg-value" hive="HKLM" key="K" value="V"/>
    <include pattern="cache\*"/>
  </category>"#;
        let cats = parse_one_category(xml);
        let spec = cats[0].targets[0].reg.as_ref().unwrap();
        assert_eq!(spec.hive, RegHive::LocalMachine);
        assert_eq!(spec.view, RegView::Native, "view 缺省 native");
        assert_eq!(spec.base, RegBase::Path, "base 缺省 path");
    }

    #[test]
    fn reg_value_invalid_attrs_drop_only_category() {
        let cases = [
            r#"<target type="reg-value" hive="HKCC" key="K" value="V"/>"#,
            r#"<target type="reg-value" hive="HKCU" key="K"/>"#,
            r#"<target type="reg-value" hive="HKCU" value="V"/>"#,
            r#"<target type="reg-value" hive="HKCU" key="K" value="V" view="x64"/>"#,
            r#"<target type="reg-value" hive="HKCU" key="K" value="V" base="file"/>"#,
        ];
        for bad_target in cases {
            let frag = format!(
                r#"<category id="bad" label="l" risk="yellow" disposition="quarantine">{bad_target}<include pattern="cache\*"/></category>
  <category id="good" label="ok" risk="yellow" disposition="quarantine"><target type="path" value="C:\tmp"/><include pattern="*"/></category>"#
            );
            let cats = parse_one_category(&frag);
            assert_eq!(cats.len(), 1, "非法 reg target 应只丢 bad、保留 good");
            assert_eq!(cats[0].id, "good");
        }
    }

    #[test]
    fn reg_value_bare_root_include_rejected() {
        // 裸根形态全部丢类目（无分隔符非纯扩展名；或全通配段）。
        for bare in ["*", "**", "*.*", "**/*", "**/**", "**\\*", "data_*", "*.??"] {
            let frag = format!(
                r#"<category id="c-bare-{i}" label="l" risk="yellow" disposition="quarantine">
      <target type="reg-value" hive="HKCU" key="K" value="V"/>
      <include pattern="{bare}"/>
    </category>"#,
                i = bare.len(),
                bare = bare
            );
            assert!(
                parse_one_category(&frag).is_empty(),
                "裸根 include `{bare}` 必须拒绝"
            );
        }
        // 子目录形态、含扩展名的递归形态、纯扩展名形态保留。
        for ok in ["cache\\**", "a/b/*.dmp", "*.dmp", "**/*.dmp"] {
            let frag = format!(
                r#"<category id="c-ok-{i}" label="l" risk="yellow" disposition="quarantine">
      <target type="reg-value" hive="HKCU" key="K" value="V"/>
      <include pattern="{ok}"/>
    </category>"#,
                i = ok.len(),
                ok = ok
            );
            assert_eq!(
                parse_one_category(&frag).len(),
                1,
                "`{ok}` 不是裸根，应保留"
            );
        }
    }

    #[test]
    fn non_reg_target_still_allows_root_include() {
        // 裸根闸只针对 reg-value；普通 path target 的 `*` 行为不变。
        let xml = r#"<category id="temp.user" label="t" risk="green" disposition="direct">
    <target type="path" value="C:\tmp"/>
    <include pattern="*"/>
  </category>"#;
        assert_eq!(parse_one_category(xml).len(), 1);
    }

    // ---- R28-2 segment-regex ----

    #[test]
    fn segment_attrs_parse_as_pair() {
        let xml = r#"<category id="cache.accounts" label="l" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\tmp"/>
    <include pattern="accounts\*\cache\**" segment-regex="[0-9]{6,12}" segment-index="1"/>
  </category>"#;
        let cats = parse_one_category(xml);
        let g = &cats[0].includes[0];
        assert_eq!(g.segment_index, Some(1));
        assert_eq!(g.segment_regex.as_deref(), Some("[0-9]{6,12}"));
    }

    #[test]
    fn segment_attrs_unpaired_or_bad_index_drop_category() {
        let cases = [
            r#"<include pattern="a\**" segment-regex="[0-9]+"/>"#,
            r#"<include pattern="a\**" segment-index="2"/>"#,
            r#"<include pattern="a\**" segment-regex="[0-9]+" segment-index="x"/>"#,
        ];
        for inc in cases {
            let frag = format!(
                r#"<category id="bad" label="l" risk="yellow" disposition="quarantine"><target type="path" value="C:\tmp"/>{inc}</category>
  <category id="good" label="ok" risk="yellow" disposition="quarantine"><target type="path" value="C:\tmp"/><include pattern="*"/></category>"#
            );
            let cats = parse_one_category(&frag);
            assert_eq!(cats.len(), 1, "段属性非法应只丢 bad：{inc}");
            assert_eq!(cats[0].id, "good");
        }
    }

    #[test]
    fn bad_segment_regex_drops_rule_and_category() {
        // 唯一 include 的正则编译失败 → include 被丢 → category 无 include → 丢类目。
        let xml = r#"<category id="bad" label="l" risk="yellow" disposition="quarantine">
    <target type="path" value="C:\tmp"/>
    <include pattern="a\**" segment-regex="[unclosed" segment-index="0"/>
  </category>"#;
        assert!(
            parse_one_category(xml).is_empty(),
            "坏正则必须 fail-closed 丢规则"
        );
    }

    // ---- R28-3 max-depth ----

    fn parse_one_include(xml_include: &str) -> GlobRule {
        let frag = format!(
            r#"<category id="c" label="l" risk="green" disposition="direct"><target type="path" value="C:\tmp"/>{xml_include}</category>"#
        );
        parse_one_category(&frag)[0].includes[0].clone()
    }

    #[test]
    fn recursive_false_normalizes_to_depth_one() {
        let g = parse_one_include(r#"<include pattern="*.lnk" recursive="false"/>"#);
        assert_eq!(g.max_depth, Some(1), "recursive=false 归一为深度 1");
        let g = parse_one_include(r#"<include pattern="a\*" recursive="false" max-depth="1"/>"#);
        assert_eq!(g.max_depth, Some(1), "false + depth=1 自洽");
    }

    #[test]
    fn explicit_max_depth_parses() {
        let g = parse_one_include(r#"<include pattern="a\**" max-depth="5"/>"#);
        assert_eq!(g.max_depth, Some(5));
        let g = parse_one_include(r#"<include pattern="*"/>"#);
        assert_eq!(g.max_depth, None, "缺省不限");
    }

    #[test]
    fn conflicting_or_bad_depth_drops_category() {
        let cases = [
            r#"<include pattern="a\**" recursive="false" max-depth="3"/>"#,
            r#"<include pattern="a\**" max-depth="0"/>"#,
            r#"<include pattern="a\**" max-depth="x"/>"#,
        ];
        for inc in cases {
            let frag = format!(
                r#"<category id="bad" label="l" risk="green" disposition="direct"><target type="path" value="C:\tmp"/>{inc}</category>
  <category id="good" label="ok" risk="green" disposition="direct"><target type="path" value="C:\tmp"/><include pattern="*"/></category>"#
            );
            let cats = parse_one_category(&frag);
            assert_eq!(cats.len(), 1, "深度配置非法应只丢 bad：{inc}");
            assert_eq!(cats[0].id, "good");
        }
    }

    #[test]
    fn shipped_rules_load_with_expected_depth() {
        // 既有规则包快照：5 个 XML 全部加载成功；
        // privacy.recent-docs 的 recursive=false 已归一为深度 1（行为零变化核对）。
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("rules");
        let mut loader = RuleLoader::new();
        let table = loader.load_dir(&dir).expect("加载随包规则包");
        assert!(table.by_id.len() >= 5, "应加载全部自研规则类目");
        let (_, recent) = table
            .by_id
            .get("privacy.recent-docs")
            .expect("最近文档类目存在");
        assert_eq!(recent.includes.len(), 1);
        assert_eq!(
            recent.includes[0].max_depth,
            Some(1),
            "*.lnk 非递归规则必须归一为深度 1"
        );
        // 历史规则包的其它类目深度保持不限（未被意外收紧）。
        for id in ["privacy.edge-history", "privacy.chrome-history"] {
            let (_, cat) = table.by_id.get(id).unwrap();
            assert!(
                cat.includes.iter().all(|g| g.max_depth.is_none()),
                "{id} 不应被隐式赋深度"
            );
        }
    }
}
