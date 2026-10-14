//! Background jobs: return an id immediately, report via events.

use std::future::Future;

use tauri::AppHandle;
use tauri_specta::Event;

use crate::events::{JobDone, JobProgress};

pub type JobId = String;

/// Sends progress for one job.
#[derive(Clone)]
pub struct JobReporter {
    app: AppHandle,
    job_id: JobId,
}

impl JobReporter {
    pub fn progress(&self, step: &str, message: String, percent: Option<f64>) {
        let event = JobProgress {
            job_id: self.job_id.clone(),
            step: step.into(),
            message,
            percent,
        };
        if let Err(e) = event.emit(&self.app) {
            eprintln!("failed to emit job progress: {e}");
        }
    }
}

/// Runs `work` in the background and emits `job://done` when it finishes.
pub fn spawn<W, F>(app: &AppHandle, work: W) -> JobId
where
    W: FnOnce(JobReporter) -> F,
    F: Future<Output = Result<(), String>> + Send + 'static,
{
    let job_id = uuid::Uuid::new_v4().to_string();
    let reporter = JobReporter {
        app: app.clone(),
        job_id: job_id.clone(),
    };
    let future = work(reporter);
    let (app, id) = (app.clone(), job_id.clone());
    tauri::async_runtime::spawn(async move {
        let result = future.await;
        let done = JobDone {
            job_id: id,
            ok: result.is_ok(),
            error: result.err(),
        };
        if let Err(e) = done.emit(&app) {
            eprintln!("failed to emit job done: {e}");
        }
    });
    job_id
}
