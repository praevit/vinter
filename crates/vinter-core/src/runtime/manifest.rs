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
    Crossover,
    Gptk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ArchiveFormat {
    TarXz,
    TarGz,
}

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

/// Runtimes that run on this machine.
pub fn available() -> Vec<RuntimeInfo> {
    available_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn available_for(os: &str, arch: &str) -> Vec<RuntimeInfo> {
    all()
        .iter()
        .filter(|r| r.runs_on(os, arch))
        .cloned()
        .collect()
}

/// Looks up an available runtime by id.
pub fn find(id: &str) -> Result<RuntimeInfo> {
    available()
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
        }
    }

    #[test]
    fn has_builds_for_both_targets() {
        assert!(!available_for("macos", "aarch64").is_empty());
        assert!(!available_for("linux", "x86_64").is_empty());
        assert!(available_for("windows", "x86_64").is_empty());
    }

    #[test]
    fn rosetta_compatibility() {
        let mac = all().iter().find(|r| r.os == "macos").unwrap();
        assert!(mac.runs_on("macos", "aarch64"));
        assert!(mac.runs_on("macos", "x86_64"));
        assert!(!mac.runs_on("linux", "x86_64"));
        let linux = all().iter().find(|r| r.os == "linux").unwrap();
        assert!(linux.runs_on("linux", "x86_64"));
        assert!(!linux.runs_on("linux", "aarch64"));
    }
}
