use super::*;
use crate::bottle::{Graphics, ProgramDef, WindowsVersion};

fn bottle_with_programs(paths: &AppPaths, id: &str, name: &str, programs: &[&str]) {
    let dir = bottle::dir(paths, id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    Bottle {
        id: id.into(),
        name: name.into(),
        runtime: "rt".into(),
        created_at: "2026-10-15T00:00:00Z".into(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        programs: programs
            .iter()
            .map(|n| ProgramDef {
                name: n.to_string(),
                exe: format!(r"C:\Program Files\{n}\{n}.exe"),
                args: vec![],
                icon: None,
                pinned: false,
            })
            .collect(),
    }
    .save(&dir)
    .unwrap();
}

fn targets(root: &std::path::Path) -> (Target, Target) {
    (
        Target {
            kind: Kind::MacApp,
            dir: root.join("Applications/Vinter"),
            icons_dir: PathBuf::new(),
            command: vec![],
        },
        Target {
            kind: Kind::LinuxDesktop,
            dir: root.join("share/applications"),
            icons_dir: root.join("share/icons/vinter"),
            command: vec!["/opt/My Apps/vinter-app".into()],
        },
    )
}

#[test]
fn deep_links_round_trip_and_reject_junk() {
    assert_eq!(deep_link("abc-1", 2), "vinter://run/abc-1/2");
    assert_eq!(
        parse_deep_link("vinter://run/abc-1/2"),
        Some(("abc-1".into(), 2))
    );
    assert_eq!(
        parse_deep_link("vinter://run/abc-1/2/"),
        Some(("abc-1".into(), 2))
    );
    for bad in [
        "vinter://run/../2",
        "vinter://run/a/b",
        "vinter://run/a/1/x",
        "https://x/run/a/1",
        "vinter://open/a/1",
    ] {
        assert_eq!(parse_deep_link(bad), None, "{bad}");
    }
}

#[test]
fn mac_launchers_follow_pins_and_indexes() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let (mac, _) = targets(tmp.path());
    bottle_with_programs(&paths, "b1", "Office", &["Word", "Excel"]);

    set_pinned(&paths, "b1", 1, true, &mac).unwrap();
    let app = mac.dir.join("Excel.app");
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    assert!(
        plist.contains("<string>vinter://run/b1/1</string>"),
        "{plist}"
    );
    assert!(plist.contains("<key>LSUIElement</key>"));
    let script = std::fs::read_to_string(app.join("Contents/MacOS/launch")).unwrap();
    assert!(
        script.ends_with("exec /usr/bin/open \"vinter://run/b1/1\"\n"),
        "{script}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(app.join("Contents/MacOS/launch"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }
    let icns = std::fs::read(app.join("Contents/Resources/icon.icns")).unwrap();
    assert_eq!(&icns[..4], b"icns");
    assert_eq!(
        u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
        icns.len()
    );
    assert_eq!(&icns[8..12], b"ic07");
    assert_eq!(&icns[16..20], b"\x89PNG");

    // Another bottle with an "Excel" gets a distinct name.
    bottle_with_programs(&paths, "b2", "Old Office", &["Excel"]);
    set_pinned(&paths, "b2", 0, true, &mac).unwrap();
    assert!(mac.dir.join("Excel (Old Office).app").is_dir());

    // Removing Word shifts Excel to index 0: a resync fixes the launcher.
    crate::programs::remove(&paths, "b1", 0).unwrap();
    sync_bottle(&paths, "b1", &mac).unwrap();
    let plist = std::fs::read_to_string(mac.dir.join("Excel.app/Contents/Info.plist")).unwrap();
    assert!(plist.contains("vinter://run/b1/0"), "{plist}");

    // Unpinning removes only that bottle's launcher.
    set_pinned(&paths, "b1", 0, false, &mac).unwrap();
    assert!(!mac.dir.join("Excel.app").exists());
    assert!(mac.dir.join("Excel (Old Office).app").is_dir());
    remove_bottle("b2", &mac).unwrap();
    assert_eq!(std::fs::read_dir(&mac.dir).unwrap().count(), 0);
}

#[test]
fn linux_launchers() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let (_, linux) = targets(tmp.path());
    bottle_with_programs(&paths, "b1", "Tools", &["7-Zip"]);

    let created = set_pinned(&paths, "b1", 0, true, &linux)
        .map(|_| ())
        .and_then(|_| sync_bottle(&paths, "b1", &linux))
        .unwrap();
    assert_eq!(created, [linux.dir.join("vinter-b1-0.desktop")]);
    let entry = std::fs::read_to_string(&created[0]).unwrap();
    assert!(entry.contains("Name=7-Zip\n"));
    assert!(
        entry.contains("Exec=\"/opt/My Apps/vinter-app\" --run b1 0\n"),
        "{entry}"
    );
    assert!(entry.contains(&format!(
        "Icon={}",
        linux.icons_dir.join("vinter-b1-0.png").display()
    )));
    assert!(linux.icons_dir.join("vinter-b1-0.png").is_file());

    remove_bottle("b1", &linux).unwrap();
    assert!(!created[0].exists());
    assert!(!linux.icons_dir.join("vinter-b1-0.png").exists());
}

#[test]
fn pinning_a_missing_program_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let (mac, _) = targets(tmp.path());
    bottle_with_programs(&paths, "b1", "X", &["A"]);
    assert!(matches!(
        set_pinned(&paths, "b1", 3, true, &mac),
        Err(Error::ProgramNotFound(3))
    ));
}
