//! Compatibility notes and engine comparisons (experimental flags).

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use vinter_core::compat::{self, CompatNote, Rating};

use super::{CommandResult, blocking};
use crate::state::AppState;

/// Every note. Fails while the flag is off.
#[tauri::command]
#[specta::specta]
pub async fn compat_notes_list(state: State<'_, AppState>) -> CommandResult<Vec<CompatNote>> {
    let paths = state.paths.clone();
    blocking(move || Ok(compat::list(&paths)?)).await
}

/// Rates a bottle's program, with the bottle's engine and backend.
#[tauri::command]
#[specta::specta]
pub async fn compat_note_set(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
    rating: Rating,
    note: String,
) -> CommandResult<CompatNote> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(compat::set(
            &paths,
            &bottle_id,
            index as usize,
            rating,
            &note,
        )?)
    })
    .await
}

/// Removes a program's note.
#[tauri::command]
#[specta::specta]
pub async fn compat_note_remove(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || Ok(compat::remove(&paths, &bottle_id, index as usize)?)).await
}

/// Asks where to save, then writes every note as JSON. `None` if cancelled.
#[tauri::command]
#[specta::specta]
pub async fn compat_notes_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let paths = state.paths.clone();
    blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Export compatibility notes")
            .set_file_name("vinter-compatibility-notes.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
            .and_then(|p| p.into_path().ok());
        let Some(dest) = picked else {
            return Ok(None);
        };
        compat::export(&paths, &dest)?;
        Ok(Some(dest.display().to_string()))
    })
    .await
}

/// Runs a program in two bottles and compares them (experimental
/// `engine-ab-compare`). Takes up to twice `seconds`.
#[tauri::command]
#[specta::specta]
pub async fn compare_run(
    state: State<'_, AppState>,
    bottle_a: String,
    program_a: u32,
    bottle_b: String,
    program_b: u32,
    seconds: u32,
) -> CommandResult<vinter_core::compare::Comparison> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(vinter_core::compare::compare(
            &paths,
            (&bottle_a, program_a as usize),
            (&bottle_b, program_b as usize),
            std::time::Duration::from_secs(u64::from(seconds.clamp(5, 120))),
        )?)
    })
    .await
}
