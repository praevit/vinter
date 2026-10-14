//! Per-run log files under `<bottle>/logs/`.

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{IoResultExt, validate_id};
use crate::{Error, Result};

/// Most of a log returned by [`read`]; older lines are cut.
const MAX_READ: u64 = 512 * 1024;

/// One log file, as listed in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LogEntry {
    /// File name; pass it to [`read`].
    pub id: String,
    /// What ran, e.g. "wineboot" or "7-Zip File Manager".
    pub label: String,
    /// RFC 3339 start time.
    pub started_at: String,
    pub size: u64,
}

/// Logs of a bottle, newest first.
pub fn list(bottle_dir: &Path) -> Result<Vec<LogEntry>> {
    let dir = bottle_dir.join(LOGS_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&dir),
    };
    let mut logs: Vec<LogEntry> = entries
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().into_string().ok()?;
            let (started_at, label) = parse_name(&id)?;
            Some(LogEntry {
                size: e.metadata().ok()?.len(),
                id,
                label,
                started_at,
            })
        })
        .collect();
    logs.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(logs)
}

/// Contents of one log (the last 512 KB if it is bigger).
pub fn read(bottle_dir: &Path, id: &str) -> Result<String> {
    validate_id(id)?;
    if parse_name(id).is_none() {
        return Err(Error::InvalidId(id.to_string()));
    }
    let path = bottle_dir.join(LOGS_DIR).join(id);
    let mut file = File::open(&path).at(&path)?;
    let len = file.metadata().at(&path)?.len();
    let mut bytes = Vec::new();
    if len > MAX_READ {
        use std::io::{Read, Seek, SeekFrom};
        file.seek(SeekFrom::Start(len - MAX_READ)).at(&path)?;
        file.take(MAX_READ).read_to_end(&mut bytes).at(&path)?;
    } else {
        use std::io::Read;
        file.read_to_end(&mut bytes).at(&path)?;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `2026-10-14T22-06-24Z-7-Zip_File_Manager.log` -> (RFC 3339 time, label).
fn parse_name(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".log")?;
    let (stamp, label) = (stem.get(..20)?, stem.get(21..)?);
    let (date, time) = stamp.split_once('T')?;
    let time = time.strip_suffix('Z')?.replace('-', ":");
    Some((format!("{date}T{time}Z"), label.replace('_', " ")))
}

pub const LOGS_DIR: &str = "logs";

/// Creates `logs/<timestamp>-<label>.log` in a bottle folder.
pub fn create(bottle_dir: &Path, label: &str) -> Result<(PathBuf, File)> {
    let dir = bottle_dir.join(LOGS_DIR);
    std::fs::create_dir_all(&dir).at(&dir)?;
    let stamp = humantime::format_rfc3339_seconds(std::time::SystemTime::now())
        .to_string()
        .replace(':', "-");
    let label: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{stamp}-{label}.log"));
    let file = File::options()
        .create(true)
        .append(true)
        .open(&path)
        .at(&path)?;
    Ok((path, file))
}

/// The last `lines` lines of a log, for error messages.
pub fn tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_named_log_and_tails_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, _) = create(tmp.path(), "wine boot").unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("-wine_boot.log"), "{name}");
        assert!(!name.contains(':'));
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        assert_eq!(tail(&path, 2), "b\nc");
        assert_eq!(tail(&path, 10), "a\nb\nc");

        let listed = list(tmp.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "wine boot");
        assert!(listed[0].started_at.ends_with('Z') && listed[0].started_at.contains(':'));
        assert_eq!(read(tmp.path(), &listed[0].id).unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn read_rejects_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("bottle.toml"), "secret").unwrap();
        for bad in ["../bottle.toml", "bottle.toml", "x.log", "/etc/passwd"] {
            assert!(read(tmp.path(), bad).is_err(), "{bad}");
        }
        assert!(list(tmp.path()).unwrap().is_empty());
    }
}
