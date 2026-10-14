// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod focus;
mod jobs;
mod state;

use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};
use vinter_core::AppPaths;

use crate::state::AppState;

/// Where the generated TypeScript bindings live, relative to this crate.
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/api/bindings.ts");

/// Every command and event the frontend can use. TS bindings are generated from this.
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::system::system_checks,
            commands::system::rosetta_install,
            commands::runtimes::runtimes_available,
            commands::runtimes::runtimes_installed,
            commands::runtimes::runtime_install,
            commands::runtimes::runtime_remove,
            commands::runtimes::runtime_open_link,
            commands::bottles::bottles_list,
            commands::bottles::bottle_get,
            commands::bottles::bottle_create,
            commands::bottles::bottle_update,
            commands::bottles::bottle_set_windows_version,
            commands::bottles::bottle_duplicate,
            commands::bottles::bottle_delete,
            commands::bottles::bottle_open_drive_c,
            commands::bottles::bottle_tool,
            commands::bottles::bottle_kill_all,
        ])
        .events(collect_events![events::JobProgress, events::JobDone])
        .error_handling(ErrorHandlingMode::Throw)
        // Byte counts (u64) stay far below 2^53, so plain numbers are exact.
        .dangerously_cast_bigints_to_number()
}

fn export_bindings(builder: &Builder<tauri::Wry>) {
    builder
        .export(specta_typescript::Typescript::default(), BINDINGS_PATH)
        .expect("failed to export TypeScript bindings");
}

fn main() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    export_bindings(&builder);

    let paths = AppPaths::discover().expect("could not find the home directory");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(paths))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vinter");
}

#[cfg(test)]
mod tests {
    /// Regenerates `app/src/api/bindings.ts`, so `cargo test` keeps it in sync.
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
