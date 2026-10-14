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
#[tauri_specta(event_name = "job://done")]
pub struct JobDone {
    pub job_id: String,
    pub ok: bool,
    pub error: Option<String>,
}
