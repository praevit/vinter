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
            "bottles_list",
            "bottle_get",
            "bottle_create",
            "bottle_update",
            "bottle_set_windows_version",
            "bottle_duplicate",
            "bottle_delete",
            "bottle_open_drive_c",
            "bottle_tool",
            "bottle_kill_all",
        ]),
    ))
    .expect("failed to run tauri-build");
}
