use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use vinter_core::settings::{self, Settings};

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
