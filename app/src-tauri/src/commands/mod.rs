//! Tauri commands. Each one is a thin wrapper over `vinter-core`.

pub mod runtimes;
pub mod system;

use serde::Serialize;

/// Error returned to the frontend. Commands reject with this value.
#[derive(Debug, Serialize, specta::Type)]
pub struct CommandError {
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

/// Runs blocking work (file system, child processes) off the async workers.
pub async fn blocking<T, F>(work: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work).await?
}
