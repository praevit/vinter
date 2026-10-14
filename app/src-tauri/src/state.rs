use std::collections::HashSet;
use std::sync::Mutex;

use vinter_core::AppPaths;

/// Shared app state, managed by Tauri.
pub struct AppState {
    pub paths: AppPaths,
    /// Ids of runtimes currently being installed.
    pub installing: Mutex<HashSet<String>>,
}

impl AppState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            installing: Mutex::default(),
        }
    }

    pub fn is_installing(&self, id: &str) -> bool {
        self.installing.lock().unwrap().contains(id)
    }
}
