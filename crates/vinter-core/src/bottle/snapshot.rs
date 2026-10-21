//! Snapshots: copies of a bottle's prefix, icons and settings under
//! `<bottle>/snapshots/<id>/`, to roll back to. They are copy-on-write clones
//! where the file system has them (APFS, btrfs, XFS), so making one is
//! instant and it takes no space until the bottle's files change.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Bottle, CONFIG_FILE, ICONS_DIR, PREFIX_DIR, SETUP_MARKER, copy_tree, dir, get};
use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Error, Result};

pub const SNAPSHOTS_DIR: &str = "snapshots";
const META_FILE: &str = "snapshot.json";
/// Automatic snapshots kept per bottle. Older ones are removed; ones made by
/// hand are kept until deleted.
pub const KEEP_AUTOMATIC: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Snapshot {
    pub id: String,
    pub label: String,
    /// Made by Vinter before a change, rather than by hand.
    pub automatic: bool,
    /// RFC 3339 with microseconds, so snapshots sort in the order they were made.
    pub created_at: String,
}

/// Snapshots a bottle. Close its programs first, or files they are writing
/// may be caught half-written. Automatic snapshots beyond
/// [`KEEP_AUTOMATIC`] are removed, oldest first.
pub fn create(paths: &AppPaths, bottle_id: &str, label: &str, automatic: bool) -> Result<Snapshot> {
    let snap = make(paths, bottle_id, label, automatic)?;
    if automatic {
        prune(paths, bottle_id)?;
    }
    Ok(snap)
}

/// Snapshots a bottle before Vinter changes it, if the file system can
/// clone: there it is free, elsewhere it would copy the whole prefix.
pub fn auto(paths: &AppPaths, bottle_id: &str, label: &str) -> Result<Option<Snapshot>> {
    let bottle_dir = dir(paths, bottle_id)?;
    if bottle_dir.join(SETUP_MARKER).exists() || !super::can_clone(&bottle_dir) {
        return Ok(None);
    }
    create(paths, bottle_id, label, true).map(Some)
}

fn make(paths: &AppPaths, bottle_id: &str, label: &str, automatic: bool) -> Result<Snapshot> {
    let bottle = get(paths, bottle_id)?;
    let bottle_dir = dir(paths, bottle_id)?;
    if bottle_dir.join(SETUP_MARKER).exists() {
        return Err(Error::BottleNotReady(bottle.name));
    }
    let label = label.trim();
    let snap = Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        label: if label.is_empty() { "Snapshot" } else { label }.to_string(),
        automatic,
        created_at: humantime::format_rfc3339_micros(std::time::SystemTime::now()).to_string(),
    };
    let root = bottle_dir.join(SNAPSHOTS_DIR);
    // Built under a hidden name, so `list` never sees a half-made one.
    let part = root.join(format!(".{}", snap.id));
    let result = (|| {
        std::fs::create_dir_all(&part).at(&part)?;
        copy_layout(&bottle_dir, &part)?;
        let meta = serde_json::to_vec_pretty(&snap).expect("a snapshot serializes");
        std::fs::write(part.join(META_FILE), meta).at(&part)?;
        let done = root.join(&snap.id);
        std::fs::rename(&part, &done).at(&done)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&part);
    }
    result.map(|()| snap)
}

/// A bottle's snapshots, newest first.
pub fn list(paths: &AppPaths, bottle_id: &str) -> Result<Vec<Snapshot>> {
    get(paths, bottle_id)?;
    let root = dir(paths, bottle_id)?.join(SNAPSHOTS_DIR);
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&root),
    };
    let mut snaps: Vec<Snapshot> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| std::fs::read(e.path().join(META_FILE)).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    snaps.sort_by(|a, b| (&b.created_at, &b.id).cmp(&(&a.created_at, &a.id)));
    Ok(snaps)
}

/// Puts the bottle back as it was in the snapshot: its prefix, icons and
/// settings (the bottle keeps its current name). Stops the bottle's Wine
/// processes first and snapshots the current state, so this can be undone.
/// Callers should sync launchers afterwards: the program list may change.
pub fn restore(paths: &AppPaths, bottle_id: &str, snapshot_id: &str) -> Result<Bottle> {
    let current = get(paths, bottle_id)?;
    let snap_dir = snapshot_dir(paths, bottle_id, snapshot_id)?;
    let bottle_dir = dir(paths, bottle_id)?;
    let _ = super::kill_all(paths, bottle_id);
    make(paths, bottle_id, "Before restore", true)?;

    // Move the current files aside, so a failed restore can put them back.
    let aside = bottle_dir.join(".restore-old");
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::create_dir_all(&aside).at(&aside)?;
    for sub in [PREFIX_DIR, ICONS_DIR] {
        let from = bottle_dir.join(sub);
        if from.exists() {
            std::fs::rename(&from, aside.join(sub)).at(&from)?;
        }
    }
    let result = (|| {
        for sub in [PREFIX_DIR, ICONS_DIR] {
            let from = snap_dir.join(sub);
            if from.exists() {
                copy_tree(&from, &bottle_dir.join(sub))?;
            }
        }
        let mut restored = Bottle::load(&snap_dir)?;
        restored.id = current.id.clone();
        restored.name = current.name.clone();
        restored.save(&bottle_dir)?;
        Ok(restored)
    })();
    match &result {
        Ok(_) => {
            let _ = std::fs::remove_dir_all(&aside);
        }
        Err(_) => {
            for sub in [PREFIX_DIR, ICONS_DIR] {
                let _ = std::fs::remove_dir_all(bottle_dir.join(sub));
                let _ = std::fs::rename(aside.join(sub), bottle_dir.join(sub));
            }
            let _ = std::fs::remove_dir(&aside);
        }
    }
    result
}

pub fn delete(paths: &AppPaths, bottle_id: &str, snapshot_id: &str) -> Result<()> {
    let snap_dir = snapshot_dir(paths, bottle_id, snapshot_id)?;
    std::fs::remove_dir_all(&snap_dir).at(&snap_dir)
}

/// Removes automatic snapshots beyond [`KEEP_AUTOMATIC`], oldest first.
fn prune(paths: &AppPaths, bottle_id: &str) -> Result<()> {
    let old = list(paths, bottle_id)?
        .into_iter()
        .filter(|s| s.automatic)
        .skip(KEEP_AUTOMATIC);
    for snap in old {
        delete(paths, bottle_id, &snap.id)?;
    }
    Ok(())
}

fn snapshot_dir(
    paths: &AppPaths,
    bottle_id: &str,
    snapshot_id: &str,
) -> Result<std::path::PathBuf> {
    validate_id(snapshot_id)?;
    get(paths, bottle_id)?;
    let path = dir(paths, bottle_id)?.join(SNAPSHOTS_DIR).join(snapshot_id);
    if !path.join(META_FILE).is_file() {
        return Err(Error::SnapshotNotFound(snapshot_id.to_string()));
    }
    Ok(path)
}

/// Copies what a snapshot holds from one bottle-shaped folder to another.
fn copy_layout(from: &Path, to: &Path) -> Result<()> {
    for sub in [PREFIX_DIR, ICONS_DIR] {
        let src = from.join(sub);
        if src.exists() {
            copy_tree(&src, &to.join(sub))?;
        }
    }
    let config = from.join(CONFIG_FILE);
    std::fs::copy(&config, to.join(CONFIG_FILE)).at(&config)?;
    Ok(())
}
