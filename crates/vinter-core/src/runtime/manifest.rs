//! The list of Wine builds Vinter can install.
//!
//! For v1 the manifest is embedded in the binary. Later it will be fetched
//! (and signature-checked) from a URL we control.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MANIFEST_JSON: &str = include_str!("../../runtimes.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Flavor {
    Stable,
    Devel,
    Staging,
    Gptk,
}

pub use crate::download::ArchiveFormat;

/// One downloadable Wine build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RuntimeInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub flavor: Flavor,
    /// Host OS, as in `std::env::consts::OS`.
    pub os: String,
    /// CPU architecture of the Wine binaries (macOS builds are x86_64 and run under Rosetta 2).
    pub arch: String,
    pub url: String,
    /// Lowercase hex sha256 of the archive.
    pub sha256: String,
    /// Download size in bytes.
    pub size: u64,
    pub format: ArchiveFormat,
    /// Path of the `wine` binary inside the archive.
    pub wine_bin: String,
    pub license: String,
    /// Exact Wine source this build was made from (LGPL requirement).
    pub source_url: String,
    /// Upstream release page of the build.
    pub release_url: String,
    /// Graphics backends the build supports.
    pub graphics: Vec<crate::bottle::Graphics>,
    /// Engine that runs it; must match `os` and `arch`.
    pub engine: crate::engine::EngineKind,
    /// Oldest macOS it runs on, e.g. "15.0" (its libraries are built for it).
    #[serde(default)]
    pub min_os_version: Option<String>,
    /// FEX release it ships, for engines that translate x86 code with FEX.
    #[serde(default)]
    pub fex_version: Option<String>,
    /// Filled in for this machine: x86_64 build on Apple Silicon.
    #[serde(default, skip_deserializing)]
    pub needs_rosetta: bool,
}

impl RuntimeInfo {
    /// True if this build runs on the given host.
    /// x86_64 macOS builds also run on Apple Silicon through Rosetta 2.
    pub fn runs_on(&self, os: &str, arch: &str) -> bool {
        self.os == os
            && (self.arch == arch || (os == "macos" && arch == "aarch64" && self.arch == "x86_64"))
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    runtimes: Vec<RuntimeInfo>,
}

/// Every runtime in the embedded manifest, for all platforms.
pub fn all() -> &'static [RuntimeInfo] {
    static PARSED: OnceLock<Vec<RuntimeInfo>> = OnceLock::new();
    PARSED.get_or_init(|| {
        let manifest: Manifest =
            serde_json::from_str(MANIFEST_JSON).expect("embedded runtimes.json is invalid");
        assert_eq!(manifest.version, 1, "unsupported runtimes.json version");
        manifest.runtimes
    })
}

/// Runtimes that run on this machine (as the user's experimental flags
/// present it: `simulate-macos28` hides the Rosetta ones).
pub fn available(paths: &crate::AppPaths) -> Vec<RuntimeInfo> {
    let host = crate::engine::Host::for_paths(paths);
    available_for(&host.os, &host.arch)
        .into_iter()
        .filter(|r| supports_version(r, host.macos_version.as_deref()))
        .filter(|r| !(host.rosetta_removed && r.engine == crate::engine::EngineKind::X86_64Rosetta))
        .collect()
}

/// True if `host` (e.g. "15.3") is at least the build's minimum. An unknown
/// host version passes.
fn supports_version(info: &RuntimeInfo, host: Option<&str>) -> bool {
    match (&info.min_os_version, host) {
        (Some(min), Some(host)) => {
            super::updates::compare_versions(host, min) != std::cmp::Ordering::Less
        }
        _ => true,
    }
}

pub fn available_for(os: &str, arch: &str) -> Vec<RuntimeInfo> {
    all()
        .iter()
        .filter(|r| r.runs_on(os, arch))
        .map(|r| RuntimeInfo {
            needs_rosetta: crate::system::rosetta::needed_on(os, arch) && r.arch == "x86_64",
            ..r.clone()
        })
        .collect()
}

/// Looks up an available runtime by id.
pub fn find(paths: &crate::AppPaths, id: &str) -> Result<RuntimeInfo> {
    available(paths)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| Error::RuntimeNotFound(id.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::error::validate_id;

    #[test]
    fn embedded_manifest_is_valid() {
        let all = all();
        let mut ids = HashSet::new();
        for r in all {
            assert!(ids.insert(&r.id), "duplicate id {}", r.id);
            validate_id(&r.id).unwrap();
            assert!(r.url.starts_with("https://"), "{}: url must be https", r.id);
            assert_eq!(r.sha256.len(), 64, "{}: sha256 length", r.id);
            assert!(
                r.sha256.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "{}: sha256 must be lowercase hex",
                r.id
            );
            assert!(r.size > 0);
            assert!(r.wine_bin.ends_with("/bin/wine"), "{}: wine_bin", r.id);
            assert!(!r.wine_bin.starts_with('/') && !r.wine_bin.contains(".."));
            assert!(r.source_url.starts_with("https://"));
            assert!(matches!(r.os.as_str(), "macos" | "linux"));
            assert_eq!(
                Some(r.engine),
                crate::engine::EngineKind::for_build(&r.os, &r.arch),
                "{}: engine must match os and arch",
                r.id
            );
        }
    }

    #[test]
    fn minimum_os_versions() {
        let mut info = all()[0].clone();
        info.min_os_version = None;
        assert!(supports_version(&info, Some("13.0")));
        info.min_os_version = Some("15.0".into());
        assert!(!supports_version(&info, Some("14.7")));
        assert!(supports_version(&info, Some("15.0")));
        assert!(supports_version(&info, Some("27.1")));
        assert!(supports_version(&info, None));
    }

    #[test]
    fn has_builds_for_both_targets() {
        assert!(!available_for("macos", "aarch64").is_empty());
        assert!(!available_for("linux", "x86_64").is_empty());
        assert!(available_for("windows", "x86_64").is_empty());
    }

    #[test]
    fn rosetta_compatibility() {
        let mac = all()
            .iter()
            .find(|r| r.os == "macos" && r.arch == "x86_64")
            .unwrap();
        assert!(mac.runs_on("macos", "aarch64"));
        assert!(mac.runs_on("macos", "x86_64"));
        assert!(!mac.runs_on("linux", "x86_64"));
        let linux = all().iter().find(|r| r.os == "linux").unwrap();
        assert!(linux.runs_on("linux", "x86_64"));
        assert!(!linux.runs_on("linux", "aarch64"));

        // Apple Silicon Wine only runs on Apple Silicon, and never needs Rosetta.
        let arm = all()
            .iter()
            .find(|r| r.engine == crate::engine::EngineKind::Arm64Fex)
            .unwrap();
        assert!(arm.runs_on("macos", "aarch64"));
        assert!(!arm.runs_on("macos", "x86_64"));
        assert!(arm.min_os_version.as_deref() == Some(crate::engine::ARM64_MIN_MACOS));
        let listed = available_for("macos", "aarch64");
        assert!(
            !listed
                .iter()
                .find(|r| r.id == arm.id)
                .unwrap()
                .needs_rosetta
        );
    }
}
