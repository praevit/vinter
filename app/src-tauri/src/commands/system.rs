use tauri::{AppHandle, State};
use vinter_core::system::{self, SystemStatus, rosetta};

use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// Checks whether this machine is ready to run Windows programs.
#[tauri::command]
#[specta::specta]
pub async fn system_checks(state: State<'_, AppState>) -> CommandResult<SystemStatus> {
    // The Rosetta check spawns a process, so keep it off the async workers.
    let paths = state.paths.clone();
    blocking(move || Ok(system::system_checks(&paths))).await
}

/// Installs Rosetta 2. The user clicking "Install Rosetta" is their consent
/// to Apple's license. macOS may show an administrator password prompt.
#[tauri::command]
#[specta::specta]
pub fn rosetta_install(app: AppHandle) -> CommandResult<JobId> {
    if !rosetta::is_needed() {
        return Err("Rosetta 2 is only needed on Apple Silicon Macs.".into());
    }
    Ok(jobs::spawn(&app, |job| async move {
        job.progress(
            "Installing Rosetta 2",
            "This can take a minute.".into(),
            None,
        );
        tauri::async_runtime::spawn_blocking(rosetta::install)
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }))
}
