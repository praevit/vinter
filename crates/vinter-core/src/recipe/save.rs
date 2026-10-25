//! Saving a bottle as a recipe, and sharing it.
//!
//! The recipe repeats the bottle's setup: Windows version, graphics backend,
//! the components winetricks installed, DLL overrides, environment variables
//! and programs. The installer is one the user provides, since Vinter does
//! not know where the bottle's apps came from. Sharing only builds a link to
//! a new GitHub issue; the user sends it, if they want to.

use std::path::PathBuf;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;

use super::{Installer, Origin, Recipe, RecipeProgram, Step, list, parse, user_dir};
use crate::bottle::{self, Bottle};
use crate::compat::CompatNote;
use crate::error::IoResultExt;
use crate::{AppPaths, Result};

/// Where shared recipes go.
const ISSUES_URL: &str = "https://github.com/praevit/vinter/issues/new";
/// Browsers and GitHub refuse much longer links.
const MAX_URL: usize = 7500;

/// A recipe saved from a bottle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SavedRecipe {
    pub id: String,
    pub name: String,
    pub path: String,
    pub toml: String,
}

/// What the user chose in "Save as recipe".
#[derive(Debug, Clone, Default)]
pub struct SaveOptions {
    pub name: String,
    pub description: String,
    /// What installer to download (empty for a default hint).
    pub installer_hint: String,
}

/// The recipe for a bottle, with `id` free among the recipes there are.
pub fn from_bottle(paths: &AppPaths, bottle_id: &str, opts: &SaveOptions) -> Result<Recipe> {
    let b = bottle::get(paths, bottle_id)?;
    let verbs = crate::components::installed(paths, bottle_id)?;
    let flavor = crate::runtime::get(paths, &b.runtime)
        .ok()
        .map(|rt| rt.flavor);
    let taken: Vec<String> = list(paths).0.into_iter().map(|r| r.recipe.id).collect();
    let name = opts.name.trim();
    let name = if name.is_empty() {
        b.name.as_str()
    } else {
        name
    };
    Ok(build(&b, name, opts, verbs, flavor, &taken))
}

fn build(
    b: &Bottle,
    name: &str,
    opts: &SaveOptions,
    verbs: Vec<String>,
    flavor: Option<crate::runtime::Flavor>,
    taken: &[String],
) -> Recipe {
    let description = match opts.description.trim() {
        "" => format!("Saved from the Vinter bottle \u{201c}{}\u{201d}.", b.name),
        d => d.to_string(),
    };
    let hint = match opts.installer_hint.trim() {
        "" => format!("Download the installer for {name} from its website, then choose it here."),
        h => h.to_string(),
    };

    let mut steps = Vec::new();
    let verbs: Vec<String> = verbs.into_iter().filter(|v| is_plain_verb(v)).collect();
    if !verbs.is_empty() {
        steps.push(Step::Winetricks { verbs });
    }
    for (dll, mode) in &b.dll_overrides {
        if bottle::validate_override(dll, mode).is_ok() {
            steps.push(Step::DllOverride {
                dll: dll.clone(),
                mode: mode.clone(),
            });
        }
    }
    let vars: std::collections::BTreeMap<_, _> = b
        .env
        .iter()
        .filter(|(k, v)| bottle::validate_env_var(k, v).is_ok())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !vars.is_empty() {
        steps.push(Step::Env { vars });
    }
    steps.push(Step::RunInstaller { args: Vec::new() });

    Recipe {
        id: free_id(&slug(name), taken),
        name: name.to_string(),
        description,
        homepage: None,
        download_page: None,
        license: None,
        runtime_flavor: flavor,
        windows_version: b.windows_version,
        graphics: b.graphics,
        installer: Some(Installer::UserProvided { hint }),
        installer_names: Vec::new(),
        steps,
        // Recipes can only start programs on C:.
        programs: b
            .programs
            .iter()
            .filter(|p| is_on_c(&p.exe))
            .map(|p| RecipeProgram {
                name: p.name.clone(),
                exe: p.exe.clone(),
                args: p.args.clone(),
            })
            .collect(),
    }
}

/// The recipe as a TOML file, with a comment saying where it came from.
pub fn to_toml(recipe: &Recipe, bottle_name: &str) -> String {
    let body = toml::to_string_pretty(recipe).expect("a recipe serializes");
    let from = bottle_name.replace(['\n', '\r'], " ");
    format!("# Saved by Vinter from the bottle \"{from}\".\n{body}")
}

/// Writes the bottle's recipe into the user's recipes folder. It shows up
/// in Install an App right away.
pub fn save(paths: &AppPaths, bottle_id: &str, opts: &SaveOptions) -> Result<SavedRecipe> {
    let b = bottle::get(paths, bottle_id)?;
    let recipe = from_bottle(paths, bottle_id, opts)?;
    let text = to_toml(&recipe, &b.name);
    let dir = user_dir(paths);
    std::fs::create_dir_all(&dir).at(&dir)?;
    let file: PathBuf = dir.join(format!("{}.toml", recipe.id));
    // Make sure it loads before it lands in the folder.
    parse(&text, Origin::User(file.clone()))?;
    let tmp = dir.join(format!(".{}.toml.tmp", recipe.id));
    std::fs::write(&tmp, &text).at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)?;
    Ok(SavedRecipe {
        id: recipe.id,
        name: recipe.name,
        path: file.display().to_string(),
        toml: text,
    })
}

/// A link to a new GitHub issue with the recipe and the bottle's
/// compatibility notes filled in. Opening it sends nothing by itself.
pub fn share_url(name: &str, toml_text: &str, notes: &[CompatNote], version: &str) -> String {
    let title = format!("Recipe: {name}");
    let mut notes_text = String::new();
    for n in notes {
        notes_text.push_str(&format!(
            "- **{}**: {} ({}, {})",
            n.app,
            rating_label(n.rating),
            n.engine.label(),
            n.backend.name()
        ));
        if !n.note.is_empty() {
            notes_text.push_str(&format!(" \u{2014} {}", n.note.replace('\n', " ")));
        }
        notes_text.push('\n');
    }
    let body = |toml_text: &str| {
        let mut body =
            format!("A recipe saved with Vinter {version}.\n\n```toml\n{toml_text}```\n");
        if !notes_text.is_empty() {
            body.push_str(&format!("\n### How well it works\n\n{notes_text}"));
        }
        body
    };
    let url = |body: &str| {
        format!(
            "{ISSUES_URL}?title={}&body={}",
            utf8_percent_encode(&title, NON_ALPHANUMERIC),
            utf8_percent_encode(body, NON_ALPHANUMERIC)
        )
    };
    let full = url(&body(toml_text));
    if full.len() <= MAX_URL {
        return full;
    }
    // Too long for a link: keep the start and ask for the rest to be pasted.
    let mut cut = toml_text.to_string();
    while url(&body(&cut)).len() > MAX_URL && !cut.is_empty() {
        let keep = cut.len() * 3 / 4;
        let keep = (0..=keep)
            .rev()
            .find(|i| cut.is_char_boundary(*i))
            .unwrap_or(0);
        cut.truncate(keep);
    }
    cut.push_str("\n# (shortened: paste the rest of the file here)\n");
    url(&body(&cut))
}

fn rating_label(r: crate::compat::Rating) -> &'static str {
    match r {
        crate::compat::Rating::Works => "Works",
        crate::compat::Rating::Partly => "Partly works",
        crate::compat::Rating::Broken => "Broken",
    }
}

fn is_plain_verb(v: &str) -> bool {
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '.'))
}

fn is_on_c(path: &str) -> bool {
    path.get(..3)
        .is_some_and(|d| d.eq_ignore_ascii_case("C:\\"))
        && !path.split('\\').any(|c| c == "..")
}

/// "Notepad++ 8" → "notepad-8".
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(48).collect();
    if out.is_empty() {
        "my-recipe".into()
    } else {
        out
    }
}

/// `base`, or `base-2`, `base-3`… if taken. Built-in recipes count too, so a
/// saved recipe never replaces one by accident.
fn free_id(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.iter().any(|t| t == id))
        .expect("some id is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Graphics, ProgramDef, WindowsVersion};
    use crate::compat::Rating;
    use crate::engine::EngineKind;

    fn bottle() -> Bottle {
        let mut b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"Office \\\"old\\\"\"\nruntime = \"rt\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.windows_version = WindowsVersion::Win7;
        b.graphics = Graphics::Dxvk;
        b.env.insert("DXVK_HUD".into(), "fps".into());
        b.dll_overrides.insert("d3d11".into(), "n,b".into());
        b.dll_overrides.insert("mscoree".into(), String::new());
        b.programs.push(ProgramDef {
            name: "Word".into(),
            exe: r"C:\Program Files\Office\WINWORD.EXE".into(),
            args: vec!["/safe".into()],
            icon: None,
            pinned: true,
            arm64_build: None,
            ..Default::default()
        });
        b.programs.push(ProgramDef {
            name: "On the Mac".into(),
            exe: r"Z:\Users\me\tool.exe".into(),
            args: vec![],
            icon: None,
            pinned: false,
            arm64_build: None,
            ..Default::default()
        });
        b
    }

    #[test]
    fn round_trips_through_the_parser() {
        let b = bottle();
        let opts = SaveOptions::default();
        let verbs = vec!["vcrun2022".into(), "corefonts".into(), "bad verb".into()];
        let recipe = build(
            &b,
            "Office 2007",
            &opts,
            verbs,
            None,
            &["office-2007".into()],
        );
        assert_eq!(recipe.id, "office-2007-2");
        let text = to_toml(&recipe, &b.name);

        let tmp = tempfile::tempdir().unwrap();
        let loaded = parse(&text, Origin::User(tmp.path().join("x.toml"))).unwrap();
        assert_eq!(loaded.recipe, recipe);
        let r = &loaded.recipe;
        assert_eq!(r.windows_version, WindowsVersion::Win7);
        assert_eq!(r.graphics, Graphics::Dxvk);
        assert_eq!(loaded.winetricks_verbs(), ["vcrun2022", "corefonts"]);
        assert!(r.steps.contains(&Step::DllOverride {
            dll: "mscoree".into(),
            mode: String::new()
        }));
        assert!(matches!(r.steps.last(), Some(Step::RunInstaller { .. })));
        assert_eq!(r.programs.len(), 1, "only programs on C:");
        assert_eq!(r.programs[0].args, ["/safe"]);
        assert_eq!(loaded.summary().installer_kind, "user_provided");
        assert!(text.starts_with("# Saved by Vinter"));
    }

    #[test]
    fn saves_into_the_recipes_folder_without_replacing_builtins() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let d = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(d.join(bottle::PREFIX_DIR)).unwrap();
        std::fs::write(
            d.join(bottle::PREFIX_DIR).join("winetricks.log"),
            "d3dx9\nd3dx9\n",
        )
        .unwrap();
        bottle().save(&d).unwrap();

        let opts = SaveOptions {
            name: "Steam".into(),
            ..SaveOptions::default()
        };
        let saved = save(&paths, "b", &opts).unwrap();
        assert_eq!(saved.id, "steam-2");
        assert!(saved.path.ends_with("steam-2.toml"));
        let found = super::super::find(&paths, "steam-2").unwrap();
        assert_eq!(found.winetricks_verbs(), ["d3dx9"]);
        // The built-in one is still there.
        assert!(
            super::super::find(&paths, "steam")
                .unwrap()
                .summary()
                .builtin
        );
        // Saving again picks the next id.
        assert_eq!(save(&paths, "b", &opts).unwrap().id, "steam-3");
    }

    #[test]
    fn share_link_has_the_recipe_and_notes() {
        let note = CompatNote {
            bottle_id: "b".into(),
            app: "Word".into(),
            exe: r"C:\x.exe".into(),
            rating: Rating::Partly,
            engine: EngineKind::Arm64Fex,
            backend: Graphics::Dxvk,
            note: "menus flicker".into(),
            updated_at: String::new(),
        };
        let url = share_url("Word & co", "id = \"w\"\n", &[note], "1.0");
        assert!(url.starts_with(
            "https://github.com/praevit/vinter/issues/new?title=Recipe%3A%20Word%20%26%20co&body="
        ));
        let body = percent_encoding::percent_decode_str(url.split("&body=").nth(1).unwrap())
            .decode_utf8()
            .unwrap();
        assert!(body.contains("```toml\nid = \"w\"\n```"), "{body}");
        assert!(body.contains("**Word**: Partly works"), "{body}");
        assert!(body.contains("menus flicker"), "{body}");

        let long = "x = 1\n".repeat(5000);
        let url = share_url("Big", &long, &[], "1.0");
        assert!(url.len() <= MAX_URL);
        assert!(url.contains("shortened"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Notepad++ 8"), "notepad-8");
        assert_eq!(slug("  ---  "), "my-recipe");
        assert_eq!(slug("Ünïcode App"), "n-code-app");
    }
}
