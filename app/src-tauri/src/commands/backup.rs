use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use vinter_core::bottle::{self, backup};

use super::CommandResult;
use crate::jobs::{self, JobId};
use crate::state::AppState;

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1e6)
}

/// Asks where to save, then writes a backup of the bottle as a job.
/// `None` if the save dialog was cancelled.
#[tauri::command]
#[specta::specta]
pub async fn bottle_export(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Option<JobId>> {
    let b = bottle::get(&state.paths, &id)?;
    if state
        .runs
        .lock()
        .unwrap()
        .values()
        .any(|r| r.bottle_id == id)
    {
        return Err("Close this bottle's programs first, so the backup is complete.".into());
    }
    let dialog_app = app.clone();
    let name = format!("{}.{}", b.name, backup::EXTENSION);
    let dest = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Back up bottle")
            .set_file_name(name)
            .add_filter("Vinter bottle backup", &[backup::EXTENSION])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok())
    })
    .await?;
    let Some(mut dest) = dest else {
        return Ok(None);
    };
    if dest.extension().is_none_or(|e| e != backup::EXTENSION) {
        dest.set_extension(backup::EXTENSION);
    }
    if !state.mark_busy(&id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(Some(jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            let result = backup::export(&paths, &id, &dest, |done, total| {
                let percent = (total > 0).then(|| done as f64 / total as f64 * 100.0);
                job.progress(
                    "Backing up",
                    format!("{} of {}", mb(done), mb(total)),
                    percent,
                );
            });
            app.state::<AppState>().clear_busy(&id);
            match &result {
                Ok(()) => crate::notify::send(
                    &app,
                    &format!("{} is backed up", b.name),
                    &dest.display().to_string(),
                ),
                Err(e) => crate::notify::send(&app, "The backup did not finish", &e.to_string()),
            }
            result.map_err(|e| e.to_string())
        }
    })))
}

/// Restores a backup as a new bottle, as a job. Without `path`, asks for
/// the file. `None` if the dialog was cancelled.
#[tauri::command]
#[specta::specta]
pub async fn bottle_import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> CommandResult<Option<JobId>> {
    let path = match path {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            let dialog_app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                dialog_app
                    .dialog()
                    .file()
                    .set_title("Restore a bottle")
                    .add_filter("Vinter bottle backup", &[backup::EXTENSION])
                    .blocking_pick_file()
                    .and_then(|p| p.into_path().ok())
            })
            .await?
        }
    };
    let Some(path) = path else {
        return Ok(None);
    };
    let paths = state.paths.clone();
    Ok(Some(jobs::spawn_blocking(&app, {
        let app = app.clone();
        move |job| {
            let result = backup::import(&paths, &path, |done, total| {
                let percent = (total > 0).then(|| done as f64 / total as f64 * 100.0);
                job.progress("Restoring", String::new(), percent);
            });
            match &result {
                Ok(b) => {
                    // Pinned programs get their launchers on this computer.
                    if let Ok(target) = crate::startup::launcher_target() {
                        let _ = vinter_core::launcher::sync_bottle(&paths, &b.id, &target);
                    }
                    crate::tray::refresh(&app);
                    crate::notify::send(
                        &app,
                        &format!("{} is restored", b.name),
                        "It is in the sidebar.",
                    );
                }
                Err(e) => {
                    crate::notify::send(&app, "The bottle could not be restored", &e.to_string())
                }
            }
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    })))
}
