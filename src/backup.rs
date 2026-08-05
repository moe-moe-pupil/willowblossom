use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use bevy::prelude::*;
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::{
    battle_round::BattleRoundStore,
    deepseek::DeepseekManager,
    napcat::NapcatMessageManager,
    scene::VoxelSceneStore,
};

pub const DATA_DIR: &str = ".data/willowblossom";
pub const BACKUP_ROOT: &str = ".data/willowblossom/backups";

const BACKUP_SETTINGS_PATH: &str = ".data/willowblossom/backup_settings.toml";
const DEFAULT_INTERVAL_MINUTES: u32 = 10;
const DEFAULT_MAX_BACKUPS: usize = 30;
const MAX_INTERVAL_MINUTES: u32 = 24 * 60;
const MAX_KEPT_BACKUPS: usize = 200;

#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct BackupSettings {
    #[serde(default = "default_auto_backup_enabled")]
    pub auto_backup_enabled: bool,
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,
    #[serde(default = "default_max_backups")]
    pub max_backups: usize,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            auto_backup_enabled: default_auto_backup_enabled(),
            interval_minutes: default_interval_minutes(),
            max_backups: default_max_backups(),
        }
    }
}

impl BackupSettings {
    pub(crate) fn interval_seconds(&self) -> f32 {
        self.interval_minutes.clamp(1, MAX_INTERVAL_MINUTES) as f32 * 60.0
    }
}

fn default_auto_backup_enabled() -> bool { true }

fn default_interval_minutes() -> u32 { DEFAULT_INTERVAL_MINUTES }

fn default_max_backups() -> usize { DEFAULT_MAX_BACKUPS }

#[derive(Resource)]
pub(crate) struct BackupState {
    elapsed_seconds: f32,
    triggered: bool,
    pub(crate) manual_requested: bool,
    pub(crate) selected_backup: Option<String>,
    pub(crate) pending_restore: bool,
    pub(crate) last_backup_name: Option<String>,
    pub(crate) last_result: String,
}

impl Default for BackupState {
    fn default() -> Self {
        Self {
            elapsed_seconds: 0.0,
            triggered: false,
            manual_requested: false,
            selected_backup: None,
            pending_restore: false,
            last_backup_name: None,
            last_result: String::new(),
        }
    }
}

impl BackupState {
    pub(crate) fn elapsed_seconds(&self) -> f32 { self.elapsed_seconds }
}

pub struct BackupPlugin;

impl Plugin for BackupPlugin {
    fn build(&self, app: &mut App) {
        let settings = Persistent::<BackupSettings>::builder()
            .name("backup_settings")
            .format(StorageFormat::Toml)
            .path(BACKUP_SETTINGS_PATH)
            .default(BackupSettings::default())
            .build()
            .expect("failed to init backup settings");
        app.insert_resource(settings);
        app.init_resource::<BackupState>();
        app.add_systems(
            Update,
            (
                backup_timer_system,
                execute_backup_system.run_if(backup_triggered),
            )
                .chain(),
        );
    }
}

fn backup_triggered(state: Res<BackupState>) -> bool { state.triggered }

fn backup_timer_system(
    time: Res<Time>,
    settings: Res<Persistent<BackupSettings>>,
    mut state: ResMut<BackupState>,
) {
    if state.manual_requested {
        state.manual_requested = false;
        state.elapsed_seconds = 0.0;
        state.triggered = true;
        return;
    }
    if !settings.auto_backup_enabled {
        state.elapsed_seconds = 0.0;
        return;
    }
    state.elapsed_seconds += time.delta_secs();
    if state.elapsed_seconds >= settings.interval_seconds() {
        state.elapsed_seconds = 0.0;
        state.triggered = true;
    }
}

fn execute_backup_system(
    mut state: ResMut<BackupState>,
    settings: Res<Persistent<BackupSettings>>,
    message_manager: Option<ResMut<Persistent<NapcatMessageManager>>>,
    deepseek_manager: Option<ResMut<Persistent<DeepseekManager>>>,
    scene_store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    battle_store: Option<ResMut<Persistent<BattleRoundStore>>>,
) {
    state.triggered = false;

    // Flush the main stores so the snapshot does not lag behind in-memory state.
    let mut flush_errors = Vec::new();
    if let Some(store) = message_manager {
        if let Err(err) = store.persist() {
            flush_errors.push(format!("聊天记录：{err}"));
        }
    }
    if let Some(store) = deepseek_manager {
        if let Err(err) = store.persist() {
            flush_errors.push(format!("DeepSeek总结：{err}"));
        }
    }
    if let Some(store) = scene_store {
        if let Err(err) = store.persist() {
            flush_errors.push(format!("体素场景：{err}"));
        }
    }
    if let Some(store) = battle_store {
        if let Err(err) = store.persist() {
            flush_errors.push(format!("战斗轮：{err}"));
        }
    }

    match run_backup(
        Path::new(DATA_DIR),
        Path::new(BACKUP_ROOT),
        settings.max_backups,
    ) {
        Ok(backup_dir) => {
            let name = backup_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "?".to_owned());
            state.last_backup_name = Some(name);
            state.last_result = if flush_errors.is_empty() {
                "备份完成".to_owned()
            } else {
                format!(
                    "备份完成，但部分数据落盘失败（可能略旧）：{}",
                    flush_errors.join("；")
                )
            };
        },
        Err(err) => {
            state.last_result = format!("备份失败：{err}");
        },
    }
}

fn run_backup(data_dir: &Path, backup_root: &Path, max_backups: usize) -> Result<PathBuf, String> {
    let stamp = format_unix_timestamp(unix_now());
    let backup_dir = unique_backup_dir(backup_root, &stamp)?;
    write_snapshot(data_dir, &backup_dir, &stamp)?;
    trim_backups(backup_root, max_backups)?;
    Ok(backup_dir)
}

fn unique_backup_dir(backup_root: &Path, stamp: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(backup_root).map_err(|err| format!("创建备份目录失败：{err}"))?;
    let mut candidate = backup_root.join(stamp);
    let mut suffix = 2u32;
    while candidate.exists() {
        candidate = backup_root.join(format!("{stamp}-{suffix}"));
        suffix += 1;
    }
    Ok(candidate)
}

pub(crate) fn list_backups(backup_root: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    if !backup_root.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(backup_root).map_err(|err| format!("读取备份目录失败：{err}"))?
    {
        let entry = entry.map_err(|err| format!("读取备份目录条目失败：{err}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() && is_backup_dir_name(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Copies a snapshot back into the data directory.
///
/// The backup's own manifest and the current backup settings are skipped so a
/// restore never overwrites the backup feature's live configuration.
pub(crate) fn restore_backup(
    backup_root: &Path,
    name: &str,
    data_dir: &Path,
) -> Result<Vec<String>, String> {
    if !is_backup_dir_name(name) {
        return Err(format!("不是有效的备份目录：{name}"));
    }
    let backup_dir = backup_root.join(name);
    if !backup_dir.is_dir() {
        return Err(format!("备份不存在：{name}"));
    }
    let mut restored = Vec::new();
    for entry in fs::read_dir(&backup_dir).map_err(|err| format!("读取备份失败：{err}"))? {
        let entry = entry.map_err(|err| format!("读取备份条目失败：{err}"))?;
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        if entry_name == "manifest.json" || entry_name == "backup_settings.toml" {
            continue;
        }
        let source = entry.path();
        let destination = data_dir.join(&entry_name);
        if source.is_dir() {
            copy_dir_recursive(&source, &destination)?;
        } else {
            fs::copy(&source, &destination)
                .map_err(|err| format!("恢复 {entry_name} 失败：{err}"))?;
        }
        restored.push(entry_name);
    }
    Ok(restored)
}

fn write_snapshot(data_dir: &Path, backup_dir: &Path, stamp: &str) -> Result<Vec<String>, String> {
    fs::create_dir_all(backup_dir).map_err(|err| format!("创建备份目录失败：{err}"))?;
    let mut copied = Vec::new();
    for (name, source) in snapshot_sources(data_dir)? {
        let destination = backup_dir.join(&name);
        if source.is_dir() {
            copy_dir_recursive(&source, &destination)?;
        } else {
            fs::copy(&source, &destination).map_err(|err| format!("复制 {name} 失败：{err}"))?;
        }
        copied.push(name);
    }
    let manifest = serde_json::json!({
        "created_at": stamp,
        "files": copied,
    });
    let manifest_text = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("生成备份清单失败：{err}"))?;
    fs::write(
        backup_dir.join("manifest.json"),
        manifest_text,
    )
    .map_err(|err| format!("写入备份清单失败：{err}"))?;
    Ok(copied)
}

fn snapshot_sources(data_dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut sources = Vec::new();
    let entries = fs::read_dir(data_dir).map_err(|err| format!("读取数据目录失败：{err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("读取数据目录条目失败：{err}"))?;
        let path = entry.path();
        if path.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            sources.push((name, path));
        }
    }
    let standees = data_dir.join("character_standees");
    if standees.is_dir() {
        sources.push((
            "character_standees".to_owned(),
            standees,
        ));
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(sources)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|err| format!("创建目录失败：{err}"))?;
    for entry in fs::read_dir(source).map_err(|err| format!("读取目录失败：{err}"))? {
        let entry = entry.map_err(|err| format!("读取目录条目失败：{err}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).map_err(|err| {
                format!(
                    "复制 {} 失败：{err}",
                    source_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn trim_backups(backup_root: &Path, max_backups: usize) -> Result<usize, String> {
    if !backup_root.is_dir() {
        return Ok(0);
    }
    let mut backups = Vec::new();
    for entry in fs::read_dir(backup_root).map_err(|err| format!("读取备份目录失败：{err}"))?
    {
        let entry = entry.map_err(|err| format!("读取备份目录条目失败：{err}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() && is_backup_dir_name(&name) {
            backups.push((name, path));
        }
    }
    backups.sort_by(|left, right| left.0.cmp(&right.0));
    let keep = max_backups.clamp(1, MAX_KEPT_BACKUPS);
    let removed = backups.len().saturating_sub(keep);
    for (_, path) in backups.into_iter().take(removed) {
        fs::remove_dir_all(&path).map_err(|err| {
            format!(
                "删除旧备份 {} 失败：{err}",
                path.display()
            )
        })?;
    }
    Ok(removed)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn format_unix_timestamp(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = unix_components(secs);
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

fn unix_components(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86_400;
    let seconds_of_day = secs % 86_400;
    let hour = (seconds_of_day / 3_600) as u32;
    let minute = ((seconds_of_day % 3_600) / 60) as u32;
    let second = (seconds_of_day % 60) as u32;
    let (year, month, day) = civil_from_days(days as i64);
    (
        year as u32,
        month,
        day,
        hour,
        minute,
        second,
    )
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar (UTC).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

fn is_backup_dir_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 15 || bytes[8] != b'-' {
        return false;
    }
    if !bytes[..8].iter().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if !bytes[9..15].iter().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if bytes.len() == 15 {
        return true;
    }
    bytes[15..]
        .strip_prefix(b"-")
        .is_some_and(|rest| !rest.is_empty() && rest.iter().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn unix_timestamp_formats_utc_components() {
        assert_eq!(
            format_unix_timestamp(0),
            "19700101-000000"
        );
        assert_eq!(
            format_unix_timestamp(86_400),
            "19700102-000000"
        );
        assert_eq!(
            format_unix_timestamp(86_400 + 12 * 3_600 + 34 * 60 + 56),
            "19700102-123456"
        );
        // 2026-08-04 00:00:00 UTC.
        assert_eq!(
            format_unix_timestamp(1_785_801_600),
            "20260804-000000"
        );
    }

    #[test]
    fn backup_dir_name_pattern_only_accepts_snapshots() {
        assert!(is_backup_dir_name("20260805-000000"));
        assert!(is_backup_dir_name("20260805-000000-2"));
        assert!(!is_backup_dir_name("backup-20260805"));
        assert!(!is_backup_dir_name("20260805"));
        assert!(!is_backup_dir_name("20260805-000000-x"));
        assert!(!is_backup_dir_name("notes"));
    }

    #[test]
    fn snapshot_copies_data_files_and_standees_but_skips_cache_dirs() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let backup_root = temp.path().join("backups");
        fs::create_dir_all(data_dir.join("character_standees")).unwrap();
        fs::create_dir_all(data_dir.join("tts")).unwrap();
        fs::create_dir_all(data_dir.join("image_cache")).unwrap();
        fs::write(
            data_dir.join("messages.toml"),
            "messages",
        )
        .unwrap();
        fs::write(data_dir.join("voxel_scene.bin"), [
            1, 2, 3,
        ])
        .unwrap();
        fs::write(
            data_dir.join("character_standees/player.png"),
            "png",
        )
        .unwrap();
        fs::write(data_dir.join("tts/voice.wav"), "huge").unwrap();
        fs::write(
            data_dir.join("image_cache/cached.png"),
            "cache",
        )
        .unwrap();

        let backup_dir = unique_backup_dir(&backup_root, "20260805-000000").unwrap();
        let copied = write_snapshot(
            &data_dir,
            &backup_dir,
            "20260805-000000",
        )
        .unwrap();

        assert!(copied.contains(&"messages.toml".to_owned()));
        assert!(copied.contains(&"voxel_scene.bin".to_owned()));
        assert!(copied.contains(&"character_standees".to_owned()));
        assert!(!copied.contains(&"tts".to_owned()));
        assert!(!copied.contains(&"image_cache".to_owned()));
        assert_eq!(
            fs::read_to_string(backup_dir.join("messages.toml")).unwrap(),
            "messages"
        );
        assert_eq!(
            fs::read_to_string(backup_dir.join("character_standees/player.png")).unwrap(),
            "png"
        );
        assert!(!backup_dir.join("tts").exists());
        assert!(!backup_dir.join("image_cache").exists());
        assert!(backup_dir.join("manifest.json").exists());
    }

    #[test]
    fn trim_backups_keeps_newest_and_ignores_unrelated_dirs() {
        let temp = tempdir().unwrap();
        let backup_root = temp.path().join("backups");
        for name in [
            "20260805-000000",
            "20260805-010000",
            "20260805-020000",
            "20260805-030000",
        ] {
            fs::create_dir_all(backup_root.join(name)).unwrap();
        }
        fs::create_dir_all(backup_root.join("notes")).unwrap();

        let removed = trim_backups(&backup_root, 2).unwrap();

        assert_eq!(removed, 2);
        assert!(!backup_root.join("20260805-000000").exists());
        assert!(!backup_root.join("20260805-010000").exists());
        assert!(backup_root.join("20260805-020000").exists());
        assert!(backup_root.join("20260805-030000").exists());
        assert!(backup_root.join("notes").exists());
    }

    #[test]
    fn run_backup_creates_snapshot_and_trims_old_ones() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let backup_root = temp.path().join("backups");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(data_dir.join("messages.toml"), "one").unwrap();

        let first = run_backup(&data_dir, &backup_root, 1).unwrap();
        fs::write(data_dir.join("messages.toml"), "two").unwrap();
        let second = run_backup(&data_dir, &backup_root, 1).unwrap();

        assert_ne!(first, second);
        assert_eq!(
            fs::read_to_string(second.join("messages.toml")).unwrap(),
            "two"
        );
        let entries = fs::read_dir(&backup_root).unwrap().count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn list_backups_returns_sorted_snapshot_dirs_only() {
        let temp = tempdir().unwrap();
        let backup_root = temp.path().join("backups");
        fs::create_dir_all(backup_root.join("20260805-020000")).unwrap();
        fs::create_dir_all(backup_root.join("20260805-000000")).unwrap();
        fs::create_dir_all(backup_root.join("20260805-010000-2")).unwrap();
        fs::create_dir_all(backup_root.join("notes")).unwrap();

        let names = list_backups(&backup_root).unwrap();

        assert_eq!(names, vec![
            "20260805-000000".to_owned(),
            "20260805-010000-2".to_owned(),
            "20260805-020000".to_owned(),
        ]);
    }

    #[test]
    fn restore_backup_copies_data_but_skips_manifest_and_settings() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("data");
        let backup_root = temp.path().join("backups");
        let backup_dir = backup_root.join("20260805-000000");
        fs::create_dir_all(data_dir.join("character_standees")).unwrap();
        fs::create_dir_all(backup_dir.join("character_standees")).unwrap();
        fs::write(
            data_dir.join("messages.toml"),
            "old chat",
        )
        .unwrap();
        fs::write(
            data_dir.join("backup_settings.toml"),
            "current settings",
        )
        .unwrap();
        fs::write(
            backup_dir.join("messages.toml"),
            "restored chat",
        )
        .unwrap();
        fs::write(backup_dir.join("voxel_scene.bin"), [
            9, 9, 9,
        ])
        .unwrap();
        fs::write(
            backup_dir.join("character_standees/player.png"),
            "restored png",
        )
        .unwrap();
        fs::write(backup_dir.join("manifest.json"), "{}").unwrap();
        fs::write(
            backup_dir.join("backup_settings.toml"),
            "old settings",
        )
        .unwrap();

        let restored = restore_backup(
            &backup_root,
            "20260805-000000",
            &data_dir,
        )
        .unwrap();

        assert!(restored.contains(&"messages.toml".to_owned()));
        assert!(restored.contains(&"voxel_scene.bin".to_owned()));
        assert!(restored.contains(&"character_standees".to_owned()));
        assert!(!restored.contains(&"manifest.json".to_owned()));
        assert!(!restored.contains(&"backup_settings.toml".to_owned()));
        assert_eq!(
            fs::read_to_string(data_dir.join("messages.toml")).unwrap(),
            "restored chat"
        );
        assert_eq!(
            fs::read_to_string(data_dir.join("backup_settings.toml")).unwrap(),
            "current settings"
        );
        assert_eq!(
            fs::read_to_string(data_dir.join("character_standees/player.png")).unwrap(),
            "restored png"
        );
        assert!(!data_dir.join("manifest.json").exists());
    }

    #[test]
    fn restore_backup_rejects_invalid_names_and_missing_dirs() {
        let temp = tempdir().unwrap();
        let backup_root = temp.path().join("backups");

        assert!(restore_backup(&backup_root, "notes", temp.path()).is_err());
        assert!(
            restore_backup(
                &backup_root,
                "20260805-000000",
                temp.path()
            )
            .is_err(),
            "a missing snapshot must be reported instead of silently succeeding"
        );
    }
}
