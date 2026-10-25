//! App-wide settings in `settings.toml`.

use serde::{Deserialize, Serialize};

use crate::bottle::BottleOptions;
use crate::error::IoResultExt;
use crate::experimental::ExperimentalSettings;
use crate::{AppPaths, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SettingsFile")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Settings {
    /// Options new bottles start with.
    pub new_bottle_options: BottleOptions,
    /// Runtime for new bottles, if not the newest installed one.
    pub default_runtime: Option<String>,
    /// Show the menu bar (tray) icon.
    pub menu_bar_icon: bool,
    /// Notify when a long job finishes or a program fails to start.
    pub notifications: bool,
    /// Check for a newer Vinter when it starts.
    pub check_for_updates: bool,
    /// Experimental features turned on (see [`crate::experimental`]).
    pub experimental: ExperimentalSettings,
    /// Light or dark windows, or the system's choice.
    pub appearance: Appearance,
}

/// How Vinter's windows look.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Appearance {
    /// Follows the system's light or dark mode.
    #[default]
    System,
    Light,
    Dark,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            new_bottle_options: BottleOptions::recommended(),
            default_runtime: None,
            menu_bar_icon: true,
            notifications: true,
            check_for_updates: true,
            experimental: ExperimentalSettings::default(),
            appearance: Appearance::System,
        }
    }
}

/// [`Settings`] as read: missing fields keep their defaults, so the API type
/// (and its generated TS) has no optional fields.
#[derive(Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
struct SettingsFile {
    new_bottle_options: BottleOptions,
    default_runtime: Option<String>,
    menu_bar_icon: bool,
    notifications: bool,
    check_for_updates: bool,
    experimental: ExperimentalSettings,
    appearance: Appearance,
}

impl Default for SettingsFile {
    fn default() -> Self {
        let d = Settings::default();
        Self {
            new_bottle_options: d.new_bottle_options,
            default_runtime: d.default_runtime,
            menu_bar_icon: d.menu_bar_icon,
            notifications: d.notifications,
            check_for_updates: d.check_for_updates,
            experimental: d.experimental,
            appearance: d.appearance,
        }
    }
}

impl From<SettingsFile> for Settings {
    fn from(f: SettingsFile) -> Self {
        Self {
            new_bottle_options: f.new_bottle_options,
            default_runtime: f.default_runtime,
            menu_bar_icon: f.menu_bar_icon,
            notifications: f.notifications,
            check_for_updates: f.check_for_updates,
            experimental: f.experimental,
            appearance: f.appearance,
        }
    }
}

/// The saved settings, or the defaults if there are none (or they are unreadable).
pub fn load(paths: &AppPaths) -> Settings {
    std::fs::read_to_string(paths.settings_file())
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes `settings.toml` atomically.
pub fn save(paths: &AppPaths, settings: &Settings) -> Result<()> {
    if let Some(id) = &settings.default_runtime {
        crate::runtime::get(paths, id)?;
    }
    let file = paths.settings_file();
    let dir = paths.data_dir();
    std::fs::create_dir_all(dir).at(dir)?;
    let tmp = dir.join(".settings.toml.tmp");
    let text = toml::to_string_pretty(settings).map_err(|e| Error::Config {
        path: file.clone(),
        message: e.to_string(),
    })?;
    std::fs::write(&tmp, text).at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        assert_eq!(load(&paths), Settings::default());

        let mut experimental = crate::experimental::ExperimentalSettings::default();
        experimental.set(crate::experimental::ExperimentalFlag::EngineAbCompare, true);
        let s = Settings {
            menu_bar_icon: false,
            appearance: Appearance::Dark,
            experimental,
            new_bottle_options: BottleOptions {
                show_fps: true,
                ..BottleOptions::recommended()
            },
            ..Settings::default()
        };
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths), s);
    }

    #[test]
    fn rejects_an_unknown_default_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let s = Settings {
            default_runtime: Some("nope".into()),
            ..Settings::default()
        };
        assert!(save(&paths, &s).is_err());
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let s: Settings = toml::from_str("notifications = false").unwrap();
        assert!(!s.notifications);
        assert!(s.menu_bar_icon);
        assert_eq!(s.appearance, Appearance::System);
    }
}
