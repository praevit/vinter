//! Automatic update checks. With "Check for updates automatically" on,
//! Vinter looks for a newer version of itself and of installed runtimes
//! shortly after it starts and then every few hours. What it finds stays
//! visible (a banner in the window, the update item in the menus) and is
//! announced once per version. Nothing is installed without the person.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;

use crate::commands::app::UpdateInfo;
use crate::events::UpdatesFound;
use crate::state::AppState;

/// After start, so the check does not compete with opening the window.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// What the last automatic check found.
#[derive(Debug, Clone, Default, PartialEq, Serialize, specta::Type)]
pub struct UpdateStatus {
    /// A newer Vinter, if there is one.
    pub app: Option<UpdateInfo>,
}

#[derive(Default)]
pub struct UpdatesState {
    status: Mutex<UpdateStatus>,
    /// Versions and runtime ids already announced, so each is announced once.
    announced: Mutex<HashSet<String>>,
}

/// The last automatic check's result.
pub fn status(app: &AppHandle) -> UpdateStatus {
    app.state::<UpdatesState>().status.lock().unwrap().clone()
}

/// Starts the checks in the background.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(&app).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

/// One automatic check. With the setting off, forgets what an earlier one found.
pub async fn check(app: &AppHandle) {
    let paths = app.state::<AppState>().paths.clone();
    if !vinter_core::settings::load(&paths).check_for_updates {
        set_status(app, UpdateStatus::default());
        return;
    }

    let runtimes = tauri::async_runtime::spawn_blocking({
        let paths = paths.clone();
        move || vinter_core::runtime::updates::find(&paths)
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default();
    for u in &runtimes {
        let name = match u.newer_build {
            Some(build) => format!("{} build {build}", u.newer.name),
            None => u.newer.name.clone(),
        };
        announce(
            app,
            &u.newer.id,
            &format!("{name} is available"),
            "Update it on the Runtimes page; your bottles move along.",
        );
    }

    // A failed check (offline, say) keeps what the last one found.
    let Ok(updater) = app.updater() else { return };
    let Ok(update) = updater.check().await else {
        return;
    };
    let found = update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    });
    if let Some(info) = &found {
        announce(
            app,
            &format!("vinter-{}", info.version),
            &format!("Vinter {} is available", info.version),
            "Open Vinter to install it, or choose Check for Updates… in its menu.",
        );
    }
    set_status(app, UpdateStatus { app: found });
}

/// Stores a result, updates the menus and tells the window.
fn set_status(app: &AppHandle, status: UpdateStatus) {
    let version = status.app.as_ref().map(|u| u.version.clone());
    *app.state::<UpdatesState>().status.lock().unwrap() = status;
    crate::tray::set_update(app, version.as_deref());
    // Runtime updates may have changed too, so always tell the window.
    let _ = UpdatesFound.emit(app);
}

fn announce(app: &AppHandle, key: &str, title: &str, body: &str) {
    let first = app
        .state::<UpdatesState>()
        .announced
        .lock()
        .unwrap()
        .insert(key.to_string());
    if first {
        crate::notify::send(app, title, body);
    }
}
