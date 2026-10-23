//! Engines: how a runtime's Wine runs on this machine.
//!
//! Each runtime declares an [`EngineKind`], and every bottle keeps the kind
//! it was created with: ARM64 and x86_64 prefixes are not interchangeable,
//! so a bottle only ever runs on runtimes of its own engine.
//!
//! Every Wine process goes through [`Engine`] (see [`crate::process::command`]),
//! which owns the `wine`/`wineserver` paths, the launch environment, the x86
//! emulator DLLs to register, and the engine's system requirements.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bottle::Bottle;
use crate::experimental::{ExperimentalFlag, Flags};
use crate::runtime::Runtime;
use crate::{AppPaths, Error, Result};

/// Oldest macOS that can run ARM64 Wine natively (x18 handling, 4K-page
/// process spawn, per-thread TSO).
pub const ARM64_MIN_MACOS: &str = "26.5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum EngineKind {
    /// x86_64 Wine on macOS. Needs Rosetta 2 on Apple Silicon.
    #[serde(rename = "x86_64-rosetta")]
    X86_64Rosetta,
    /// ARM64 Wine on macOS, with FEX translating x86 code in-process.
    #[serde(rename = "arm64-fex")]
    Arm64Fex,
    /// x86_64 Wine on x86_64 Linux.
    #[serde(rename = "x86_64-native")]
    X86_64Native,
    /// ARM64 Wine on ARM64 Linux with FEX. Behind the `arm64-fex-linux` flag.
    #[serde(rename = "arm64-fex-linux")]
    Arm64FexLinux,
}

impl EngineKind {
    pub const ALL: [Self; 4] = [
        Self::X86_64Rosetta,
        Self::Arm64Fex,
        Self::X86_64Native,
        Self::Arm64FexLinux,
    ];

    /// The stable id used in `runtimes.json`, `runtime.toml` and `bottle.toml`.
    pub fn id(self) -> &'static str {
        match self {
            Self::X86_64Rosetta => "x86_64-rosetta",
            Self::Arm64Fex => "arm64-fex",
            Self::X86_64Native => "x86_64-native",
            Self::Arm64FexLinux => "arm64-fex-linux",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// What people see, e.g. in a bottle's details.
    pub fn label(self) -> &'static str {
        match self {
            Self::X86_64Rosetta => "Intel (Rosetta)",
            Self::Arm64Fex => "Apple Silicon",
            Self::X86_64Native => "x86_64",
            Self::Arm64FexLinux => "ARM64",
        }
    }

    /// The engine of a Wine build for `os` (as in `std::env::consts::OS`)
    /// whose binaries are `arch`.
    pub fn for_build(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("macos", "x86_64") => Some(Self::X86_64Rosetta),
            ("macos", "aarch64" | "arm64") => Some(Self::Arm64Fex),
            ("linux", "x86_64") => Some(Self::X86_64Native),
            ("linux", "aarch64" | "arm64") => Some(Self::Arm64FexLinux),
            _ => None,
        }
    }

    /// The engine of runtimes and bottles made before engines existed: every
    /// Wine build Vinter offered until then was x86_64.
    pub fn legacy() -> Self {
        Self::for_build(std::env::consts::OS, "x86_64").unwrap_or(Self::X86_64Native)
    }

    pub fn engine(self) -> &'static dyn Engine {
        match self {
            Self::X86_64Rosetta => &Rosetta,
            Self::Arm64Fex => &Fex { linux: false },
            Self::X86_64Native => &Native,
            Self::Arm64FexLinux => &Fex { linux: true },
        }
    }
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// An x86 emulator DLL that Wine loads in-process, and what it translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EmulatorDll {
    /// File name inside the runtime's Wine `lib` folder.
    pub file: &'static str,
    pub translates: Translates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Translates {
    /// 64-bit x86 code, through ARM64EC.
    X86_64,
    /// 32-bit x86 code, through WoW64.
    X86,
}

const FEX_DLLS: &[EmulatorDll] = &[
    EmulatorDll {
        file: "libarm64ecfex.dll",
        translates: Translates::X86_64,
    },
    EmulatorDll {
        file: "libwow64fex.dll",
        translates: Translates::X86,
    },
];

/// What runs a runtime's Wine on this machine.
pub trait Engine: Sync {
    fn kind(&self) -> EngineKind;

    /// The `wine` binary.
    fn wine(&self, rt: &Runtime) -> PathBuf {
        rt.wine_bin.clone()
    }

    /// The `wineserver` binary.
    fn wineserver(&self, rt: &Runtime) -> PathBuf {
        rt.bin_dir().join("wineserver")
    }

    /// The full environment for a Wine process in `bottle`.
    fn env(
        &self,
        prefix: &Path,
        bottle: &Bottle,
        base_overrides: &[(&str, &str)],
    ) -> Vec<(String, String)> {
        crate::process::wine_env(prefix, bottle, base_overrides)
    }

    /// x86 emulator DLLs a prefix of this engine registers. Empty when Wine
    /// runs x86 code directly (natively or under Rosetta).
    fn emulator_dlls(&self) -> &'static [EmulatorDll] {
        &[]
    }

    /// Fails if `rt` cannot run on this machine, saying why.
    fn check(&self, rt: &Runtime) -> Result<()>;
}

/// The host facts requirement checks look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub os: String,
    pub arch: String,
    /// macOS version, e.g. "26.5"; `None` elsewhere or if unknown.
    pub macos_version: Option<String>,
}

impl Host {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            macos_version: crate::system::os_version(),
        }
    }
}

/// Why `kind` cannot run on `host`, if it cannot. Rosetta being installed is
/// checked separately, since that needs a process.
pub fn unmet_requirement(kind: EngineKind, host: &Host) -> Option<String> {
    let silicon = matches!(host.arch.as_str(), "aarch64" | "arm64");
    match kind {
        EngineKind::X86_64Rosetta if host.os != "macos" => Some("it needs macOS".into()),
        EngineKind::X86_64Rosetta => None,
        EngineKind::X86_64Native if host.os != "linux" || host.arch != "x86_64" => {
            Some("it needs Linux on an x86_64 processor".into())
        }
        EngineKind::X86_64Native => None,
        EngineKind::Arm64Fex if host.os != "macos" || !silicon => {
            Some("it needs a Mac with Apple Silicon".into())
        }
        EngineKind::Arm64Fex => match &host.macos_version {
            Some(v) if crate::runtime::updates::compare_versions(v, ARM64_MIN_MACOS).is_lt() => {
                Some(format!(
                    "it needs macOS {ARM64_MIN_MACOS} or newer (this Mac has {v})"
                ))
            }
            // An unknown version gets the benefit of the doubt, as runtime minimums do.
            _ => None,
        },
        EngineKind::Arm64FexLinux if host.os != "linux" || !silicon => {
            Some("it needs Linux on an ARM64 processor".into())
        }
        EngineKind::Arm64FexLinux => None,
    }
}

fn check_host(kind: EngineKind, rt: &Runtime) -> Result<()> {
    match unmet_requirement(kind, &Host::current()) {
        Some(reason) => Err(Error::EngineUnavailable {
            runtime: rt.name.clone(),
            reason,
        }),
        None => Ok(()),
    }
}

/// x86_64 Wine on macOS: runs natively on Intel, under Rosetta 2 on Apple Silicon.
struct Rosetta;

impl Engine for Rosetta {
    fn kind(&self) -> EngineKind {
        EngineKind::X86_64Rosetta
    }

    fn check(&self, rt: &Runtime) -> Result<()> {
        check_host(self.kind(), rt)?;
        if rt.needs_rosetta && !crate::system::rosetta::is_installed() {
            return Err(Error::RosettaRequired(rt.name.clone()));
        }
        Ok(())
    }
}

/// x86_64 Wine on x86_64 Linux.
struct Native;

impl Engine for Native {
    fn kind(&self) -> EngineKind {
        EngineKind::X86_64Native
    }

    fn check(&self, rt: &Runtime) -> Result<()> {
        check_host(self.kind(), rt)
    }
}

/// ARM64 Wine with FEX loaded in-process for x86 code.
struct Fex {
    linux: bool,
}

impl Engine for Fex {
    fn kind(&self) -> EngineKind {
        if self.linux {
            EngineKind::Arm64FexLinux
        } else {
            EngineKind::Arm64Fex
        }
    }

    fn emulator_dlls(&self) -> &'static [EmulatorDll] {
        FEX_DLLS
    }

    fn check(&self, rt: &Runtime) -> Result<()> {
        check_host(self.kind(), rt)?;
        if self.linux {
            // Runtimes live in <data>/runtimes/<id>.
            let data_dir = rt.path.parent().and_then(Path::parent);
            let flags = data_dir
                .map(|d| Flags::load(&AppPaths::with_data_dir(d)))
                .unwrap_or_default();
            if !flags.is_enabled(ExperimentalFlag::Arm64FexLinux) {
                return Err(Error::EngineUnavailable {
                    runtime: rt.name.clone(),
                    reason: format!(
                        "the Linux ARM64 engine is experimental; turn on {} in Settings first",
                        ExperimentalFlag::Arm64FexLinux
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Fails unless `bottle` can run on `rt`: the runtime's engine must be the
/// one the bottle was created with.
pub fn check_bottle(rt: &Runtime, bottle: &Bottle) -> Result<()> {
    if rt.engine != bottle.engine {
        return Err(Error::EngineMismatch {
            bottle: bottle.name.clone(),
            bottle_engine: bottle.engine.label().into(),
            runtime: rt.name.clone(),
            runtime_engine: rt.engine.label().into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(os: &str, arch: &str, version: Option<&str>) -> Host {
        Host {
            os: os.into(),
            arch: arch.into(),
            macos_version: version.map(Into::into),
        }
    }

    #[test]
    fn ids_match_serde_and_parse() {
        for k in EngineKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.id());
            assert_eq!(EngineKind::parse(k.id()), Some(k));
            assert_eq!(k.engine().kind(), k);
        }
    }

    #[test]
    fn builds_map_to_engines() {
        use EngineKind::*;
        assert_eq!(
            EngineKind::for_build("macos", "x86_64"),
            Some(X86_64Rosetta)
        );
        assert_eq!(EngineKind::for_build("macos", "aarch64"), Some(Arm64Fex));
        assert_eq!(EngineKind::for_build("linux", "x86_64"), Some(X86_64Native));
        assert_eq!(
            EngineKind::for_build("linux", "aarch64"),
            Some(Arm64FexLinux)
        );
        assert_eq!(EngineKind::for_build("windows", "x86_64"), None);
        let legacy = if cfg!(target_os = "macos") {
            X86_64Rosetta
        } else {
            X86_64Native
        };
        assert_eq!(EngineKind::legacy(), legacy);
    }

    #[test]
    fn requirements() {
        use EngineKind::*;
        let m1_27 = host("macos", "aarch64", Some("27.0"));
        let m1_26_4 = host("macos", "aarch64", Some("26.4.1"));
        let intel_mac = host("macos", "x86_64", Some("27.0"));
        let linux = host("linux", "x86_64", None);
        let linux_arm = host("linux", "aarch64", None);

        assert_eq!(unmet_requirement(X86_64Rosetta, &m1_27), None);
        assert_eq!(unmet_requirement(X86_64Rosetta, &intel_mac), None);
        assert!(unmet_requirement(X86_64Rosetta, &linux).is_some());

        assert_eq!(unmet_requirement(Arm64Fex, &m1_27), None);
        assert_eq!(
            unmet_requirement(Arm64Fex, &host("macos", "aarch64", Some("26.5"))),
            None
        );
        let old = unmet_requirement(Arm64Fex, &m1_26_4).unwrap();
        assert!(old.contains("26.5") && old.contains("26.4.1"), "{old}");
        assert!(unmet_requirement(Arm64Fex, &intel_mac).is_some());
        assert_eq!(
            unmet_requirement(Arm64Fex, &host("macos", "aarch64", None)),
            None
        );

        assert_eq!(unmet_requirement(X86_64Native, &linux), None);
        assert!(unmet_requirement(X86_64Native, &linux_arm).is_some());
        assert!(unmet_requirement(X86_64Native, &m1_27).is_some());

        assert_eq!(unmet_requirement(Arm64FexLinux, &linux_arm), None);
        assert!(unmet_requirement(Arm64FexLinux, &linux).is_some());
    }

    #[test]
    fn only_fex_engines_register_emulators() {
        assert!(
            EngineKind::X86_64Rosetta
                .engine()
                .emulator_dlls()
                .is_empty()
        );
        assert!(EngineKind::X86_64Native.engine().emulator_dlls().is_empty());
        for k in [EngineKind::Arm64Fex, EngineKind::Arm64FexLinux] {
            let dlls = k.engine().emulator_dlls();
            assert!(dlls.iter().any(|d| d.translates == Translates::X86_64));
            assert!(dlls.iter().any(|d| d.translates == Translates::X86));
        }
    }
}
