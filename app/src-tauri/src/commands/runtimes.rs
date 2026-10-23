use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use vinter_core::runtime::updates::RuntimeUpdate;
use vinter_core::runtime::{self, InstallProgress, InstallStage, Runtime, RuntimeInfo};

use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// Runtimes from the manifest that run on this machine.
#[tauri::command]
#[specta::specta]
pub fn runtimes_available() -> Vec<RuntimeInfo> {
    runtime::manifest::available()
}

/// Id of the runtime new bottles use unless the user picks another.
#[tauri::command]
#[specta::specta]
pub async fn runtime_default(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    let paths = state.paths.clone();
    blocking(move || Ok(runtime::default_for_new_bottles(&paths)?.map(|r| r.id))).await
}

/// Installed runtimes, with their disk usage.
#[tauri::command]
#[specta::specta]
pub async fn runtimes_installed(state: State<'_, AppState>) -> CommandResult<Vec<Runtime>> {
    let paths = state.paths.clone();
    blocking(move || Ok(runtime::installed(&paths)?)).await
}

/// Starts downloading and installing a runtime. Progress comes as job events.
#[tauri::command]
#[specta::specta]
pub fn runtime_install(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<JobId> {
    let info = runtime::manifest::find(&id)?;
    if runtime::get(&state.paths, &id).is_ok() {
        return Err(format!("{} is already installed.", info.name).into());
    }
    if !state.installing.lock().unwrap().insert(id.clone()) {
        return Err(format!("{} is already being installed.", info.name).into());
    }

    let paths = state.paths.clone();
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = runtime::install(&paths, &info, |p| report(&job, p)).await;
            app.state::<AppState>()
                .installing
                .lock()
                .unwrap()
                .remove(&info.id);
            match &result {
                Ok(_) => crate::notify::send(
                    &app,
                    &format!("{} is installed", info.name),
                    "You can create bottles with it now.",
                ),
                Err(e) => crate::notify::send(
                    &app,
                    &format!("Could not install {}", info.name),
                    &e.to_string(),
                ),
            }
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }))
}

pub(crate) fn report(job: &jobs::JobReporter, p: InstallProgress) {
    let message = match (p.stage, p.total) {
        (InstallStage::Finishing, _) => String::new(),
        (_, Some(total)) => format!("{} of {}", mb(p.done), mb(total)),
        (_, None) => mb(p.done),
    };
    job.progress(p.stage.label(), message, p.fraction().map(|f| f * 100.0));
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}

/// Deletes an installed runtime.
#[tauri::command]
#[specta::specta]
pub async fn runtime_remove(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    if state.is_installing(&id) {
        return Err("This runtime is still being installed.".into());
    }
    let paths = state.paths.clone();
    blocking(move || Ok(runtime::remove(&paths, &id)?)).await
}

#[derive(Debug, Clone, Copy, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLink {
    /// The Wine source code the build was made from.
    Source,
    /// The upstream release page of the build.
    Release,
}

/// Opens a runtime's source or release page in the browser. Only URLs from
/// the manifest can be opened, never one sent by the frontend.
#[tauri::command]
#[specta::specta]
pub fn runtime_open_link(app: AppHandle, id: String, link: RuntimeLink) -> CommandResult<()> {
    let info = runtime::manifest::find(&id)?;
    let url = match link {
        RuntimeLink::Source => info.source_url,
        RuntimeLink::Release => info.release_url,
    };
    app.opener().open_url(url, None::<&str>)?;
    Ok(())
}

/// Newer builds of installed runtimes.
#[tauri::command]
#[specta::specta]
pub async fn runtime_updates(state: State<'_, AppState>) -> CommandResult<Vec<RuntimeUpdate>> {
    let paths = state.paths.clone();
    super::blocking(move || Ok(runtime::updates::find(&paths)?)).await
}

/// Installs the newer build of an installed runtime and moves its bottles
/// onto it, as a job. The old runtime stays installed until removed.
#[tauri::command]
#[specta::specta]
pub fn runtime_update(
    app: AppHandle,
    state: State<'_, AppState>,
    installed_id: String,
) -> CommandResult<JobId> {
    let update = runtime::updates::find(&state.paths)?
        .into_iter()
        .find(|u| u.installed_id == installed_id)
        .ok_or("There is no newer version of this runtime.")?;
    let info = update.newer;
    if !state.installing.lock().unwrap().insert(info.id.clone()) {
        return Err(format!("{} is already being installed.", info.name).into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = async {
                runtime::install(&paths, &info, |p| report(&job, p)).await?;
                job.progress("Moving bottles", String::new(), None);
                runtime::updates::move_bottles(&paths, &installed_id, &info.id)
            }
            .await;
            app.state::<AppState>()
                .installing
                .lock()
                .unwrap()
                .remove(&info.id);
            match &result {
                Ok(n) => crate::notify::send(
                    &app,
                    &format!("{} is installed", info.name),
                    &format!(
                        "{n} bottle(s) now use it. You can remove the old version on the Runtimes page."
                    ),
                ),
                Err(e) => crate::notify::send(
                    &app,
                    &format!("Could not update to {}", info.name),
                    &e.to_string(),
                ),
            }
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }))
}
