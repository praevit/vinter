//! Wine runtime management: what is available, what is installed.

pub mod install;
pub mod manifest;
pub mod updates;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bottle::Graphics;
pub use crate::download::{InstallProgress, InstallStage};
use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Error, Result};
pub use install::{install, install_from_archive};
pub use manifest::{ArchiveFormat, Flavor, RuntimeInfo};

/// File written into every installed runtime folder.
pub const RECORD_FILE: &str = "runtime.toml";
/// The Wine license, copied next to every installed runtime.
pub const LICENSE_FILE: &str = "LICENSE-wine.txt";
pub(crate) const WINE_LICENSE: &str = include_str!("../../licenses/wine-COPYING.LIB");

/// Contents of `runtime.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRecord {
    pub id: String,
    pub name: String,
    pub version: String,
    pub flavor: Flavor,
    /// Where the archive was downloaded from.
    pub url: String,
    pub sha256: String,
    /// RFC 3339 timestamp.
    pub installed_at: String,
    /// Path of `wine`, relative to the runtime folder.
    pub wine_bin: String,
    pub license: String,
    pub source_url: String,
    /// CPU architecture of the Wine binaries. Records written by M1 lack it;
    /// every build offered then was x86_64.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Graphics backends this runtime supports. Records written before M4
    /// lack it; every runtime offered then supported wined3d and DXVK.
    #[serde(default = "default_graphics")]
    pub graphics: Vec<Graphics>,
}

fn default_graphics() -> Vec<Graphics> {
    vec![Graphics::Wined3d, Graphics::Dxvk]
}

fn default_arch() -> String {
    "x86_64".into()
}

/// True when `arch` binaries need Rosetta 2 on this machine.
pub fn arch_needs_rosetta(arch: &str) -> bool {
    crate::system::rosetta::is_needed() && arch == "x86_64"
}

/// An installed runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Runtime {
    pub id: String,
    pub name: String,
    pub version: String,
    pub flavor: Flavor,
    /// Root of the extracted build.
    pub path: PathBuf,
    /// The `wine` binary inside it.
    pub wine_bin: PathBuf,
    pub installed_at: String,
    /// Disk usage in bytes.
    pub size: u64,
    pub source_url: String,
    pub arch: String,
    /// x86_64 build on Apple Silicon: runs only with Rosetta 2.
    pub needs_rosetta: bool,
    /// Graphics backends it supports.
    pub graphics: Vec<Graphics>,
}

impl Runtime {
    fn from_record(path: PathBuf, record: RuntimeRecord) -> Self {
        Self {
            wine_bin: path.join(&record.wine_bin),
            size: crate::storage::dir_size(&path),
            id: record.id,
            name: record.name,
            version: record.version,
            flavor: record.flavor,
            installed_at: record.installed_at,
            source_url: record.source_url,
            needs_rosetta: arch_needs_rosetta(&record.arch),
            arch: record.arch,
            graphics: record.graphics,
            path,
        }
    }

    /// Fails with `RosettaRequired` if this runtime cannot run on this machine.
    pub fn check_can_run(&self) -> Result<()> {
        if self.needs_rosetta && !crate::system::rosetta::is_installed() {
            return Err(Error::RosettaRequired(self.name.clone()));
        }
        Ok(())
    }

    /// Folder holding `wine`, `wineserver` and friends.
    pub fn bin_dir(&self) -> &Path {
        self.wine_bin.parent().unwrap_or(&self.path)
    }

    /// A command for `program` with this runtime's `bin` folder first on `PATH`.
    /// Bare names that exist in `bin` (e.g. `wine`) resolve to that file.
    pub fn command(&self, program: &str) -> std::process::Command {
        let in_bin = self.bin_dir().join(program);
        let resolved = if !program.contains('/') && in_bin.is_file() {
            in_bin
        } else {
            PathBuf::from(program)
        };
        let mut cmd = std::process::Command::new(resolved);
        let mut path = vec![self.bin_dir().to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            path.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(path) {
            cmd.env("PATH", joined);
        }
        cmd
    }
}

fn read_record(dir: &Path) -> Result<RuntimeRecord> {
    let file = dir.join(RECORD_FILE);
    let text = std::fs::read_to_string(&file).at(&file)?;
    toml::from_str(&text).map_err(|e| Error::Config {
        path: file,
        message: e.to_string(),
    })
}

/// All installed runtimes, sorted by id. Folders without a readable
/// `runtime.toml` (including half-finished installs) are skipped.
pub fn installed(paths: &AppPaths) -> Result<Vec<Runtime>> {
    let dir = paths.runtimes_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&dir),
    };
    let mut runtimes = Vec::new();
    for entry in entries {
        let path = entry.at(&dir)?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden || !path.is_dir() {
            continue;
        }
        if let Ok(record) = read_record(&path) {
            runtimes.push(Runtime::from_record(path, record));
        }
    }
    runtimes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(runtimes)
}

/// Looks up one installed runtime.
pub fn get(paths: &AppPaths, id: &str) -> Result<Runtime> {
    validate_id(id)?;
    let dir = paths.runtimes_dir().join(id);
    if !dir.is_dir() {
        return Err(Error::RuntimeNotInstalled(id.to_string()));
    }
    let record = read_record(&dir)?;
    Ok(Runtime::from_record(dir, record))
}

/// Deletes an installed runtime. Runtimes can be downloaded again, so this
/// is a real delete rather than a move to the trash.
pub fn remove(paths: &AppPaths, id: &str) -> Result<()> {
    validate_id(id)?;
    let dir = paths.runtimes_dir().join(id);
    if !dir.join(RECORD_FILE).is_file() {
        return Err(Error::RuntimeNotInstalled(id.to_string()));
    }
    let users = crate::bottle::using_runtime(paths, id)?;
    if !users.is_empty() {
        let name = read_record(&dir)
            .map(|r| r.name)
            .unwrap_or_else(|_| id.to_string());
        return Err(Error::RuntimeInUse {
            runtime: name,
            bottles: users,
        });
    }
    // Rename first so a half-deleted folder never looks installed.
    let trash = paths
        .runtimes_dir()
        .join(format!(".removing-{id}-{}", uuid::Uuid::new_v4()));
    std::fs::rename(&dir, &trash).at(&dir)?;
    std::fs::remove_dir_all(&trash).at(&trash)
}
