//! Wine runtime management: what is available, what is installed.

pub mod install;
pub mod manifest;
pub mod updates;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bottle::Graphics;
pub use crate::download::{InstallProgress, InstallStage};
use crate::engine::{Engine, EngineKind};
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
    /// Engine that runs it. Records written before M8 lack it; their
    /// engine follows from `arch`.
    #[serde(default)]
    pub engine: Option<EngineKind>,
    /// FEX release it ships, if any.
    #[serde(default)]
    pub fex_version: Option<String>,
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
    pub engine: EngineKind,
    /// FEX release it ships, if any.
    pub fex_version: Option<String>,
    /// Why its engine cannot run on this machine, if it cannot (e.g. too old
    /// a macOS, or Rosetta hidden by `simulate-macos28`).
    pub unavailable: Option<String>,
    /// x86_64 build on Apple Silicon: runs only with Rosetta 2.
    pub needs_rosetta: bool,
    /// Graphics backends it supports.
    pub graphics: Vec<Graphics>,
    /// Built with Kerberos, so programs can sign in with a domain account.
    pub kerberos: bool,
    /// Reports a bottle's domain to programs (Vinter Wine's domain patch).
    /// Other runtimes only take over the user name.
    pub domain_identity: bool,
}

impl Runtime {
    fn from_record(path: PathBuf, record: RuntimeRecord) -> Self {
        let engine = record.engine.unwrap_or_else(|| {
            EngineKind::for_build(std::env::consts::OS, &record.arch)
                .unwrap_or_else(EngineKind::legacy)
        });
        let host = match path.parent().and_then(Path::parent) {
            Some(data) => crate::engine::Host::for_paths(&AppPaths::with_data_dir(data)),
            None => crate::engine::Host::current(),
        };
        let unavailable = crate::engine::unmet_requirement(engine, &host);
        let wine_bin = path.join(&record.wine_bin);
        // The Wine installation: the folder holding bin/, lib/ and share/.
        let root = wine_bin
            .parent()
            .and_then(Path::parent)
            .unwrap_or(&path)
            .to_path_buf();
        Self {
            kerberos: has_kerberos(&root),
            domain_identity: has_domain_patch(&root),
            wine_bin,
            size: crate::storage::dir_size(&path),
            id: record.id,
            name: record.name,
            version: record.version,
            flavor: record.flavor,
            installed_at: record.installed_at,
            source_url: record.source_url,
            needs_rosetta: arch_needs_rosetta(&record.arch),
            engine,
            unavailable,
            arch: record.arch,
            fex_version: record.fex_version,
            graphics: record.graphics,
            path,
        }
    }

    /// What runs this runtime's Wine.
    pub fn engine(&self) -> &'static dyn Engine {
        self.engine.engine()
    }

    /// Fails if this runtime cannot run on this machine (its engine's
    /// requirements, e.g. `RosettaRequired`).
    pub fn check_can_run(&self) -> Result<()> {
        self.engine().check(self)
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

/// Wine builds its Kerberos module only when it finds libkrb5.
fn has_kerberos(root: &Path) -> bool {
    ["lib", "lib64"].iter().any(|lib| {
        ["x86_64-unix", "aarch64-unix"].iter().any(|unix| {
            root.join(lib)
                .join("wine")
                .join(unix)
                .join("kerberos.so")
                .is_file()
        })
    })
}

/// Vinter Wine ships its patches in share/vinter/patches.
fn has_domain_patch(root: &Path) -> bool {
    std::fs::read_dir(root.join("share/vinter/patches")).is_ok_and(|entries| {
        entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("Active-Directory-domain")
        })
    })
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

/// The runtime new bottles use: the one chosen in Settings if it is still
/// installed, else the newest runtime on the Apple Silicon engine when this
/// Mac can run it, else the newest one that can run here at all.
pub fn default_for_new_bottles(paths: &AppPaths) -> Result<Option<Runtime>> {
    let installed = installed(paths)?;
    if let Some(chosen) = crate::settings::load(paths).default_runtime
        && let Some(rt) = installed.iter().find(|r| r.id == chosen)
    {
        return Ok(Some(rt.clone()));
    }
    Ok(pick_default(
        installed,
        &crate::engine::Host::for_paths(paths),
    ))
}

fn pick_default(installed: Vec<Runtime>, host: &crate::engine::Host) -> Option<Runtime> {
    let runs = |r: &Runtime| crate::engine::unmet_requirement(r.engine, host).is_none();
    let newest = |rs: Vec<Runtime>| {
        rs.into_iter()
            .max_by(|a, b| updates::compare_versions(&a.version, &b.version).then(b.id.cmp(&a.id)))
    };
    let (arm64, rest): (Vec<_>, Vec<_>) = installed
        .into_iter()
        .filter(runs)
        .partition(|r| r.engine == EngineKind::Arm64Fex);
    newest(arm64).or_else(|| newest(rest))
}

#[cfg(test)]
mod default_tests {
    use super::*;
    use crate::engine::Host;

    fn rt(id: &str, version: &str, engine: EngineKind) -> Runtime {
        Runtime {
            id: id.into(),
            name: id.into(),
            version: version.into(),
            flavor: Flavor::Stable,
            path: PathBuf::new(),
            wine_bin: PathBuf::new(),
            installed_at: String::new(),
            size: 0,
            source_url: String::new(),
            arch: String::new(),
            engine,
            fex_version: None,
            unavailable: None,
            needs_rosetta: false,
            graphics: vec![],
            kerberos: false,
            domain_identity: false,
        }
    }

    #[test]
    fn prefers_the_apple_silicon_engine_where_it_runs() {
        let new_mac = Host {
            os: "macos".into(),
            arch: "aarch64".into(),
            macos_version: Some("27.0".into()),
            rosetta_removed: false,
        };
        let old_mac = Host {
            macos_version: Some("26.4".into()),
            ..new_mac.clone()
        };
        let all = || {
            vec![
                rt("a-rosetta-11.19", "11.19", EngineKind::X86_64Rosetta),
                rt("b-arm-11.10", "11.10", EngineKind::Arm64Fex),
                rt("c-rosetta-11.0", "11.0", EngineKind::X86_64Rosetta),
            ]
        };
        assert_eq!(pick_default(all(), &new_mac).unwrap().id, "b-arm-11.10");
        // Too old for ARM64 Wine: the newest Rosetta runtime.
        assert_eq!(pick_default(all(), &old_mac).unwrap().id, "a-rosetta-11.19");
        assert!(pick_default(vec![], &new_mac).is_none());
    }
}
