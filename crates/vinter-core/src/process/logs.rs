//! Per-run log files under `<bottle>/logs/`.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::error::IoResultExt;

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
    }
}
