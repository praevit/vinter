//! Recipes: declarative setups for specific apps (plan section 4.4).
//!
//! Built-in recipes live in the repository's `recipes/` folder and are
//! compiled in; user recipes are `<data>/recipes/*.toml`, with their files
//! next to them. The format is the [`Recipe`] struct below; the built-in
//! recipes in `recipes/` are examples.

pub mod apply;
pub mod winetricks;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use apply::{ApplyOptions, RecipeProgress, RecipeState, apply, resume, state};

use crate::bottle::{self, Graphics, WindowsVersion};
use crate::error::validate_id;
use crate::runtime::Flavor;
use crate::{AppPaths, Error, Result};

/// Built-in recipes and the files they use, embedded at compile time.
const BUILTIN_RECIPES: &[&str] = &[
    include_str!("../../../../recipes/notepad-plus-plus.toml"),
    include_str!("../../../../recipes/7-zip.toml"),
    include_str!("../../../../recipes/putty.toml"),
    include_str!("../../../../recipes/foobar2000.toml"),
    include_str!("../../../../recipes/winrar.toml"),
    include_str!("../../../../recipes/sumatrapdf.toml"),
    include_str!("../../../../recipes/total-commander.toml"),
    include_str!("../../../../recipes/steam.toml"),
];
const BUILTIN_FILES: &[(&str, &[u8])] = &[(
    "fontsmooth-rgb.reg",
    include_bytes!("../../../../recipes/files/fontsmooth-rgb.reg"),
)];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// License of the app itself.
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub runtime_flavor: Option<Flavor>,
    #[serde(default)]
    pub windows_version: WindowsVersion,
    #[serde(default)]
    pub graphics: Graphics,
    #[serde(default)]
    pub installer: Option<Installer>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub programs: Vec<RecipeProgram>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Installer {
    /// The user picks the installer file.
    UserProvided { hint: String },
    /// Vinter downloads it and checks the sha256.
    Download {
        url: String,
        sha256: String,
        #[serde(default)]
        size: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Winetricks {
        verbs: Vec<String>,
    },
    DllOverride {
        dll: String,
        mode: String,
    },
    Env {
        vars: BTreeMap<String, String>,
    },
    Registry {
        file: String,
    },
    CopyFile {
        from: String,
        to: String,
    },
    RunInstaller {
        #[serde(default)]
        args: Vec<String>,
    },
    Run {
        exe: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl Step {
    /// Short description for progress and errors.
    pub fn label(&self) -> String {
        match self {
            Self::Winetricks { verbs } => {
                format!("Installing {} with winetricks", verbs.join(", "))
            }
            Self::DllOverride { dll, mode } => format!(
                "Setting {dll} to {}",
                if mode.is_empty() { "disabled" } else { mode }
            ),
            Self::Env { vars } => format!(
                "Setting {}",
                vars.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
            Self::Registry { file } => format!("Importing {file}"),
            Self::CopyFile { to, .. } => format!("Copying {to}"),
            Self::RunInstaller { .. } => "Running the installer".into(),
            Self::Run { exe, .. } => format!("Running {}", exe.rsplit('\\').next().unwrap_or(exe)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeProgram {
    pub name: String,
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Where a recipe came from (decides where its files are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    /// A user recipe file; its files are in the same folder.
    User(PathBuf),
}

#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    pub recipe: Recipe,
    pub origin: Origin,
}

impl LoadedRecipe {
    /// Bytes of a file used by a `registry` or `copy_file` step.
    pub fn file(&self, name: &str) -> Result<Vec<u8>> {
        validate_id(name)
            .map_err(|_| Error::InvalidRecipe(format!("{name:?} is not a plain file name")))?;
        match &self.origin {
            Origin::Builtin => BUILTIN_FILES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, bytes)| bytes.to_vec())
                .ok_or_else(|| Error::InvalidRecipe(format!("file {name:?} is not built in"))),
            Origin::User(path) => {
                let file = path.parent().unwrap_or(path).join(name);
                std::fs::read(&file).map_err(|e| {
                    Error::InvalidRecipe(format!("cannot read {}: {e}", file.display()))
                })
            }
        }
    }

    pub fn winetricks_verbs(&self) -> Vec<String> {
        self.recipe
            .steps
            .iter()
            .flat_map(|s| match s {
                Step::Winetricks { verbs } => verbs.clone(),
                _ => Vec::new(),
            })
            .collect()
    }
}

/// What the "Install an App" list shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RecipeSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub builtin: bool,
    /// "download", "user_provided" or "none".
    pub installer_kind: String,
    /// For user-provided installers: what to download.
    pub installer_hint: Option<String>,
    /// Download size of the installer, if known.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub download_size: Option<u64>,
    /// Winetricks verbs that need the user's consent.
    pub winetricks_verbs: Vec<String>,
    pub graphics: Graphics,
    pub step_count: u32,
}

/// A user recipe that could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BrokenRecipe {
    pub file: String,
    pub error: String,
}

impl LoadedRecipe {
    pub fn summary(&self) -> RecipeSummary {
        let r = &self.recipe;
        let (installer_kind, installer_hint, download_size) = match &r.installer {
            Some(Installer::Download { size, .. }) => ("download", None, *size),
            Some(Installer::UserProvided { hint }) => ("user_provided", Some(hint.clone()), None),
            None => ("none", None, None),
        };
        RecipeSummary {
            id: r.id.clone(),
            name: r.name.clone(),
            description: r.description.clone(),
            homepage: r.homepage.clone(),
            license: r.license.clone(),
            builtin: self.origin == Origin::Builtin,
            installer_kind: installer_kind.into(),
            installer_hint,
            download_size,
            winetricks_verbs: self.winetricks_verbs(),
            graphics: r.graphics,
            step_count: r.steps.len() as u32,
        }
    }
}

/// Parses and validates a recipe. Errors list every problem found.
pub fn parse(text: &str, origin: Origin) -> Result<LoadedRecipe> {
    let recipe: Recipe = toml::from_str(text).map_err(|e| Error::InvalidRecipe(e.to_string()))?;
    let loaded = LoadedRecipe { recipe, origin };
    validate(&loaded)?;
    Ok(loaded)
}

fn validate(loaded: &LoadedRecipe) -> Result<()> {
    let r = &loaded.recipe;
    let mut problems = Vec::new();
    let mut check = |ok: bool, msg: String| {
        if !ok {
            problems.push(msg);
        }
    };

    check(
        validate_id(&r.id).is_ok(),
        format!("id {:?}: use only letters, digits, '.', '_' and '-'", r.id),
    );
    check(!r.name.trim().is_empty(), "name is empty".into());
    if let Some(Installer::Download { url, sha256, .. }) = &r.installer {
        check(
            url.starts_with("https://"),
            format!("installer url must start with https://: {url}"),
        );
        check(
            sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_hexdigit()),
            "installer sha256 must be 64 hex characters".into(),
        );
    }
    if let Some(Installer::UserProvided { hint }) = &r.installer {
        check(
            !hint.trim().is_empty(),
            "user_provided installer needs a hint".into(),
        );
    }
    let windows_path_ok = |p: &str| {
        p.get(..3).is_some_and(|d| d.eq_ignore_ascii_case("C:\\"))
            && !p.split('\\').any(|c| c == "..")
    };

    for (i, step) in r.steps.iter().enumerate() {
        let at = format!("step {} ({})", i + 1, step_type(step));
        match step {
            Step::Winetricks { verbs } => {
                check(!verbs.is_empty(), format!("{at}: no verbs"));
                for v in verbs {
                    check(
                        !v.is_empty()
                            && v.chars().all(|c| {
                                c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '.')
                            }),
                        format!("{at}: {v:?} is not a winetricks verb"),
                    );
                }
            }
            Step::DllOverride { dll, mode } => {
                if let Err(e) = bottle::validate_override(dll, mode) {
                    check(false, format!("{at}: {e}"));
                }
            }
            Step::Env { vars } => {
                check(!vars.is_empty(), format!("{at}: no variables"));
                for (k, v) in vars {
                    if let Err(e) = bottle::validate_env_var(k, v) {
                        check(false, format!("{at}: {e}"));
                    }
                }
            }
            Step::Registry { file } => {
                check(
                    file.to_ascii_lowercase().ends_with(".reg"),
                    format!("{at}: {file:?} is not a .reg file"),
                );
                check(
                    loaded.file(file).is_ok(),
                    format!("{at}: file {file:?} not found"),
                );
            }
            Step::CopyFile { from, to } => {
                check(
                    loaded.file(from).is_ok(),
                    format!("{at}: file {from:?} not found"),
                );
                check(
                    windows_path_ok(to),
                    format!("{at}: `to` must be a path on C:, like C:\\users\\Public\\x"),
                );
            }
            Step::RunInstaller { .. } => {
                check(
                    r.installer.is_some(),
                    format!("{at}: the recipe has no [installer]"),
                );
            }
            Step::Run { exe, .. } => {
                check(
                    windows_path_ok(exe),
                    format!("{at}: exe must be a path on C:"),
                );
            }
        }
    }
    for p in &r.programs {
        check(!p.name.trim().is_empty(), "a program has no name".into());
        check(
            windows_path_ok(&p.exe),
            format!("program {:?}: exe must be a path on C:", p.name),
        );
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidRecipe(format!(
            "{}: {}",
            r.id,
            problems.join("; ")
        )))
    }
}

fn step_type(step: &Step) -> &'static str {
    match step {
        Step::Winetricks { .. } => "winetricks",
        Step::DllOverride { .. } => "dll_override",
        Step::Env { .. } => "env",
        Step::Registry { .. } => "registry",
        Step::CopyFile { .. } => "copy_file",
        Step::RunInstaller { .. } => "run_installer",
        Step::Run { .. } => "run",
    }
}

/// The folder for user recipes.
pub fn user_dir(paths: &AppPaths) -> PathBuf {
    paths.recipes_dir()
}

/// All recipes (user recipes replace built-ins with the same id), sorted by
/// name, plus user recipe files that failed to load.
pub fn list(paths: &AppPaths) -> (Vec<LoadedRecipe>, Vec<BrokenRecipe>) {
    let mut recipes: Vec<LoadedRecipe> = BUILTIN_RECIPES
        .iter()
        .map(|text| parse(text, Origin::Builtin).expect("built-in recipes are valid"))
        .collect();
    let mut broken = Vec::new();
    if let Ok(entries) = std::fs::read_dir(user_dir(paths)) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for file in files {
            let result = std::fs::read_to_string(&file)
                .map_err(|e| Error::InvalidRecipe(e.to_string()))
                .and_then(|text| parse(&text, Origin::User(file.clone())));
            match result {
                Ok(r) => {
                    recipes.retain(|x| x.recipe.id != r.recipe.id);
                    recipes.push(r);
                }
                Err(e) => broken.push(BrokenRecipe {
                    file: file.display().to_string(),
                    error: e.to_string(),
                }),
            }
        }
    }
    recipes.sort_by_key(|r| r.recipe.name.to_lowercase());
    (recipes, broken)
}

pub fn find(paths: &AppPaths, id: &str) -> Result<LoadedRecipe> {
    list(paths)
        .0
        .into_iter()
        .find(|r| r.recipe.id == id)
        .ok_or_else(|| Error::RecipeNotFound(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_recipes_are_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let (recipes, broken) = list(&AppPaths::with_data_dir(tmp.path()));
        assert!(broken.is_empty());
        let ids: Vec<_> = recipes.iter().map(|r| r.recipe.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "7-zip",
                "foobar2000",
                "notepad-plus-plus",
                "putty",
                "steam",
                "sumatrapdf",
                "total-commander",
                "winrar"
            ]
        );
        for r in &recipes {
            // Built-ins never need Microsoft downloads (no winetricks).
            assert!(r.winetricks_verbs().is_empty(), "{}", r.recipe.id);
            assert!(!r.recipe.programs.is_empty());
        }
        let npp = recipes
            .iter()
            .find(|r| r.recipe.id == "notepad-plus-plus")
            .unwrap();
        assert_eq!(npp.summary().installer_kind, "download");
        assert_eq!(npp.file("fontsmooth-rgb.reg").unwrap()[..8], *b"REGEDIT4");
    }

    #[test]
    fn plan_example_parses() {
        // A recipe using every kind of step, with its registry file present.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("notepad-plus-plus.reg"), "REGEDIT4\n").unwrap();
        let text = r#"
id = "notepad-plus-plus"
name = "Notepad++"
description = "Text editor"
runtime_flavor = "stable"
windows_version = "win10"
graphics = "wined3d"

[installer]
kind = "user_provided"
hint = "Download the 64-bit installer from the official site."

[[steps]]
type = "winetricks"
verbs = ["corefonts"]

[[steps]]
type = "dll_override"
dll = "d3d9"
mode = "native,builtin"

[[steps]]
type = "registry"
file = "notepad-plus-plus.reg"

[[steps]]
type = "run_installer"
args = ["/S"]

[[programs]]
name = "Notepad++"
exe = 'C:\Program Files\Notepad++\notepad++.exe'
"#;
        let r = parse(text, Origin::User(tmp.path().join("npp.toml"))).unwrap();
        assert_eq!(r.recipe.steps.len(), 4);
        assert_eq!(r.winetricks_verbs(), ["corefonts"]);
        assert_eq!(r.summary().installer_kind, "user_provided");
    }

    #[test]
    fn clear_errors() {
        let user = Origin::User(PathBuf::from("/nowhere/x.toml"));
        // A typo in a key names the key and the line.
        let err = parse(
            "id = \"x\"\nname = \"X\"\ndescription = \"\"\nstpes = []\n",
            user.clone(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("stpes") && err.contains("line 4"), "{err}");

        // Every semantic problem is listed, with its step number.
        let text = r#"
id = "bad id"
name = "X"
description = ""
[[steps]]
type = "dll_override"
dll = "d3d11"
mode = "sometimes"
[[steps]]
type = "registry"
file = "../secret.reg"
[[steps]]
type = "run_installer"
[[steps]]
type = "winetricks"
verbs = ["corefonts; rm -rf ~"]
[[programs]]
name = "X"
exe = '/usr/bin/x'
"#;
        let err = parse(text, user).unwrap_err().to_string();
        for needle in [
            "id \"bad id\"",
            "step 1 (dll_override)",
            "step 2 (registry): file \"../secret.reg\" not found",
            "step 3 (run_installer): the recipe has no [installer]",
            "step 4 (winetricks)",
            "program \"X\": exe must be a path on C:",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in {err}");
        }

        let download = "id = \"x\"\nname = \"X\"\ndescription = \"\"\n[installer]\nkind = \"download\"\nurl = \"http://a\"\nsha256 = \"abc\"\n";
        let err = parse(download, Origin::Builtin).unwrap_err().to_string();
        assert!(err.contains("https://") && err.contains("64 hex"), "{err}");
    }

    #[test]
    fn user_recipes_override_builtins_and_broken_ones_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        std::fs::create_dir_all(user_dir(&paths)).unwrap();
        std::fs::write(
            user_dir(&paths).join("mine.toml"),
            "id = \"7-zip\"\nname = \"My 7-Zip\"\ndescription = \"custom\"\n",
        )
        .unwrap();
        std::fs::write(user_dir(&paths).join("broken.toml"), "id = ").unwrap();
        let (recipes, broken) = list(&paths);
        let seven = recipes.iter().find(|r| r.recipe.id == "7-zip").unwrap();
        assert_eq!(seven.recipe.name, "My 7-Zip");
        assert!(!seven.summary().builtin);
        assert_eq!(broken.len(), 1);
        assert!(broken[0].file.ends_with("broken.toml"));
        assert!(matches!(
            find(&paths, "nope"),
            Err(Error::RecipeNotFound(_))
        ));
    }
}
