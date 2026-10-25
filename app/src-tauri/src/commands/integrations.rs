//! The `vinter` command and AI agents, automatic backups, saving a bottle as
//! a recipe, and a bottle's settings history.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use vinter_core::auto_backup::{self, AutoBackupSettings, BottleBackup};
use vinter_core::bottle::history;
use vinter_core::cli_install::{self, CliStatus};
use vinter_core::recipe::save::{self, SaveOptions, SavedRecipe};

use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// The `vinter` command inside the app. Tauri puts it next to the app's own
/// program (`Contents/MacOS` on macOS, `/usr/bin` in the Linux packages).
fn bundled_cli() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    // In development, cargo builds the real command right there.
    #[cfg(debug_assertions)]
    if dir.join(cli_install::COMMAND).is_file() {
        return Some(dir.join(cli_install::COMMAND));
    }
    Some(dir.join("vinter-cli"))
}

fn home() -> CommandResult<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "Could not find your home folder.".into())
}

/// An AppImage's files move on every start, so it installs a copy.
fn is_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

fn cli_status_now() -> CommandResult<CliStatus> {
    Ok(cli_install::status(
        bundled_cli().as_deref(),
        &home()?,
        &cli_install::terminal_path(),
    ))
}

/// Whether the `vinter` command is installed, and where it would go.
#[tauri::command]
#[specta::specta]
pub async fn cli_status() -> CommandResult<CliStatus> {
    blocking(cli_status_now).await
}

/// Puts `vinter` on the PATH: a link to the app's copy.
#[tauri::command]
#[specta::specta]
pub async fn cli_install() -> CommandResult<CliStatus> {
    blocking(|| {
        let bundled =
            bundled_cli().ok_or("This build of Vinter does not include the vinter command.")?;
        cli_install::install(&bundled, &cli_install::install_dir(&home()?), is_appimage())?;
        cli_status_now()
    })
    .await
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AutoBackupView {
    pub settings: AutoBackupSettings,
    pub bottles: Vec<BottleBackup>,
    /// Backups are being made right now.
    pub running: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn auto_backup_get(state: State<'_, AppState>) -> CommandResult<AutoBackupView> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(AutoBackupView {
            settings: auto_backup::load(&paths),
            bottles: auto_backup::status(&paths)?,
            running: crate::backups::is_running(),
        })
    })
    .await
}

/// Saves the automatic backup settings; turning them on checks right away.
#[tauri::command]
#[specta::specta]
pub async fn auto_backup_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AutoBackupSettings,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    let enabled = settings.enabled;
    blocking(move || Ok(auto_backup::save(&paths, &settings)?)).await?;
    if enabled {
        crate::backups::run_due(&app, false);
    }
    Ok(())
}

/// Asks for the folder backups go to. `None` if cancelled.
#[tauri::command]
#[specta::specta]
pub async fn auto_backup_choose_folder(app: AppHandle) -> CommandResult<Option<String>> {
    let folder = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Folder for bottle backups")
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
    })
    .await?;
    Ok(folder.map(|p| p.display().to_string()))
}

/// Backs up every bottle now, in the background. False if backups are
/// already running.
#[tauri::command]
#[specta::specta]
pub fn auto_backup_run_now(app: AppHandle, state: State<'_, AppState>) -> CommandResult<bool> {
    if auto_backup::load(&state.paths).folder.is_none() {
        return Err("Choose a folder for the backups first.".into());
    }
    Ok(crate::backups::run_due(&app, true))
}

/// Writes a recipe made from the bottle into the recipes folder.
#[tauri::command]
#[specta::specta]
pub async fn bottle_save_recipe(
    state: State<'_, AppState>,
    id: String,
    name: String,
    description: String,
    installer_hint: String,
) -> CommandResult<SavedRecipe> {
    let paths = state.paths.clone();
    blocking(move || {
        let opts = SaveOptions {
            name,
            description,
            installer_hint,
        };
        Ok(save::save(&paths, &id, &opts)?)
    })
    .await
}

/// Opens a new GitHub issue in the browser with the recipe and the bottle's
/// compatibility notes filled in. Nothing is sent until the user submits it.
#[tauri::command]
#[specta::specta]
pub async fn recipe_share(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    name: String,
    toml: String,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    let notes = blocking(move || Ok(vinter_core::compat::for_bottle(&paths, &bottle_id)?)).await?;
    let version = app.package_info().version.to_string();
    let url = save::share_url(&name, &toml, &notes, &version);
    app.opener().open_url(url, None::<&str>)?;
    Ok(())
}

/// A bottle's settings changes, newest first.
#[tauri::command]
#[specta::specta]
pub async fn bottle_history(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Vec<history::HistoryItem>> {
    let paths = state.paths.clone();
    blocking(move || Ok(history::list(&paths, &id)?)).await
}

/// Puts a setting back as it was before a change. Changes that run Wine
/// (Windows version, display options, graphics) run as a job, whose id is
/// returned; the others are done when this returns `None`.
#[tauri::command]
#[specta::specta]
pub async fn bottle_history_revert(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    entry_id: String,
) -> CommandResult<Option<JobId>> {
    let entry = history::find(&state.paths, &id, &entry_id)?;
    if state
        .runs
        .lock()
        .unwrap()
        .values()
        .any(|r| r.bottle_id == id)
    {
        return Err("Close this bottle's programs first.".into());
    }
    if !state.mark_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    if !entry.change.is_slow() {
        let result = blocking({
            let id = id.clone();
            move || Ok(history::revert(&paths, &id, &entry_id)?)
        })
        .await;
        state.clear_busy(&id);
        return result.map(|()| None);
    }
    Ok(Some(jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            job.progress("Reverting", String::new(), None);
            let result = history::revert(&paths, &id, &entry_id);
            app.state::<AppState>().clear_busy(&id);
            result.map_err(|e| e.to_string())
        }
    })))
}
