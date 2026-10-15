//! Native launchers for pinned programs (plan section 4.7).
//!
//! - macOS: a tiny `.app` in `~/Applications/Vinter/` that opens
//!   `vinter://run/<bottle>/<program>`, so Spotlight, Launchpad and the Dock
//!   can start the program. Vinter handles the link, starting in the
//!   background if it is not running.
//! - Linux: a `.desktop` file in `~/.local/share/applications/` that runs
//!   Vinter with `--run <bottle> <program>`.
//!
//! Launchers point at a program's index, so after programs are removed or
//! renamed, call [`sync_bottle`] to rebuild a bottle's launchers.

pub mod linux;
pub mod macos;

use std::io::Cursor;
use std::path::PathBuf;

use crate::bottle::{self, Bottle, ICONS_DIR};
use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Error, Result};

pub const SCHEME: &str = "vinter";
const FALLBACK_ICON: &[u8] = include_bytes!("../../assets/vinter-128.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    MacApp,
    LinuxDesktop,
}

/// Where and how launchers are written.
#[derive(Debug, Clone)]
pub struct Target {
    pub kind: Kind,
    /// `~/Applications/Vinter` or `~/.local/share/applications`.
    pub dir: PathBuf,
    /// Linux only: where launcher icons are saved.
    pub icons_dir: PathBuf,
    /// Linux only: the command (program and leading arguments) that gets
    /// `--run <bottle> <index>` appended, e.g. the Vinter app binary.
    pub command: Vec<String>,
}

impl Target {
    /// The usual locations on this OS. `command` is used on Linux only.
    pub fn for_this_os(command: Vec<String>) -> Result<Self> {
        let home = directories::BaseDirs::new().ok_or(Error::NoHomeDir)?;
        if cfg!(target_os = "macos") {
            Ok(Self {
                kind: Kind::MacApp,
                dir: home.home_dir().join("Applications").join("Vinter"),
                icons_dir: PathBuf::new(),
                command,
            })
        } else {
            Ok(Self {
                kind: Kind::LinuxDesktop,
                dir: home.data_dir().join("applications"),
                icons_dir: home.data_dir().join("icons").join("vinter"),
                command,
            })
        }
    }
}

/// `vinter://run/<bottle>/<index>`
pub fn deep_link(bottle_id: &str, index: usize) -> String {
    format!("{SCHEME}://run/{bottle_id}/{index}")
}

/// Parses `vinter://run/<bottle>/<index>` (also with a trailing slash).
pub fn parse_deep_link(url: &str) -> Option<(String, usize)> {
    let rest = url.strip_prefix(&format!("{SCHEME}://run/"))?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (id, index) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || validate_id(id).is_err() {
        return None;
    }
    Some((id.to_string(), index.parse().ok()?))
}

/// Pins or unpins a program, then rebuilds the bottle's launchers.
pub fn set_pinned(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    pinned: bool,
    target: &Target,
) -> Result<Bottle> {
    let mut bottle = bottle::get(paths, bottle_id)?;
    let program = bottle
        .programs
        .get_mut(index)
        .ok_or(Error::ProgramNotFound(index))?;
    program.pinned = pinned;
    bottle.save(&bottle::dir(paths, bottle_id)?)?;
    sync_bottle(paths, bottle_id, target)?;
    Ok(bottle)
}

/// Removes a bottle's launchers and creates one per pinned program.
/// Returns the launchers created.
pub fn sync_bottle(paths: &AppPaths, bottle_id: &str, target: &Target) -> Result<Vec<PathBuf>> {
    remove_bottle(bottle_id, target)?;
    let bottle = bottle::get(paths, bottle_id)?;
    let mut created = Vec::new();
    for (index, program) in bottle.programs.iter().enumerate().filter(|(_, p)| p.pinned) {
        let icon = icon_png(paths, &bottle, program.icon.as_deref())?;
        let path = match target.kind {
            Kind::MacApp => {
                let name = unique_app_name(target, &program.name, &bottle.name);
                macos::create(&target.dir, &name, &bottle, index, &icon)?
            }
            Kind::LinuxDesktop => linux::create(target, &bottle, index, &icon)?,
        };
        created.push(path);
    }
    Ok(created)
}

/// Removes every launcher that belongs to a bottle.
pub fn remove_bottle(bottle_id: &str, target: &Target) -> Result<()> {
    let found = match target.kind {
        Kind::MacApp => macos::find_for_bottle(&target.dir, bottle_id),
        Kind::LinuxDesktop => linux::find_for_bottle(target, bottle_id),
    };
    for path in found {
        if path.is_dir() {
            std::fs::remove_dir_all(&path).at(&path)?;
        } else {
            std::fs::remove_file(&path).at(&path)?;
        }
    }
    if target.kind == Kind::LinuxDesktop {
        linux::remove_icons(target, bottle_id)?;
    }
    Ok(())
}

/// The program's icon as a 128x128 PNG (Vinter's icon if it has none).
fn icon_png(paths: &AppPaths, bottle: &Bottle, icon: Option<&str>) -> Result<Vec<u8>> {
    let source = icon
        .and_then(|i| i.strip_prefix(&format!("{ICONS_DIR}/")))
        .filter(|name| validate_id(name).is_ok())
        .and_then(|name| {
            std::fs::read(
                bottle::dir(paths, &bottle.id)
                    .ok()?
                    .join(ICONS_DIR)
                    .join(name),
            )
            .ok()
        })
        .unwrap_or_else(|| FALLBACK_ICON.to_vec());
    let image = image::load_from_memory(&source)
        .or_else(|_| image::load_from_memory(FALLBACK_ICON))
        .map_err(|e| Error::Unsupported(format!("unreadable icon: {e}")))?;
    let image = image.resize_exact(128, 128, image::imageops::FilterType::Lanczos3);
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| Error::Unsupported(format!("could not encode icon: {e}")))?;
    Ok(png)
}

/// "<Program>", or "<Program> (<Bottle>)" if another bottle's launcher has that name.
fn unique_app_name(target: &Target, program: &str, bottle: &str) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if matches!(c, '/' | ':' | '\\') {
                    '-'
                } else {
                    c
                }
            })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let plain = clean(program);
    if !target.dir.join(format!("{plain}.app")).exists() {
        plain
    } else {
        format!("{plain} ({})", clean(bottle))
    }
}

/// FNV-1a, for stable launcher identifiers.
pub(crate) fn short_hash(s: &str) -> String {
    let h = s.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("{h:016x}")
}

#[cfg(test)]
mod tests;
