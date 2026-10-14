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
    }
}
