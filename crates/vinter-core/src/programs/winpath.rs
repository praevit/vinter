//! Mapping between Windows paths (`C:\Program Files\App\app.exe`) and the
//! files inside a Wine prefix.

use std::path::{Component, Path, PathBuf};

/// Resolves a Windows path to a file in the prefix. Only `C:` is mapped
/// (other drive letters point outside the bottle). Matching is
/// case-insensitive like Windows, preferring an exact match.
pub fn to_unix(prefix: &Path, windows: &str) -> Option<PathBuf> {
    let windows = windows.trim().trim_matches('"');
    let rest = windows
        .strip_prefix("C:\\")
        .or_else(|| windows.strip_prefix("c:\\"))?;
    let mut path = prefix.join("drive_c");
    for part in rest.split('\\').filter(|p| !p.is_empty()) {
        if part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        // Look the name up in the listing (not `exists()`, which ignores case
        // on macOS) so the result always has the on-disk spelling.
        let names: Vec<String> = std::fs::read_dir(&path)
            .ok()?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        let name = names
            .iter()
            .find(|n| n.as_str() == part)
            .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(part)))?;
        path = path.join(name);
    }
    Some(path)
}

/// Like [`to_unix`], for a file that may not exist yet: existing folders are
/// matched case-insensitively, the rest is used as written.
pub fn to_unix_for_write(prefix: &Path, windows: &str) -> Option<PathBuf> {
    let windows = windows.trim().trim_matches('"');
    let rest = windows
        .strip_prefix("C:\\")
        .or_else(|| windows.strip_prefix("c:\\"))?;
    let parts: Vec<&str> = rest
        .split('\\')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    let mut path = prefix.join("drive_c");
    let mut existing = true;
    for part in parts {
        if existing {
            if let Some(found) = to_unix(
                prefix,
                &to_windows(prefix, &path.join(part)).unwrap_or_default(),
            ) {
                path = found;
                continue;
            }
            existing = false;
        }
        path.push(part);
    }
    Some(path)
}

/// The Windows path of a file inside the prefix's `drive_c`.
pub fn to_windows(prefix: &Path, unix: &Path) -> Option<String> {
    let rel = unix.strip_prefix(prefix.join("drive_c")).ok()?;
    let parts: Option<Vec<&str>> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    Some(format!("C:\\{}", parts?.join("\\")))
}

/// The Windows path of any file on this computer, through the bottle's drive
/// letters (`dosdevices/`): `C:` for the bottle's own files, a folder mapped
/// as its own drive, or `Z:` (the whole computer). The most specific drive
/// wins. `None` if no drive shows the file, e.g. with `Z:` hidden.
pub fn host_to_windows(prefix: &Path, host: &Path) -> Option<String> {
    if let Some(win) = to_windows(prefix, host) {
        return Some(win);
    }
    let host = host.canonicalize().ok()?;
    let dosdevices = prefix.join("dosdevices");
    let mut best: Option<(usize, char, PathBuf)> = None;
    for entry in std::fs::read_dir(&dosdevices).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(letter) = drive_letter(&name) else {
            continue;
        };
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let Ok(target) = dosdevices.join(target).canonicalize() else {
            continue;
        };
        if let Ok(rest) = host.strip_prefix(&target) {
            let depth = target.components().count();
            if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
                best = Some((depth, letter, rest.to_path_buf()));
            }
        }
    }
    let (_, letter, rest) = best?;
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    Some(format!(
        "{}:\\{}",
        letter.to_ascii_uppercase(),
        parts?.join("\\")
    ))
}

/// A host file as an argument for a Wine program: its Windows path, or with
/// no drive for it, Wine's `\\?\unix\` form. Prefixes Wine has not set up
/// yet (no `dosdevices/`) get Wine's default `Z:`.
pub fn host_arg(prefix: &Path, host: &Path) -> String {
    if let Some(win) = host_to_windows(prefix, host) {
        return win;
    }
    let unix = host.display().to_string().replace('/', "\\");
    if prefix.join("dosdevices").is_dir() {
        format!(r"\\?\unix{unix}")
    } else {
        format!("Z:{unix}")
    }
}

/// The host path of a Windows path on any of the bottle's drives (`C:` is
/// matched like [`to_unix`]; other drives through their `dosdevices/` link).
pub fn windows_to_host(prefix: &Path, windows: &str) -> Option<PathBuf> {
    let windows = windows.trim().trim_matches('"');
    let letter = drive_letter(windows.get(..2)?)?;
    if letter == 'c' {
        return to_unix(prefix, windows);
    }
    let rest = windows.get(2..)?.trim_start_matches(['\\', '/']);
    let dosdevices = prefix.join("dosdevices");
    let target = std::fs::read_link(dosdevices.join(format!("{letter}:"))).ok()?;
    let mut path = dosdevices.join(target);
    for part in rest
        .split(['\\', '/'])
        .filter(|p| !p.is_empty() && *p != ".")
    {
        if part == ".." {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

/// `d:` or `D:` -> `d`.
fn drive_letter(name: &str) -> Option<char> {
    let mut chars = name.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(c), Some(':'), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn host_files_through_drive_letters() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        let docs = tmp.path().join("Docs");
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        std::fs::create_dir_all(docs.join("Sub")).unwrap();
        let file = docs.join("Sub/report.docx");
        std::fs::write(&file, b"").unwrap();
        let link = |target: &Path, name: &str| {
            std::os::unix::fs::symlink(target, prefix.join("dosdevices").join(name)).unwrap()
        };
        link(Path::new("../drive_c"), "c:");
        link(Path::new("/"), "z:");

        // Only Z: shows it.
        let canonical = file.canonicalize().unwrap();
        let expected = format!("Z:{}", canonical.display().to_string().replace('/', "\\"));
        assert_eq!(host_to_windows(&prefix, &file).unwrap(), expected);

        // A folder drive is more specific than Z:.
        link(&docs, "d:");
        assert_eq!(
            host_to_windows(&prefix, &file).unwrap(),
            r"D:\Sub\report.docx"
        );
        assert_eq!(
            windows_to_host(&prefix, r"d:\Sub\report.docx").unwrap(),
            prefix
                .join("dosdevices")
                .join(&docs)
                .join("Sub/report.docx")
        );
        assert!(windows_to_host(&prefix, r"D:\..\x").is_none());

        // Without Z:, files outside the drives have no Windows path.
        std::fs::remove_file(prefix.join("dosdevices/z:")).unwrap();
        let outside = tmp.path().join("other.txt");
        std::fs::write(&outside, b"").unwrap();
        assert!(host_to_windows(&prefix, &outside).is_none());
        assert!(host_arg(&prefix, &outside).starts_with(r"\\?\unix\"));
        // Before Wine set up the prefix: its default Z:.
        assert!(host_arg(&tmp.path().join("new"), &outside).starts_with(r"Z:\"));
    }

    #[test]
    fn round_trips_and_ignores_case() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path();
        let exe = prefix.join("drive_c/Program Files/7-Zip/7zFM.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();

        let win = to_windows(prefix, &exe).unwrap();
        assert_eq!(win, r"C:\Program Files\7-Zip\7zFM.exe");
        assert_eq!(to_unix(prefix, &win).unwrap(), exe);
        assert_eq!(
            to_unix(prefix, r"c:\PROGRAM FILES\7-zip\7ZFM.EXE").unwrap(),
            exe
        );
        assert_eq!(
            to_unix(prefix, r#""C:\Program Files\7-Zip\7zFM.exe""#).unwrap(),
            exe
        );
        assert!(to_unix(prefix, r"C:\Program Files\Missing\x.exe").is_none());
        assert!(to_unix(prefix, r"D:\x.exe").is_none());
        assert!(to_unix(prefix, r"C:\Program Files\..\..\etc").is_none());
        assert!(to_windows(prefix, Path::new("/etc/passwd")).is_none());

        // For writing: existing folders keep their case, new parts are added.
        assert_eq!(
            to_unix_for_write(prefix, r"C:\program files\7-zip\new\x.ini").unwrap(),
            prefix.join("drive_c/Program Files/7-Zip/new/x.ini")
        );
        assert!(to_unix_for_write(prefix, r"C:\a\..\..\etc").is_none());
        assert!(to_unix_for_write(prefix, r"D:\x").is_none());
    }
}
