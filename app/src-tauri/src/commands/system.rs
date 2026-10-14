use vinter_core::AppPaths;
use vinter_core::system::SystemStatus;

use super::CommandResult;

/// Checks whether this machine is ready to run Windows programs.
#[tauri::command]
#[specta::specta]
pub async fn system_checks() -> CommandResult<SystemStatus> {
    // The Rosetta check spawns a process, so keep it off the async workers.
    let status = tauri::async_runtime::spawn_blocking(|| {
        AppPaths::discover().map(|paths| vinter_core::system::system_checks(&paths))
    })
    .await??;
    Ok(status)
}
