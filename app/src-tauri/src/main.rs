// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use tauri_specta::{Builder, ErrorHandlingMode, collect_commands};

/// Where the generated TypeScript bindings live, relative to this crate.
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/api/bindings.ts");

/// Every command the frontend can call. TS bindings are generated from this.
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![commands::system::system_checks])
        .error_handling(ErrorHandlingMode::Throw)
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

    tauri::Builder::default()
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
