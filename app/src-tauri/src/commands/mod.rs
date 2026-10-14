//! Tauri commands. Each one is a thin wrapper over `vinter-core`.

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
