//! Installing the `vinter` command that ships inside the app, so a terminal
//! (or an AI agent through `vinter mcp`) can use it: a link in a folder on
//! the user's PATH, pointing at the app's copy.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::IoResultExt;
use crate::{Error, Result};

/// Name of the installed command.
pub const COMMAND: &str = "vinter";
/// Written into the stand-in a development build bundles when the real
/// command was not built (see the app's `build.rs`).
pub const PLACEHOLDER_MARKER: &str = "vinter-cli-placeholder";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CliStatus {
    /// The command inside the app, if this build has one.
    pub bundled: Option<String>,
    /// Where `vinter` is installed, if it is (and points at this app).
    pub installed_at: Option<String>,
    /// A different `vinter` is in the way at the install location.
    pub conflict: Option<String>,
    /// Folder an install puts the link in.
    pub install_dir: String,
    /// Whether that folder is on the PATH of a new terminal.
    pub on_path: bool,
    /// JSON for an AI agent's MCP settings, starting `vinter mcp` (through
    /// the installed link, else the app's copy).
    pub mcp_config: Option<String>,
}

/// Whether `path` is the stand-in rather than the real command.
pub fn is_placeholder(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| {
        bytes.len() < 4096 && String::from_utf8_lossy(&bytes).contains(PLACEHOLDER_MARKER)
    })
}

/// The folder to install into: `/usr/local/bin` when it exists and can be
/// written without a password, else `~/.local/bin`.
pub fn install_dir(home: &Path) -> PathBuf {
    let system = Path::new("/usr/local/bin");
    if system.is_dir() && is_writable(system) {
        system.to_path_buf()
    } else {
        home.join(".local").join("bin")
    }
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".vinter-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where a `vinter` linked to (or a copy of) `bundled` already is, among
/// `dirs`.
pub fn find_installed(bundled: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(COMMAND))
        .find(|p| points_at(p, bundled))
}

/// True if `link` is a link to `bundled`, or an identical copy of it.
fn points_at(link: &Path, bundled: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(link) {
        let target = if target.is_absolute() {
            target
        } else {
            link.parent().unwrap_or(Path::new("/")).join(target)
        };
        return same_file(&target, bundled);
    }
    match (std::fs::read(link), std::fs::read(bundled)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Puts `vinter` in `dir`, linked to `bundled`; with `copy`, a copy instead
/// (for an AppImage, whose files move on every start). Replaces an earlier
/// link, never someone else's program.
pub fn install(bundled: &Path, dir: &Path, copy: bool) -> Result<PathBuf> {
    if !bundled.is_file() || is_placeholder(bundled) {
        return Err(Error::Unsupported(
            "this build of Vinter does not include the vinter command".into(),
        ));
    }
    std::fs::create_dir_all(dir).at(dir)?;
    let dest = dir.join(COMMAND);
    if let Ok(meta) = std::fs::symlink_metadata(&dest) {
        let ours = meta.file_type().is_symlink() || (copy && points_at(&dest, bundled));
        if !ours && !(copy && is_old_copy(&dest)) {
            return Err(Error::InvalidSetting(format!(
                "there is already a vinter command at {}; remove it first",
                dest.display()
            )));
        }
        std::fs::remove_file(&dest).at(&dest)?;
    }
    if copy {
        let tmp = dir.join(format!(".{COMMAND}.tmp"));
        std::fs::copy(bundled, &tmp).at(&tmp)?;
        std::fs::rename(&tmp, &dest).at(&dest)?;
    } else {
        #[cfg(unix)]
        std::os::unix::fs::symlink(bundled, &dest).at(&dest)?;
        #[cfg(not(unix))]
        return Err(Error::Unsupported("links need macOS or Linux".into()));
    }
    Ok(dest)
}

/// Text in the command's help, used to recognise an earlier copy of it.
const CLI_ABOUT: &[u8] = b"Run Windows apps on macOS and Linux";

/// An earlier copy of Vinter's command (an AppImage install from an older
/// version), recognised by its help text.
fn is_old_copy(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes.windows(CLI_ABOUT.len()).any(|w| w == CLI_ABOUT))
}

/// Whether `dir` is in a `PATH` value.
pub fn path_contains(path_var: &str, dir: &Path) -> bool {
    std::env::split_paths(path_var).any(|p| p == dir || same_file(&p, dir))
}

/// The `PATH` a new terminal gets: apps started from the Dock or a desktop
/// launcher have a shorter one, so ask the user's login shell (briefly).
pub fn terminal_path() -> String {
    let fallback = std::env::var("PATH").unwrap_or_default();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let Ok(mut child) = std::process::Command::new(shell)
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return fallback;
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return fallback;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if out.trim().is_empty() {
        fallback
    } else {
        out.trim().to_string()
    }
}

/// Everything Settings shows about the command. `bundled` is the app's
/// copy (or `None` without one); `copy` is true for an AppImage.
pub fn status(bundled: Option<&Path>, home: &Path, path_var: &str) -> CliStatus {
    let bundled = bundled.filter(|b| b.is_file() && !is_placeholder(b));
    let dir = install_dir(home);
    let dirs = [
        PathBuf::from("/usr/local/bin"),
        home.join(".local").join("bin"),
    ];
    let installed = bundled.and_then(|b| find_installed(b, &dirs));
    let conflict = match &installed {
        Some(_) => None,
        None => {
            let dest = dir.join(COMMAND);
            std::fs::symlink_metadata(&dest)
                .is_ok_and(|m| !m.file_type().is_symlink() && !is_old_copy(&dest))
                .then(|| dest.display().to_string())
        }
    };
    let on_path = path_contains(
        path_var,
        installed.as_deref().and_then(Path::parent).unwrap_or(&dir),
    );
    CliStatus {
        mcp_config: installed
            .as_deref()
            .or(bundled)
            .map(|p| mcp_config(&p.display().to_string())),
        bundled: bundled.map(|p| p.display().to_string()),
        installed_at: installed.map(|p| p.display().to_string()),
        conflict,
        install_dir: dir.display().to_string(),
        on_path,
    }
}

/// The JSON an MCP client (an AI agent's settings) needs to start
/// `vinter mcp`.
pub fn mcp_config(command: &str) -> String {
    let config = serde_json::json!({
        "mcpServers": { "vinter": { "command": command, "args": ["mcp"] } }
    });
    serde_json::to_string_pretty(&config).expect("json serializes")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn installs_a_link_and_finds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("Vinter.app/Contents/MacOS");
        std::fs::create_dir_all(&app).unwrap();
        let bundled = app.join("vinter-cli");
        std::fs::write(&bundled, b"\x7fELF real program").unwrap();
        let bin = tmp.path().join("home/.local/bin");

        let dest = install(&bundled, &bin, false).unwrap();
        assert_eq!(dest, bin.join("vinter"));
        assert_eq!(std::fs::read_link(&dest).unwrap(), bundled);
        assert_eq!(
            find_installed(&bundled, std::slice::from_ref(&bin)),
            Some(dest.clone())
        );
        // Installing again replaces the link.
        install(&bundled, &bin, false).unwrap();

        // Someone else's vinter is left alone.
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(&dest, b"another program").unwrap();
        assert!(install(&bundled, &bin, false).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"another program");
        assert_eq!(find_installed(&bundled, std::slice::from_ref(&bin)), None);
    }

    #[test]
    fn copies_for_an_appimage_and_refuses_the_placeholder() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("vinter-cli");
        std::fs::write(&bundled, b"Run Windows apps on macOS and Linux v2").unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            bin.join("vinter"),
            b"Run Windows apps on macOS and Linux v1",
        )
        .unwrap();
        let dest = install(&bundled, &bin, true).unwrap();
        assert!(std::fs::symlink_metadata(&dest).unwrap().is_file());
        assert_eq!(
            find_installed(&bundled, std::slice::from_ref(&bin)),
            Some(dest)
        );

        let stand_in = tmp.path().join("stand-in");
        std::fs::write(
            &stand_in,
            format!("#!/bin/sh\n# {PLACEHOLDER_MARKER}\nexit 1\n"),
        )
        .unwrap();
        assert!(is_placeholder(&stand_in));
        assert!(install(&stand_in, &bin, false).is_err());
        let s = status(Some(&stand_in), tmp.path(), "");
        assert_eq!((s.bundled, s.mcp_config), (None, None));
    }

    #[test]
    fn path_and_mcp_config() {
        assert!(path_contains(
            "/usr/bin:/home/me/.local/bin",
            Path::new("/home/me/.local/bin")
        ));
        assert!(!path_contains("/usr/bin", Path::new("/home/me/.local/bin")));
        let v: serde_json::Value = serde_json::from_str(&mcp_config("/x/vinter")).unwrap();
        assert_eq!(v["mcpServers"]["vinter"]["command"], "/x/vinter");
        assert_eq!(v["mcpServers"]["vinter"]["args"][0], "mcp");
    }
}
