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
    assert_eq!(steps, ["Creating Wine prefix", "Setting Windows version"]);
    let pfx = prefix(&paths, &bottle.id);
    assert!(pfx.join("drive_c/windows").is_dir());
    assert_eq!(read(pfx.join("windows-version.txt")).trim(), "win10");

    let env = read(pfx.join("boot-env.txt"));
    assert!(
        env.contains(&format!("WINEPREFIX={}", pfx.display())),
        "{env}"
    );
    assert!(env.contains("WINEDEBUG=-all"), "{env}");
    assert!(env.contains("WINEDLLOVERRIDES=mscoree=;mshtml="), "{env}");
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
        created_at: now(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        programs: vec![],
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
fn ids_are_validated() {
    let (_tmp, paths) = setup();
    assert!(matches!(get(&paths, "../x"), Err(Error::InvalidId(_))));
    assert!(matches!(drive_c(&paths, "a/b"), Err(Error::InvalidId(_))));
}
