//! Installer files: which recipe one belongs to (from its file name), a
//! bottle name for one, and installers that just arrived in a folder such as
//! ~/Downloads.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Installer, LoadedRecipe, list};
use crate::AppPaths;

/// File types Vinter can run as an installer.
const INSTALLER_EXTENSIONS: &[&str] = &["exe", "msi", "msix", "msixbundle", "appx", "appxbundle"];

/// An `installer_names` entry: a file name (no folders) with `*` and `?`.
pub(super) fn valid_pattern(pattern: &str) -> bool {
    !pattern.trim().is_empty() && !pattern.contains(['/', '\\']) && pattern.contains('.')
}

/// Matches `name` against `pattern` (`*` is any run of characters, `?` one
/// character), ignoring case.
fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // Where the last `*` was, and the name position it matched up to.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

impl LoadedRecipe {
    /// The file name looks like this app's official installer.
    pub fn matches_installer(&self, file_name: &str) -> bool {
        self.recipe.installer.as_ref().is_some_and(|_| {
            self.recipe
                .installer_names
                .iter()
                .any(|p| glob(p, file_name))
        })
    }

    /// A file in a download folder that could be this recipe's installer:
    /// one matching `installer_names`, or, if it has none, any installer.
    fn could_be_installer(&self, file_name: &str) -> bool {
        if self.recipe.installer_names.is_empty() {
            is_installer_file(file_name)
        } else {
            self.matches_installer(file_name)
        }
    }
}

fn is_installer_file(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            INSTALLER_EXTENSIONS
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        })
}

/// The recipe whose official installer has this file name, if any.
pub fn match_installer(paths: &AppPaths, file_name: &str) -> Option<LoadedRecipe> {
    list(paths)
        .0
        .into_iter()
        .find(|r| r.matches_installer(file_name))
}

/// The user's Downloads folder, where browsers save installers.
pub fn downloads_folder() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf))
}

/// Installers for `recipe` in `dir` changed at or after `since`, newest
/// first. Unfinished downloads (.crdownload, .part, .download) do not count,
/// since their name does not end in an installer extension.
pub fn recent_downloads(dir: &Path, recipe: &LoadedRecipe, since: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok()?;
            let changed = meta.modified().ok()?;
            (meta.is_file()
                && changed >= since
                && is_installer_file(&name)
                && recipe.could_be_installer(&name)
                && matches!(
                    recipe.recipe.installer,
                    Some(Installer::UserProvided { .. } | Installer::Download { .. })
                ))
            .then(|| (changed, e.path()))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, p)| p).collect()
}

/// A bottle name for an installer file: "npp.8.6.Installer.x64.exe" gives
/// "Npp", "SteamSetup (1).exe" gives "Steam". Version numbers, "setup",
/// "installer", architectures and download copy numbers are left out.
pub fn bottle_name_for(file_name: &str) -> String {
    const NOISE: &[&str] = &[
        "setup",
        "installer",
        "install",
        "x64",
        "x86",
        "win64",
        "win32",
        "64bit",
        "32bit",
        "amd64",
        "arm64",
        "64",
        "32",
        "full",
        "web",
        "offline",
        "online",
        "latest",
        "release",
    ];
    let stem = Path::new(file_name)
        .file_stem()
        .map_or(file_name.into(), |s| s.to_string_lossy());
    // "SteamSetup" -> "Steam Setup", so "Setup" can be left out.
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        if c.is_uppercase() && prev.is_some_and(char::is_lowercase) {
            spaced.push(' ');
        }
        spaced.push(c);
        prev = Some(c);
    }
    let words: Vec<&str> = spaced
        .split([' ', '.', '_', '-', '(', ')', '[', ']'])
        .filter(|w| !w.is_empty())
        .filter(|w| !NOISE.iter().any(|n| w.eq_ignore_ascii_case(n)))
        // Versions and copy numbers: "8", "v2", "2604", "1158x64".
        .filter(|w| {
            let w = w.strip_prefix(['v', 'V']).unwrap_or(w);
            !w.starts_with(|c: char| c.is_ascii_digit())
        })
        .collect();
    let name = words
        .iter()
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = name.chars().take(64).collect();
    if name.trim().is_empty() {
        stem.chars().take(64).collect()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Origin, parse};

    #[test]
    fn globs() {
        assert!(glob("SteamSetup*.exe", "SteamSetup.exe"));
        assert!(glob("SteamSetup*.exe", "steamsetup (1).EXE"));
        assert!(glob(
            "npp.*.Installer.x64*.exe",
            "npp.8.9.8.1.Installer.x64.exe"
        ));
        assert!(!glob(
            "npp.*.Installer.x64*.exe",
            "npp.8.9.8.1.Installer.exe"
        ));
        assert!(glob("a?c.exe", "abc.exe"));
        assert!(!glob("a?c.exe", "ac.exe"));
        assert!(glob("*", ""));
        assert!(!glob("SteamSetup*.exe", "SteamSetup.exe.crdownload"));
    }

    #[test]
    fn builtin_recipes_match_their_installers() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let id = |file: &str| match_installer(&paths, file).map(|r| r.recipe.id);
        assert_eq!(id("SteamSetup.exe").as_deref(), Some("steam"));
        assert_eq!(
            id("npp.8.9.8.1.Installer.x64.exe").as_deref(),
            Some("notepad-plus-plus")
        );
        assert_eq!(id("7z2604-x64.exe").as_deref(), Some("7-zip"));
        assert_eq!(
            id("putty-64bit-0.85-installer.msi").as_deref(),
            Some("putty")
        );
        assert_eq!(id("winrar-x64-723.exe").as_deref(), Some("winrar"));
        assert_eq!(id("tcmd1158x64.exe").as_deref(), Some("total-commander"));
        assert_eq!(
            id("SumatraPDF-3.6.1-64-install.exe").as_deref(),
            Some("sumatrapdf")
        );
        assert_eq!(
            id("foobar2000-x64_v2.24.1.exe").as_deref(),
            Some("foobar2000")
        );
        assert_eq!(id("setup.exe"), None);
        assert_eq!(id("Firefox Installer.exe"), None);
    }

    #[test]
    fn bad_patterns_are_rejected() {
        let text = "id = \"x\"\nname = \"X\"\ndescription = \"\"\ninstaller_names = [\"../x.exe\", \"\"]\ndownload_page = \"http://x\"\n";
        let err = parse(text, Origin::Builtin).unwrap_err().to_string();
        assert!(
            err.contains("\"../x.exe\" is not a file name pattern"),
            "{err}"
        );
        assert!(err.contains("\"\" is not a file name pattern"), "{err}");
        assert!(
            err.contains("download_page must start with https://"),
            "{err}"
        );
    }

    #[test]
    fn finds_recent_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let steam = crate::recipe::find(&paths, "steam").unwrap();
        let dir = tmp.path().join("Downloads");
        std::fs::create_dir_all(&dir).unwrap();
        let since = SystemTime::now() - std::time::Duration::from_secs(60);
        for f in [
            "SteamSetup.exe",
            "SteamSetup.exe.crdownload",
            "other.exe",
            "notes.txt",
        ] {
            std::fs::write(dir.join(f), "MZ").unwrap();
        }
        assert_eq!(
            recent_downloads(&dir, &steam, since),
            [dir.join("SteamSetup.exe")]
        );
        // Older files are left out.
        let later = SystemTime::now() + std::time::Duration::from_secs(60);
        assert!(recent_downloads(&dir, &steam, later).is_empty());

        // A recipe without installer_names takes any installer.
        let text = "id = \"y\"\nname = \"Y\"\ndescription = \"\"\n[installer]\nkind = \"user_provided\"\nhint = \"Get it.\"\n";
        let any = parse(text, Origin::Builtin).unwrap();
        let mut found = recent_downloads(&dir, &any, since);
        found.sort();
        assert_eq!(found, [dir.join("SteamSetup.exe"), dir.join("other.exe")]);
        assert!(!any.matches_installer("other.exe"));
    }

    #[test]
    fn bottle_names_from_files() {
        assert_eq!(bottle_name_for("SteamSetup.exe"), "Steam");
        assert_eq!(bottle_name_for("SteamSetup (1).exe"), "Steam");
        assert_eq!(bottle_name_for("npp.8.9.8.1.Installer.x64.exe"), "Npp");
        assert_eq!(bottle_name_for("7z2604-x64.exe"), "7z2604-x64");
        assert_eq!(bottle_name_for("Firefox Installer.exe"), "Firefox");
        assert_eq!(bottle_name_for("GOG_Galaxy_2.0.exe"), "GOG Galaxy");
        assert_eq!(bottle_name_for("foobar2000-x64_v2.24.1.exe"), "Foobar2000");
        assert_eq!(bottle_name_for("putty-64bit-0.85-installer.msi"), "Putty");
        assert_eq!(bottle_name_for("setup.exe"), "setup");
        assert_eq!(bottle_name_for("EpicInstaller-15.17.1.msi"), "Epic");
    }
}
