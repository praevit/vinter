use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AppInfo {
    pub version: String,
    /// "macos" or "linux".
    pub os: String,
    pub data_dir: String,
    /// Where pinned programs' launchers go.
    pub launchers_dir: String,
}

#[tauri::command]
#[specta::specta]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.into(),
        data_dir: state.paths.data_dir().display().to_string(),
        launchers_dir: crate::startup::launcher_target()
            .map(|t| t.dir.display().to_string())
            .unwrap_or_default(),
    }
}

/// Fixed places the About screen can open.
#[derive(Debug, Clone, Copy, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AppLink {
    Website,
    Source,
    Issues,
    /// GitHub Sponsors, through getvinter.app/support so the target can change.
    Support,
    DataFolder,
    LaunchersFolder,
    WineLicense,
}

#[tauri::command]
#[specta::specta]
pub fn app_open_link(
    app: AppHandle,
    state: State<'_, AppState>,
    link: AppLink,
) -> CommandResult<()> {
    let url = match link {
        AppLink::Website => "https://getvinter.app",
        AppLink::Source => "https://getvinter.app/github",
        AppLink::Issues => "https://getvinter.app/issues",
        AppLink::Support => "https://getvinter.app/support",
        AppLink::WineLicense => "https://www.winehq.org/license",
        AppLink::DataFolder | AppLink::LaunchersFolder => {
            let dir = match link {
                AppLink::DataFolder => state.paths.data_dir().to_path_buf(),
                _ => crate::startup::launcher_target()?.dir,
            };
            std::fs::create_dir_all(&dir)?;
            app.opener()
                .open_path(dir.display().to_string(), None::<&str>)?;
            return Ok(());
        }
    };
    app.opener().open_url(url, None::<&str>)?;
    Ok(())
}

/// Text for "Copy debug report".
#[tauri::command]
#[specta::specta]
pub async fn debug_report(state: State<'_, AppState>, bottle_id: String) -> CommandResult<String> {
    let paths = state.paths.clone();
    blocking(move || Ok(vinter_core::report::debug_report(&paths, &bottle_id)?)).await
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

/// Asks getvinter.app for a newer version.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> CommandResult<Option<UpdateInfo>> {
    let update = app.updater()?.check().await?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Downloads and installs the newer version, then restarts Vinter.
#[tauri::command]
#[specta::specta]
pub fn update_install(app: AppHandle) -> CommandResult<JobId> {
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = async {
                let update = app
                    .updater()?
                    .check()
                    .await?
                    .ok_or(tauri_plugin_updater::Error::ReleaseNotFound)?;
                let mut done = 0usize;
                update
                    .download_and_install(
                        |chunk, total| {
                            done += chunk;
                            let percent = total.map(|t| done as f64 / t as f64 * 100.0);
                            job.progress("Downloading the update", String::new(), percent);
                        },
                        || job.progress("Installing the update", String::new(), None),
                    )
                    .await?;
                Ok::<_, tauri_plugin_updater::Error>(())
            }
            .await;
            match result {
                Ok(()) => app.restart(),
                Err(e) => Err(e.to_string()),
            }
        }
    }))
}

/// Pins or unpins a program: pinned programs get a launcher
/// (Spotlight/Launchpad on macOS, the app menu on Linux).
#[tauri::command]
#[specta::specta]
pub async fn program_set_pinned(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
    pinned: bool,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || {
        let target = crate::startup::launcher_target()?;
        vinter_core::launcher::set_pinned(&paths, &bottle_id, index as usize, pinned, &target)?;
        Ok(())
    })
    .await
}
