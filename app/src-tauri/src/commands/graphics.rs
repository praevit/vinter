use tauri::{AppHandle, Manager, State};
use vinter_core::bottle;
use vinter_core::graphics::{self, Backend, BackendInfo, GraphicsReport};
use vinter_core::runtime;

use super::runtimes::report;
use super::{CommandResult, blocking};
use crate::jobs::{self, JobId};
use crate::state::AppState;

/// Graphics backends for a bottle, with availability and download info.
#[tauri::command]
#[specta::specta]
pub async fn graphics_backends(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<BackendInfo>> {
    let paths = state.paths.clone();
    blocking(move || {
        let b = bottle::get(&paths, &bottle_id)?;
        let rt = runtime::get(&paths, &b.runtime)?;
        Ok(graphics::backends(&paths, &rt))
    })
    .await
}

/// Switches a bottle's backend, downloading its component first if needed.
#[tauri::command]
#[specta::specta]
pub fn bottle_set_graphics(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    backend: Backend,
) -> CommandResult<JobId> {
    let running = state
        .runs
        .lock()
        .unwrap()
        .values()
        .any(|r| r.bottle_id == bottle_id);
    if running {
        return Err("Close this bottle's programs before switching graphics.".into());
    }
    let b = bottle::get(&state.paths, &bottle_id)?;
    let rt = runtime::get(&state.paths, &b.runtime)?;
    let info = graphics::backends(&state.paths, &rt)
        .into_iter()
        .find(|i| i.backend == backend)
        .expect("every backend is listed");
    if let Some(reason) = info.reason {
        return Err(reason.into());
    }
    if !state.mark_busy(&bottle_id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = async {
                if let Some(c) = &info.component {
                    graphics::component::ensure(&paths, c, |p| report(&job, p)).await?;
                }
                job.progress("Applying", String::new(), None);
                let paths = paths.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    graphics::apply(&paths, &bottle_id, backend)
                })
                .await
                .map_err(|e| vinter_core::Error::Unsupported(e.to_string()))??;
                Ok::<_, vinter_core::Error>(())
            }
            .await;
            app.state::<AppState>().clear_busy(&b.id);
            result.map_err(|e| e.to_string())
        }
    }))
}

/// Runs a small Direct3D 11 program in the bottle and reports what answered.
#[tauri::command]
#[specta::specta]
pub async fn bottle_test_graphics(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<GraphicsReport> {
    let paths = state.paths.clone();
    blocking(move || Ok(graphics::test(&paths, &bottle_id)?)).await
}
