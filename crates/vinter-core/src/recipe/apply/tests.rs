//! Recipe runs against a fake runtime whose `wine` simulates an installer.

use std::path::Path;

use super::*;
use crate::recipe::{Origin, parse};
use crate::runtime::{Flavor, RuntimeRecord};

/// `wine` that creates a prefix, records regedit imports, "installs"
/// `C:\Program Files\App\app.exe` when given an installer path, and records
/// other programs it runs. `helper.exe` fails until `helper-ok` exists.
const FAKE_WINE: &str = r#"#!/bin/sh
case "$1" in
  wineboot) mkdir -p "$WINEPREFIX/drive_c/windows/system32" "$WINEPREFIX/drive_c/users/Public" ;;
  winecfg) ;;
  regedit) printf '%s\n' "$3" >> "$WINEPREFIX/regedit.txt" ;;
  /*)
    mkdir -p "$WINEPREFIX/drive_c/Program Files/App"
    printf 'MZ' > "$WINEPREFIX/drive_c/Program Files/App/app.exe"
    printf '%s\n' "$*" >> "$WINEPREFIX/installer.txt" ;;
  *helper.exe)
    [ -f "$WINEPREFIX/helper-ok" ] || exit 7
    printf '%s\n' "$*" >> "$WINEPREFIX/ran.txt" ;;
  *) printf '%s\n' "$*" >> "$WINEPREFIX/ran.txt" ;;
esac
"#;

fn fake_runtime(paths: &AppPaths) {
    use std::os::unix::fs::PermissionsExt;
    let dir = paths.runtimes_dir().join("fake-stable");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    for (name, script) in [("wine", FAKE_WINE), ("wineserver", "#!/bin/sh\nexit 0\n")] {
        let p = dir.join("bin").join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let record = RuntimeRecord {
        id: "fake-stable".into(),
        name: "Fake Stable".into(),
        version: "11.0".into(),
        flavor: Flavor::Stable,
        url: String::new(),
        sha256: "0".repeat(64),
        installed_at: "2026-10-15T00:00:00Z".into(),
        wine_bin: "bin/wine".into(),
        license: String::new(),
        source_url: String::new(),
        arch: std::env::consts::ARCH.into(),
        graphics: vec![Graphics::Wined3d],
        engine: Some(crate::engine::EngineKind::legacy()),
        fex_version: None,
    };
    std::fs::write(dir.join("runtime.toml"), toml::to_string(&record).unwrap()).unwrap();
}

const RECIPE: &str = r#"
id = "app"
name = "App"
description = "Test app"
runtime_flavor = "stable"
windows_version = "win7"

[installer]
kind = "user_provided"
hint = "Get setup.exe from example.com."

[[steps]]
type = "registry"
file = "tweak.reg"

[[steps]]
type = "dll_override"
dll = "d3d9"
mode = "n,b"

[[steps]]
type = "env"
vars = { APP_MODE = "fast" }

[[steps]]
type = "copy_file"
from = "app.ini"
to = 'C:\users\Public\App\app.ini'

[[steps]]
type = "run_installer"
args = ["/S"]

[[steps]]
type = "run"
exe = 'C:\Program Files\App\helper.exe'
args = ["--init"]

[[programs]]
name = "The App"
exe = 'C:\Program Files\App\app.exe'
"#;

struct Fixture {
    tmp: tempfile::TempDir,
    paths: AppPaths,
    recipe: LoadedRecipe,
    installer: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_data_dir(tmp.path().join("data"));
    fake_runtime(&paths);
    // A user recipe, where Vinter looks for them (so resume can find it by id).
    let recipes = crate::recipe::user_dir(&paths);
    std::fs::create_dir_all(&recipes).unwrap();
    std::fs::write(recipes.join("tweak.reg"), "REGEDIT4\n").unwrap();
    std::fs::write(recipes.join("app.ini"), "[app]\nfast=1\n").unwrap();
    std::fs::write(recipes.join("app.toml"), RECIPE).unwrap();
    let recipe = parse(RECIPE, Origin::User(recipes.join("app.toml"))).unwrap();
    let installer = tmp.path().join("setup.exe");
    std::fs::write(&installer, "MZ").unwrap();
    Fixture {
        tmp,
        paths,
        recipe,
        installer,
    }
}

fn read(p: impl AsRef<Path>) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn options(installer: Option<PathBuf>) -> ApplyOptions {
    ApplyOptions {
        bottle_id: Some("recipe-bottle".into()),
        bottle_name: "My App".into(),
        installer,
        accept_downloads: false,
        runtime: None,
    }
}

#[tokio::test]
async fn fails_at_a_step_then_resumes_to_a_working_bottle() {
    let f = fixture();
    let mut progress = Vec::new();
    let err = apply(
        &f.paths,
        &f.recipe,
        options(Some(f.installer.clone())),
        |p| progress.push(p),
    )
    .await
    .unwrap_err();
    // The helper is missing, so step 6 fails; steps 1-5 are done and recorded.
    assert!(
        err.to_string().contains("step 6 (Running helper.exe)"),
        "{err}"
    );
    let st = state(&f.paths, "recipe-bottle").unwrap();
    assert_eq!((st.completed_steps, st.total_steps), (5, 6));
    assert!(st.error.unwrap().contains("helper.exe"));
    assert!(!st.needs_installer, "the installer path is remembered");
    assert!(progress.iter().any(|p| p.label == "Creating the bottle"));
    assert!(progress.iter().all(|p| p.task <= p.tasks));

    let prefix = bottle::dir(&f.paths, "recipe-bottle")
        .unwrap()
        .join(PREFIX_DIR);
    assert!(read(prefix.join("regedit.txt")).contains(r"Z:\"));
    assert!(read(prefix.join("installer.txt")).contains("setup.exe /S"));
    assert_eq!(
        read(prefix.join("drive_c/users/Public/App/app.ini")),
        "[app]\nfast=1\n"
    );
    let b = bottle::get(&f.paths, "recipe-bottle").unwrap();
    assert_eq!(b.dll_overrides["d3d9"], "n,b");
    assert_eq!(b.env["APP_MODE"], "fast");
    assert_eq!(b.windows_version, crate::bottle::WindowsVersion::Win7);
    assert!(b.recipe.is_none(), "not finished yet");

    // Fix the cause and resume: only step 6 runs again.
    std::fs::write(prefix.join("drive_c/Program Files/App/helper.exe"), "MZ").unwrap();
    std::fs::write(prefix.join("helper-ok"), "").unwrap();
    let b = resume(&f.paths, "recipe-bottle", None, false, |_| {})
        .await
        .unwrap();
    assert_eq!(b.recipe.as_deref(), Some("app"));
    assert_eq!(b.programs.len(), 1);
    assert_eq!(b.programs[0].name, "The App");
    assert_eq!(
        read(prefix.join("installer.txt")).lines().count(),
        1,
        "installer ran once"
    );
    assert!(read(prefix.join("ran.txt")).contains("helper.exe --init"));
    assert!(
        state(&f.paths, "recipe-bottle").is_none(),
        "state is cleared when done"
    );
    assert!(matches!(
        resume(&f.paths, "recipe-bottle", None, false, |_| {}).await,
        Err(Error::NothingToResume(_))
    ));
    drop(f.tmp);
}

#[tokio::test]
async fn user_provided_installer_is_required() {
    let f = fixture();
    let err = apply(&f.paths, &f.recipe, options(None), |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InstallerRequired(ref hint) if hint.contains("example.com")),
        "{err}"
    );
    let err = apply(
        &f.paths,
        &f.recipe,
        options(Some(f.tmp.path().join("missing.exe"))),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::InstallerNotFound(_)), "{err}");
    assert!(
        bottle::list(&f.paths).unwrap().is_empty(),
        "no bottle is created"
    );
}

#[tokio::test]
async fn winetricks_needs_consent() {
    let f = fixture();
    let text = RECIPE.replace(
        "[[steps]]\ntype = \"registry\"",
        "[[steps]]\ntype = \"winetricks\"\nverbs = [\"corefonts\"]\n\n[[steps]]\ntype = \"registry\"",
    );
    let recipe = parse(&text, f.recipe.origin.clone()).unwrap();
    let err = apply(
        &f.paths,
        &recipe,
        options(Some(f.installer.clone())),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, Error::ConsentRequired(ref v) if v == "corefonts"),
        "{err}"
    );
    assert!(bottle::list(&f.paths).unwrap().is_empty());
}

#[tokio::test]
async fn falls_back_to_shortcuts_when_listed_programs_are_missing() {
    let f = fixture();
    let text = RECIPE
        .replace("C:\\Program Files\\App\\app.exe'", "C:\\Program Files\\Elsewhere\\x.exe'")
        .replace("[[steps]]\ntype = \"run\"\nexe = 'C:\\Program Files\\App\\helper.exe'\nargs = [\"--init\"]\n", "");
    let recipe = parse(&text, f.recipe.origin.clone()).unwrap();
    let err = apply(
        &f.paths,
        &recipe,
        options(Some(f.installer.clone())),
        |_| {},
    )
    .await
    .unwrap_err();
    // No shortcut was created either, so this is a clear failure.
    assert!(
        err.to_string().contains("did not install any program"),
        "{err}"
    );
}

#[tokio::test]
async fn creates_a_bottle_for_an_installer() {
    let f = fixture();
    let mut progress = Vec::new();
    let bottle = create_for_installer(
        &f.paths,
        Some("for-installer".into()),
        "Steam".into(),
        |p| progress.push(p.label),
    )
    .await
    .unwrap();
    assert_eq!(bottle.id, "for-installer");
    assert_eq!(bottle.name, "Steam");
    assert_eq!(bottle.runtime, "fake-stable");
    // The runtime is installed already, so creating the bottle is the only task.
    assert_eq!(progress, ["Creating the bottle"]);
    assert!(bottle::list(&f.paths).unwrap()[0].setup_complete);
}
