//! Running a recipe: runtime, installer, bottle, graphics, steps, programs.
//!
//! Progress is saved in `<bottle>/.recipe-state.toml` after every step, so a
//! failed setup can be resumed where it stopped.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Installer, LoadedRecipe, Step, find, winetricks};
use crate::bottle::{self, Bottle, Graphics, NewBottle, PREFIX_DIR, ProgramDef};
use crate::download::{self, InstallProgress};
use crate::error::IoResultExt;
use crate::process::{self, logs};
use crate::programs::{self, Source, winpath};
use crate::runtime::{self, Runtime};
use crate::{AppPaths, Error, Result, graphics};

const STATE_FILE: &str = ".recipe-state.toml";
const FILES_DIR: &str = "recipe-files";
const STEP_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct ApplyOptions {
    /// Id for the new bottle (so a UI can follow it); generated if `None`.
    pub bottle_id: Option<String>,
    pub bottle_name: String,
    /// The installer file, required for `user_provided` installers.
    /// For `download` installers it replaces the download (no checksum check).
    pub installer: Option<PathBuf>,
    /// The user agreed to winetricks downloads (may include Microsoft software).
    pub accept_downloads: bool,
    /// Installed runtime to use instead of the one the recipe would pick.
    pub runtime: Option<String>,
}

/// Progress of a recipe run: task `task` of `tasks`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeProgress {
    pub task: usize,
    pub tasks: usize,
    pub label: String,
    pub detail: String,
    pub percent: Option<f64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StateFile {
    recipe: String,
    completed: Vec<usize>,
    installer: Option<PathBuf>,
    error: Option<String>,
}

/// An unfinished recipe in a bottle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RecipeState {
    pub recipe_id: String,
    pub recipe_name: String,
    pub completed_steps: u32,
    pub total_steps: u32,
    pub error: Option<String>,
    /// Resuming needs the user to pick the installer again.
    pub needs_installer: bool,
}

/// The unfinished recipe in a bottle, if any.
pub fn state(paths: &AppPaths, bottle_id: &str) -> Option<RecipeState> {
    let s = read_state(paths, bottle_id).ok()?;
    let loaded = find(paths, &s.recipe).ok();
    let total = loaded.as_ref().map_or(0, |l| l.recipe.steps.len());
    let user_provided = matches!(
        loaded.as_ref().and_then(|l| l.recipe.installer.as_ref()),
        Some(Installer::UserProvided { .. })
    );
    Some(RecipeState {
        recipe_name: loaded.map_or_else(|| s.recipe.clone(), |l| l.recipe.name),
        recipe_id: s.recipe,
        completed_steps: s.completed.len() as u32,
        total_steps: total as u32,
        error: s.error,
        needs_installer: user_provided && !s.installer.as_deref().is_some_and(Path::is_file),
    })
}

/// Creates a bottle from scratch and sets it up with `loaded`.
pub async fn apply<P>(
    paths: &AppPaths,
    loaded: &LoadedRecipe,
    opts: ApplyOptions,
    mut on_progress: P,
) -> Result<Bottle>
where
    P: FnMut(RecipeProgress) + Send,
{
    let r = &loaded.recipe;
    check_consent(loaded, opts.accept_downloads)?;
    check_installer_given(loaded, opts.installer.as_deref())?;
    let runtime_choice = match &opts.runtime {
        Some(id) => RuntimeChoice::Installed(runtime::get(paths, id)?),
        None => pick_runtime(paths, r.runtime_flavor)?,
    };

    let mut tasks = Tasks::new(&mut on_progress);
    tasks.total = r.steps.len()
        + 2 // create bottle, add programs
        + usize::from(matches!(runtime_choice, RuntimeChoice::Install(_)))
        + usize::from(needs_download(loaded, opts.installer.as_deref()))
        + usize::from(!loaded.winetricks_verbs().is_empty())
        + usize::from(r.graphics != Graphics::Wined3d);

    let rt = match runtime_choice {
        RuntimeChoice::Installed(rt) => rt,
        RuntimeChoice::Install(info) => {
            tasks.start(&format!("Getting {}", info.name));
            runtime::install(paths, &info, |p| tasks.download(p)).await?
        }
    };
    let installer = get_installer(paths, loaded, opts.installer.clone(), &mut tasks).await?;
    let script = get_winetricks(paths, loaded, &mut tasks).await?;

    tasks.start("Creating the bottle");
    let new = NewBottle {
        id: opts.bottle_id,
        name: opts.bottle_name,
        runtime: rt.id.clone(),
        windows_version: r.windows_version,
        graphics: Some(Graphics::Wined3d),
    };
    let bottle = bottle::prepare_create(paths, new)?;
    let mut state = StateFile {
        recipe: r.id.clone(),
        installer: installer.clone(),
        ..Default::default()
    };
    write_state(paths, &bottle.id, &state)?;
    {
        let (paths, bottle) = (paths.clone(), bottle.clone());
        blocking(move || bottle::finish_create(&paths, &bottle, |_| {})).await?;
    }

    finish(
        paths, loaded, &bottle.id, &rt, installer, script, &mut state, &mut tasks,
    )
    .await
}

/// Continues an unfinished recipe in a bottle from the first step not done.
pub async fn resume<P>(
    paths: &AppPaths,
    bottle_id: &str,
    installer: Option<PathBuf>,
    accept_downloads: bool,
    mut on_progress: P,
) -> Result<Bottle>
where
    P: FnMut(RecipeProgress) + Send,
{
    let mut state =
        read_state(paths, bottle_id).map_err(|_| Error::NothingToResume(bottle_id.to_string()))?;
    let loaded = find(paths, &state.recipe)?;
    check_consent(&loaded, accept_downloads)?;
    let bottle = bottle::get(paths, bottle_id)?;
    let rt = runtime::get(paths, &bottle.runtime)?;
    let given = installer.or_else(|| state.installer.clone().filter(|p| p.is_file()));
    check_installer_given(&loaded, given.as_deref())?;

    let mut tasks = Tasks::new(&mut on_progress);
    let remaining =
        loaded.recipe.steps.len() - state.completed.len().min(loaded.recipe.steps.len());
    tasks.total = remaining
        + 1
        + usize::from(needs_download(&loaded, given.as_deref()))
        + usize::from(!loaded.winetricks_verbs().is_empty())
        + usize::from(loaded.recipe.graphics != Graphics::Wined3d);
    let installer = get_installer(paths, &loaded, given, &mut tasks).await?;
    let script = get_winetricks(paths, &loaded, &mut tasks).await?;
    state.installer = installer.clone();
    state.error = None;
    finish(
        paths, &loaded, bottle_id, &rt, installer, script, &mut state, &mut tasks,
    )
    .await
}

/// Graphics, remaining steps, programs; then marks the bottle done.
#[allow(clippy::too_many_arguments)]
async fn finish<P>(
    paths: &AppPaths,
    loaded: &LoadedRecipe,
    bottle_id: &str,
    rt: &Runtime,
    installer: Option<PathBuf>,
    script: Option<PathBuf>,
    state: &mut StateFile,
    tasks: &mut Tasks<'_, P>,
) -> Result<Bottle>
where
    P: FnMut(RecipeProgress) + Send,
{
    let r = &loaded.recipe;
    if r.graphics != Graphics::Wined3d {
        tasks.start(&format!("Setting up {}", r.graphics.name()));
        if bottle::get(paths, bottle_id)?.graphics != r.graphics {
            let info = graphics::backends(paths, rt)
                .into_iter()
                .find(|b| b.backend == r.graphics)
                .expect("every backend is listed");
            match info.reason {
                // Apple Silicon bottles only offer what their runtime ships;
                // the recipe still works with WineD3D, just slower.
                Some(_) if crate::engine::is_fex(rt.engine) => {}
                Some(reason) => return Err(Error::BackendUnavailable(reason)),
                None => {
                    if let Some(c) = &info.component {
                        graphics::component::ensure(paths, c, |p| tasks.download(p)).await?;
                    }
                    let (paths, id, backend) = (paths.clone(), bottle_id.to_string(), r.graphics);
                    blocking(move || graphics::apply(&paths, &id, backend)).await?;
                }
            }
        }
    }

    for (i, step) in r.steps.iter().enumerate() {
        if state.completed.contains(&i) {
            continue;
        }
        tasks.start(&step.label());
        let result = {
            let (paths, loaded, id, step) = (
                paths.clone(),
                loaded.clone(),
                bottle_id.to_string(),
                step.clone(),
            );
            let (installer, script) = (installer.clone(), script.clone());
            blocking(move || {
                run_step(
                    &paths,
                    &loaded,
                    &id,
                    &step,
                    installer.as_deref(),
                    script.as_deref(),
                )
            })
            .await
        };
        if let Err(e) = result {
            let err = Error::RecipeStepFailed {
                step: format!("step {} ({})", i + 1, step.label()),
                message: e.to_string(),
            };
            state.error = Some(err.to_string());
            write_state(paths, bottle_id, state)?;
            return Err(err);
        }
        state.completed.push(i);
        write_state(paths, bottle_id, state)?;
    }

    tasks.start("Adding programs");
    {
        let (paths, loaded, id) = (paths.clone(), loaded.clone(), bottle_id.to_string());
        blocking(move || add_programs(&paths, &loaded, &id)).await?;
    }

    let mut bottle = bottle::get(paths, bottle_id)?;
    bottle.recipe = Some(r.id.clone());
    let dir = bottle::dir(paths, bottle_id)?;
    bottle.save(&dir)?;
    let _ = std::fs::remove_file(dir.join(STATE_FILE));
    let _ = std::fs::remove_dir_all(dir.join(FILES_DIR));
    if matches!(r.installer, Some(Installer::Download { .. })) {
        if let Some(path) = installer.filter(|p| p.starts_with(paths.downloads_dir())) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(bottle)
}

fn run_step(
    paths: &AppPaths,
    loaded: &LoadedRecipe,
    bottle_id: &str,
    step: &Step,
    installer: Option<&Path>,
    winetricks_script: Option<&Path>,
) -> Result<()> {
    let bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);
    match step {
        Step::Winetricks { verbs } => {
            let script = winetricks_script
                .ok_or_else(|| Error::InvalidRecipe("winetricks is missing".into()))?;
            winetricks::run(paths, bottle_id, script, verbs)
        }
        Step::DllOverride { dll, mode } => {
            let mut overrides = bottle.dll_overrides.clone();
            overrides.insert(dll.clone(), mode.clone());
            bottle::set_dll_overrides(paths, bottle_id, overrides).map(|_| ())
        }
        Step::Env { vars } => {
            let mut env = bottle.env.clone();
            env.extend(vars.clone());
            bottle::set_env(paths, bottle_id, env).map(|_| ())
        }
        Step::Registry { file } => {
            let files = dir.join(FILES_DIR);
            std::fs::create_dir_all(&files).at(&files)?;
            let path = files.join(file);
            std::fs::write(&path, loaded.file(file)?).at(&path)?;
            let rt = runtime::get(paths, &bottle.runtime)?;
            rt.check_can_run()?;
            let (log, _) = logs::create(&dir, "regedit")?;
            let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wine")?;
            cmd.args(["regedit", "/S"]).arg(z_path(&path));
            process::run_and_wait(
                cmd,
                &log,
                &format!("Importing {file}"),
                Duration::from_secs(120),
            )
        }
        Step::CopyFile { from, to } => {
            let target = winpath::to_unix_for_write(&prefix, to)
                .ok_or_else(|| Error::InvalidRecipe(format!("{to} is not a path on C:")))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).at(parent)?;
            }
            std::fs::write(&target, loaded.file(from)?).at(&target)
        }
        Step::RunInstaller { args } => {
            let installer = installer.ok_or_else(|| Error::InstallerRequired(String::new()))?;
            match programs::run_installer(paths, bottle_id, installer, args, |_| {})? {
                Some(0) => Ok(()),
                Some(code) => Err(Error::Unsupported(format!(
                    "the installer exited with code {code} (it may have been cancelled)"
                ))),
                None => Err(Error::Unsupported("the installer was stopped".into())),
            }
        }
        Step::Run { exe, args } => {
            let unix = winpath::to_unix(&prefix, exe).ok_or_else(|| Error::ProgramMissing {
                name: exe.clone(),
                exe: exe.clone(),
            })?;
            let rt = runtime::get(paths, &bottle.runtime)?;
            rt.check_can_run()?;
            let (log, _) = logs::create(&dir, "recipe-run")?;
            let mut cmd = process::command(&rt, &prefix, &bottle, &[], "wine")?;
            cmd.arg(exe).args(args);
            if let Some(folder) = unix.parent() {
                cmd.current_dir(folder);
            }
            match process::run_status(cmd, &log, &format!("Running {exe}"), STEP_TIMEOUT)? {
                Some(0) => Ok(()),
                other => Err(Error::Unsupported(format!("{exe} exited with {other:?}"))),
            }
        }
    }
}

/// Adds the recipe's programs that exist; if none do, the installer's shortcuts.
/// They are pinned: an app set up from a recipe gets a launcher right away
/// (the caller creates it with [`crate::launcher::sync_bottle`]).
fn add_programs(paths: &AppPaths, loaded: &LoadedRecipe, bottle_id: &str) -> Result<()> {
    let prefix = bottle::dir(paths, bottle_id)?.join(PREFIX_DIR);
    let mut defs: Vec<ProgramDef> = loaded
        .recipe
        .programs
        .iter()
        .filter_map(|p| {
            let unix = winpath::to_unix(&prefix, &p.exe)?;
            Some(ProgramDef {
                name: p.name.clone(),
                exe: p.exe.clone(),
                args: p.args.clone(),
                icon: programs::save_icon_for(paths, bottle_id, &unix),
                pinned: true,
                arm64_build: None,
                last_launched: None,
                launch_count: 0,
            })
        })
        .collect();
    if defs.is_empty() {
        defs = programs::discover(paths, bottle_id)?
            .into_iter()
            .filter(|p| p.source == Source::Shortcut && !p.already_added)
            .map(|p| ProgramDef {
                pinned: true,
                ..p.to_def()
            })
            .collect();
    }
    if defs.is_empty() && !loaded.recipe.programs.is_empty() {
        return Err(Error::RecipeStepFailed {
            step: "Adding programs".into(),
            message: "the installer did not install any program Vinter can find".into(),
        });
    }
    programs::add(paths, bottle_id, defs).map(|_| ())
}

fn check_consent(loaded: &LoadedRecipe, accepted: bool) -> Result<()> {
    let verbs = loaded.winetricks_verbs();
    if !verbs.is_empty() && !accepted {
        return Err(Error::ConsentRequired(verbs.join(", ")));
    }
    Ok(())
}

fn check_installer_given(loaded: &LoadedRecipe, given: Option<&Path>) -> Result<()> {
    let runs_installer = loaded
        .recipe
        .steps
        .iter()
        .any(|s| matches!(s, Step::RunInstaller { .. }));
    match (&loaded.recipe.installer, given) {
        (_, Some(path)) if !path.is_file() => Err(Error::InstallerNotFound(path.to_path_buf())),
        (Some(Installer::UserProvided { hint }), None) if runs_installer => {
            Err(Error::InstallerRequired(hint.clone()))
        }
        _ => Ok(()),
    }
}

fn needs_download(loaded: &LoadedRecipe, given: Option<&Path>) -> bool {
    given.is_none() && matches!(loaded.recipe.installer, Some(Installer::Download { .. }))
}

enum RuntimeChoice {
    Installed(Runtime),
    Install(runtime::RuntimeInfo),
}

/// An installed runtime of the wanted flavor, or one to install. Recipes
/// were written and tested on Intel runtimes, so those come first; Apple
/// Silicon ones are used only when nothing else fits.
fn pick_runtime(paths: &AppPaths, flavor: Option<runtime::Flavor>) -> Result<RuntimeChoice> {
    let wanted = flavor.unwrap_or(runtime::Flavor::Stable);
    let fex_last = |engine: crate::engine::EngineKind| crate::engine::is_fex(engine);
    let mut installed = runtime::installed(paths)?;
    installed.retain(|r| r.unavailable.is_none());
    installed.sort_by_key(|r| fex_last(r.engine));
    if let Some(rt) = installed
        .iter()
        .find(|r| r.flavor == wanted)
        .or(if flavor.is_none() {
            installed.first()
        } else {
            None
        })
    {
        return Ok(RuntimeChoice::Installed(rt.clone()));
    }
    let mut offered = runtime::manifest::available(paths);
    offered.sort_by_key(|r| fex_last(r.engine));
    offered
        .into_iter()
        .find(|r| r.flavor == wanted)
        .map(RuntimeChoice::Install)
        .ok_or_else(|| Error::RuntimeNotFound(format!("{wanted:?}").to_lowercase()))
}

async fn get_installer<P>(
    paths: &AppPaths,
    loaded: &LoadedRecipe,
    given: Option<PathBuf>,
    tasks: &mut Tasks<'_, P>,
) -> Result<Option<PathBuf>>
where
    P: FnMut(RecipeProgress) + Send,
{
    if given.is_some() {
        return Ok(given);
    }
    let Some(Installer::Download { url, sha256, size }) = &loaded.recipe.installer else {
        return Ok(None);
    };
    tasks.start("Downloading the installer");
    let ext = url
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 4 && e.chars().all(char::is_alphanumeric))
        .unwrap_or("exe");
    let dir = paths.downloads_dir();
    tokio::fs::create_dir_all(&dir).await.at(&dir)?;
    let path = dir.join(format!(
        "recipe-{}-{}.{ext}",
        loaded.recipe.id,
        &sha256[..12]
    ));
    if !path.is_file()
        || download::verify(&path, sha256, &mut |p| tasks.download(p))
            .await
            .is_err()
    {
        download::download(url, &path, size.unwrap_or(0), &mut |p| tasks.download(p)).await?;
        download::verify(&path, sha256, &mut |p| tasks.download(p)).await?;
    }
    Ok(Some(path))
}

async fn get_winetricks<P>(
    paths: &AppPaths,
    loaded: &LoadedRecipe,
    tasks: &mut Tasks<'_, P>,
) -> Result<Option<PathBuf>>
where
    P: FnMut(RecipeProgress) + Send,
{
    if loaded.winetricks_verbs().is_empty() {
        return Ok(None);
    }
    tasks.start("Getting winetricks");
    winetricks::ensure(paths, &mut |p| tasks.download(p))
        .await
        .map(Some)
}

/// `Z:\path\to\file` for a host path (Wine maps Z: to `/`).
fn z_path(path: &Path) -> String {
    format!("Z:{}", path.display().to_string().replace('/', "\\"))
}

fn state_path(paths: &AppPaths, bottle_id: &str) -> Result<PathBuf> {
    Ok(bottle::dir(paths, bottle_id)?.join(STATE_FILE))
}

fn read_state(paths: &AppPaths, bottle_id: &str) -> Result<StateFile> {
    let path = state_path(paths, bottle_id)?;
    let text = std::fs::read_to_string(&path).at(&path)?;
    toml::from_str(&text).map_err(|e| Error::Config {
        path,
        message: e.to_string(),
    })
}

fn write_state(paths: &AppPaths, bottle_id: &str, state: &StateFile) -> Result<()> {
    let path = state_path(paths, bottle_id)?;
    std::fs::write(&path, toml::to_string(state).expect("state serializes")).at(&path)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::Unsupported(format!("background task failed: {e}")))?
}

/// Numbers tasks and turns download progress into recipe progress.
struct Tasks<'a, P> {
    on_progress: &'a mut P,
    total: usize,
    current: usize,
    label: String,
}

impl<'a, P: FnMut(RecipeProgress) + Send> Tasks<'a, P> {
    fn new(on_progress: &'a mut P) -> Self {
        Self {
            on_progress,
            total: 0,
            current: 0,
            label: String::new(),
        }
    }

    fn start(&mut self, label: &str) {
        self.current += 1;
        self.label = label.to_string();
        self.emit(String::new(), None);
    }

    fn download(&mut self, p: InstallProgress) {
        let detail = match p.total {
            Some(t) if t > 0 => format!(
                "{}: {:.0} of {:.0} MB",
                p.stage.label(),
                p.done as f64 / 1e6,
                t as f64 / 1e6
            ),
            _ => p.stage.label().to_string(),
        };
        self.emit(detail, p.fraction().map(|f| f * 100.0));
    }

    fn emit(&mut self, detail: String, percent: Option<f64>) {
        (self.on_progress)(RecipeProgress {
            task: self.current,
            tasks: self.total.max(self.current),
            label: self.label.clone(),
            detail,
            percent,
        });
    }
}

#[cfg(test)]
mod tests;
