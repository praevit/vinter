//! `bottle.toml`: a bottle's settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::EngineKind;
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

/// How much Wine writes to a program's log (`WINEDEBUG`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum WineDebug {
    /// Nothing from Wine itself; the program's own output is still logged.
    Off,
    /// Wine's error messages. The default: the log doctor needs them (for
    /// example to name a missing DLL), and they are few.
    #[default]
    Errors,
    /// Errors, warnings and unimplemented functions, plus which DLLs load
    /// and the exceptions raised. Logs get large; for troubleshooting.
    Verbose,
}

impl WineDebug {
    pub const ALL: [Self; 3] = [Self::Off, Self::Errors, Self::Verbose];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Errors => "errors",
            Self::Verbose => "verbose",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.id() == s)
    }

    /// The `WINEDEBUG` value.
    pub fn env_value(self) -> &'static str {
        match self {
            Self::Off => "-all",
            Self::Errors => "-all,err+all",
            Self::Verbose => "err+all,warn+all,fixme+all,+loaddll,+seh",
        }
    }
}

/// Per-bottle display, keyboard and performance switches. The default (all
/// off, Wine errors logged) is what bottles made before these options existed get.
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
    /// How much Wine writes to the log.
    pub wine_debug: WineDebug,
}

/// [`BottleOptions`] as read: missing switches are off, except
/// `option_as_alt`, which follows `command_as_ctrl` (it was part of it), and
/// `wine_debug`, which logs errors.
#[derive(Deserialize, Default)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
struct BottleOptionsFile {
    retina_mode: bool,
    command_as_ctrl: bool,
    option_as_alt: Option<bool>,
    show_fps: bool,
    wine_debug: WineDebug,
}

impl From<BottleOptionsFile> for BottleOptions {
    fn from(f: BottleOptionsFile) -> Self {
        Self {
            retina_mode: f.retina_mode,
            command_as_ctrl: f.command_as_ctrl,
            option_as_alt: f.option_as_alt.unwrap_or(f.command_as_ctrl),
            show_fps: f.show_fps,
            wine_debug: f.wine_debug,
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
            wine_debug: WineDebug::Errors,
        }
    }
}

/// A Windows domain account the bottle's programs run as. The password is
/// not stored here but in the system keychain (see [`super::domain`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DomainAccount {
    /// Windows user name, e.g. `jdoe`.
    pub user: String,
    /// NetBIOS domain name, e.g. `CORP`.
    pub domain: String,
    /// DNS domain, e.g. `corp.example.com`. In upper case, also the Kerberos realm.
    pub dns_domain: String,
    /// Windows computer name; empty keeps the Mac's.
    #[serde(default)]
    pub computer_name: String,
    /// Domain controller to sign in with, e.g. `dc01.corp.example.com`;
    /// empty finds one through DNS.
    #[serde(default)]
    pub kdc: String,
}

/// A program shown in a bottle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProgramDef {
    /// Stable id, used by launchers and links (indexes shift when a program
    /// is removed). Made when the program is saved; older files get one on load.
    #[serde(default)]
    pub id: String,
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
    /// Windows path of the app's own ARM64 build, launched instead of `exe`
    /// in Apple Silicon bottles (the `per-app-engine` experimental flag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arm64_build: Option<String>,
    /// RFC 3339 time of the last launch, if it was ever launched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launched: Option<String>,
    /// How many times it was launched.
    #[serde(default)]
    pub launch_count: u32,
    /// File extensions it opens (lowercase, without the dot), e.g. `docx`.
    #[serde(default)]
    pub file_types: Vec<String>,
}

/// A folder on this computer shown to the bottle's programs as its own drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DriveMapping {
    /// Drive letter, `D` to `Y`.
    pub letter: String,
    /// Absolute folder path.
    pub path: String,
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
    /// Engine it was created with. Fixed: it only runs on runtimes of this engine.
    pub engine: EngineKind,
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
    /// Domain account the programs run as, if any.
    pub domain: Option<DomainAccount>,
    /// Folders shown as drive letters (see [`super::drives`]).
    pub drives: Vec<DriveMapping>,
    /// Hide `Z:`, which Wine maps to the whole computer (`/`).
    pub hide_root_drive: bool,
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
    /// Bottles made before M8 lack it; they were all made with x86_64 Wine.
    #[serde(default = "EngineKind::legacy")]
    engine: EngineKind,
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
    #[serde(default)]
    domain: Option<DomainAccount>,
    #[serde(default)]
    drives: Vec<DriveMapping>,
    #[serde(default)]
    hide_root_drive: bool,
}

impl From<BottleFile> for Bottle {
    fn from(f: BottleFile) -> Self {
        Self {
            id: f.id,
            name: f.name,
            runtime: f.runtime,
            engine: f.engine,
            created_at: f.created_at,
            windows_version: f.windows_version,
            graphics: f.graphics,
            recipe: f.recipe,
            env: f.env,
            dll_overrides: f.dll_overrides,
            options: f.options,
            programs: f.programs,
            domain: f.domain,
            drives: f.drives,
            hide_root_drive: f.hide_root_drive,
        }
    }
}

impl Bottle {
    pub fn load(dir: &Path) -> Result<Self> {
        let file = dir.join(CONFIG_FILE);
        let text = std::fs::read_to_string(&file).at(&file)?;
        let mut bottle: Self = toml::from_str(&text).map_err(|e| Error::Config {
            path: file,
            message: e.to_string(),
        })?;
        // Programs saved before ids existed get one, written back so it stays
        // the same. Best effort: a read-only folder still loads.
        if bottle.assign_program_ids() {
            let _ = bottle.save(dir);
        }
        Ok(bottle)
    }

    /// Writes `bottle.toml` atomically (temp file, then rename). Programs
    /// without an id get one.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let file = dir.join(CONFIG_FILE);
        let tmp = dir.join(format!(".{CONFIG_FILE}.tmp"));
        let with_ids;
        let mut bottle = self;
        if self.programs.iter().any(|p| p.id.is_empty()) {
            let mut copy = self.clone();
            copy.assign_program_ids();
            with_ids = copy;
            bottle = &with_ids;
        }
        let text = toml::to_string_pretty(bottle).expect("bottle config serializes");
        std::fs::write(&tmp, text).at(&tmp)?;
        std::fs::rename(&tmp, &file).at(&file)
    }

    /// Gives every program without an id (or with one another program has
    /// already) a new one. Returns true if any changed.
    pub fn assign_program_ids(&mut self) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut changed = false;
        for i in 0..self.programs.len() {
            let id = self.programs[i].id.clone();
            if !id.is_empty() && seen.insert(id) {
                continue;
            }
            let fresh = loop {
                let candidate = new_program_id();
                if !seen.contains(&candidate) && !self.programs.iter().any(|p| p.id == candidate) {
                    break candidate;
                }
            };
            seen.insert(fresh.clone());
            self.programs[i].id = fresh;
            changed = true;
        }
        changed
    }

    /// Index of the program with this id.
    pub fn program_index(&self, id: &str) -> Option<usize> {
        self.programs.iter().position(|p| p.id == id)
    }
}

/// A short random program id. Never all digits, so links can tell it from an
/// old-style program index.
pub fn new_program_id() -> String {
    loop {
        let id = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
        if !id.chars().all(|c| c.is_ascii_digit()) {
            return id;
        }
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

[domain]
user = "jdoe"
domain = "CORP"
dns_domain = "corp.example.com"
"#;

    #[test]
    fn parses_plan_example_and_round_trips() {
        let bottle: Bottle = toml::from_str(PLAN_EXAMPLE).unwrap();
        assert_eq!(bottle.windows_version, WindowsVersion::Win10);
        assert_eq!(bottle.graphics, Graphics::Dxmt);
        assert_eq!(bottle.env["WINEESYNC"], "1");
        assert_eq!(bottle.dll_overrides["d3d11"], "native,builtin");
        let domain = bottle.domain.as_ref().unwrap();
        assert_eq!(
            (domain.user.as_str(), domain.computer_name.as_str()),
            ("jdoe", "")
        );
        assert_eq!(
            bottle.programs[0].exe,
            r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"
        );

        let tmp = tempfile::tempdir().unwrap();
        let mut bottle = bottle;
        bottle.assign_program_ids();
        bottle.save(tmp.path()).unwrap();
        assert_eq!(Bottle::load(tmp.path()).unwrap(), bottle);
    }

    #[test]
    fn older_programs_get_ids_that_stay() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(CONFIG_FILE), PLAN_EXAMPLE).unwrap();
        let first = Bottle::load(tmp.path()).unwrap();
        let id = first.programs[0].id.clone();
        assert!(!id.is_empty() && !id.chars().all(|c| c.is_ascii_digit()));
        // Written back on load, so the next load has the same one.
        let text = std::fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        assert!(text.contains(&format!("id = \"{id}\"")), "{text}");
        assert_eq!(Bottle::load(tmp.path()).unwrap().programs[0].id, id);
        assert_eq!(first.program_index(&id), Some(0));

        // Duplicated ids are fixed; saving fills in missing ones.
        let mut b = first.clone();
        b.programs.push(b.programs[0].clone());
        b.programs.push(ProgramDef {
            name: "New".into(),
            ..Default::default()
        });
        assert!(b.assign_program_ids());
        assert_eq!(b.programs[0].id, id);
        assert!(b.programs[1].id != id && !b.programs[2].id.is_empty());
        assert!(!b.assign_program_ids());
    }

    #[test]
    fn drives_default_to_none() {
        let bottle: Bottle = toml::from_str(PLAN_EXAMPLE).unwrap();
        assert!(bottle.drives.is_empty() && !bottle.hide_root_drive);
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
    fn wine_errors_are_logged_unless_turned_off() {
        // Older files have no `wine_debug`: log errors, so the doctor can see them.
        let old: BottleOptions = toml::from_str("show_fps = true").unwrap();
        assert_eq!(old.wine_debug, WineDebug::Errors);
        assert_eq!(BottleOptions::recommended().wine_debug, WineDebug::Errors);
        let off: BottleOptions = toml::from_str("wine_debug = \"off\"").unwrap();
        assert_eq!(off.wine_debug, WineDebug::Off);
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
        assert!(bottle.domain.is_none());
        // Made before engines existed, so with x86_64 Wine.
        assert_eq!(bottle.engine, EngineKind::legacy());
    }

    #[test]
    fn engine_is_kept() {
        let bottle: Bottle = toml::from_str(
            "id = \"a\"\nname = \"A\"\nruntime = \"r\"\nengine = \"arm64-fex\"\ncreated_at = \"2026-10-14T20:00:00Z\"\n",
        )
        .unwrap();
        assert_eq!(bottle.engine, EngineKind::Arm64Fex);
        let tmp = tempfile::tempdir().unwrap();
        bottle.save(tmp.path()).unwrap();
        let text = std::fs::read_to_string(tmp.path().join(CONFIG_FILE)).unwrap();
        assert!(text.contains("engine = \"arm64-fex\""), "{text}");
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
