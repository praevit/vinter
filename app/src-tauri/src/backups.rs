//! Automatic backups. With "Back up bottles automatically" on, Vinter
//! checks shortly after it starts and then every half hour which bottles are
//! due, and backs them up one at a time in the background. Bottles with
//! programs running, or busy with another job, wait for the next check.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Manager};
use vinter_core::auto_backup;

use crate::state::AppState;

const FIRST_CHECK: Duration = Duration::from_secs(2 * 60);
const INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Set while backups run, so checks never overlap.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Starts the checks in the background.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            run_due(&app, false);
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Backs up the bottles that are due (with `force`, every bottle) on a
/// worker thread. False if backups were already running.
pub fn run_due(app: &AppHandle, force: bool) -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let due = auto_backup::due(&state.paths, SystemTime::now(), force).unwrap_or_default();
        let mut failed = Vec::new();
        for id in due {
            let running = state
                .runs
                .lock()
                .unwrap()
                .values()
                .any(|r| r.bottle_id == id);
            if running || !state.mark_busy(&id) {
                continue;
            }
            let result = auto_backup::run(&state.paths, &id);
            state.clear_busy(&id);
            if let Err(e) = result {
                let name = vinter_core::bottle::get(&state.paths, &id)
                    .map(|b| b.name)
                    .unwrap_or(id);
                failed.push(format!("{name}: {e}"));
            }
        }
        if !failed.is_empty() {
            crate::notify::send(
                &app,
                "An automatic backup did not finish",
                &failed.join("\n"),
            );
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    true
}
