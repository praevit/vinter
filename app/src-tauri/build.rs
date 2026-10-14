fn main() {
    // Declaring the app's commands makes Tauri require an explicit
    // `allow-<command>` permission in a capability before the UI can call them.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["system_checks"])),
    )
    .expect("failed to run tauri-build");
}
