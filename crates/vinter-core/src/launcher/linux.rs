//! Linux launchers: `.desktop` files (freedesktop.org Desktop Entry spec).

use std::path::PathBuf;

use super::Target;
use crate::Result;
use crate::bottle::Bottle;
use crate::error::IoResultExt;

fn file_name(bottle_id: &str, program_id: &str) -> String {
    format!("vinter-{bottle_id}-{program_id}")
}

/// Writes `vinter-<bottle>-<program id>.desktop` and its icon.
pub fn create(target: &Target, bottle: &Bottle, index: usize, icon_png: &[u8]) -> Result<PathBuf> {
    let program = &bottle.programs[index];
    std::fs::create_dir_all(&target.dir).at(&target.dir)?;
    std::fs::create_dir_all(&target.icons_dir).at(&target.icons_dir)?;
    let base = file_name(&bottle.id, &program.id);
    let icon = target.icons_dir.join(format!("{base}.png"));
    std::fs::write(&icon, icon_png).at(&icon)?;

    let mut exec: Vec<String> = target.command.clone();
    exec.extend(["--run".to_string(), bottle.id.clone(), program.id.clone()]);
    let mut exec = exec.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let mime = mime_types(&program.file_types);
    if !mime.is_empty() {
        // The file manager adds the opened files here.
        exec.push_str(" %F");
    }
    let mut entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Windows program in the Vinter bottle {bottle}\nExec={exec}\nIcon={icon}\nTerminal=false\nCategories=Wine;\nStartupNotify=true\nX-Vinter-Bottle={id}\n",
        name = single_line(&program.name),
        bottle = single_line(&bottle.name),
        icon = icon.display(),
        id = bottle.id,
    );
    if !mime.is_empty() {
        entry.push_str(&format!("MimeType={};\n", mime.join(";")));
    }
    let path = target.dir.join(format!("{base}.desktop"));
    std::fs::write(&path, entry).at(&path)?;
    Ok(path)
}

pub fn find_for_bottle(target: &Target, bottle_id: &str) -> Vec<PathBuf> {
    let prefix = format!("vinter-{bottle_id}-");
    let Ok(entries) = std::fs::read_dir(&target.dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".desktop"))
        })
        .collect()
}

/// Made before program ids: named `vinter-<bottle>-<index>.desktop` (ids
/// are never all digits).
pub fn is_legacy(path: &std::path::Path, bottle_id: &str) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix(&format!("vinter-{bottle_id}-")))
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

pub fn remove_icons(target: &Target, bottle_id: &str) -> Result<()> {
    let prefix = format!("vinter-{bottle_id}-");
    if let Ok(entries) = std::fs::read_dir(&target.icons_dir) {
        for path in entries.flatten().map(|e| e.path()) {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
            {
                std::fs::remove_file(&path).at(&path)?;
            }
        }
    }
    Ok(())
}

/// Quotes an `Exec` argument as the Desktop Entry spec requires.
fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '+' | ',' | '=')
        });
    if plain {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        if c == '%' {
            out.push('%');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn single_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Common document types and their MIME types (shared-mime-info names).
const MIME_TYPES: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("log", "text/x-log"),
    ("csv", "text/csv"),
    ("ini", "text/x-dosini"),
    ("xml", "application/xml"),
    ("json", "application/json"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("rtf", "application/rtf"),
    ("pdf", "application/pdf"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("bmp", "image/bmp"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("svg", "image/svg+xml"),
    ("psd", "image/vnd.adobe.photoshop"),
    ("dwg", "image/vnd.dwg"),
    ("dxf", "image/vnd.dxf"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/x-wav"),
    ("flac", "audio/flac"),
    ("mp4", "video/mp4"),
    ("avi", "video/x-msvideo"),
    ("mkv", "video/x-matroska"),
    ("zip", "application/zip"),
    ("7z", "application/x-7z-compressed"),
    ("rar", "application/vnd.rar"),
];

/// MIME types for file extensions: from [`MIME_TYPES`], otherwise the
/// `application/x-extension-<ext>` type file managers use for unknown ones.
pub fn mime_types(extensions: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ext in extensions {
        let mime = MIME_TYPES
            .iter()
            .find(|(e, _)| e == ext)
            .map(|(_, m)| m.to_string())
            .unwrap_or_else(|| format!("application/x-extension-{ext}"));
        if !out.contains(&mime) {
            out.push(mime);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{mime_types, quote};

    #[test]
    fn mime_types_for_extensions() {
        let exts = ["docx", "jpg", "jpeg", "xyz"].map(String::from);
        assert_eq!(
            mime_types(&exts),
            [
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "image/jpeg",
                "application/x-extension-xyz",
            ]
        );
    }

    #[test]
    fn exec_quoting() {
        assert_eq!(quote("/usr/bin/vinter"), "/usr/bin/vinter");
        assert_eq!(quote("/opt/My Apps/vinter"), "\"/opt/My Apps/vinter\"");
        assert_eq!(quote("a\"b$c"), "\"a\\\"b\\$c\"");
        assert_eq!(quote("100%"), "\"100%%\"");
    }
}
