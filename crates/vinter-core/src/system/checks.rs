//! Checks that tell the user whether this machine can run Vinter.

use std::path::Path;

use serde::Serialize;

use crate::AppPaths;
use crate::engine::{ARM64_MIN_MACOS, EngineKind, Host, unmet_requirement};

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
    /// macOS version, e.g. "27.0.1"; `None` elsewhere or if unknown.
    pub macos_version: Option<String>,
    /// This machine can run the Apple Silicon engine (Wine without Rosetta).
    pub arm64_engine: bool,
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
    let host = Host::current();

    let rosetta = if super::rosetta::needed_on(os, arch) {
        rosetta_check(super::rosetta::is_installed())
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
        macos_version: host.macos_version.clone(),
        arm64_engine: super::readiness::arm64_available(&host),
        checks: vec![
            platform_check(os, arch),
            cpu_check(arch, cpu_name().as_deref()),
            macos_check(&host),
            rosetta,
            arm64_engine_check(&host),
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

fn cpu_check(arch: &str, name: Option<&str>) -> Check {
    let kind = match arch {
        "aarch64" => "ARM64",
        other => other,
    };
    let detail = match name {
        Some(name) => format!("{name} ({kind})."),
        None => format!("{kind} processor."),
    };
    Check::new("cpu", "Processor", CheckStatus::Ok, detail)
}

/// The processor's marketing name, e.g. "Apple M2".
fn cpu_name() -> Option<String> {
    let name = if cfg!(target_os = "macos") {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()?;
        String::from_utf8(out.stdout).ok()?
    } else {
        let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        info.lines()
            .find(|l| l.starts_with("model name"))?
            .split_once(':')?
            .1
            .to_string()
    };
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

fn macos_check(host: &Host) -> Check {
    let label = "macOS";
    if host.os != "macos" {
        return Check::new("macos", label, CheckStatus::Skipped, "Not a Mac.");
    }
    let Some(version) = &host.macos_version else {
        return Check::new(
            "macos",
            label,
            CheckStatus::Warning,
            "Could not read the macOS version.",
        );
    };
    if unmet_requirement(EngineKind::Arm64Fex, host).is_some() && host.arch == "aarch64" {
        return Check::new(
            "macos",
            label,
            CheckStatus::Warning,
            format!(
                "macOS {version}. Update to macOS {ARM64_MIN_MACOS} or newer to run Windows apps without Rosetta."
            ),
        );
    }
    Check::new("macos", label, CheckStatus::Ok, format!("macOS {version}."))
}

fn arm64_engine_check(host: &Host) -> Check {
    let label = "Native Wine";
    if host.os != "macos" || host.arch != "aarch64" {
        return Check::new(
            "arm64_engine",
            label,
            CheckStatus::Skipped,
            "Only Macs with Apple Silicon run Wine natively.",
        );
    }
    match unmet_requirement(EngineKind::Arm64Fex, host) {
        None => Check::new(
            "arm64_engine",
            label,
            CheckStatus::Ok,
            "This Mac can run Windows apps without Rosetta, in Apple Silicon bottles.",
        ),
        Some(reason) => Check::new(
            "arm64_engine",
            label,
            CheckStatus::Warning,
            format!(
                "Apple Silicon bottles cannot run yet: {reason}. Until then bottles use Rosetta, which macOS 28 mostly removes."
            ),
        ),
    }
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
            CheckStatus::Warning,
            "Not installed. The current Wine builds for Mac need it; Vinter will offer to install it when one is used.",
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

    fn host(os: &str, arch: &str, version: Option<&str>) -> Host {
        Host {
            os: os.into(),
            arch: arch.into(),
            macos_version: version.map(Into::into),
        }
    }

    #[test]
    fn macos_and_arm64_engine_checks() {
        let new_mac = host("macos", "aarch64", Some("27.0.1"));
        let old_mac = host("macos", "aarch64", Some("26.4"));
        let intel = host("macos", "x86_64", Some("27.0"));
        let linux = host("linux", "x86_64", None);

        assert_eq!(macos_check(&new_mac).status, CheckStatus::Ok);
        assert_eq!(macos_check(&new_mac).detail, "macOS 27.0.1.");
        assert_eq!(macos_check(&old_mac).status, CheckStatus::Warning);
        assert!(macos_check(&old_mac).detail.contains("26.5"));
        assert_eq!(macos_check(&intel).status, CheckStatus::Ok);
        assert_eq!(macos_check(&linux).status, CheckStatus::Skipped);

        assert_eq!(arm64_engine_check(&new_mac).status, CheckStatus::Ok);
        assert_eq!(arm64_engine_check(&old_mac).status, CheckStatus::Warning);
        assert_eq!(arm64_engine_check(&intel).status, CheckStatus::Skipped);
        assert_eq!(arm64_engine_check(&linux).status, CheckStatus::Skipped);
    }

    #[test]
    fn cpu_names() {
        assert_eq!(
            cpu_check("aarch64", Some("Apple M2")).detail,
            "Apple M2 (ARM64)."
        );
        assert_eq!(cpu_check("x86_64", None).detail, "x86_64 processor.");
    }

    #[test]
    fn rosetta_states() {
        assert_eq!(rosetta_check(true).status, CheckStatus::Ok);
        assert_eq!(rosetta_check(false).status, CheckStatus::Warning);
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
        assert_eq!(
            ids,
            [
                "platform",
                "cpu",
                "macos",
                "rosetta",
                "arm64_engine",
                "data_dir",
                "disk_space"
            ]
        );
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
