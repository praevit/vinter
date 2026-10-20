//! Core library for Vinter.
//!
//! Everything that touches the system (paths, processes, downloads, Wine
//! prefixes) lives here, so it can be tested without a UI and shared by the
//! Tauri app and the `vinter` CLI.

pub mod bottle;
pub mod download;
pub mod error;
pub mod graphics;
pub mod launcher;
pub mod paths;
pub mod process;
pub mod programs;
pub mod recipe;
pub mod report;
pub mod runtime;
pub mod secret;
pub mod settings;
pub mod storage;
pub mod system;

pub use error::{Error, Result, validate_id};
pub use paths::AppPaths;

/// Version of this crate, shown in the UI and debug reports.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
