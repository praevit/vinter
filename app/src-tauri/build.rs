/// Sidecar name in `tauri.conf.json` (`bundle.externalBin`).
const CLI_SIDECAR: &str = "binaries/vinter-cli";

/// Tauri needs `binaries/vinter-cli-<target>` to exist when it builds. The
/// release workflow puts the real `vinter` command there first; other builds
/// get a stand-in that says how to build it, so `cargo build` keeps working.
fn ensure_cli_sidecar() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET");
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::PathBuf::from(format!("{CLI_SIDECAR}-{target}{ext}"));
    if path.exists() {
        return;
    }
    std::fs::create_dir_all(path.parent().unwrap()).expect("create binaries/");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         # vinter-cli-placeholder: this build of Vinter does not include the vinter command.\n\
         echo 'This build of Vinter does not include the vinter command. Build it with: cargo build --release -p vinter-cli' >&2\n\
         exit 1\n",
    )
    .expect("write the vinter command stand-in");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make the stand-in executable");
    }
}

fn main() {
    ensure_cli_sidecar();
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
            "runtime_updates",
            "runtime_update",
            "runtime_open_link",
            "bottles_list",
            "bottle_get",
            "bottle_create",
            "bottle_update",
            "bottle_set_windows_version",
            "bottle_set_options",
            "bottle_domain_status",
            "bottle_set_domain",
            "bottle_domain_sign_in",
            "bottle_duplicate",
            "bottle_export",
            "bottle_import",
            "bottle_delete",
            "bottle_open_drive_c",
            "bottle_tool",
            "bottle_kill_all",
            "bottle_set_drives",
            "drive_choose_folder",
            "components_list",
            "components_install",
            "snapshots_list",
            "snapshot_create",
            "snapshot_restore",
            "snapshot_delete",
            "installer_choose",
            "installer_run",
            "programs_discover",
            "programs_add",
            "program_remove",
            "program_update",
            "program_choose_icon",
            "programs_list",
            "program_launch",
            "program_launch_logged",
            "program_open_files",
            "program_set_file_types",
            "program_stop",
            "program_show",
            "runs_list",
            "logs_list",
            "log_read",
            "log_diagnose",
            "graphics_backends",
            "bottle_set_graphics",
            "bottle_test_graphics",
            "recipes_list",
            "recipe_apply",
            "recipe_state",
            "recipe_resume",
            "recipes_open_folder",
            "recipe_for_installer",
            "recipe_open_download_page",
            "recipe_find_downloads",
            "installer_setup_bottle",
            "app_info",
            "app_open_link",
            "debug_report",
            "update_check",
            "update_status",
            "update_install",
            "program_set_pinned",
            "open_files_take",
            "approvals_pending",
            "approval_decide",
            "settings_get",
            "settings_set",
            "settings_set_launch_at_login",
            "settings_window_show",
            "storage_usage",
            "storage_clear_downloads",
            "bottle_disk_usage",
            "bottle_clean",
            "d3dmetal_status",
            "d3dmetal_choose",
            "d3dmetal_import",
            "d3dmetal_remove",
            "cli_status",
            "cli_install",
            "auto_backup_get",
            "auto_backup_set",
            "auto_backup_choose_folder",
            "auto_backup_run_now",
            "bottle_save_recipe",
            "recipe_share",
            "bottle_history",
            "bottle_history_revert",
        ]),
    ))
    .expect("failed to run tauri-build");
}
