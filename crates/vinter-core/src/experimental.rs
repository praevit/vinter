//! Experimental features: off by default, turned on per user in Settings or
//! with the CLI, and saved under `[experimental]` in `settings.toml`.
//!
//! Each feature checks its flag with [`Flags::is_enabled`]. Ids are stable:
//! they are what `settings.toml` and the CLI use, so never rename one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::settings::{self, Settings};
use crate::{AppPaths, Result};

/// One experimental feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ExperimentalFlag {
    #[serde(rename = "per-app-engine")]
    PerAppEngine,
    #[serde(rename = "engine-ab-compare")]
    EngineAbCompare,
    #[serde(rename = "simulate-macos28")]
    SimulateMacos28,
    #[serde(rename = "bottle-migrate-arm64")]
    BottleMigrateArm64,
    #[serde(rename = "fex-config-editor")]
    FexConfigEditor,
    #[serde(rename = "wow64-large-address")]
    Wow64LargeAddress,
    #[serde(rename = "graphics-dxvk-arm64ec")]
    GraphicsDxvkArm64ec,
    #[serde(rename = "graphics-user-gptk")]
    GraphicsUserGptk,
    #[serde(rename = "runtime-channel-nightly")]
    RuntimeChannelNightly,
    #[serde(rename = "runtime-build-from-source")]
    RuntimeBuildFromSource,
    #[serde(rename = "arm64-fex-linux")]
    Arm64FexLinux,
    #[serde(rename = "compat-notes")]
    CompatNotes,
    #[serde(rename = "launcher-shims")]
    LauncherShims,
}

/// How much can go wrong with a flag on. High-risk flags ask before turning on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// The registry: every flag, in the order Settings lists them.
pub fn all() -> &'static [ExperimentalFlag] {
    use ExperimentalFlag::*;
    &[
        SimulateMacos28,
        EngineAbCompare,
        CompatNotes,
        PerAppEngine,
        Wow64LargeAddress,
        GraphicsDxvkArm64ec,
        GraphicsUserGptk,
        RuntimeChannelNightly,
        Arm64FexLinux,
        BottleMigrateArm64,
        FexConfigEditor,
        RuntimeBuildFromSource,
        LauncherShims,
    ]
}

impl ExperimentalFlag {
    /// The stable id used in `settings.toml` and the CLI.
    pub fn id(self) -> &'static str {
        use ExperimentalFlag::*;
        match self {
            PerAppEngine => "per-app-engine",
            EngineAbCompare => "engine-ab-compare",
            SimulateMacos28 => "simulate-macos28",
            BottleMigrateArm64 => "bottle-migrate-arm64",
            FexConfigEditor => "fex-config-editor",
            Wow64LargeAddress => "wow64-large-address",
            GraphicsDxvkArm64ec => "graphics-dxvk-arm64ec",
            GraphicsUserGptk => "graphics-user-gptk",
            RuntimeChannelNightly => "runtime-channel-nightly",
            RuntimeBuildFromSource => "runtime-build-from-source",
            Arm64FexLinux => "arm64-fex-linux",
            CompatNotes => "compat-notes",
            LauncherShims => "launcher-shims",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        all().iter().copied().find(|f| f.id() == id)
    }

    pub fn title(self) -> &'static str {
        use ExperimentalFlag::*;
        match self {
            PerAppEngine => "Engine per app",
            EngineAbCompare => "Compare engines",
            SimulateMacos28 => "Pretend Rosetta is gone",
            BottleMigrateArm64 => "Recreate a bottle for Apple Silicon",
            FexConfigEditor => "FEX settings per app",
            Wow64LargeAddress => "Memory-hungry 32-bit apps",
            GraphicsDxvkArm64ec => "DXVK on Apple Silicon Wine",
            GraphicsUserGptk => "Your own Game Porting Toolkit",
            RuntimeChannelNightly => "Nightly runtimes",
            RuntimeBuildFromSource => "Build runtimes from source",
            Arm64FexLinux => "Linux ARM64 engine",
            CompatNotes => "Compatibility notes",
            LauncherShims => "Game launcher fixes",
        }
    }

    pub fn description(self) -> &'static str {
        use ExperimentalFlag::*;
        match self {
            PerAppEngine => {
                "Pick a different engine for one app in a bottle, where that can work, such as starting a native ARM64 build directly."
            }
            EngineAbCompare => {
                "Start the same app with Rosetta and with Apple Silicon Wine, and compare start time, crashes and frame rate side by side."
            }
            SimulateMacos28 => {
                "Hide the Rosetta engine everywhere, to see Vinter as it will be once macOS no longer has Rosetta."
            }
            BottleMigrateArm64 => {
                "Make a new Apple Silicon bottle from an old one: copies your documents and saves, and reinstalls its apps. The old bottle is never changed."
            }
            FexConfigEditor => {
                "Change the x86 emulator's settings for one app, using the options the installed FEX version offers."
            }
            Wow64LargeAddress => {
                "Extra checks and warnings for 32-bit apps that run out of address space on Apple Silicon Wine."
            }
            GraphicsDxvkArm64ec => {
                "Offer DXVK over MoltenVK in Apple Silicon bottles, when the runtime includes it."
            }
            GraphicsUserGptk => {
                "Use D3DMetal from a Game Porting Toolkit you installed yourself. Vinter never downloads it."
            }
            RuntimeChannelNightly => "Show nightly and preview runtimes in the runtime list.",
            RuntimeBuildFromSource => {
                "Build Apple Silicon Wine and FEX from chosen git sources, for testing runtimes before they are published."
            }
            Arm64FexLinux => "Run Windows apps on ARM64 Linux with FEX.",
            CompatNotes => {
                "Keep your own notes on how well each app works, stored on this computer and exportable as JSON."
            }
            LauncherShims => {
                "Apply known workarounds for game launchers that fail on Apple Silicon Wine, with a list of what was changed."
            }
        }
    }

    pub fn risk(self) -> Risk {
        use ExperimentalFlag::*;
        match self {
            EngineAbCompare | SimulateMacos28 | CompatNotes => Risk::Low,
            PerAppEngine
            | Wow64LargeAddress
            | GraphicsDxvkArm64ec
            | GraphicsUserGptk
            | RuntimeChannelNightly
            | Arm64FexLinux => Risk::Medium,
            BottleMigrateArm64 | FexConfigEditor | RuntimeBuildFromSource | LauncherShims => {
                Risk::High
            }
        }
    }

    /// Whether turning it on does anything yet. Flags are registered before
    /// their feature is built, so their ids (and saved choices) stay stable.
    pub fn available(self) -> bool {
        matches!(
            self,
            Self::SimulateMacos28
                | Self::CompatNotes
                | Self::RuntimeChannelNightly
                | Self::Wow64LargeAddress
                | Self::GraphicsDxvkArm64ec
                | Self::EngineAbCompare
                | Self::BottleMigrateArm64
        )
    }
}

impl std::fmt::Display for ExperimentalFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// The `[experimental]` section of `settings.toml`: flag id → on.
///
/// Ids this version does not know are kept as they are (a newer Vinter may
/// have written them) but never count as enabled. Entries that are not
/// `true`/`false` are dropped rather than making the whole file unreadable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExperimentalSettings(BTreeMap<String, bool>);

impl<'de> Deserialize<'de> for ExperimentalSettings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::IgnoredAny;

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Entry {
            On(bool),
            Other(IgnoredAny),
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Section {
            Table(BTreeMap<String, Entry>),
            Other(IgnoredAny),
        }

        let map = match Section::deserialize(d)? {
            Section::Table(entries) => entries
                .into_iter()
                .filter_map(|(id, e)| match e {
                    Entry::On(on) => Some((id, on)),
                    Entry::Other(_) => None,
                })
                .collect(),
            Section::Other(_) => BTreeMap::new(),
        };
        Ok(Self(map))
    }
}

impl ExperimentalSettings {
    pub fn is_enabled(&self, flag: ExperimentalFlag) -> bool {
        self.0.get(flag.id()).copied().unwrap_or(false)
    }

    pub fn set(&mut self, flag: ExperimentalFlag, enabled: bool) {
        if enabled {
            self.0.insert(flag.id().to_string(), true);
        } else {
            self.0.remove(flag.id());
        }
    }
}

/// Flags turned on for this process only (`vinter --experimental <id>`).
static SESSION: Mutex<BTreeSet<ExperimentalFlag>> = Mutex::new(BTreeSet::new());

/// Turns a flag on until this process exits, without saving it.
pub fn enable_for_session(flag: ExperimentalFlag) {
    SESSION.lock().unwrap().insert(flag);
}

/// The flags in effect: the saved ones plus any turned on for this process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    enabled: BTreeSet<ExperimentalFlag>,
}

impl Flags {
    pub fn load(paths: &AppPaths) -> Self {
        Self::from_settings(&settings::load(paths))
    }

    pub fn from_settings(settings: &Settings) -> Self {
        let mut enabled: BTreeSet<_> = all()
            .iter()
            .copied()
            .filter(|f| settings.experimental.is_enabled(*f))
            .collect();
        enabled.extend(SESSION.lock().unwrap().iter().copied());
        Self { enabled }
    }

    pub fn is_enabled(&self, flag: ExperimentalFlag) -> bool {
        self.enabled.contains(&flag)
    }
}

/// A flag with everything Settings and `vinter experimental list` show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FlagState {
    pub flag: ExperimentalFlag,
    pub title: &'static str,
    pub description: &'static str,
    pub risk: Risk,
    /// False while the feature behind it is not built yet.
    pub available: bool,
    /// In effect now (saved, or turned on for this command).
    pub enabled: bool,
    /// Saved in `settings.toml`.
    pub saved: bool,
}

/// Every flag and whether it is on.
pub fn list(paths: &AppPaths) -> Vec<FlagState> {
    let settings = settings::load(paths);
    let flags = Flags::from_settings(&settings);
    all()
        .iter()
        .map(|&flag| FlagState {
            flag,
            title: flag.title(),
            description: flag.description(),
            risk: flag.risk(),
            available: flag.available(),
            enabled: flags.is_enabled(flag),
            saved: settings.experimental.is_enabled(flag),
        })
        .collect()
}

/// Turns a flag on or off and saves it.
pub fn set(paths: &AppPaths, flag: ExperimentalFlag, enabled: bool) -> Result<()> {
    let mut settings = settings::load(paths);
    settings.experimental.set(flag, enabled);
    settings::save(paths, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_match_serde() {
        let ids: BTreeSet<_> = all().iter().map(|f| f.id()).collect();
        assert_eq!(ids.len(), all().len());
        for &f in all() {
            assert_eq!(serde_json::to_value(f).unwrap(), f.id());
            assert_eq!(ExperimentalFlag::parse(f.id()), Some(f));
        }
        assert_eq!(ExperimentalFlag::parse("nope"), None);
    }

    #[test]
    fn off_by_default() {
        let flags = Flags::from_settings(&Settings::default());
        // Session flags are process-wide; no test turns this one on.
        assert!(!flags.is_enabled(ExperimentalFlag::LauncherShims));
    }

    #[test]
    fn saves_and_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let flag = ExperimentalFlag::FexConfigEditor;
        set(&paths, flag, true).unwrap();
        assert!(Flags::load(&paths).is_enabled(flag));
        let text = std::fs::read_to_string(paths.settings_file()).unwrap();
        assert!(text.contains("[experimental]"), "{text}");
        assert!(text.contains("fex-config-editor = true"), "{text}");

        set(&paths, flag, false).unwrap();
        assert!(!Flags::load(&paths).is_enabled(flag));
        assert!(!list(&paths).iter().any(|s| s.saved));
    }

    #[test]
    fn unknown_ids_are_ignored_and_kept() {
        let s: Settings = toml::from_str(
            "[experimental]\nfrom-the-future = true\ncompat-notes = true\nper-app-engine = false\n",
        )
        .unwrap();
        let flags = Flags::from_settings(&s);
        assert!(flags.is_enabled(ExperimentalFlag::CompatNotes));
        assert!(!flags.is_enabled(ExperimentalFlag::PerAppEngine));

        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        settings::save(&paths, &s).unwrap();
        set(&paths, ExperimentalFlag::CompatNotes, false).unwrap();
        let text = std::fs::read_to_string(paths.settings_file()).unwrap();
        assert!(text.contains("from-the-future = true"), "{text}");
    }

    #[test]
    fn a_bad_section_keeps_the_other_settings() {
        // A wrong type under [experimental] must not reset everything else.
        let s: Settings =
            toml::from_str("notifications = false\n[experimental]\nx = 1\ncompat-notes = true\n")
                .unwrap();
        assert!(!s.notifications);
        assert!(s.experimental.is_enabled(ExperimentalFlag::CompatNotes));
        let s: Settings = toml::from_str("notifications = false\nexperimental = 3\n").unwrap();
        assert!(!s.notifications);
        assert_eq!(s.experimental, ExperimentalSettings::default());
    }

    #[test]
    fn session_flags_are_on_but_not_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let flag = ExperimentalFlag::RuntimeChannelNightly;
        enable_for_session(flag);
        let state = list(&paths).into_iter().find(|s| s.flag == flag).unwrap();
        assert!(state.enabled);
        assert!(!state.saved);
        assert!(!paths.settings_file().exists());
    }
}
