//! 软件内操作日志：把关键操作、状态变化和后台任务的耗时追加写入
//! `.data/willowblossom/logs/operations.log`，方便事后浏览和排查性能问题。
//!
//! 日志按"分类 + 一句话描述"组织（如 `replay` / `video` / `speech` /
//! `status`），每次应用启动会在文件里写一条会话分隔线。

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;

#[derive(Resource)]
pub(crate) struct OpLog {
    file: Mutex<Option<BufWriter<File>>>,
}

impl Default for OpLog {
    fn default() -> Self {
        Self::new()
    }
}

impl OpLog {
    pub(crate) fn new() -> Self {
        Self::new_at(log_file_path())
    }

    /// `new()` 的可注入版本；测试用临时文件，避免污染真实日志。
    fn new_at(path: Option<PathBuf>) -> Self {
        let mut file = None;
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match OpenOptions::new().create(true).append(true).open(&path) {
                Ok(opened) => {
                    let mut writer = BufWriter::new(opened);
                    let _ = writeln!(writer, "===== 会话开始 {} =====", local_timestamp());
                    let _ = writer.flush();
                    file = Some(writer);
                },
                Err(err) => eprintln!("failed to open operation log {}: {err}", path.display()),
            }
        }
        Self {
            file: Mutex::new(file),
        }
    }

    /// 记录一条操作日志。低频操作逐条落盘并立即 flush，崩溃也不丢。
    pub(crate) fn record(&self, category: &str, message: impl AsRef<str>) {
        let line = format!(
            "{} [{category}] {}",
            local_timestamp(),
            message.as_ref().replace('\n', " ")
        );
        if let Ok(mut guard) = self.file.lock() {
            if let Some(writer) = guard.as_mut() {
                let _ = writeln!(writer, "{line}");
                let _ = writer.flush();
            }
        }
    }
}

fn log_file_path() -> Option<PathBuf> {
    Some(
        Path::new(".data")
            .join("willowblossom")
            .join("logs")
            .join("operations.log"),
    )
}

/// 本地时间戳 `YYYY-MM-DD HH:MM:SS.mmm`；取不到本地时间时退回 UTC 毫秒。
fn local_timestamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY：GetLocalTime 只是读取本地时间，无副作用。
        unsafe {
            let mut system_time: windows_sys::Win32::Foundation::SYSTEMTIME =
                std::mem::zeroed();
            windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut system_time);
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                system_time.wYear,
                system_time.wMonth,
                system_time.wDay,
                system_time.wHour,
                system_time.wMinute,
                system_time.wSecond,
                system_time.wMilliseconds
            );
        }
    }
    #[cfg(not(windows))]
    {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        format!("utc-ms {millis}")
    }}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_timestamped_and_categorized_on_disk() {
        let path = std::env::temp_dir().join(format!(
            "willowblossom-oplog-test-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let log = OpLog::new_at(Some(path.clone()));
        log.record("test", "hello\nworld");
        drop(log);
        let content = std::fs::read_to_string(&path).expect("log file");
        let _ = std::fs::remove_file(&path);
        let entry = content
            .lines()
            .last()
            .expect("at least one entry");
        assert!(entry.contains("[test]"), "missing category: {entry}");
        // 换行会被折叠成空格，保证每条日志占一行。
        assert!(!entry.contains('\n'), "entry contains newline: {entry}");
        assert!(entry.contains("hello world"), "message not normalized: {entry}");
        // 时间戳以 4 位年份开头（Windows 本地时间）。
        #[cfg(windows)]
        {
            let year_prefix = &entry[0..4];
            assert!(year_prefix.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
