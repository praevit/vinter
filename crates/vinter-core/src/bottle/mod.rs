//! Bottles: a Wine prefix plus its settings, under `<data>/bottles/<id>/`.

pub mod config;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

pub use config::{Bottle, CONFIG_FILE, Graphics, ProgramDef, WindowsVersion};

use crate::error::{IoResultExt, validate_id};
use crate::process::{self, logs};
use crate::runtime::{self, Runtime};
use crate::{AppPaths, Error, Result};

/// Present while a bottle is being created or duplicated. A bottle that
/// still has it when no job is running is one whose setup was interrupted.
pub const SETUP_MARKER: &str = ".setup-incomplete";
pub const PREFIX_DIR: &str = "prefix";
pub const ICONS_DIR: &str = "icons";

/// `wineboot --init` can take a while on first run (and under Rosetta).
const WINEBOOT_TIMEOUT: Duration = Duration::from_secs(300);
const WINECFG_TIMEOUT: Duration = Duration::from_secs(120);
const WINESERVER_TIMEOUT: Duration = Duration::from_secs(60);

/// Skip the Wine Mono and Gecko download prompts during setup. Apps that need
/// .NET or the HTML engine get them later through recipes (M5).
const SETUP_OVERRIDES: &[(&str, &str)] = &[("mscoree", ""), ("mshtml", "")];

/// What the bottle list shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BottleSummary {
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub windows_version: WindowsVersion,
    /// False while setup runs, or if it was interrupted.
    pub setup_complete: bool,
    pub program_count: u32,
}

/// Built-in Wine programs that can be opened in a bottle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Tool {
    Winecfg,
    Regedit,
    Taskmgr,
    Control,
    Notepad,
    Explorer,
}

impl Tool {
    pub const ALL: [Self; 6] = [
        Self::Winecfg,
        Self::Regedit,
        Self::Taskmgr,
        Self::Control,
        Self::Notepad,
        Self::Explorer,
    ];

    pub fn program(self) -> &'static str {
        match self {
            Self::Winecfg => "winecfg",
            Self::Regedit => "regedit",
            Self::Taskmgr => "taskmgr",
            Self::Control => "control",
            Self::Notepad => "notepad",
            Self::Explorer => "explorer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.program() == s)
    }
}

/// Settings for a new bottle.
#[derive(Debug, Clone)]
pub struct NewBottle {
    pub name: String,
    pub runtime: String,
    pub windows_version: WindowsVersion,
    pub graphics: Graphics,
}

/// Folder of a bottle (validates the id; does not check it exists).
pub fn dir(paths: &AppPaths, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(paths.bottles_dir().join(id))
}

/// The `drive_c` folder of a bottle.
pub fn drive_c(paths: &AppPaths, id: &str) -> Result<PathBuf> {
    get(paths, id)?;
    Ok(dir(paths, id)?.join(PREFIX_DIR).join("drive_c"))
}

/// Trims a bottle name and checks it is usable.
pub fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(Error::InvalidName(
            "use 1 to 64 characters, without line breaks".into(),
        ));
    }
    Ok(name.to_string())
}

/// All bottles, sorted by name. Folders without a readable `bottle.toml` are skipped.
pub fn list(paths: &AppPaths) -> Result<Vec<BottleSummary>> {
    let root = paths.bottles_dir();
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&root),
    };
    let mut bottles = Vec::new();
    for entry in entries {
        let path = entry.at(&root)?.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden || !path.is_dir() {
            continue;
        }
        if let Ok(b) = Bottle::load(&path) {
            bottles.push(BottleSummary {
                setup_complete: !path.join(SETUP_MARKER).exists(),
                program_count: b.programs.len() as u32,
                id: b.id,
                name: b.name,
                runtime: b.runtime,
                windows_version: b.windows_version,
            });
        }
    }
    bottles.sort_by_key(|b| (b.name.to_lowercase(), b.id.clone()));
    Ok(bottles)
}

pub fn get(paths: &AppPaths, id: &str) -> Result<Bottle> {
    let dir = dir(paths, id)?;
    if !dir.join(CONFIG_FILE).is_file() {
        return Err(Error::BottleNotFound(id.to_string()));
    }
    Bottle::load(&dir)
}

/// Finds a bottle by id, unique id prefix, or exact name (for the CLI).
pub fn resolve(paths: &AppPaths, key: &str) -> Result<Bottle> {
    let all = list(paths)?;
    let matches: Vec<_> = all
        .iter()
        .filter(|b| b.id == key || b.name == key || (key.len() >= 4 && b.id.starts_with(key)))
        .collect();
    match matches.as_slice() {
        [one] => get(paths, &one.id),
        [] => Err(Error::BottleNotFound(key.to_string())),
        _ => Err(Error::AmbiguousBottle(key.to_string())),
    }
}

/// Names of bottles that use a runtime.
pub fn using_runtime(paths: &AppPaths, runtime_id: &str) -> Result<Vec<String>> {
    Ok(list(paths)?
        .into_iter()
        .filter(|b| b.runtime == runtime_id)
        .map(|b| b.name)
        .collect())
}

/// Creates a bottle: [`prepare_create`] then [`finish_create`].
/// Blocks for up to a few minutes; `on_step` gets a label per stage.
pub fn create(paths: &AppPaths, new: NewBottle, on_step: impl FnMut(&str)) -> Result<Bottle> {
    let bottle = prepare_create(paths, new)?;
    finish_create(paths, &bottle, on_step)?;
    Ok(bottle)
}

/// Fast part of creating: checks the settings and writes `bottle.toml` with
/// the setup marker, so the bottle shows up (as not ready) right away.
pub fn prepare_create(paths: &AppPaths, new: NewBottle) -> Result<Bottle> {
    let name = validate_name(&new.name)?;
    let rt = runtime::get(paths, &new.runtime)?;
    rt.check_can_run()?;

    let bottle = Bottle {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        runtime: rt.id.clone(),
        created_at: now(),
        windows_version: new.windows_version,
        graphics: new.graphics,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        programs: Vec::new(),
    };
    let dir = dir(paths, &bottle.id)?;
    std::fs::create_dir_all(dir.join(PREFIX_DIR)).at(&dir)?;
    std::fs::write(dir.join(SETUP_MARKER), "").at(&dir)?;
    bottle.save(&dir)?;
    Ok(bottle)
}

/// Slow part of creating: runs `wineboot --init` and sets the Windows
/// version. On failure the bottle folder is removed again.
pub fn finish_create(
    paths: &AppPaths,
    bottle: &Bottle,
    mut on_step: impl FnMut(&str),
) -> Result<()> {
    let dir = dir(paths, &bottle.id)?;
    let result = (|| {
        let rt = runtime::get(paths, &bottle.runtime)?;
        rt.check_can_run()?;
        on_step("Creating Wine prefix");
        boot_prefix(&rt, &dir, bottle)?;
        on_step("Setting Windows version");
        apply_windows_version(&rt, &dir, bottle)?;
        std::fs::remove_file(dir.join(SETUP_MARKER)).at(&dir)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

fn boot_prefix(rt: &Runtime, dir: &Path, bottle: &Bottle) -> Result<()> {
    let prefix = dir.join(PREFIX_DIR);
    let (log, _) = logs::create(dir, "wineboot")?;
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine");
    cmd.args(["wineboot", "--init"]);
    process::run_and_wait(cmd, &log, "Creating the Wine prefix", WINEBOOT_TIMEOUT)?;
    wait_for_wineserver(rt, &prefix, bottle, &log)
}

fn apply_windows_version(rt: &Runtime, dir: &Path, bottle: &Bottle) -> Result<()> {
    let prefix = dir.join(PREFIX_DIR);
    let (log, _) = logs::create(dir, "winecfg")?;
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine");
    cmd.args(["winecfg", "/v", bottle.windows_version.winecfg_name()]);
    process::run_and_wait(cmd, &log, "Setting the Windows version", WINECFG_TIMEOUT)?;
    wait_for_wineserver(rt, &prefix, bottle, &log)
}

/// Waits until Wine has written the registry and its server has exited.
fn wait_for_wineserver(rt: &Runtime, prefix: &Path, bottle: &Bottle, log: &Path) -> Result<()> {
    let mut cmd = process::command(rt, prefix, bottle, &[], "wineserver");
    cmd.arg("-w");
    process::run_and_wait(cmd, log, "Waiting for Wine to finish", WINESERVER_TIMEOUT)
}

pub fn rename(paths: &AppPaths, id: &str, name: &str) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    bottle.name = validate_name(name)?;
    bottle.save(&dir(paths, id)?)?;
    Ok(bottle)
}

/// Switches the runtime. Wine updates the prefix on its next start.
pub fn set_runtime(paths: &AppPaths, id: &str, runtime_id: &str) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    runtime::get(paths, runtime_id)?;
    bottle.runtime = runtime_id.to_string();
    bottle.save(&dir(paths, id)?)?;
    Ok(bottle)
}

/// Applies a Windows version with `winecfg /v`, then saves it. Blocks for a few seconds.
pub fn set_windows_version(paths: &AppPaths, id: &str, version: WindowsVersion) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    let rt = runtime_for(paths, &bottle)?;
    rt.check_can_run()?;
    bottle.windows_version = version;
    let dir = dir(paths, id)?;
    apply_windows_version(&rt, &dir, &bottle)?;
    bottle.save(&dir)?;
    Ok(bottle)
}

/// Copies a bottle (prefix, icons, programs) under a new name. Logs are not copied.
/// Close the source bottle's programs first, or the copy may be inconsistent.
pub fn duplicate(paths: &AppPaths, id: &str, new_name: &str) -> Result<Bottle> {
    let copy = prepare_duplicate(paths, id, new_name)?;
    finish_duplicate(paths, id, &copy)?;
    Ok(copy)
}

/// Fast part of duplicating: writes the new `bottle.toml` with the setup marker.
pub fn prepare_duplicate(paths: &AppPaths, id: &str, new_name: &str) -> Result<Bottle> {
    let source = get(paths, id)?;
    let name = validate_name(new_name)?;
    let copy = Bottle {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        created_at: now(),
        ..source
    };
    let dst_dir = dir(paths, &copy.id)?;
    std::fs::create_dir_all(&dst_dir).at(&dst_dir)?;
    std::fs::write(dst_dir.join(SETUP_MARKER), "").at(&dst_dir)?;
    copy.save(&dst_dir)?;
    Ok(copy)
}

/// Slow part of duplicating: copies the files. On failure the copy is removed.
pub fn finish_duplicate(paths: &AppPaths, source_id: &str, copy: &Bottle) -> Result<()> {
    let src_dir = dir(paths, source_id)?;
    let dst_dir = dir(paths, &copy.id)?;
    let result = (|| {
        for sub in [PREFIX_DIR, ICONS_DIR] {
            let from = src_dir.join(sub);
            if from.exists() {
                copy_tree(&from, &dst_dir.join(sub))?;
            }
        }
        std::fs::remove_file(dst_dir.join(SETUP_MARKER)).at(&dst_dir)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dst_dir);
    }
    result
}

/// Copies a folder tree. Symlinks are recreated as symlinks and never followed:
/// a Wine prefix links `dosdevices/z:` to `/`.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(|e| Error::Io {
            path: e
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| from.to_path_buf()),
            source: e.into(),
        })?;
        let rel = entry
            .path()
            .strip_prefix(from)
            .expect("walkdir stays under root");
        let target = to.join(rel);
        let ft = entry.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&target).at(&target)?;
        } else if ft.is_symlink() {
            let link = std::fs::read_link(entry.path()).at(entry.path())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link, &target).at(&target)?;
            #[cfg(not(unix))]
            let _ = link;
        } else {
            std::fs::copy(entry.path(), &target).at(&target)?;
        }
    }
    Ok(())
}

/// Stops the bottle's Wine processes and moves its folder to the OS trash
/// (renamed to "<name> (Vinter bottle)" so it is recognisable there).
pub fn delete(paths: &AppPaths, id: &str) -> Result<()> {
    let bottle = get(paths, id)?;
    let _ = kill_all(paths, id);
    let dir = dir(paths, id)?;

    let staging_root = paths.data_dir().join("cache").join("trash");
    let staging = staging_root.join(id);
    std::fs::create_dir_all(&staging).at(&staging)?;
    let safe: String = bottle
        .name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let named = staging.join(format!("{safe} (Vinter bottle)"));
    std::fs::rename(&dir, &named).at(&dir)?;

    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        // No Finder automation permission prompt, unlike the default method.
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    let result = ctx.delete(&named).map_err(|e| {
        // Put it back so nothing is lost.
        let _ = std::fs::rename(&named, &dir);
        Error::Trash(e.to_string())
    });
    let _ = std::fs::remove_dir(&staging);
    let _ = std::fs::remove_dir(&staging_root);
    result
}

/// Stops every Wine process in the bottle (`wineserver -k`).
pub fn kill_all(paths: &AppPaths, id: &str) -> Result<()> {
    let bottle = get(paths, id)?;
    let rt = runtime_for(paths, &bottle)?;
    let dir = dir(paths, id)?;
    let prefix = dir.join(PREFIX_DIR);
    let (log, _) = logs::create(&dir, "wineserver-kill")?;
    let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wineserver");
    cmd.arg("-k");
    // `wineserver -k` exits non-zero when no server is running; that is fine.
    let _ = process::run_and_wait(cmd, &log, "Stopping Wine", WINESERVER_TIMEOUT);
    Ok(())
}

/// Opens a built-in Wine tool in the bottle. Returns once it has started.
pub fn run_tool(paths: &AppPaths, id: &str, tool: Tool) -> Result<u32> {
    let bottle = get(paths, id)?;
    if dir(paths, id)?.join(SETUP_MARKER).exists() {
        return Err(Error::BottleNotReady(bottle.name));
    }
    let rt = runtime_for(paths, &bottle)?;
    rt.check_can_run()?;
    let dir = dir(paths, id)?;
    let (log, _) = logs::create(&dir, tool.program())?;
    let mut cmd = process::command(&rt, &dir.join(PREFIX_DIR), &bottle, &[], "wine");
    cmd.arg(tool.program());
    process::spawn_detached(cmd, &log, &format!("Opening {}", tool.program()))
}

fn runtime_for(paths: &AppPaths, bottle: &Bottle) -> Result<Runtime> {
    runtime::get(paths, &bottle.runtime).map_err(|e| match e {
        Error::RuntimeNotInstalled(id) => Error::BottleRuntimeMissing {
            bottle: bottle.name.clone(),
            runtime: id,
        },
        other => other,
    })
}

fn now() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

#[cfg(test)]
mod tests;
