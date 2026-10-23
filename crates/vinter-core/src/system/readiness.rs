//! "Rosetta readiness": whether a bottle keeps working once macOS stops
//! shipping full Rosetta 2 (macOS 28, fall 2027).

use serde::Serialize;

use crate::engine::{EngineKind, Host, unmet_requirement};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ReadinessLevel {
    /// Runs on the Apple Silicon engine here: no Rosetta needed.
    Green,
    /// Uses Rosetta, but this Mac can run Apple Silicon bottles.
    Yellow,
    /// Uses Rosetta and this Mac cannot run Apple Silicon bottles, or the
    /// bottle's own engine cannot run here.
    Red,
    /// Not a Rosetta question here (Linux, Intel Macs).
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Readiness {
    pub level: ReadinessLevel,
    /// One or two sentences for a tooltip or notice.
    pub summary: String,
}

const GOING_AWAY: &str =
    "Rosetta is going away: from macOS 28 (fall 2027) only a small part of it remains.";

/// True when this host can run the Apple Silicon engine.
pub fn arm64_available(host: &Host) -> bool {
    unmet_requirement(EngineKind::Arm64Fex, host).is_none()
}

/// Readiness of a bottle with `engine` on `host`.
pub fn for_engine(engine: EngineKind, host: &Host) -> Readiness {
    let silicon_mac = host.os == "macos" && matches!(host.arch.as_str(), "aarch64" | "arm64");
    let arm64 = unmet_requirement(EngineKind::Arm64Fex, host);
    let (level, summary) = match engine {
        EngineKind::Arm64Fex => match arm64 {
            None => (
                ReadinessLevel::Green,
                "Runs without Rosetta, so it is ready for macOS 28 and later.".to_string(),
            ),
            Some(reason) => (
                ReadinessLevel::Red,
                format!("Made for the Apple Silicon engine, which cannot run here: {reason}."),
            ),
        },
        EngineKind::X86_64Rosetta if !silicon_mac => (
            ReadinessLevel::NotApplicable,
            "This Mac runs Intel Wine without Rosetta.".to_string(),
        ),
        EngineKind::X86_64Rosetta => match arm64 {
            None => (
                ReadinessLevel::Yellow,
                format!(
                    "Uses Rosetta. {GOING_AWAY} This Mac can run Apple Silicon bottles: create one to move your apps."
                ),
            ),
            Some(reason) => (
                ReadinessLevel::Red,
                format!(
                    "Uses Rosetta. {GOING_AWAY} Apple Silicon bottles cannot run on this Mac yet: {reason}."
                ),
            ),
        },
        EngineKind::X86_64Native | EngineKind::Arm64FexLinux => (
            ReadinessLevel::NotApplicable,
            "Rosetta is only used on Macs.".to_string(),
        ),
    };
    Readiness { level, summary }
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
    fn levels() {
        use EngineKind::*;
        use ReadinessLevel::*;
        let new_mac = host("macos", "aarch64", Some("27.0.1"));
        let old_mac = host("macos", "aarch64", Some("26.4"));
        let intel = host("macos", "x86_64", Some("27.0"));
        let linux = host("linux", "x86_64", None);

        assert_eq!(for_engine(Arm64Fex, &new_mac).level, Green);
        assert_eq!(for_engine(X86_64Rosetta, &new_mac).level, Yellow);
        assert_eq!(for_engine(X86_64Rosetta, &old_mac).level, Red);
        assert_eq!(for_engine(Arm64Fex, &old_mac).level, Red);
        assert_eq!(for_engine(X86_64Rosetta, &intel).level, NotApplicable);
        assert_eq!(for_engine(X86_64Native, &linux).level, NotApplicable);

        let red = for_engine(X86_64Rosetta, &old_mac).summary;
        assert!(red.contains("macOS 28") && red.contains("26.5"), "{red}");
        assert!(
            for_engine(X86_64Rosetta, &new_mac)
                .summary
                .contains("macOS 28")
        );
        assert!(arm64_available(&new_mac) && !arm64_available(&old_mac));
    }
}
