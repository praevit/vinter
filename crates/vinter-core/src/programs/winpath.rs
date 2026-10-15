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

#[cfg(test)]
mod tests {
    use super::*;

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
