//! Automatic backups: every bottle backed up to a folder the user picks,
//! daily or weekly, keeping the newest few. Settings are in
//! `auto-backup.toml` in the data folder; the app runs the schedule.
//!
//! A bottle's automatic backups are found by name in the folder
//! (`<name> (auto <id>) <date>.vinterbottle`), so the folder itself is the
//! record of when each bottle was last backed up.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::bottle::{self, backup};
use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result};

const FILE: &str = "auto-backup.toml";
pub const MAX_KEEP: u32 = 50;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Frequency {
    #[default]
    Daily,
    Weekly,
}

impl Frequency {
    pub fn interval(self) -> Duration {
        let day = Duration::from_secs(24 * 60 * 60);
        match self {
            Self::Daily => day,
            Self::Weekly => day * 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AutoBackupFile")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AutoBackupSettings {
    pub enabled: bool,
    pub frequency: Frequency,
    /// Folder the backups go to.
    pub folder: Option<String>,
    /// Backups kept per bottle; older ones are removed.
    pub keep: u32,
}

/// [`AutoBackupSettings`] as read: missing fields keep their defaults, so the
/// API type (and its generated TS) has no optional fields.
#[derive(Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
struct AutoBackupFile {
    enabled: bool,
    frequency: Frequency,
    folder: Option<String>,
    keep: u32,
}

impl Default for AutoBackupFile {
    fn default() -> Self {
        let d = AutoBackupSettings::default();
        Self {
            enabled: d.enabled,
            frequency: d.frequency,
            folder: d.folder,
            keep: d.keep,
        }
    }
}

impl From<AutoBackupFile> for AutoBackupSettings {
    fn from(f: AutoBackupFile) -> Self {
        Self {
            enabled: f.enabled,
            frequency: f.frequency,
            folder: f.folder,
            keep: f.keep,
        }
    }
}

impl Default for AutoBackupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency: Frequency::Daily,
            folder: None,
            keep: 3,
        }
    }
}

/// When one bottle was last backed up automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BottleBackup {
    pub bottle_id: String,
    pub name: String,
    /// RFC 3339, or null if never (in the current folder).
    pub last: Option<String>,
}

/// The saved settings, or the defaults.
pub fn load(paths: &AppPaths) -> AutoBackupSettings {
    std::fs::read_to_string(paths.data_dir().join(FILE))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(paths: &AppPaths, settings: &AutoBackupSettings) -> Result<()> {
    if settings.keep == 0 || settings.keep > MAX_KEEP {
        return Err(Error::InvalidSetting(format!(
            "keep between 1 and {MAX_KEEP} backups"
        )));
    }
    if let Some(folder) = &settings.folder {
        if !Path::new(folder).is_absolute() {
            return Err(Error::InvalidSetting(format!(
                "{folder} is not a full path"
            )));
        }
    }
    if settings.enabled && settings.folder.is_none() {
        return Err(Error::InvalidSetting(
            "choose a folder for the backups first".into(),
        ));
    }
    let dir = paths.data_dir();
    std::fs::create_dir_all(dir).at(dir)?;
    let file = dir.join(FILE);
    let tmp = dir.join(format!(".{FILE}.tmp"));
    let text = toml::to_string_pretty(settings).expect("settings serialize");
    std::fs::write(&tmp, text).at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)
}

/// The part of a backup's file name that ties it to a bottle.
fn tag(bottle_id: &str) -> String {
    let short: String = bottle_id.chars().take(8).collect();
    format!("(auto {short}) ")
}

/// `<name> (auto <id>) 2026-10-25 143000.vinterbottle`, in UTC.
fn file_name(name: &str, bottle_id: &str, at: SystemTime) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let stamp = humantime::format_rfc3339_seconds(at).to_string();
    let stamp = stamp[..19].replace('T', " ").replace(':', "");
    format!("{safe} {}{stamp}.{}", tag(bottle_id), backup::EXTENSION)
}

/// A bottle's automatic backups in `folder`, newest first.
pub fn backups_of(folder: &Path, bottle_id: &str) -> Vec<(PathBuf, SystemTime)> {
    let tag = tag(bottle_id);
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == backup::EXTENSION))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().contains(&tag))
        })
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((p, modified))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    found
}

/// When a bottle was last backed up to `folder`.
pub fn last_backup(folder: &Path, bottle_id: &str) -> Option<SystemTime> {
    backups_of(folder, bottle_id).first().map(|b| b.1)
}

/// Whether a backup made at `last` is old enough for another one.
pub fn is_due(last: Option<SystemTime>, frequency: Frequency, now: SystemTime) -> bool {
    match last {
        None => true,
        // A few minutes' slack, so a daily backup does not drift later each day.
        Some(t) => now
            .duration_since(t)
            .is_ok_and(|age| age + Duration::from_secs(10 * 60) >= frequency.interval()),
    }
}

/// Removes all but the newest `keep` automatic backups of a bottle.
pub fn prune(folder: &Path, bottle_id: &str, keep: u32) -> Result<usize> {
    let old: Vec<_> = backups_of(folder, bottle_id)
        .into_iter()
        .skip(keep.max(1) as usize)
        .collect();
    for (path, _) in &old {
        std::fs::remove_file(path).at(path)?;
    }
    Ok(old.len())
}

/// Each bottle and its last automatic backup.
pub fn status(paths: &AppPaths) -> Result<Vec<BottleBackup>> {
    let settings = load(paths);
    let folder = settings.folder.as_deref().map(Path::new);
    Ok(bottle::list(paths)?
        .into_iter()
        .map(|b| BottleBackup {
            last: folder
                .and_then(|f| last_backup(f, &b.id))
                .map(|t| humantime::format_rfc3339_seconds(t).to_string()),
            bottle_id: b.id,
            name: b.name,
        })
        .collect())
}

/// Bottles whose backup is due now (all set-up bottles with `force`). Empty
/// while automatic backups are off or the folder is missing (an unplugged
/// drive, say).
pub fn due(paths: &AppPaths, now: SystemTime, force: bool) -> Result<Vec<String>> {
    let settings = load(paths);
    let Some(folder) = settings.folder.as_deref().map(Path::new) else {
        return Ok(Vec::new());
    };
    if !(settings.enabled || force) || !folder.is_dir() {
        return Ok(Vec::new());
    }
    Ok(bottle::list(paths)?
        .into_iter()
        .filter(|b| b.setup_complete)
        .filter(|b| force || is_due(last_backup(folder, &b.id), settings.frequency, now))
        .map(|b| b.id)
        .collect())
}

/// Backs up one bottle into the folder, then removes its oldest backups.
/// Close the bottle's programs first.
pub fn run(paths: &AppPaths, bottle_id: &str) -> Result<PathBuf> {
    let settings = load(paths);
    let folder = settings
        .folder
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| Error::InvalidSetting("no folder for automatic backups".into()))?;
    let b = bottle::get(paths, bottle_id)?;
    let dest = folder.join(file_name(&b.name, &b.id, SystemTime::now()));
    backup::export(paths, bottle_id, &dest, |_, _| {})?;
    prune(&folder, bottle_id, settings.keep)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_and_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let s = load(&paths);
        assert!(!s.enabled);
        assert_eq!(s.keep, 3);
        assert!(
            save(
                &paths,
                &AutoBackupSettings {
                    enabled: true,
                    ..s.clone()
                }
            )
            .is_err()
        );
        assert!(
            save(
                &paths,
                &AutoBackupSettings {
                    keep: 0,
                    ..s.clone()
                }
            )
            .is_err()
        );
        let partial: AutoBackupSettings = toml::from_str("enabled = true").unwrap();
        assert_eq!(partial.keep, 3);
        let s = AutoBackupSettings {
            enabled: true,
            frequency: Frequency::Weekly,
            folder: Some(tmp.path().join("b").display().to_string()),
            keep: 5,
        };
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths), s);
    }

    #[test]
    fn due_and_prune() {
        let now = SystemTime::now();
        let hour = Duration::from_secs(3600);
        assert!(is_due(None, Frequency::Daily, now));
        assert!(!is_due(Some(now - hour * 5), Frequency::Daily, now));
        assert!(is_due(Some(now - hour * 24), Frequency::Daily, now));
        assert!(!is_due(Some(now - hour * 24), Frequency::Weekly, now));

        let tmp = tempfile::tempdir().unwrap();
        let id = "0123456789ab";
        for i in 0..5u64 {
            let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + i * 86_400);
            let path = tmp.path().join(file_name("My: App", id, at));
            std::fs::write(&path, b"x").unwrap();
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(at).unwrap();
        }
        // Another bottle's backup and a half-written one are left alone.
        std::fs::write(tmp.path().join(file_name("Other", "zzzzzzzz", now)), b"x").unwrap();
        std::fs::write(
            tmp.path()
                .join("My App (auto 01234567) x.vinterbottle.part"),
            b"x",
        )
        .unwrap();

        let all = backups_of(tmp.path(), id);
        assert_eq!(all.len(), 5);
        let newest = all[0].0.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(
            newest,
            "My- App (auto 01234567) 2027-01-19 080000.vinterbottle"
        );

        assert_eq!(prune(tmp.path(), id, 3).unwrap(), 2);
        assert_eq!(backups_of(tmp.path(), id).len(), 3);
        assert_eq!(
            backups_of(tmp.path(), id)[0]
                .0
                .file_name()
                .unwrap()
                .to_string_lossy(),
            newest
        );
        assert_eq!(backups_of(tmp.path(), "zzzzzzzz").len(), 1);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 5);
    }
}
