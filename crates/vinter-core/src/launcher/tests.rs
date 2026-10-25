use super::*;
use crate::bottle::{Graphics, ProgramDef, WindowsVersion};

fn bottle_with_programs(paths: &AppPaths, id: &str, name: &str, programs: &[&str]) {
    let dir = bottle::dir(paths, id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    Bottle {
        id: id.into(),
        name: name.into(),
        runtime: "rt".into(),
        engine: crate::engine::EngineKind::legacy(),
        created_at: "2026-10-15T00:00:00Z".into(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        options: Default::default(),
        programs: programs
            .iter()
            .map(|n| ProgramDef {
                id: format!("id-{}", n.to_lowercase()),
                name: n.to_string(),
                exe: format!(r"C:\Program Files\{n}\{n}.exe"),
                args: vec![],
                icon: None,
                pinned: false,
                arm64_build: None,
                ..Default::default()
            })
            .collect(),
        domain: None,
        drives: Vec::new(),
        hide_root_drive: false,
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
    let run = |program: ProgramRef, files: Vec<PathBuf>| {
        Some(DeepLink::Run {
            bottle: "abc-1".into(),
            program,
            files,
        })
    };
    assert_eq!(deep_link("abc-1", "p7f3"), "vinter://run/abc-1/p7f3");
    assert_eq!(
        parse_deep_link("vinter://run/abc-1/p7f3"),
        run(ProgramRef::Id("p7f3".into()), vec![])
    );
    // Launchers made before program ids used indexes.
    assert_eq!(
        parse_deep_link("vinter://run/abc-1/2/"),
        run(ProgramRef::Index(2), vec![])
    );
    let files = vec![
        PathBuf::from("/Users/me/My Docs/report & notes.docx"),
        PathBuf::from("/tmp/ü.txt"),
    ];
    let url = deep_link_with_files("abc-1", "p7f3", &files);
    assert!(url.starts_with("vinter://run/abc-1/p7f3?file=/Users/me/My%20Docs/report%20%26"));
    assert_eq!(
        parse_deep_link(&url),
        run(ProgramRef::Id("p7f3".into()), files)
    );
    assert_eq!(
        parse_deep_link("vinter://stop/abc-1/p7f3"),
        Some(DeepLink::Stop {
            bottle: "abc-1".into(),
            program: ProgramRef::Id("p7f3".into())
        })
    );
    assert_eq!(
        parse_deep_link("vinter://open-bottle/abc-1/"),
        Some(DeepLink::OpenBottle {
            bottle: "abc-1".into()
        })
    );
    for bad in [
        "vinter://run/../2",
        "vinter://run/a/.x",
        "vinter://run/a/1/x",
        "vinter://run/a/b?file=relative/path",
        "https://x/run/a/1",
        "vinter://open/a/1",
        "vinter://stop/a",
        "vinter://open-bottle/a/b",
    ] {
        assert_eq!(parse_deep_link(bad), None, "{bad}");
    }
}

#[test]
fn program_refs_resolve() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    bottle_with_programs(&paths, "b1", "Office", &["Word", "Excel"]);
    let bottle = bottle::get(&paths, "b1").unwrap();
    assert_eq!(ProgramRef::Id("id-excel".into()).resolve(&bottle), Some(1));
    assert_eq!(ProgramRef::Index(0).resolve(&bottle), Some(0));
    assert_eq!(ProgramRef::Index(2).resolve(&bottle), None);
    assert_eq!(ProgramRef::Id("gone".into()).resolve(&bottle), None);
}

#[test]
fn file_types_are_normalized() {
    let typed = [
        "DOCX, .txt  md".to_string(),
        "*.Rtf;docx".into(),
        "a/b".into(),
    ];
    assert_eq!(normalize_file_types(&typed), ["docx", "txt", "md", "rtf"]);
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
        plist.contains("<string>vinter://run/b1/id-excel</string>"),
        "{plist}"
    );
    assert!(plist.contains("<key>LSUIElement</key>"));
    let script = std::fs::read_to_string(app.join("Contents/MacOS/launch")).unwrap();
    assert!(
        script.ends_with("exec /usr/bin/open \"vinter://run/b1/id-excel\"\n"),
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

    // Removing Word shifts Excel to index 0; its launcher keeps its id.
    crate::programs::remove(&paths, "b1", 0).unwrap();
    sync_bottle(&paths, "b1", &mac).unwrap();
    let plist = std::fs::read_to_string(mac.dir.join("Excel.app/Contents/Info.plist")).unwrap();
    assert!(plist.contains("vinter://run/b1/id-excel"), "{plist}");

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
    assert_eq!(created, [linux.dir.join("vinter-b1-id-7-zip.desktop")]);
    let entry = std::fs::read_to_string(&created[0]).unwrap();
    assert!(entry.contains("Name=7-Zip\n"));
    assert!(
        entry.contains("Exec=\"/opt/My Apps/vinter-app\" --run b1 id-7-zip\n"),
        "{entry}"
    );
    assert!(entry.contains(&format!(
        "Icon={}",
        linux.icons_dir.join("vinter-b1-id-7-zip.png").display()
    )));
    assert!(linux.icons_dir.join("vinter-b1-id-7-zip.png").is_file());

    remove_bottle("b1", &linux).unwrap();
    assert!(!created[0].exists());
    assert!(!linux.icons_dir.join("vinter-b1-id-7-zip.png").exists());
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

#[test]
fn launchers_for_programs_that_open_files() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let (mac, linux) = targets(tmp.path());
    bottle_with_programs(&paths, "b1", "Office", &["Word"]);
    let dir = bottle::dir(&paths, "b1").unwrap();
    let mut b = bottle::get(&paths, "b1").unwrap();
    b.programs[0].file_types = vec!["docx".into(), "xyz".into()];
    b.programs[0].pinned = true;
    b.save(&dir).unwrap();

    let created = sync_bottle(&paths, "b1", &linux).unwrap();
    let entry = std::fs::read_to_string(&created[0]).unwrap();
    assert!(entry.contains(" --run b1 id-word %F\n"), "{entry}");
    assert!(
        entry.contains(
            "MimeType=application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/x-extension-xyz;\n"
        ),
        "{entry}"
    );

    // macOS: an AppleScript droplet that lists the types, or where
    // `osacompile` is missing, the plain script launcher without them.
    let created = sync_bottle(&paths, "b1", &mac).unwrap();
    let plist = std::fs::read_to_string(created[0].join("Contents/Info.plist")).unwrap();
    let droplet = created[0]
        .join("Contents/Resources/Scripts/main.scpt")
        .is_file();
    assert_eq!(plist.contains("CFBundleDocumentTypes"), droplet, "{plist}");
    if droplet {
        assert!(plist.contains("<string>docx</string>"), "{plist}");
        assert!(plist.contains("vinter://run/b1/id-word"), "{plist}");
    }
    assert_eq!(macos::find_for_bottle(&mac.dir, "b1"), created);
}

#[test]
fn launchers_made_with_indexes_are_upgraded() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let (mac, linux) = targets(tmp.path());
    bottle_with_programs(&paths, "b1", "Office", &["Word"]);
    set_pinned(&paths, "b1", 0, true, &mac).unwrap();
    sync_bottle(&paths, "b1", &linux).unwrap();
    assert!(upgrade_legacy(&paths, &mac).unwrap().is_empty());
    assert!(upgrade_legacy(&paths, &linux).unwrap().is_empty());

    // What older versions wrote.
    let plist = mac.dir.join("Word.app/Contents/Info.plist");
    let old = std::fs::read_to_string(&plist)
        .unwrap()
        .replace("VinterProgramID", "VinterProgramIndex");
    std::fs::write(&plist, old).unwrap();
    std::fs::rename(
        linux.dir.join("vinter-b1-id-word.desktop"),
        linux.dir.join("vinter-b1-0.desktop"),
    )
    .unwrap();

    assert_eq!(upgrade_legacy(&paths, &mac).unwrap(), ["b1"]);
    assert!(!macos::is_legacy(&mac.dir.join("Word.app")));
    assert_eq!(upgrade_legacy(&paths, &linux).unwrap(), ["b1"]);
    assert!(linux.dir.join("vinter-b1-id-word.desktop").is_file());
    assert!(!linux.dir.join("vinter-b1-0.desktop").exists());
}
