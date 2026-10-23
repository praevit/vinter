//! Finding programs installed in a bottle.

pub mod arch;
pub mod icons;
pub mod msix;
pub mod winpath;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bottle::{self, Bottle, ICONS_DIR, PREFIX_DIR, ProgramDef, SETUP_MARKER};
use crate::error::IoResultExt;
use crate::process::{self, Stream, logs};
use crate::{AppPaths, Error, Result, runtime};

/// Folders Wine itself puts in Program Files; their stubs are not "installed apps".
const WINE_BUILTIN_DIRS: &[&str] = &[
    "common files",
    "internet explorer",
    "windows media player",
    "windows nt",
];

/// Lowercase name fragments of helpers that are never the program itself.
const SKIP_NAME_PARTS: &[&str] = &[
    "unins",
    "uninst",
    "update",
    "updater",
    "crashreport",
    "crashpad",
    "crash_report",
    "bugreport",
    "errorreport",
    "vcredist",
    "vc_redist",
    "dxsetup",
    "dotnetfx",
    "installer",
    "setup",
    "elevate",
    "helper",
    "repair",
];

/// How a program was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Source {
    /// A Start Menu or Desktop shortcut.
    Shortcut,
    /// An `.exe` in Program Files without a shortcut.
    Executable,
}

/// A program found in a bottle, offered to the user as a checkbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DiscoveredProgram {
    pub name: String,
    /// Windows path of the `.exe`.
    pub exe: String,
    pub args: Vec<String>,
    /// PNG path relative to the bottle folder, if an icon was found.
    pub icon: Option<String>,
    /// The icon as a `data:` URL, for showing it in the UI.
    pub icon_data_url: Option<String>,
    pub source: Source,
    /// Already one of the bottle's programs.
    pub already_added: bool,
}

impl DiscoveredProgram {
    pub fn to_def(&self) -> ProgramDef {
        ProgramDef {
            name: self.name.clone(),
            exe: self.exe.clone(),
            args: self.args.clone(),
            icon: self.icon.clone(),
            pinned: false,
            arm64_build: None,
        }
    }
}

/// Scans a bottle for installed programs (section 4.6 of the plan):
/// Start Menu and Desktop shortcuts first, then `.exe` files in Program
/// Files folders that no shortcut points into. Extracts icons as it goes.
pub fn discover(paths: &AppPaths, bottle_id: &str) -> Result<Vec<DiscoveredProgram>> {
    let bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);
    let drive_c = prefix.join("drive_c");

    let mut found: Vec<(String, PathBuf, Vec<String>, Source)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut covered_dirs: HashSet<PathBuf> = HashSet::new();

    for lnk in shortcut_files(&drive_c) {
        let Some((target, args)) = read_shortcut(&prefix, &lnk) else {
            continue;
        };
        let name = file_stem(&lnk);
        if !is_exe(&target) || is_helper(&name) || is_helper(&file_stem(&target)) {
            continue;
        }
        if seen.insert(key(&target)) {
            if let Some(top) = program_folder(&drive_c, &target) {
                covered_dirs.insert(top);
            }
            found.push((name, target, args, Source::Shortcut));
        }
    }

    for folder in program_folders(&drive_c) {
        if covered_dirs.contains(&folder) {
            continue;
        }
        for exe in exes_in(&folder) {
            let name = file_stem(&exe);
            if !is_helper(&name) && seen.insert(key(&exe)) {
                found.push((name, exe, Vec::new(), Source::Executable));
            }
        }
    }

    let added: HashSet<String> = bottle
        .programs
        .iter()
        .map(|p| p.exe.to_lowercase())
        .collect();
    let mut programs = Vec::new();
    for (name, exe, args, source) in found {
        let Some(windows) = winpath::to_windows(&prefix, &exe) else {
            continue;
        };
        let (icon, icon_data_url) = save_icon(&dir, &exe).unwrap_or((None, None));
        programs.push(DiscoveredProgram {
            already_added: added.contains(&windows.to_lowercase()),
            name,
            exe: windows,
            args,
            icon,
            icon_data_url,
            source,
        });
    }
    programs.sort_by(|a, b| {
        (a.source != Source::Shortcut, a.name.to_lowercase())
            .cmp(&(b.source != Source::Shortcut, b.name.to_lowercase()))
    });
    Ok(programs)
}

/// Adds programs to a bottle, skipping ones whose exe is already there.
/// Programs must live on the bottle's C: drive, and icons must be ones
/// [`discover`] saved, since both come from the frontend.
pub fn add(paths: &AppPaths, bottle_id: &str, programs: Vec<ProgramDef>) -> Result<Bottle> {
    let mut bottle = bottle::get(paths, bottle_id)?;
    for program in &programs {
        let exe_ok = program
            .exe
            .get(..3)
            .is_some_and(|p| p.eq_ignore_ascii_case("C:\\"));
        let icon_ok = program.icon.as_deref().is_none_or(|icon| {
            icon.strip_prefix("icons/").is_some_and(|name| {
                crate::error::validate_id(name).is_ok() && name.ends_with(".png")
            })
        });
        if !exe_ok || !icon_ok || program.name.trim().is_empty() {
            return Err(Error::InvalidProgram(program.name.clone()));
        }
    }
    for program in programs {
        let exists = bottle
            .programs
            .iter()
            .any(|p| p.exe.eq_ignore_ascii_case(&program.exe));
        if !exists {
            bottle.programs.push(program);
        }
    }
    bottle.save(&bottle::dir(paths, bottle_id)?)?;
    Ok(bottle)
}

/// Changes to a program; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProgramPatch {
    pub name: Option<String>,
    pub args: Option<Vec<String>>,
    /// An image (`.png`, `.jpg`, `.ico`) or `.exe` to take the icon from.
    pub icon_from: Option<String>,
}

/// Renames a program, changes its arguments or its icon. Launchers are not
/// rebuilt here; callers sync them (they depend on name and icon).
pub fn update(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    patch: ProgramPatch,
) -> Result<Bottle> {
    let mut bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    let program = bottle
        .programs
        .get_mut(index)
        .ok_or(Error::ProgramNotFound(index))?;
    if let Some(name) = patch.name {
        let name = name.trim();
        if name.is_empty() || name.contains(['\0', '/', ':']) {
            return Err(Error::InvalidProgram(name.to_string()));
        }
        program.name = name.to_string();
    }
    if let Some(args) = patch.args {
        if args.iter().any(|a| a.contains('\0')) {
            return Err(Error::InvalidProgram(program.name.clone()));
        }
        program.args = args;
    }
    if let Some(from) = patch.icon_from {
        let from = Path::new(&from);
        let png = icons::png_from_file(from)?.ok_or_else(|| {
            Error::InvalidSetting(format!("{} has no image Vinter can use", from.display()))
        })?;
        let name = format!("{:016x}.png", fnv1a(&png));
        let icons = dir.join(ICONS_DIR);
        std::fs::create_dir_all(&icons).at(&icons)?;
        let path = icons.join(&name);
        std::fs::write(&path, &png).at(&path)?;
        program.icon = Some(format!("{ICONS_DIR}/{name}"));
    }
    bottle.save(&dir)?;
    Ok(bottle)
}

/// Removes a program from a bottle (the files stay installed).
pub fn remove(paths: &AppPaths, bottle_id: &str, index: usize) -> Result<Bottle> {
    let mut bottle = bottle::get(paths, bottle_id)?;
    if index >= bottle.programs.len() {
        return Err(crate::Error::ProgramNotFound(index));
    }
    bottle.programs.remove(index);
    bottle.save(&bottle::dir(paths, bottle_id)?)?;
    Ok(bottle)
}

/// A started program.
#[derive(Debug, Clone)]
pub struct Launched {
    pub pid: u32,
    pub name: String,
    /// Log file id (see [`process::logs::read`]).
    pub log_id: String,
}

/// Starts one of the bottle's programs. Output goes to a new log and to
/// `on_line`; `on_exit` gets the exit code when the program ends.
pub fn launch(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    on_line: impl Fn(Stream, &str) + Send + Sync + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> Result<Launched> {
    launch_with(paths, bottle_id, index, &[], on_line, on_exit)
}

/// [`launch`] with extra environment variables for this run only (they win
/// over the bottle's own).
pub fn launch_with(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    extra_env: &[(&str, &str)],
    on_line: impl Fn(Stream, &str) + Send + Sync + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> Result<Launched> {
    let bottle = ready_bottle(paths, bottle_id)?;
    let program = bottle
        .programs
        .get(index)
        .cloned()
        .ok_or(Error::ProgramNotFound(index))?;
    let rt = runtime::get(paths, &bottle.runtime)?;
    rt.check_can_run()?;
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);
    let exe = winpath::to_unix(&prefix, &program.exe).ok_or_else(|| Error::ProgramMissing {
        name: program.name.clone(),
        exe: program.exe.clone(),
    })?;

    // per-app-engine: the app's own ARM64 build instead, natively.
    let (program, exe) = match native_build(paths, &rt, &prefix, &program) {
        Some((win, unix)) => (
            ProgramDef {
                exe: win,
                ..program
            },
            unix,
        ),
        None => (program, exe),
    };
    let found = check_architecture(&program.name, &exe, &bottle, &rt)?;
    let (log, _) = logs::create(&dir, &program.name)?;
    bottle::domain::refresh_ticket(&rt, &dir, &bottle, &log);
    let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wine")?;
    cmd.envs(extra_env.iter().copied());
    let notes = wow64_notes(&rt, &exe, found);
    crate::diag::write_launch_header(&log, &program.name, found, &rt, &bottle, &cmd, &notes)?;
    // Skip empty or NUL-containing arguments; programs added before the
    // shortcut fix may have a stray "\0" argument saved.
    let args = program
        .args
        .iter()
        .filter(|a| !a.is_empty() && !a.contains('\0'));
    cmd.arg(&program.exe).args(args);
    if let Some(folder) = exe.parent() {
        // Many programs expect to start in their own folder.
        cmd.current_dir(folder);
    }
    let pid = process::launch(
        cmd,
        &log,
        &format!("Starting {}", program.name),
        on_line,
        on_exit,
    )?;
    Ok(Launched {
        pid,
        name: program.name,
        log_id: log_id(&log),
    })
}

/// With the `wow64-large-address` flag: what a 32-bit program's address
/// space is, and a warning on Apple Silicon, where WoW64 is new. The log
/// doctor uses the marker to explain out-of-memory errors.
pub(crate) fn wow64_notes(
    rt: &runtime::Runtime,
    exe: &Path,
    found: Option<(arch::PeArch, arch::RunPath)>,
) -> Vec<String> {
    let flag = crate::experimental::ExperimentalFlag::Wow64LargeAddress;
    if !crate::engine::flags_for_runtime(rt).is_enabled(flag)
        || !matches!(found, Some((arch::PeArch::I386, _)))
    {
        return Vec::new();
    }
    let aware = arch::large_address_aware(exe).unwrap_or(false);
    let mut notes = vec![format!(
        "{flag}: 32-bit program, large-address-aware: {}",
        if aware {
            "yes (up to 4 GB of address space under WoW64)"
        } else {
            "no (limited to 2 GB of address space)"
        }
    )];
    if crate::engine::is_fex(rt.engine) {
        notes.push(format!(
            "{flag}: warning: 32-bit programs on Apple Silicon run in an experimental 4 GB WoW64 window; memory-hungry ones may run out of address space"
        ));
    }
    notes
}

/// The ARM64 build to launch instead of `program`'s own exe, if the
/// `per-app-engine` flag is on, the bottle is on the Apple Silicon engine
/// and the build chosen for it is still there and still ARM.
fn native_build(
    paths: &AppPaths,
    rt: &runtime::Runtime,
    prefix: &Path,
    program: &ProgramDef,
) -> Option<(String, PathBuf)> {
    let flag = crate::experimental::ExperimentalFlag::PerAppEngine;
    if !crate::engine::is_fex(rt.engine)
        || !crate::experimental::Flags::load(paths).is_enabled(flag)
    {
        return None;
    }
    let win = program.arm64_build.clone()?;
    let unix = winpath::to_unix(prefix, &win)?;
    arch::detect(&unix)
        .is_some_and(|a| a.is_arm())
        .then_some((win, unix))
}

/// ARM64 builds of a bottle's program found next to it: `.exe` files in its
/// folder, or in an `arm64`/`aarch64` folder beside it, with the same name
/// and built for ARM. These are what `per-app-engine` can launch instead.
pub fn arm64_builds(paths: &AppPaths, bottle_id: &str, index: usize) -> Result<Vec<String>> {
    let bottle = bottle::get(paths, bottle_id)?;
    let program = bottle
        .programs
        .get(index)
        .ok_or(Error::ProgramNotFound(index))?;
    let prefix = bottle::dir(paths, bottle_id)?.join(PREFIX_DIR);
    let Some(exe) = winpath::to_unix(&prefix, &program.exe) else {
        return Ok(Vec::new());
    };
    let (Some(folder), Some(stem)) = (exe.parent(), exe.file_stem()) else {
        return Ok(Vec::new());
    };
    let stem = stem.to_string_lossy().to_ascii_lowercase();
    // ARM folders as they are spelled on disk (macOS file systems ignore
    // case, so guessing spellings would find one folder several times).
    let mut dirs = vec![folder.to_path_buf()];
    for base in [Some(folder), folder.parent()].into_iter().flatten() {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(name.as_str(), "arm64" | "aarch64") && entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
    }
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let is_exe = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            let same_app = name == stem
                || name.strip_prefix(&stem).is_some_and(|rest| {
                    matches!(
                        rest.trim_start_matches(['-', '_', '.']),
                        "arm64" | "aarch64" | "arm"
                    )
                });
            if is_exe
                && same_app
                && path != exe
                && arch::detect(&path).is_some_and(|a| a.is_arm())
                && let Some(win) = winpath::to_windows(&prefix, &path)
            {
                found.push(win);
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// Chooses (or with `None`, clears) the ARM64 build `per-app-engine` launches
/// for a program. It must be one of [`arm64_builds`].
pub fn set_arm64_build(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    build: Option<String>,
) -> Result<Bottle> {
    let flag = crate::experimental::ExperimentalFlag::PerAppEngine;
    if !crate::experimental::Flags::load(paths).is_enabled(flag) {
        return Err(Error::Unsupported(format!(
            "choosing an engine per app is experimental; turn on {flag} first"
        )));
    }
    if let Some(b) = &build
        && !arm64_builds(paths, bottle_id, index)?.contains(b)
    {
        return Err(Error::InvalidSetting(format!(
            "{b} is not an ARM64 build of this program"
        )));
    }
    let mut bottle = bottle::get(paths, bottle_id)?;
    let program = bottle
        .programs
        .get_mut(index)
        .ok_or(Error::ProgramNotFound(index))?;
    program.arm64_build = build;
    bottle.save(&bottle::dir(paths, bottle_id)?)?;
    Ok(bottle)
}

/// For a bottle's 32-bit program: whether it is large-address-aware.
pub fn large_address_aware_of(
    paths: &AppPaths,
    bottle_id: &str,
    program: &ProgramDef,
) -> Option<bool> {
    let prefix = bottle::dir(paths, bottle_id).ok()?.join(PREFIX_DIR);
    let exe = winpath::to_unix(&prefix, &program.exe)?;
    if arch::detect(&exe)? != arch::PeArch::I386 {
        return None;
    }
    arch::large_address_aware(&exe)
}

/// What CPU a bottle's program is built for, if its file can be read.
pub fn arch_of(paths: &AppPaths, bottle_id: &str, program: &ProgramDef) -> Option<arch::PeArch> {
    let prefix = bottle::dir(paths, bottle_id).ok()?.join(PREFIX_DIR);
    arch::detect(&winpath::to_unix(&prefix, &program.exe)?)
}

/// The architecture of `exe` and how it runs on `rt`, or an error if it
/// cannot run there: an ARM64 program on an Intel runtime, or an Intel
/// program on an Apple Silicon runtime without the FEX emulator it needs.
/// `None` for files whose architecture is unknown: Wine says what is wrong.
fn check_architecture(
    name: &str,
    exe: &Path,
    bottle: &Bottle,
    rt: &runtime::Runtime,
) -> Result<Option<(arch::PeArch, arch::RunPath)>> {
    let Some(pe) = arch::detect(exe) else {
        return Ok(None);
    };
    let wrong = |reason: String| Error::WrongArchitecture {
        name: name.to_string(),
        bottle: bottle.name.clone(),
        reason,
    };
    let path = arch::run_path(pe, rt.engine).map_err(wrong)?;
    let needs = match path {
        arch::RunPath::Fex64 => Some(crate::engine::Translates::X86_64),
        arch::RunPath::FexWow64 => Some(crate::engine::Translates::X86),
        _ => None,
    };
    if let Some(needs) = needs {
        let shipped = crate::engine::installed_emulators(rt.engine(), rt);
        if !shipped.iter().any(|d| d.translates == needs) {
            return Err(wrong(format!(
                "{} does not include FEX's {} emulator, which Intel programs like this need \
                 on Apple Silicon. Use another runtime, or an Intel (Rosetta) bottle.",
                rt.name,
                match needs {
                    crate::engine::Translates::X86_64 => "64-bit",
                    crate::engine::Translates::X86 => "32-bit",
                }
            )));
        }
    }
    Ok(Some((pe, path)))
}

/// Runs an installer (`.exe` or `.msi`) in a bottle and waits for it to exit.
/// Returns its exit code: many installers return non-zero when cancelled,
/// so callers should still look for new programs afterwards.
/// `on_started` gets the pid once it runs.
pub fn run_installer(
    paths: &AppPaths,
    bottle_id: &str,
    installer: &Path,
    args: &[String],
    on_started: impl FnOnce(u32),
) -> Result<Option<i32>> {
    let bottle = ready_bottle(paths, bottle_id)?;
    if !installer.is_file() {
        return Err(Error::InstallerNotFound(installer.to_path_buf()));
    }
    let rt = runtime::get(paths, &bottle.runtime)?;
    rt.check_can_run()?;
    let file = installer.file_name().unwrap_or_default().to_string_lossy();
    let found = check_architecture(&file, installer, &bottle, &rt)?;
    bottle::snapshot::auto(paths, bottle_id, &format!("Before installing {file}"))?;
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);
    // Also for bottles made before Vinter did this at creation.
    bottle::keep_desktop_in_bottle(&prefix)?;
    // MSIX/AppX packages are unpacked by Vinter; Wine cannot install them.
    if msix::is_package(installer) {
        msix::install(paths, bottle_id, installer)?;
        return Ok(Some(0));
    }

    let (log, _) = logs::create(&dir, "installer")?;
    bottle::domain::refresh_ticket(&rt, &dir, &bottle, &log);
    let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wine")?;
    let notes = wow64_notes(&rt, installer, found);
    crate::diag::write_launch_header(&log, &file, found, &rt, &bottle, &cmd, &notes)?;
    // Wine's msiexec crashes on long host paths (seen with a 153-character
    // path: msi.dll read part of the file name as a pointer). MSI packages are
    // self-contained, so run them from a short path inside the bottle.
    // .exe installers stay put: many need files next to them.
    let staged_msi = if has_extension(installer, "msi") {
        let name = installer.file_name().unwrap_or_default();
        let folder = prefix.join("drive_c/windows/Temp/vinter");
        std::fs::create_dir_all(&folder).at(&folder)?;
        let staged = folder.join(name);
        std::fs::copy(installer, &staged).at(&staged)?;
        cmd.args(["msiexec", "/i"]).arg(format!(
            r"C:\windows\Temp\vinter\{}",
            name.to_string_lossy()
        ));
        Some(staged)
    } else {
        cmd.arg(installer);
        None
    };
    cmd.args(args);
    if let Some(folder) = installer.parent() {
        cmd.current_dir(folder);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let pid = process::launch(
        cmd,
        &log,
        "Running the installer",
        |_, _| {},
        move |code| {
            let _ = tx.send(code);
        },
    )?;
    on_started(pid);
    let code = rx.recv().unwrap_or(None);
    if let Some(staged) = staged_msi {
        let _ = std::fs::remove_file(staged);
    }
    Ok(code)
}

fn ready_bottle(paths: &AppPaths, bottle_id: &str) -> Result<Bottle> {
    let bottle = bottle::get(paths, bottle_id)?;
    if bottle::dir(paths, bottle_id)?.join(SETUP_MARKER).exists() {
        return Err(Error::BottleNotReady(bottle.name));
    }
    Ok(bottle)
}

fn log_id(log: &Path) -> String {
    log.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A bottle's program icon as a `data:` URL.
pub fn icon_data_url(paths: &AppPaths, bottle_id: &str, icon: &str) -> Option<String> {
    // Icons are stored as `icons/<name>.png`; never read anything else.
    let name = icon.strip_prefix("icons/")?;
    crate::error::validate_id(name).ok()?;
    let path = bottle::dir(paths, bottle_id)
        .ok()?
        .join(ICONS_DIR)
        .join(name);
    std::fs::read(path).ok().map(|png| icons::data_url(&png))
}

/// Extracts `exe`'s icon into the bottle and returns its `icons/...` path.
pub fn save_icon_for(paths: &AppPaths, bottle_id: &str, exe: &Path) -> Option<String> {
    let dir = bottle::dir(paths, bottle_id).ok()?;
    save_icon(&dir, exe).ok()?.0
}

/// Extracts and saves `exe`'s icon as `icons/<hash>.png`.
fn save_icon(dir: &Path, exe: &Path) -> Result<(Option<String>, Option<String>)> {
    let Some(png) = icons::extract_png(exe)? else {
        return Ok((None, None));
    };
    let name = format!("{:016x}.png", fnv1a(key(exe).as_bytes()));
    let icons = dir.join(ICONS_DIR);
    std::fs::create_dir_all(&icons).at(&icons)?;
    let path = icons.join(&name);
    std::fs::write(&path, &png).at(&path)?;
    Ok((
        Some(format!("{ICONS_DIR}/{name}")),
        Some(icons::data_url(&png)),
    ))
}

/// Stable, dependency-free hash for icon file names.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

fn shortcut_files(drive_c: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        drive_c.join("ProgramData/Microsoft/Windows/Start Menu"),
        drive_c.join("users/Public/Desktop"),
    ];
    if let Ok(users) = std::fs::read_dir(drive_c.join("users")) {
        for user in users.flatten() {
            let u = user.path();
            roots.push(u.join("AppData/Roaming/Microsoft/Windows/Start Menu"));
            roots.push(u.join("Desktop"));
        }
    }
    let mut files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| {
            walkdir::WalkDir::new(root)
                .max_depth(6)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| e.into_path())
                .filter(|p| has_extension(p, "lnk"))
        })
        .collect();
    files.sort();
    files
}

/// Target exe and arguments of a `.lnk`, resolved inside the prefix.
fn read_shortcut(prefix: &Path, lnk: &Path) -> Option<(PathBuf, Vec<String>)> {
    // The `lnk` crate panics on some malformed links; treat those as unreadable.
    let parsed = std::panic::catch_unwind(|| {
        let link = lnk::ShellLink::open(lnk, lnk::encoding::WINDOWS_1252).ok()?;
        let target = link.link_target();
        let relative = link.string_data().relative_path().clone();
        let args = link.string_data().command_line_arguments().clone();
        Some((target, relative, args))
    })
    .ok()??;
    // Shortcut strings may carry their NUL terminator (Wine writes an
    // "empty" argument string as a lone `\0`); cut at the first NUL.
    let (target, relative, args) = (
        parsed.0.map(|s| until_nul(&s)),
        parsed.1.map(|s| until_nul(&s)),
        parsed.2.map(|s| until_nul(&s)),
    );

    let resolved = target
        .and_then(|t| winpath::to_unix(prefix, &t))
        .or_else(|| {
            let rel = relative?.replace('\\', "/");
            let joined = lnk.parent()?.join(rel);
            let canonical = joined.canonicalize().ok()?;
            canonical
                .starts_with(prefix.canonicalize().ok()?)
                .then_some(canonical)
        })?;
    let args = args.map(|a| split_args(&a)).unwrap_or_default();
    Some((resolved, args))
}

fn until_nul(s: &str) -> String {
    s.split('\0').next().unwrap_or_default().to_string()
}

/// Splits a Windows command line into arguments (double quotes group).
fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Top-level folders in Program Files, minus Wine's own.
fn program_folders(drive_c: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for root in ["Program Files", "Program Files (x86)"] {
        let Ok(entries) = std::fs::read_dir(drive_c.join(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if entry.path().is_dir() && !WINE_BUILTIN_DIRS.contains(&name.as_str()) {
                folders.push(entry.path());
            }
        }
    }
    folders.sort();
    folders
}

/// The Program Files subfolder a path is in, e.g. `.../Program Files/7-Zip`.
fn program_folder(drive_c: &Path, path: &Path) -> Option<PathBuf> {
    for root in ["Program Files", "Program Files (x86)"] {
        let base = drive_c.join(root);
        if let Ok(rel) = path.strip_prefix(&base) {
            return rel.components().next().map(|c| base.join(c));
        }
    }
    None
}

fn exes_in(folder: &Path) -> Vec<PathBuf> {
    let mut exes: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| is_exe(p))
        .collect();
    exes.sort();
    exes
}

fn is_exe(path: &Path) -> bool {
    has_extension(path, "exe")
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
}

fn is_helper(name: &str) -> bool {
    let lower = name.to_lowercase();
    SKIP_NAME_PARTS.iter().any(|part| lower.contains(part))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests;
