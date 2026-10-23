//! Bottle tests against a fake runtime whose `wine` and `wineserver` are
//! shell scripts that record how they were called.

use std::path::Path;

use super::*;
use crate::runtime::RuntimeRecord;

const FAKE_WINE: &str = r#"#!/bin/sh
case "$1" in
  wineboot)
    mkdir -p "$WINEPREFIX/drive_c/windows" "$WINEPREFIX/dosdevices"
    ln -s ../drive_c "$WINEPREFIX/dosdevices/c:"
    ln -s / "$WINEPREFIX/dosdevices/z:"
    echo "registry" > "$WINEPREFIX/user.reg"
    env | grep '^WINE' | sort > "$WINEPREFIX/boot-env.txt"
    echo "booted"
    ;;
  winecfg) echo "$3" > "$WINEPREFIX/windows-version.txt" ;;
  regedit) echo "$2" >> "$WINEPREFIX/regedit.txt" ;;
  *) echo "$1" >> "$WINEPREFIX/launched.txt" ;;
esac
"#;
const FAKE_WINESERVER: &str = "#!/bin/sh\necho \"$*\" >> \"$WINEPREFIX/wineserver.txt\"\n";
const BROKEN_WINE: &str = "#!/bin/sh\necho 'err:something broke' >&2\nexit 1\n";

fn install_fake_runtime(paths: &AppPaths, id: &str, wine: &str) {
    let dir = paths.runtimes_dir().join(id);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    for (name, script) in [("wine", wine), ("wineserver", FAKE_WINESERVER)] {
        let path = dir.join("bin").join(name);
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let record = RuntimeRecord {
        id: id.into(),
        name: format!("Fake {id}"),
        version: "11.0".into(),
        flavor: runtime::Flavor::Stable,
        url: "https://example.invalid".into(),
        sha256: "0".repeat(64),
        installed_at: "2026-10-14T20:00:00Z".into(),
        wine_bin: "bin/wine".into(),
        license: "LGPL-2.1-or-later".into(),
        source_url: "https://example.invalid".into(),
        arch: std::env::consts::ARCH.into(),
        graphics: vec![crate::bottle::Graphics::Wined3d],
        engine: Some(crate::engine::EngineKind::legacy()),
    };
    std::fs::write(
        dir.join(runtime::RECORD_FILE),
        toml::to_string(&record).unwrap(),
    )
    .unwrap();
}

fn setup() -> (tempfile::TempDir, AppPaths) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    install_fake_runtime(&paths, "fake", FAKE_WINE);
    (tmp, paths)
}

fn new_bottle(name: &str, runtime: &str) -> NewBottle {
    NewBottle {
        id: None,
        name: name.into(),
        runtime: runtime.into(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
    }
}

fn prefix(paths: &AppPaths, id: &str) -> PathBuf {
    dir(paths, id).unwrap().join(PREFIX_DIR)
}

fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Waits for a detached process to write its marker file.
fn wait_for(path: &Path) -> String {
    for _ in 0..50 {
        let text = read(path);
        if !text.is_empty() {
            return text;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    String::new()
}

#[cfg(unix)]
#[test]
fn create_runs_wineboot_with_the_right_environment() {
    let (_tmp, paths) = setup();
    let mut steps = Vec::new();
    let bottle = create(&paths, new_bottle("  Office  ", "fake"), |s| {
        steps.push(s.to_string())
    })
    .unwrap();

    assert_eq!(bottle.name, "Office");
    let mut expected = vec!["Creating Wine prefix", "Setting Windows version"];
    if cfg!(target_os = "macos") {
        // New bottles get Retina mode and ⌘ as Ctrl.
        expected.push("Applying display and keyboard settings");
    }
    assert_eq!(steps, expected);
    let pfx = prefix(&paths, &bottle.id);
    assert!(pfx.join("drive_c/windows").is_dir());
    assert_eq!(read(pfx.join("windows-version.txt")).trim(), "win10");

    let env = read(pfx.join("boot-env.txt"));
    assert!(
        env.contains(&format!("WINEPREFIX={}", pfx.display())),
        "{env}"
    );
    assert!(env.contains("WINEDEBUG=-all"), "{env}");
    assert!(
        env.contains("WINEDLLOVERRIDES=mscoree=;mshtml=;winemenubuilder.exe=d"),
        "{env}"
    );
    assert!(read(pfx.join("wineserver.txt")).contains("-w"));

    let summaries = list(&paths).unwrap();
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].setup_complete);
    assert_eq!(get(&paths, &bottle.id).unwrap(), bottle);
    let logs: Vec<_> = std::fs::read_dir(dir(&paths, &bottle.id).unwrap().join("logs"))
        .unwrap()
        .collect();
    assert!(!logs.is_empty());
}

#[cfg(unix)]
#[test]
fn failed_create_leaves_nothing_behind() {
    let (_tmp, paths) = setup();
    install_fake_runtime(&paths, "broken", BROKEN_WINE);
    let err = create(&paths, new_bottle("Nope", "broken"), |_| {}).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Creating the Wine prefix") && msg.contains("something broke"),
        "{msg}"
    );
    assert!(list(&paths).unwrap().is_empty());
    assert!(
        std::fs::read_dir(paths.bottles_dir())
            .unwrap()
            .next()
            .is_none(),
        "bottle folder must be removed"
    );
}

#[test]
fn create_checks_name_and_runtime() {
    let (_tmp, paths) = setup();
    assert!(matches!(
        create(&paths, new_bottle("   ", "fake"), |_| {}),
        Err(Error::InvalidName(_))
    ));
    assert!(matches!(
        create(&paths, new_bottle("A", "missing"), |_| {}),
        Err(Error::RuntimeNotInstalled(_))
    ));
    assert!(validate_name(&"x".repeat(65)).is_err());
    assert!(validate_name("a\nb").is_err());
    assert_eq!(validate_name(" Steam ").unwrap(), "Steam");
}

#[cfg(unix)]
#[test]
fn rename_windows_version_tools_and_kill() {
    let (_tmp, paths) = setup();
    let b = create(&paths, new_bottle("Games", "fake"), |_| {}).unwrap();
    let pfx = prefix(&paths, &b.id);

    assert_eq!(
        rename(&paths, &b.id, "Old games").unwrap().name,
        "Old games"
    );
    assert_eq!(get(&paths, &b.id).unwrap().name, "Old games");

    let updated = set_windows_version(&paths, &b.id, WindowsVersion::Win7).unwrap();
    assert_eq!(updated.windows_version, WindowsVersion::Win7);
    assert_eq!(read(pfx.join("windows-version.txt")).trim(), "win7");
    assert_eq!(
        get(&paths, &b.id).unwrap().windows_version,
        WindowsVersion::Win7
    );

    run_tool(&paths, &b.id, Tool::Notepad).unwrap();
    assert_eq!(wait_for(&pfx.join("launched.txt")).trim(), "notepad");

    kill_all(&paths, &b.id).unwrap();
    assert!(read(pfx.join("wineserver.txt")).contains("-k"));
}

#[cfg(unix)]
#[test]
fn duplicate_copies_prefix_without_following_symlinks() {
    let (_tmp, paths) = setup();
    let original = create(&paths, new_bottle("Base", "fake"), |_| {}).unwrap();
    let copy = duplicate(&paths, &original.id, "Copy").unwrap();

    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, "Copy");
    assert_eq!(copy.runtime, original.runtime);
    let pfx = prefix(&paths, &copy.id);
    assert_eq!(read(pfx.join("user.reg")).trim(), "registry");
    let z = pfx.join("dosdevices/z:");
    assert!(z.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_link(&z).unwrap(), Path::new("/"));
    assert_eq!(
        std::fs::read_link(pfx.join("dosdevices/c:")).unwrap(),
        Path::new("../drive_c")
    );
    assert!(!dir(&paths, &copy.id).unwrap().join("logs").exists());
    assert!(!dir(&paths, &copy.id).unwrap().join(SETUP_MARKER).exists());

    let names: Vec<_> = list(&paths).unwrap().into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["Base", "Copy"]);
}

#[cfg(unix)]
#[test]
fn runtime_in_use_cannot_be_removed() {
    let (_tmp, paths) = setup();
    create(&paths, new_bottle("Uses fake", "fake"), |_| {}).unwrap();
    let err = runtime::remove(&paths, "fake").unwrap_err();
    assert!(matches!(err, Error::RuntimeInUse { .. }), "{err}");
    assert!(err.to_string().contains("Uses fake"));
}

#[cfg(unix)]
#[test]
fn resolve_by_id_prefix_or_name() {
    let (_tmp, paths) = setup();
    let b = create(&paths, new_bottle("Notepad++", "fake"), |_| {}).unwrap();
    assert_eq!(resolve(&paths, "Notepad++").unwrap().id, b.id);
    assert_eq!(resolve(&paths, &b.id[..8]).unwrap().id, b.id);
    assert!(matches!(
        resolve(&paths, "nope"),
        Err(Error::BottleNotFound(_))
    ));
}

#[test]
fn interrupted_setup_is_reported() {
    let (_tmp, paths) = setup();
    let d = paths.bottles_dir().join("half");
    std::fs::create_dir_all(&d).unwrap();
    Bottle {
        id: "half".into(),
        name: "Half".into(),
        runtime: "fake".into(),
        engine: crate::engine::EngineKind::legacy(),
        created_at: now(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        options: Default::default(),
        programs: vec![],
        domain: None,
    }
    .save(&d)
    .unwrap();
    std::fs::write(d.join(SETUP_MARKER), "").unwrap();
    assert!(!list(&paths).unwrap()[0].setup_complete);
    assert!(matches!(
        run_tool(&paths, "half", Tool::Notepad),
        Err(Error::BottleNotReady(_))
    ));
}

#[test]
fn env_and_override_editors_validate_input() {
    use std::collections::BTreeMap;
    let (_tmp, paths) = setup();
    let d = paths.bottles_dir().join("x");
    std::fs::create_dir_all(&d).unwrap();
    Bottle {
        id: "x".into(),
        name: "X".into(),
        runtime: "fake".into(),
        engine: crate::engine::EngineKind::legacy(),
        created_at: now(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        options: Default::default(),
        programs: vec![],
        domain: None,
    }
    .save(&d)
    .unwrap();

    let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let b = set_dll_overrides(&paths, "x", map(&[("d3d11", "n,b"), ("mscoree", "")])).unwrap();
    assert_eq!(b.dll_overrides["d3d11"], "n,b");
    assert!(set_dll_overrides(&paths, "x", map(&[("d3d11", "sometimes")])).is_err());
    assert!(set_dll_overrides(&paths, "x", map(&[("../evil", "n")])).is_err());

    let b = set_env(&paths, "x", map(&[("WINEESYNC", "1"), ("DXVK_HUD", "fps")])).unwrap();
    assert_eq!(b.env["DXVK_HUD"], "fps");
    assert!(set_env(&paths, "x", map(&[("WINEPREFIX", "/tmp")])).is_err());
    assert!(set_env(&paths, "x", map(&[("1BAD", "x")])).is_err());
    assert!(set_env(&paths, "x", map(&[("A B", "x")])).is_err());
    assert!(set_env(&paths, "x", map(&[("OK", "a\0b")])).is_err());
    assert_eq!(get(&paths, "x").unwrap().env.len(), 2);
}

#[test]
fn ids_are_validated() {
    let (_tmp, paths) = setup();
    assert!(matches!(get(&paths, "../x"), Err(Error::InvalidId(_))));
    assert!(matches!(drive_c(&paths, "a/b"), Err(Error::InvalidId(_))));
}

#[test]
fn options_reg_maps_switches_to_the_mac_driver_and_dpi() {
    let on = options_reg(&BottleOptions {
        retina_mode: true,
        command_as_ctrl: true,
        option_as_alt: false,
        show_fps: false,
    });
    assert!(on.starts_with("REGEDIT4\r\n"));
    assert!(on.contains("\"LeftOptionIsAlt\"=\"n\""));
    assert!(on.contains("\"RightOptionIsAlt\"=\"n\""));
    assert!(on.contains("[HKEY_CURRENT_USER\\Software\\Wine\\Mac Driver]"));
    assert!(on.contains("\"RetinaMode\"=\"y\""));
    assert!(on.contains("\"LeftCommandIsCtrl\"=\"y\""));
    assert!(on.contains("\"LogPixels\"=dword:000000c0"));
    let off = options_reg(&BottleOptions::default());
    assert!(off.contains("\"RetinaMode\"=\"n\""));
    assert!(off.contains("\"LogPixels\"=dword:00000060"));
}

#[test]
fn options_are_saved_and_written_to_the_registry_on_macos() {
    let (_tmp, paths) = setup();
    install_fake_runtime(&paths, "rt", FAKE_WINE);
    let b = create(&paths, new_bottle("Opts", "rt"), |_| {}).unwrap();
    // New bottles get the recommended options (no settings file here).
    assert_eq!(b.options, BottleOptions::recommended());

    let fps_only = BottleOptions {
        show_fps: true,
        ..b.options
    };
    let saved = set_options(&paths, &b.id, fps_only).unwrap();
    assert_eq!(get(&paths, &b.id).unwrap().options, saved.options);
    let regedit_runs = || {
        std::fs::read_to_string(prefix(&paths, &b.id).join("regedit.txt"))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    };
    let before = regedit_runs();
    // The overlay is an environment variable: no registry change.
    assert_eq!(before, if cfg!(target_os = "macos") { 1 } else { 0 });

    set_options(&paths, &b.id, BottleOptions::default()).unwrap();
    let expected = if cfg!(target_os = "macos") { 2 } else { 0 };
    assert_eq!(regedit_runs(), expected);
    assert!(!bottle_dir_has_reg_file(&paths, &b.id));
}

fn bottle_dir_has_reg_file(paths: &AppPaths, id: &str) -> bool {
    dir(paths, id).unwrap().join(".options.reg").exists()
}

#[test]
fn desktop_link_becomes_a_folder_of_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let mac_desktop = tmp.path().join("MacDesktop");
    std::fs::create_dir_all(&mac_desktop).unwrap();
    std::fs::write(mac_desktop.join("mine.txt"), "keep").unwrap();
    let prefix = tmp.path().join("prefix");
    let user = prefix.join("drive_c/users/me");
    std::fs::create_dir_all(&user).unwrap();
    std::os::unix::fs::symlink(&mac_desktop, user.join("Desktop")).unwrap();
    std::os::unix::fs::symlink(&mac_desktop, user.join("Documents")).unwrap();

    keep_desktop_in_bottle(&prefix).unwrap();
    let desktop = std::fs::symlink_metadata(user.join("Desktop")).unwrap();
    assert!(desktop.is_dir() && !desktop.file_type().is_symlink());
    assert!(
        std::fs::symlink_metadata(user.join("Documents"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(mac_desktop.join("mine.txt").exists());
    // Running it again changes nothing.
    keep_desktop_in_bottle(&prefix).unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_restore_rolls_back_files_and_settings() {
    let (_tmp, paths) = setup();
    let b = create(&paths, new_bottle("Base", "fake"), |_| {}).unwrap();
    let clean = snapshot::create(&paths, &b.id, " Clean ", false).unwrap();
    assert_eq!(clean.label, "Clean");

    let pfx = prefix(&paths, &b.id);
    std::fs::write(pfx.join("user.reg"), "changed").unwrap();
    std::fs::write(pfx.join("drive_c/new.txt"), "new").unwrap();
    let env = [("FOO".to_string(), "1".to_string())].into();
    set_env(&paths, &b.id, env).unwrap();
    rename(&paths, &b.id, "Renamed").unwrap();

    let restored = snapshot::restore(&paths, &b.id, &clean.id).unwrap();
    assert_eq!(restored.name, "Renamed", "the name is kept");
    assert!(restored.env.is_empty());
    assert_eq!(get(&paths, &b.id).unwrap(), restored);
    assert_eq!(read(pfx.join("user.reg")).trim(), "registry");
    assert!(!pfx.join("drive_c/new.txt").exists());
    let z = pfx.join("dosdevices/z:");
    assert_eq!(std::fs::read_link(&z).unwrap(), Path::new("/"));
    assert!(!dir(&paths, &b.id).unwrap().join(".restore-old").exists());

    // Restoring snapshotted the state it replaced, so it can be undone.
    let snaps = snapshot::list(&paths, &b.id).unwrap();
    assert_eq!(snaps.len(), 2);
    let before = snaps.iter().find(|s| s.label == "Before restore").unwrap();
    assert!(before.automatic);
    snapshot::restore(&paths, &b.id, &before.id).unwrap();
    assert_eq!(read(pfx.join("user.reg")), "changed");
    assert_eq!(get(&paths, &b.id).unwrap().env["FOO"], "1");
}

#[cfg(unix)]
#[test]
fn automatic_snapshots_are_pruned_and_manual_ones_kept() {
    let (_tmp, paths) = setup();
    let b = create(&paths, new_bottle("Base", "fake"), |_| {}).unwrap();
    let manual = snapshot::create(&paths, &b.id, "Mine", false).unwrap();
    for i in 0..snapshot::KEEP_AUTOMATIC + 2 {
        snapshot::create(&paths, &b.id, &format!("auto {i}"), true).unwrap();
    }
    let snaps = snapshot::list(&paths, &b.id).unwrap();
    assert_eq!(
        snaps.iter().filter(|s| s.automatic).count(),
        snapshot::KEEP_AUTOMATIC
    );
    assert!(snaps.iter().any(|s| s.id == manual.id));
    assert!(
        !snaps
            .iter()
            .any(|s| s.label == "auto 0" || s.label == "auto 1")
    );

    snapshot::delete(&paths, &b.id, &manual.id).unwrap();
    assert!(matches!(
        snapshot::restore(&paths, &b.id, &manual.id),
        Err(Error::SnapshotNotFound(_))
    ));
    assert!(matches!(
        snapshot::delete(&paths, &b.id, "../escape"),
        Err(Error::InvalidId(_))
    ));
}

#[cfg(unix)]
#[test]
fn switching_runtime_snapshots_first_where_clones_are_free() {
    let (_tmp, paths) = setup();
    install_fake_runtime(&paths, "other", FAKE_WINE);
    let b = create(&paths, new_bottle("Base", "fake"), |_| {}).unwrap();
    set_runtime(&paths, &b.id, "other").unwrap();
    let snaps = snapshot::list(&paths, &b.id).unwrap();
    if can_clone(&dir(&paths, &b.id).unwrap()) {
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].label, "Before switching to other");
    } else {
        assert!(snaps.is_empty());
    }
    // The snapshot is not counted in the bottle's size.
    let usage = crate::storage::usage(&paths).unwrap();
    let total = crate::storage::dir_size(&dir(&paths, &b.id).unwrap());
    assert!(snaps.is_empty() || usage.bottles[0].bytes < total);
}

/// Rewrites a fake runtime's record with another engine.
fn set_fake_engine(paths: &AppPaths, id: &str, engine: crate::engine::EngineKind) {
    let file = paths.runtimes_dir().join(id).join(runtime::RECORD_FILE);
    let mut record: RuntimeRecord =
        toml::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    record.engine = Some(engine);
    std::fs::write(&file, toml::to_string(&record).unwrap()).unwrap();
}

#[test]
fn engine_is_fixed_after_creation() {
    let (_tmp, paths) = setup();
    let b = create(&paths, new_bottle("Base", "fake"), |_| {}).unwrap();
    assert_eq!(b.engine, crate::engine::EngineKind::legacy());
    assert_eq!(get(&paths, &b.id).unwrap().engine, b.engine);

    // Switching to a runtime of another engine is refused and changes nothing.
    install_fake_runtime(&paths, "arm", FAKE_WINE);
    set_fake_engine(&paths, "arm", crate::engine::EngineKind::Arm64Fex);
    let err = set_runtime(&paths, &b.id, "arm").unwrap_err();
    assert!(matches!(err, Error::EngineMismatch { .. }), "{err}");
    assert_eq!(get(&paths, &b.id).unwrap().runtime, "fake");
    assert!(snapshot::list(&paths, &b.id).unwrap().is_empty());

    // A runtime whose engine changed under the bottle does not launch it.
    set_fake_engine(&paths, "fake", crate::engine::EngineKind::Arm64Fex);
    let rt = runtime::get(&paths, "fake").unwrap();
    let prefix = dir(&paths, &b.id).unwrap().join(PREFIX_DIR);
    let err = process::command(&rt, &prefix, &b, &[], "wine").unwrap_err();
    assert!(matches!(err, Error::EngineMismatch { .. }), "{err}");
}
