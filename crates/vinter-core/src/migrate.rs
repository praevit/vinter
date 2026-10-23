//! "Recreate as Apple Silicon" (the `bottle-migrate-arm64` experimental
//! flag): makes a new bottle on the Apple Silicon engine, re-runs the old
//! bottle's recipe there if it has one, and copies the Windows user's own
//! folders (documents, app data, saved games) across. The old bottle is only
//! ever read.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bottle::{self, Bottle, NewBottle, PREFIX_DIR};
use crate::engine::{self, EngineKind};
use crate::error::IoResultExt;
use crate::experimental::{ExperimentalFlag, Flags};
use crate::recipe::{self, ApplyOptions, RecipeProgress};
use crate::{AppPaths, Error, Result, runtime};

/// Folders of a Windows user that hold the user's own data. Only real
/// folders are copied: Wine links some of them (Documents, Downloads...) to
/// the Mac's own folders, and the new bottle gets the same links.
const USER_FOLDERS: &[&str] = &[
    "AppData",
    "Desktop",
    "Documents",
    "Saved Games",
    "Favorites",
    "Contacts",
    "Links",
    "Music",
    "Pictures",
    "Videos",
];
/// Inside the folders above: caches nobody needs to keep.
const SKIP: &[&str] = &[
    "AppData/Local/Temp",
    "AppData/Local/Microsoft/Windows/INetCache",
];

/// What a migration would do, for the wizard to show first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MigrationPlan {
    pub source: String,
    /// The Apple Silicon runtime the new bottle uses.
    pub runtime_id: String,
    pub runtime_name: String,
    /// Recipe that will reinstall the apps, if the bottle was made from one.
    pub recipe: Option<String>,
    /// User folders that will be copied, as `user/folder`.
    pub folders: Vec<String>,
    /// Bytes to copy.
    pub bytes: u64,
    /// Programs that will not be there until reinstalled (no recipe).
    pub reinstall: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MigrateOptions {
    /// Id for the new bottle (so a UI can follow it); generated if `None`.
    pub bottle_id: Option<String>,
    pub name: String,
    /// Installer for a recipe that needs the user's own.
    pub installer: Option<PathBuf>,
    /// The user agreed to the recipe's downloads.
    pub accept_downloads: bool,
}

fn ensure_enabled(paths: &AppPaths) -> Result<()> {
    if Flags::load(paths).is_enabled(ExperimentalFlag::BottleMigrateArm64) {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "recreating bottles for Apple Silicon is experimental; turn on {} first",
            ExperimentalFlag::BottleMigrateArm64
        )))
    }
}

/// The newest installed Apple Silicon runtime this Mac can run.
fn arm64_runtime(paths: &AppPaths) -> Result<runtime::Runtime> {
    let host = engine::Host::for_paths(paths);
    runtime::installed(paths)?
        .into_iter()
        .filter(|r| r.engine == EngineKind::Arm64Fex)
        .filter(|r| engine::unmet_requirement(r.engine, &host).is_none())
        .max_by(|a, b| runtime::updates::compare_versions(&a.version, &b.version))
        .ok_or_else(|| {
            Error::Unsupported(
                "install an Apple Silicon runtime first (Runtimes page), on macOS 26.5 or newer"
                    .into(),
            )
        })
}

/// What [`migrate`] would do for bottle `id`.
pub fn plan(paths: &AppPaths, id: &str) -> Result<MigrationPlan> {
    ensure_enabled(paths)?;
    let source = bottle::get(paths, id)?;
    if engine::is_fex(source.engine) {
        return Err(Error::Unsupported(format!(
            "{} already uses the Apple Silicon engine",
            source.name
        )));
    }
    // Legacy 32-bit bottles are allowed: their user folders still copy over.
    let rt = arm64_runtime(paths)?;
    let folders = user_folders(&bottle::dir(paths, id)?.join(PREFIX_DIR));
    let bytes = folders.iter().map(|(_, dir)| copy_size(dir)).sum();
    Ok(MigrationPlan {
        source: source.name.clone(),
        runtime_id: rt.id,
        runtime_name: rt.name,
        recipe: source.recipe.clone(),
        folders: folders.into_iter().map(|(label, _)| label).collect(),
        bytes,
        reinstall: if source.recipe.is_some() {
            Vec::new()
        } else {
            source.programs.iter().map(|p| p.name.clone()).collect()
        },
    })
}

/// Makes the new bottle and copies the user folders. On failure the new
/// bottle is moved to the Trash; the old one is never changed.
pub async fn migrate<P>(
    paths: &AppPaths,
    id: &str,
    opts: MigrateOptions,
    mut on_progress: P,
) -> Result<Bottle>
where
    P: FnMut(RecipeProgress) + Send,
{
    let plan = plan(paths, id)?;
    let source = bottle::get(paths, id)?;
    let new_id = opts
        .bottle_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let created = match &source.recipe {
        Some(recipe_id) => {
            let loaded = recipe::find(paths, recipe_id)?;
            let apply = ApplyOptions {
                bottle_id: Some(new_id.clone()),
                bottle_name: opts.name.clone(),
                installer: opts.installer.clone(),
                accept_downloads: opts.accept_downloads,
                runtime: Some(plan.runtime_id.clone()),
            };
            recipe::apply(paths, &loaded, apply, &mut on_progress).await
        }
        None => {
            on_progress(RecipeProgress {
                task: 1,
                tasks: 2,
                label: "Creating the bottle".into(),
                detail: String::new(),
                percent: None,
            });
            let new = NewBottle {
                id: Some(new_id.clone()),
                name: opts.name.clone(),
                runtime: plan.runtime_id.clone(),
                windows_version: source.windows_version,
                graphics: None,
            };
            let paths = paths.clone();
            tokio::task::spawn_blocking(move || bottle::create(&paths, new, |_| {}))
                .await
                .map_err(|e| Error::Unsupported(e.to_string()))?
        }
    };
    let created = match created {
        Ok(b) => b,
        Err(e) => {
            cleanup(paths, &new_id);
            return Err(e);
        }
    };

    on_progress(RecipeProgress {
        task: 2,
        tasks: 2,
        label: "Copying your files".into(),
        detail: String::new(),
        percent: None,
    });
    let (from, to) = (
        bottle::dir(paths, id)?.join(PREFIX_DIR),
        bottle::dir(paths, &created.id)?.join(PREFIX_DIR),
    );
    let copied = tokio::task::spawn_blocking(move || copy_user_folders(&from, &to))
        .await
        .map_err(|e| Error::Unsupported(e.to_string()))
        .and_then(|r| r);
    if let Err(e) = copied {
        cleanup(paths, &created.id);
        return Err(e);
    }
    // The bottle's own display and keyboard options carry over too.
    bottle::set_options(paths, &created.id, source.options)
        .or_else(|_| bottle::get(paths, &created.id))
}

fn cleanup(paths: &AppPaths, id: &str) {
    if bottle::get(paths, id).is_ok() {
        let _ = bottle::delete(paths, id);
    } else if let Ok(dir) = bottle::dir(paths, id) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// `(user/folder, path)` of every real (not linked) user folder to copy.
fn user_folders(prefix: &Path) -> Vec<(String, PathBuf)> {
    let users = prefix.join("drive_c/users");
    let Ok(entries) = std::fs::read_dir(&users) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for user in entries.filter_map(|e| e.ok()) {
        let name = user.file_name().to_string_lossy().into_owned();
        if !user.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for folder in USER_FOLDERS {
            let dir = user.path().join(folder);
            let real_dir = std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
            if real_dir && copy_size(&dir) > 0 {
                out.push((format!("{name}/{folder}"), dir));
            }
        }
    }
    out.sort();
    out
}

fn skipped(rel: &Path) -> bool {
    let rel = rel.to_string_lossy().replace('\\', "/");
    SKIP.iter()
        .any(|s| rel == *s || rel.starts_with(&format!("{s}/")))
}

/// Bytes of regular files under `dir`, not following links and skipping caches.
fn copy_size(dir: &Path) -> u64 {
    let user_dir = dir.parent().unwrap_or(dir);
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !skipped(e.path().strip_prefix(user_dir).unwrap_or(e.path())))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Copies every real user folder of `from` into the same place in `to`.
/// Links are never followed or copied; files already in `to` are replaced.
fn copy_user_folders(from: &Path, to: &Path) -> Result<()> {
    for (label, dir) in user_folders(from) {
        let target_root = to.join("drive_c/users").join(&label);
        // The new bottle may have this folder as a link to the Mac's own
        // folder (e.g. Documents): leave that link alone.
        if std::fs::symlink_metadata(&target_root).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let user_dir = dir.parent().unwrap_or(&dir).to_path_buf();
        for entry in walkdir::WalkDir::new(&dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !skipped(e.path().strip_prefix(&user_dir).unwrap_or(e.path())))
        {
            let entry = entry.map_err(|e| Error::Unsupported(e.to_string()))?;
            let rel = entry.path().strip_prefix(&dir).unwrap_or(entry.path());
            let target = target_root.join(rel);
            let kind = entry.file_type();
            if kind.is_dir() {
                std::fs::create_dir_all(&target).at(&target)?;
            } else if kind.is_file() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).at(parent)?;
                }
                std::fs::copy(entry.path(), &target).at(&target)?;
            }
            // Links inside are skipped: they point into the old bottle or the Mac.
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copies_real_user_folders_and_never_follows_links() {
        let tmp = tempfile::tempdir().unwrap();
        let mac_documents = tmp.path().join("mac-documents");
        write(&mac_documents.join("private.txt"), "the Mac's own file");

        let old = tmp.path().join("old/prefix");
        let user = old.join("drive_c/users/me");
        write(&user.join("AppData/Roaming/Game/save1.dat"), "save");
        write(&user.join("AppData/Local/Temp/huge.tmp"), "temp");
        write(&user.join("Saved Games/Game/slot.sav"), "slot");
        write(&user.join("Desktop/notes.txt"), "notes");
        std::os::unix::fs::symlink(&mac_documents, user.join("Documents")).unwrap();
        std::os::unix::fs::symlink(
            mac_documents.join("private.txt"),
            user.join("AppData/Roaming/link.txt"),
        )
        .unwrap();

        let labels: Vec<_> = user_folders(&old).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["me/AppData", "me/Desktop", "me/Saved Games"]);
        assert_eq!(
            copy_size(&user.join("AppData")),
            4,
            "temp files are not counted"
        );

        let new = tmp.path().join("new/prefix");
        let new_user = new.join("drive_c/users/me");
        std::fs::create_dir_all(&new_user).unwrap();
        std::os::unix::fs::symlink(&mac_documents, new_user.join("Documents")).unwrap();
        copy_user_folders(&old, &new).unwrap();

        let read = |p: &str| std::fs::read_to_string(new_user.join(p)).ok();
        assert_eq!(
            read("AppData/Roaming/Game/save1.dat").as_deref(),
            Some("save")
        );
        assert_eq!(read("Saved Games/Game/slot.sav").as_deref(), Some("slot"));
        assert_eq!(read("Desktop/notes.txt").as_deref(), Some("notes"));
        assert!(!new_user.join("AppData/Local/Temp/huge.tmp").exists());
        assert!(!new_user.join("AppData/Roaming/link.txt").exists());
        // The Mac's folder was neither copied nor touched.
        assert_eq!(std::fs::read_dir(&mac_documents).unwrap().count(), 1);
        // The old bottle is unchanged.
        assert!(user.join("AppData/Local/Temp/huge.tmp").exists());
    }

    #[test]
    fn needs_the_flag_and_an_apple_silicon_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let err = plan(&paths, "nope").unwrap_err();
        assert!(err.to_string().contains("bottle-migrate-arm64"), "{err}");

        crate::experimental::set(&paths, ExperimentalFlag::BottleMigrateArm64, true).unwrap();
        let dir = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"Old\"\nruntime = \"rt\"\nengine = \"x86_64-rosetta\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.save(&dir).unwrap();
        let err = plan(&paths, "b").unwrap_err();
        assert!(err.to_string().contains("Apple Silicon runtime"), "{err}");
    }
}
