use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;
use vinter_core::bottle::{self, Bottle, ProgramDef};
use vinter_core::process::logs::{self, LogEntry};
use vinter_core::programs::{self, DiscoveredProgram};
use vinter_core::runtime;

use super::{CommandResult, blocking};
use crate::events::{RunExit, RunLog};
use crate::focus;
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// A program started from Vinter that is still running.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RunInfo {
    pub run_id: String,
    pub bottle_id: String,
    pub program_index: u32,
    pub name: String,
    pub pid: u32,
    /// RFC 3339 start time.
    pub started_at: String,
    /// Log file of this run, for `log_read`.
    pub log_id: String,
}

/// A bottle's program as shown on its tile.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProgramView {
    pub index: u32,
    pub name: String,
    pub exe: String,
    pub icon_data_url: Option<String>,
    /// Has a launcher (Spotlight/Launchpad on macOS, app menu on Linux).
    pub pinned: bool,
}

/// Lets the user pick an installer (.exe or .msi) with the system file dialog.
/// This is the only way a file path reaches the backend from the UI.
#[tauri::command]
#[specta::specta]
pub async fn installer_choose(app: AppHandle) -> CommandResult<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a Windows installer")
            .add_filter("Windows installers", &["exe", "msi", "EXE", "MSI"])
            .blocking_pick_file()
    })
    .await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

/// Runs an installer in a bottle, as a job that ends when the installer exits.
#[tauri::command]
#[specta::specta]
pub fn installer_run(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    path: String,
    args: Vec<String>,
) -> CommandResult<JobId> {
    let bottle = bottle::get(&state.paths, &bottle_id)?;
    let runtime = runtime::get(&state.paths, &bottle.runtime)?;
    if !state.mark_busy(&bottle_id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            job.progress(
                "Installer running",
                "Follow the steps in the installer's window.".into(),
                None,
            );
            let watch = focus::Watch::start(&app, &runtime.path);
            let result = programs::run_installer(
                &paths,
                &bottle_id,
                std::path::Path::new(&path),
                &args,
                |_| watch.focus_new_app(),
            );
            app.state::<AppState>().clear_busy(&bottle_id);
            // A non-zero exit often just means "cancelled"; the UI looks for programs either way.
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }))
}

/// Looks for installed programs in a bottle (and extracts their icons).
#[tauri::command]
#[specta::specta]
pub async fn programs_discover(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<DiscoveredProgram>> {
    let paths = state.paths.clone();
    blocking(move || Ok(programs::discover(&paths, &bottle_id)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn programs_add(
    state: State<'_, AppState>,
    bottle_id: String,
    programs: Vec<ProgramDef>,
) -> CommandResult<Bottle> {
    let paths = state.paths.clone();
    blocking(move || Ok(programs::add(&paths, &bottle_id, programs)?)).await
}

/// Removes a program's tile. The program stays installed in the bottle.
#[tauri::command]
#[specta::specta]
pub async fn program_remove(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
) -> CommandResult<Bottle> {
    let paths = state.paths.clone();
    blocking(move || {
        let bottle = programs::remove(&paths, &bottle_id, index as usize)?;
        // Launchers point at program indexes, which just shifted.
        vinter_core::launcher::sync_bottle(
            &paths,
            &bottle_id,
            &crate::startup::launcher_target()?,
        )?;
        Ok(bottle)
    })
    .await
}

/// A bottle's programs, with icons as `data:` URLs.
#[tauri::command]
#[specta::specta]
pub async fn programs_list(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<ProgramView>> {
    let paths = state.paths.clone();
    blocking(move || {
        let bottle = bottle::get(&paths, &bottle_id)?;
        Ok(bottle
            .programs
            .iter()
            .enumerate()
            .map(|(i, p)| ProgramView {
                index: i as u32,
                name: p.name.clone(),
                exe: p.exe.clone(),
                pinned: p.pinned,
                icon_data_url: p
                    .icon
                    .as_deref()
                    .and_then(|icon| programs::icon_data_url(&paths, &bottle_id, icon)),
            })
            .collect())
    })
    .await
}

/// Starts a program. Output arrives as `run://log`, the end as `run://exit`.
#[tauri::command]
#[specta::specta]
pub async fn program_launch(
    app: AppHandle,
    bottle_id: String,
    program_index: u32,
) -> CommandResult<RunInfo> {
    tauri::async_runtime::spawn_blocking(move || launch_program(&app, bottle_id, program_index))
        .await?
}

/// Starts a program and tracks it (shared by the UI, deep links and `--run`).
/// Blocks briefly while the process starts.
pub(crate) fn launch_program(
    app: &AppHandle,
    bottle_id: String,
    program_index: u32,
) -> CommandResult<RunInfo> {
    let paths = app.state::<AppState>().paths.clone();
    let bottle = bottle::get(&paths, &bottle_id)?;
    let runtime = runtime::get(&paths, &bottle.runtime)?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let watch = focus::Watch::start(app, &runtime.path);

    let (log_app, exit_app) = (app.clone(), app.clone());
    let (log_run, exit_run) = (run_id.clone(), run_id.clone());
    let launched = programs::launch(
        &paths,
        &bottle_id,
        program_index as usize,
        move |stream, line| {
            let _ = RunLog {
                run_id: log_run.clone(),
                line: line.to_string(),
                stream: stream.into(),
            }
            .emit(&log_app);
        },
        move |code| {
            let state = exit_app.state::<AppState>();
            let none_left = {
                let mut runs = state.runs.lock().unwrap();
                runs.remove(&exit_run);
                runs.is_empty()
            };
            let _ = RunExit {
                run_id: exit_run,
                code,
            }
            .emit(&exit_app);
            // Started in the background just for this program: quit with it.
            if none_left && crate::startup::was_headless(&exit_app) {
                exit_app.exit(0);
            }
        },
    )?;
    watch.focus_new_app();

    let info = RunInfo {
        run_id: run_id.clone(),
        bottle_id,
        program_index,
        name: launched.name,
        pid: launched.pid,
        started_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        log_id: launched.log_id,
    };
    app.state::<AppState>()
        .runs
        .lock()
        .unwrap()
        .insert(run_id, info.clone());
    Ok(info)
}

/// Stops a running program. Other programs in the bottle keep running.
#[tauri::command]
#[specta::specta]
pub fn program_stop(state: State<'_, AppState>, run_id: String) -> CommandResult<()> {
    let pid = state.runs.lock().unwrap().get(&run_id).map(|r| r.pid);
    match pid {
        Some(pid) => Ok(vinter_core::process::terminate(pid)?),
        // Already exited.
        None => Ok(()),
    }
}

/// Programs started from Vinter that are still running.
#[tauri::command]
#[specta::specta]
pub fn runs_list(state: State<'_, AppState>) -> Vec<RunInfo> {
    let mut runs: Vec<RunInfo> = state.runs.lock().unwrap().values().cloned().collect();
    runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    runs
}

#[tauri::command]
#[specta::specta]
pub async fn logs_list(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<LogEntry>> {
    let paths = state.paths.clone();
    blocking(move || Ok(logs::list(&bottle::dir(&paths, &bottle_id)?)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn log_read(
    state: State<'_, AppState>,
    bottle_id: String,
    log_id: String,
) -> CommandResult<String> {
    let paths = state.paths.clone();
    blocking(move || Ok(logs::read(&bottle::dir(&paths, &bottle_id)?, &log_id)?)).await
}
