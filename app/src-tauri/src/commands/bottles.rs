use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use vinter_core::bottle::{self, Bottle, BottleSummary, Graphics, NewBottle, Tool, WindowsVersion};

use super::{CommandResult, blocking};
use crate::focus;
use crate::jobs::{self, JobId};
use crate::state::AppState;

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BottleStatus {
    Ready,
    /// Being created or duplicated right now.
    SettingUp,
    /// Setup was interrupted (e.g. Vinter quit). Can only be deleted.
    Incomplete,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BottleListItem {
    #[serde(flatten)]
    pub summary: BottleSummary,
    pub status: BottleStatus,
}

/// A bottle whose setup has started in the background.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct StartedBottle {
    pub bottle_id: String,
    pub job_id: JobId,
}

#[tauri::command]
#[specta::specta]
pub async fn bottles_list(state: State<'_, AppState>) -> CommandResult<Vec<BottleListItem>> {
    let paths = state.paths.clone();
    let summaries = blocking(move || Ok(bottle::list(&paths)?)).await?;
    Ok(summaries
        .into_iter()
        .map(|summary| BottleListItem {
            status: if summary.setup_complete {
                BottleStatus::Ready
            } else if state.is_busy(&summary.id) {
                BottleStatus::SettingUp
            } else {
                BottleStatus::Incomplete
            },
            summary,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn bottle_get(state: State<'_, AppState>, id: String) -> CommandResult<Bottle> {
    let paths = state.paths.clone();
    blocking(move || Ok(bottle::get(&paths, &id)?)).await
}

/// Creates a bottle. It is listed right away; setup runs as a job.
#[tauri::command]
#[specta::specta]
pub fn bottle_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    runtime_id: String,
    windows_version: WindowsVersion,
) -> CommandResult<StartedBottle> {
    let new = NewBottle {
        id: None,
        name,
        runtime: runtime_id,
        windows_version,
        graphics: Graphics::default(),
    };
    let bottle = bottle::prepare_create(&state.paths, new)?;
    state.mark_busy(&bottle.id);
    let paths = state.paths.clone();
    let bottle_id = bottle.id.clone();
    let job_id = jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            let result = bottle::finish_create(&paths, &bottle, |step| {
                job.progress(step, String::new(), None)
            });
            app.state::<AppState>().clear_busy(&bottle.id);
            result.map_err(|e| e.to_string())
        }
    });
    Ok(StartedBottle { bottle_id, job_id })
}

/// Fields to change; missing (null) ones are left alone.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct BottlePatch {
    pub name: Option<String>,
    pub runtime: Option<String>,
    /// Replaces all extra environment variables.
    pub env: Option<BTreeMap<String, String>>,
    /// Replaces all DLL overrides.
    pub dll_overrides: Option<BTreeMap<String, String>>,
}

#[tauri::command]
#[specta::specta]
pub async fn bottle_update(
    state: State<'_, AppState>,
    id: String,
    patch: BottlePatch,
) -> CommandResult<Bottle> {
    if state.is_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    blocking(move || {
        let mut bottle = bottle::get(&paths, &id)?;
        if let Some(name) = patch.name {
            bottle = bottle::rename(&paths, &id, &name)?;
        }
        if let Some(runtime) = patch.runtime {
            bottle = bottle::set_runtime(&paths, &id, &runtime)?;
        }
        if let Some(env) = patch.env {
            bottle = bottle::set_env(&paths, &id, env)?;
        }
        if let Some(overrides) = patch.dll_overrides {
            bottle = bottle::set_dll_overrides(&paths, &id, overrides)?;
        }
        Ok(bottle)
    })
    .await
}

/// Applies a Windows version (runs `winecfg /v`), as a job.
#[tauri::command]
#[specta::specta]
pub fn bottle_set_windows_version(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    version: WindowsVersion,
) -> CommandResult<JobId> {
    bottle::get(&state.paths, &id)?;
    if !state.mark_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            job.progress("Setting Windows version", String::new(), None);
            let result = bottle::set_windows_version(&paths, &id, version);
            app.state::<AppState>().clear_busy(&id);
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    }))
}

/// Copies a bottle under a new name. The copy is listed right away.
#[tauri::command]
#[specta::specta]
pub fn bottle_duplicate(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    new_name: String,
) -> CommandResult<StartedBottle> {
    if state.is_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let copy = bottle::prepare_duplicate(&state.paths, &id, &new_name)?;
    state.mark_busy(&copy.id);
    state.mark_busy(&id);
    let paths = state.paths.clone();
    let bottle_id = copy.id.clone();
    let job_id = jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            job.progress("Copying files", String::new(), None);
            let result = bottle::finish_duplicate(&paths, &id, &copy);
            let state = app.state::<AppState>();
            state.clear_busy(&copy.id);
            state.clear_busy(&id);
            result.map_err(|e| e.to_string())
        }
    });
    Ok(StartedBottle { bottle_id, job_id })
}

/// Stops the bottle's programs and moves it to the Trash.
#[tauri::command]
#[specta::specta]
pub async fn bottle_delete(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    if state.is_busy(&id) {
        return Err("This bottle is busy. Wait for it to finish first.".into());
    }
    let paths = state.paths.clone();
    blocking(move || Ok(bottle::delete(&paths, &id)?)).await
}

/// Opens the bottle's C: drive in Finder (or the file manager on Linux).
#[tauri::command]
#[specta::specta]
pub async fn bottle_open_drive_c(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let path = bottle::drive_c(&state.paths, &id)?;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)?;
    Ok(())
}

/// Opens a built-in Wine tool in the bottle and brings its window to the front.
#[tauri::command]
#[specta::specta]
pub async fn bottle_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    tool: Tool,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || {
        let bottle = bottle::get(&paths, &id)?;
        let runtime = vinter_core::runtime::get(&paths, &bottle.runtime)?;
        let watch = focus::Watch::start(&app, &runtime.path);
        bottle::run_tool(&paths, &id, tool)?;
        watch.focus_new_app();
        Ok(())
    })
    .await
}

/// Stops every Wine process in the bottle (`wineserver -k`).
#[tauri::command]
#[specta::specta]
pub async fn bottle_kill_all(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || Ok(bottle::kill_all(&paths, &id)?)).await
}
