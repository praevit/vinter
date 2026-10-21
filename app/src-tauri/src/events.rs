//! Events sent to the frontend. Long operations return a job id right away
//! and report through `job://progress` and `job://done`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://progress")]
pub struct JobProgress {
    pub job_id: String,
    /// Short name of the current step, e.g. "Downloading".
    pub step: String,
    /// Detail for the step, e.g. "64 of 185 MB".
    pub message: String,
    /// 0 to 100, or null when the step has no measurable progress.
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "run://log")]
pub struct RunLog {
    pub run_id: String,
    pub line: String,
    pub stream: Stream,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "run://exit")]
pub struct RunExit {
    pub run_id: String,
    /// Exit code, or null if the program was stopped by a signal.
    pub code: Option<i32>,
}

/// Output stream of a log line.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

impl From<vinter_core::process::Stream> for Stream {
    fn from(s: vinter_core::process::Stream) -> Self {
        match s {
            vinter_core::process::Stream::Stdout => Self::Stdout,
            vinter_core::process::Stream::Stderr => Self::Stderr,
        }
    }
}

/// A message for the user that has no other place to appear, e.g. a
/// launcher that failed to start its program.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "app://notice")]
pub struct Notice {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "job://done")]
pub struct JobDone {
    pub job_id: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Windows programs were opened with Vinter (Finder, Open With, or the
/// command line). The interface fetches them with `open_files_take`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "app://open-files")]
pub struct OpenFiles;

/// An AI agent asked to do something (`vinter://approve/<id>`). The
/// interface fetches the requests with `approvals_pending`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "app://approval")]
pub struct ApprovalRequested;

/// Asks the interface to show a page (from the app menu).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "app://navigate")]
pub struct Navigate {
    pub to: NavigateTo,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NavigateTo {
    Settings,
}
