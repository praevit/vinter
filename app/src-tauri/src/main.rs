// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod focus;
mod jobs;
mod notify;
mod startup;
mod state;
mod tray;
mod windows;

use tauri::Manager;
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};
use vinter_core::AppPaths;

use crate::state::AppState;

/// Where the generated TypeScript bindings live, relative to this crate.
#[cfg(any(debug_assertions, test))]
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
            commands::runtimes::runtime_updates,
            commands::runtimes::runtime_update,
            commands::runtimes::runtime_open_link,
            commands::bottles::bottles_list,
            commands::bottles::bottle_get,
            commands::bottles::bottle_create,
            commands::bottles::bottle_update,
            commands::bottles::bottle_set_windows_version,
            commands::bottles::bottle_set_options,
            commands::bottles::bottle_domain_status,
            commands::bottles::bottle_set_domain,
            commands::bottles::bottle_domain_sign_in,
            commands::bottles::bottle_duplicate,
            commands::backup::bottle_export,
            commands::backup::bottle_import,
            commands::bottles::bottle_delete,
            commands::bottles::bottle_open_drive_c,
            commands::bottles::bottle_tool,
            commands::bottles::bottle_kill_all,
            commands::programs::installer_choose,
            commands::programs::installer_run,
            commands::programs::programs_discover,
            commands::programs::programs_add,
            commands::programs::program_remove,
            commands::programs::program_update,
            commands::programs::program_choose_icon,
            commands::programs::programs_list,
            commands::programs::program_launch,
            commands::programs::program_stop,
            commands::programs::runs_list,
            commands::programs::logs_list,
            commands::programs::log_read,
            commands::graphics::graphics_backends,
            commands::graphics::bottle_set_graphics,
            commands::graphics::bottle_test_graphics,
            commands::recipes::recipes_list,
            commands::recipes::recipe_apply,
            commands::recipes::recipe_state,
            commands::recipes::recipe_resume,
            commands::recipes::recipes_open_folder,
            commands::app::app_info,
            commands::app::app_open_link,
            commands::app::debug_report,
            commands::app::update_check,
            commands::app::update_install,
            commands::app::program_set_pinned,
            commands::app::open_files_take,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::settings::settings_set_launch_at_login,
            commands::settings::storage_usage,
            commands::settings::storage_clear_downloads,
            commands::settings::bottle_disk_usage,
            commands::settings::bottle_clean,
            commands::settings::d3dmetal_status,
            commands::settings::d3dmetal_choose,
            commands::settings::d3dmetal_import,
            commands::settings::d3dmetal_remove,
        ])
        .events(collect_events![
            events::JobProgress,
            events::JobDone,
            events::RunLog,
            events::RunExit,
            events::Notice,
            events::OpenFiles,
            events::Navigate
        ])
        .error_handling(ErrorHandlingMode::Throw)
        // Byte counts (u64) stay far below 2^53, so plain numbers are exact.
        .dangerously_cast_bigints_to_number()
}

#[cfg(any(debug_assertions, test))]
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

    let app = tauri::Builder::default()
        // Must come first: a second Vinter hands its arguments to this one.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            startup::on_second_instance(app, args);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        // "Start at login" starts Vinter in the menu bar, without its window.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![startup::HIDDEN_ARG]),
        ))
        .manage(AppState::new(paths))
        .manage(startup::Startup::default())
        .manage(startup::OpenedFiles::default())
        .manage(tray::TrayState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            use tauri_plugin_deep_link::DeepLinkExt;
            // Linux has no bundle metadata during development; register at runtime.
            #[cfg(target_os = "linux")]
            let _ = app.deep_link().register_all();
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                startup::on_urls(
                    &handle,
                    event.urls().iter().map(|u| u.to_string()).collect(),
                );
            });
            tray::create(app.handle());
            startup::check_for_updates_later(app.handle());
            startup::on_start(app.handle(), std::env::args().collect());
            Ok(())
        })
        // With a menu bar icon, closing the window keeps Vinter running there.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && tray::is_shown(window.app_handle()) {
                    api.prevent_close();
                    startup::hide_main_window(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Vinter");

    app.run(|app, event| {
        // macOS: clicking the Dock icon shows the window again.
        #[cfg(target_os = "macos")]
        match &event {
            tauri::RunEvent::Reopen { .. } => startup::show_main_window(app),
            // Finder: double-click or Open With on a .exe/.msi.
            tauri::RunEvent::Opened { urls } => startup::open_files(
                app,
                urls.iter().filter_map(|u| u.to_file_path().ok()).collect(),
            ),
            _ => {}
        }
        let _ = (app, event);
    });
}

#[cfg(test)]
mod tests {
    /// Regenerates `app/src/api/bindings.ts`, so `cargo test` keeps it in sync.
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
