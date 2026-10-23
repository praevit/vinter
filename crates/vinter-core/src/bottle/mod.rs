//! Bottles: a Wine prefix plus its settings, under `<data>/bottles/<id>/`.

pub mod backup;
pub mod config;
pub mod domain;
pub mod snapshot;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

pub use config::{
    Bottle, BottleOptions, CONFIG_FILE, DomainAccount, Graphics, ProgramDef, WindowsVersion,
};

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
    pub engine: crate::engine::EngineKind,
    /// Whether it keeps working once Rosetta is gone.
    pub readiness: crate::system::readiness::Readiness,
    /// A 32-bit prefix made by older Wine (Vinter only makes 64-bit ones).
    pub legacy_32bit: bool,
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
    /// Id to use (so a caller can follow the bottle before it exists); generated if `None`.
    pub id: Option<String>,
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
    let host = crate::engine::Host::current();
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
            let legacy_32bit = is_32bit_prefix(&path.join(PREFIX_DIR));
            let readiness = if legacy_32bit {
                crate::system::readiness::for_32bit_prefix(&host)
            } else {
                crate::system::readiness::for_engine(b.engine, &host)
            };
            bottles.push(BottleSummary {
                legacy_32bit,
                setup_complete: !path.join(SETUP_MARKER).exists(),
                program_count: b.programs.len() as u32,
                id: b.id,
                name: b.name,
                runtime: b.runtime,
                windows_version: b.windows_version,
                readiness,
                engine: b.engine,
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

/// True if `prefix` is a 32-bit (`WINEARCH=win32`) prefix: Wine writes the
/// prefix's architecture at the top of `system.reg`.
pub fn is_32bit_prefix(prefix: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(prefix.join("system.reg")) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .take(10)
        .map_while(|l| l.ok())
        .any(|l| l.trim() == "#arch=win32")
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

    let id = match new.id {
        Some(id) => {
            validate_id(&id)?;
            if dir(paths, &id)?.exists() {
                return Err(Error::InvalidName(format!(
                    "a bottle with id {id} already exists"
                )));
            }
            id
        }
        None => uuid::Uuid::new_v4().to_string(),
    };
    let bottle = Bottle {
        id,
        name,
        runtime: rt.id.clone(),
        engine: rt.engine,
        created_at: now(),
        windows_version: new.windows_version,
        graphics: new.graphics,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        options: crate::settings::load(paths).new_bottle_options,
        programs: Vec::new(),
        domain: None,
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
        let emulators = crate::engine::installed_emulators(rt.engine(), &rt);
        if !emulators.is_empty() {
            on_step("Setting up x86 emulation");
            register_emulators(&rt, &dir, bottle, &emulators)?;
        }
        keep_desktop_in_bottle(&dir.join(PREFIX_DIR))?;
        on_step("Setting Windows version");
        apply_windows_version(&rt, &dir, bottle)?;
        if needs_registry(&BottleOptions::default(), &bottle.options) {
            on_step("Applying display and keyboard settings");
            apply_options(&rt, &dir, bottle)?;
        }
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
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine")?;
    cmd.args(["wineboot", "--init"]);
    process::run_and_wait(cmd, &log, "Creating the Wine prefix", WINEBOOT_TIMEOUT)?;
    wait_for_wineserver(rt, &prefix, bottle, &log)
}

fn apply_windows_version(rt: &Runtime, dir: &Path, bottle: &Bottle) -> Result<()> {
    let prefix = dir.join(PREFIX_DIR);
    let (log, _) = logs::create(dir, "winecfg")?;
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine")?;
    cmd.args(["winecfg", "/v", bottle.windows_version.winecfg_name()]);
    process::run_and_wait(cmd, &log, "Setting the Windows version", WINECFG_TIMEOUT)?;
    wait_for_wineserver(rt, &prefix, bottle, &log)
}

/// The registry part of the options (macOS Wine driver and DPI), as a `.reg` file.
fn options_reg(options: &BottleOptions) -> String {
    let yes_no = |on: bool| if on { "y" } else { "n" };
    let cmd = yes_no(options.command_as_ctrl);
    let opt = yes_no(options.option_as_alt);
    let dpi = if options.retina_mode { 192 } else { 96 };
    format!(
        "REGEDIT4\r\n\r\n\
         [HKEY_CURRENT_USER\\Software\\Wine\\Mac Driver]\r\n\
         \"RetinaMode\"=\"{retina}\"\r\n\
         \"LeftCommandIsCtrl\"=\"{cmd}\"\r\n\
         \"RightCommandIsCtrl\"=\"{cmd}\"\r\n\
         \"LeftOptionIsAlt\"=\"{opt}\"\r\n\
         \"RightOptionIsAlt\"=\"{opt}\"\r\n\r\n\
         [HKEY_CURRENT_USER\\Control Panel\\Desktop]\r\n\
         \"LogPixels\"=dword:{dpi:08x}\r\n",
        retina = yes_no(options.retina_mode),
    )
}

/// True if going from `old` to `new` changes the registry (macOS only: the
/// options there are for Wine's Mac driver).
fn needs_registry(old: &BottleOptions, new: &BottleOptions) -> bool {
    cfg!(target_os = "macos")
        && (old.retina_mode != new.retina_mode
            || old.command_as_ctrl != new.command_as_ctrl
            || old.option_as_alt != new.option_as_alt)
}

/// Tells Wine which DLLs translate x86 code (FEX on the Apple Silicon engine).
fn register_emulators(
    rt: &Runtime,
    dir: &Path,
    bottle: &Bottle,
    emulators: &[crate::engine::EmulatorDll],
) -> Result<()> {
    let prefix = dir.join(PREFIX_DIR);
    let reg = dir.join(".emulators.reg");
    std::fs::write(&reg, crate::engine::emulators_reg(emulators)).at(&reg)?;
    let (log, _) = logs::create(dir, "emulators")?;
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine")?;
    cmd.args(["regedit", "/S"]).arg(format!(
        "Z:{}",
        reg.display().to_string().replace('/', "\\")
    ));
    let result = process::run_and_wait(cmd, &log, "Setting up x86 emulation", WINECFG_TIMEOUT)
        .and_then(|()| wait_for_wineserver(rt, &prefix, bottle, &log));
    let _ = std::fs::remove_file(&reg);
    result
}

fn apply_options(rt: &Runtime, dir: &Path, bottle: &Bottle) -> Result<()> {
    let prefix = dir.join(PREFIX_DIR);
    let reg = dir.join(".options.reg");
    std::fs::write(&reg, options_reg(&bottle.options)).at(&reg)?;
    let (log, _) = logs::create(dir, "options")?;
    let mut cmd = process::command(rt, &prefix, bottle, SETUP_OVERRIDES, "wine")?;
    cmd.args(["regedit", "/S"]).arg(format!(
        "Z:{}",
        reg.display().to_string().replace('/', "\\")
    ));
    let result = process::run_and_wait(cmd, &log, "Applying settings", WINECFG_TIMEOUT)
        .and_then(|()| wait_for_wineserver(rt, &prefix, bottle, &log));
    let _ = std::fs::remove_file(&reg);
    result
}

/// Changes a bottle's options. Display and keyboard changes are written to
/// the registry (a few seconds); close the bottle's programs first, they see
/// the change on their next start.
pub fn set_options(paths: &AppPaths, id: &str, options: BottleOptions) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    let dir = dir(paths, id)?;
    if needs_registry(&bottle.options, &options) {
        let rt = runtime_for(paths, &bottle)?;
        rt.check_can_run()?;
        let old = bottle.options;
        bottle.options = options;
        if let Err(e) = apply_options(&rt, &dir, &bottle) {
            bottle.options = old;
            return Err(e);
        }
    }
    bottle.options = options;
    bottle.save(&dir)?;
    Ok(bottle)
}

/// Gives each Windows user in the prefix a Desktop folder of its own. Wine
/// links it to the Mac's Desktop, where installers then drop `.lnk`
/// shortcuts. Documents, Downloads and the like stay linked. Anything already
/// on the Mac's Desktop is untouched; only the link is replaced.
pub fn keep_desktop_in_bottle(prefix: &Path) -> Result<()> {
    let users = prefix.join("drive_c").join("users");
    let Ok(entries) = std::fs::read_dir(&users) else {
        return Ok(());
    };
    for user in entries.filter_map(|e| e.ok()) {
        let desktop = user.path().join("Desktop");
        let is_link = std::fs::symlink_metadata(&desktop).is_ok_and(|m| m.file_type().is_symlink());
        if is_link {
            std::fs::remove_file(&desktop).at(&desktop)?;
            std::fs::create_dir(&desktop).at(&desktop)?;
        }
    }
    Ok(())
}

/// Waits until Wine has written the registry and its server has exited.
fn wait_for_wineserver(rt: &Runtime, prefix: &Path, bottle: &Bottle, log: &Path) -> Result<()> {
    let mut cmd = process::command(rt, prefix, bottle, &[], "wineserver")?;
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
/// The new runtime must have the bottle's engine.
pub fn set_runtime(paths: &AppPaths, id: &str, runtime_id: &str) -> Result<Bottle> {
    let mut bottle = get(paths, id)?;
    let rt = runtime::get(paths, runtime_id)?;
    crate::engine::check_bottle(&rt, &bottle)?;
    if bottle.runtime != runtime_id {
        snapshot::auto(paths, id, &format!("Before switching to {runtime_id}"))?;
    }
    bottle.runtime = runtime_id.to_string();
    bottle.save(&dir(paths, id)?)?;
    Ok(bottle)
}

/// Replaces the bottle's DLL overrides. Modes: `n`, `b`, `n,b`, `b,n`,
/// `native`, `builtin`, `native,builtin`, `builtin,native`, or empty (disabled).
pub fn set_dll_overrides(
    paths: &AppPaths,
    id: &str,
    overrides: std::collections::BTreeMap<String, String>,
) -> Result<Bottle> {
    for (dll, mode) in &overrides {
        validate_override(dll, mode)?;
    }
    let mut bottle = get(paths, id)?;
    bottle.dll_overrides = overrides;
    bottle.save(&dir(paths, id)?)?;
    Ok(bottle)
}

/// Checks one DLL override (also used for recipes).
pub fn validate_override(dll: &str, mode: &str) -> Result<()> {
    const MODES: &[&str] = &[
        "",
        "n",
        "b",
        "n,b",
        "b,n",
        "native",
        "builtin",
        "native,builtin",
        "builtin,native",
    ];
    let dll_ok = !dll.is_empty()
        && dll.len() <= 64
        && dll
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !dll_ok {
        return Err(Error::InvalidSetting(format!("{dll:?} is not a DLL name")));
    }
    if !MODES.contains(&mode) {
        return Err(Error::InvalidSetting(format!(
            "{mode:?} is not a valid mode for {dll}; use n, b, n,b, b,n or leave it empty to disable"
        )));
    }
    Ok(())
}

/// Replaces the bottle's extra environment variables (applied last).
pub fn set_env(
    paths: &AppPaths,
    id: &str,
    env: std::collections::BTreeMap<String, String>,
) -> Result<Bottle> {
    for (key, value) in &env {
        validate_env_var(key, value)?;
    }
    let mut bottle = get(paths, id)?;
    bottle.env = env;
    bottle.save(&dir(paths, id)?)?;
    Ok(bottle)
}

/// Checks one environment variable (also used for recipes).
pub fn validate_env_var(key: &str, value: &str) -> Result<()> {
    let key_ok = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && key.len() <= 128;
    if !key_ok {
        return Err(Error::InvalidSetting(format!(
            "{key:?} is not a variable name"
        )));
    }
    if key == "WINEPREFIX" {
        return Err(Error::InvalidSetting(
            "WINEPREFIX is managed by Vinter".into(),
        ));
    }
    if key == "WINEARCH" {
        return Err(Error::InvalidSetting(
            "bottles are always 64-bit; 32-bit programs run in them through WoW64".into(),
        ));
    }
    if value.contains('\0') {
        return Err(Error::InvalidSetting(format!(
            "the value of {key} contains a NUL byte"
        )));
    }
    Ok(())
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
        if copy.domain.is_some() {
            // Best effort: without it, the copy asks for the password again.
            let _ = domain::copy_password(source_id, &copy.id, &format!("Vinter: {}", copy.name));
        }
        std::fs::remove_file(dst_dir.join(SETUP_MARKER)).at(&dst_dir)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dst_dir);
    }
    result
}

/// Copies a folder tree, as copy-on-write clones where the file system has
/// them (APFS, btrfs, XFS): instant, and no extra space until files change.
/// Symlinks are recreated as symlinks and never followed: a Wine prefix links
/// `dosdevices/z:` to `/`. `to` must not exist yet.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    if !to.exists() {
        if clone_path(from, to) {
            return Ok(());
        }
        // A failed directory clone may leave part of the tree behind.
        let _ = std::fs::remove_dir_all(to);
    }
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
        } else if !clone_path(entry.path(), &target) {
            std::fs::copy(entry.path(), &target).at(&target)?;
        }
    }
    Ok(())
}

/// Whether files in `dir` can be cloned, i.e. copies there are free.
pub(crate) fn can_clone(dir: &Path) -> bool {
    let probe = dir.join(".clone-probe");
    let copy = dir.join(".clone-probe-copy");
    let ok = std::fs::write(&probe, b"vinter").is_ok() && clone_path(&probe, &copy);
    let _ = std::fs::remove_file(&probe);
    let _ = std::fs::remove_file(&copy);
    ok
}

/// Clones a file, or on macOS a whole folder tree, without following
/// symlinks. False if the file system cannot; `to` is then left absent.
#[cfg(target_os = "macos")]
fn clone_path(from: &Path, to: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    /// From `<sys/clonefile.h>`.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    let (Ok(src), Ok(dst)) = (
        CString::new(from.as_os_str().as_bytes()),
        CString::new(to.as_os_str().as_bytes()),
    ) else {
        return false;
    };
    unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) == 0 }
}

/// Reflinks a file (`FICLONE`), keeping its permissions.
#[cfg(target_os = "linux")]
fn clone_path(from: &Path, to: &Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(src) = std::fs::File::open(from) else {
        return false;
    };
    let Ok(meta) = src.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    let Ok(dst) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
    else {
        return false;
    };
    let ok = unsafe { libc::ioctl(dst.as_raw_fd(), libc::FICLONE, src.as_raw_fd()) } == 0
        && dst.set_permissions(meta.permissions()).is_ok();
    drop(dst);
    if !ok {
        let _ = std::fs::remove_file(to);
    }
    ok
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clone_path(_from: &Path, _to: &Path) -> bool {
    false
}

/// Stops the bottle's Wine processes and moves its folder to the OS trash
/// (renamed to "<name> (Vinter bottle)" so it is recognisable there).
pub fn delete(paths: &AppPaths, id: &str) -> Result<()> {
    let bottle = get(paths, id)?;
    let _ = kill_all(paths, id);
    let dir = dir(paths, id)?;
    if bottle.domain.is_some() {
        let _ = crate::secret::delete(id);
    }

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

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
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
    let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wineserver")?;
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
    domain::refresh_ticket(&rt, &dir, &bottle, &log);
    let mut cmd = process::command(&rt, &dir.join(PREFIX_DIR), &bottle, &[], "wine")?;
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
