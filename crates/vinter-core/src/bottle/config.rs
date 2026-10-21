//! `bottle.toml`: a bottle's settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::IoResultExt;
use crate::{Error, Result};

pub const CONFIG_FILE: &str = "bottle.toml";

/// Windows version reported to programs, set with `winecfg /v`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum WindowsVersion {
    Win11,
    #[default]
    Win10,
    Win81,
    Win8,
    Win7,
    Winxp64,
}

impl WindowsVersion {
    pub const ALL: [Self; 6] = [
        Self::Win11,
        Self::Win10,
        Self::Win81,
        Self::Win8,
        Self::Win7,
        Self::Winxp64,
    ];

    /// The argument `winecfg /v` expects.
    pub fn winecfg_name(self) -> &'static str {
        match self {
            Self::Win11 => "win11",
            Self::Win10 => "win10",
            Self::Win81 => "win81",
            Self::Win8 => "win8",
            Self::Win7 => "win7",
            Self::Winxp64 => "winxp64",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.winecfg_name() == s)
    }
}

/// Graphics translation layer. Only stored in M2; applied in M4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Graphics {
    #[default]
    Wined3d,
    Dxvk,
    Dxmt,
    D3dmetal,
}

/// Per-bottle display, keyboard and performance switches. The default (all
/// off) is what bottles made before these options existed had.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BottleOptionsFile")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BottleOptions {
    /// macOS: draw at full Retina resolution (and at 192 DPI so text keeps its size).
    pub retina_mode: bool,
    /// macOS: ⌘ works as Ctrl (so ⌘C copies).
    pub command_as_ctrl: bool,
    /// macOS: ⌥ works as Alt. Off keeps ⌥ for typing characters such as @ on
    /// many European keyboards.
    pub option_as_alt: bool,
    /// Show a frame rate overlay in games (DXVK's and Metal's HUD).
    pub show_fps: bool,
}

/// [`BottleOptions`] as read: missing switches are off, except
/// `option_as_alt`, which follows `command_as_ctrl` (it was part of it).
#[derive(Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
struct BottleOptionsFile {
    retina_mode: bool,
    command_as_ctrl: bool,
    option_as_alt: Option<bool>,
    show_fps: bool,
}

impl From<BottleOptionsFile> for BottleOptions {
    fn from(f: BottleOptionsFile) -> Self {
        Self {
            retina_mode: f.retina_mode,
            command_as_ctrl: f.command_as_ctrl,
            option_as_alt: f.option_as_alt.unwrap_or(f.command_as_ctrl),
            show_fps: f.show_fps,
        }
    }
}

impl BottleOptions {
    /// For new bottles: the Mac-friendly settings on (macOS only).
    pub fn recommended() -> Self {
        let mac = cfg!(target_os = "macos");
        Self {
            retina_mode: mac,
            command_as_ctrl: mac,
            option_as_alt: mac,
            show_fps: false,
        }
    }
}

/// A program shown in a bottle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProgramDef {
    pub name: String,
    /// Windows path, e.g. `C:\Program Files\App\app.exe`.
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// PNG path relative to the bottle folder.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

/// Contents of `bottle.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BottleFile")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Bottle {
    pub id: String,
    pub name: String,
    /// Id of the runtime it runs with.
    pub runtime: String,
    /// RFC 3339 timestamp.
    pub created_at: String,
    pub windows_version: WindowsVersion,
    pub graphics: Graphics,
    /// Recipe that set it up, if any.
    pub recipe: Option<String>,
    /// Extra environment variables. Applied last, so they win.
    pub env: BTreeMap<String, String>,
    pub dll_overrides: BTreeMap<String, String>,
    pub options: BottleOptions,
    pub programs: Vec<ProgramDef>,
}

/// `bottle.toml` as read from disk: everything after `created_at` is optional.
/// Kept separate so the API type (and its generated TS) has no optional fields.
#[derive(Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
struct BottleFile {
    id: String,
    name: String,
    runtime: String,
    created_at: String,
    #[serde(default)]
    windows_version: WindowsVersion,
    #[serde(default)]
    graphics: Graphics,
    #[serde(default)]
    recipe: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    dll_overrides: BTreeMap<String, String>,
    #[serde(default)]
    options: BottleOptions,
    #[serde(default)]
    programs: Vec<ProgramDef>,
}

impl From<BottleFile> for Bottle {
    fn from(f: BottleFile) -> Self {
        Self {
            id: f.id,
            name: f.name,
            runtime: f.runtime,
            created_at: f.created_at,
            windows_version: f.windows_version,
            graphics: f.graphics,
            recipe: f.recipe,
            env: f.env,
            dll_overrides: f.dll_overrides,
            options: f.options,
            programs: f.programs,
        }
    }
}

impl Bottle {
    pub fn load(dir: &Path) -> Result<Self> {
        let file = dir.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&file).at(&file)?;
        toml::from_str(&text).map_err(|e| Error::Config {
            path: file,
            message: e.to_string(),
        })
    }

    /// Writes `bottle.toml` atomically (temp file, then rename).
    pub fn save(&self, dir: &Path) -> Result<()> {
        let file = dir.join(CONFIG_FILE);
        let tmp = dir.join(format!(".{CONFIG_FILE}.tmp"));
        let text = toml::to_string_pretty(self).expect("bottle config serializes");
        std::fs::write(&tmp, text).at(&tmp)?;
        std::fs::rename(&tmp, &file).at(&file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full example with every field set.
    const PLAN_EXAMPLE: &str = r#"
id = "8f2c"
name = "Office 2016"
runtime = "wine-stable-11.0-macos"
created_at = "2026-10-14T20:00:00Z"
windows_version = "win10"
graphics = "dxmt"
recipe = "office-2016"

[env]
WINEESYNC = "1"

[dll_overrides]
"d3d11" = "native,builtin"

[[programs]]
name = "Word"
exe = 'C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE'
args = []
icon = "icons/word.png"
pinned = true
"#;

    #[test]
    fn parses_plan_example_and_round_trips() {
        let bottle: Bottle = toml::from_str(PLAN_EXAMPLE).unwrap();
        assert_eq!(bottle.windows_version, WindowsVersion::Win10);
        assert_eq!(bottle.graphics, Graphics::Dxmt);
        assert_eq!(bottle.env["WINEESYNC"], "1");
        assert_eq!(bottle.dll_overrides["d3d11"], "native,builtin");
        assert_eq!(
            bottle.programs[0].exe,
            r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"
        );

        let tmp = tempfile::tempdir().unwrap();
        bottle.save(tmp.path()).unwrap();
        assert_eq!(Bottle::load(tmp.path()).unwrap(), bottle);
    }

    #[test]
    fn option_as_alt_follows_command_as_ctrl_in_older_files() {
        let old: BottleOptions = toml::from_str("command_as_ctrl = true").unwrap();
        assert!(old.option_as_alt);
        let old: BottleOptions = toml::from_str("command_as_ctrl = false").unwrap();
        assert!(!old.option_as_alt);
        let new: BottleOptions =
            toml::from_str("command_as_ctrl = true\noption_as_alt = false").unwrap();
        assert!(new.command_as_ctrl && !new.option_as_alt);
    }

    #[test]
    fn minimal_config_uses_defaults() {
        let bottle: Bottle = toml::from_str(
            "id = \"a\"\nname = \"A\"\nruntime = \"r\"\ncreated_at = \"2026-10-14T20:00:00Z\"\n",
        )
        .unwrap();
        assert_eq!(bottle.windows_version, WindowsVersion::Win10);
        assert_eq!(bottle.graphics, Graphics::Wined3d);
        assert!(bottle.programs.is_empty() && bottle.env.is_empty());
    }

    #[test]
    fn windows_versions_match_serde_names() {
        for v in WindowsVersion::ALL {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(json.trim_matches('"'), v.winecfg_name());
            assert_eq!(WindowsVersion::parse(v.winecfg_name()), Some(v));
        }
    }
}
