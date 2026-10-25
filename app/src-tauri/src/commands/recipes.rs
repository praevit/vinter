use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use vinter_core::bottle::Bottle;
use vinter_core::recipe::{
    self, ApplyOptions, BrokenRecipe, RecipeProgress, RecipeState, RecipeSummary,
};

use super::bottles::StartedBottle;
use super::{CommandResult, blocking};
use crate::jobs::{self, JobId, JobReporter};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RecipeList {
    pub recipes: Vec<RecipeSummary>,
    /// User recipe files that could not be loaded, with the reason.
    pub broken: Vec<BrokenRecipe>,
}

#[tauri::command]
#[specta::specta]
pub fn recipes_list(state: State<'_, AppState>) -> RecipeList {
    let (recipes, broken) = recipe::list(&state.paths);
    RecipeList {
        recipes: recipes.iter().map(|r| r.summary()).collect(),
        broken,
    }
}

pub(crate) fn report(job: &JobReporter, p: RecipeProgress) {
    let mut message = format!("Step {} of {}", p.task, p.tasks);
    if !p.detail.is_empty() {
        message.push_str(" · ");
        message.push_str(&p.detail);
    }
    job.progress(&p.label, message, p.percent);
}

/// A finished recipe: creates launchers for the programs it pinned, so the
/// app can be opened from Spotlight or the app menu right away.
pub(crate) fn finish(app: &AppHandle, result: vinter_core::Result<Bottle>) -> Result<(), String> {
    let bottle = match result {
        Ok(bottle) => bottle,
        Err(e) => {
            crate::notify::send(app, "Setting up the app did not finish", &e.to_string());
            return Err(e.to_string());
        }
    };
    let paths = &app.state::<AppState>().paths;
    let launchers = crate::startup::launcher_target()
        .and_then(|target| vinter_core::launcher::sync_bottle(paths, &bottle.id, &target));
    if let Err(e) = launchers {
        return Err(format!(
            "{} is set up, but its launcher could not be created: {e}",
            bottle.name
        ));
    }
    crate::tray::refresh(app);
    crate::notify::send(
        app,
        &format!("{} is ready", bottle.name),
        "Open it from Your apps, the menu bar or Spotlight.",
    );
    Ok(())
}

/// Sets an app up from a recipe in a new bottle. The bottle id is returned
/// right away; the bottle appears once its runtime and installer are ready.
#[tauri::command]
#[specta::specta]
pub fn recipe_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    recipe_id: String,
    bottle_name: String,
    installer_path: Option<String>,
    accept_downloads: bool,
) -> CommandResult<StartedBottle> {
    let loaded = recipe::find(&state.paths, &recipe_id)?;
    let bottle_id = uuid::Uuid::new_v4().to_string();
    state.mark_busy(&bottle_id);
    let paths = state.paths.clone();
    let opts = ApplyOptions {
        bottle_id: Some(bottle_id.clone()),
        bottle_name,
        installer: installer_path.map(PathBuf::from),
        accept_downloads,
        runtime: None,
    };
    let id = bottle_id.clone();
    let job_id = jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = recipe::apply(&paths, &loaded, opts, |p| report(&job, p)).await;
            app.state::<AppState>().clear_busy(&id);
            finish(&app, result)
        }
    });
    Ok(StartedBottle { bottle_id, job_id })
}

/// What "Recreate for Apple Silicon" would do (experimental
/// `bottle-migrate-arm64`).
#[tauri::command]
#[specta::specta]
pub fn bottle_migrate_plan(
    state: State<'_, AppState>,
    bottle_id: String,
) -> CommandResult<vinter_core::migrate::MigrationPlan> {
    Ok(vinter_core::migrate::plan(&state.paths, &bottle_id)?)
}

/// Recreates a bottle on the Apple Silicon engine, as a job. The old bottle
/// is not changed.
#[tauri::command]
#[specta::specta]
pub fn bottle_migrate_start(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    name: String,
    installer_path: Option<String>,
    accept_downloads: bool,
) -> CommandResult<StartedBottle> {
    vinter_core::migrate::plan(&state.paths, &bottle_id)?;
    let new_id = uuid::Uuid::new_v4().to_string();
    state.mark_busy(&new_id);
    let paths = state.paths.clone();
    let opts = vinter_core::migrate::MigrateOptions {
        bottle_id: Some(new_id.clone()),
        name,
        installer: installer_path.map(PathBuf::from),
        accept_downloads,
    };
    let id = new_id.clone();
    let job_id = jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result =
                vinter_core::migrate::migrate(&paths, &bottle_id, opts, |p| report(&job, p)).await;
            app.state::<AppState>().clear_busy(&id);
            finish(&app, result)
        }
    });
    Ok(StartedBottle {
        bottle_id: new_id,
        job_id,
    })
}

/// The unfinished recipe in a bottle, if setting it up stopped.
#[tauri::command]
#[specta::specta]
pub fn recipe_state(state: State<'_, AppState>, bottle_id: String) -> Option<RecipeState> {
    recipe::state(&state.paths, &bottle_id)
}

/// Continues an unfinished recipe from the step where it stopped.
#[tauri::command]
#[specta::specta]
pub fn recipe_resume(
    app: AppHandle,
    state: State<'_, AppState>,
    bottle_id: String,
    installer_path: Option<String>,
    accept_downloads: bool,
) -> CommandResult<JobId> {
    if !state.mark_busy(&bottle_id) {
        return Err("This bottle is busy. Try again in a moment.".into());
    }
    let paths = state.paths.clone();
    Ok(jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result = recipe::resume(
                &paths,
                &bottle_id,
                installer_path.map(PathBuf::from),
                accept_downloads,
                |p| report(&job, p),
            )
            .await;
            app.state::<AppState>().clear_busy(&bottle_id);
            finish(&app, result)
        }
    }))
}

/// The recipe made for this installer file, if its name matches one (e.g.
/// SteamSetup.exe and the Steam recipe).
#[tauri::command]
#[specta::specta]
pub fn recipe_for_installer(state: State<'_, AppState>, path: String) -> Option<RecipeSummary> {
    let name = std::path::Path::new(&path).file_name()?.to_string_lossy();
    recipe::match_installer(&state.paths, &name).map(|r| r.summary())
}

/// Opens the page to download a recipe's installer from (or its homepage).
#[tauri::command]
#[specta::specta]
pub fn recipe_open_download_page(
    app: AppHandle,
    state: State<'_, AppState>,
    recipe_id: String,
) -> CommandResult<()> {
    let summary = recipe::find(&state.paths, &recipe_id)?.summary();
    let url = summary
        .download_page
        .filter(|u| u.starts_with("https://"))
        .ok_or("This recipe has no download page.")?;
    app.opener().open_url(url, None::<&str>)?;
    Ok(())
}

/// Installers for a recipe that arrived in ~/Downloads since `since_ms`
/// (milliseconds since 1970), newest first.
#[tauri::command]
#[specta::specta]
pub async fn recipe_find_downloads(
    state: State<'_, AppState>,
    recipe_id: String,
    since_ms: f64,
) -> CommandResult<Vec<String>> {
    let loaded = recipe::find(&state.paths, &recipe_id)?;
    blocking(move || {
        let Some(dir) = recipe::downloads_folder() else {
            return Ok(Vec::new());
        };
        // A little slack: some browsers keep the time the download started.
        let since = std::time::UNIX_EPOCH
            + std::time::Duration::from_millis(since_ms.max(0.0) as u64)
                .saturating_sub(std::time::Duration::from_secs(5));
        Ok(recipe::recent_downloads(&dir, &loaded, since)
            .into_iter()
            .map(|p| p.display().to_string())
            .collect())
    })
    .await
}

/// Sets up a new bottle for an installer opened with Vinter, named after the
/// file. Installs a runtime first if none is installed. The bottle id is
/// returned right away; the job ends when the bottle is ready.
#[tauri::command]
#[specta::specta]
pub fn installer_setup_bottle(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<StartedBottle> {
    let file = std::path::Path::new(&path)
        .file_name()
        .ok_or("This is not a file.")?
        .to_string_lossy()
        .into_owned();
    let name = recipe::bottle_name_for(&file);
    let bottle_id = uuid::Uuid::new_v4().to_string();
    state.mark_busy(&bottle_id);
    let paths = state.paths.clone();
    let id = bottle_id.clone();
    let job_id = jobs::spawn(&app, |job| {
        let app = app.clone();
        async move {
            let result =
                recipe::create_for_installer(&paths, Some(id.clone()), name, |p| report(&job, p))
                    .await;
            app.state::<AppState>().clear_busy(&id);
            result.map(|_| ()).map_err(|e| e.to_string())
        }
    });
    Ok(StartedBottle { bottle_id, job_id })
}

/// Opens the folder for your own recipes (creating it if needed).
#[tauri::command]
#[specta::specta]
pub fn recipes_open_folder(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let dir = recipe::user_dir(&state.paths);
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)?;
    Ok(())
}
