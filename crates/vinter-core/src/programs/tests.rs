use std::path::Path;

use super::*;
use crate::Error;
use crate::bottle::{Graphics, WindowsVersion};

/// Builds a minimal MS-SHLLINK `.lnk` pointing at `target` (a Windows path)
/// with `args`: header, LinkInfo with a local base path, Unicode arguments.
fn make_lnk(path: &Path, target: &str, args: &str) {
    const HAS_LINK_INFO: u32 = 0x02;
    const HAS_ARGUMENTS: u32 = 0x20;
    const IS_UNICODE: u32 = 0x80;

    let mut out = Vec::new();
    // ShellLinkHeader (76 bytes)
    out.extend(0x4Cu32.to_le_bytes());
    out.extend([
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ]);
    out.extend((HAS_LINK_INFO | HAS_ARGUMENTS | IS_UNICODE).to_le_bytes());
    out.extend(0x20u32.to_le_bytes()); // FILE_ATTRIBUTE_ARCHIVE
    out.extend([0u8; 24]); // creation, access, write times
    out.extend(0u32.to_le_bytes()); // file size
    out.extend(0i32.to_le_bytes()); // icon index
    out.extend(1u32.to_le_bytes()); // SW_SHOWNORMAL
    out.extend([0u8; 2 + 2 + 4 + 4]); // hotkey, reserved
    assert_eq!(out.len(), 76);

    // LinkInfo: header, VolumeID, LocalBasePath, CommonPathSuffix
    let mut volume = Vec::new();
    volume.extend(0x11u32.to_le_bytes()); // size
    volume.extend(3u32.to_le_bytes()); // DRIVE_FIXED
    volume.extend(0u32.to_le_bytes()); // serial
    volume.extend(0x10u32.to_le_bytes()); // label offset
    volume.push(0); // empty label
    let header_size = 0x1Cu32;
    let base_path_offset = header_size + volume.len() as u32;
    let mut base = target.as_bytes().to_vec();
    base.push(0);
    let suffix_offset = base_path_offset + base.len() as u32;
    let total = suffix_offset + 1;
    out.extend(total.to_le_bytes());
    out.extend(header_size.to_le_bytes());
    out.extend(1u32.to_le_bytes()); // VolumeIDAndLocalBasePath
    out.extend(header_size.to_le_bytes()); // VolumeID offset
    out.extend(base_path_offset.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // no network link
    out.extend(suffix_offset.to_le_bytes());
    out.extend(volume);
    out.extend(base);
    out.push(0); // empty common path suffix

    // StringData: COMMAND_LINE_ARGUMENTS (UTF-16LE, length-prefixed)
    let utf16: Vec<u16> = args.encode_utf16().collect();
    out.extend((utf16.len() as u16).to_le_bytes());
    for unit in utf16 {
        out.extend(unit.to_le_bytes());
    }
    // ExtraData terminal block
    out.extend(0u32.to_le_bytes());

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, out).unwrap();
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"MZ").unwrap();
}

/// A bottle with a hand-made prefix (no Wine needed).
fn fake_bottle() -> (tempfile::TempDir, AppPaths, String, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    let id = "test-bottle".to_string();
    let dir = bottle::dir(&paths, &id).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    Bottle {
        id: id.clone(),
        name: "Test".into(),
        runtime: "fake".into(),
        engine: crate::engine::EngineKind::legacy(),
        created_at: "2026-10-14T20:00:00Z".into(),
        windows_version: WindowsVersion::Win10,
        graphics: Graphics::Wined3d,
        recipe: None,
        env: Default::default(),
        dll_overrides: Default::default(),
        options: Default::default(),
        programs: vec![],
        domain: None,
    }
    .save(&dir)
    .unwrap();
    let drive_c = dir.join("prefix/drive_c");
    (tmp, paths, id, drive_c)
}

#[test]
fn discovers_shortcuts_then_unclaimed_program_folders() {
    let (_tmp, paths, id, c) = fake_bottle();
    // 7-Zip: has a Start Menu shortcut, so its other exes are not offered.
    touch(&c.join("Program Files/7-Zip/7zFM.exe"));
    touch(&c.join("Program Files/7-Zip/7z.exe"));
    touch(&c.join("Program Files/7-Zip/Uninstall.exe"));
    make_lnk(
        &c.join("ProgramData/Microsoft/Windows/Start Menu/Programs/7-Zip/7-Zip File Manager.lnk"),
        r"C:\Program Files\7-Zip\7zFM.exe",
        r#"--open "C:\My Files""#,
    );
    make_lnk(
        &c.join("ProgramData/Microsoft/Windows/Start Menu/Programs/7-Zip/Uninstall 7-Zip.lnk"),
        r"C:\Program Files\7-Zip\Uninstall.exe",
        "",
    );
    // A game without shortcuts: its exe is found, its crash reporter is not.
    touch(&c.join("Program Files (x86)/Game/game.exe"));
    touch(&c.join("Program Files (x86)/Game/bin/CrashReporter.exe"));
    // Wine's own stubs are never offered.
    touch(&c.join("Program Files/Windows NT/Accessories/wordpad.exe"));
    touch(&c.join("Program Files/Internet Explorer/iexplore.exe"));
    // Wine writes "no arguments" as a lone NUL; it must not become an argument.
    touch(&c.join("Program Files/Notepad++/notepad++.exe"));
    make_lnk(
        &c.join("ProgramData/Microsoft/Windows/Start Menu/Programs/Notepad++.lnk"),
        r"C:\Program Files\Notepad++\notepad++.exe",
        "\0",
    );
    // A desktop shortcut to a missing file is ignored.
    make_lnk(
        &c.join("users/Public/Desktop/Gone.lnk"),
        r"C:\Program Files\Gone\gone.exe",
        "",
    );

    let found = discover(&paths, &id).unwrap();
    let summary: Vec<(&str, &str, Source)> = found
        .iter()
        .map(|p| (p.name.as_str(), p.exe.as_str(), p.source))
        .collect();
    assert_eq!(
        summary,
        [
            (
                "7-Zip File Manager",
                r"C:\Program Files\7-Zip\7zFM.exe",
                Source::Shortcut
            ),
            (
                "Notepad++",
                r"C:\Program Files\Notepad++\notepad++.exe",
                Source::Shortcut
            ),
            (
                "game",
                r"C:\Program Files (x86)\Game\game.exe",
                Source::Executable
            ),
        ]
    );
    assert_eq!(found[0].args, ["--open", r"C:\My Files"]);
    assert!(found[1].args.is_empty(), "{:?}", found[1].args);
    assert!(found.iter().all(|p| !p.already_added && p.icon.is_none()));
}

#[test]
fn add_skips_duplicates_and_marks_added() {
    let (_tmp, paths, id, c) = fake_bottle();
    touch(&c.join("Program Files/App/app.exe"));
    let found = discover(&paths, &id).unwrap();
    assert_eq!(found.len(), 1);

    let def = found[0].to_def();
    add(&paths, &id, vec![def.clone()]).unwrap();
    let mut upper = def.clone();
    upper.exe = upper.exe.to_uppercase();
    let bottle = add(&paths, &id, vec![upper]).unwrap();
    assert_eq!(bottle.programs, vec![def.clone()]);
    assert!(discover(&paths, &id).unwrap()[0].already_added);

    let mut outside = def.clone();
    outside.exe = r"Z:\etc\passwd".into();
    assert!(matches!(
        add(&paths, &id, vec![outside]),
        Err(Error::InvalidProgram(_))
    ));
    let mut bad_icon = def.clone();
    bad_icon.icon = Some("../bottle.toml".into());
    assert!(matches!(
        add(&paths, &id, vec![bad_icon]),
        Err(Error::InvalidProgram(_))
    ));

    let bottle = remove(&paths, &id, 0).unwrap();
    assert!(bottle.programs.is_empty());
    assert!(matches!(
        remove(&paths, &id, 0),
        Err(Error::ProgramNotFound(0))
    ));
}

#[test]
fn helper_names_and_argument_splitting() {
    for name in [
        "unins000",
        "Uninstall",
        "AutoUpdater",
        "CrashReporter",
        "vc_redist.x64",
        "setup",
    ] {
        assert!(is_helper(name), "{name}");
    }
    for name in ["7zFM", "notepad++", "Game", "Steam"] {
        assert!(!is_helper(name), "{name}");
    }
    assert_eq!(split_args(r#"-a "b c"  d"#), ["-a", "b c", "d"]);
    assert!(split_args("   ").is_empty());
}

/// Installs a fake runtime whose `wine` prints its arguments and working
/// directory, then exits with the code in `$FAKE_EXIT` (default 0).
#[cfg(unix)]
fn fake_runtime(paths: &AppPaths) {
    use std::os::unix::fs::PermissionsExt;
    let dir = paths.runtimes_dir().join("fake");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let wine = dir.join("bin/wine");
    std::fs::write(
        &wine,
        "#!/bin/sh\nprintf 'args: %s\\n' \"$*\"\nprintf 'cwd: %s\\n' \"$(pwd)\" >&2\nexit 3\n",
    )
    .unwrap();
    std::fs::set_permissions(&wine, std::fs::Permissions::from_mode(0o755)).unwrap();
    let record = crate::runtime::RuntimeRecord {
        id: "fake".into(),
        name: "Fake".into(),
        version: "1".into(),
        flavor: crate::runtime::Flavor::Stable,
        url: "https://example.invalid".into(),
        sha256: "0".repeat(64),
        installed_at: "2026-10-14T20:00:00Z".into(),
        wine_bin: "bin/wine".into(),
        license: "LGPL-2.1-or-later".into(),
        source_url: "https://example.invalid".into(),
        arch: std::env::consts::ARCH.into(),
        graphics: vec![crate::bottle::Graphics::Wined3d],
        engine: Some(crate::engine::EngineKind::legacy()),
        fex_version: None,
    };
    std::fs::write(dir.join("runtime.toml"), toml::to_string(&record).unwrap()).unwrap();
}

#[cfg(unix)]
#[test]
fn launches_programs_in_their_folder_and_logs_output() {
    use std::sync::{Arc, Mutex};
    let (_tmp, paths, id, c) = fake_bottle();
    fake_runtime(&paths);
    touch(&c.join("Program Files/App/app.exe"));
    add(
        &paths,
        &id,
        vec![ProgramDef {
            name: "My App".into(),
            exe: r"C:\Program Files\App\app.exe".into(),
            // "\0" mimics entries saved before the NUL fix; it must be skipped.
            args: vec!["--fast".into(), "\0".into()],
            icon: None,
            pinned: false,
        }],
    )
    .unwrap();

    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let launched = launch(
        &paths,
        &id,
        0,
        move |_, line| sink.lock().unwrap().push(line.to_string()),
        move |code| tx.send(code).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
        Some(3)
    );
    let lines = lines.lock().unwrap().join("\n");
    assert!(
        lines.contains(r"args: C:\Program Files\App\app.exe --fast"),
        "{lines}"
    );
    assert!(lines.contains("Program Files/App"), "{lines}");
    assert_eq!(launched.name, "My App");

    let dir = bottle::dir(&paths, &id).unwrap();
    let log = process::logs::read(&dir, &launched.log_id).unwrap();
    assert!(log.contains("args:"));
    assert!(matches!(
        launch(&paths, &id, 5, |_, _| {}, |_| {}),
        Err(Error::ProgramNotFound(5))
    ));
}

#[cfg(unix)]
#[test]
fn runs_msi_installers_through_msiexec() {
    let (tmp, paths, id, _) = fake_bottle();
    fake_runtime(&paths);
    let msi = tmp.path().join("Setup Thing.msi");
    std::fs::write(&msi, b"").unwrap();
    let code = run_installer(&paths, &id, &msi, &["/quiet".into()], |_| {}).unwrap();
    assert_eq!(code, Some(3));
    let dir = bottle::dir(&paths, &id).unwrap();
    let logs = process::logs::list(&dir).unwrap();
    let text = process::logs::read(&dir, &logs[0].id).unwrap();
    // MSIs run from a short path inside the bottle (Wine's msiexec crashes on long ones).
    assert!(
        text.contains(r"args: msiexec /i C:\windows\Temp\vinter\Setup Thing.msi /quiet"),
        "{text}"
    );
    let staged = bottle::dir(&paths, &id)
        .unwrap()
        .join("prefix/drive_c/windows/Temp/vinter/Setup Thing.msi");
    assert!(!staged.exists(), "the staged copy is removed afterwards");
    assert!(matches!(
        run_installer(&paths, &id, &tmp.path().join("missing.exe"), &[], |_| {}),
        Err(Error::InstallerNotFound(_))
    ));
}

#[test]
fn icon_paths_cannot_escape_the_icons_folder() {
    let (_tmp, paths, id, _) = fake_bottle();
    assert!(icon_data_url(&paths, &id, "../bottle.toml").is_none());
    assert!(icon_data_url(&paths, &id, "icons/../../x.png").is_none());
    assert!(icon_data_url(&paths, &id, "icons/missing.png").is_none());
}

#[test]
fn update_renames_changes_args_and_icon() {
    let (tmp, paths, id, _c) = fake_bottle();
    add(
        &paths,
        &id,
        vec![ProgramDef {
            name: "App".into(),
            exe: r"C:\App\app.exe".into(),
            args: vec![],
            icon: None,
            pinned: false,
        }],
    )
    .unwrap();
    let image = tmp.path().join("logo.png");
    image::RgbaImage::new(32, 32).save(&image).unwrap();

    let b = update(
        &paths,
        &id,
        0,
        ProgramPatch {
            name: Some("  My App ".into()),
            args: Some(vec!["--fast".into(), "a b".into()]),
            icon_from: Some(image.display().to_string()),
        },
    )
    .unwrap();
    let p = &b.programs[0];
    assert_eq!(p.name, "My App");
    assert_eq!(p.args, ["--fast", "a b"]);
    let icon = p.icon.as_deref().unwrap();
    assert!(icon_data_url(&paths, &id, icon).is_some());

    // Nothing given: nothing changes.
    let same = update(&paths, &id, 0, ProgramPatch::default()).unwrap();
    assert_eq!(same.programs[0], *p);
    // Bad input is refused.
    let bad_name = ProgramPatch {
        name: Some("  ".into()),
        ..Default::default()
    };
    assert!(update(&paths, &id, 0, bad_name).is_err());
    assert!(update(&paths, &id, 5, ProgramPatch::default()).is_err());
}

#[test]
fn intel_programs_need_the_runtimes_fex_emulator() {
    use crate::engine::EngineKind;
    use crate::programs::arch::{PeArch, RunPath};

    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path());
    let rt_dir = paths.runtimes_dir().join("arm");
    std::fs::create_dir_all(rt_dir.join("bin")).unwrap();
    std::fs::write(
        rt_dir.join("runtime.toml"),
        "id = \"arm\"\nname = \"Arm Wine\"\nversion = \"11.10\"\nflavor = \"devel\"\nurl = \"\"\n\
         sha256 = \"\"\ninstalled_at = \"2026-10-23T00:00:00Z\"\nwine_bin = \"bin/wine\"\n\
         license = \"\"\nsource_url = \"\"\narch = \"aarch64\"\ngraphics = [\"wined3d\"]\n\
         engine = \"arm64-fex\"\n",
    )
    .unwrap();
    let rt = crate::runtime::get(&paths, "arm").unwrap();
    let bottle: Bottle = toml::from_str(
        "id = \"b\"\nname = \"Arm bottle\"\nruntime = \"arm\"\nengine = \"arm64-fex\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
    )
    .unwrap();
    let exe = tmp.path().join("app.exe");
    std::fs::write(&exe, crate::programs::arch::tests::pe(0x8664, true, false)).unwrap();

    // No libarm64ecfex.dll in the runtime: refused, saying why.
    let err = check_architecture("App", &exe, &bottle, &rt).unwrap_err();
    assert!(matches!(err, Error::WrongArchitecture { .. }), "{err}");
    assert!(err.to_string().contains("64-bit emulator"), "{err}");

    // With it: x86-64 through FEX.
    let pe = rt_dir.join("lib/wine/aarch64-windows");
    std::fs::create_dir_all(&pe).unwrap();
    std::fs::write(pe.join("libarm64ecfex.dll"), "").unwrap();
    assert_eq!(
        check_architecture("App", &exe, &bottle, &rt).unwrap(),
        Some((PeArch::Amd64, RunPath::Fex64))
    );
    assert_eq!(rt.engine, EngineKind::Arm64Fex);
}
