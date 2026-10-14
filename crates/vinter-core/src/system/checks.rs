//! Checks that tell the user whether this machine can run Vinter.

use std::path::Path;

use serde::Serialize;

use crate::AppPaths;

/// Below this much free space we refuse to be optimistic.
const DISK_ERROR_BYTES: u64 = 2 * GB;
/// A runtime plus one bottle with a real app easily takes this much.
const DISK_WARNING_BYTES: u64 = 10 * GB;
const GB: u64 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
    /// The check does not apply to this machine.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Check {
    /// Stable identifier, e.g. `"rosetta"`.
    pub id: String,
    /// Short human-readable name.
    pub label: String,
    pub status: CheckStatus,
    /// One sentence explaining the result.
    pub detail: String,
}

impl Check {
    fn new(id: &str, label: &str, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            status,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SystemStatus {
    /// Version of `vinter-core`.
    pub version: String,
    /// `std::env::consts::OS`, e.g. `"macos"`.
    pub os: String,
    /// `std::env::consts::ARCH`, e.g. `"aarch64"`.
    pub arch: String,
    /// Data folder, for display only.
    pub data_dir: String,
    pub checks: Vec<Check>,
}

impl SystemStatus {
    /// True when no check reported an error.
    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Error)
    }
}

/// Runs all checks against the current machine.
pub fn system_checks(paths: &AppPaths) -> SystemStatus {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let data_dir = paths.data_dir();

    let rosetta = if needs_rosetta(os, arch) {
        rosetta_check(rosetta_installed())
    } else {
        Check::new(
            "rosetta",
            "Rosetta 2",
            CheckStatus::Skipped,
            "Only needed on Apple Silicon Macs.",
        )
    };

    SystemStatus {
        version: crate::VERSION.into(),
        os: os.into(),
        arch: arch.into(),
        data_dir: data_dir.display().to_string(),
        checks: vec![
            platform_check(os, arch),
            rosetta,
            data_dir_check(data_dir),
            disk_space_check(free_space(data_dir)),
        ],
    }
}

fn platform_check(os: &str, arch: &str) -> Check {
    let (status, detail) = match (os, arch) {
        ("macos", "aarch64") => (
            CheckStatus::Ok,
            "macOS on Apple Silicon is fully supported.",
        ),
        ("linux", "x86_64") => (CheckStatus::Ok, "Linux x86_64 is supported."),
        ("macos", "x86_64") => (
            CheckStatus::Warning,
            "Intel Macs are not supported yet. Things may not work.",
        ),
        _ => (
            CheckStatus::Error,
            "This platform is not supported. Vinter runs on macOS (Apple Silicon) and Linux x86_64.",
        ),
    };
    Check::new("platform", "Platform", status, detail)
}

fn needs_rosetta(os: &str, arch: &str) -> bool {
    os == "macos" && arch == "aarch64"
}

/// Wine builds for macOS are x86_64, so they need Rosetta 2 on Apple Silicon.
fn rosetta_installed() -> bool {
    std::process::Command::new("/usr/bin/arch")
        .args(["-x86_64", "/usr/bin/true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn rosetta_check(installed: bool) -> Check {
    if installed {
        Check::new(
            "rosetta",
            "Rosetta 2",
            CheckStatus::Ok,
            "Rosetta 2 is installed.",
        )
    } else {
        Check::new(
            "rosetta",
            "Rosetta 2",
            CheckStatus::Error,
            "Rosetta 2 is not installed. Wine needs it to run on Apple Silicon.",
        )
    }
}

fn data_dir_check(dir: &Path) -> Check {
    let (status, detail) = match std::fs::metadata(dir) {
        Ok(m) if m.is_dir() => (CheckStatus::Ok, "Data folder exists."),
        Ok(_) => (
            CheckStatus::Error,
            "A file is in the way of the data folder. Move or delete it.",
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (CheckStatus::Ok, "Data folder will be created on first use.")
        }
        Err(_) => (CheckStatus::Error, "Data folder cannot be read."),
    };
    Check::new("data_dir", "Data folder", status, detail)
}

fn disk_space_check(free: Option<u64>) -> Check {
    let (status, detail) = match free {
        None => (
            CheckStatus::Warning,
            "Could not determine free disk space.".to_string(),
        ),
        Some(bytes) => {
            let status = if bytes < DISK_ERROR_BYTES {
                CheckStatus::Error
            } else if bytes < DISK_WARNING_BYTES {
                CheckStatus::Warning
            } else {
                CheckStatus::Ok
            };
            (status, format!("{} free.", format_gb(bytes)))
        }
    };
    Check::new("disk_space", "Disk space", status, detail)
}

fn format_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GB as f64)
}

/// Free space available to the user on the volume holding `path`.
/// Walks up to the nearest existing ancestor, since the data folder may not exist yet.
fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    statvfs_available(existing)
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // field widths differ between macOS and Linux
fn statvfs_available(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` points to
    // writable memory of the right type. We only read it after success.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it initialised the struct.
    let stat = unsafe { stat.assume_init() };
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(unix))]
fn statvfs_available(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_support() {
        assert_eq!(platform_check("macos", "aarch64").status, CheckStatus::Ok);
        assert_eq!(platform_check("linux", "x86_64").status, CheckStatus::Ok);
        assert_eq!(
            platform_check("macos", "x86_64").status,
            CheckStatus::Warning
        );
        assert_eq!(
            platform_check("windows", "x86_64").status,
            CheckStatus::Error
        );
        assert_eq!(
            platform_check("linux", "aarch64").status,
            CheckStatus::Error
        );
    }

    #[test]
    fn rosetta_only_on_apple_silicon() {
        assert!(needs_rosetta("macos", "aarch64"));
        assert!(!needs_rosetta("macos", "x86_64"));
        assert!(!needs_rosetta("linux", "x86_64"));
        assert_eq!(rosetta_check(true).status, CheckStatus::Ok);
        assert_eq!(rosetta_check(false).status, CheckStatus::Error);
    }

    #[test]
    fn disk_space_thresholds() {
        assert_eq!(disk_space_check(None).status, CheckStatus::Warning);
        assert_eq!(disk_space_check(Some(GB)).status, CheckStatus::Error);
        assert_eq!(disk_space_check(Some(5 * GB)).status, CheckStatus::Warning);
        assert_eq!(disk_space_check(Some(50 * GB)).status, CheckStatus::Ok);
        assert_eq!(disk_space_check(Some(50 * GB)).detail, "50.0 GB free.");
    }

    #[test]
    fn data_dir_states() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(data_dir_check(tmp.path()).status, CheckStatus::Ok);

        let missing = tmp.path().join("missing");
        let check = data_dir_check(&missing);
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check.detail.contains("created"));

        let file = tmp.path().join("file");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(data_dir_check(&file).status, CheckStatus::Error);
    }

    #[test]
    fn free_space_of_missing_dir_uses_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let free = free_space(&tmp.path().join("a/b/c"));
        if cfg!(unix) {
            assert!(free.is_some_and(|b| b > 0));
        }
    }

    #[test]
    fn full_run_reports_every_check() {
        let tmp = tempfile::tempdir().unwrap();
        let status = system_checks(&AppPaths::with_data_dir(tmp.path()));
        let ids: Vec<_> = status.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["platform", "rosetta", "data_dir", "disk_space"]);
        assert_eq!(status.data_dir, tmp.path().display().to_string());
        assert_eq!(status.version, crate::VERSION);
    }

    #[test]
    fn serializes_status_as_snake_case() {
        let check = Check::new("x", "X", CheckStatus::Skipped, "d");
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["status"], "skipped");
    }
}
