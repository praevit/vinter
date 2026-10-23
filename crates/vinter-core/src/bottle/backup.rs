//! Backing up a bottle to one `.vinterbottle` file, and restoring it.
//!
//! The file is a gzip-compressed tar of the bottle folder (without logs)
//! under `bottle/`. Restoring treats it as untrusted: only plain relative
//! paths, no writing through links, and only Wine's own drive links
//! (`dosdevices/c:` and `z:`) are recreated.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use super::{Bottle, CONFIG_FILE, SETUP_MARKER, dir, get, list, validate_name};
use crate::error::IoResultExt;
use crate::process::logs::LOGS_DIR;
use crate::{AppPaths, Error, Result};

/// File extension of a backup.
pub const EXTENSION: &str = "vinterbottle";
const ROOT: &str = "bottle";

/// Writes a backup of a bottle to `dest`. `progress(done, total)` is in bytes
/// of bottle files. Close the bottle's programs first.
pub fn export(
    paths: &AppPaths,
    id: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    get(paths, id)?;
    let src = dir(paths, id)?;
    let total = crate::storage::dir_size(&src);
    let part = dest.with_extension(format!("{EXTENSION}.part"));
    let result = (|| {
        let file = File::create(&part).at(&part)?;
        let gz = flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        tar.follow_symlinks(false);
        let mut done = 0u64;
        for entry in walkdir::WalkDir::new(&src).min_depth(1).sort_by_file_name() {
            let entry = entry.map_err(|e| Error::BadArchive(e.to_string()))?;
            let rel = entry.path().strip_prefix(&src).expect("inside the bottle");
            let first = rel.components().next().map(|c| c.as_os_str().to_owned());
            let skipped = [LOGS_DIR, super::snapshot::SNAPSHOTS_DIR, ".restore-old"];
            if first.is_some_and(|f| skipped.iter().any(|s| f == *s))
                || rel == Path::new(SETUP_MARKER)
            {
                continue;
            }
            let name = Path::new(ROOT).join(rel);
            tar.append_path_with_name(entry.path(), &name)
                .at(entry.path())?;
            if entry.file_type().is_file() {
                done += entry.metadata().map(|m| m.len()).unwrap_or(0);
                progress(done, total);
            }
        }
        let gz = tar.into_inner().at(&part)?;
        gz.finish().at(&part)?.flush().at(&part)
    })();
    match result {
        Ok(()) => std::fs::rename(&part, dest).at(dest),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Restores a backup as a new bottle (new id; the name gets "(restored)"
/// if taken). `progress(done, total)` is in bytes of the backup file.
pub fn import(
    paths: &AppPaths,
    archive: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<Bottle> {
    let total = std::fs::metadata(archive).at(archive)?.len();
    let bottles = paths.bottles_dir();
    std::fs::create_dir_all(&bottles).at(&bottles)?;
    let id = uuid::Uuid::new_v4().to_string();
    let staging = bottles.join(format!(".restore-{id}"));
    std::fs::create_dir(&staging).at(&staging)?;

    let result = (|| {
        let file = File::open(archive).at(archive)?;
        let counted = Counted {
            inner: BufReader::new(file),
            read: 0,
            total,
            progress: &mut progress,
        };
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(counted));
        let bad =
            |why: &str| Error::BadArchive(format!("this is not a Vinter bottle backup ({why})"));
        for entry in tar.entries().map_err(|_| bad("unreadable"))? {
            let mut entry = entry.map_err(|_| bad("unreadable"))?;
            let path = entry.path().map_err(|_| bad("bad path"))?.into_owned();
            let Some(rel) = safe_relative(&path) else {
                return Err(bad("unsafe path"));
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let target = staging.join(&rel);
            if passes_through_link(&staging, &rel) {
                return Err(bad("path through a link"));
            }
            let kind = entry.header().entry_type();
            if kind.is_dir() {
                std::fs::create_dir_all(&target).at(&target)?;
            } else if kind.is_file() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).at(parent)?;
                }
                entry.unpack(&target).at(&target)?;
            } else if kind.is_symlink() {
                let link = entry.link_name().ok().flatten().map(|l| l.into_owned());
                if let Some(link) = link.filter(|l| is_wine_drive_link(&rel, l)) {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).at(parent)?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&link, &target).at(&target)?;
                }
                // Other links (e.g. to the old Mac's Documents) are left out.
            }
            // Hard links, devices and the like are skipped.
        }

        let mut bottle = Bottle::load(&staging).map_err(|_| bad("no bottle.toml"))?;
        let taken: Vec<String> = list(paths)?.into_iter().map(|b| b.name).collect();
        let mut name = bottle.name.clone();
        if taken.contains(&name) {
            name = format!("{name} (restored)");
        }
        bottle.id = id.clone();
        bottle.name = validate_name(&name)?;
        // Launchers belong to this Mac; the app recreates them for pinned programs.
        bottle.save(&staging)?;
        let _ = std::fs::remove_file(staging.join(SETUP_MARKER));
        let final_dir = dir(paths, &id)?;
        std::fs::rename(&staging, &final_dir).at(&final_dir)?;
        Ok(bottle)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// `bottle/<rest>` → `<rest>`, if every part is a plain name.
fn safe_relative(path: &Path) -> Option<PathBuf> {
    let mut parts = path.components();
    match parts.next()? {
        Component::Normal(first) if first == ROOT => {}
        _ => return None,
    }
    let mut rel = PathBuf::new();
    for part in parts {
        match part {
            Component::Normal(p) => rel.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    // The config file must be at the top, as written by `export`.
    if rel.file_name().is_some_and(|f| f == CONFIG_FILE) && rel != Path::new(CONFIG_FILE) {
        return None;
    }
    Some(rel)
}

/// True if a parent of `rel` inside `root` is a symlink.
fn passes_through_link(root: &Path, rel: &Path) -> bool {
    let mut at = root.to_path_buf();
    let parents: Vec<_> = rel
        .parent()
        .map(|p| p.components().collect())
        .unwrap_or_default();
    for part in parents {
        at.push(part);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

/// Wine's drive links: `prefix/dosdevices/c:` → `../drive_c`, `z:` → `/`.
fn is_wine_drive_link(rel: &Path, link: &Path) -> bool {
    let dosdevices = Path::new(super::PREFIX_DIR).join("dosdevices");
    (rel == dosdevices.join("c:") && link == Path::new("../drive_c"))
        || (rel == dosdevices.join("z:") && link == Path::new("/"))
}

/// Counts bytes read, for progress.
struct Counted<'a, R, P: FnMut(u64, u64)> {
    inner: R,
    read: u64,
    total: u64,
    progress: &'a mut P,
}

impl<R: Read, P: FnMut(u64, u64)> Read for Counted<'_, R, P> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.progress)(self.read, self.total);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Graphics, PREFIX_DIR, WindowsVersion};

    fn bottle(paths: &AppPaths, id: &str, name: &str) -> PathBuf {
        let d = dir(paths, id).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        Bottle {
            id: id.into(),
            name: name.into(),
            runtime: "rt".into(),
            engine: crate::engine::EngineKind::legacy(),
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
        .save(&d)
        .unwrap();
        d
    }

    #[test]
    fn round_trips_without_logs_and_with_drive_links() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let d = bottle(&paths, "src", "Office");
        let prefix = d.join(PREFIX_DIR);
        std::fs::create_dir_all(prefix.join("drive_c/Program Files/App")).unwrap();
        std::fs::write(prefix.join("drive_c/Program Files/App/app.exe"), b"MZ").unwrap();
        std::fs::write(prefix.join("user.reg"), b"registry").unwrap();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        std::os::unix::fs::symlink("/", prefix.join("dosdevices/z:")).unwrap();
        std::os::unix::fs::symlink("/Users/someone/Documents", prefix.join("drive_c/Docs"))
            .unwrap();
        std::fs::create_dir_all(d.join("logs")).unwrap();
        std::fs::write(d.join("logs/old.log"), b"log").unwrap();

        let backup = tmp.path().join("office.vinterbottle");
        let mut calls = 0;
        export(&paths, "src", &backup, |_, _| calls += 1).unwrap();
        assert!(calls > 0);
        assert!(!tmp.path().join("office.vinterbottle.part").exists());

        let restored = import(&paths, &backup, |_, _| {}).unwrap();
        assert_ne!(restored.id, "src");
        assert_eq!(restored.name, "Office (restored)");
        let r = dir(&paths, &restored.id).unwrap();
        assert_eq!(
            std::fs::read(r.join("prefix/drive_c/Program Files/App/app.exe")).unwrap(),
            b"MZ"
        );
        assert_eq!(
            std::fs::read_link(r.join("prefix/dosdevices/z:")).unwrap(),
            Path::new("/")
        );
        assert_eq!(
            std::fs::read_link(r.join("prefix/dosdevices/c:")).unwrap(),
            Path::new("../drive_c")
        );
        assert!(std::fs::symlink_metadata(r.join("prefix/drive_c/Docs")).is_err());
        assert!(!r.join("logs").exists());
        assert_eq!(get(&paths, &restored.id).unwrap().name, "Office (restored)");
    }

    fn tar_gz(path: &Path, build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<File>>)) {
        let gz =
            flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        build(&mut b);
        b.into_inner().unwrap().finish().unwrap();
    }

    fn file_header(size: u64) -> tar::Header {
        let mut h = tar::Header::new_gnu();
        h.set_size(size);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Regular);
        h
    }

    #[test]
    fn rejects_unsafe_backups_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let outside = tmp.path().join("escaped");

        // A link to outside, then a file written through it.
        let evil = tmp.path().join("evil.vinterbottle");
        tar_gz(&evil, |b| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            b.append_link(&mut h, "bottle/prefix/dosdevices/z:", "/")
                .unwrap();
            let mut f = file_header(1);
            b.append_data(
                &mut f,
                format!("bottle/prefix/dosdevices/z:{}/x", outside.display()),
                &b"x"[..],
            )
            .unwrap();
        });
        assert!(import(&paths, &evil, |_, _| {}).is_err());
        assert!(!outside.exists());

        // Not ours at all.
        let other = tmp.path().join("other.vinterbottle");
        tar_gz(&other, |b| {
            let mut f = file_header(1);
            b.append_data(&mut f, "something/else.txt", &b"x"[..])
                .unwrap();
        });
        assert!(import(&paths, &other, |_, _| {}).is_err());

        // No bottle.toml.
        let empty = tmp.path().join("empty.vinterbottle");
        tar_gz(&empty, |b| {
            let mut f = file_header(1);
            b.append_data(&mut f, "bottle/prefix/user.reg", &b"x"[..])
                .unwrap();
        });
        assert!(import(&paths, &empty, |_, _| {}).is_err());

        let leftovers: Vec<_> = std::fs::read_dir(paths.bottles_dir()).unwrap().collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}
