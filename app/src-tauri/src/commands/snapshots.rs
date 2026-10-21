use tauri::{AppHandle, State};
use vinter_core::bottle::{Bottle, snapshot};

use super::{CommandResult, blocking};
use crate::state::AppState;

/// A bottle's snapshots, newest first.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_list(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Vec<snapshot::Snapshot>> {
    let paths = state.paths.clone();
    blocking(move || Ok(snapshot::list(&paths, &id)?)).await
}

#[tauri::command]
#[specta::specta]
pub async fn snapshot_create(
    state: State<'_, AppState>,
    id: String,
    label: String,
) -> CommandResult<snapshot::Snapshot> {
    if state.is_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    blocking(move || Ok(snapshot::create(&paths, &id, &label, false)?)).await
}

/// Stops the bottle's programs and puts it back as it was in the snapshot.
#[tauri::command]
#[specta::specta]
pub async fn snapshot_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    snapshot_id: String,
) -> CommandResult<Bottle> {
    if state.is_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    blocking(move || {
        let b = snapshot::restore(&paths, &id, &snapshot_id)?;
        // The snapshot's programs may be pinned differently.
        vinter_core::launcher::sync_bottle(&paths, &id, &crate::startup::launcher_target()?)?;
        crate::tray::refresh(&app);
        Ok(b)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn snapshot_delete(
    state: State<'_, AppState>,
    id: String,
    snapshot_id: String,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || Ok(snapshot::delete(&paths, &id, &snapshot_id)?)).await
}
