fn main() {
    // Declaring the app's commands makes Tauri require an explicit
    // `allow-<command>` permission in a capability before the UI can call them.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "system_checks",
            "rosetta_install",
            "runtimes_available",
            "runtimes_installed",
            "runtime_install",
            "runtime_remove",
            "runtime_open_link",
        ]),
    ))
    .expect("failed to run tauri-build");
}
