//! Rosetta 2, which the x86_64 macOS Wine builds need on Apple Silicon.

use std::process::{Command, Output, Stdio};

use crate::{Error, Result};

const SOFTWAREUPDATE: &str = "/usr/sbin/softwareupdate";
const INSTALL_ARGS: [&str; 2] = ["--install-rosetta", "--agree-to-license"];
/// Same command, run with the admin password prompt macOS shows for installers.
/// The string is a constant: no user input is ever interpolated into it.
const ADMIN_SCRIPT: &str = "do shell script \"/usr/sbin/softwareupdate --install-rosetta --agree-to-license\" with administrator privileges";
/// AppleScript error code when the user cancels the password prompt.
const USER_CANCELED: &str = "-128";

/// True when this Mac needs Rosetta for Wine (Apple Silicon).
pub fn is_needed() -> bool {
    needed_on(std::env::consts::OS, std::env::consts::ARCH)
}

pub(crate) fn needed_on(os: &str, arch: &str) -> bool {
    os == "macos" && arch == "aarch64"
}

/// True when x86_64 programs can run, i.e. Rosetta 2 is installed.
pub fn is_installed() -> bool {
    Command::new("/usr/bin/arch")
        .args(["-x86_64", "/usr/bin/true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Installs Rosetta 2, accepting Apple's license.
///
/// Only call this after the user has asked for it: running it is how they
/// agree to the license. Blocks until done (usually under a minute).
/// Tries without privileges first, then asks macOS for an admin password.
pub fn install() -> Result<()> {
    if !is_needed() {
        return Err(Error::Unsupported(
            "Rosetta 2 is only needed on Apple Silicon Macs.".into(),
        ));
    }
    if is_installed() {
        return Ok(());
    }

    let direct = Command::new(SOFTWAREUPDATE)
        .args(INSTALL_ARGS)
        .stdin(Stdio::null())
        .output();
    if direct.as_ref().is_ok_and(|o| o.status.success()) && is_installed() {
        return Ok(());
    }

    let elevated = Command::new("/usr/bin/osascript")
        .args(["-e", ADMIN_SCRIPT])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::RosettaInstall(e.to_string()))?;
    if elevated.status.success() && is_installed() {
        return Ok(());
    }
    let message = output_text(&elevated);
    if message.contains(USER_CANCELED) {
        return Err(Error::RosettaInstall(
            "the administrator password prompt was cancelled".into(),
        ));
    }
    let message = if message.is_empty() {
        direct.map(|o| output_text(&o)).unwrap_or_default()
    } else {
        message
    };
    Err(Error::RosettaInstall(if message.is_empty() {
        "softwareupdate did not install Rosetta 2".into()
    } else {
        message
    }))
}

fn output_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_apple_silicon_needs_rosetta() {
        assert!(needed_on("macos", "aarch64"));
        assert!(!needed_on("macos", "x86_64"));
        assert!(!needed_on("linux", "x86_64"));
    }

    #[test]
    fn install_refuses_where_not_needed() {
        if !is_needed() {
            assert!(matches!(install(), Err(Error::Unsupported(_))));
        }
    }
}
