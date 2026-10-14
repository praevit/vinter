//! Where Vinter keeps its data on disk.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Environment variable that overrides the data folder (tests, portable installs).
pub const DATA_DIR_ENV: &str = "VINTER_DATA_DIR";

/// Resolved locations of Vinter's data folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    /// Resolves the platform data folder:
    /// `~/Library/Application Support/Vinter/` on macOS and
    /// `$XDG_DATA_HOME/vinter/` (usually `~/.local/share/vinter/`) on Linux.
    /// `VINTER_DATA_DIR` takes precedence when set and non-empty.
    pub fn discover() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|d| !d.is_empty()) {
            return Ok(Self::with_data_dir(dir));
        }
        let base = directories::BaseDirs::new().ok_or(Error::NoHomeDir)?;
        let folder = if cfg!(target_os = "macos") {
            "Vinter"
        } else {
            "vinter"
        };
        Ok(Self::with_data_dir(base.data_dir().join(folder)))
    }

    /// Uses an explicit data folder.
    pub fn with_data_dir(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.data_dir.join("runtimes")
    }

    pub fn bottles_dir(&self) -> PathBuf {
        self.data_dir.join("bottles")
    }

    pub fn recipes_dir(&self) -> PathBuf {
        self.data_dir.join("recipes")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.data_dir.join("cache").join("downloads")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_plan() {
        let paths = AppPaths::with_data_dir("/data");
        assert_eq!(paths.data_dir(), Path::new("/data"));
        assert_eq!(paths.runtimes_dir(), Path::new("/data/runtimes"));
        assert_eq!(paths.bottles_dir(), Path::new("/data/bottles"));
        assert_eq!(paths.recipes_dir(), Path::new("/data/recipes"));
        assert_eq!(paths.downloads_dir(), Path::new("/data/cache/downloads"));
        assert_eq!(paths.settings_file(), Path::new("/data/settings.toml"));
    }

    #[test]
    fn discover_uses_platform_folder_name() {
        // Skip when the override is set in the environment running the tests.
        if std::env::var_os(DATA_DIR_ENV).is_some() {
            return;
        }
        let paths = AppPaths::discover().unwrap();
        let name = paths.data_dir().file_name().unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(name, "Vinter");
            assert!(
                paths
                    .data_dir()
                    .ends_with("Library/Application Support/Vinter")
            );
        } else {
            assert_eq!(name, "vinter");
        }
    }
}
