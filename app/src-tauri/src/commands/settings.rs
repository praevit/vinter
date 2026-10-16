use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use vinter_core::graphics::d3dmetal::{self, D3dMetal};
use vinter_core::settings::{self, Settings};
use vinter_core::storage::{self, Usage};

use super::{CommandResult, blocking};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    /// Kept by the system (a login item), not in `settings.toml`.
    pub launch_at_login: bool,
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(app: AppHandle, state: State<'_, AppState>) -> SettingsView {
    SettingsView {
        settings: settings::load(&state.paths),
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

/// Saves the settings and applies the ones that act right away.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    let show_icon = settings.menu_bar_icon;
    blocking(move || Ok(settings::save(&paths, &settings)?)).await?;
    crate::tray::set_visible(&app, show_icon);
    Ok(())
}

/// Starts Vinter at login (in the menu bar, without its window), or stops that.
#[tauri::command]
#[specta::specta]
pub fn settings_set_launch_at_login(app: AppHandle, enabled: bool) -> CommandResult<()> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()?;
    } else {
        autolaunch.disable()?;
    }
    Ok(())
}

/// Disk usage of bottles, runtimes, graphics components and downloads.
#[tauri::command]
#[specta::specta]
pub async fn storage_usage(state: State<'_, AppState>) -> CommandResult<Usage> {
    let paths = state.paths.clone();
    blocking(move || Ok(storage::usage(&paths)?)).await
}

/// Removes leftover downloads. Returns the bytes freed.
#[tauri::command]
#[specta::specta]
pub async fn storage_clear_downloads(state: State<'_, AppState>) -> CommandResult<u64> {
    if !state.installing.lock().unwrap().is_empty()
        || !state.busy_bottles.lock().unwrap().is_empty()
    {
        return Err("Something is downloading or being set up. Try again when it is done.".into());
    }
    let paths = state.paths.clone();
    blocking(move || Ok(storage::clear_downloads(&paths)?)).await
}

/// Size of one bottle in bytes.
#[tauri::command]
#[specta::specta]
pub async fn bottle_disk_usage(state: State<'_, AppState>, id: String) -> CommandResult<u64> {
    let paths = state.paths.clone();
    blocking(move || Ok(storage::dir_size(&vinter_core::bottle::dir(&paths, &id)?))).await
}

/// Removes a bottle's old logs and temp files. Returns the bytes freed.
#[tauri::command]
#[specta::specta]
pub async fn bottle_clean(state: State<'_, AppState>, id: String) -> CommandResult<u64> {
    let running = state
        .runs
        .lock()
        .unwrap()
        .values()
        .any(|r| r.bottle_id == id);
    if running || state.is_busy(&id) {
        return Err("Close this bottle's programs first.".into());
    }
    let paths = state.paths.clone();
    blocking(move || Ok(storage::clean_bottle(&paths, &id)?)).await
}

/// The imported copy of Apple's D3DMetal, if any.
#[tauri::command]
#[specta::specta]
pub fn d3dmetal_status(state: State<'_, AppState>) -> Option<D3dMetal> {
    d3dmetal::installed(&state.paths)
}

/// A Game Porting Toolkit download the user picked, with Apple's license.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct D3dMetalSource {
    pub path: String,
    pub version: String,
    pub license: String,
}

/// Lets the user pick the toolkit (.dmg) and reads its version and license.
/// `None` if the dialog was cancelled.
#[tauri::command]
#[specta::specta]
pub async fn d3dmetal_choose(app: AppHandle) -> CommandResult<Option<D3dMetalSource>> {
    blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Choose Apple's Game Porting Toolkit")
            .add_filter("Disk images", &["dmg"])
            .blocking_pick_file()
            .and_then(|p| p.into_path().ok());
        let Some(path) = picked else {
            return Ok(None);
        };
        let found = d3dmetal::inspect(&path)?;
        Ok(Some(D3dMetalSource {
            path: path.display().to_string(),
            version: found.version,
            license: found.license,
        }))
    })
    .await
}

/// Imports D3DMetal from a toolkit the user chose and agreed to.
#[tauri::command]
#[specta::specta]
pub async fn d3dmetal_import(state: State<'_, AppState>, path: String) -> CommandResult<D3dMetal> {
    let paths = state.paths.clone();
    blocking(move || Ok(d3dmetal::import(&paths, std::path::Path::new(&path))?)).await
}

/// Removes the imported copy.
#[tauri::command]
#[specta::specta]
pub async fn d3dmetal_remove(state: State<'_, AppState>) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || Ok(d3dmetal::remove(&paths)?)).await
}
