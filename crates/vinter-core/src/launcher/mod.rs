//! Native launchers for pinned programs (plan section 4.7).
//!
//! - macOS: a tiny `.app` in `~/Applications/Vinter/` that opens
//!   `vinter://run/<bottle>/<program>`, so Spotlight, Launchpad and the Dock
//!   can start the program. Vinter handles the link, starting in the
//!   background if it is not running.
//! - Linux: a `.desktop` file in `~/.local/share/applications/` that runs
//!   Vinter with `--run <bottle> <program> [files…]`.
//!
//! Launchers point at a program's stable id. They show its name, icon and
//! file types, so after those change, call [`sync_bottle`] to rebuild a
//! bottle's launchers. Programs with file types get launchers that open
//! documents: Finder's "Open With" on macOS, `MimeType=` on Linux.

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
    /// `--run <bottle> <program id>` appended, e.g. the Vinter app binary.
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

/// `vinter://run/<bottle>/<program id>`
pub fn deep_link(bottle_id: &str, program_id: &str) -> String {
    format!("{SCHEME}://run/{bottle_id}/{program_id}")
}

/// A program in a link: its stable id, or (in links made before ids
/// existed) its position in the bottle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramRef {
    Id(String),
    Index(usize),
}

impl ProgramRef {
    /// Digits are an old-style index; anything else must be a valid id
    /// (program ids are never all digits).
    pub fn parse(s: &str) -> Option<Self> {
        if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
            return s.parse().ok().map(Self::Index);
        }
        validate_id(s).ok()?;
        Some(Self::Id(s.to_string()))
    }

    /// The program's index in `bottle`, if it is still there.
    pub fn resolve(&self, bottle: &Bottle) -> Option<usize> {
        match self {
            Self::Id(id) => bottle.program_index(id),
            Self::Index(i) => (*i < bottle.programs.len()).then_some(*i),
        }
    }
}

/// What a `vinter://` link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// `vinter://run/<bottle>/<program>`, optionally with files to open:
    /// `?file=<percent-encoded path>&file=…`.
    Run {
        bottle: String,
        program: ProgramRef,
        files: Vec<PathBuf>,
    },
    /// `vinter://stop/<bottle>/<program>`: stops it if Vinter started it.
    Stop { bottle: String, program: ProgramRef },
    /// `vinter://open-bottle/<bottle>`: shows the bottle in Vinter.
    OpenBottle { bottle: String },
}

/// Parses a `vinter://` link (a trailing slash is fine). `None` for links
/// Vinter does not understand.
pub fn parse_deep_link(url: &str) -> Option<DeepLink> {
    let rest = url.strip_prefix(&format!("{SCHEME}://"))?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let bottle = |s: &str| validate_id(s).ok().map(|()| s.to_string());
    match parts.as_slice() {
        ["run", b, p] => Some(DeepLink::Run {
            bottle: bottle(b)?,
            program: ProgramRef::parse(p)?,
            files: query_files(query)?,
        }),
        ["stop", b, p] if query.is_empty() => Some(DeepLink::Stop {
            bottle: bottle(b)?,
            program: ProgramRef::parse(p)?,
        }),
        ["open-bottle", b] if query.is_empty() => Some(DeepLink::OpenBottle { bottle: bottle(b)? }),
        _ => None,
    }
}

/// `file=` values of a query, decoded. Other keys are ignored; files must
/// be absolute paths.
fn query_files(query: &str) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let Some(value) = pair.strip_prefix("file=") else {
            continue;
        };
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        let path = PathBuf::from(decoded.as_ref());
        if !path.is_absolute() || decoded.contains('\0') {
            return None;
        }
        files.push(path);
    }
    Some(files)
}

/// `vinter://run/<bottle>/<program>?file=…` for opening files with a program.
pub fn deep_link_with_files(bottle_id: &str, program_id: &str, files: &[PathBuf]) -> String {
    const KEEP: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~')
        .remove(b'/');
    let mut url = deep_link(bottle_id, program_id);
    for (i, file) in files.iter().enumerate() {
        url.push(if i == 0 { '?' } else { '&' });
        url.push_str("file=");
        url.extend(percent_encoding::utf8_percent_encode(
            &file.display().to_string(),
            KEEP,
        ));
    }
    url
}

/// Normalizes file types typed by the user ("DOCX, .txt  md") to lowercase
/// extensions without dots, each once. Anything that is not a plain
/// extension is dropped.
pub fn normalize_file_types(types: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in types.iter().flat_map(|t| t.split([',', ' ', ';'])) {
        let ext = raw.trim().trim_start_matches("*.").trim_start_matches('.');
        let ext = ext.to_ascii_lowercase();
        let ok = !ext.is_empty()
            && ext.len() <= 32
            && ext
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'));
        if ok && !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
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

/// Rebuilds the launchers of bottles that still have ones made before
/// program ids (they point at indexes). Returns the bottles rebuilt.
pub fn upgrade_legacy(paths: &AppPaths, target: &Target) -> Result<Vec<String>> {
    let mut upgraded = Vec::new();
    for summary in bottle::list(paths)? {
        let legacy = match target.kind {
            Kind::MacApp => macos::find_for_bottle(&target.dir, &summary.id)
                .iter()
                .any(|app| macos::is_legacy(app)),
            Kind::LinuxDesktop => linux::find_for_bottle(target, &summary.id)
                .iter()
                .any(|file| linux::is_legacy(file, &summary.id)),
        };
        if legacy {
            sync_bottle(paths, &summary.id, target)?;
            upgraded.push(summary.id);
        }
    }
    Ok(upgraded)
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
