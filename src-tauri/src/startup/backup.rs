//! 启动项备份记录（R07）。`<data_root>\startup-backup\`：
//! - `manifest.jsonl`：每个已禁用项一行（还原定位用）；
//! - `<id>.reg`：注册表项备份；`<id>.<ext>`：启动文件夹文件本体（移入保管）。
//!
//! 纪律（红线"先日志后动手"）：备份落盘成功后才移除原启动项；备份即事务记录。

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contract::StartupSource;
use crate::storage::data_root;

/// 一条禁用记录（manifest.jsonl 单行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub source: StartupSource,
    pub name: String,
    pub command: String,
    pub disabled_at: i64,
    /// `<id>.reg`（注册表项）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reg_file: Option<String>,
    /// 备份目录中的文件名（启动文件夹项，文件本体已移入）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_backup: Option<String>,
    /// 启动文件夹项的原完整路径（还原目标）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig_path: Option<String>,
    /// 计划任务项的任务名（schtasks 定位）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
}

/// 备份目录。
pub fn backup_dir() -> PathBuf {
    data_root().join("startup-backup")
}

fn manifest_path() -> PathBuf {
    backup_dir().join("manifest.jsonl")
}

/// 读取全部记录（文件缺失 → 空；单行损坏跳过不致命，同 quarantine manifest）。
pub fn load_manifest() -> Vec<BackupRecord> {
    let Ok(f) = fs::File::open(manifest_path()) else {
        return vec![];
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<BackupRecord>(&line).ok())
        .collect()
}

/// 追加一条记录。
pub fn append_record(rec: &BackupRecord) -> io::Result<()> {
    fs::create_dir_all(backup_dir())?;
    let line = serde_json::to_string(rec)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(manifest_path())?;
    writeln!(f, "{line}")
}

/// 移除一条记录（还原成功后调用；整文件重写，属本模块私有状态文件）。
pub fn remove_record(id: &str) -> io::Result<()> {
    let path = manifest_path();
    if !path.is_file() {
        return Ok(());
    }
    let kept: Vec<String> = BufReader::new(fs::File::open(&path)?)
        .lines()
        .map_while(Result::ok)
        .filter(|line| {
            serde_json::from_str::<BackupRecord>(line)
                .map(|r| r.id != id)
                .unwrap_or(true)
        })
        .collect();
    let mut f = fs::File::create(&path)?;
    for line in kept {
        writeln!(f, "{line}")?;
    }
    Ok(())
}
