//! Disk usage, and cleaning up files Vinter or Wine no longer need.

use std::path::Path;

use serde::Serialize;

use crate::bottle::{self, PREFIX_DIR};
use crate::error::IoResultExt;
use crate::process::logs::LOGS_DIR;
use crate::{AppPaths, Result};

/// Logs kept per bottle by [`clean_bottle`]: the newest ones.
const KEEP_LOGS: usize = 10;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BottleUsage {
    pub id: String,
    pub name: String,
    pub bytes: u64,
}

/// What Vinter's data folder holds.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Usage {
    /// Largest first.
    pub bottles: Vec<BottleUsage>,
    pub runtimes: u64,
    /// Graphics components (DXVK and the like).
    pub components: u64,
    /// Leftover downloads: installers and archives from setups that stopped.
    pub downloads: u64,
}

/// Total size of regular files under `dir`, not following symlinks (a
/// prefix links to `/` as its Z: drive). 0 if it does not exist.
pub fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Size of a bottle's files. Snapshots are left out: they are mostly clones
/// sharing the bottle's space, and counting them would multiply its size.
fn bottle_size(dir: &Path) -> u64 {
    let snapshots = dir.join(bottle::snapshot::SNAPSHOTS_DIR);
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.path() != snapshots)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Disk usage of everything. Walks every bottle, so it can take a moment.
pub fn usage(paths: &AppPaths) -> Result<Usage> {
    let mut bottles: Vec<BottleUsage> = bottle::list(paths)?
        .into_iter()
        .map(|b| BottleUsage {
            bytes: bottle::dir(paths, &b.id)
                .map(|d| bottle_size(&d))
                .unwrap_or(0),
            id: b.id,
            name: b.name,
        })
        .collect();
    bottles.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    Ok(Usage {
        bottles,
        runtimes: dir_size(&paths.runtimes_dir()),
        components: dir_size(&paths.data_dir().join("components")),
        downloads: dir_size(&paths.downloads_dir()),
    })
}

/// Removes leftover downloads. Returns the bytes freed. Do not call while a
/// runtime, component or recipe is downloading.
pub fn clear_downloads(paths: &AppPaths) -> Result<u64> {
    let dir = paths.downloads_dir();
    let freed = dir_size(&dir);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(freed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e).at(&dir),
    }
}

/// Removes a bottle's old logs (keeping the newest few) and Windows' temp
/// files. Returns the bytes freed. Close the bottle's programs first.
pub fn clean_bottle(paths: &AppPaths, id: &str) -> Result<u64> {
    let dir = bottle::dir(paths, id)?;
    bottle::get(paths, id)?;
    let mut freed = 0;

    let mut logs: Vec<_> = std::fs::read_dir(dir.join(LOGS_DIR))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    // Log names start with a timestamp, so they sort oldest first.
    logs.sort();
    let old = logs.len().saturating_sub(KEEP_LOGS);
    for log in &logs[..old] {
        freed += std::fs::symlink_metadata(log).map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(log).at(log)?;
    }

    let drive_c = dir.join(PREFIX_DIR).join("drive_c");
    let mut temps = vec![drive_c.join("windows").join("temp")];
    if let Ok(users) = std::fs::read_dir(drive_c.join("users")) {
        for user in users.filter_map(|e| e.ok()) {
            temps.push(user.path().join("AppData").join("Local").join("Temp"));
            temps.push(user.path().join("Temp"));
        }
    }
    for temp in temps {
        freed += empty_dir(&temp)?;
    }
    Ok(freed)
}

/// Deletes what is inside `dir` (not `dir` itself, and never through a
/// symlink). Returns the bytes freed.
fn empty_dir(dir: &Path) -> Result<u64> {
    let is_real_dir = std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir());
    if !is_real_dir {
        return Ok(0);
    }
    let mut freed = 0;
    for entry in std::fs::read_dir(dir).at(dir)?.filter_map(|e| e.ok()) {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).at(&path)?;
        if meta.is_dir() {
            freed += dir_size(&path);
            std::fs::remove_dir_all(&path).at(&path)?;
        } else if meta.is_symlink() {
            // Only the link goes; what it points to stays.
            std::fs::remove_file(&path).at(&path)?;
        } else {
            freed += meta.len();
            std::fs::remove_file(&path).at(&path)?;
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Bottle, Graphics, WindowsVersion};

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn measures_and_cleans_without_following_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let dir = bottle::dir(&paths, "b").unwrap();
        Bottle {
            id: "b".into(),
            name: "B".into(),
            runtime: "rt".into(),
            created_at: "2026-10-15T00:00:00Z".into(),
            windows_version: WindowsVersion::Win10,
            graphics: Graphics::Wined3d,
            recipe: None,
            env: Default::default(),
            dll_overrides: Default::default(),
            options: Default::default(),
            programs: vec![],
            domain: None,
        }
        .save(&{
            std::fs::create_dir_all(&dir).unwrap();
            dir.clone()
        })
        .unwrap();
        let c = dir.join("prefix/drive_c");
        write(&c.join("Program Files/App/app.exe"), 1000);
        write(&c.join("windows/temp/a.tmp"), 100);
        write(&c.join("users/me/AppData/Local/Temp/sub/b.tmp"), 50);
        for i in 0..12 {
            write(
                &dir.join(format!("logs/2026-10-15T00-00-{i:02}-run.log")),
                10,
            );
        }
        // Outside data that a symlink in the prefix points to must survive.
        let outside = tmp.path().join("outside");
        write(&outside.join("keep.txt"), 7);
        std::fs::create_dir_all(dir.join("prefix/dosdevices")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("prefix/dosdevices/z:")).unwrap();
        std::os::unix::fs::symlink(&outside, c.join("windows/temp/link")).unwrap();
        write(&paths.downloads_dir().join("old.exe.part"), 300);

        let u = usage(&paths).unwrap();
        assert_eq!(
            u.bottles[0].bytes,
            std::fs::metadata(dir.join("bottle.toml")).unwrap().len() + 1000 + 150 + 120
        );
        assert_eq!(u.downloads, 300);

        assert_eq!(clean_bottle(&paths, "b").unwrap(), 150 + 20);
        assert!(c.join("Program Files/App/app.exe").exists());
        assert!(c.join("windows/temp").is_dir());
        assert_eq!(std::fs::read_dir(dir.join("logs")).unwrap().count(), 10);
        assert!(dir.join("logs/2026-10-15T00-00-11-run.log").exists());
        assert!(outside.join("keep.txt").exists());

        assert_eq!(clear_downloads(&paths).unwrap(), 300);
        assert_eq!(clear_downloads(&paths).unwrap(), 0);
    }
}
