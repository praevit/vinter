use std::collections::HashSet;
use std::sync::Mutex;

use vinter_core::AppPaths;

/// Shared app state, managed by Tauri.
pub struct AppState {
    pub paths: AppPaths,
    /// Ids of runtimes currently being installed.
    pub installing: Mutex<HashSet<String>>,
    /// Ids of bottles with a running setup, duplicate or settings job.
    pub busy_bottles: Mutex<HashSet<String>>,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            installing: Mutex::default(),
            busy_bottles: Mutex::default(),
        }
    }

    pub fn is_installing(&self, id: &str) -> bool {
        self.installing.lock().unwrap().contains(id)
    }

    pub fn is_busy(&self, bottle_id: &str) -> bool {
        self.busy_bottles.lock().unwrap().contains(bottle_id)
    }

    /// Marks a bottle busy; false if it already was.
    pub fn mark_busy(&self, bottle_id: &str) -> bool {
        self.busy_bottles
            .lock()
            .unwrap()
            .insert(bottle_id.to_string())
    }

    pub fn clear_busy(&self, bottle_id: &str) {
        self.busy_bottles.lock().unwrap().remove(bottle_id);
    }
}
