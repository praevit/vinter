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

/// ARM64 builds of a program found next to it (experimental `per-app-engine`).
#[tauri::command]
#[specta::specta]
pub async fn program_arm64_builds(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
) -> CommandResult<Vec<String>> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(vinter_core::programs::arm64_builds(
            &paths,
            &bottle_id,
            index as usize,
        )?)
    })
    .await
}

/// Launches a program's ARM64 build instead (or, with `None`, the program
/// as installed again).
#[tauri::command]
#[specta::specta]
pub async fn program_set_arm64_build(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
    build: Option<String>,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || {
        vinter_core::programs::set_arm64_build(&paths, &bottle_id, index as usize, build)?;
        Ok(())
    })
    .await
}

/// FEX's settings for a bottle's runtime, from FEX's own schema
/// (experimental `fex-config-editor`).
#[tauri::command]
#[specta::specta]
pub async fn fex_options(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<Vec<vinter_core::fex_config::FexOption>> {
    let paths = state.paths.clone();
    blocking(move || {
        let b = vinter_core::bottle::get(&paths, &bottle_id)?;
        let rt = vinter_core::runtime::get(&paths, &b.runtime)?;
        Ok(vinter_core::fex_config::options_for(&rt)?)
    })
    .await
}

/// A program's own FEX settings.
#[tauri::command]
#[specta::specta]
pub async fn fex_program_settings(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(vinter_core::fex_config::program_settings(
            &paths,
            &bottle_id,
            index as usize,
        )?)
    })
    .await
}

/// Sets (or with `None`, clears) one of a program's FEX settings.
#[tauri::command]
#[specta::specta]
pub async fn fex_setting_set(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
    name: String,
    value: Option<String>,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    let paths = state.paths.clone();
    blocking(move || {
        Ok(vinter_core::fex_config::set(
            &paths,
            &bottle_id,
            index as usize,
            &name,
            value.as_deref(),
        )?)
    })
    .await
}

/// A launcher fix that fits a program, and whether it is on.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ShimView {
    #[serde(flatten)]
    pub shim: vinter_core::shims::Shim,
    pub on: bool,
}

/// Launcher fixes for a program (experimental `launcher-shims`).
#[tauri::command]
#[specta::specta]
pub async fn program_shims(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
) -> CommandResult<Vec<ShimView>> {
    let paths = state.paths.clone();
    blocking(move || {
        let b = vinter_core::bottle::get(&paths, &bottle_id)?;
        let program = b
            .programs
            .get(index as usize)
            .ok_or(vinter_core::Error::ProgramNotFound(index as usize))?;
        let on = vinter_core::shims::turned_on(&paths, &bottle_id, index as usize)?;
        Ok(vinter_core::shims::for_program(program)
            .into_iter()
            .map(|shim| ShimView {
                on: on.iter().any(|i| i == shim.id),
                shim,
            })
            .collect())
    })
    .await
}

/// Turns a launcher fix on or off for a program.
#[tauri::command]
#[specta::specta]
pub async fn program_shim_set(
    state: State<'_, AppState>,
    bottle_id: String,
    index: u32,
    id: String,
    on: bool,
) -> CommandResult<()> {
    let paths = state.paths.clone();
    blocking(move || {
        vinter_core::shims::set(&paths, &bottle_id, index as usize, &id, on)?;
        Ok(())
    })
    .await
}

/// Opens a launcher fix's documentation. Only the fixed URLs Vinter ships.
#[tauri::command]
#[specta::specta]
pub fn shim_open_source(app: AppHandle, id: String) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let shim = vinter_core::shims::all()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| vinter_core::Error::InvalidSetting(format!("no fix {id}")))?;
    app.opener().open_url(shim.source, None::<&str>)?;
    Ok(())
}
