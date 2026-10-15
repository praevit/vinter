//! Host system inspection.

pub mod checks;
pub mod rosetta;

pub use checks::{Check, CheckStatus, SystemStatus, system_checks};

/// The macOS version, e.g. "27.0" (`None` elsewhere or if unknown).
pub fn os_version() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    static VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            let out = std::process::Command::new("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()
                .ok()?;
            let text = String::from_utf8(out.stdout).ok()?;
            Some(text.trim().to_string()).filter(|v| !v.is_empty())
        })
        .clone()
}
