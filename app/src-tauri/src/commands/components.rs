use tauri::{AppHandle, Manager, State};
use vinter_core::components::{self, WindowsComponent};
use vinter_core::recipe::winetricks;

use super::runtimes::report;
use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// Windows components that can be installed, marked installed or not.
#[tauri::command]
#[specta::specta]
pub async fn components_list(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<WindowsComponent>> {
    let paths = state.paths.clone();
    blocking(move || Ok(components::list(&paths, &bottle_id)?)).await
}

/// Installs components with winetricks. `accepted`: the user agreed to
/// downloads from Microsoft and others.
#[tauri::command]
#[specta::specta]
pub fn components_install(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    verbs: Vec<String>,
    accepted: bool,
) -> CommandResult<JobId> {
    if !state.mark_busy(&bottle_id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = async {
                if accepted {
                    winetricks::ensure(&paths, &mut |p| report(&job, p)).await?;
                }
                // Can take many minutes (.NET); winetricks reports no progress.
                job.progress("Installing", verbs.join(", "), None);
                components::install(&paths, &bottle_id, &verbs, accepted, |_| {}).await
            }
            .await;
            app.state::<AppState>().clear_busy(&bottle_id);
            result.map_err(|e| e.to_string())
        }
    }))
}
